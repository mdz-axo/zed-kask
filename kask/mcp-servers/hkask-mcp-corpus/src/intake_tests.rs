//! Wire-level pins for the document-intake family (convert, is_complex, ocr,
//! chunk) and the reduction family (dedup, consolidate): every call is a
//! `tools/call` JSON request through the production transport, and every
//! assertion reads what a consumer receives.
use std::sync::Arc;

use serde_json::json;

use crate::containment_tests::{Wire, content, inside, refusal};

fn server() -> crate::CorpusServer {
    crate::helpers::corpus_test_server(Arc::new(crate::helpers::NoInference(
        "intake pins must not invoke inference",
    )))
}

/// A one-page PDF whose only content is a filled rectangle: no text layer,
/// so triage classifies the page `no-text` and extraction yields no words.
fn image_only_pdf(path: &std::path::Path) {
    let content = "0.2 g 72 72 400 600 re f\n";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>".to_string(),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
    ];
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{body}\nendobj\n", index + 1));
    }
    let xref = pdf.len();
    pdf.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    std::fs::write(path, pdf).expect("image-only pdf");
}

/// expect: "A validation refusal reads as one kind and one sentence, not a
/// JSON document inside the message; a per-file refusal in directory mode
/// reads the same way."
/// pre: an entity_ref_prefix with a space (chunk), a directory holding one
/// source the file arm refuses (an empty file).
/// post: `structuredContent.kind` is the kind and the text is the sentence;
/// the directory report's failure carries `kind` and `error` as fields.
#[tokio::test]
async fn validation_refusals_are_not_double_encoded() {
    let inside = inside();
    let within = |name: &str| inside.path().join(name).to_string_lossy().into_owned();
    std::fs::create_dir(inside.path().join("docs")).expect("docs");
    std::fs::write(within("docs/empty.txt"), b"").expect("empty source");
    std::fs::write(
        within("docs/full.txt"),
        "A ledger names its keeper. ".repeat(80),
    )
    .expect("source");

    let mut wire = Wire::start(server()).await;
    let result = wire
        .call(
            "corpus_chunk",
            json!({"text":"one two","entity_ref_prefix":"bad prefix","index":false}),
        )
        .await;
    let (kind, message) = refusal("corpus_chunk", &result);
    assert_eq!(kind, "invalid_argument");
    assert!(
        message.contains("entity_ref_prefix contains invalid characters") && !message.contains('{'),
        "{message}"
    );

    let result = wire
        .call(
            "corpus_convert",
            json!({"path":within("docs"),"output":within("out")}),
        )
        .await;
    let report = content("corpus_convert", &result);
    assert_eq!(report["extracted"], 1, "{report}");
    assert_eq!(report["failed"], 1, "{report}");
    let failure = &report["failures"][0];
    assert_eq!(failure["kind"], "invalid_argument", "{failure}");
    assert!(
        failure["error"]
            .as_str()
            .is_some_and(|error| error.contains("is empty")),
        "{failure}"
    );
}

/// expect: "corpus_ocr refuses a document that has no OCR path before it
/// asks the registry for a model, and the refusal names the accepted
/// kinds."
/// pre: a text file; a server whose port refuses every inference call.
/// post: invalid_argument naming PDFs and the decodable image extensions;
/// an absent image path is still a containment refusal.
#[tokio::test]
async fn ocr_refuses_a_non_image_before_model_resolution() {
    let inside = inside();
    let within = |name: &str| inside.path().join(name).to_string_lossy().into_owned();
    std::fs::write(within("notes.txt"), b"plain text").expect("text");

    let mut wire = Wire::start(server()).await;
    let result = wire
        .call("corpus_ocr", json!({"path":within("notes.txt")}))
        .await;
    let (kind, message) = refusal("corpus_ocr", &result);
    assert_eq!(kind, "invalid_argument", "{message}");
    assert!(
        message.contains("supports PDFs and decodable images") && message.contains("png"),
        "{message}"
    );

    let result = wire
        .call("corpus_ocr", json!({"path":within("absent.png")}))
        .await;
    let (kind, message) = refusal("corpus_ocr", &result);
    assert_eq!(kind, "invalid_argument", "{message}");
    assert!(message.contains("Cannot resolve path"), "{message}");
}

/// expect: "A clustering threshold outside [0, 1] is refused before the
/// database is opened, by both reduction tools."
/// pre: a db_path that does not exist; thresholds 1.5, -0.1 and NaN-free
/// out-of-range values. post: invalid_argument naming the range; the
/// database path is never reached (a missing database would be not_found).
#[tokio::test]
async fn reduction_tools_refuse_an_out_of_range_threshold_first() {
    let inside = inside();
    let within = |name: &str| inside.path().join(name).to_string_lossy().into_owned();
    std::fs::write(within("tagged.jsonl"), "{}\n").expect("tagged");
    let mut wire = Wire::start(server()).await;
    for tool in ["corpus_dedup_chunks", "corpus_consolidate_chunks"] {
        for threshold in [1.5, -0.1] {
            let result = wire
                .call(
                    tool,
                    json!({"tagged_jsonl":within("tagged.jsonl"),"output":within("out.jsonl"),
                        "db_path":within("absent.db"),"threshold":threshold}),
                )
                .await;
            let (kind, message) = refusal(tool, &result);
            assert_eq!(kind, "invalid_argument", "{tool} {threshold}: {message}");
            assert!(
                message.contains("cosine similarity in [0, 1]"),
                "{tool} {threshold}: {message}"
            );
        }
    }
}

/// expect: "A single source that yields no words is an error naming the
/// usual cause, never a success with zero passages; a multi-tier result
/// carries index_error like the single-tier one."
/// pre: an image-only PDF (no OCR model), whitespace text, and a short text
/// chunked multi-tier without indexing.
/// post: failed_precondition naming the OCR setting for the first two;
/// `index_error: null` present in the multi-tier result.
#[tokio::test]
async fn chunk_reports_an_empty_source_and_keeps_one_result_shape() {
    let inside = inside();
    let within = |name: &str| inside.path().join(name).to_string_lossy().into_owned();
    image_only_pdf(&inside.path().join("scan.pdf"));

    let mut wire = Wire::start(server()).await;
    for arguments in [
        json!({"path":within("scan.pdf"),"entity_ref_prefix":"pin","index":false}),
        json!({"text":"   \n ","entity_ref_prefix":"pin","index":false}),
    ] {
        let result = wire.call("corpus_chunk", arguments.clone()).await;
        let (kind, message) = refusal("corpus_chunk", &result);
        assert_eq!(kind, "failed_precondition", "{arguments}: {message}");
        assert!(
            message.contains("yielded no words") && message.contains("kask.models.ocr_model"),
            "{arguments}: {message}"
        );
    }

    let result = wire
        .call(
            "corpus_chunk",
            json!({"text":"one two three four","entity_ref_prefix":"pin","index":false,"multi_tier":true}),
        )
        .await;
    let summary = content("corpus_chunk", &result);
    assert_eq!(summary["multi_tier"], true, "{summary}");
    assert_eq!(summary["indexed"], 0, "{summary}");
    assert!(
        summary
            .get("index_error")
            .is_some_and(serde_json::Value::is_null),
        "index_error must be present and null: {summary}"
    );
}
