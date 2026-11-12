//! The serving route — the DME probe leg's ONE route.
//!
//! The harness launches nothing. The operator runs a llama.cpp server
//! (`llama-server -m <gguf> -c <n_ctx>`) and the probe connects to it:
//! `GET /props` yields the receipts (the server's build, the model it
//! loaded, the slot context), `/apply-template` renders the GGUF's own
//! chat template with `chat_template_kwargs` (`enable_thinking` — the
//! DME-0 mechanism), and `/completion` runs the pinned sampling contract.
//!
//! Artifact binding: the rows carry the sha256 of the GGUF named on the
//! command line, so the connect step refuses a server whose loaded
//! `model_path` is a different file. A router-mode server (no single
//! loaded model) is refused for the same reason.
//!
//! Sampling parity: every knob — temperature, top_p, top_k, min_p,
//! presence_penalty, repeat_penalty, seed, n_predict, repeat_last_n — is
//! verified PER ROW against the server's `generation_settings` echo. A
//! missing or mismatched knob is a typed failure naming it, never an
//! approximation.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Value, json};

/// The environment variable carrying the server's API key, when the
/// server was started with one (llama-server's own variable name). The
/// key reaches the harness from the environment, never argv.
pub const API_KEY_ENV: &str = "LLAMA_API_KEY";

/// A connected llama.cpp server and the receipts read from its `/props`.
pub struct Serving {
    /// The server's model alias.
    pub model_id: String,
    /// The server's base URL.
    pub endpoint: String,
    /// The server's `build_info` — the engine receipt on every row.
    pub engine_version: String,
    /// The per-slot context length the server runs.
    pub n_ctx: u32,
    token: Option<String>,
    client: reqwest::blocking::Client,
}

/// The pinned sampling contract for one completion (the field names are
/// the server's own).
#[derive(Debug, Clone, Serialize)]
pub struct CompletionRequest {
    pub prompt: String,
    pub n_predict: u32,
    pub temperature: f64,
    pub top_p: f64,
    pub top_k: u32,
    pub min_p: f64,
    pub presence_penalty: f64,
    pub repeat_penalty: f64,
    pub seed: u32,
    /// The presence-penalty window, pinned to the whole context (the
    /// card semantics — `penalty_last_n = n_ctx`).
    pub repeat_last_n: i32,
}

/// One completion's outcome: the generated text, the token counts, and
/// the server's `generation_settings` echo (the parity receipt).
#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub content: String,
    pub tokens_predicted: u64,
    pub tokens_evaluated: u64,
    pub generation_settings: Value,
}

/// The receipts a `/props` body must carry.
#[derive(Debug, PartialEq)]
struct ServerProps {
    build_info: String,
    model_alias: String,
    model_path: String,
    n_ctx: u32,
}

/// Read the receipts out of a `/props` body. Every field is required: a
/// server that does not report one cannot be bound to a receipt.
fn parse_props(props: &Value) -> Result<ServerProps> {
    let text = |key: &str| -> Result<String> {
        props[key]
            .as_str()
            .map(str::to_string)
            .with_context(|| format!("the /props response carries `{key}`"))
    };
    let n_ctx = props["default_generation_settings"]["n_ctx"]
        .as_u64()
        .context("the /props response carries default_generation_settings.n_ctx")?;
    Ok(ServerProps {
        build_info: text("build_info")?,
        model_alias: text("model_alias")?,
        model_path: text("model_path")?,
        n_ctx: u32::try_from(n_ctx).context("the server's n_ctx fits in u32")?,
    })
}

/// Refuse a server whose loaded model is not the artifact the rows will
/// hash. Paths compare canonicalized, so a symlinked or relative spelling
/// of the same file passes.
fn verify_served_artifact(served: &str, artifact: &Path) -> Result<()> {
    if served.is_empty() || served == "none" {
        bail!(
            "the server reports no single loaded model (router mode) — start \
             it with `-m {}` so the rows' sha256 names the model that answered",
            artifact.display()
        );
    }
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    if canonical(Path::new(served)) != canonical(artifact) {
        bail!(
            "the server loaded `{served}` but the probe was given `{}` — the \
             rows would hash a model that did not answer",
            artifact.display()
        );
    }
    Ok(())
}

impl Serving {
    /// Connect to a running llama.cpp server and bind it to `artifact`,
    /// the GGUF the rows' sha256 is taken from.
    pub fn connect(endpoint: &str, artifact: &Path) -> Result<Self> {
        let endpoint = endpoint.trim_end_matches('/').to_string();
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5400))
            .build()
            .context("build the route client")?;
        let token = std::env::var(API_KEY_ENV).ok().filter(|t| !t.is_empty());
        let url = format!("{endpoint}/props");
        let mut request = client.get(&url);
        if let Some(token) = &token {
            request = request.bearer_auth(token);
        }
        let resp = request
            .send()
            .with_context(|| format!("GET {url} — is a llama.cpp server running at {endpoint}?"))?;
        let status = resp.status();
        let body: Value = resp
            .json()
            .with_context(|| format!("read the {url} body"))?;
        if !status.is_success() {
            bail!("GET {url} -> HTTP {status}: {body}");
        }
        let props = parse_props(&body)?;
        verify_served_artifact(&props.model_path, artifact)?;
        Ok(Self {
            model_id: props.model_alias,
            endpoint,
            engine_version: props.build_info,
            n_ctx: props.n_ctx,
            token,
            client,
        })
    }

    fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let url = format!("{}{path}", self.endpoint);
        let mut request = self.client.post(&url).json(body);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let resp = request.send().with_context(|| format!("POST {url}"))?;
        let status = resp.status();
        let body: Value = resp
            .json()
            .with_context(|| format!("read the {url} body"))?;
        if !status.is_success() {
            bail!("POST {url} -> HTTP {status}: {body}");
        }
        Ok(body)
    }

    /// Render the chat prompt under a config's thinking mode — the
    /// GGUF's own template through the server's `/apply-template`
    /// (the DME-0 mechanism: `enable_thinking` in
    /// `chat_template_kwargs`).
    pub fn render_prompt(&self, user_content: &str, enable_thinking: bool) -> Result<String> {
        let body = json!({
            "messages": [{ "role": "user", "content": user_content }],
            "chat_template_kwargs": { "enable_thinking": enable_thinking }
        });
        let out = self.post("/apply-template", &body)?;
        out["prompt"]
            .as_str()
            .map(str::to_string)
            .context("the apply-template response carries the rendered prompt")
    }

    /// One completion under the pinned sampling contract; the echo is
    /// verified before the outcome returns.
    pub fn complete(&self, req: &CompletionRequest) -> Result<CompletionResponse> {
        let body = serde_json::to_value(req)?;
        let out = self.post("/completion", &body)?;
        let content = out["content"]
            .as_str()
            .map(str::to_string)
            .context("the completion response carries content")?;
        let response = CompletionResponse {
            content,
            tokens_predicted: out["tokens_predicted"].as_u64().unwrap_or(0),
            tokens_evaluated: out["tokens_evaluated"].as_u64().unwrap_or(0),
            generation_settings: out["generation_settings"].clone(),
        };
        verify_sampling_parity(req, &response.generation_settings)?;
        Ok(response)
    }
}

/// Verify every sampling knob against the server's echo, per row. A
/// missing knob is named as missing; a mismatched knob is named with
/// both values. Never an approximation.
pub fn verify_sampling_parity(req: &CompletionRequest, echo: &Value) -> Result<()> {
    let knobs: [(&str, Value); 9] = [
        ("temperature", json!(req.temperature)),
        ("top_p", json!(req.top_p)),
        ("top_k", json!(req.top_k)),
        ("min_p", json!(req.min_p)),
        ("presence_penalty", json!(req.presence_penalty)),
        ("repeat_penalty", json!(req.repeat_penalty)),
        ("seed", json!(req.seed)),
        ("n_predict", json!(req.n_predict)),
        ("repeat_last_n", json!(req.repeat_last_n)),
    ];
    let mut missing = Vec::new();
    let mut mismatched = Vec::new();
    for (name, expected) in knobs {
        let Some(actual) = echo.get(name) else {
            missing.push(name);
            continue;
        };
        let matches = match (expected.as_f64(), actual.as_f64()) {
            (Some(a), Some(b)) => (a - b).abs() < 1e-9,
            _ => expected == *actual,
        };
        if !matches {
            mismatched.push(format!("{name}: requested {expected}, echoed {actual}"));
        }
    }
    if !missing.is_empty() {
        bail!(
            "sampling parity FAILED — the server's generation_settings echo \
             is missing knobs the route requires: {} (an in-repo route fix, \
             never an approximation)",
            missing.join(", ")
        );
    }
    if !mismatched.is_empty() {
        bail!(
            "sampling parity FAILED — the echo diverges from the pinned \
             request: {}",
            mismatched.join("; ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    fn request() -> CompletionRequest {
        CompletionRequest {
            prompt: "p".into(),
            n_predict: 64,
            temperature: 0.6,
            top_p: 0.95,
            top_k: 20,
            min_p: 0.0,
            presence_penalty: 1.5,
            repeat_penalty: 1.0,
            seed: 7,
            repeat_last_n: 8192,
        }
    }

    fn echo() -> Value {
        json!({
            "temperature": 0.6, "top_p": 0.95, "top_k": 20, "min_p": 0.0,
            "presence_penalty": 1.5, "repeat_penalty": 1.0, "seed": 7,
            "n_predict": 64, "repeat_last_n": 8192
        })
    }

    #[test]
    fn parity_passes_on_a_matching_echo_and_names_each_failure() {
        verify_sampling_parity(&request(), &echo()).expect("a matching echo passes");

        let mut diverged = echo();
        diverged["top_k"] = json!(40);
        let err = verify_sampling_parity(&request(), &diverged)
            .expect_err("a diverged knob fails")
            .to_string();
        assert!(err.contains("top_k: requested 20, echoed 40"), "{err}");

        let mut dropped = echo();
        dropped.as_object_mut().unwrap().remove("min_p");
        let err = verify_sampling_parity(&request(), &dropped)
            .expect_err("a missing knob fails")
            .to_string();
        assert!(
            err.contains("missing knobs") && err.contains("min_p"),
            "{err}"
        );
    }

    #[test]
    fn props_parse_requires_every_receipt() {
        let full = json!({
            "build_info": "b1-9e2a04a",
            "model_alias": "qwen35",
            "model_path": "/m/q.gguf",
            "default_generation_settings": { "n_ctx": 8192 }
        });
        assert_eq!(
            parse_props(&full).expect("a full body parses"),
            ServerProps {
                build_info: "b1-9e2a04a".into(),
                model_alias: "qwen35".into(),
                model_path: "/m/q.gguf".into(),
                n_ctx: 8192,
            }
        );
        for key in ["build_info", "model_alias", "model_path"] {
            let mut body = full.clone();
            body.as_object_mut().unwrap().remove(key);
            let err = parse_props(&body).expect_err("a missing receipt fails");
            assert!(err.to_string().contains(key), "{err}");
        }
        let mut body = full;
        body["default_generation_settings"] = json!({});
        assert!(parse_props(&body).is_err(), "a missing n_ctx fails");
    }

    #[test]
    fn a_server_serving_another_file_is_refused() {
        let dir = TempDir::new("served-artifact");
        let given = dir.path().join("given.gguf");
        let other = dir.path().join("other.gguf");
        std::fs::write(&given, b"a").unwrap();
        std::fs::write(&other, b"b").unwrap();

        verify_served_artifact(&given.display().to_string(), &given).expect("the same file passes");
        // A different spelling of the same file passes (canonicalized).
        let dotted = dir.path().join(".").join("given.gguf");
        verify_served_artifact(&dotted.display().to_string(), &given)
            .expect("an equivalent path passes");

        let err = verify_served_artifact(&other.display().to_string(), &given)
            .expect_err("another file is refused")
            .to_string();
        assert!(
            err.contains("other.gguf") && err.contains("given.gguf"),
            "{err}"
        );

        for router in ["none", ""] {
            let err = verify_served_artifact(router, &given)
                .expect_err("router mode is refused")
                .to_string();
            assert!(err.contains("router mode"), "{err}");
        }
    }
}
