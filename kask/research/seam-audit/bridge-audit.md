# Bridge Audit — `kask_bridge` dead-surface + deepening (2026-08-11)

> Method: `refactor-architecture` + `essentialist` over the bridge's three
> largest files (`memory.rs` 2,942; `settings.rs` 1,949; `inference_ipc_server.rs`
> 1,588 — 53% of the crate's 12,229 lines). Grep-verified caller counts; the
> essentialist deletion test; deepening assessment. Read-only — no edits.

## Headline

The bridge is in **good shape**. For 12,229 lines it has remarkably little dead
surface: 5 dead symbols (~40 lines) + 1 orphaned re-export + 1 visibility nit.
The known `.rules` traps are already honored (tokio-handle + channel boundary,
`model_constants` reference, settings `Default`/`From` discipline, ordinal-keyed
`extract_final_step_result`). The one **live bug** is not dead surface — it's
an advertised-invariant-without-enforcement-point: the data-service enable
toggles are inert.

## Findings

### BD-01 — Data-service enable toggles are inert (HIGH, live bug)
- **file:line**: `kask/crates/kask_bridge/src/inference_providers.rs:376-380`
- **Evidence**: `credential_urls_for_mcp` injects data-service secrets
  **unconditionally** — `for desc in DATA_SERVICES { if desc.is_secret() { urls.push(...) } }`
  — while inference-provider secrets are gated on their toggle at L384-395
  (`if enabled { urls.push(...) }`, reading `settings.inference_providers.<p>_enabled`).
  A workspace grep finds **no** `*_ENABLED` env var emitted by `mcp_env` and
  **no** MCP server reading one. So `eodhd_enabled=false` (etc.) in settings
  still results in `HKASK_EODHD_API_KEY` being injected into MCP server children
  when the keychain entry exists. The settings UI (`settings_ui/.../data_services.rs`)
  and `docs/reference/kask-settings.md` both advertise the toggles as gating
  injection. The L371 comment rationalizes unconditional injection ("no harm in
  listing all secrets") — a non-sequitur w.r.t. the toggle's disable purpose.
- **Trap**: `.rules` "Advertised invariants need enforcement points" — the
  toggle advertises enable/disable; there is no enforcement point.
- **Remediation**: gate data-service injection on the toggle, mirroring the
  inference-provider pattern: `if settings.data_services.<svc>_enabled && desc.is_secret()`.
  **Behavioral change → pin with a test**: disabled service's credential is
  NOT injected; enabled service's IS.

### BD-02 — `KaskModelsSettings` dead cluster (low, dead surface)
- **file:line**: `kask/crates/kask_bridge/src/settings.rs:640,646,662,672`
- **Evidence**: `effective_embedding_model` (L662), `effective_classifier_model`
  (L672), `DEFAULT_EMBEDDING_MODEL` const (L640), `DEFAULT_CLASSIFIER_MODEL`
  const (L646) — zero callers. They mirror the live inference-model pair
  (`effective_default_model` + `DEFAULT_INFERENCE_MODEL`, used in
  `crates/zed/src/main.rs:1884,2931`) but were never wired; the composition root
  emits the raw `embedding_model`/`classifier_model` fields as env vars. The
  two consts' sole reader is the two dead methods, so all four die together.
  The canonical constants live in `hkask_inference::model_constants` (referenced
  directly elsewhere) — these re-exports are the "advertised convenience no
  caller uses" trap.
- **Deletion test**: complexity_vanishes. **Remediation**: delete all four.

### BD-03 — `RealMemoryPort::from_env` dead (low, dead surface)
- **file:line**: `kask/crates/kask_bridge/src/memory.rs:245-282` (~40 lines)
- **Evidence**: zero callers (production AND test); not in the `pub use memory::{...}`
  re-export. Superseded by `identity.rs::provision_agent → RealMemoryPort::new`
  (`identity.rs:186` doc comment explicitly says "construct a RealMemoryPort
  directly, WITHOUT going through from_env()").
- **Deletion test**: complexity_vanishes. **Remediation**: delete.

### BD-04 — `memory.rs` partial split (medium, deepening)
- **file:line**: `kask/crates/kask_bridge/src/memory.rs` (2,942 lines)
- **Assessment**: the memory-port core (`RealMemoryPort` + `MemoryPort` impl +
  recall helpers + consolidation timer + `BridgeMemoryPort` adapter) is **deep
  and cohesive** — shares private state (store, embedding_port, tokio_handle,
  curator_store); splitting would re-introduce coupling. KEEP. But two blocks
  are **shallow grouping** (grouped by storage location, not responsibility):
  - **`alert_escalation` (L695-792, ~98 lines)** — clearest. A *different* port
    (`AlertEscalationSink`) with **zero coupling** to the memory port. Extract
    `alert_escalation.rs`.
  - **`curator_stores` (L793-993, ~220 lines)** — moderate. `CuratorStore` +
    `open_curator_store` + `curator_db_path` + `open_curator_regulation_archive`;
    one-way dependency (memory port → curator_stores, never reverse). Extract
    `curator_stores.rs`. Caveat: `curator_db_path` is shared by alert-escalation,
    so if both extracted it needs a shared home.
- **Deletion test**: core → complexity_reappears (keep); the two blocks →
  complexity_vanishes (extract). Net: memory.rs 2,942 → ~2,524 if both extracted.

### BD-05 — `shared_worktree_spawner` over-visible (low, visibility)
- **file:line**: `kask/crates/kask_bridge/src/inference_ipc_server.rs:93`; re-export `kask_bridge.rs:43`
- **Evidence**: the function is live (sole internal caller at L386) but `pub` +
  re-exported in `kask_bridge.rs:43` with **no external caller** (the siblings
  `set_worktree_spawner` and `WorktreeSpawner` ARE used externally in `main.rs`).
- **Remediation**: narrow to `pub(crate)`; drop from the `pub use` block.

## Cleared (evidence-backed, no finding)
- **`inference_ipc_server.rs`**: 0 dead dispatch arms (all 10 `InferenceMethod`
  variants have verified live MCP-server senders via `InferenceIpcClient`).
  `WorktreeSpawner` is a justified dependency-inversion seam (impl in a different
  crate; used via `dyn` at L392) — not trait-with-one-impl theater. The
  `own_uid` `unwrap_or(0)` (L188) is a uid fallback for socket-dir naming
  (documented), not a regulation-loop sense input. Cohesive — keep.
- **`settings.rs`**: all 20 sub-structs are live (UI or env-var consumers); the
  `Default`/`From` discipline is clean. The `KaskDataServiceSettings` struct
  itself is live (UI consumes it) — only its toggles' *enforcement* is missing
  (BD-01), not the struct.
- **`memory.rs`**: all 3 traits (`MemoryPort`, `AlertEscalationSink`,
  `agent::ThreadMemoryPort`) consumed via `dyn` — no trait-with-one-impl. The
  `unwrap_or(false)` at L452 is correct first-run boolean semantics, not a
  measurement-defaulted-to-zero. All 5 `pub use memory::{...}` re-exports have
  external callers in `main.rs`.

## Recommended action order
1. **BD-01** — the live bug. Highest value (user-facing: the operator's toggle
   does nothing). Behavioral change → needs a test + compile verification.
2. **BD-02 + BD-03 + BD-05** — the dead-surface/visibility cluster. Zero
   callers (verified) → no behavioral change, but needs compile verification.
   Quick codegraph wins; also removes two "advertised convenience" traps.
3. **BD-04** — the `memory.rs` partial split. Deepening, not dead surface.
   Lowest risk is the `alert_escalation.rs` extraction (zero coupling); the
   `curator_stores.rs` extraction is medium-risk (shared `curator_db_path`).
   Optional — the core is cohesive and earns its size.