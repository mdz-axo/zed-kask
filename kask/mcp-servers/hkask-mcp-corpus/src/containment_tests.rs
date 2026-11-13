//! Wire-level pins for path containment and honest degradation: every call
//! goes through the production rmcp transport as a `tools/call` JSON request,
//! and every assertion reads the JSON-RPC result a consumer receives.
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatToolDefinition, EmbeddingBatch, EmbeddingGenerationError, InferenceError, InferencePort,
    InferenceResult,
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::CorpusServer;

/// Counts every model call. Embedding refuses any batch holding the word
/// `refuse` (or every batch when `refuse_all`); generation always refuses.
#[derive(Default)]
struct CountingPort {
    calls: AtomicUsize,
    refuse_all: bool,
}

impl InferencePort for CountingPort {
    fn generate(
        &self,
        _: &str,
        _: &LLMParameters,
        _: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Err(InferenceError::Connection("counting port".into())) })
    }

    fn embed_with_dimensions<'a>(
        &'a self,
        model: &str,
        texts: &[String],
        _: Option<u32>,
    ) -> hkask_types::EmbedWithIdentityFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let refuse = self.refuse_all || texts.iter().any(|text| text.contains("refuse"));
        let requested_model = model.to_string();
        let count = texts.len();
        Box::pin(async move {
            if refuse {
                return Err(EmbeddingGenerationError::NotConfigured(
                    "PROBE_API_KEY not set".into(),
                ));
            }
            Ok(EmbeddingBatch {
                vectors: vec![vec![1.0; crate::embedding_dim()]; count],
                requested_model,
                actual_model: None,
                usage: hkask_types::InferenceUsage::default(),
            })
        })
    }
}

/// One served corpus server behind the production stdio-shaped transport.
pub(crate) struct Wire {
    read: BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
    write: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    next_id: u64,
}

impl Wire {
    pub(crate) async fn start(server: CorpusServer) -> Self {
        use rmcp::ServiceExt;
        let (server_io, client_io) = tokio::io::duplex(1 << 20);
        let (read, write) = tokio::io::split(server_io);
        let transport =
            rmcp::transport::async_rw::AsyncRwTransport::<rmcp::RoleServer, _, _>::new(read, write);
        tokio::spawn(async move {
            if let Ok(service) = server.serve(transport).await {
                let _ = service.waiting().await;
            }
        });
        let (read, write) = tokio::io::split(client_io);
        let mut wire = Self {
            read: BufReader::new(read),
            write,
            next_id: 1,
        };
        wire.send(
            json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
            "protocolVersion":"2024-11-05","capabilities":{},
            "clientInfo":{"name":"containment-pin","version":"1"}}}),
        )
        .await;
        wire.response(0).await;
        wire.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await;
        wire
    }

    async fn send(&mut self, value: Value) {
        self.write
            .write_all(format!("{value}\n").as_bytes())
            .await
            .expect("wire write");
        self.write.flush().await.expect("wire flush");
    }

    async fn response(&mut self, id: u64) -> Value {
        loop {
            let mut line = String::new();
            let read = tokio::time::timeout(
                std::time::Duration::from_secs(60),
                self.read.read_line(&mut line),
            )
            .await
            .expect("response within bound")
            .expect("wire read");
            assert!(read > 0, "server closed the wire before answering {id}");
            let message: Value = serde_json::from_str(&line).expect("json-rpc line");
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                return message;
            }
        }
    }

    /// The `result` object of one `tools/call`.
    pub(crate) async fn call(&mut self, name: &str, arguments: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":name,"arguments":arguments}}))
            .await;
        let message = self.response(id).await;
        message
            .get("result")
            .cloned()
            .unwrap_or_else(|| panic!("{name} answered without a result: {message}"))
    }
}

/// `(kind, message)` of a refused call, as the consumer reads them.
pub(crate) fn refusal(tool: &str, result: &Value) -> (String, String) {
    assert_eq!(result["isError"], true, "{tool} must refuse: {result}");
    (
        result["structuredContent"]["kind"]
            .as_str()
            .unwrap_or_else(|| panic!("{tool} refusal carries no kind: {result}"))
            .to_string(),
        result["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    )
}

/// The `content` object of a successful call.
pub(crate) fn content(tool: &str, result: &Value) -> Value {
    assert_eq!(result["isError"], false, "{tool} must succeed: {result}");
    let envelope: Value = serde_json::from_str(
        result["content"][0]["text"]
            .as_str()
            .expect("envelope text"),
    )
    .expect("envelope json");
    envelope["content"].clone()
}

/// A fixture directory inside an allowed root (the crate's ignored target dir).
pub(crate) fn inside() -> tempfile::TempDir {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/containment-test");
    std::fs::create_dir_all(&root).expect("fixture root");
    tempfile::tempdir_in(root).expect("inside fixture")
}

fn listing(directory: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .expect("list")
        .map(|entry| entry.expect("entry").file_name().to_string_lossy().into())
        .collect();
    names.sort();
    names
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(bytes))
}

fn chunk_rows() -> String {
    [
        json!({"entity_ref":"pin:doc:0","source":"doc.txt","text":"The keeper writes the hour.","word_count":5}),
        json!({"entity_ref":"pin:doc:1","source":"doc.txt","text":"The provider will refuse this one.","word_count":6}),
    ]
    .map(|row| row.to_string())
    .join("\n")
}

/// expect: "No corpus tool reads, opens or creates anything at a path outside
/// the allowed roots, and no model call is spent on a refused request."
/// pre: every path argument that reached the filesystem unchecked before this
/// pin (twelve db_path arguments, the calibration sources and directories, the
/// grounding bundle directory) names a location outside every root.
/// post: each call is invalid_argument naming the allowed root; the outside
/// directory holds exactly what it held; the model port was never called.
#[tokio::test]
async fn an_escaping_path_argument_is_refused_and_leaves_nothing_outside() {
    let inside = inside();
    let outside = tempfile::tempdir().expect("outside fixture");
    let within = |name: &str| inside.path().join(name).to_string_lossy().into_owned();
    let beyond = |name: &str| outside.path().join(name).to_string_lossy().into_owned();

    let source_text = "A ledger names its keeper. ".repeat(80);
    std::fs::write(within("canon.txt"), &source_text).expect("inside source");
    std::fs::write(beyond("canon.txt"), &source_text).expect("outside source");
    std::fs::write(beyond("existing.db"), b"not a database").expect("outside file");
    std::fs::write(within("chunks.jsonl"), chunk_rows()).expect("chunks");
    std::os::unix::fs::symlink(outside.path(), inside.path().join("link-out")).expect("symlink");
    let digest = sha256(source_text.as_bytes());
    let before = listing(outside.path());

    let policy = |min: usize, max: usize| json!({"min_words":min,"max_words":max,"overlap_words":0,"sentence_boundary":".!?"});
    let representations = |raw: String, canonical: String, output: String| {
        json!({"accepted_sources":[{"source":"s1","raw_path":raw,"raw_sha256":digest,
                "canonical_path":canonical,"canonical_sha256":digest}],
            "output_dir":output,"entity_ref_prefix":"pin:",
            "current_policy":policy(50,150),"fine_policy":policy(20,60),"parent_policy":policy(100,300)})
    };
    let export = |run: String, output: String| {
        json!({"run_dir":run,"output_dir":output,"source_id":"pin","display_name":"Pin",
            "index_name":"fine","bundle_key":"public-pin-key"})
    };
    let chunks = within("chunks.jsonl");
    let cases: Vec<(&str, Value)> = vec![
        ("corpus_purge_qa", json!({"db_path":beyond("new.db")})),
        ("corpus_purge_qa", json!({"db_path":beyond("existing.db")})),
        (
            "corpus_dedup_chunks",
            json!({"db_path":beyond("existing.db"),"tagged_jsonl":chunks,"output":within("d.jsonl")}),
        ),
        (
            "corpus_consolidate_chunks",
            json!({"db_path":beyond("existing.db"),"tagged_jsonl":chunks,"output":within("c.jsonl")}),
        ),
        (
            "corpus_embed",
            json!({"db_path":beyond("new.db"),"chunks_jsonl":chunks,"model":"offline"}),
        ),
        (
            "corpus_embed",
            json!({"db_path":within("link-out/new.db"),"chunks_jsonl":chunks,"model":"offline"}),
        ),
        (
            "corpus_extract_assertions",
            json!({"db_path":beyond("new.db"),"chunks_jsonl":chunks}),
        ),
        (
            "corpus_embedding_inventory",
            json!({"db_path":beyond("existing.db"),"chunks_jsonl":chunks,"expected_model":"x/y"}),
        ),
        (
            "corpus_centroid",
            json!({"db_path":beyond("existing.db"),"author":"pin"}),
        ),
        (
            "corpus_compose",
            json!({"db_path":beyond("existing.db"),"author":"pin","prompt":"one line"}),
        ),
        (
            "corpus_rewrite",
            json!({"db_path":beyond("existing.db"),"author":"pin","content":"one line"}),
        ),
        (
            "corpus_query",
            json!({"db_path":beyond("existing.db"),"query":"ledger"}),
        ),
        (
            "corpus_query",
            json!({"db_path":beyond("absent.db"),"query":"ledger"}),
        ),
        (
            "corpus_build_prompts",
            json!({"db_path":beyond("existing.db"),"tagged_jsonl":chunks,"output":within("p.jsonl"),"context_k":1}),
        ),
        (
            "corpus_ingest_qa",
            json!({"db_path":beyond("new.db"),"generated_jsonl":chunks,"grounding_manifest":within("m.json"),
            "source_chunks_jsonl":chunks,"output":within("qa.jsonl")}),
        ),
        (
            "corpus_build_chunk_representations",
            representations(within("canon.txt"), within("canon.txt"), beyond("repr")),
        ),
        (
            "corpus_build_chunk_representations",
            representations(beyond("canon.txt"), within("canon.txt"), within("repr-raw")),
        ),
        (
            "corpus_build_chunk_representations",
            representations(
                within("canon.txt"),
                beyond("canon.txt"),
                within("repr-canonical"),
            ),
        ),
        (
            "corpus_export_bundle",
            export(beyond("run"), within("package")),
        ),
        (
            "corpus_export_bundle",
            export(within("run"), beyond("package")),
        ),
        (
            "corpus_ground_generated_qa",
            json!({"generated_jsonl":chunks,"source_chunks_jsonl":chunks,"output_dir":beyond("bundle")}),
        ),
        // The tools that write `output` last refuse it before their work.
        (
            "corpus_tag_chunks",
            json!({"chunks_jsonl":chunks,"output":beyond("tags.jsonl")}),
        ),
        (
            "corpus_dedup_chunks",
            json!({"db_path":within("x.db"),"tagged_jsonl":chunks,"output":beyond("d.jsonl")}),
        ),
        (
            "corpus_consolidate_chunks",
            json!({"db_path":within("x.db"),"tagged_jsonl":chunks,"output":beyond("c.jsonl")}),
        ),
        (
            "corpus_build_prompts",
            json!({"tagged_jsonl":chunks,"output":beyond("p.jsonl")}),
        ),
        (
            "corpus_ingest_qa",
            json!({"db_path":within("x.db"),"generated_jsonl":chunks,"grounding_manifest":within("m.json"),
                "source_chunks_jsonl":chunks,"output":beyond("qa.jsonl")}),
        ),
    ];

    // A configured embedding model, so the only refusal left is containment.
    crate::retrieval_tests::ensure_offline_embedding_model_env();
    let port = Arc::new(CountingPort::default());
    let mut wire = Wire::start(crate::helpers::corpus_test_server(port.clone())).await;
    for (tool, arguments) in &cases {
        let result = wire.call(tool, arguments.clone()).await;
        let (kind, message) = refusal(tool, &result);
        assert_eq!(kind, "invalid_argument", "{tool} {arguments}: {message}");
        assert!(
            message.contains("is outside the allowed root"),
            "{tool} {arguments}: {message}"
        );
    }
    assert_eq!(
        listing(outside.path()),
        before,
        "a refused call must leave the outside directory as it was"
    );
    for made in ["repr-raw", "repr-canonical", "package"] {
        assert!(
            !inside.path().join(made).exists(),
            "{made} must not be published by a refused call"
        );
    }
    assert_eq!(
        port.calls.load(Ordering::SeqCst),
        0,
        "no model call is spent on a refused path"
    );
}

/// expect: "The two names an export turns into file names cannot carry a path."
#[tokio::test]
async fn export_bundle_names_are_single_path_components() {
    let inside = inside();
    let within = |name: &str| inside.path().join(name).to_string_lossy().into_owned();
    let mut wire = Wire::start(crate::helpers::corpus_test_server(Arc::new(
        CountingPort::default(),
    )))
    .await;
    for (field, source_id, index_name) in [
        ("index_name", "pin", "../fine"),
        ("index_name", "pin", ".hidden"),
        ("source_id", "nested/pin", "fine"),
    ] {
        let result = wire
            .call(
                "corpus_export_bundle",
                json!({"run_dir":within("run"),"output_dir":within("package"),"source_id":source_id,
                    "display_name":"Pin","index_name":index_name,"bundle_key":"public-pin-key"}),
            )
            .await;
        let (kind, message) = refusal("corpus_export_bundle", &result);
        assert_eq!(kind, "invalid_argument", "{message}");
        assert!(message.contains(field), "{field}: {message}");
    }
    assert!(!inside.path().join("package").exists());
}

/// expect: "An embedding run that stored nothing tells me why, as an error;
/// a run that stored part tells me the first provider failure beside the counts."
#[tokio::test]
async fn an_embed_run_reports_the_provider_failure() {
    let inside = inside();
    let within = |name: &str| inside.path().join(name).to_string_lossy().into_owned();
    std::fs::write(within("chunks.jsonl"), chunk_rows()).expect("chunks");

    let mut wire = Wire::start(crate::helpers::corpus_test_server(Arc::new(CountingPort {
        refuse_all: true,
        ..Default::default()
    })))
    .await;
    let result = wire
        .call(
            "corpus_embed",
            json!({"db_path":within("none.db"),"chunks_jsonl":within("chunks.jsonl"),"model":"offline"}),
        )
        .await;
    let (kind, message) = refusal("corpus_embed", &result);
    assert_eq!(kind, "permission_denied", "{message}");
    assert!(
        message.contains("no chunk was embedded (2 requested, model offline)")
            && message.contains("PROBE_API_KEY not set"),
        "{message}"
    );

    let mut wire = Wire::start(crate::helpers::corpus_test_server(Arc::new(
        CountingPort::default(),
    )))
    .await;
    let result = wire
        .call(
            "corpus_embed",
            json!({"db_path":within("part.db"),"chunks_jsonl":within("chunks.jsonl"),"model":"offline","batch_size":1}),
        )
        .await;
    let summary = content("corpus_embed", &result);
    assert_eq!(summary["embedded"], 1, "{summary}");
    assert_eq!(summary["failed"], 1, "{summary}");
    assert_eq!(summary["failed_entity_refs"], json!(["pin:doc:1"]));
    assert!(
        summary["first_error"]
            .as_str()
            .is_some_and(|error| error.contains("PROBE_API_KEY not set")),
        "{summary}"
    );
}

/// expect: "With no model configured, each model-using tool refuses naming
/// its setting, calls no model, and leaves no database or output behind."
/// pre: an isolated process with no model variables and an explicitly blank
/// embedding setting. post: permission_denied naming the setting and its
/// variable in one clean sentence; nothing created.
#[tokio::test]
async fn unset_models_refuse_naming_the_setting_and_leave_nothing() {
    const CHILD: &str = "HKASK_CORPUS_UNSET_MODEL_PIN_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let directory = tempfile::tempdir().expect("isolated root");
        let config = directory.path().join("zed-kask");
        std::fs::create_dir_all(&config).expect("config dir");
        std::fs::write(
            config.join("settings.json"),
            r#"{"kask":{"models":{"embedding_model":""}}}"#,
        )
        .expect("blank settings");
        let output = tokio::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "containment_tests::unset_models_refuse_naming_the_setting_and_leave_nothing",
                "--nocapture",
            ])
            .env_clear()
            .env(CHILD, "1")
            .env("HOME", directory.path())
            .env("XDG_CONFIG_HOME", directory.path())
            .env("XDG_RUNTIME_DIR", directory.path())
            .env("HKASK_DATA_DIR", directory.path().join("data"))
            .env("HKASK_ARTIFACTS_DIR", directory.path().join("artifacts"))
            .current_dir(directory.path())
            .output()
            .await
            .expect("isolated run");
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    std::fs::write("chunks.jsonl", chunk_rows()).expect("chunks");
    std::fs::write("style.db", b"present, never opened").expect("placeholder database");
    let port = Arc::new(CountingPort::default());
    let mut wire = Wire::start(crate::helpers::corpus_test_server(port.clone())).await;
    let classifier = "kask.models.classifier_model (injected as HKASK_CLASSIFIER_MODEL)";
    let embedding = "kask.models.embedding_model (injected as HKASK_EMBEDDING_MODEL)";
    let cases = [
        (
            "corpus_extract_assertions",
            json!({"db_path":"assertions.db","chunks_jsonl":"chunks.jsonl"}),
            classifier,
        ),
        (
            "corpus_tag_chunks",
            json!({"chunks_jsonl":"chunks.jsonl","output":"tags.jsonl"}),
            classifier,
        ),
        (
            "corpus_embed",
            json!({"db_path":"embed.db","chunks_jsonl":"chunks.jsonl"}),
            embedding,
        ),
        ("corpus_query", json!({"query":"ledger"}), embedding),
        (
            "corpus_centroid",
            json!({"db_path":"style.db","author":"pin"}),
            embedding,
        ),
        (
            "corpus_compose",
            json!({"db_path":"style.db","author":"pin","prompt":"one line"}),
            embedding,
        ),
        (
            "corpus_rewrite",
            json!({"db_path":"style.db","author":"pin","content":"one line"}),
            embedding,
        ),
    ];
    for (tool, arguments, setting) in cases {
        let result = wire.call(tool, arguments).await;
        let (kind, message) = refusal(tool, &result);
        assert_eq!(kind, "permission_denied", "{tool}: {message}");
        assert!(message.contains(setting), "{tool}: {message}");
        assert!(
            !message.contains('\\') && !message.contains("  "),
            "{tool}: the refusal is one clean sentence: {message:?}"
        );
    }
    // `artifacts` is the server's own artifacts root, made by containment.
    assert_eq!(
        listing(std::path::Path::new(".")),
        ["artifacts", "chunks.jsonl", "style.db", "zed-kask"],
        "a refused call creates no database, lock or output"
    );
    assert_eq!(port.calls.load(Ordering::SeqCst), 0);
}
