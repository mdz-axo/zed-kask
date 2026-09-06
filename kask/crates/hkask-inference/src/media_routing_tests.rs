//! Offline pins for the operator's 2026-09-06 routing decision (P1/P4).
use crate::InferenceConfig;
use crate::media_router::MediaRouter;
use crate::provider::{MediaOp, MediaProvider, ProviderRegistry};
use hkask_types::{InferenceError, InferencePort, MediaGenerateParams};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const SELECTABLE: [(MediaOp, &str); 8] = [
    (MediaOp::GenerateImage, "HKASK_MEDIA_IMAGE_GEN_MODEL"),
    (MediaOp::ImageToImage, "HKASK_MEDIA_IMAGE_GEN_MODEL"),
    (MediaOp::GenerateSpeech, "HKASK_MEDIA_TTS_MODEL"),
    (MediaOp::Transcribe, "HKASK_MEDIA_STT_MODEL"),
    (MediaOp::GenerateVideo, "HKASK_MEDIA_VIDEO_MODEL"),
    (MediaOp::ImageToVideo, "HKASK_MEDIA_VIDEO_MODEL"),
    (MediaOp::ChatAudio, "HKASK_MEDIA_AUDIO_CHAT_MODEL"),
    (MediaOp::ChatJson, "HKASK_MEDIA_STRUCTURED_PASS_MODEL"),
];

type Calls = Arc<Mutex<Vec<(String, MediaOp, Option<String>)>>>;
struct RecordingProvider {
    id: &'static str,
    calls: Calls,
    failure: Option<fn() -> InferenceError>,
    supported: bool,
}
impl MediaProvider for RecordingProvider {
    fn id(&self) -> &'static str {
        self.id
    }
    fn supports(&self, _: MediaOp) -> bool {
        self.supported
    }
    fn execute<'a>(
        &'a self,
        op: MediaOp,
        params: &'a MediaGenerateParams,
    ) -> Pin<Box<dyn Future<Output = Result<Value, InferenceError>> + Send + 'a>> {
        Box::pin(async move {
            self.calls
                .lock()
                .expect("calls lock")
                .push((self.id.into(), op, params.model.clone()));
            match self.failure {
                Some(failure) => Err(failure()),
                None => Ok(json!({"provider": self.id, "model": params.model})),
            }
        })
    }
}
fn registry(
    calls: &Calls,
    failure: Option<fn() -> InferenceError>,
    supported: bool,
) -> ProviderRegistry {
    ProviderRegistry::new(vec![
        Arc::new(RecordingProvider {
            id: "deepinfra",
            calls: calls.clone(),
            failure: None,
            supported: true,
        }),
        Arc::new(RecordingProvider {
            id: "openrouter",
            calls: calls.clone(),
            failure,
            supported,
        }),
    ])
}
fn params(model: &str) -> MediaGenerateParams {
    MediaGenerateParams {
        model: Some(model.into()),
        ..Default::default()
    }
}

/// expect: "Each operation honors my full provider name, independent of order."
/// [P1] Motivating; dcterms:identifier: ProviderRegistry::execute
#[tokio::test]
async fn every_selectable_op_routes_once_and_strips_only_provider() {
    for (op, _) in SELECTABLE {
        for (prefix, expected) in [("oPeNrOuTeR", "openrouter"), ("DEEPINFRA", "deepinfra")] {
            let calls = Calls::default();
            let result = registry(&calls, None, true)
                .execute(op, &params(&format!("{prefix}/vendor/model")))
                .await
                .expect("route");
            assert_eq!(result["model"], "vendor/model");
            assert_eq!(
                *calls.lock().expect("calls"),
                vec![(expected.into(), op, Some("vendor/model".into()))]
            );
        }
    }
}

/// expect: "Invalid model selection sends nothing to any provider."
/// [P4] Motivating; dcterms:identifier: ProviderRegistry::execute
#[tokio::test]
async fn invalid_or_ambiguous_selection_never_dispatches() {
    let calls = Calls::default();
    let providers = registry(&calls, None, true);
    for (op, _) in SELECTABLE {
        for model in [
            "",
            " ",
            "model",
            "vendor/model",
            "OR/model",
            "DI/model",
            "ollama/model",
            "OpenRouter/",
            "DeepInfra//",
            "OpenRouter/vendor/",
            " OpenRouter/model",
            "OpenRouter/ model",
            "OpenRouter/model\n",
            "DeepInfra/vendor/mo del",
        ] {
            assert!(
                matches!(providers.execute(op, &params(model)).await, Err(InferenceError::Model(message)) if message.contains("OpenRouter/<model>") && message.contains("DeepInfra/<model>")),
                "{op:?}: {model:?}"
            );
        }
    }
    assert!(matches!(
        registry(&calls, None, false)
            .execute(MediaOp::GenerateImage, &params("OpenRouter/model"))
            .await,
        Err(InferenceError::Model(_))
    ));
    let duplicate = ProviderRegistry::new(vec![
        Arc::new(RecordingProvider {
            id: "OpenRouter",
            calls: calls.clone(),
            failure: None,
            supported: true,
        }),
        Arc::new(RecordingProvider {
            id: "openrouter",
            calls: calls.clone(),
            failure: None,
            supported: true,
        }),
    ]);
    assert!(
        matches!(duplicate.execute(MediaOp::GenerateImage, &params("OpenRouter/model")).await, Err(InferenceError::Model(message)) if message.contains("ambiguous"))
    );
    assert!(calls.lock().expect("calls").is_empty());
}

/// expect: "A provider failure keeps its original type and never retries elsewhere."
/// [P4] Motivating; dcterms:identifier: ProviderRegistry::execute
#[tokio::test]
async fn selected_errors_are_unchanged_without_fallback() {
    let failures: [fn() -> InferenceError; 4] = [
        || InferenceError::Auth("sentinel auth".into()),
        || InferenceError::NotConfigured("sentinel config".into()),
        || InferenceError::Connection("sentinel connection".into()),
        || InferenceError::Model("sentinel model".into()),
    ];
    for failure in failures {
        let calls = Calls::default();
        let error = registry(&calls, Some(failure), true)
            .execute(MediaOp::GenerateImage, &params("OpenRouter/vendor/model"))
            .await
            .expect_err("selected failure");
        assert_eq!(
            std::mem::discriminant(&error),
            std::mem::discriminant(&failure())
        );
        assert_eq!(error.to_string(), failure().to_string());
        assert_eq!(calls.lock().expect("calls").len(), 1);
        assert_eq!(calls.lock().expect("calls")[0].0, "openrouter");
    }
}

/// expect: "A missing selected key never borrows the other provider's key."
/// [P4] Motivating; dcterms:identifier: MediaRouter::media_generate
#[tokio::test]
async fn missing_selected_credentials_and_unsupported_ops_make_no_requests() {
    let deepinfra = HttpCapture::start(200).await;
    let openrouter = HttpCapture::start(200).await;
    for (model, key, mut config) in [
        (
            "OpenRouter/vendor/model",
            "OPENROUTER_API_KEY",
            InferenceConfig {
                deepinfra_api_key: "sentinel".into(),
                ..Default::default()
            },
        ),
        (
            "DeepInfra/vendor/model",
            "DEEPINFRA_API_KEY",
            InferenceConfig {
                openrouter_api_key: "sentinel".into(),
                ..Default::default()
            },
        ),
    ] {
        config.deepinfra_base_url = deepinfra.url.clone();
        config.openrouter_base_url = openrouter.url.clone();
        let error = MediaRouter::new(config)
            .media_generate("generate_image", &params(model))
            .await
            .expect_err("missing named key");
        assert!(matches!(error, InferenceError::NotConfigured(message) if message.contains(key)));
    }
    let router = MediaRouter::new(InferenceConfig {
        deepinfra_api_key: "sentinel".into(),
        openrouter_api_key: "sentinel".into(),
        deepinfra_base_url: deepinfra.url.clone(),
        openrouter_base_url: openrouter.url.clone(),
        ..Default::default()
    });
    for (op, model) in [
        ("chat_audio", "DeepInfra/model"),
        ("chat_json", "DeepInfra/model"),
        ("image_to_image", "OpenRouter/model"),
        ("generate_speech", "OpenRouter/model"),
    ] {
        assert!(
            matches!(router.media_generate(op, &params(model)).await, Err(InferenceError::Model(message)) if message.contains("does not support"))
        );
    }
    let router = MediaRouter::new(InferenceConfig {
        openrouter_api_key: "sentinel".into(),
        openrouter_base_url: openrouter.url.clone(),
        deepinfra_base_url: deepinfra.url.clone(),
        ..Default::default()
    });
    for op in ["remove_background", "upscale"] {
        assert!(
            matches!(router.media_generate(op, &MediaGenerateParams::default()).await,
            Err(InferenceError::NotConfigured(message)) if message.contains("DEEPINFRA_API_KEY"))
        );
    }
    assert!(
        deepinfra
            .requests
            .lock()
            .expect("DeepInfra capture")
            .is_empty()
    );
    assert!(
        openrouter
            .requests
            .lock()
            .expect("OpenRouter capture")
            .is_empty()
    );
}

/// expect: "Fixed operations need no model configuration and reject ignored overrides."
/// [P1] Motivating; dcterms:identifier: ProviderRegistry::execute
#[tokio::test]
async fn fixed_ops_choose_deepinfra_and_reject_overrides() {
    for op in [MediaOp::RemoveBackground, MediaOp::Upscale] {
        let calls = Calls::default();
        let providers = registry(&calls, None, true);
        let result = providers
            .execute(op, &MediaGenerateParams::default())
            .await
            .expect("fixed route");
        assert_eq!(result["provider"], "deepinfra");
        assert!(result["model"].is_null());
        for model in ["", "DeepInfra/Bria/remove_background", "OpenRouter/model"] {
            assert!(matches!(
                providers.execute(op, &params(model)).await,
                Err(InferenceError::Model(_))
            ));
        }
        assert_eq!(calls.lock().expect("calls").len(), 1);
    }
}

/// Env-driven checks run in clean subprocesses; neither env mutation nor
/// LazyInferencePort's OnceLock can contaminate another test.
fn subprocess(test: &str, case: &str) -> std::process::Command {
    let mut command = std::process::Command::new(std::env::current_exe().expect("test executable"));
    command
        .env_clear()
        .arg("--exact")
        .arg(format!("media_routing_tests::{test}"))
        .arg("--nocapture")
        .env("HKASK_MEDIA_ROUTING_TEST_CASE", case);
    command
}

/// expect: "The operation's env model applies only when I supply no override."
/// [P1] Motivating; dcterms:identifier: ProviderRegistry::execute
#[tokio::test]
async fn env_resolution_is_operation_specific_and_overrides_win() {
    if let Ok(case) = std::env::var("HKASK_MEDIA_ROUTING_TEST_CASE") {
        let (op, variable) = SELECTABLE
            .iter()
            .find(|(op, _)| op.as_str() == case)
            .expect("op case");
        let calls = Calls::default();
        let providers = registry(&calls, None, true);
        if let Ok(configured) = std::env::var(variable) {
            if configured.is_empty() {
                assert!(matches!(
                    providers
                        .execute(*op, &MediaGenerateParams::default())
                        .await,
                    Err(InferenceError::Model(_))
                ));
            } else {
                let result = providers
                    .execute(*op, &MediaGenerateParams::default())
                    .await
                    .expect("env route");
                assert_eq!(result["provider"], "deepinfra");
                assert_eq!(result["model"], "env/model");
            }
        } else {
            assert!(
                matches!(providers.execute(*op, &MediaGenerateParams::default()).await, Err(InferenceError::NotConfigured(message)) if message.contains(variable))
            );
        }
        let result = providers
            .execute(*op, &params("OpenRouter/override/model"))
            .await
            .expect("override");
        assert_eq!(result["provider"], "openrouter");
        assert_eq!(result["model"], "override/model");
        assert!(matches!(
            providers.execute(*op, &params("")).await,
            Err(InferenceError::Model(_))
        ));
        return;
    }
    for (op, variable) in SELECTABLE {
        for configured in [None, Some(""), Some("DeepInfra/env/model")] {
            let mut command = subprocess(
                "env_resolution_is_operation_specific_and_overrides_win",
                op.as_str(),
            );
            if let Some(model) = configured {
                command.env(variable, model);
            }
            let output = tokio::time::timeout(
                Duration::from_secs(10),
                tokio::process::Command::from(command)
                    .kill_on_drop(true)
                    .output(),
            )
            .await
            .expect("bounded isolated test")
            .expect("isolated test");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
    }
}

#[derive(Debug)]
struct HttpRequest {
    path: String,
    headers: String,
    body: String,
}
struct HttpCapture {
    url: String,
    requests: Arc<Mutex<Vec<HttpRequest>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for HttpCapture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl HttpCapture {
    async fn start(status: u16) -> Self {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind HTTP");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let task = tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.expect("accept HTTP");
                let mut stream = BufReader::new(stream);
                let mut line = String::new();
                stream.read_line(&mut line).await.expect("request line");
                let path = line
                    .split_whitespace()
                    .nth(1)
                    .expect("request path")
                    .to_owned();
                let mut headers = String::new();
                let mut length = 0;
                loop {
                    line.clear();
                    stream.read_line(&mut line).await.expect("header");
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse().expect("content length");
                    }
                    headers.push_str(&line);
                }
                let mut body = vec![0; length];
                stream.read_exact(&mut body).await.expect("request body");
                captured.lock().expect("capture").push(HttpRequest {
                    path,
                    headers,
                    body: String::from_utf8(body).expect("UTF-8 payload"),
                });
                // A complete video result avoids timers; all other adapters use
                // their actual decoding path over this provider response fixture.
                let response = if status == 200 {
                    r#"{"id":"job","status":"completed","url":"https://example.invalid/video.mp4","data":[{"b64_json":"QUJD"}],"text":"hello","words":[],"choices":[{"message":{"content":"{}"}}]}"#
                } else {
                    r#"{"error":"sentinel rejection"}"#
                };
                stream.get_mut().write_all(format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await.expect("response");
            }
        });
        Self {
            url,
            requests,
            task,
        }
    }
}

/// expect: "The actual adapter receives only my selected local model and media payload."
/// [P1] Motivating; [P4] Constraining: no call to the unselected listener.
/// dcterms:identifier: MediaRouter::media_generate
#[tokio::test]
async fn real_router_adapters_capture_models_payloads_and_no_cross_provider_requests() {
    let directory = tempfile::tempdir().expect("audio directory");
    let audio = directory.path().join("input.wav");
    std::fs::write(&audio, b"RIFF").expect("audio fixture");
    for provider in ["DeepInfra", "OpenRouter"] {
        for op in SELECTABLE
            .iter()
            .map(|(op, _)| *op)
            .chain([MediaOp::RemoveBackground, MediaOp::Upscale])
        {
            let supported = match provider {
                "DeepInfra" => !matches!(op, MediaOp::ChatAudio | MediaOp::ChatJson),
                _ => !matches!(
                    op,
                    MediaOp::ImageToImage
                        | MediaOp::GenerateSpeech
                        | MediaOp::RemoveBackground
                        | MediaOp::Upscale
                ),
            };
            if !supported {
                continue;
            }
            let selected = HttpCapture::start(200).await;
            let other = HttpCapture::start(200).await;
            let (deepinfra_url, openrouter_url) = if provider == "DeepInfra" {
                (&selected.url, &other.url)
            } else {
                (&other.url, &selected.url)
            };
            let router = MediaRouter::new(InferenceConfig {
                deepinfra_api_key: "sentinel-deepinfra".into(),
                openrouter_api_key: "sentinel-openrouter".into(),
                deepinfra_base_url: deepinfra_url.clone(),
                openrouter_base_url: openrouter_url.clone(),
                ..Default::default()
            });
            let parameters = MediaGenerateParams {
                model: op.model_env().map(|_| format!("{provider}/vendor/model")),
                prompt: Some("sentinel prompt".into()),
                image_url: Some("https://example.invalid/input.png".into()),
                audio_url: Some(audio.to_string_lossy().into_owned()),
                text: Some("sentinel speech".into()),
                voice: Some("sentinel voice".into()),
                schema: Some(r#"{"type":"object"}"#.into()),
                size: Some("512x512".into()),
                count: Some(2),
                strength: Some(0.5),
                mask: Some("data:image/png;base64,QUJD".into()),
                scale: Some(2),
                duration: Some(3.0),
                language: Some("en".into()),
            };
            tokio::time::timeout(
                Duration::from_secs(3),
                router.media_generate(op.as_str(), &parameters),
            )
            .await
            .expect("bounded HTTP")
            .expect("real adapter success");
            assert!(
                other
                    .requests
                    .lock()
                    .expect("unselected capture")
                    .is_empty(),
                "{provider} {op:?}"
            );
            let requests = selected.requests.lock().expect("capture");
            assert_eq!(
                requests.len(),
                if provider == "OpenRouter"
                    && matches!(op, MediaOp::GenerateVideo | MediaOp::ImageToVideo)
                {
                    2
                } else {
                    1
                }
            );
            let request = requests.first().expect("selected request");
            let expected_path = if provider == "DeepInfra" {
                match op {
                    MediaOp::GenerateImage => "/v1/openai/images/generations",
                    MediaOp::RemoveBackground => "/v1/inference/Bria/remove_background",
                    MediaOp::Upscale => "/v1/inference/latentconsistency/upscale",
                    _ => "/v1/inference/vendor/model",
                }
            } else {
                match op {
                    MediaOp::GenerateImage => "/v1/images",
                    MediaOp::Transcribe => "/v1/audio/transcriptions",
                    MediaOp::GenerateVideo | MediaOp::ImageToVideo => "/v1/videos",
                    _ => "/v1/chat/completions",
                }
            };
            assert_eq!(request.path, expected_path);
            assert!(request.headers.contains(&format!(
                "Bearer sentinel-{}",
                provider.to_ascii_lowercase()
            )));
            if provider == "DeepInfra" && op == MediaOp::Transcribe {
                assert!(request.body.contains("RIFF"));
                assert!(request.body.contains("name=\"language\""));
                continue;
            }
            let body: Value = serde_json::from_str(&request.body).expect("JSON payload");
            if provider == "OpenRouter" || op == MediaOp::GenerateImage {
                assert_eq!(body["model"], "vendor/model");
            }
            match op {
                MediaOp::GenerateImage => {
                    assert_eq!(body["prompt"], "sentinel prompt");
                    assert_eq!(body["n"], 2);
                    assert_eq!(body["size"], "512x512");
                }
                MediaOp::ImageToImage => {
                    assert_eq!(body["image_url"], parameters.image_url.expect("image"));
                    assert_eq!(body["mask_url"], parameters.mask.expect("mask"));
                    assert_eq!(body["strength"], 0.5);
                }
                MediaOp::GenerateSpeech => {
                    assert_eq!(body["text"], "sentinel speech");
                    assert_eq!(body["voice"], "sentinel voice");
                }
                MediaOp::Transcribe => {
                    assert_eq!(body["input_audio"]["data"], "UklGRg==");
                    assert_eq!(body["timestamp_granularities"], json!(["word", "segment"]));
                }
                MediaOp::ChatAudio => assert_eq!(
                    body["messages"][0]["content"][1]["input_audio"]["data"],
                    "UklGRg=="
                ),
                MediaOp::ChatJson => {
                    assert_eq!(body["response_format"]["json_schema"]["strict"], true)
                }
                MediaOp::Upscale => assert_eq!(body["outscale"], 2),
                MediaOp::RemoveBackground => {
                    assert_eq!(body["image_url"], parameters.image_url.expect("image"))
                }
                MediaOp::GenerateVideo | MediaOp::ImageToVideo => {
                    assert_eq!(body["duration"], 3.0);
                    assert_eq!(body["prompt"], "sentinel prompt");
                }
            }
        }
    }
}

/// expect: "A real provider's rejected key remains an Auth error, never a retry."
/// [P4] Motivating; dcterms:identifier: MediaRouter::media_generate
#[tokio::test]
async fn real_provider_http_failures_preserve_type_without_fallback() {
    for provider in ["DeepInfra", "OpenRouter"] {
        for status in [401, 403, 500] {
            let selected = HttpCapture::start(status).await;
            let other = HttpCapture::start(200).await;
            let router = MediaRouter::new(InferenceConfig {
                deepinfra_api_key: "sentinel".into(),
                openrouter_api_key: "sentinel".into(),
                deepinfra_base_url: if provider == "DeepInfra" {
                    selected.url.clone()
                } else {
                    other.url.clone()
                },
                openrouter_base_url: if provider == "OpenRouter" {
                    selected.url.clone()
                } else {
                    other.url.clone()
                },
                ..Default::default()
            });
            let error = tokio::time::timeout(
                Duration::from_secs(3),
                router.media_generate(
                    "generate_image",
                    &params(&format!("{provider}/vendor/model")),
                ),
            )
            .await
            .expect("bounded HTTP")
            .expect_err("HTTP rejection");
            if status == 500 {
                assert!(matches!(error, InferenceError::Connection(_)));
            } else {
                assert!(matches!(error, InferenceError::Auth(_)));
            }
            assert!(error.to_string().contains("sentinel rejection"));
            assert_eq!(selected.requests.lock().expect("selected").len(), 1);
            assert!(other.requests.lock().expect("unselected").is_empty());
        }
    }
}

/// expect: "Media uses child-local keys even with a live IPC listener."
/// [P4] Motivating; dcterms:identifier: LazyInferencePort::media_generate
#[cfg(unix)]
#[tokio::test]
async fn lazy_media_is_child_local_with_actual_settings_stt_default() {
    if std::env::var_os("HKASK_MEDIA_ROUTING_TEST_CASE").is_some() {
        let result = crate::LazyInferencePort::new()
            .media_generate(
                "transcribe",
                &MediaGenerateParams {
                    audio_url: Some(
                        std::env::var("HKASK_MEDIA_TEST_AUDIO").expect("audio fixture"),
                    ),
                    ..Default::default()
                },
            )
            .await
            .expect("child-local transcription");
        assert_eq!(result["text"], "hello");
        return;
    }
    let directory = tempfile::tempdir().expect("temporary fixtures");
    let socket = directory.path().join("inference.sock");
    let listener = tokio::net::UnixListener::bind(&socket).expect("IPC listener");
    let audio = directory.path().join("input.wav");
    std::fs::write(&audio, b"RIFF").expect("audio fixture");
    let selected = HttpCapture::start(200).await;
    let other = HttpCapture::start(200).await;
    let mut command = subprocess(
        "lazy_media_is_child_local_with_actual_settings_stt_default",
        "lazy",
    );
    command
        .env("HKASK_INFERENCE_SOCKET", &socket)
        .env("HKASK_MEDIA_TEST_AUDIO", &audio)
        .env(
            "HKASK_MEDIA_STT_MODEL",
            crate::model_constants::DEFAULT_MEDIA_STT_MODEL,
        )
        .env("OPENROUTER_API_KEY", "sentinel-openrouter")
        .env("DEEPINFRA_API_KEY", "sentinel-deepinfra")
        .env("OPENROUTER_BASE_URL", &selected.url)
        .env("DEEPINFRA_BASE_URL", &other.url);
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::from(command)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("bounded child")
    .expect("child output");
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(20), listener.accept())
            .await
            .is_err(),
        "media must not connect to IPC"
    );
    assert!(other.requests.lock().expect("unselected").is_empty());
    let requests = selected.requests.lock().expect("capture");
    assert_eq!(requests.len(), 1);
    let request = requests.first().expect("STT request");
    assert_eq!(request.path, "/v1/audio/transcriptions");
    let body: Value = serde_json::from_str(&request.body).expect("STT payload");
    assert_eq!(
        body["model"],
        crate::model_constants::DEFAULT_MEDIA_STT_MODEL
            .split_once('/')
            .expect("qualified default")
            .1
    );
    assert_eq!(body["input_audio"]["data"], "UklGRg==");
}

const UNSAFE_LOCAL_MODELS: &[&str] = &[
    "vendor/model#ignored",
    "vendor/model?query=value",
    "../../other",
    "vendor/../other",
    "vendor/./model",
    "vendor/.",
    "vendor/..",
    ".",
    "..",
    "vendor\\model",
    "vendor/model\\..\\other",
    "vendor/%2e%2e/other",
    "vendor/%2E/model",
    "vendor/model%2fother",
    "vendor/model%5cother",
    "vendor/model%23fragment",
    "vendor/model%3fquery",
    "vendor/model%252e",
    "vendor/model%",
    "vendor/model\0",
    "vendor/model\u{1}",
    "vendor/model\u{7f}",
    "vendor/model\u{9f}",
    "vendor/model\r\n",
];

/// expect: "Unsafe selected models are rejected before any adapter runs."
/// [P4] Motivating; pre: recording adapters registered for both provider names.
/// post: every unsafe local model returns Model with zero adapter invocations.
/// dcterms:identifier: ProviderRegistry::execute
#[tokio::test]
async fn model_url_safety_registry_rejects_before_adapter_entry() {
    let calls = Calls::default();
    let providers = registry(&calls, None, true);
    for provider in ["DeepInfra", "OpenRouter"] {
        for local_model in UNSAFE_LOCAL_MODELS {
            assert!(matches!(
                providers
                    .execute(
                        MediaOp::GenerateVideo,
                        &params(&format!("{provider}/{local_model}"))
                    )
                    .await,
                Err(InferenceError::Model(_))
            ));
        }
    }
    assert!(calls.lock().expect("adapter calls").is_empty());
}

fn model_url_safety_config(deepinfra: &HttpCapture, openrouter: &HttpCapture) -> InferenceConfig {
    InferenceConfig {
        deepinfra_api_key: "sentinel-deepinfra".into(),
        openrouter_api_key: "sentinel-openrouter".into(),
        deepinfra_base_url: deepinfra.url.clone(),
        openrouter_base_url: openrouter.url.clone(),
        ..Default::default()
    }
}

/// expect: "A model cannot change my authenticated endpoint via URL syntax."
/// [P4] Motivating; [P1] Constraining: do not silently change model identity.
/// pre: actual adapters configured against loopback listeners.
/// post: each unsafe explicit model returns Model and neither listener sees a request.
/// dcterms:identifier: MediaRouter::media_generate
#[tokio::test]
async fn model_url_safety_rejects_explicit_models_without_http() {
    let deepinfra = HttpCapture::start(200).await;
    let openrouter = HttpCapture::start(200).await;
    let router = MediaRouter::new(model_url_safety_config(&deepinfra, &openrouter));
    let mut failures = Vec::new();
    for provider in ["DeepInfra", "OpenRouter"] {
        for local_model in UNSAFE_LOCAL_MODELS {
            let model = format!("{provider}/{local_model}");
            let result = tokio::time::timeout(
                Duration::from_secs(3),
                router.media_generate("generate_video", &params(&model)),
            )
            .await
            .expect("bounded request");
            if !matches!(result, Err(InferenceError::Model(_))) {
                failures.push(format!("{model:?}: {result:?}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "expected Model before HTTP: {failures:#?}"
    );
    assert!(
        deepinfra
            .requests
            .lock()
            .expect("DeepInfra capture")
            .is_empty()
    );
    assert!(
        openrouter
            .requests
            .lock()
            .expect("OpenRouter capture")
            .is_empty()
    );
}

/// expect: "Environment model selection has the same endpoint safety as an override."
/// [P4] Motivating; pre: clean env-isolated subprocess with real loopback adapters.
/// post: unsafe env model with params.model=None returns Model and sends no requests.
/// dcterms:identifier: ProviderRegistry::execute
#[tokio::test]
async fn model_url_safety_rejects_env_models_without_http() {
    if std::env::var_os("HKASK_MEDIA_ROUTING_TEST_CASE").is_some() {
        let deepinfra = HttpCapture::start(200).await;
        let openrouter = HttpCapture::start(200).await;
        let router = MediaRouter::new(model_url_safety_config(&deepinfra, &openrouter));
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            router.media_generate("generate_video", &MediaGenerateParams::default()),
        )
        .await
        .expect("bounded request");
        assert!(
            matches!(result, Err(InferenceError::Model(_))),
            "expected Model before HTTP: {result:?}"
        );
        assert!(
            deepinfra
                .requests
                .lock()
                .expect("DeepInfra capture")
                .is_empty()
        );
        assert!(
            openrouter
                .requests
                .lock()
                .expect("OpenRouter capture")
                .is_empty()
        );
        return;
    }
    let mut failures = Vec::new();
    for provider in ["DeepInfra", "OpenRouter"] {
        // OS environment strings cannot contain NUL; the explicit/direct tests cover it.
        for local_model in UNSAFE_LOCAL_MODELS
            .iter()
            .filter(|model| !model.contains('\0'))
        {
            let model = format!("{provider}/{local_model}");
            let mut command = subprocess(
                "model_url_safety_rejects_env_models_without_http",
                "unsafe-env",
            );
            command.env("HKASK_MEDIA_VIDEO_MODEL", &model);
            let output = tokio::time::timeout(
                Duration::from_secs(10),
                tokio::process::Command::from(command)
                    .kill_on_drop(true)
                    .output(),
            )
            .await
            .expect("bounded child")
            .expect("child output");
            if !output.status.success() {
                failures.push(format!(
                    "{model:?}: {}",
                    String::from_utf8_lossy(&output.stdout)
                ));
            }
        }
    }
    assert!(failures.is_empty(), "unsafe env failures: {failures:#?}");
}

/// expect: "Calling an adapter directly cannot bypass endpoint safety."
/// [P4] Motivating; pre: real MediaProvider implementations, provider-local models.
/// post: unsafe local models return Model before any HTTP, without registry dispatch.
/// dcterms:identifier: MediaProvider::execute
#[tokio::test]
async fn model_url_safety_direct_adapters_cannot_bypass_validation() {
    use crate::media_providers::{DeepInfraMediaProvider, OpenRouterMediaProvider};
    let deepinfra = HttpCapture::start(200).await;
    let openrouter = HttpCapture::start(200).await;
    let config = model_url_safety_config(&deepinfra, &openrouter);
    let client = Arc::new(reqwest::Client::new());
    let providers: Vec<Box<dyn MediaProvider>> = vec![
        Box::new(DeepInfraMediaProvider::new(&config, client.clone()).expect("DeepInfra adapter")),
        Box::new(OpenRouterMediaProvider::new(&config, client).expect("OpenRouter adapter")),
    ];
    let mut failures = Vec::new();
    for provider in providers {
        for local_model in UNSAFE_LOCAL_MODELS {
            let result = tokio::time::timeout(
                Duration::from_secs(3),
                provider.execute(MediaOp::GenerateVideo, &params(local_model)),
            )
            .await
            .expect("bounded request");
            if !matches!(result, Err(InferenceError::Model(_))) {
                failures.push(format!("{} {local_model:?}: {result:?}", provider.id()));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "direct adapter safety failures: {failures:#?}"
    );
    assert!(
        deepinfra
            .requests
            .lock()
            .expect("DeepInfra capture")
            .is_empty()
    );
    assert!(
        openrouter
            .requests
            .lock()
            .expect("OpenRouter capture")
            .is_empty()
    );
}

/// expect: "Legitimate model names keep their identity in every native endpoint."
/// [P1] Motivating; [P4] Constraining: preserve configured base path and host.
/// pre: real DeepInfra adapter, loopback base URL, local audio fixture.
/// post: safe punctuation/vendor segments reach the exact native path, no other provider.
/// dcterms:identifier: DeepInfraMediaProvider::execute
#[tokio::test]
async fn model_url_safety_preserves_native_model_identity() {
    let directory = tempfile::tempdir().expect("audio directory");
    let audio = directory.path().join("input.wav");
    std::fs::write(&audio, b"RIFF").expect("audio fixture");
    let deepinfra = HttpCapture::start(200).await;
    let openrouter = HttpCapture::start(200).await;
    let mut config = model_url_safety_config(&deepinfra, &openrouter);
    config.deepinfra_base_url.push_str("/configured-prefix");
    let router = MediaRouter::new(config);
    for (local_model, encoded_path) in [
        ("vendor/model.v1-rc_2:8b", "vendor/model.v1-rc_2:8b"),
        ("vendor/.hidden-model", "vendor/.hidden-model"),
        ("vendor/model..weights", "vendor/model..weights"),
        ("vendor/model+variant@v1", "vendor/model+variant@v1"),
        ("vendor/modèle", "vendor/mod%C3%A8le"),
    ] {
        for op in [
            MediaOp::ImageToImage,
            MediaOp::GenerateSpeech,
            MediaOp::Transcribe,
            MediaOp::GenerateVideo,
            MediaOp::ImageToVideo,
        ] {
            let parameters = MediaGenerateParams {
                model: Some(format!("DeepInfra/{local_model}")),
                audio_url: Some(audio.to_string_lossy().into_owned()),
                image_url: Some("https://example.invalid/input.png".into()),
                ..Default::default()
            };
            tokio::time::timeout(
                Duration::from_secs(3),
                router.media_generate(op.as_str(), &parameters),
            )
            .await
            .expect("bounded request")
            .expect("valid native model");
            let requests = deepinfra.requests.lock().expect("capture");
            let request = requests.last().expect("native request");
            assert_eq!(
                request.path,
                format!("/configured-prefix/v1/inference/{encoded_path}")
            );
            assert!(request.headers.contains("Bearer sentinel-deepinfra"));
        }
    }
    assert_eq!(deepinfra.requests.lock().expect("capture").len(), 25);
    assert!(openrouter.requests.lock().expect("unselected").is_empty());
}
