//! MCP server child-process env construction, allowlist filtering, and keychain sweep.

use super::catalog::find_server;
use gpui::AppContext;

/// Filter a list of `(env_var, credential_url)` pairs to only those the
/// specified server is allowed to receive — only env vars in the server's
/// credential allowlist are kept.
///
/// This limits the blast radius of a compromised MCP server — a server that
/// only needs `OPENROUTER_API_KEY` won't receive unrelated credentials.
#[must_use]
pub fn filter_credentials_for_server(
    server_id: &str,
    credentials: &[(String, String)],
) -> Vec<(String, String)> {
    let Some(server) = find_server(server_id) else {
        // Fail closed: an unknown server id receives no credentials.
        tracing::warn!(
            target: "reg.mcp",
            server_id = %server_id,
            "Unknown MCP server id — no credentials will be injected"
        );
        return Vec::new();
    };
    credentials
        .iter()
        .filter(|(env_var, _)| server.credentials.contains(&env_var.as_str()))
        .cloned()
        .collect()
}

/// Tracks `(server_id, env_var)` pairs already warned about being missing.
/// `build_mcp_server_env` is called on every `SettingsStore` observer fire
/// (for env-diffing, not just launching), so without deduplication a missing
/// credential spams the log every time any unrelated setting changes. A
/// (server, env var, read-state) triple is warned at most once per process
/// lifetime — the state in the key lets a CHANGED state re-warn (Missing
/// later flipping to ProviderError, or a wedge's Transient flipping to
/// Missing once the portal recovers but the key is truly absent, stays
/// visible) while the same state repeated stays quiet. Restarting Zed resets
/// the set.
static WARNED_MISSING_CREDENTIALS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashSet<(String, String, String)>>,
> = std::sync::OnceLock::new();

/// The single canonical env-construction path for a kask MCP server child
/// process.
///
/// There used to be two: `KaskMcpDescriptor::command` (zed's per-project
/// `ContextServerStore`) and `kask_server_env` (the governed `McpRuntime` +
/// the settings-change restart observer). They composed the same helpers in
/// different orders and diverged in opposite directions — Path A leaked the
/// full unfiltered `mcp_env()` map (the per-server `config_env` allowlist was
/// bypassed), Path B dropped every credential (the config filter ran on the
/// credential map too, and credentials live in `credentials`, not `config_env`).
/// Both bugs were invisible because the allowlist-alignment tests exercise the
/// filter helpers in isolation, never the composed path.
///
/// This function is the one place that composes the filters. The order is
/// load-bearing: config is filtered first, then credentials are resolved and
/// merged into the already-filtered map — the two filters apply to disjoint
/// key sets and never run in sequence on the same map. The inference socket is
/// injected last and is not in any allowlist (every server may route inference
/// through zed's `LanguageModelRegistry`).
///
/// `inference_socket` and `inference_timeout_secs` are parameters, not global
/// reads, so this function is unit-testable without touching
/// `INFERENCE_SOCKET_PATH`. The timeout publishes the server's
/// `stream_completion` establishment deadline to child-process IPC clients so
/// they can align their read deadline; see
/// `hkask_types::inference_ipc::INFERENCE_TIMEOUT_ENV`.
pub async fn build_mcp_server_env_with_sweep(
    server_id: &str,
    settings: &crate::KaskSettings,
    credentials_provider: &dyn credentials_provider::CredentialsProvider,
    inference_socket: Option<&str>,
    inference_timeout_secs: Option<u64>,
    sweep: &CredentialSweep,
    cx: &gpui::AsyncApp,
) -> hkask_types::ServerEnv {
    // 1. Config env: build, then filter per-server. `mcp_env()` is the full
    //    unfiltered map; the allowlist is what keeps unrelated config out.
    let mut env = filter_config_env_for_server(server_id, &settings.mcp_env());
    let catalog = match crate::database_inventory_path() {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(error) => {
            tracing::warn!(%error, "Database inventory route unavailable; child will refuse the empty route");
            String::new()
        }
    };
    env.insert(hkask_storage::DATABASE_CATALOG_ENV.into(), catalog);
    // Update authority before credential awaits: an older env-building future
    // must not re-grant access after a newer settings pass revoked it.
    let loaded =
        settings.mcp.load_default && *settings.mcp.overrides.get(server_id).unwrap_or(&true);
    let tools = settings
        .mcp
        .delegated_tools
        .get(server_id)
        .filter(|_| loaded)
        .cloned()
        .unwrap_or_default();
    if let Some(grant) = crate::delegation_grants::grant_for_server(server_id, &tools) {
        env.insert(
            hkask_types::inference_ipc::TOOL_GRANT_ENV.to_string(),
            grant,
        );
    }

    // 2. Credentials: resolve URLs, filter per-server, read from keychain.
    //    Shell overrides win (preserves the polarity the previous
    //    `mcp_env_with_credentials` established: an empty env var in the
    //    parent shell is not a meaningful override and would silently break
    //    inference with an untraceable "API key not configured" error).
    //
    //    The governed McpRuntime path (`start_server_with_env`) calls
    //    `cmd.env_clear()` before injecting `extra_env`, so a shell-set
    //    credential that is merely `continue`-d here is LOST — the child sees
    //    neither the shell value nor the keychain value. The prior `continue`
    //    was correct for the zed context-server path (which inherits the parent
    //    env), but the governed path does not inherit. Insert the parent value
    //    into `env` so it survives the `env_clear()`: the shell value still
    //    wins over the keychain (it is inserted first, and the keychain branch
    //    below only runs when the parent env did not provide a non-empty
    //    value), and the governed child receives it.
    let cred_urls = filter_credentials_for_server(server_id, &crate::credential_urls_for_mcp());
    for (env_var, url) in cred_urls {
        if let Ok(value) = std::env::var(&env_var)
            && !value.is_empty()
        {
            env.insert(env_var, value);
            continue;
        }
        let read = match sweep.cached(&url) {
            Some(read) => read,
            None => {
                // Hang-audit Pr3: one keychain read per distinct URL per
                // sweep — see CredentialSweep. The memoized outcome carries
                // the per-env-var trim: one URL resolves through one env
                // var, so the value is what every server sharing the URL
                // computes.
                let read = read_keychain_credential(server_id, &env_var, &url).await;
                sweep.record(&url, read.clone());
                read
            }
        };
        match read {
            KeychainCredentialRead::Loaded(value) => {
                env.insert(env_var, value);
                continue;
            }
            // The five distinguishable failure states fall through to the
            // per-state warnings below — never a single "not set or empty"
            // message, so the operator can tell "not configured" (Missing)
            // from "configured but broken" (ProviderError, InvalidUtf8,
            // Empty) from "temporarily unreachable" (Transient — the key
            // may exist; the portal is wedged). Each state carries its own
            // remediation.
            other => {
                // Dedup per (server, env var, STATE): `build_mcp_server_env`
                // runs on every SettingsStore observer fire, and a state
                // CHANGE must re-warn (a key fixed after an error flips the
                // message back through Empty→Loaded silently otherwise).
                // Same state twice stays quiet.
                let warned_set = WARNED_MISSING_CREDENTIALS
                    .get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));
                let is_new = warned_set
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert((
                        server_id.to_string(),
                        env_var.clone(),
                        warn_dedup_state_key(&other).to_string(),
                    ));
                if is_new {
                    let message = keychain_read_warning_message(server_id, &env_var, &url, &other);
                    tracing::warn!(
                        target: "reg.mcp",
                        server_id = %server_id,
                        env_var = %env_var,
                        "{message}"
                    );
                }
            }
        }
        // Startup-default passphrases: provision the required "allostery"
        // default before falling through to the missing-credential warning.
        // See `DEFAULT_PASSPHRASE_ENV_VARS` for why this tier exists at all.
        if let Some(passphrase) = provision_default_passphrase(&env_var, cx).await {
            // Mirror into zed's keychain so the primary ctx.credentials tier
            // (and the settings UI's `has_credential`) sees the provisioned
            // value on subsequent reads; the env insertion below serves this
            // launch directly even if the mirror write fails.
            //
            // The write is atomic (oo7 `create_item` with `replace=true`)
            // and only affects the passphrase entry — it cannot clobber
            // other credentials.
            if let Err(error) = credentials_provider
                .write_credentials(&url, "kask", passphrase.as_bytes(), cx)
                .await
            {
                tracing::warn!(
                    target: "reg.mcp",
                    server_id = %server_id,
                    env_var = %env_var,
                    %error,
                    "Failed to mirror provisioned passphrase to CredentialsProvider — \
                     the value is injected into this launch's env, but future reads \
                     will re-provision until the mirror succeeds"
                );
            }
            env.insert(env_var, passphrase);
            continue;
        }
        // Neither the parent shell, the keychain, nor a startup-default
        // passphrase tier provided a value. The per-state warning already
        // fired inside the keychain-read match above (Missing, Empty,
        // InvalidUtf8, ProviderError, Transient — each with its own
        // remediation); this
        // fall-through carries no separate message. The MCP server will
        // surface `permission_denied` when it tries to use the credential
        // (see `.rules` — "Missing credentials must surface as
        // `McpToolError::permission_denied`").
    }

    // 3. Inference IPC socket — not in any allowlist; every server may route
    //    inference through zed's LanguageModelRegistry over the IPC bridge.
    if let Some(socket) = inference_socket {
        env.insert(
            hkask_types::inference_ipc::INFERENCE_SOCKET_ENV.to_string(),
            socket.to_string(),
        );
    }

    // 4. Admission-to-completion timeout — publish the server's deadline so
    //    IPC clients can set their read timeout to `server_timeout + grace`
    //    instead of inventing an independent (shorter) one. Without this, a
    //    slow-but-alive provider produces a storm of `BrokenPipe` warnings: the
    //    client times out first, closes its socket, and the server's later
    //    response write hits EPIPE. With this, the client strictly outlasts the
    //    server, so a timed-out inference produces one timeout (the server's).
    if let Some(secs) = inference_timeout_secs {
        env.insert(
            hkask_types::inference_ipc::INFERENCE_TIMEOUT_ENV.to_string(),
            secs.to_string(),
        );
    }

    hkask_types::ServerEnv::from_canonical(env)
}

/// The canonical per-server env composer with a fresh
/// [`CredentialSweep`] per call: every credential is read from the keychain
/// individually. The launch and restart loops call the sweep-sharing form
/// ([`build_mcp_server_env_with_sweep`]) so a credential shared by several
/// servers costs one keychain read per pass; tests and single-call callers
/// use this form.
pub async fn build_mcp_server_env(
    server_id: &str,
    settings: &crate::KaskSettings,
    credentials_provider: &dyn credentials_provider::CredentialsProvider,
    inference_socket: Option<&str>,
    inference_timeout_secs: Option<u64>,
    cx: &gpui::AsyncApp,
) -> hkask_types::ServerEnv {
    build_mcp_server_env_with_sweep(
        server_id,
        settings,
        credentials_provider,
        inference_socket,
        inference_timeout_secs,
        &CredentialSweep::new(),
        cx,
    )
    .await
}

/// Env vars with a required startup-default passphrase.
///
/// The default ("allostery") is a stated first-run requirement: DB-backed
/// MCP servers must open their SQLCipher databases without login or manual
/// keychain configuration. The default used to be provisioned only in zed's
/// deferred task (`provision_agent` + keychain mirrors in
/// `main.rs`), but servers resolve their launch env before the deferred task runs —
/// on a machine that never signs in, the default never landed and every
/// DB-backed server failed with `permission_denied`. Provisioning at the
/// canonical env path guarantees the default regardless of launch ordering.
///
/// All other credentials have NO default — a miss keeps warning so "not
/// configured" stays visible to the operator.
pub(super) const DEFAULT_PASSPHRASE_ENV_VARS: [&str; 1] = ["HKASK_DB_PASSPHRASE"];

/// The outcome of reading one MCP-server credential from the keychain —
/// five failure states kept distinguishable (the `.ok()??` collapse this
/// replaces made them indistinguishable: a locked keychain, a missing
/// entry, an empty value, and non-UTF-8 bytes all read as "not set",
/// so the operator could not tell "not configured" from "configured but
/// broken" — the broken-feedback-loop class). Modeled on the distinguishable
/// reads in `inference_providers.rs` and `crates/language_model/src/api_key.rs`.
///
/// The states map one-to-one from the keystore's typed `KeychainError`
/// taxonomy via the read seam below the provider trait
/// (`typed_keychain_read`): `NotFound` → `Missing`, `Platform` →
/// `ProviderError`, `Timeout` → `Transient`. The provider trait's
/// `read_credentials` has no transient state — it collapses `Timeout` and
/// `Platform` to `Ok(None)` — which classified a timed-out read of an
/// EXISTING key as `Missing` and told the operator to set keys that exist
/// (observed live 2026-10-25 21:36 and 2026-10-26 11:07, the wedged secret
/// portal; card db0d8e88).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum KeychainCredentialRead {
    /// A non-empty value, trimmed for injection where trimming applies.
    Loaded(String),
    /// The keychain answered and holds no entry for this URL — the
    /// keystore's typed `NotFound`.
    Missing,
    /// The entry exists but its value is empty — including a value that is
    /// only whitespace, which previously survived the emptiness filter and
    /// was injected as an EMPTY env value after trimming.
    Empty,
    /// The entry exists but its bytes are not valid UTF-8.
    InvalidUtf8,
    /// The keychain failed at the platform layer (locked, unavailable,
    /// D-Bus/I/O) — carries the typed `Platform` error text. The key may
    /// exist; it is unreachable, not absent.
    ProviderError(String),
    /// The keychain read did not answer within its deadline — the secret
    /// portal may be wedged. The key may exist and be temporarily
    /// unreachable: this is NOT a missing entry. Carries the expired
    /// deadline from the keystore's typed `Timeout` error.
    Transient { timeout_secs: u64 },
}

/// One typed keychain read at the seam below the `CredentialsProvider`
/// trait.
///
/// The provider trait's `read_credentials` has no transient state: it
/// collapses `Timeout` and `Platform` failures to `Ok(None)`, so a wedged
/// secret portal arrived at the classifier as "absent" — a timed-out read
/// of an EXISTING key classified as `Missing` and the operator was told to
/// set a key that exists (observed live 2026-10-25 21:36 and 2026-10-26
/// 11:07; card db0d8e88). This seam reads the keystore directly — the same
/// deadline-bounded read the process-global provider performs (`new` is
/// pinned to the keychain provider) — preserving the typed
/// `KeychainError` taxonomy the trait collapses.
///
/// The payload is raw bytes, not `String`, so a corrupt entry stays
/// classifiable: the current keystore returns (lossily decoded) UTF-8
/// strings, so `InvalidUtf8` cannot arise from this path today — the
/// classifier stays total over what a keychain entry can hold.
///
/// One keychain read per distinct credential URL per sweep (hang-audit Pr3).
///
/// The launch loop and the settings-change restart observer each recompute
/// EVERY server's env in one pass, so a credential shared by several servers
/// — `HKASK_DB_PASSPHRASE` alone is on seven allowlists — was read from the
/// OS keychain once per server per sweep. Under the wedged-portal class each
/// read burns its full 5s deadline (`KeychainError::Timeout` — the audit's
/// 179-timeout hour), multiplying a wedge by the server count.
///
/// Sweep-scoped by construction: the caller creates one `CredentialSweep`
/// per launch or restart pass, so `nudge_mcp_servers`'s freshness contract
/// holds — a keychain write nudges the observer, the NEXT sweep re-reads
/// the fresh value, and the restarted server carries it. Never
/// process-global: a cached outcome never outlives the pass that read it.
///
/// The memo is keyed by URL and stores the classified
/// [`KeychainCredentialRead`]: one keychain URL resolves through exactly one
/// env var (the fixed `credential_urls_for_mcp` table), and the injection
/// trim is env-var-based, so a cached `Loaded` value is the value every
/// server sharing that URL computes.
#[derive(Default)]
pub struct CredentialSweep {
    reads: std::sync::Mutex<std::collections::HashMap<String, KeychainCredentialRead>>,
}

impl CredentialSweep {
    /// Create the per-pass memo. Construct one at the top of each launch
    /// or restart sweep and share it across every
    /// `build_mcp_server_env_with_sweep` call in that pass.
    pub fn new() -> Self {
        Self::default()
    }

    fn cached(&self, url: &str) -> Option<KeychainCredentialRead> {
        self.reads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(url)
            .cloned()
    }

    fn record(&self, url: &str, read: KeychainCredentialRead) {
        self.reads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(url.to_string(), read);
    }
}

/// Test seam: tests install canned typed outcomes via `MockKeychain` in
/// `crate::test_support`; without an installed mock, the real keystore is
/// read.
type TypedKeychainRead = Result<Vec<u8>, hkask_keystore::KeychainError>;

async fn typed_keychain_read(url: &str) -> TypedKeychainRead {
    #[cfg(test)]
    {
        // A mock installed by `MockKeychain::install` serves every URL —
        // an entry with no canned outcome reads as typed `NotFound`, like
        // a keychain that answered and holds no entry. No mock installed →
        // the real keystore below.
        if let Some(outcome) = crate::test_support::injected_keychain_read(url) {
            return outcome;
        }
    }
    match hkask_keystore::Keychain.retrieve_by_url_async(url).await {
        Ok(secret) => Ok(secret.to_vec()),
        Err(error) => Err(error),
    }
}

/// Read one MCP-server credential from the keychain, classified over the
/// keystore's typed error taxonomy — every read state the operator can be
/// warned about maps to exactly one honest remediation.
pub(super) async fn read_keychain_credential(
    server_id: &str,
    env_var: &str,
    url: &str,
) -> KeychainCredentialRead {
    let password = match typed_keychain_read(url).await {
        Ok(bytes) => bytes,
        // Typed `NotFound`: the keychain answered and holds no entry at
        // this URL — the honest "not configured" state.
        Err(hkask_keystore::KeychainError::NotFound(_)) => {
            return KeychainCredentialRead::Missing;
        }
        // Typed `Platform`: the keychain answered with a failure (D-Bus,
        // locked keyring, I/O). The key may exist; it is unreachable.
        Err(hkask_keystore::KeychainError::Platform(error)) => {
            return KeychainCredentialRead::ProviderError(error);
        }
        // Typed `Timeout`: the read did not answer within its deadline —
        // the wedged-portal class. The key may exist and be temporarily
        // unreachable: NOT Missing, never "has no keychain entry".
        Err(hkask_keystore::KeychainError::Timeout { timeout_secs, .. }) => {
            return KeychainCredentialRead::Transient { timeout_secs };
        }
    };
    let value = match String::from_utf8(password) {
        Ok(value) => value,
        Err(_) => return KeychainCredentialRead::InvalidUtf8,
    };
    // The emptiness check is PRE-trim, deliberately: the DB passphrase is
    // exempt from trimming (its exact bytes key the databases) — a
    // post-trim check would classify a whitespace-only passphrase as
    // Loaded, injecting an empty env value. A value that is empty or
    // whitespace-only at the source is empty, whichever credential it is.
    if value.trim().is_empty() {
        return KeychainCredentialRead::Empty;
    }
    KeychainCredentialRead::Loaded(trim_credential_for_injection(server_id, env_var, value))
}

/// The per-state remediation message for a failed MCP-server credential
/// read — extracted from the warn site so the message contract itself is
/// pinnable: a timed-out read of an EXISTING key must never be told "has
/// no keychain entry" (the dishonest remediation the provider-trait
/// collapse shipped). One honest message per state.
pub(super) fn keychain_read_warning_message(
    server_id: &str,
    env_var: &str,
    url: &str,
    read: &KeychainCredentialRead,
) -> String {
    match read {
        KeychainCredentialRead::Missing => format!(
            "Credential {env_var} for server {server_id} has no keychain entry \
             at {url} — the server will fail with permission_denied when it \
             tries to use it. Set the key via the kask settings UI to fix this."
        ),
        KeychainCredentialRead::Empty => format!(
            "Credential {env_var} for server {server_id} is set but empty \
             (or whitespace-only) at {url} — the server will fail with \
             permission_denied when it tries to use it. Re-set a non-empty \
             key via the kask settings UI."
        ),
        KeychainCredentialRead::InvalidUtf8 => format!(
            "Credential {env_var} for server {server_id} at {url} holds \
             non-UTF-8 bytes — the server will fail with permission_denied \
             when it tries to use it. The keychain entry is corrupt; \
             re-enter the key via the kask settings UI."
        ),
        KeychainCredentialRead::ProviderError(error) => format!(
            "Reading credential {env_var} for server {server_id} from \
             {url} failed: {error} — the keychain may be locked or \
             unavailable. The server will fail with permission_denied when \
             it tries to use the credential; retry after unlocking the \
             keychain."
        ),
        KeychainCredentialRead::Transient { timeout_secs } => format!(
            "Reading credential {env_var} for server {server_id} from {url} \
             timed out after {timeout_secs}s — the secret portal may be \
             wedged, so the key may exist but is temporarily unreachable: \
             this is not a missing keychain entry. The server will fail \
             with permission_denied when it tries to use the credential; \
             retry after the keychain recovers, or set the credential via \
             its env var to bypass the keychain."
        ),
        KeychainCredentialRead::Loaded(_) => {
            unreachable!("Loaded reads inject the credential; they never reach the warn site")
        }
    }
}

/// The dedup key for a failed-read state — one warn per (server, env var,
/// state) per process, so a CHANGED state re-warns (a wedge flipping to
/// Missing once the portal recovers but the key is truly absent stays
/// visible) while the same state repeated stays quiet.
pub(super) fn warn_dedup_state_key(read: &KeychainCredentialRead) -> &'static str {
    match read {
        KeychainCredentialRead::Missing => "missing",
        KeychainCredentialRead::Empty => "empty",
        KeychainCredentialRead::InvalidUtf8 => "invalid-utf8",
        KeychainCredentialRead::ProviderError(_) => "provider-error",
        KeychainCredentialRead::Transient { .. } => "transient",
        KeychainCredentialRead::Loaded(_) => {
            unreachable!("Loaded reads inject the credential; they never reach the warn site")
        }
    }
}

/// Trim surrounding whitespace from a keychain-read credential before env
/// injection. Keychain values are pasted by hand; a trailing newline from a
/// copy-paste is not part of any credential, and it breaks HTTP header
/// construction downstream (observed live 2026-08-30: a 36-byte Firecrawl
/// key whose last byte was `\n`, failing every request from the long-lived
/// server instance with "failed to parse header value" while fresh spawns
/// — whose `env -i $(...)` harness shell-word-split the newline away —
/// succeeded). The DB passphrase is exempt: its exact bytes key the
/// SQLCipher databases, so trimming it would make every DB unopenable.
pub(super) fn trim_credential_for_injection(
    server_id: &str,
    env_var: &str,
    value: String,
) -> String {
    if DEFAULT_PASSPHRASE_ENV_VARS.contains(&env_var) {
        return value;
    }
    let trimmed = value.trim().to_string();
    if trimmed != value {
        tracing::warn!(
            target: "reg.mcp",
            server_id = %server_id,
            env_var = %env_var,
            "Keychain credential has surrounding whitespace — trimmed at injection. \
             Re-set the key via the kask settings UI to fix it at the source."
        );
    }
    trimmed
}

/// Provision the startup-default passphrase for `env_var`, if it has one.
///
/// Runs the keystore chain (env override → existing keychain entry → default
/// "allostery" stored on first run) on the background executor — keyring I/O
/// is synchronous D-Bus and must not block the foreground thread. Returns the
/// resolved passphrase, or `None` when `env_var` has no startup default or
/// provisioning failed (the caller then falls through to the
/// missing-credential warning, which names the env var).
pub(super) async fn provision_default_passphrase(
    env_var: &str,
    cx: &gpui::AsyncApp,
) -> Option<String> {
    if !DEFAULT_PASSPHRASE_ENV_VARS.contains(&env_var) {
        return None;
    }
    let env_var = env_var.to_string();
    let env_var_for_log = env_var.clone();
    let provisioned = cx
        .background_spawn(async move { crate::identity::provision_db_passphrase() })
        .await;
    match provisioned {
        Ok(passphrase) if !passphrase.is_empty() => Some(passphrase),
        Ok(_) => None,
        Err(error) => {
            tracing::warn!(
                target: "reg.mcp",
                env_var = %env_var_for_log,
                %error,
                "Failed to provision the default passphrase — the server will \
                 fail with permission_denied until a passphrase is provisioned \
                 (sign in, or set the env var / keychain entry)"
            );
            None
        }
    }
}

/// Filter a base config env map (`mcp_env()` output) to only the env vars the
/// specified server is allowed to receive — only env vars in the server's
/// config allowlist are kept.
///
/// This prevents a server's config from being injected into servers that don't
/// allowlist it.
#[must_use]
pub(crate) fn filter_config_env_for_server(
    server_id: &str,
    config_env: &std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, String> {
    let Some(server) = find_server(server_id) else {
        // Fail closed: an unknown server id receives no config env.
        tracing::warn!(
            target: "reg.mcp",
            server_id = %server_id,
            "Unknown MCP server id — no config env will be injected"
        );
        return std::collections::HashMap::new();
    };
    config_env
        .iter()
        .filter(|(env_var, _)| server.config_env.contains(&env_var.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}
