//! CLI-only native HTTP acquisition. No inference router, model fallback, or tools.
//! A current-thread runtime is owned here, never borrowed from GPUI.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::*;

const TIMEOUT: Duration = Duration::from_secs(60);
const MODEL: &str = "openai/gpt-5.4-mini-2026-03-17";
// Same native URL as open_ai::OPEN_AI_API_URL; do not pull editor crates into this CLI.
const OPENAI_API_URL: &str = "https://api.openai.com/v1";
const KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(5);
// Study A model-profile-v1.1.0.json, generation_cost_proposal: published Standard
// rates from https://developers.openai.com/api/docs/models/gpt-5.4-mini.md.
// Study-specific admission floors, not general inference defaults or a bill guarantee.
const STUDY_INPUT_FLOOR_MICROUSD_PER_MILLION: u64 = 750_000;
const STUDY_OUTPUT_FLOOR_MICROUSD_PER_MILLION: u64 = 4_500_000;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CountTariff {
    pub(super) source: String,
    pub(super) recorded_at: String,
    pub(super) basis: TariffBasis,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum TariffBasis {
    SuppliedTariff,
    MockOnly,
}

/// This is supplied tariff provenance, not authentication of a vendor promise.
fn validate_policy(protocol: &Protocol, mock: bool) -> Result<(), String> {
    protocol.validate()?;
    if protocol.limits.count_request_max_fee_microusd.is_none() {
        return Err("unknown count-request maximum fee; no HTTP admission".into());
    }
    let tariff = protocol
        .count_tariff
        .as_ref()
        .ok_or("count tariff provenance required")?;
    if tariff.source.trim().is_empty() || tariff.recorded_at.trim().is_empty() {
        return Err("count tariff source and recorded_at required".into());
    }
    if !mock && tariff.basis != TariffBasis::SuppliedTariff {
        return Err("mock-only tariff cannot authorize provider HTTP".into());
    }
    let limits = &protocol.limits;
    if protocol.model != MODEL
        || limits.max_attempts > 170
        || limits.spending_ceiling_microusd > 10_000_000
        || limits.max_input_tokens > 8192
        || limits.max_output_tokens > 1024
        || limits.provider_output_max != 128_000
        || limits.prompt_microusd_per_million_tokens < STUDY_INPUT_FLOOR_MICROUSD_PER_MILLION
        || limits.completion_microusd_per_million_tokens < STUDY_OUTPUT_FLOOR_MICROUSD_PER_MILLION
    {
        return Err("live study requires pinned snapshot, <=170 attempts each, <=10USD all-in, <=8192/1024 tokens and provider maximum 128000 with generation tariffs >=750000/4500000 microUSD per million input/output tokens".into());
    }
    Ok(())
}

fn authorization_from_key(key: &str, source: &str) -> Result<reqwest::header::HeaderValue, String> {
    // RFC 6750 b64token syntax, including optional trailing padding. A configured
    // empty, whitespace-padded, non-ASCII or malformed value fails without fallback.
    let token = key.trim_end_matches('=');
    if token.is_empty()
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._~+/".contains(&byte))
    {
        return Err(format!(
            "{source} is empty or malformed; expected an ASCII bearer token"
        ));
    }
    let mut value = reqwest::header::HeaderValue::from_str(&format!("Bearer {key}"))
        .map_err(|_| format!("{source} is not a valid bearer token"))?;
    value.set_sensitive(true);
    Ok(value)
}

/// Pure injected lookup seam: no process environment or OS keychain access here.
/// Policy must reject unknown financial liability before either loader is called.
fn resolve_authorization<S: AsRef<str>>(
    protocol: &Protocol,
    environment: impl FnOnce() -> Result<String, std::env::VarError>,
    keychain: impl FnOnce(&str) -> Result<S, hkask_keystore::KeychainError>,
) -> Result<reqwest::header::HeaderValue, String> {
    validate_policy(protocol, false)?;
    match environment() {
        Ok(key) => authorization_from_key(&key, "OPENAI_API_KEY"),
        Err(std::env::VarError::NotUnicode(_)) => Err("OPENAI_API_KEY is not Unicode/ASCII; configured environment values never fall back to native provider Settings".into()),
        Err(std::env::VarError::NotPresent) => {
            let key = keychain(OPENAI_API_URL).map_err(|error| match error {
                // Error payloads may contain secrets or platform-controlled text.
                // Classify only; never Display/Debug them into an operator report.
                hkask_keystore::KeychainError::NotFound(_) => "Native OpenAI credential not found; set OPENAI_API_KEY or configure native OpenAI provider Settings",
                hkask_keystore::KeychainError::Platform(_) => "Native OpenAI keychain is inaccessible (platform failure), not a missing credential; set OPENAI_API_KEY or check native OpenAI provider Settings",
                hkask_keystore::KeychainError::Timeout { .. } => "Native OpenAI keychain read timed out; keychain is inaccessible, not a missing credential; set OPENAI_API_KEY or check native OpenAI provider Settings",
            }.to_string())?;
            authorization_from_key(key.as_ref(), "native OpenAI provider Settings credential")
        }
    }
}

/// The existing synchronous URL helper is unbounded. Outlive it on a CLI-only
/// worker after five seconds; a wedged worker may linger, as in the keystore's
/// bounded async read. No GPUI context is captured and no credential is written.
fn bounded_keychain_lookup(
    url: &str,
) -> Result<impl AsRef<str> + use<>, hkask_keystore::KeychainError> {
    let url = url.to_owned();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("packet-study-native-keychain".into())
        .spawn(move || {
            let result = hkask_keystore::Keychain.retrieve_by_url(&url);
            if let Err(expired_receiver) = sender.send(result) {
                // The caller already surfaced timeout/disconnection. Drop the
                // zeroizing secret instead of logging its undeliverable payload.
                drop(expired_receiver);
            }
        })
        .map_err(|_| {
            hkask_keystore::KeychainError::Platform("Cannot start native keychain worker".into())
        })?;
    match receiver.recv_timeout(KEYCHAIN_TIMEOUT) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            Err(hkask_keystore::KeychainError::Timeout {
                key: OPENAI_API_URL.into(),
                timeout_secs: KEYCHAIN_TIMEOUT.as_secs(),
            })
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(
            hkask_keystore::KeychainError::Platform("Native keychain worker stopped".into()),
        ),
    }
}

pub(super) struct Dispatcher {
    runtime: tokio::runtime::Runtime,
    client: reqwest::Client,
    authorization: reqwest::header::HeaderValue,
    base: String,
    pub(super) http_attempts: u64,
    mock: bool,
}

impl Dispatcher {
    /// Called only by the standalone CLI after protocol/case validation. Neither
    /// preparation nor replay resolves credentials. Only an absent environment
    /// value falls back to the existing canonical native provider URL.
    pub(super) fn from_environment(protocol: &Protocol) -> Result<Self, String> {
        let authorization = resolve_authorization(
            protocol,
            || std::env::var("OPENAI_API_KEY"),
            bounded_keychain_lookup,
        )?;
        Self::with_authorization(protocol, authorization, None, TIMEOUT)
    }

    #[cfg(test)]
    fn new(
        protocol: &Protocol,
        key: Option<&str>,
        mock_base: Option<String>,
        timeout: Duration,
    ) -> Result<Self, String> {
        validate_policy(protocol, mock_base.is_some())?;
        let key = key
            .filter(|key| !key.trim().is_empty())
            .ok_or("OPENAI_API_KEY required for native packet-study execute")?;
        let authorization = authorization_from_key(key, "test credential")?;
        Self::with_authorization(protocol, authorization, mock_base, timeout)
    }

    fn with_authorization(
        protocol: &Protocol,
        authorization: reqwest::header::HeaderValue,
        mock_base: Option<String>,
        timeout: Duration,
    ) -> Result<Self, String> {
        validate_policy(protocol, mock_base.is_some())?;
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(
                "packet-study execute requires the standalone CLI, outside an existing runtime"
                    .into(),
            );
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "cannot create packet-study CLI runtime")?;
        let client = {
            let _entered = runtime.enter();
            reqwest::Client::builder()
                .redirect_policy(reqwest::redirect::Policy::none())
                .referer(false)
                .no_proxy()
                // Reqwest's automatic HTTP/2 stream retry is not a study attempt.
                // Disable HTTP/2, idle reuse, and replayable request bodies.
                .http1_only()
                .pool_max_idle_per_host(0)
                .timeout(timeout)
                .connect_timeout(timeout)
                .no_gzip()
                .no_brotli()
                .no_deflate()
                .no_zstd()
                .build()
                .map_err(|_| "cannot create native HTTP client")?
        };
        let mock = mock_base.is_some();
        Ok(Self {
            runtime,
            client,
            authorization,
            base: mock_base.unwrap_or_else(|| OPENAI_API_URL.into()),
            http_attempts: 0,
            mock,
        })
    }

    pub(super) fn mode(&self) -> &'static str {
        if self.mock {
            "local_mock_http"
        } else if self.http_attempts > 0 {
            "live_packet_study"
        } else {
            "live_not_dispatched"
        }
    }

    fn acquire(
        &mut self,
        output: &Path,
        id: &str,
        endpoint: &str,
        body: &Value,
    ) -> Result<(HttpReplay, Result<Value, String>), String> {
        let suffix = endpoint
            .strip_prefix("https://api.openai.com/v1")
            .filter(|suffix| matches!(*suffix, "/responses" | "/responses/input_tokens"))
            .ok_or("unsupported native endpoint; no fallback")?;
        let bytes = serde_json::to_vec(body).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err("HTTP request exceeds byte limit".into());
        }
        let request_bytes_sha256 = digest(&bytes);
        let length = bytes.len();
        let body =
            reqwest::Body::wrap_stream(futures_util::stream::once(std::future::ready(Ok::<
                _,
                std::io::Error,
            >(
                bytes::Bytes::from(bytes),
            ))));
        let request = self
            .client
            .post(format!("{}{suffix}", self.base))
            .header(reqwest::header::AUTHORIZATION, self.authorization.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::CONTENT_LENGTH, length)
            .body(body)
            .build()
            .map_err(|_| "cannot build native request")?;
        let started_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "system clock predates epoch")?
            .as_millis();
        self.http_attempts += 1;
        let (status, headers, bytes, complete, error) = self.runtime.block_on(async {
            let mut bytes = Vec::new();
            let mut headers = BTreeMap::new();
            let mut response = match self.client.execute(request).await {
                Ok(response) => response,
                Err(error) => return (0, headers, bytes, false, Some(transport_error(&error))),
            };
            let status = response.status().as_u16();
            // Allowlist response metadata; never persist request auth or arbitrary headers.
            for (name, value) in response.headers() {
                let name = name.as_str();
                if matches!(
                    name,
                    "x-request-id"
                        | "request-id"
                        | "date"
                        | "content-type"
                        | "retry-after"
                        | "openai-processing-ms"
                ) || name.starts_with("x-ratelimit-")
                {
                    headers.insert(
                        name.to_owned(),
                        String::from_utf8_lossy(value.as_bytes()).into_owned(),
                    );
                }
            }
            loop {
                match response.chunk().await {
                    Ok(Some(chunk)) => {
                        let remaining = MAX_RESPONSE_BYTES - bytes.len();
                        bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
                        if chunk.len() > remaining {
                            return (
                                status,
                                headers,
                                bytes,
                                false,
                                Some("response exceeds 1MiB; retained bounded prefix".into()),
                            );
                        }
                    }
                    Ok(None) => return (status, headers, bytes, true, None),
                    Err(error) => {
                        return (status, headers, bytes, false, Some(transport_error(&error)));
                    }
                }
            }
        });
        let response_file = format!("{id}.response.bin");
        write_bytes(&output.join(&response_file), &bytes)?;
        let acquired = HttpReplay {
            http_status: status,
            body_complete: complete,
            response_file,
            response_sha256: digest(&bytes),
            transport_error: error,
        };
        write_json(
            &output.join(format!("{id}.receipt.json")),
            &json!({
                "mode":self.mode(), "http_status":status, "response_headers":headers,
                "started_unix_ms":started_unix_ms, "retained_bytes":bytes.len(),
                "request_bytes_sha256":request_bytes_sha256,
                "response_sha256":acquired.response_sha256, "body_complete":complete,
                "transport_error":acquired.transport_error,
                "response_id":serde_json::from_slice::<Value>(&bytes).ok().and_then(|v| v.get("id").cloned()),
            }),
        )?;
        let parsed =
            if !complete || acquired.transport_error.is_some() || !(200..300).contains(&status) {
                Err(format!(
                    "native HTTP failed/incomplete (status {status}); provisions retained"
                ))
            } else {
                serde_json::from_slice(&bytes)
                    .map_err(|e| format!("malformed native response: {e}"))
            };
        Ok((acquired, parsed))
    }
}

fn transport_error(error: &reqwest::Error) -> String {
    // Reqwest error Display is intentionally not persisted: no URL, credential,
    // proxy diagnostic, or provider-controlled request echo can leak through it.
    if error.is_timeout() {
        "native HTTP timeout"
    } else if error.is_connect() {
        "native HTTP connection failure"
    } else if error.is_body() || error.is_decode() {
        "native HTTP body failure"
    } else {
        "native HTTP transport failure"
    }
    .into()
}

pub(super) enum Source<'a> {
    Replay {
        records: &'a BTreeMap<String, ReplayRecord>,
        directory: &'a Path,
    },
    Http(Box<Dispatcher>),
}

impl Source<'_> {
    pub(super) fn mode(&self) -> &'static str {
        match self {
            Self::Replay { .. } => "instrument_verification_replay",
            Self::Http(dispatcher) => dispatcher.mode(),
        }
    }

    pub(super) fn check_binding(
        &self,
        request: &Prepared,
        frozen: &CountRequest,
    ) -> Result<(), String> {
        if let Self::Replay { records, .. } = self {
            let record = records
                .get(&request.request_sha256)
                .ok_or("request-digest replay mismatch; no live fallback")?;
            let count = record
                .count
                .as_ref()
                .ok_or("missing native count replay; no local token estimate")?;
            if count.generation_body_sha256 != frozen.generation_body_sha256
                || count.count_request_sha256 != frozen.count_request_sha256
            {
                return Err("count/generation payload binding mismatch".into());
            }
        }
        Ok(())
    }

    pub(super) fn acquire(
        &mut self,
        output: &Path,
        request: &Prepared,
        frozen: Option<&CountRequest>,
    ) -> Result<(HttpReplay, Result<Value, String>), String> {
        let id = if frozen.is_some() {
            format!("{}.count", request.id)
        } else {
            request.id.clone()
        };
        match self {
            Self::Replay { records, directory } => {
                let record = records
                    .get(&request.request_sha256)
                    .ok_or("missing replay; no live fallback")?;
                let response = if frozen.is_some() {
                    &record
                        .count
                        .as_ref()
                        .ok_or("missing native count replay")?
                        .response
                } else {
                    &record.response
                };
                acquire_http(directory, output, &id, response)
            }
            Self::Http(dispatcher) => {
                let (endpoint, body) = frozen
                    .map_or((request.endpoint.as_str(), &request.body), |count| {
                        (count.endpoint, &count.body)
                    });
                dispatcher.acquire(output, &id, endpoint, body)
            }
        }
    }
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
