# Seam Specifications — D1–D10

Each seam is a named, isolated change in zed-kask. Every seam has a **port contract** (the trait hKask defines + the adapter the bridge implements) and **acceptance criteria**. The governing invariant (§13.1): hKask crates never depend on zed-kask crates; `kask_bridge` is the sole bidirectional seam.

---

## D1 — Skill Execution

**Surface:** `crates/agent_skills` + `crates/agent/src/tools/skill_tool.rs`

**Change:** Replace `render_skill_envelope()` (injects `SKILL.md` body into model context) with a call to the compiled-in `ManifestExecutor` cascade (WordAct/FlowDef/KnowAct/RenderAct + PDCA convergence + gas/rjoule + OCAP gating). `SKILL.md` frontmatter stays the discovery-only catalog entry (name + description for zed-kask's 50KB catalog budget).

**Port contract:**
- The `ManifestExecutor` (in `hkask-templates`) takes `Arc<dyn InferencePort>` + `Arc<dyn ToolPort>`.
- The bridge (`kask_bridge`) provides both: `InferencePort` over zed-kask's `LanguageModel` (D4/D8), `ToolPort` over the in-process tool registry (D3/D8).
- zed-kask's `skill_tool.rs` calls `ManifestExecutor::execute_knowact()` or `execute_manifest()` via the bridge; the result is rendered back to `LanguageModelToolResultContent` (D12/R12).

**AC:**
- `/grill-me` in a zed-kask thread runs the KnowAct cascade, returns the assessment, <10s.
- `reg.skill.activate` + `reg.skill.*` spans present.
- `SKILL.md` frontmatter is the only thing in the catalog (body not injected).
- 50 skills fit the 50KB catalog budget (empirical).

**Dependencies:** D4 (InferencePort adapter), D3 (ToolPort adapter), D8 (bridge).

---

## D2 — Curator Agent

**Surface:** `crates/agent/src/agent.rs` + `native_agent_server.rs` + `crates/agent_servers`

**Change:** Register the Curator (VSM S4 singleton) as a native in-process zed-kask agent, selectable in the Agent Panel. The Curator stays in-process (`CuratorHandle` mpsc authority never crosses a process boundary). ACP variant optional (only for external-agent interop).

**Port contract:**
- `CuratorTurnPort` (NEW, in `hkask-types`): `async fn turn(&self, input: &str) -> Result<CuratorResponse>`.
- The bridge implements `CuratorTurnPort` by routing a zed-kask native-agent turn to the in-process `CuratorAgent` (tokio via `gpui_tokio`).
- zed-kask's `native_agent_server.rs` constructs a `NativeAgentServer` backed by `CuratorTurnPort` (not the default coding-agent thread).

**AC:**
- Curator is addressable as a selectable agent in the Agent Panel.
- Addressing the Curator reaches the in-process singleton (not a separate process).
- `CuratorHandle` mpsc directives still work in-process.
- `reg.meta.*` spans emitted on Curator metacognition.

**Dependencies:** D8 (bridge + gpui_tokio), the `KaskCore` composition root (§13.3).

---

## D3 — hKask Tools In-Process

**Surface:** `crates/context_server/src/client.rs` + `transport/`

**Change:** Add an in-process transport alongside `StdioTransport`; host the 12 default-load hKask MCP tools in-process. ⚠ R4: the MCP servers are refactored off `DaemonClient` (daemon Unix socket) to **direct in-process handles** — the daemon owned storage/regulation/memory; with no daemon, ownership moves to `KaskCore` (§13.3).

**Port contract:**
- `ToolPort` (in `hkask-capability`, OCAP+gas-gated): `async fn invoke(&self, tool: &str, args: &Value) -> Result<Value>`.
- The bridge implements `ToolPort` over the in-process MCP tool registry (the 12 servers, each taking `KaskCore` handles for storage/regulation/memory).
- zed-kask's context_server `client.rs` gains an `InProcessTransport` (alongside `StdioTransport`) that dispatches to the in-process tool registry instead of spawning a subprocess.

**AC:**
- A tool call from a zed-kask thread runs in-process (no subprocess).
- `reg.tool.*` span emitted per tool.
- `VarietyTracker` shows the 12 tool domains.
- OCAP: a tool call without a `DelegationToken` is denied; `reg.tool` block span emitted.
- No `DaemonClient` / daemon socket remaining.

**Dependencies:** T3.0 (DaemonClient→direct-handles refactor), D8 (bridge), `KaskCore` (§13.3).

---

## D4 — Guard Layer

**Surface:** `crates/language_model_core`/`language_model` (the streaming `LanguageModel` trait)

**Change:** `GuardedInferencePort` (from `hkask-guard`) wraps an `InferencePort`-over-`LanguageModel` adapter so `scan_input`/`scan_output` run on every inference call (direct chat + skill cascade + Curator). ⚠ R2: `GuardedInferencePort` is typed to hKask's non-streaming `InferencePort`, not zed-kask's streaming `LanguageModel` — cannot wrap directly. ⚠ R3: direct-chat streaming needs a buffer/incremental decision.

**Port contract:**
- `InferencePort` (in `hkask-types`): `fn generate(&self, prompt: &str, params: &LLMParameters, tools: Option<&[ChatToolDefinition]>) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>` (non-streaming, returns full `InferenceResult`).
- The bridge implements `InferencePort` over zed-kask's `LanguageModel` by collecting the stream into an `InferenceResult`.
- `GuardedInferencePort` wraps the `InferencePort` adapter: `scan_input` before, `scan_output` after.
- **OR** a zed-kask `LanguageModel` decorator calling `scan_input`/`scan_output` as pure functions (keeps hKask↛zed-kask). Decision: Open Q 15.

**AC:**
- A guarded inference call works in-process; `reg.inference` span emitted.
- All inference (direct chat + cascade + Curator) is guarded.
- The guard's `scan_input`/`scan_output` detect prompt-injection / sensitive-output (P3.1 floor).

**Dependencies:** D8 (bridge), `hkask-guard`.

---

## D5 — Sovereignty Keys

**Surface:** `crates/credentials_provider` / `zed_credentials_provider`

**Change:** hKask sovereignty keys (OCAP signing, DB passphrase, internal-secret derivation with key versioning) stored via the `SecretsPort` adapter over `CredentialsProvider` (kask namespace, e.g. `kask://credentials/ocap_signing`, `kask://credentials/db_passphrase`). Data-service API keys (EODHD, FMP) also via `SecretsPort` (D9b). The trimmed `hkask-keystore` becomes a thin crypto-derivation layer over the shared `CredentialsProvider`.

**Port contract:**
- `SecretsPort` (NEW, in `hkask-types`): `async fn read_secret(&self, key: &str) -> Result<Option<Vec<u8>>>; async fn write_secret(&self, key: &str, val: &[u8]) -> Result<()>; async fn delete_secret(&self, key: &str) -> Result<()>`.
- The bridge implements `SecretsPort` over zed-kask's `CredentialsProvider` (`read_credentials`/`write_credentials`/`delete_credentials`, keyed by URL `kask://credentials/<name>`).
- hKask crates use `SecretsPort` (NOT `CredentialsProvider` directly — keeps hKask↛zed-kask).

**AC:**
- Sovereignty keys (OCAP signing, DB passphrase) readable/writable via `SecretsPort`.
- Data-service keys (EODHD, FMP) readable via `SecretsPort`.
- Keys are in the OS keychain (not settings.json).
- The trimmed `hkask-keystore` compiles with only `SecretsPort` as its storage backend.

**Dependencies:** D8 (bridge), D9b (kask credentials namespace).

---

## D6 — Thread → Memory

**Surface:** `crates/agent/src/thread.rs` / `thread_store.rs`

**Change:** Hook thread completion → hKask memory ingestion (episodic + semantic h_mems). UserPod threads → UserPod episodic memory; Curator threads → Curator episodic + semantic publish (P11). Extends the existing ACP per-turn encoding (`hkask-acp/main_impl.rs` L348–380) to full-thread transcripts.

**Port contract:**
- `MemoryPort` (NEW, in `hkask-types`): `async fn ingest_thread(&self, pod: &PodId, thread: &ThreadTranscript) -> Result<()>; async fn recall_semantic(&self, query: &str) -> Result<Vec<HMem>>`.
- The bridge implements `MemoryPort` over in-process `EpisodicMemory`/`SemanticMemory` handles from `KaskCore`.
- zed-kask's `thread_store.rs` calls `MemoryPort::ingest_thread()` on thread completion.

**AC:**
- A closed zed-kask thread → episodic h_mems for the owning pod.
- Curator thread → `reg.semantic.published` + Curator `SemanticIndex` entry.
- Thread-watcher background task (F3, replaces 7R7) emits `reg.*` for the conversation surface.
- No `hkask-api`/daemon endpoint dependency (uses in-process `MemoryPort`).

**Dependencies:** D8 (bridge), `KaskCore` (§13.3), F3 (thread watcher).

---

## D7 — App-Identity Separation

**Surface:** `crates/paths/src/paths.rs`, `crates/release_channel/src/lib.rs`, `crates/zed/src/zed/mac_only_instance.rs`, `crates/zed/Cargo.toml`, `script/install.sh`/`uninstall.sh`/`bundle-linux`

**Change:** Rename the local footprint so zed-kask coexists with an upstream zed install (no collision). Keep shared `*.zed.dev` account/collab endpoints (user logs into their existing Zed account).

**Knobs:**
- `paths.rs`: `APP_NAME` → `"Zed-Kask"` (+ derived `APP_NAME_LOWERCASE` → `"zed-kask"`). Renames all config/data/state/cache/logs dirs.
- `release_channel.rs`: `app_identifier()` → `"Zed-Kask-Editor"`, `app_id()` → `"dev.zed-kask.Zed-Kask"`, `display_name()` Stable → `"Zed-Kask"`.
- `mac_only_instance.rs`: distinct port block (offset or `Kask` channel arm) + `instance_handshake()` → `"Zed-Kask …"` (C1: port is keyed on channel+uid, NOT on APP_NAME — must fix explicitly).
- `paths.rs` + `util/src/shell.rs`: `.zed_server`/`.zed_wsl_server` → `.zed-kask_server`/`.zed-kask_wsl_server` (C2: hardcoded remote dirs).
- `Cargo.toml`: `[[bin]] name = "zed"` → `"zed-kask"` (C3).
- `Cargo.toml` L281–305: macOS display names → `"Zed-Kask …"` (C4).
- `install.sh`/`uninstall.sh`/`bundle-linux`: `appid` → `dev.zed-kask.Zed-Kask`.

**AC (T-A8):** with both `zed` and `zed-kask` installed: both launch independently, separate settings, same Zed account login.

**Dependencies:** None (pure fork-renaming, no hKask dependency).

---

## D8 — Async-Runtime Bridge + Trait Adapters

**Surface:** `kask/crates/kask_bridge` + `gpui_tokio`

**Change:** The sole bidirectional seam. Bridges GPUI↔tokio via `gpui_tokio`; implements all ports (`InferencePort`, `ToolPort`, `SecretsPort`, `CuratorTurnPort`, `MemoryPort`) over zed-kask facilities.

**Contract:**
- `kask_bridge` depends on BOTH hKask port traits (from `hkask-types`/`hkask-capability`) AND zed-kask facilities (`gpui`, `gpui_tokio`, `language_model`, `context_server`, `credentials_provider`). It is the ONLY crate that depends on both sides.
- hKask crates NEVER depend on `kask_bridge` or any zed-kask crate. Enforced by `kask/scripts/check-hkask-no-zed-deps.sh`.
- The bridge provides a tokio runtime (via `gpui_tokio`) on which hKask's async tasks (Curator, regulation, ManifestExecutor, MCP servers) run.

**AC:**
- A hKask async task (e.g. `ManifestExecutor::execute_knowact`) runs on the bridged tokio runtime from a GPUI context.
- `InferencePort`-over-`LanguageModel` adapter: collect stream → `InferenceResult`; `GuardedInferencePort` wraps it.
- All 5 ports implemented and injectable into `ManifestExecutor`/Curator/MCP servers/kask panel.

**Dependencies:** `gpui_tokio` (zed), all hKask keep-crates (after T0.6 migration).

---

## D9 — Kask Settings + Credentials

**Surface:** new `KaskSettings` struct + `"kask": {...}` settings.json section + `CredentialsProvider` kask namespace + `crates/settings_ui/src/pages/kask_page.rs`

**Change:** kask-unique non-secret config (data-service toggles, MCP load set, curator, sovereignty, guard, memory) lives in a new `"kask"` settings section. Data-service API keys (EODHD, FMP) in the OS keychain via `SecretsPort` (D5/D9b). A new Kask page in the settings UI.

**KaskSettings fields:**
```json
{
  "kask": {
    "data_services": { "eodhd": { "enabled": true }, "fmp": { "enabled": true } },
    "mcp": { "load_default": ["memory","condenser","research","companies","media","docproc","training","replica","kata-kanban","codegraph","scenarios","regulation"] },
    "curator": { "always_on": false },
    "sovereignty": { "pod": {} },
    "guard": { "direct_chat": "buffer_threshold" },
    "memory": { "consolidation_every": 10 },
    "panel": { "dock": "right" }
  }
}
```

**AC:**
- `KaskSettings` registered with zed's settings system; appears in `zed://schemas/settings`.
- Kask page in settings UI; data-service key entry writes to keychain via `SecretsPort`.
- `KaskSettings` → `KaskCore` construction params (settings→config is construction-time).
- `kask import-config` migrates env `HKASK_*` → keychain + settings (T6.3).

**Dependencies:** D8 (bridge, SecretsPort), D5, zed's settings + settings_ui crates.

---

## D10 — Kask Panel

**Surface:** `kask/crates/kask_panel` (`impl Panel`)

**Change:** Native GPUI panel: catalog of the 12 MCP servers + per-server view (direct `:tool args` invocation + scoped inference). Reuses zed's `ui` components + theme tokens (`ui::prelude::*`). Copy-template: `agent_ui/src/agent_panel.rs`.

**Contract:**
- `KaskPanel` implements `Panel` (from `crates/workspace/src/dock.rs`): `persistent_name() → "KaskPanel"`, `toggle_action() → ToggleKaskPanel`, `position()`, `default_size()`, `icon()`, `Render`, `Focusable`, `EventEmitter<PanelEvent>`.
- Renders: `TabBar` of open servers (or `List` catalog when none) + active view (`Editor` input + results via `data_table`/`conversation_view`).
- Direct `:tool args` → `ToolPort` (OCAP-gated, `reg.tool.*` emitted). Scoped inference → `InferencePort` (guarded, `reg.inference` emitted).
- Dock position persists to `kask.panel.dock` (D9).

**AC:**
- Panel is dockable (right/bottom), toggleable via `ToggleKaskPanel` action.
- Selecting a server opens a per-server view with the server's tools.
- Direct `:tool args` invokes the tool in-process; `reg.tool.*` span + gas consumed.
- Scoped inference runs guarded; `reg.inference` span.
- Uses `ui::prelude::*` + `cx.theme()` (inherits zed's light/dark themes).

**Dependencies:** D8 (bridge — ToolPort + InferencePort), D3 (in-process tool registry), D9 (panel dock setting).

---

## T0.6-storage — hkask-storage rewrite (CRITICAL PATH)

**Blocked by:** the `libsqlite3-sys` conflict (zed pins 0.30.1 via `sqlez` + `sqlx-sqlite`; no `rusqlite` version is compatible).

**Change:** rewrite `hkask-storage` to use zed's `sqlez` (or `sqlx`) instead of `rusqlite`. This is the **conform-to-zed dependency policy** (DIVERGENCE.md): refactor hKask to use zed's stack, not the reverse.

**SQLCipher → application-layer encryption:** `sqlez` uses plain SQLite (no SQLCipher). hKask's per-pod encryption (P11.1) becomes an **application-layer encryption** (encrypt h_mems before storing, decrypt after reading) rather than SQLCipher at the SQLite level. The sovereign private sphere is preserved (data is encrypted at rest) without requiring SQLCipher in zed's `libsqlite3-sys` build.

**Scope:**
- Rewrite `agent_wallet_store.rs` to use `sqlez` instead of `rusqlite` + `define_driver_store!` macro.
- Rewrite `hkask-storage`'s `database::driver` / `database::sqlite` modules to use `sqlez` connections.
- The `WalletStore`, `RegulationArchive`, etc. — reimplement on `sqlez`.
- The regulation crate's wallet modules (`agent_wallet_store`, `wallet_manager`, `wallet_budget`, `well`, `wallet_gas_calibrator`, `wallet_energy_estimator`) then compile (they depend on the storage types).
- `cybernetics_loop` and `energy_budget_management` unblock (they use wallet types as struct fields).

**Unblocks:** `hkask-regulation` → `hkask-guard` → `hkask-templates` → `hkask-pods` → MCP servers.

**AC:**
- `cargo check -p hkask-storage` compiles with zed's `libsqlite3-sys 0.30.1` (no `links` conflict).
- `cargo check -p hkask-regulation` compiles (all wallet modules present).
- The §13.1 invariant holds (no hKask crate depends on a zed crate — `hkask-storage` uses `sqlez` which is a zed crate... wait, that's an inversion!).

**⚠ Dependency-direction note:** `sqlez` IS a zed crate. If `hkask-storage` depends on `sqlez`, that's hKask→zed-kask (inversion of §13.1). Resolution: define a `StoragePort` trait in `hkask-types`; the `kask_bridge` implements it over `sqlez`. `hkask-storage` depends on `StoragePort` (not `sqlez` directly). This keeps hKask↛zed-kask. BUT — `hkask-storage`'s CURRENT code uses `rusqlite` directly (concrete types, not a port). So the rewrite is actually a **port-ification**: `hkask-storage` moves from concrete `rusqlite` to a `StoragePort` trait (defined in `hkask-types`), and the `kask_bridge` provides the `sqlez` adapter. This is the ports-and-adapters pattern applied to the storage layer.

**Revised scope (port-ification):**
- Define `StoragePort` trait in `hkask-types` (the storage port — `query`, `execute`, etc.).
- Rewrite `hkask-storage` to use `StoragePort` instead of `rusqlite` directly.
- `kask_bridge` implements `StoragePort` over `sqlez`.
- The wallet modules use `StoragePort` (not concrete storage types).
- `hkask-regulation` depends on `hkask-storage` (which now depends only on `hkask-types::StoragePort`, not on `rusqlite` or `sqlez`).
