//! Document admission contracts at the public corpus_convert boundary.
use super::ConvertRequest;
use crate::CorpusServer;
use hkask_types::template::LLMParameters;
use hkask_types::{ChatToolDefinition, InferenceError, InferencePort, InferenceResult};
use rmcp::handler::server::wrapper::Parameters;
use std::io::{Seek, Write};
use std::{future::Future, path::Path, pin::Pin, sync::Arc};

struct NoInference;
impl InferencePort for NoInference {
    fn generate(
        &self,
        _: &str,
        _: &LLMParameters,
        _: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        panic!("native document fixtures must not invoke inference")
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
    let root = std::env::current_dir()?.join("target/document-test");
    std::fs::create_dir_all(&root)?;
    Ok(tempfile::tempdir_in(root)?)
}

fn request(path: &Path, output: &Path) -> ConvertRequest {
    ConvertRequest {
        path: path.to_string_lossy().into_owned(),
        output: Some(output.to_string_lossy().into_owned()),
        force_ocr: false,
        target_pages: None,
        include_structure: None,
    }
}

// An unreferenced sparse stream makes the PDF large without a large test
// allocation. Real Poppler still reads the xref and the native page content.
fn large_native_pdf(path: &Path) -> anyhow::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(b"%PDF-1.4\n")?;
    let mut offsets = Vec::new();
    let mut text =
        String::from("1 0 0 rg 40 400 100 100 re f\n0 0 0 rg\nBT /F1 12 Tf 40 760 Td 16 TL\n");
    for line in 0..12 {
        text.push_str(&format!("(Source line {line} explains how evidence supports careful decisions about systems and their observed behavior.) Tj T*\n"));
    }
    text.push_str("ET\n");
    for (id, body) in [
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_string()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string()),
        (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_string()),
        (4, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string()),
        (5, format!("<< /Length {} >>\nstream\n{text}endstream", text.len())),
    ] {
        offsets.push(file.stream_position()?);
        writeln!(file, "{id} 0 obj\n{body}\nendobj")?;
    }
    offsets.push(file.stream_position()?);
    let padding = crate::path_safety::MAX_READ_BYTES + 1;
    writeln!(file, "6 0 obj\n<< /Length {padding} >>\nstream")?;
    let end = file.stream_position()? + padding;
    file.set_len(end)?;
    file.seek(std::io::SeekFrom::Start(end))?;
    file.write_all(b"\nendstream\nendobj\n")?;
    let xref = file.stream_position()?;
    file.write_all(b"xref\n0 7\n0000000000 65535 f \n")?;
    for offset in offsets {
        writeln!(file, "{offset:010} 00000 n ")?;
    }
    writeln!(
        file,
        "trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF"
    )?;
    Ok(())
}

/// expect: [P7] Every approved book can use normal conversion, even when PDF bytes exceed the text-input cap.
/// pre: a contained native-text PDF larger than MAX_READ_BYTES, with Poppler installed.
/// post: directory conversion writes its extracted text without inference or scope loss.
/// inv: the input document remains unchanged.
#[tokio::test]
async fn large_pdf_converts_through_directory_without_ocr() -> anyhow::Result<()> {
    let dir = fixture()?;
    let sources = dir.path().join("sources");
    let output = dir.path().join("extracted");
    std::fs::create_dir(&sources)?;
    let pdf = sources.join("large.pdf");
    large_native_pdf(&pdf)?;
    let original_size = std::fs::metadata(&pdf)?.len();
    assert!(original_size > crate::path_safety::MAX_READ_BYTES);
    let result = server()
        .corpus_convert(Parameters(request(&sources, &output)))
        .await?;
    let result = hkask_types::tool_response::unwrap_tool_envelope(serde_json::from_str(&result)?);
    assert_eq!(result["failed"], 0, "{result}");
    assert_eq!(result["source_documents"], 1);
    assert_eq!(result["extracted"], 1);
    let text = std::fs::read_to_string(output.join("large.pdf.txt"))?;
    assert!(text.contains("Source line 0 explains"));
    assert!(text.contains("Source line 11 explains"));
    assert_eq!(std::fs::metadata(&pdf)?.len(), original_size);
    Ok(())
}

const PAGE_HEADER: &str = "---\nprimary_language: en\nis_rotation_valid: true\nrotation_correction: 0\nis_table: false\nis_diagram: false\n---\n";

struct VisionPort {
    calls: std::sync::atomic::AtomicUsize,
    response: String,
}
impl InferencePort for VisionPort {
    fn generate(
        &self,
        _: &str,
        _: &LLMParameters,
        _: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        panic!("OCR must use the vision route")
    }

    fn list_models(
        &self,
    ) -> Pin<
        Box<dyn Future<Output = Result<Vec<hkask_types::ModelEntry>, InferenceError>> + Send + '_>,
    > {
        Box::pin(async {
            Ok(vec![hkask_types::ModelEntry {
                prefixed_name: "fixture/ocr".into(),
                model: "ocr".into(),
                supports_vision: true,
            }])
        })
    }

    fn generate_vision(
        &self,
        _: &str,
        _: &[String],
        _: &LLMParameters,
        model: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        assert_eq!(model, Some("fixture/ocr"));
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async {
            Ok(InferenceResult {
                text: self.response.clone(),
                model: "fixture/ocr".into(),
                usage: hkask_types::InferenceUsage {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
                finish_reason: "stop".into(),
                tool_calls: vec![],
                reasoning: None,
                cost_usd: None,
            })
        })
    }
}

/// expect: [P7] OCR preserves transcribed text and reports model figure annotations separately, never as created files or source quotations.
#[tokio::test]
async fn ocr_protocol_separates_page_text_and_annotations() -> anyhow::Result<()> {
    let dir = fixture()?;
    let input = dir.path().join("control.png");
    image::DynamicImage::new_rgb8(8, 8).save(&input)?;
    let body = "The pump runs for seventeen minutes.\n\n![Pump diagram](page_0_0_100_100.png)\n\nThe gauge reads five bars.";
    let port: Arc<dyn InferencePort> = Arc::new(VisionPort {
        calls: 0.into(),
        response: format!("{PAGE_HEADER}{body}"),
    });
    let ocr = Arc::new(crate::ocr::llm_ocr::LlmOcrExecutor::new(port.clone()));
    let server = CorpusServer::new(
        hkask_types::WebID::new(),
        Some("fixture/ocr".into()),
        port,
        Default::default(),
        ocr,
    );
    let mut request = request(&input, &dir.path().join("text.txt"));
    request.force_ocr = true;
    let result = server.corpus_convert(Parameters(request)).await?;
    let result = hkask_types::tool_response::unwrap_tool_envelope(serde_json::from_str(&result)?);
    let text = result["text"].as_str().expect("text");
    assert_eq!(
        text.split_whitespace().collect::<Vec<_>>().join(" "),
        "The pump runs for seventeen minutes. The gauge reads five bars."
    );
    assert_eq!(
        result["page_reports"][0]["metadata"]["primary_language"],
        "en"
    );
    assert_eq!(
        result["page_reports"][0]["figure_annotations"][0]["description"],
        "Pump diagram"
    );
    assert_eq!(
        result["page_reports"][0]["figure_annotations_provenance"],
        "model_inference"
    );
    assert_eq!(result["page_reports"][0]["figure_artifacts_created"], false);
    let second = server
        .corpus_ocr(Parameters(super::OcrRequest {
            path: input.to_string_lossy().into_owned(),
            model: None,
        }))
        .await?;
    let second = hkask_types::tool_response::unwrap_tool_envelope(serde_json::from_str(&second)?);
    assert_eq!(second["text"], result["text"]);
    assert_eq!(second["page_reports"], result["page_reports"]);
    Ok(())
}

/// expect: [P4] A rejected page response cannot create a purported extraction file.
#[tokio::test]
async fn ocr_protocol_rejects_bad_page_before_output() -> anyhow::Result<()> {
    let dir = fixture()?;
    let input = dir.path().join("control.png");
    image::DynamicImage::new_rgb8(8, 8).save(&input)?;
    for (index, response) in [
        "legacy plain text".to_string(),
        format!("{PAGE_HEADER}![Bad](https://example.com/fake.png)"),
    ]
    .into_iter()
    .enumerate()
    {
        let port: Arc<dyn InferencePort> = Arc::new(VisionPort {
            calls: 0.into(),
            response,
        });
        let ocr = Arc::new(crate::ocr::llm_ocr::LlmOcrExecutor::new(port.clone()));
        let server = CorpusServer::new(
            hkask_types::WebID::new(),
            Some("fixture/ocr".into()),
            port,
            Default::default(),
            ocr,
        );
        let output = dir.path().join(format!("rejected-{index}.txt"));
        let mut request = request(&input, &output);
        request.force_ocr = true;
        let error = server
            .corpus_convert(Parameters(request))
            .await
            .expect_err("invalid page response");
        assert!(error.to_string().contains("page protocol"), "{error}");
        assert!(!output.exists());
    }
    Ok(())
}

/// expect: [P4] Page rendering preserves color and treats its bound as pixels, not DPI.
#[tokio::test]
async fn ocr_page_rendering_preserves_color_with_pixel_bound() -> anyhow::Result<()> {
    let dir = fixture()?;
    let input = dir.path().join("control.pdf");
    large_native_pdf(&input)?;
    for images in [
        crate::ocr::decimation::pdf_to_images(&input, 80)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?,
        crate::ocr::decimation::pdf_to_images_for_pages(&input, 80, &[0])
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?,
    ] {
        let image = images.first().expect("rendered page");
        assert_eq!(image.width().max(image.height()), 80);
        assert!(image.to_rgb8().pixels().any(|pixel| {
            let [r, g, b] = pixel.0;
            r > g && r > b
        }));
    }
    Ok(())
}

/// expect: [P7] A real OCR execution persists its report and resumes without paying for OCR again.
#[tokio::test]
async fn staged_ocr_report_round_trips_without_reinference() -> anyhow::Result<()> {
    let dir = fixture()?;
    let sources = dir.path().join("sources");
    let output = dir.path().join("extracted");
    std::fs::create_dir(&sources)?;
    large_native_pdf(&sources.join("book.pdf"))?;
    let port = Arc::new(VisionPort {
        calls: 0.into(),
        response: format!(
            "{PAGE_HEADER}{}",
            (0..80)
                .map(|n| format!("term {n}"))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    });
    let inference: Arc<dyn InferencePort> = port.clone();
    let ocr = Arc::new(crate::ocr::llm_ocr::LlmOcrExecutor::new(inference.clone()));
    let server = CorpusServer::new(
        hkask_types::WebID::new(),
        Some("fixture/ocr".into()),
        inference,
        Default::default(),
        ocr,
    );
    let mut first = request(&sources, &output);
    first.force_ocr = true;
    let first = server.corpus_convert(Parameters(first)).await?;
    let first = hkask_types::tool_response::unwrap_tool_envelope(serde_json::from_str(&first)?);
    assert_eq!(first["staged"], 1, "{first}");
    assert_eq!(first["verification_failed"], 0);
    assert_eq!(first["document_reports"][0]["verification_passed"], true);
    let calls = port.calls.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(calls, 1);
    let second = server
        .corpus_convert(Parameters(request(&sources, &output)))
        .await?;
    let second = hkask_types::tool_response::unwrap_tool_envelope(serde_json::from_str(&second)?);
    assert_eq!(second["skipped_staged"], 1, "{second}");
    assert_eq!(second["document_reports"], first["document_reports"]);
    assert_eq!(port.calls.load(std::sync::atomic::Ordering::SeqCst), calls);
    let report_path = dir
        .path()
        .join("extracted-ocr-staging/book.pdf.report.json");
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(&report_path)?)?;
    for field in ["text", "path", "verification_passed", "ocr_protocol"] {
        let mut mismatched = report.clone();
        mismatched[field] = serde_json::Value::Null;
        std::fs::write(&report_path, serde_json::to_vec(&mismatched)?)?;
        let rejected = server
            .corpus_convert(Parameters(request(&sources, &output)))
            .await?;
        let rejected =
            hkask_types::tool_response::unwrap_tool_envelope(serde_json::from_str(&rejected)?);
        assert_eq!(rejected["failed"], 1, "{field}: {rejected}");
        assert_eq!(rejected["skipped_staged"], 0);
        assert_eq!(port.calls.load(std::sync::atomic::Ordering::SeqCst), calls);
    }
    assert!(!output.join("book.pdf.txt").exists());
    Ok(())
}

/// expect: [P4] A staged OCR extraction without its page verification cannot be resumed as success.
#[tokio::test]
async fn staged_ocr_without_report_stays_unverified() -> anyhow::Result<()> {
    let dir = fixture()?;
    let sources = dir.path().join("sources");
    let output = dir.path().join("extracted");
    let staging = dir.path().join("extracted-ocr-staging");
    std::fs::create_dir(&sources)?;
    std::fs::create_dir(&staging)?;
    let text = (0..80)
        .map(|n| format!("term {n}"))
        .collect::<Vec<_>>()
        .join(" ");
    std::fs::write(sources.join("book.txt"), &text)?;
    std::fs::write(staging.join("book.txt.txt"), &text)?;
    let result = server()
        .corpus_convert(Parameters(request(&sources, &output)))
        .await?;
    let result = hkask_types::tool_response::unwrap_tool_envelope(serde_json::from_str(&result)?);
    assert_eq!(result["failed"], 1, "{result}");
    assert_eq!(result["skipped_staged"], 0);
    assert!(
        result["failures"]
            .to_string()
            .contains("verification report")
    );
    assert!(!output.join("book.txt.txt").exists());
    assert_eq!(std::fs::read_to_string(staging.join("book.txt.txt"))?, text);
    Ok(())
}

/// expect: [P4] Resuming staged text must surface its failed per-page verification, not hide it behind whole-file quality.
#[tokio::test]
async fn staged_ocr_resume_returns_failed_report() -> anyhow::Result<()> {
    let dir = fixture()?;
    let sources = dir.path().join("sources");
    let output = dir.path().join("extracted");
    let staging = dir.path().join("extracted-ocr-staging");
    std::fs::create_dir(&sources)?;
    std::fs::create_dir(&staging)?;
    let text = (0..80)
        .map(|n| format!("term {n}"))
        .collect::<Vec<_>>()
        .join(" ");
    let source = sources.join("book.txt");
    std::fs::write(&source, &text)?;
    std::fs::write(staging.join("book.txt.txt"), &text)?;
    let report = serde_json::json!({"path":source,"method":"selective_ocr", "verification_passed":false,
        "ocr_protocol":crate::ocr::response::OCR_PROTOCOL,
        "page_count_match":true,"quality_failed_pages":[7],"error_count":0});
    let mut stored = report.clone();
    stored["text"] = serde_json::json!(text);
    std::fs::write(
        staging.join("book.txt.report.json"),
        serde_json::to_vec(&stored)?,
    )?;
    let result = server()
        .corpus_convert(Parameters(request(&sources, &output)))
        .await?;
    let result = hkask_types::tool_response::unwrap_tool_envelope(serde_json::from_str(&result)?);
    assert_eq!(result["skipped_staged"], 1, "{result}");
    assert_eq!(result["verification_failed"], 1, "{result}");
    assert_eq!(result["document_reports"][0], report);
    assert!(!output.join("book.txt.txt").exists());
    Ok(())
}

/// expect: [P4] Large-document support must not remove the raw-text input safety cap.
#[tokio::test]
async fn oversized_text_still_rejected_before_output() -> anyhow::Result<()> {
    let dir = fixture()?;
    let input = dir.path().join("large.txt");
    let output = dir.path().join("output.txt");
    std::fs::File::create(&input)?.set_len(crate::path_safety::MAX_READ_BYTES + 1)?;
    let error = server()
        .corpus_convert(Parameters(request(&input, &output)))
        .await
        .expect_err("text cap");
    assert!(error.to_string().contains("read cap"), "{error}");
    assert!(!output.exists());
    Ok(())
}

/// expect: [P4] PDF path-based extraction must still reject out-of-root documents before creating output.
#[cfg(unix)]
#[tokio::test]
async fn pdf_symlink_escape_rejected_before_output() -> anyhow::Result<()> {
    let dir = fixture()?;
    let outside = tempfile::tempdir()?;
    let pdf = outside.path().join("outside.pdf");
    large_native_pdf(&pdf)?;
    let input = dir.path().join("escaped.pdf");
    std::os::unix::fs::symlink(pdf, &input)?;
    let output = dir.path().join("output.txt");
    let error = server()
        .corpus_convert(Parameters(request(&input, &output)))
        .await
        .expect_err("containment");
    assert!(error.to_string().contains("escapes"), "{error}");
    assert!(!output.exists());
    Ok(())
}
