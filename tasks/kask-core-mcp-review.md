# zed-kask findings review — kask core, MCP servers, and seams

**Date:** 2026-09-18 · **Mode:** findings-only, read-only (no edits to source, no commits) ·
**Compatibility:** none required — pre-release; breaking-change proposals allowed.
**Judge question:** does this code, as built, produce elegant, efficient, precise, functional
behavior — with *elegant* anchored to the project's own principles
(P5 essentialism/deletion test, honest feedback loops, D-seam discipline), not private taste.

---

## 1. Evaluation frame

### 1.1 Architecture (the fork)

- Minimal-divergence fork of Zed (`DIVERGENCE.md`, release 0.40.0 on upstream 1.21.0).
  Everything under `kask/` is kask-owned and additive; the only other divergences are
  the named D-seams (D1–D66) plus the workspace-member arrays.
- **Governing invariant (§13.1):** hKask crates never depend on zed-kask crates;
  zed-kask depends on hKask. The sole bidirectional seam is `kask_bridge` (D8).
  Enforced by `kask/scripts/check-hkask-no-zed-deps.sh`.
- 12 MCP servers run as child processes over stdio under a governed `McpRuntime`
  (single spawn authority, D3); the agent's tool surface routes through the
  process-global `KaskToolSource`.
- Keychain is the single source of truth for API keys (D5/D9); one passphrase
  (`HKASK_DB_PASSPHRASE`) for every SQLCipher DB, resolved via the canonical
  2-tier helper `hkask_mcp_server::server::credentials::resolve_db_passphrase`.
- Standardized artifact storage (D28): one rooted data tree with class subdirs.

### 1.2 Human-in-the-loop philosophy (what the system is for)

- The user is the product manager (keeper of functional requirements, judge of what
  the work is for); the agent is the technical program manager
  (`kask/docs/architecture/functional-interaction-spec.md`, D40).
- The Curator is the in-process cybernetic regulator: algedonic alerts, escalations,
  calibration, operator advice review (P9). Regulation must be an honest feedback
  loop — every deviation observable, every degradation surfaced.
- Magna Carta sovereignty (P1–P4), SQLCipher file as private-sphere boundary (P11.1),
  P12 authenticated host mandate with *surfaced* fallbacks.

### 1.3 Quality principles (the judge's yardstick)

- **P5 Essentialism** — remove before adding; deletion test; 5W1H gate.
- **P7 Evolutionary Architecture** — no speculative abstraction ("reserved for
  future" surface is the anti-pattern).
- **P8 Semantic Grounding** — P8.3 fallback ladder; absence must never be recorded
  as zero.
- **P9 Homeostatic Self-Regulation** — errors surfaced, never swallowed;
  degradations labeled, never reported as success; "not configured" must be
  distinguishable from "configured but broken."
- **`.rules` operational discipline** — no `unwrap()` on fallible paths; never
  discard errors with `let _ =`; advertised invariants must point to their
  enforcement line; stale comments are active misinformation; deletions clean up
  what they orphan; MCP servers are leaf crates.

### 1.4 Frame verification (`.rules`/docs claims checked against the tree)

- `script/clippy` machete scope is `kask/`-only — **verified** (`script/clippy:56-59`).
- Canonical passphrase helper adopted — **verified** (no server inlines
  `HKASK_DB_PASSPHRASE` reads; all route through `credentials.rs`).
- Doc drift confirmed at grounding (see F3): `hkask-ledger` in DIVERGENCE's member
  list is deleted; server-count and D-range claims are stale in several docs.

---

## 2. Crate inventory and lens assignments

Process lenses (lead): **kata-improvement** (outer loop), **metacognition**
(per-phase predictions, §5), **falsifiability** (every finding below carries a
discriminating check), **grill-me** (adversarial verification — see §5.3),
**refactor-architecture** (cross-crate synthesis from the sweeps below).

Per-crate lenses: **deep-module**, **code-review**, **bug-hunt**, **lean-prover**.
Coverage method — every deep-scope crate received: (a) the full pattern-sweep set
(production `unwrap()`/`expect()` censuses with `#[cfg(test)]` filtering, `let _ =`
census, silent-`.ok()` sweep, `block_on`/tokio-trap scan, `panic!` scan,
`unwrap_or(0)` sense-input scan, `allow(dead_code)`/`mod.rs` census, envelope-pattern
and passphrase-resolution greps, env-read census per server), plus (b) targeted
deep reads where the crate is seam-central or the sweep flagged it. Crates whose
coverage was sweeps + partial reads are marked **bounded** — the mission's
bounded-pass allowance, recorded per crate.

### kask/crates/ (core)

| Crate | LOC | Coverage | Notes |
|---|---|---|---|
| `hkask-types` | 7,399 | sweeps + targeted reads | `InferenceUsage` (F10), `tool_schema` dead-code allow = justified test fixture on the recorded baseline; `voice.rs` alive (media users); ports clean |
| `kask_bridge` | 19,342 | deep (mcp_servers.rs, settings.rs, mcp_env.rs, memory/*, alert_escalation.rs, ingest.rs) | F3, F5, F8, F9 citations; env construction exemplary (mcp_servers.rs:11-22, 646-664, 698-821) |
| `hkask-tool-port` | 147 | sweeps + dependent check | clean; 4 dependents earn it |
| `hkask-keystore` | 777 | sweeps + targeted reads | keychain docs honest (async-std backend note); passphrase chain documented "not a security boundary" |
| `hkask-regulation` | 12,765 | deep (cybernetics_loop.rs, cycle.rs head, system_simulator.rs, doc-invariant sample) | F7; production cycle exemplary (absence-vs-zero modeling, `AlertQueueOutcome` honesty, no-sink warns); `block_on` sites all test-module |
| `hkask-forecast` | 1,566 | sweeps + dependents | 4 server dependents; alive |
| `hkask-lisp` | 2,028 | sweeps + dependents | agent/companies/corpus dependents; no production unwrap hits |
| `hkask-memory` | 5,288 | sweeps + targeted reads (`MemoryStore::open`, memory_store.rs:153-168) | F1 chain confirmed here; embed/h_mem split clean |
| `hkask-mcp` | 3,649 | deep (runtime.rs:1-420 — config, healing docs, PASSTHROUGH_ENV, governance) | F3 (runtime doc defects), F6; SPAWN_RUNTIME reactor-hop exemplary (runtime.rs:107-129) |
| `hkask-mcp-server` | 2,144 | deep (credentials.rs full, transport.rs P12) | canonical chain exemplary (credentials.rs:61-104, parse_env_warn:122-144); P12 fallback surfaced with warns |
| `hkask-event-store` | 921 | sweeps + dependents | 3 dependents (bridge/swarm/training); alive |
| `hkask-storage` | 9,565 | deep (sqlite.rs:1-140, core/connection.rs PRAGMA path) | driver exemplary (WAL ordering invariant :14-25, labeled pools, `with_durability` honesty); SQLCipher probe-before-pool documented (connection.rs:426-441) |
| `hkask-inference` | 6,316 | targeted reads (hkask_inference.rs wire-parse, openai_compat.rs, ipc_client.rs) | F10 citations; `openai_compat.rs:85` clean (Option::max); probe-connect `let _` intentional (ipc_client.rs:327-336) |
| `hkask-bridge-ontology` | 3,526 | sweeps + panic-site check | `panic!` sites all `#[cfg(test)]` fixture loads; ladder vocabulary intact |
| `hkask-condenser` | 2,283 | sweeps + dependents | bridge-dependent (BridgeThreadCondenser); alive |
| `hkask-services-core` | 792 | sweeps + doc read | F3 (stale "CLI, API, REPL" consumers); live consumer = corpus direct-launch settings |
| `hkask-email` | 328 | sweeps + dependents | bridge+zed dependents (email sink); alive |
| `hkask-steer-core` | 242 | sweeps + read | tool-advertisement truth; clean |
| `hkask-spreadsheet` | 1,795 | sweeps + expect census | expect sites test-module; engine alive (portfolio + spreadsheet servers) |

### kask/mcp-servers/ (servers)

| Crate | LOC | Coverage | Notes |
|---|---|---|---|
| `hkask-mcp-companies` | 25,509 | sweeps + targeted reads (screener.rs, research.rs, fibo_cache.rs, acquisition_tests exclusion) | F4 (screener regexes); research.rs statics use expect-on-compile (acceptable); `.ok()` sites = external-data tolerance |
| `hkask-mcp-corpus` | 25,032 | deep (helpers.rs, index.rs, tools/corpus.rs, storage.rs, compose_tools.rs, semantic.rs, calibration.rs envelope/passphrase chain) | F1, F2, F3, F4 citations; `write_contained` single enforcement point exemplary (helpers.rs:186-192) |
| `hkask-mcp-media` | 25,712 | sweeps + targeted reads (jobs.rs, hkask_mcp_media.rs structure, faces.rs) | job-store strict contract deliberate + pinned (jobs.rs:28-46); production envelope use = strict decode, not duplication |
| `hkask-mcp-swarm` | 23,359 | sweeps + targeted reads (cloud_swarm_tools.rs, a2a_http.rs, local_tools.rs, grounding.rs) | F13; F5 (no HKASK_SKILLS_DIR readers); `.ok()` sites = best-effort response parsing |
| `hkask-mcp-research` | 14,827 | sweeps + targeted reads (db.rs, providers.rs, synthetic.rs, hkask_mcp_research.rs:1335) | F11; F12; `pick_best_provider` guarded (providers.rs:874-881) — not a finding; db.rs `.ok()`s legitimate optional lookups |
| `hkask-mcp-prediction-markets` | 10,458 | sweeps + targeted reads (streaming.rs, hkask_mcp_prediction_markets.rs, fetch_contracts.rs) | F12; envelope extractor test-only; HTTP timeouts pinned below the 60s MCP cap (test-documented) |
| `hkask-mcp-scenarios` | 6,403 | sweeps (bounded) | no production unwrap/expect/let-_/trap hits beyond sweeps |
| `hkask-mcp-training` | 9,303 | sweeps + targeted reads (submit.rs:111 guard-invariant, env census) | guard-implied expect documented; env reads align with allowlist (mcp_servers.rs pins) |
| `hkask-mcp-curator` | 8,144 | sweeps + targeted reads (distillation.rs expect sites, tool_behavior references) | expect sites test-module or epoch invariants; distillation config env allowlist documents live-observed gap fix |
| `hkask-mcp-kata-kanban` | 8,758 | sweeps + targeted reads (service.rs let-_= sites) | trait-param silencing at service.rs:352/569/939 (interface mismatch, minor — observation); idempotency design documented + pinned (D3) |
| `hkask-mcp-portfolio` | 6,440 | sweeps + targeted read (server.rs:175) | schema-validity expect at startup, by construction; provider-agnostic allowlist pinned |
| `hkask-mcp-spreadsheet` | 625 | sweeps (bounded) | minimal mutation-owner server; 0 env reads, allowlist = artifacts dir only |

### zed-side kask-owned (bounded scope — deletion test + seam health + trap sweeps)

| Crate | LOC | Coverage | Notes |
|---|---|---|---|
| `hkask-tool-invoker` | 352 | full read | exemplary leaf crate — `InvokeError` retry taxonomy replaces string-matching |
| `hkask-conversation-injector` | 258 | full read | exemplary — per-app `Global` (not process-global) with leak rationale |
| `hkask-steer` | 579 | read + sweeps | Steer lifecycle; clean |
| `hkask-viz-core` | 660 | sweeps (bounded) | registry composition; no traps |
| `hkask-media-widget` | 6,243 | sweeps + cfg(test) verification | block_on sites all test-module (verified :1305+) |
| `hkask-graph-widget` | 2,732 | sweeps (bounded) | no trap hits |
| `hkask-kanban-widget` | 3,147 | sweeps (bounded) | no trap hits |
| `hkask-portfolio-widget` | 2,199 | sweeps (bounded) | clean |
| `hkask-scenarios-widget` | 1,698 | sweeps (bounded) | clean |
| `hkask-spreadsheet-widget` | 1,659 | sweeps (bounded) | clean |
| `hkask-swarm-widget` | 519 | sweeps (bounded) | clean |
| `hkask-media-benchmarks` | 317 | sweeps (bounded) | isolated bench package (D18) |
| `swarm_panel` | 10,145 | sweeps + spot checks | no production block_on/trap hits |
| `kanban_panel` | 3,922 | sweeps + targeted read (board_picker.rs:120-155) | production `foreground.block_on(match_strings)` at board_picker.rs:145 — deliberate, documented ("board list is small… same approach as ThreadPicker"), bounded work → observation, not finding |
| `portfolio_panel` | 992 | sweeps (bounded) | clean |
| `marketplace_ui_common` | 220 | sweeps (bounded) | shared chrome; survives D30 by documented reuse |

### Seam audit results

1. **Tool-contract envelopes:** CLEAN — production uses shared
   `hkask_types::tool_response::unwrap_tool_envelope`; per-server extractors
   (corpus `unwrap_content`, media `content_of`, pm `unwrap_content`) are
   test-module helpers.
2. **Credential/passphrase resolution:** canonical chain exemplary
   (`credentials.rs:61-104`, typed `permission_denied`, warn-on-miss);
   exceptions are F1 (corpus gap) and F9 (NEBIUS classification).
3. **Settings-to-server sync:** allowlists are rationale-documented and
   pin-tested per server (`mcp_servers.rs:1037-1100` — allowlist-vs-reads
   assertions); exception is F5 (dead entry).
4. **Shared DB patterns:** driver exemplary (PRAGMA ordering invariant, labeled
   pools, durability honesty, probe-before-pool); the gap is corpus-side (F1).
5. **GPUI/background-runtime boundary:** production clean everywhere reviewed;
   `block_on`/`tokio::time` traps confined to `#[cfg(test)]` (verified per file);
   the one production `block_on` (board_picker.rs:145) is documented + bounded.
6. **Tool-advertisement truth:** bounded pass — `hkask-steer-core` (render/verify
   + generated `TOOL_NAMES`, D2) read; prompt-token tests documented in DIVERGENCE.
7. **Inference IPC lifecycle:** socket re-set + dual re-sync verified in code
   (`mcp_servers.rs:797-818`); `SPAWN_RUNTIME` reactor-hop for off-runtime
   reconnect documented with the live incident (runtime.rs:107-129).

---

## 3. Prioritized findings

Ranked by severity ÷ effort (highest value first). Every claim is IS (verified in
tree this session); directions are proposals, not patches.

### F1 — Corpus SQLCipher passphrase: empty-string fallback validated on only one of ~11 consumer paths · severity medium-high · effort S

- **file:line:** `kask/mcp-servers/hkask-mcp-corpus/src/helpers.rs:69-85`
  (`default_corpus_passphrase()` — `resolve_credential(...).ok().unwrap_or_default()`,
  doc delegates the invariant: "callers must surface permission_denied"); the gate
  exists only at `src/index.rs:271-276` (`hydrate_if_empty` checks
  `passphrase.is_empty()` → `permission_denied`); direct consumers without a check:
  `src/tools/corpus.rs:323` (`open_memory_store(&req.db_path, &req.passphrase)?`)
  plus serde-default sites `tools/corpus.rs:599,629,718`, `tools/storage.rs:568`,
  `tools/semantic.rs:588,622`, `tools/compose_tools.rs:129,146,164`,
  `tools/calibration.rs:54`; the open chain adds no check:
  `helpers.rs:174-180` → `kask/crates/hkask-memory/src/memory_store.rs:153-159`
  → `kask/crates/hkask-storage/src/core/connection.rs:430-431`
  (`PRAGMA key = '{escaped}'`).
- **Claim (IS, falsifiable):** when `HKASK_DB_PASSPHRASE` resolution fails,
  `default_corpus_passphrase()` returns `""` (helpers.rs:84) and every consumer
  except `hydrate_if_empty` passes `""` straight into the SQLCipher key PRAGMA —
  so a direct-launched server (no env injection, no keychain entry) attempts an
  open with an empty key instead of returning `permission_denied`. The
  "callers must surface permission_denied" invariant is enforced nowhere central.
- **Principle:** P9 (a resolution failure silently becomes an open attempt —
  "not configured" must not look like a normal call); lean-prover (comment-delegated
  invariant, no enforcement line).
- **Check:** unset `HKASK_DB_PASSPHRASE` (and clear the keychain tier), direct-launch
  the corpus server, and call any serde-default consumer (e.g. `corpus_embed` with a
  fresh `db_path`): the claim predicts the call reaches the SQLCipher open with
  `PRAGMA key = ''`; the honest behavior is `permission_denied` before any open.
- **Direction:** enforce the empty check once — in `open_memory_store` or
  `default_corpus_passphrase` (return `Result`, map empty → `permission_denied`) —
  and delete the per-caller comment invariant.
- **Counterfactual:** if any layer validated empty centrally, this finding is false —
  verified none does (greps + reads above).

### F2 — Credential field model-exposed across ~10 corpus tool schemas · severity medium · effort M

- **file:line:** `kask/mcp-servers/hkask-mcp-corpus/src/tools/corpus.rs:599-600,629-630,718`,
  `tools/storage.rs:567-568`, `tools/semantic.rs:587-588,621-622`,
  `tools/compose_tools.rs:128-129,145-146,163-164`, `tools/calibration.rs:53-54`
  — `#[serde(default = "...passphrase")] pub passphrase: String` on
  `JsonSchema`-deriving request structs. Live evidence: this session's own
  `corpus_query` tool schema exposes `passphrase` to the model.
- **Claim (IS, falsifiable):** the DB passphrase is a model-settable parameter on
  ~10 model-facing schemas although the model never legitimately supplies it (the
  serde default resolves server-side when omitted). A model-supplied value on a
  fresh `db_path` creates a DB encrypted under a key the operator never set; the
  field also puts credential-shaped surface into every prompt's schema tokens.
- **Principle:** P5 (interface minimalism — expose what the caller may set);
  P4 (credential authority belongs to the server's resolution chain, not the caller).
- **Check:** call `corpus_embed` with `passphrase: "model-chosen"` and a fresh
  `db_path` — a DB opens under that key; then grep the server's emitted
  `input_schema` JSON for the `passphrase` property (present today).
- **Direction:** remove the field from model-facing schemas entirely; resolve
  server-side only (what the default already does when omitted). Breaking change —
  allowed.

### F3 — Systemic doc drift on load-bearing surfaces · severity medium · effort S–M

- **file:line (all verified against the tree):**
  - `DIVERGENCE.md:239` lists deleted `hkask-ledger` as a workspace member
    (deleted 2026-09-08; `zed-host-architecture-plan.md:88`; absent from tree and
    root manifest).
  - `kask/docs/README.md:13` "11 managed MCP servers" and `:15`/`:137` "D1–D56" —
    the tree has **12** servers (`hkask-mcp-spreadsheet` added) and DIVERGENCE runs
    to **D66**; same stale count in `kask/docs/reference/mcp-servers/README.md:40`
    ("11 built-in servers") and `kask/docs/architecture/core/MDS.md` (via README:32,
    "18 library/composition crates, and 11 MCP servers" — actual: 19 + 12).
  - `DIVERGENCE.md:254` — upstream-sync runbook says "D1–D58" while `:125` says
    "D1–D66".
  - `kask/crates/hkask-mcp/src/runtime.rs:102` — mid-sentence fragment
    ("…(observed live 2026-08-29). up. Reset to zero on the first healthy connection
    seen."); `:144-148` — doc comment for the **removed** degraded-interval
    mechanism (per D3), now mis-attached above `resolve_duration_env_secs`;
    `:358-359` — contract reference to deleted `crates/hkask-cli/src/repl/builtin_servers.rs`.
  - `kask/crates/kask_bridge/src/mcp_servers.rs:119-121` — stale comment: "the DB is
    silently encrypted with the hardcoded dev passphrase" (current code returns `""`
    + warn — see F1).
  - `kask/crates/hkask-services-core/src/standalone_settings.rs:2-4` — names deleted
    consumers ("CLI, API, REPL"); the live consumer is MCP servers under direct launch.
- **Claim (IS, falsifiable):** at least six load-bearing docs/comments contradict
  the tree. Per `.rules` ("stale comments are active misinformation" — agents follow
  comments over code), these are the exact surfaces agents and upstream-rebasers
  consult first.
- **Principle:** `.rules` stale-comment rule; doc-truth; D-seam bookkeeping.
- **Check:** each citation is directly contradicted by the tree (count server dirs =
  12; `grep hkask-ledger Cargo.toml` → empty; `ls crates/hkask-cli` → absent;
  `grep DEGRADED kask/crates/hkask-mcp/src/runtime.rs` → only an unrelated hit).

### F4 — Regexes recompiled per call on corpus/companies hot paths · severity medium · effort S

- **file:line:** `kask/mcp-servers/hkask-mcp-corpus/src/convert.rs:226`
  (`unescape_html` — `Regex::new(r"&#(\d+);")` per call), `:254`
  (`strip_html_comments` per call), `:267-274` (`sanitize_links` builds **four**
  regexes per call — runs pre-chunking on every document page);
  `kask/mcp-servers/hkask-mcp-companies/src/screener.rs:597,627,759,834`
  (`Regex::new(&pattern)` per keyword per prompt).
  `grep -c 'LazyLock\|once_cell'` = 0 in both files.
- **Claim (IS, falsifiable):** these functions compile regexes on every invocation;
  a large `corpus_convert` recompiles four patterns per page — pure CPU waste on
  the documented hot path.
- **Principle:** efficiency (the judge question's "efficient"); P5 (a `LazyLock`
  static is the standard deep fix; companies' `research.rs` already uses the
  compile-once pattern — `research.rs:595` "static numeric-extraction pattern
  compiles").
- **Check:** instrument regex compilation (or time a 1,000-page convert) before/after
  wrapping `sanitize_links`' four patterns in `LazyLock`.

### F5 — Dead operator knob: `kask.swarm.skills_dir` → `HKASK_SKILLS_DIR` · severity medium-low · effort S

- **file:line:** `kask/crates/kask_bridge/src/settings.rs:448` (`pub skills_dir:
  String` on `KaskSwarmSettings`), `:513`, `:983`; `src/mcp_env.rs:333-334`
  (emits `HKASK_SKILLS_DIR`); `src/mcp_servers.rs:389-393` (allowlist entry whose
  own comment says "the swarm server no longer reads this env var").
- **Claim (IS, falsifiable):** the setting flows settings → `mcp_env` → allowlist →
  **nothing** — zero readers in `kask/mcp-servers/hkask-mcp-swarm/src` (grep
  verified). An operator who sets it gets silent no-effect — precisely the
  "not configured vs configured-but-broken" indistinguishability `.rules` forbids.
- **Principle:** P5 (delete dead surface — the entry's own comment admits it);
  P9 (silent no-op knob).
- **Check:** `grep -rn 'HKASK_SKILLS_DIR' kask/mcp-servers/hkask-mcp-swarm/src` →
  zero hits, while the setting round-trips (`mcp_env.rs:926` sets it in tests).

### F6 — `startup_timeout` default borrows the health-check constant · severity medium-low · effort S

- **file:line:** `kask/crates/hkask-mcp/src/runtime.rs:229-232` —
  `resolve_duration_env_secs("HKASK_MCP_STARTUP_TIMEOUT_SECS", DEFAULT_HEALTH_CHECK_INTERVAL)`.
- **Claim (IS, falsifiable):** the handshake/discovery deadline's fallback is
  `DEFAULT_HEALTH_CHECK_INTERVAL` (60s), not a dedicated startup constant (none
  exists in the const list, runtime.rs:58-105). Today the value is coincidentally
  the intended 60s (DIVERGENCE D3), but any tuning of the health interval silently
  re-times startup handshakes — latent cross-wiring between unrelated mechanisms.
- **Principle:** precise behavior (constant coupling).
- **Check:** in a test build, change `DEFAULT_HEALTH_CHECK_INTERVAL` alone and
  observe `McpRuntimeConfig::default().startup_timeout` follow it.

### F7 — `system_simulator` production module framed as an aspirational "digital twin" · severity low · effort S

- **file:line:** `kask/crates/hkask-regulation/src/system_simulator.rs:1-8`
  ("Predictive regulation via a moving-average digital twin", "Future (Fermi-style
  ODE models)" — referencing a `dynamics` crate that does not exist in the
  workspace); used in production at `cybernetics_loop.rs:231` (`simulator:
  MovingAverageExtrapolator`) and `:332`.
- **Claim (IS, falsifiable):** the production loop's trend predictor lives in a
  module named and motivated as a simulator with an unimplemented ODE future; the
  internal docs are honest ("simple moving-average… no learning"), but the framing
  invites speculative growth and misdescribes what production runs.
- **Principle:** P7 (no speculative abstraction).
- **Check:** `grep -r 'dynamics' Cargo.toml` → no such crate; the module's real API
  is a windowed moving-average fit.

### F8 — `AlertEscalationSink::persist_alert` carries a discard-by-contract legacy variant · severity low · effort S

- **file:line:** `kask/crates/hkask-regulation/src/algedonic.rs:170-200` (trait:
  `try_persist_alert`'s default falls back to `persist_alert`, `:187`);
  `kask/crates/kask_bridge/src/memory/alert_escalation.rs:390-394`
  (`let _ = self.persist_alert_reporting(...)` — "the legacy contract").
- **Claim (IS, falsifiable):** the sink trait requires a best-effort variant whose
  contract is to discard persistence failures, beside the honest
  `try_persist_alert` (which the production cycle already calls —
  `cycle.rs:134`). In a no-backward-compat project this is a legacy surface; the
  trait could collapse to the honest variant.
- **Principle:** P5 (delete the legacy path); P9.
- **Check:** `grep -rn '\.persist_alert('` → the only production caller is the
  default fallback at `algedonic.rs:187`.

### F9 — NEBIUS IDs classified as credentials · severity low · effort S

- **file:line:** `kask/crates/kask_bridge/src/mcp_servers.rs:435-436`
  (`NEBIUS_PROJECT_ID`, `NEBIUS_SUBNET_ID` under `credentials:`) vs `:457-465`
  (the file's own reclassification of `RUNPOD_TEMPLATE_ID` to `config_env`:
  "a template ID… not a key").
- **Claim (IS, falsifiable):** non-secret infrastructure IDs ride the credential
  allowlist, inconsistent with the file's documented classification rule; both are
  read via plain `std::env::var` (`hkask_mcp_training.rs:322`, `providers.rs:55`)
  — the config-var access pattern.
- **Principle:** P4 (boundary semantics — the credentials allowlist should mean
  secrets); internal consistency.
- **Check:** the two reads are `std::env::var` — identical to config vars; no
  keychain tier is consulted for them.

### F10 — `InferenceUsage` cannot distinguish "unreported" from zero · severity low · effort S–M

- **file:line:** `kask/crates/hkask-types/src/ports/inference_types.rs:77-81`
  (`pub struct InferenceUsage { prompt_tokens: u32, completion_tokens: u32,
  total_tokens: u32 }`); produced at `kask/crates/hkask-inference/src/hkask_inference.rs:707-724`
  (`content.unwrap_or_default()`, usage fields `.unwrap_or(0)` on optional wire data).
- **Claim (IS, falsifiable):** a provider that omits usage yields an all-zero
  `InferenceUsage`, indistinguishable at the type level from a genuine 0-token call
  — so any future token/cost accounting cannot tell "provider didn't report" from
  "nothing used". The project's own regulation layer models this correctly
  ("absence, not zero — a fabricated 0 would read as a real measurement",
  `cybernetics_loop.rs:70-71`); the inference port does not.
- **Principle:** P8/P9 (absence-vs-zero honesty).
- **Check:** read the struct (no Option/validity field); send a chat request whose
  response omits `usage` — the port reports zeros, not "unreported".

### F11 — Synthetic-feed error-status write discards its result silently · severity low · effort S

- **file:line:** `kask/mcp-servers/hkask-mcp-research/src/hkask_mcp_research.rs:1335-1338`
  (`let _ = spawn_db(db.clone(), move |conn| { update_synthetic_status(...) }).await;`).
- **Claim (IS, falsifiable):** the DB record that a synthetic feed failed is written
  best-effort with the `Result` dropped with no log — the tool error is returned to
  the caller, but a lost status write is invisible; a later feed listing shows a
  stale status with no trace.
- **Principle:** P9; `.rules` `let _ =` on fallible operations (borderline
  fire-and-forget bookkeeping — minimum remedy is a warn).
- **Check:** make `update_synthetic_status` fail (read-only DB file) and trigger a
  failed fetch — no log line names the lost write.

### F12 — "Reserved for future" parameters in live interfaces · severity low · effort S

- **file:line:** `kask/mcp-servers/hkask-mcp-research/src/research/synthetic.rs:148`
  (`let _ = content_type; // reserved for future content-type-based dispatch`);
  `kask/mcp-servers/hkask-mcp-prediction-markets/src/hkask_mcp_prediction_markets.rs:320`
  (`let _ = (&store, &path); // reserved for a future price-snapshot join`).
- **Claim (IS, falsifiable):** two production signatures carry parameters whose only
  use is silencing, justified by futures that don't exist — P7's exact anti-pattern
  (types and seams should emerge from real usage).
- **Principle:** P7.
- **Check:** `grep -n 'content_type' synthetic.rs` → no read site; no
  price-snapshot join exists.

### F13 — Repeated `json!(…).as_object_mut().expect("just constructed object")` (9 sites) · severity low · effort S

- **file:line:** `kask/mcp-servers/hkask-mcp-swarm/src/cloud_swarm_tools.rs:336, 2058,
  2124, 2272, 2516, 2582, 2638, 2690, 2745`.
- **Claim (IS, falsifiable):** nine copies of a just-constructed-object `expect` —
  each true by construction, but nine panic-anchored sites where one small helper
  (`fn insert(payload: &mut Value, …)`) would delete the pattern.
- **Principle:** P5 (pattern reuse over syntax repetition).
- **Check:** count the sites (`grep -c 'just constructed object'` → 9).

### Observations (examined, deliberately not findings)

- `crates/kanban_panel/src/board_picker.rs:145` — production
  `foreground.block_on(match_strings(...))` on the main thread: the `.rules`
  `block_on` class, but documented ("board list is small… same approach as
  ThreadPicker"), bounded work, upstream precedent. Watch under list growth.
- Media job store persists tool-result envelopes and re-parses strictly
  (`tools/jobs.rs:28-46`) — deliberate strict-contract design, pinned by
  `job_list_decoder_rejects_legacy_and_incomplete_shapes`.
- `default_corpus_passphrase`'s ProcessGlobal capture (`helpers.rs:44-54`) and
  `write_contained` single enforcement point (`helpers.rs:186-192`) are good
  deep-module patterns the F1 fix should follow.

---

## 4. Ranked opportunities

1. **Centralize corpus credential resolution** (F1 + F2 together): make
   `default_corpus_passphrase` return `Result` (empty → `permission_denied` at one
   enforcement line), and delete the model-facing `passphrase` fields. One seam,
   two findings, one change; shrinks ~10 schemas.
2. **Doc-truth pass** (F3): the six drift sites are the surfaces agents and
   rebasers trust most; the project's own `DOCUMENTATION_STANDARDS.md` lifecycle
   discipline should sweep `DIVERGENCE.md` counts, the docs portal, `runtime.rs`
   doc fragments, `mcp_servers.rs:119-121`, and `standalone_settings.rs` framing.
3. **Regex caching** (F4): `LazyLock` the corpus `convert.rs` and companies
   `screener.rs` patterns — measurable hot-path win, S effort.
4. **Constant hygiene in the MCP runtime** (F6): introduce
   `DEFAULT_STARTUP_TIMEOUT` and stop borrowing the health interval.
5. **Dead-knob audit** (F5): `skills_dir` is confirmed dead end-to-end; a
   one-command audit pattern (setting → emission → allowlist → reader grep) would
   catch its siblings if any exist.
6. **Regulation naming cleanup** (F7 + F8): rename `system_simulator` to its
   actual function (e.g. `extrapolation`), delete the ODE future-docs, and collapse
   `AlertEscalationSink` to the honest variant.
7. **Small honesty fixes** (F9–F13): NEBIUS reclassification, `InferenceUsage`
   absence modeling, the research status-write warn, removing the two
   "reserved for future" params, and the swarm `json!` helper.

---

## 5. Process record

### 5.1 Kata target condition

Every inventory crate examined by its assigned lenses; every finding falsifiable
with file:line; top findings adversarially verified; report ranks by
severity × effort; no upstream-direct-edit proposals; no duplicated findings;
no backward-compatibility concessions. Exit when the report exists — achieved.

### 5.2 Phase record

- **Phase 0 (Ground) — exit met.** Frame from DIVERGENCE.md, docs README,
  PRINCIPLES.md (P1–P12), Magna Carta, `.rules`; `.rules` spot-verified
  (machete scope ✓; `hkask-ledger` stale ✗; server counts stale ✗).
- **Phase 1 (Inventory) — exit met.** 31 deep-scope crates (19 core + 12 servers,
  ~228K LOC) + 16 zed-side kask-owned crates (~37K LOC, bounded scope,
  interpretation recorded in §2). Every crate assigned coverage.
- **Phase 2 (Review passes) — completed via direct passes.** *Stall record:*
  the initial plan delegated per-cluster passes to five parallel sub-agents; the
  operator canceled the wave, so the lead ran the passes directly (pattern
  sweeps for breadth + targeted deep reads on seam-central crates). Every crate
  received the sweep set; depth concentrated where the mission's center of mass
  is (bridge, runtime, storage, regulation, corpus). Bounded passes are recorded
  per crate in §2.
- **Phase 3 (Grill) — completed inline.** Every finding in §3 was verified against
  its cited lines this session; candidates that failed adversarial checks were
  dropped or demoted to observations: envelope-extractor "duplication" (test-only,
  verified), research `db.rs` `.ok()`s (legitimate optional lookups), ontology
  `panic!`s (test fixtures), `pick_best_provider` (guarded caller), media
  `block_on`s (test modules, verified per file), `openai_compat.rs:85`
  (Option::max, not a sense input), `allow(dead_code)` in `tool_schema.rs`
  (justified test fixture on the recorded baseline), `HKASK_QA_MODEL` (live reader),
  `board_picker.rs:145` (documented, bounded, precedent-backed).
- **Phase 4 (Synthesize) — this document.** Deduped (doc drift merged into F3;
  regex caching merged across two crates; the corpus passphrase chain split into
  its two distinct defects, F1 enforcement-gap vs F2 interface-exposure).

### 5.3 Metacognition — predictions vs. outcomes

| # | Phase-2 prediction (recorded before the passes) | Outcome | Gap |
|---|---|---|---|
| P2-1 | Servers re-implement shared helpers (envelope/DB/env) ≥3 duplication findings | **Wrong** — envelope discipline clean (shared helper; per-server extractors test-only); allowlists pin-tested per server | The codebase is more disciplined than the prior mean; the one duplication-class finding is the corpus passphrase chain (F1/F2) |
| P2-2 | Doc drift systemic (≥4 docs) | **Confirmed** — F3 cites six+ sites | — |
| P2-3 | Bridge allowlist misalignment ≥2 | **Wrong** — the allowlist surface is exemplary (rationale + pin tests); found one dead entry (F5) + one classification nit (F9) | — |
| P2-4 | ≥2 advertised invariants without enforcement lines | **Partial** — one comment-delegated invariant (F1); the invariant surface is unusually honest (Enforced/Gap/Unverified vocabulary, `loops/core.rs`) | Regulation's enforcement-status modeling raises the bar |
| P2-5 | `hkask-types` carries residual dead types from teardowns | **Wrong** — `voice.rs` alive; every suspect crate (`email`, `forecast`, `services-core`, `event-store`, `condenser`) has live dependents | Deletion discipline has been applied for real |

**Learning banked:** the kask tree's discipline is above the `.rules`-trap baseline —
future reviews should weight *doc-truth* and *cross-mechanism coupling* (F3, F6)
over the classic silent-failure patterns, which the sweeps show are largely
engineered out in production paths (confined to tests, where they are acceptable).

### 5.4 Calibration note (strengths observed — for honesty, not flattery)

Exemplary surfaces worth preserving as patterns: `hkask-storage`'s driver honesty
(PRAGMA ordering invariant, labeled pools, `with_durability`), the canonical
credential chain (`credentials.rs`), `parse_env_warn` as the reference env pattern,
`mcp_servers.rs`' rationale-documented allowlists with per-server pin tests,
`cycle.rs`'s absence-vs-zero modeling and `AlertQueueOutcome` distinctions,
`ingest.rs`'s `IngestionReport` honest counters, the two zed-side leaf crates
(`InvokeError` taxonomy; per-app-global leak rationale), `SPAWN_RUNTIME`'s
documented reactor-hop with the live incident, and `WARNED_MISSING_CREDENTIALS`
log-spam dedup. Production `unwrap()`/`expect()` is essentially confined to
documented invariants and mutex-poison propagation; GPUI/tokio traps are confined
to test modules.

### 5.5 Coverage honesty

Sweeps (unwrap/expect/let-_/ok()/block_on/tokio/panic/unwrap_or(0)/dead-code/
mod.rs/envelope/passphrase/env-read) ran over **all** 31 deep-scope crates and the
16 bounded crates; targeted deep reads covered the seam-central subset (§2 tables).
Crates marked *bounded* received sweeps + partial reads, not full-file reads —
recorded per crate, per the mission's bounded-pass allowance. No upstream-direct
edits are proposed; the zed-side findings (F3's `runtime.rs` citations are kask
crates; `board_picker.rs` is an observation) stay on the kask side of the seam.
---

## Repair record (2026-09-18 execution)

Executed under the continuation prompt with this report as spec. Repairs were
committed by the operator's stream, interleaved with concurrent work; hashes
name the commits carrying each repair (commit subjects sometimes describe the
stream's own headline work folded alongside staged repairs — content verified
per finding, not inferred from subject).

| Finding | State | Commit(s) | Pin |
|---|---|---|---|
| F1 — corpus passphrase fail-closed | repaired | `cc602605a8` | `open_memory_store_refuses_empty_passphrase_before_any_db_open` (permission_denied + no DB file created); `seeded_resolution_never_returns_an_empty_passphrase` |
| F2 — model-facing passphrase fields | repaired | `cc602605a8`, `8b7af7598f`, `1a0d5f5b68` | `no_tool_schema_exposes_a_passphrase_property` (walks all 26 tool schemas) |
| F3 — doc drift | repaired | `429812b116` (report-named sites) + stream doc-syncs `59798ddd23`, `d31881eff0`, `8399d39c85`, `f0a8692bc9`, `8e15e03ed7` | acceptance greps: 12 server dirs; `hkask-ledger` zero in manifest + member lists; `crates/hkask-cli` absent, zero refs; `DEGRADED` zero hits in runtime.rs; docs 67 < 70; DIVERGENCE.md member lists re-verified against root Cargo.toml (19 kask/crates members) |
| F4 — regex recompilation | repaired | `e21e3e5d2d` | conversion + screener suites green post-change (338/338); compile failures surfaced via `warn!` in `compiled()` instead of silent `.ok()` skip |
| F5 — dead skills_dir knob | repaired | `8a1bd7877b` | `mcp_env` emission tests updated; full-repo symbol sweep: zero `HKASK_SKILLS_DIR`/knob refs (settings_content, bridge settings/mcp_env/mcp_servers, settings_ui kask_page, 4 doc tables) |
| F6 — startup_timeout constant | repaired | `8a1bd7877b` | `startup_timeout_default_is_the_dedicated_sixty_second_constant` (runtime.rs config_tests) |
| F7 — system_simulator rename | repaired | `acbefb647b` | symbol sweep: zero `system_simulator`/`digital twin` refs; module now `extrapolation`, field `extrapolator`; diataxis row re-measured |
| F8 — persist_alert collapse | repaired | `acbefb647b`, `c86faf6a59` | `try_persist_alert_reports_confirmed_insert_and_supersede`; bridge tests now assert Confirmed(id)/Confirmed(None)/Attempted outcomes explicitly; legacy-path test deleted as subsumed |
| F9 — NEBIUS classification | repaired | `a7c54681f6` | `training_allowlist_matches_actual_reads` extended: both IDs must be absent from credentials and present in config_env |
| F10 — InferenceUsage absence | repaired (follow-up pass) | see notes below | `usage_absence_is_not_zero` (hkask-inference) — omitted wire usage → `reported: false`, present → `reported: true` |
| F11 — silent status write | repaired | `a7c54681f6` | both failure layers (db + join) `warn!` naming `feed_id`; tool error unchanged |
| F12 — reserved-for-future params | repaired | `a7c54681f6` | sweep: zero `reserved for future` silencing sites; orphaned `content_type` locals swept |
| F13 — nine expect sites | repaired | `a7c54681f6` | `insert_optional_field` helper (typed internal error, never a panic); grep: zero `just constructed object` sites; wire payloads byte-identical |

**F10 resolution (follow-up pass, 2026-09-18):** implemented the
recommended shape after the operator's go-ahead — `InferenceUsage` gained a
`reported: bool` field (serde-defaulted; `false` = the token counts are
placeholders, nothing was measured), mirroring `InferenceResult`'s
`cost_usd: Option<f64>` absence precedent (D20). The wire producer
(`usage_from_wire` in hkask-inference) sets it from wire `usage` presence;
the bridge's stream accumulator sets `true` on a `UsageUpdate` event (a
stream that ends without one leaves the default — unreported, not zero).
All ~18 fixture constructions across corpus/swarm/curator/media set the
flag honestly (nonzero fixtures → true, zero fixtures → false). Summing
readers (corpus tagging ops, swarm agent_executor) are unchanged — they
opt in to the flag only if future accounting needs it.

**Gate unblock outside findings:** 4 pre-existing `clippy::redundant_clone`
errors in corpus `services/qa_pipeline.rs` (present at review HEAD
`c9ecc55fd4`) were removed (folded into `a7c54681f6`) so the full kask-scope
`./script/clippy` gate is green at final state.

**Validation at final state:** `./script/clippy` (kask scope incl. machete)
green; `cargo nextest` green for hkask-mcp-companies, kask_bridge, hkask-mcp,
hkask-regulation, hkask-mcp-research, hkask-mcp-prediction-markets,
hkask-mcp-swarm, hkask-mcp-training (884/884); hkask-mcp-corpus green except
one failing test in `tools/gather.rs` introduced by the concurrent stream's own
gather hardening in `a7c54681f6` (their active work, not a repair regression —
corpus was 179/179 at the repair state); `cargo check -p zed` green; `cargo fmt
--check` clean except the concurrent stream's unstaged
`crates/agent/src/tools/context_server_registry.rs`. OCR test env
precondition: corpus OCR executor tests require `HKASK_TEMPLATE_ROOT` pointing
at the registry (`HKASK_TEMPLATE_ROOT=$PWD/kask/registry`).

**Follow-up pass (2026-09-18, open-items program):** the OCR precondition is
now resolved — the affected nine corpus tests seed the registry root
themselves via `helpers::seed_registry_template_root()` (per `template.rs`'s
own "repository tests must set it explicitly" contract; the corpus crate
adopted the repo-precedented `#![cfg_attr(not(test), forbid(unsafe_code))]`
posture to permit the one-shot test `set_var`), and the full corpus suite is
green with the variable unset. The commit-msg hook was hardened against the
placeholder-subject class (inline-fence subjects, "No changes were
provided"/"Here are the changes"/refusal prefaces), pinned by
`kask/scripts/check-commit-msg-selftest.sh` (9 cases, all green). The
gather-path test failure was root-caused to the committed test's fixture
nesting its "outside" target inside an allowed root — the stream's working-tree
correction (standalone system tempdir) is right and passes under both
harnesses; an independent probe confirmed the hardened containment rejects a
leaf symlink pointing outside the allowed roots. Two RR-0020 violations
pre-dating this work (`hkask-spreadsheet`, `hkask-steer-core` missing the
unsafe-gating attribute; both zero-unsafe crates) were fixed with
`#![forbid(unsafe_code)]`.

**Commit hygiene observations for the operator:** (1) commit `c86faf6a59`
landed on main with a malformed placeholder subject ("No changes were
provided…") while carrying real F8 bridge test rework — the `commit-msg` hook
missed this class; (2) several repair slices were folded into commits whose
subjects describe unrelated stream work (e.g. F3 inside "Register spreadsheet
MCP server and add CI gates", Batch E inside "Harden MCP tool dispatch and
cache path safety") — content verified per finding via `git log -S`.

**Dead-knob audit (operator-gated extension) — dispositioned in the follow-up
pass:** the setting → `mcp_env` emission → allowlist → server-reader sweep
found, beyond the removed `skills_dir`:

1. **Email-escalation family — REMOVED.** `HKASK_ALERT_EMAIL`,
   `HKASK_MXROUTE_SERVER`, `HKASK_CURATOR_EMAIL`, `HKASK_SMTP_USERNAME`,
   `HKASK_AUTHORIZED_EMAILS` (config_env) and `HKASK_SMTP_PASSWORD`
   (credentials) were removed from the curator server entry: the server has
   no email sink (links `hkask-regulation`, not `hkask-email`; zero env
   reads), while the alert email runs in the editor process (sink wired from
   settings; password from the keychain into the editor's own env). The
   emission helper and both false pin assertions were deleted/flipped;
   `kask-settings.md` corrected. **New functional finding surfaced for the
   operator:** the `kask.curator.email` transport fields (`mxroute_server`,
   `smtp_username`, `curator_email`) reach no consumer — the editor's
   `send_email` reads them from the editor process env, which nothing
   populates from settings (only the keychain password is set there). The
   feature currently works only with shell-exported vars; wiring settings →
   editor env (or migrating `send_email` to settings-based config) is an
   operator decision.
2. **`HKASK_CONDENSER_PERSONA_KEYWORDS` / `HKASK_CONDENSE_SALIENCY_WINDOW` —
   REMOVED end to end** (the `skills_dir` class): zero allowlist entries
   (filtered before any child) AND zero server readers AND zero editor
   readers — the `condenser.persona_keywords` and `condenser.saliency_window`
   settings fed only the dead emission (the live condenser fields
   `profile`/`auto_compress_tool_results` are consumed in-process from
   settings and stay). Settings fields, content-schema fields, emission fn,
   UI controls + dispatcher arm, doc rows, and the vacuous test pin deleted.
3. False positive for the record: `HKASK_SWARM_MEMORY_PASSPHRASE` matches only
   the comment documenting that it does NOT exist (the no-separate-passphrase
   invariant holds).
