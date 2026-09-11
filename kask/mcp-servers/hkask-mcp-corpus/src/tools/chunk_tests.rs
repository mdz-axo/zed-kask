//! Approved slice2 contracts at the public corpus_chunk boundary; no inference or production state.
use super::ChunkRequest;
use crate::CorpusServer;
use hkask_types::template::LLMParameters;
use hkask_types::{ChatToolDefinition, InferenceError, InferencePort, InferenceResult};
use rmcp::handler::server::wrapper::Parameters;
use serde_json::{Value, json};
use std::{future::Future, pin::Pin, sync::Arc};

struct NoInference;
impl InferencePort for NoInference {
    fn generate(
        &self,
        _: &str,
        _: &LLMParameters,
        _: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        panic!("chunk fixtures must not invoke inference")
    }
}

fn server() -> CorpusServer {
    let port: Arc<dyn InferencePort> = Arc::new(NoInference);
    let ocr = Arc::new(crate::ocr::llm_ocr::LlmOcrExecutor::new(Arc::clone(&port)));
    CorpusServer::new(
        hkask_types::WebID::new(),
        None,
        port,
        Default::default(),
        ocr,
    )
}

fn fixture() -> anyhow::Result<tempfile::TempDir> {
    Ok(tempfile::tempdir_in(std::env::current_dir()?)?)
}

fn request(patch: Value) -> anyhow::Result<ChunkRequest> {
    let mut value = json!({"text":"one two three four five six seven eight nine ten",
        "entity_ref_prefix":"chunk-fixture", "index":false});
    for (key, item) in patch.as_object().expect("request object") {
        value[key] = item.clone();
    }
    Ok(serde_json::from_value(value)?)
}

async fn run(server: &CorpusServer, patch: Value) -> anyhow::Result<Value> {
    let result = server.corpus_chunk(Parameters(request(patch)?)).await?;
    Ok(hkask_types::tool_response::unwrap_tool_envelope(
        serde_json::from_str(&result)?,
    ))
}

/// expect: [P4] Invalid effective budgets fail before reading, writing, or choosing any chunk mode.
#[tokio::test]
async fn rejects_nonprogress_budgets_before_branches() -> anyhow::Result<()> {
    let server = server();
    for mode in [
        json!({}),
        json!({"text":null,"path":"missing.txt"}),
        json!({"text":null,"input_dir":"missing-dir","output":"missing-output.jsonl"}),
        json!({"multi_tier":true}),
    ] {
        for field in [
            "max_tokens",
            "coarse_max_tokens",
            "medium_max_tokens",
            "fine_max_tokens",
        ] {
            for bad in [0, 1] {
                let mut patch = mode.clone();
                patch[field] = json!(bad);
                let error = server
                    .corpus_chunk(Parameters(request(patch)?))
                    .await
                    .expect_err("bad budget");
                assert!(
                    error.to_string().contains("tokens"),
                    "{field}={bad}: {error}"
                );
            }
        }
        for overlap in [1, 16, 17, usize::MAX] {
            let mut patch = mode.clone();
            patch["max_tokens"] = json!(16);
            patch["overlap_tokens"] = json!(overlap);
            let error = server
                .corpus_chunk(Parameters(request(patch)?))
                .await
                .expect_err("bad overlap");
            assert!(error.to_string().contains("overlap"), "{error}");
        }
    }
    Ok(())
}

fn reconstruct(passages: &[Value], overlap: usize, max_words: usize) -> String {
    let mut all: Vec<String> = Vec::new();
    for (index, row) in passages.iter().enumerate() {
        let words: Vec<_> = row["text"]
            .as_str()
            .expect("passage text")
            .split_whitespace()
            .collect();
        assert!(words.len() <= max_words, "word budget exceeded");
        let repeated = if index == 0 { 0 } else { overlap };
        assert!(
            words.len() > repeated,
            "every passage contributes new source words"
        );
        if repeated > 0 {
            assert_eq!(&all[all.len() - repeated..], &words[..repeated]);
        }
        all.extend(words[repeated..].iter().map(|word| (*word).to_owned()));
    }
    all.join(" ")
}

/// expect: [P3] Repeated context is real, all Unicode/escaped source words reconstruct in order, and IDs are unique.
#[tokio::test]
async fn explicit_overlap_reconstructs_with_word_bounds() -> anyhow::Result<()> {
    let server = server();
    let text = (0..120)
        .map(|n| format!("λ{n} \"quoted{n}\" path\\{n} sentence{n}."))
        .collect::<Vec<_>>()
        .join("\n\n");
    for max_tokens in [8, 16, 32] {
        for overlap_tokens in [2, 4, max_tokens - 1] {
            let result = run(
                &server,
                json!({"text":text, "max_tokens":max_tokens,
                "overlap_tokens":overlap_tokens}),
            )
            .await?;
            let passages = result["passages"].as_array().expect("passages");
            let words = crate::tokens_to_words(max_tokens);
            let overlap = crate::tokens_to_words(overlap_tokens);
            assert_eq!(
                reconstruct(passages, overlap, words),
                text.split_whitespace().collect::<Vec<_>>().join(" ")
            );
            let refs: std::collections::HashSet<_> = passages
                .iter()
                .map(|row| row["entity_ref"].as_str().expect("ref"))
                .collect();
            assert_eq!(refs.len(), passages.len());
        }
    }
    let minimal = run(&server, json!({"max_tokens":2,"overlap_tokens":0})).await?;
    assert_eq!(minimal["total_passages"], 10);
    Ok(())
}

/// expect: [P3] Explicit overlap applies to each requested tier rather than being silently ignored.
#[tokio::test]
async fn overlap_applies_to_all_tiers() -> anyhow::Result<()> {
    let server = server();
    let text = (0..100)
        .map(|n| format!("word{n}"))
        .collect::<Vec<_>>()
        .join(" ");
    let result = run(
        &server,
        json!({"text":text,"multi_tier":true,"overlap_tokens":4,
        "coarse_max_tokens":32,"medium_max_tokens":16,"fine_max_tokens":8}),
    )
    .await?;
    for (tier, tokens) in [("coarse", 32), ("medium", 16), ("fine", 8)] {
        assert_eq!(
            reconstruct(
                result[tier].as_array().expect("tier"),
                3,
                crate::tokens_to_words(tokens)
            ),
            text
        );
    }
    Ok(())
}

/// expect: [P3] Colliding legacy filenames get distinct stable refs; JSONL preserves original source names and text.
#[tokio::test]
async fn directory_source_ids_are_injective_and_stable() -> anyhow::Result<()> {
    let dir = fixture()?;
    let server = server();
    let names = ["a.b.txt", "a_b.txt", "a%2eb.txt", "λ\".txt"];
    for name in names {
        std::fs::write(
            dir.path().join(name),
            "alpha βeta \"quote\" back\\slash one two three four five six",
        )?;
    }
    let output = dir.path().join("chunks.jsonl");
    let patch = json!({"text":null,"input_dir":dir.path(),"output":output,"max_tokens":8,"overlap_tokens":2});
    let first = run(&server, patch.clone()).await?;
    let original = std::fs::read_to_string(&output)?;
    run(&server, patch).await?;
    assert_eq!(std::fs::read_to_string(&output)?, original);
    let rows: Vec<Value> = original
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    let refs: std::collections::HashSet<_> = rows
        .iter()
        .map(|row| row["entity_ref"].as_str().expect("ref"))
        .collect();
    assert_eq!(refs.len(), rows.len());
    assert_eq!(first["total_documents"], names.len());
    for name in names {
        let passages: Vec<_> = rows
            .iter()
            .filter(|row| row["source"] == name)
            .cloned()
            .collect();
        assert!(!passages.is_empty());
        assert_eq!(
            reconstruct(&passages, 1, 6),
            std::fs::read_to_string(dir.path().join(name))?
        );
    }
    Ok(())
}

/// expect: [P4] Every discovered txt symlink is contained before reading; broken links and invalid UTF-8 are visible errors.
#[cfg(unix)]
#[tokio::test]
async fn directory_rejects_escaping_and_broken_children() -> anyhow::Result<()> {
    let dir = fixture()?;
    let outside = tempfile::tempdir()?;
    let server = server();
    let secret = outside.path().join("secret.txt");
    std::fs::write(&secret, "outside fixture must never be consumed")?;
    let link = dir.path().join("escape.txt");
    std::os::unix::fs::symlink(&secret, &link)?;
    let output = dir.path().join("chunks.jsonl");
    std::fs::write(&output, "existing output")?;
    let patch = json!({"text":null,"input_dir":dir.path(),"output":output});
    let error = run(&server, patch.clone())
        .await
        .expect_err("escape rejected");
    assert!(error.to_string().contains("outside"), "{error}");
    assert_eq!(std::fs::read_to_string(&output)?, "existing output");
    std::fs::remove_file(&secret)?;
    assert!(
        run(&server, patch.clone()).await.is_err(),
        "broken txt link must not be silently skipped"
    );
    std::fs::remove_file(&link)?;
    std::fs::write(dir.path().join("invalid.txt"), [0xff, 0xfe])?;
    assert!(
        run(&server, patch).await.is_err(),
        "invalid UTF-8 must be surfaced"
    );
    Ok(())
}

/// expect: [P3] File-path mode preserves original source provenance and never aliases punctuation variants, in any tier mode.
#[tokio::test]
async fn file_sources_keep_provenance_and_distinct_refs() -> anyhow::Result<()> {
    let dir = fixture()?;
    let server = server();
    let text = "one two three four five six seven eight nine ten eleven twelve";
    for name in ["a.b.txt", "a_b.txt"] {
        std::fs::write(dir.path().join(name), text)?;
    }
    for multi_tier in [false, true] {
        let mut refs = std::collections::HashSet::new();
        for name in ["a.b.txt", "a_b.txt"] {
            let source = dir.path().join(name);
            let patch = json!({"text":null,"path":source,"multi_tier":multi_tier,"max_tokens":8,
                "overlap_tokens":2,"coarse_max_tokens":8,"medium_max_tokens":8,"fine_max_tokens":8});
            let first = run(&server, patch.clone()).await?;
            assert_eq!(first["source"], source.to_str().expect("UTF-8 fixture"));
            assert_eq!(first, run(&server, patch).await?);
            for field in if multi_tier {
                &["coarse", "medium", "fine"][..]
            } else {
                &["passages"][..]
            } {
                let rows = first[field].as_array().expect("passages");
                assert_eq!(reconstruct(rows, 1, 6), text);
                for row in rows {
                    assert!(refs.insert(row["entity_ref"].as_str().expect("ref").to_owned()));
                }
            }
        }
    }
    Ok(())
}

/// expect: [P3] Structured documents retain heading-only sections as source content, with or without requested overlap.
#[tokio::test]
async fn structured_source_retains_heading_only_sections() -> anyhow::Result<()> {
    let dir = fixture()?;
    let path = dir.path().join("headings.docx");
    let paragraph =
        |text: &str| docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(text));
    docx_rs::Docx::new()
        .add_paragraph(paragraph("First").style("Heading1"))
        .add_paragraph(paragraph("Second").style("Heading1"))
        .add_paragraph(paragraph(
            "one two three four five six seven eight nine ten",
        ))
        .add_paragraph(paragraph("Last").style("Heading1"))
        .build()
        .pack(std::fs::File::create(&path)?)?;
    for overlap in [None, Some(2)] {
        let result = run(
            &server(),
            json!({"text":null,"path":path,"max_tokens":16,"overlap_tokens":overlap}),
        )
        .await?;
        let reconstructed = reconstruct(
            result["passages"].as_array().expect("passages"),
            overlap.map_or(0, crate::tokens_to_words),
            12,
        );
        // DocStructure::text renders heading markers for the flat overlapping
        // path; the legacy section path uses the heading text without markers.
        let expected = if overlap.is_some() {
            "# First # Second one two three four five six seven eight nine ten # Last"
        } else {
            "First Second one two three four five six seven eight nine ten Last"
        };
        assert_eq!(reconstructed, expected);
    }
    Ok(())
}

/// expect: [P3] Omitted overlap retains canonical non-overlap defaults, and result metadata reports the actual settings-derived budget.
#[tokio::test]
async fn omitted_overlap_preserves_default_chunking() -> anyhow::Result<()> {
    let text = (0..1000)
        .map(|n| format!("word{n}"))
        .collect::<Vec<_>>()
        .join(" ");
    let result = run(&server(), json!({"text":text})).await?;
    let tokens = crate::HkaskSettings::load().chunk_max_tokens();
    let max = crate::tokens_to_words(tokens);
    let min = crate::tokens_to_words(64).max(max / 4);
    let expected = hkask_memory::chunk_text(&text, "chunk-fixture", min, max, ".!? ");
    assert_eq!(
        result["passages"],
        json!(crate::serialize_passages(&expected))
    );
    assert_eq!(result["max_tokens"], tokens);
    assert_eq!(result["overlap_tokens"], 0);
    Ok(())
}

/// expect: [P4] Invalid input cannot replace existing output or leave temporary artifacts behind.
#[tokio::test]
async fn directory_failure_preserves_output_and_cleans_temporary_files() -> anyhow::Result<()> {
    let dir = fixture()?;
    std::fs::write(dir.path().join("valid.txt"), "a valid source")?;
    std::fs::write(dir.path().join("invalid.txt"), [0xff])?;
    let output = dir.path().join("chunks.jsonl");
    std::fs::write(&output, "original output")?;
    let before: std::collections::HashSet<_> = std::fs::read_dir(dir.path())?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()?;
    let patch = json!({"text":null,"input_dir":dir.path(),"output":output});
    assert!(run(&server(), patch).await.is_err());
    let after: std::collections::HashSet<_> = std::fs::read_dir(dir.path())?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()?;
    assert_eq!(before, after);
    assert_eq!(std::fs::read_to_string(output)?, "original output");
    Ok(())
}

/// expect: [P3] The existing extracted-book directory path accepts text above 32 MiB without a new generic cap.
#[tokio::test]
async fn directory_preserves_large_book_policy() -> anyhow::Result<()> {
    let dir = fixture()?;
    let text = format!("{} end", "λ".repeat(17 * 1024 * 1024));
    std::fs::write(dir.path().join("large.txt"), &text)?;
    let output = dir.path().join("chunks.jsonl");
    let result = run(
        &server(),
        json!({"text":null,"input_dir":dir.path(),"output":output,"overlap_tokens":0}),
    )
    .await?;
    assert_eq!(result["total_documents"], 1);
    assert_eq!(result["total_chunks"], 1);
    let row: Value = serde_json::from_str(&std::fs::read_to_string(output)?)?;
    assert_eq!(row["text"], text);
    Ok(())
}
