//! The canonical serving route (P4) — the DME probe leg's ONE route.
//!
//! The L4 supervisor launches the canonical llama.cpp build per the L1
//! manifest (fleet resolution → GGUF parse → hardware probe → the
//! guard-verified LaunchPlan → startup probe → spawn → health-wait),
//! and the completions ride the M3 `openai_compatible` semantics
//! directly against `LaunchPlan::endpoint()`: `/apply-template` renders
//! the GGUF's own chat template with `chat_template_kwargs`
//! (`enable_thinking` — the DME-0 mechanism), `/completion` runs the
//! pinned sampling contract. The request shape is the same semantics
//! `kask_bridge/src/inference_providers.rs` routes for Zed; the bridge
//! crate itself is gpui-coupled and is NOT linked here.
//!
//! Sampling parity (the P4 gate): every knob — temperature, top_p,
//! top_k, min_p, presence_penalty, repeat_penalty, seed, n_predict,
//! repeat_last_n — is verified PER ROW against the server's
//! `generation_settings` echo. A missing or mismatched knob is a typed
//! failure naming it, never an approximation.

use std::io::Read as _;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use hkask_private_inference::local_leg::placement::{GpuBackend, PlacementPolicy, probe_backend};
use hkask_private_inference::local_leg::supervisor::{LaunchConfig, Supervisor, prepare_launch};
use hkask_types::agent_paths::{resolve_under_artifacts_dir, resolve_under_data_dir};
use serde::Serialize;
use serde_json::{Value, json};

/// A running canonical server: the endpoint, the route client, and the
/// runtime that keeps the supervisor's watch task alive. Dropping the
/// struct drops the runtime (the watch task dies with it); call
/// [`Serving::stop`] for the ordered drain (SIGTERM → wait → SIGKILL).
pub struct Serving {
    pub model_id: String,
    pub endpoint: String,
    pub base: String,
    /// The serving engine's `--version` line — the pin receipt.
    pub engine_version: String,
    /// The rendered LaunchPlan — the exact argv the supervisor spawned.
    pub launch_plan: String,
    runtime: tokio::runtime::Runtime,
    supervisor: Supervisor,
    token: String,
    client: reqwest::blocking::Client,
}

/// The pinned sampling contract for one completion (the M3 route
/// semantics; the field names are the server's own).
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

impl Serving {
    /// Launch a fleet model through the supervisor (the canonical
    /// route — no hand-spawned servers, no ad-hoc flags: the plan is
    /// compiled from the L1 manifest, the L3 placement and the policy).
    pub fn launch(model_id: &str, policy: PlacementPolicy) -> Result<Self> {
        let backend = probe_backend();
        let binary = match backend {
            GpuBackend::Hip { binary } | GpuBackend::Vulkan { binary } => binary,
            GpuBackend::None => bail!(
                "no canonical llama.cpp build under the engine root — build it via \
                 kask/scripts/build-llamacpp-mainline.sh (the L4 supervisor refuses \
                 to launch anything else)"
            ),
        };
        let token = mint_token()?;
        let config = LaunchConfig {
            model_id: model_id.to_string(),
            binary: binary.clone(),
            api_key: Some(token.clone()),
            // The harness's own machine-state subtree — separate from the
            // bridge's (different launch records, different policy port:
            // never the PI page's slot).
            state_dir: resolve_under_data_dir(Path::new("private-inference/dme-harness")),
            models_root: resolve_under_artifacts_dir(Path::new("private-inference/models")),
            policy,
        };
        let prepared = prepare_launch(&config)
            .with_context(|| format!("prepare the canonical launch of `{model_id}`"))?;
        let launch_plan = prepared.plan.to_string();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("build the serving runtime")?;
        let supervisor = Supervisor::default();
        // prepare + spawn + health-wait (bounded by the measured 20-minute
        // load bound inside the supervisor).
        let snapshot = runtime
            .block_on(supervisor.launch_prepared(config, &prepared))
            .with_context(|| format!("launch `{model_id}` on the canonical route"))?;
        let engine_version = probe_version_line(&binary)?;
        let endpoint = snapshot.endpoint.clone();
        let base = endpoint
            .strip_suffix("/v1")
            .map(str::to_string)
            .unwrap_or_else(|| endpoint.clone());
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5400))
            .build()
            .context("build the route client")?;
        Ok(Self {
            model_id: snapshot.model_id,
            endpoint,
            base,
            engine_version,
            launch_plan,
            runtime,
            supervisor,
            token,
            client,
        })
    }

    /// The ordered drain: SIGTERM → wait → SIGKILL (the supervisor's
    /// stop path — no orphan servers after a leg).
    pub fn stop(self) -> Result<()> {
        let supervisor = self.supervisor;
        self.runtime
            .block_on(supervisor.stop())
            .context("stop the canonical server")
    }

    fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let url = format!("{}{path}", self.base);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .with_context(|| format!("POST {url}"))?;
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

/// Verify every sampling knob against the server's echo — the P4 gate,
/// applied per row. A missing knob is named as missing; a mismatched
/// knob is named with both values. Never an approximation.
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

/// The engine's `--version` first line (the pin receipt on every row).
fn probe_version_line(binary: &Path) -> Result<String> {
    let out = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .with_context(|| format!("probe {binary:?} --version"))?;
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    text.lines()
        .find(|l| l.trim().starts_with("version:"))
        .map(|l| l.trim().to_string())
        .context("the engine binary prints a version line")
}

/// Mint an endpoint token from the OS entropy pool — the supervisor's
/// discipline: the token reaches the server from a 0600 file, never
/// argv, and the requests carry it as the Bearer.
fn mint_token() -> Result<String> {
    let mut f = std::fs::File::open("/dev/urandom").context("open /dev/urandom")?;
    let mut bytes = [0u8; 32];
    f.read_exact(&mut bytes)
        .context("read 32 bytes of entropy")?;
    let mut s = String::with_capacity(64);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    Ok(s)
}
