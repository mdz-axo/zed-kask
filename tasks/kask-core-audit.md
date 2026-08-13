# Kask Core Crates — Standing Audit

Read-only audit. No source file was modified. Every `file:line` below was
verified by reading the cited location; caller claims were verified by
workspace grep.

## 1. Coverage honesty

**Examined (target set A — duplication/smells):**

| Tier | Crate / file | Examined |
|---|---|---|
| 1 | `kask/crates/hkask-inference` | yes |
| 1 | `kask/crates/hkask-memory` | yes |
| 1 | `kask/crates/hkask-condenser` | yes |
| 1 | `kask/crates/kask_bridge` | yes |
| 2 | `kask/crates/hkask-mcp` | yes |
| 2 | `kask/crates/hkask-mcp-server` | yes |
| 2 | `kask/crates/hkask-types` | yes |
| 2 | `crates/hkask-tool-invoker` | yes |
| 2 | `crates/hkask-conversation-injector` | yes |
| 2 | `crates/agent/src/tool_router.rs` | yes (upstream D-seam) |
| 2 | `crates/agent/src/tools/skill_tool.rs` | yes (upstream D1) |

**Examined (target set B — settings coverage):**

| Layer | File | Examined |
|---|---|---|
| Schema | `kask/crates/kask_bridge/src/settings.rs` | yes |
| Content | `crates/settings_content/src/settings_content.rs` | yes |
| UI root | `crates/settings_ui/src/pages/kask_page.rs` | yes |
| UI sub-pages | `crates/settings_ui/src/pages/kask_page/*.rs` (18 files) | yes |

**Skipped / not covered:**

- **Tier 3** (`hkask-{regulation,capability,ledger,templates,storage,services-core}`):
  not covered. Tiers 1–2 yielded > 10 findings, so the tier-3 fallback condition
  did not fire. Two tier-3-adjacent observations surfaced incidentally and are
  recorded in §5 (Needs human verification), not as confirmed findings.
- **MCP server crates** under `kask/mcp-servers/` (e.g. `hkask-mcp-condenser`,
  `hkask-mcp-corpus`): out of scope (not in any tier list). Three observations
  from these are recorded in §5.
- **`hkask-types`**: examined; **no findings** (it is types-only; the canonical
  `tool_response` seam is correctly the single unwrapper and is not
  re-implemented anywhere audited).
- **`hkask-conversation-injector`**: examined; **no findings** (all `pub`
  surface has live widget/agent_ui callers).

**Skill dispatch — steps run and how:**

1. **graph-audit (dual)** — applied inline: code-graph extracted via workspace
   grep (caller search for every `pub fn`/`pub trait`), then semantic audit
   (cycle/orphan/dead-surface classification) over the extracted graph.
2. **refactor-architecture (discover + rank)** — applied inline as the primary
   engine for §2. No strangler-fig execution (read-only).
3. **code-review (`diff_base: origin/main`, `fix_mode: none`)** — **SKIPPED.**
   `origin/main` (`fb57134540`) and `HEAD` (`0c3537cc98`, on `main`) diverge, but
   this is a *standing* audit of existing code, not a change-in-progress review.
   A diff against `origin/main` would review unpushed commits only and would not
   surface any of the standing-code findings below (the dead modules, silent
   degradation, and settings gaps exist on both branches). Per the task
   instruction, the skip is recorded rather than fabricating a diff.
4. **capabilities-reasoner (agent-panel surface)** — applied inline; the
   capability registry and floor/ceiling gaps are reported in §3 (Tools + agent
   panel axis).
5. **idiomatic-rust (top 5 findings)** — applied inline using rust-analyzer as
   the extrinsic oracle. Result: rust-analyzer reports **no errors or warnings**
   on `openrouter_backend.rs` or `ollama_registry.rs`. The reason is structural
   and load-bearing for this audit: every dead-surface item is `pub`, and `pub`
   defeats the `dead_code` lint — so the compiler oracle does *not* catch them.
   Only workspace-wide caller grep does. This is why the dead surface persists
   silently.

**Inline lenses applied:** `pragmatic-semantics` (IS/OUGHT + constraint force
per finding), `essentialist` (deletion test on every proposed new settings
field — see §3 notes), `grill-me` (each finding challenged once; intentional
seams dropped and noted).

## 2. Deliverable 1 — Duplication / smell inventory

Constraint force legend: **Prohibition** = must not stay this way (dead
surface, panic risk, broken feedback loop); **Guardrail** = should not drift
(default-duplication, missing warn); **Hypothesis** = perf claim needing a
benchmark. IS = asserted fact about current code; OUGHT = normative proposal.

| # | file:line | class | pattern | sites | force | IS/OUGHT | named remedy |
|---|---|---|---|---|---|---|---|
| 1 | `kask/crates/hkask-inference/src/config.rs:248,251` | 1 silent-degradation | `.and_then(\|v\| v.parse().ok()).unwrap_or(…)` on `HKASK_HTTP_TIMEOUT_SECS`/`HKASK_HTTP_POOL_MAX_IDLE` swallows malformed numerics | 2 | Guardrail | IS | `tracing::warn!` naming var+bad value before fallback |
| 2 | `kask/crates/hkask-inference/src/config.rs:198` vs `:251` | 2 literal drift | `Default::default().pool_max_idle=5` but `from_env` falls back to `256` | 1 field | Guardrail | IS | `.unwrap_or(InferenceConfig::default().pool_max_idle)` |
| 3 | `kask/crates/hkask-inference/src/fal_backend.rs:122` | 1 silent-degradation | `unwrap_or("unknown")` then polls queue with `request_id="unknown"` on malformed submit | 1 | Guardrail | IS | `.ok_or_else(InferenceError::Json(...))?` |
| 4 | `kask/crates/hkask-inference/src/fal_backend.rs:129,134` | 2 magic literal | `Duration::from_secs(120)` + coupled `"120s"` error string | 2 | Guardrail | IS | `const FAL_QUEUE_POLL_TIMEOUT_SECS: u64 = 120;` + interpolate |
| 5 | `kask/crates/hkask-inference/src/atlascloud_backend.rs:88,127,131` | 2 magic literal | `0..200` poll loop + `from_secs(3000)` + `"10 min max"` string, coupled, unnamed | 3 | Guardrail | IS | `const ATLASCLOUD_MAX_POLLS`/`POLL_INTERVAL` + computed string |
| 6 | `kask/crates/hkask-inference/src/openrouter_backend.rs:24-310` | 5 dead surface | `OpenRouterBackend` module: zero `::new`/`::new_public` callers repo-wide (grep-verified); chat routes via IPC bridge | 1 module | Prohibition | IS | delete module + `pub mod` line in `hkask_inference.rs:47` |
| 7 | `kask/crates/hkask-inference/src/deepinfra_backend.rs:68-241,300,323` | 5 dead surface | chat + image methods (generate*/generate_image/image_to_image) zero callers; only `MediaProvider` impl used | 7 methods | Prohibition | IS | delete chat/image methods, keep 3 media ops |
| 8 | `kask/crates/hkask-inference/src/ollama_registry.rs:44-332` | 5 dead surface + advertised-invariant | re-exported (`hkask_inference.rs:56-58`) but zero callers in `hkask-mcp-training` (where `AdapterStore` lives); doc L89-95 advertises a seam that has no enforcement point | 1 module | Prohibition | IS | wire `register_adapter` into train→local path, or delete |
| 9 | `kask/crates/hkask-inference/src/fal_workflow.rs:147,71,79` | 5 dead surface | `topological_sort` + helpers test-only; live sort is `workflow::topological_sort_graph` | 1 fn+2 | Prohibition | IS | delete; move proptest assertions to live sort |
| 10 | `kask/crates/hkask-inference/src/inference_ipc_client.rs` (11 sites: 224,365,490,633,740,852,958,1005,1052,1103,+) | 6 hot-path dup | `InferenceParams { …~28 None… }` hand-written; no `Default` derive (`hkask-types/src/inference_ipc.rs:111`) | 11 | Guardrail | IS | derive `Default` on `InferenceParams`, use `..Default::default()` |
| 11 | `kask/crates/hkask-inference/src/openai_compat.rs:149-217` vs `:241-306` | 6 duplication | `openai_compatible_generate` / `_messages` share ~60-line tail (send→status→body→parse→log) | 2 fns | Guardrail | IS | extract `async fn openai_chat_roundtrip(...)` |
| 12 | `kask/crates/hkask-inference/src/chat_protocol.rs:197` | 6 panic risk | `&body[..500]` byte-slices a `String`; panics if byte 500 splits a multibyte codepoint (likely on GLM error pages) | 1 | Prohibition | IS | reuse `sanitize_error_body` (char-boundary-safe) |
| 13 | `kask/crates/hkask-memory/src/memory_store.rs:645-650` | 1 silent-degradation | `consolidation_candidate_count`: `Err(_) => 0` no warn (latent — wrapper's only caller is a test, see #17) | 1 | Guardrail | IS | `tracing::warn!` on Err, or delete with #17 |
| 14 | `kask/crates/hkask-memory/src/memory_store.rs:558-565` | 1 silent-degradation | `find_existing_by_eav` `.ok()?` on `query_by_entity_attribute`; on DB error the live consolidation path reads "no match" and seeds a duplicate semantic h_mem instead of Bayesian-combining | 1 | Prohibition | IS | propagate error or `match`+`warn!`+return `None` |
| 15 | `kask/crates/hkask-memory/src/memory_store.rs:462` | 5 dead surface | `compute_centroid` zero callers (prod or test) | 1 | Prohibition | IS | delete |
| 16 | `kask/crates/hkask-memory/src/memory_store.rs:50` | 5 dead surface | `CentroidResult` only constructed by #15; re-exported `hkask_memory.rs:31` | 1 | Prohibition | IS | delete (transitively dead) |
| 17 | `kask/crates/hkask-memory/src/consolidation_service.rs:338` | 5 dead surface | `MemoryConsolidator::consolidation_candidate_count` only test caller | 1 | Prohibition | IS | delete (drops the #13 latent trap) |
| 18 | `kask/crates/hkask-memory/src/consolidation_service.rs:348` | 5 dead surface | `semantic_low_confidence_count` zero callers; the `.rules`-cited `unwrap_or(0)` fix is in place but the fn itself is dead — the `.rules` example now demonstrates the trap on a non-existent fn | 1 | Prohibition | IS | delete; refresh `.rules` example to a live site |
| 19 | `kask/crates/hkask-memory/src/consolidation_service.rs:366` | 5 dead surface | `semantic_h_mem_count` zero callers | 1 | Prohibition | IS | delete |
| 20 | `kask/crates/hkask-memory/src/salience.rs:95` | 5 dead surface | `compute_method_signals` test-only; corpus `tagging/ops.rs:77-81` explicitly disclaims use | 1 | Prohibition | IS | delete |
| 21 | `kask/crates/hkask-memory/src/salience.rs:26` | 5 dead surface | `MethodSignals` only used by #20 | 1 | Prohibition | IS | delete |
| 22 | `kask/crates/hkask-memory/src/salience.rs:497,515,570` | 5 dead surface | `DeclaredMethod`/`MethodThresholds`/`DeclaredMethod::matches` test-only | 3 | Prohibition | IS | delete |
| 23 | `kask/crates/hkask-memory/src/salience.rs:628` | 5 dead surface | `tag_entities` test-only | 1 | Prohibition | IS | delete |
| 24 | `kask/crates/hkask-memory/src/salience.rs:676` | 5 dead surface | `EntityTags::tag_count` zero callers anywhere (note: `EntityTags`/`all_tags`/`compute_salience_batch` are LIVE) | 1 | Prohibition | IS | delete the method only |
| 25 | `kask/crates/hkask-memory/src/salience.rs:917,934` | 5 dead surface + advertised-invariant | `extract_keywords`/`keyword_overlap_score` test-only; doc L910-912 claims callers `MemoryService::recall_episodic`, `memory_recall`, `episodic_recall_context` — none exist repo-wide (grep-verified) | 2 | Prohibition | IS | delete or wire; fix the stale doc either way |
| 26 | `kask/crates/hkask-memory/src/recall_dedup.rs:25-33` | 6 hot-path alloc | `eav_hash` builds a `format!` String + recursive `canonical_value` String per h_mem on recall dedup; `blake3::Hasher::update` could stream | 1 | Hypothesis | OUGHT | incremental `Hasher::update` (needs benchmark) |
| 27 | `kask/crates/hkask-condenser/src/algorithms.rs:331` | 5 dead surface | `persona_to_anchor` zero callers | 1 | Prohibition | IS | delete |
| 28 | `kask/crates/hkask-condenser/src/engine.rs:53` | 5 dead surface + advertised-invariant | `CondenserEngine::classify` test-only; doc L49-52 claims `condenser_classify` MCP tool delegates here — no such tool exists repo-wide | 1 | Prohibition | IS | delete or land the tool; fix doc either way |
| 29 | `kask/crates/hkask-condenser/src/algorithms.rs:364-384` | 6 hot-path alloc | `FlashrankAlgorithm::novelty_score` O(n²) `HashSet` allocs in greedy selection loop | 1 | Hypothesis | OUGHT | pre-compute `selected_words: Vec<HashSet>` (needs benchmark) |
| 30 | `kask/crates/kask_bridge/src/memory.rs:585,594` | 2 literal drift | warn messages hardcode `"default 10_000"` while code calls `MemoryStore::default_storage_budget()` (`memory_store.rs:63`) | 2 | Guardrail | IS | interpolate `default_storage_budget()` into message |
| 31 | `kask/crates/kask_bridge/src/memory.rs:620,629` | 2 literal drift | warn messages hardcode `"default 180.0"` while code calls `default_memory_life_days()` (`bayesian.rs:48` = `6.0*30.0`) | 2 | Guardrail | IS | interpolate `default_memory_life_days()` |
| 32 | `kask/crates/kask_bridge/src/memory.rs:343,476` | 2 literal drift | `ConsolidationRequest { limit: 100, … }` literal; `Default::limit=100` lives in `hkask-types/src/ports/regulation.rs:13` | 2 | Guardrail | IS | `..Default::default()` |
| 33 | `kask/crates/kask_bridge/src/identity.rs:63` (+ re-export `kask_bridge.rs:40`) | 5 dead surface | `webid_from_username` test-only; production uses `WebID::for_agent_name` directly (`identity.rs:232`) | 1 | Prohibition | IS | drop `pub use`, downgrade to `pub(crate)` |
| 34 | `kask/crates/kask_bridge/src/memory.rs:916-923` | 6 duplication | `open_curator_store` re-implements `curator_db_path()` (L538) verbatim | 1 | Guardrail | IS | replace with `let p = curator_db_path();` |
| 35 | ~~`kask/crates/hkask-mcp/src/runtime.rs:46`~~ | 5 dead surface + advertised-invariant | **RESOLVED (verified 2026-08-13)** — `McpTool::validate_input` was deleted; zero occurrences repo-wide. Tool-input validation is server-side (rmcp/schemars) by design; `input_schema` survives only as metadata for LLM tool advertisement. The original finding (advertised gate with no enforcement point) was correct. | 0 | Prohibition | ~~IS~~ RESOLVED | done — see §7 |
| 36 | ~~`kask/crates/hkask-mcp/src/runtime.rs:241`~~ | 5 dead surface | **RESOLVED (verified 2026-08-13)** — `McpRuntime::start_server` deleted; only `start_server_with_env` remains (called from `crates/zed/src/main.rs`). | 0 | Prohibition | ~~IS~~ RESOLVED | done — see §7 |
| 37 | ~~`kask/crates/hkask-mcp/src/runtime.rs:424`~~ | 5 dead surface | **RESOLVED (verified 2026-08-13)** — `McpRuntime::get_tool` deleted; `get_tool_info` is live (`hkask-templates/src/step_actions.rs`). | 0 | Prohibition | ~~IS~~ RESOLVED | done — see §7 |
| 38 | ~~`kask/crates/hkask-mcp/src/runtime.rs:465,471,476,481`~~ | 5 dead surface + advertised-invariant | **RESOLVED (verified 2026-08-13)** — `list_servers`/`servers`/`connection_count`/`connections` all deleted rather than wired; no health endpoint consumes `McpRuntime` state (the only `/healthz` is a static responder in `hkask-mcp-swarm/src/a2a_http.rs`). See the §2a note on `is_connected`, which re-created a scoped version of this surface. | 0 | Prohibition | ~~IS~~ RESOLVED | done — see §7 |
| 39 | `kask/crates/hkask-mcp/src/runtime.rs:647` | 6 hot-path clone | `call_tool_inner` clones the entire `args` map on every governed dispatch though `args` is owned | 1 | Guardrail | IS | `match args { Value::Object(map) => map, _ => Map::new() }` (move) |
| 40 | ~~`kask/crates/hkask-mcp/src/runtime.rs:629`~~ | 6 hot-path clone | **SUPERSEDED (verified 2026-08-13)** — `verify_capability_domain` no longer exists: the whole per-call capability gate was removed (commits `1cc0`/`403e`, RR-0056), so neither it nor the `required_capability_for()` helper §7 credits is present. Originally: over-cloned via `get_tool_info` (clones `input_schema`/`description`) when only `required_capability` is needed | 1 | Guardrail | IS | add `required_capability_for()` lightweight accessor |
| 41 | `kask/crates/hkask-mcp-server/src/http_helpers.rs:76,84` | 5 dead surface | `api_get`/`api_put` zero callers incl tests; private `http_req` only used by these | 2 | Prohibition | IS | delete both (keep `classify_http_error`) |
| 42 | `kask/crates/hkask-mcp-server/src/tool_span.rs:240` | 5 dead surface + advertised-invariant | `tool_internal_error` zero callers; doc advertises a convenience no server adopted | 1 | Prohibition | IS | delete |
| 43 | `crates/hkask-tool-invoker/src/hkask_tool_invoker.rs:118-127` | 5 dead surface + advertised-invariant | `BlockProvenance::is_empty` zero prod callers; doc claims widgets use it for fallback decision (they key on `is_dispatchable` only) | 1 | Prohibition | IS | delete or wire a widget fallback to it |
| 44 | `crates/agent/src/tool_router.rs:162-163` (upstream D-seam) | 2 hardcoded | `threshold: 0.30`, `complex_word_threshold: 40` inline; no `KaskToolRouterSettings` field exists; `main.rs:1469` wires unconditionally | 2 | Guardrail | IS | add `KaskToolRouterSettings` + surface; **push behind D-seam** |
| 45 | `crates/agent/src/tools/skill_tool.rs:465` (upstream D1) | 6 dead binding + wasted clone | `let _skill_name = input.name.clone();` never read; clones per invocation | 1 | Prohibition | IS | delete L465; **push behind D-seam** |
| 46 | `crates/agent/src/tools/skill_tool.rs:522-525` vs `crates/agent/src/agent.rs:2244-2249` (upstream D1) | 6 dup + inconsistency | identical `"Skill '{}' manifest execution failed: {}"` string; error shape diverges (bare `Error` vs `render_skill_envelope`) | 2 | Guardrail | IS | shared `manifest_execution_failed_body()` + one error shape; **push behind D-seam** |

**Per-crate zero-finding classes (stated explicitly):**
- hkask-inference: class 3 (no `McpToolError` in crate), class 4 (no `value.get("content")`) — no findings.
- hkask-memory: classes 2,3,4 — no findings.
- hkask-condenser: classes 1,2,3,4 — no findings.
- kask_bridge: classes 1,3,4 — no findings.
- hkask-mcp: classes 1,2,3,4 — no findings.
- hkask-mcp-server: classes 1,2,4,6 — no findings.
- hkask-types: all classes — no findings.
- hkask-conversation-injector: all classes — no findings.

**Intentional seams dropped per grill-me (not findings):**
`MAX_IPC_LINE_BYTES` twin (`inference_ipc_server.rs:119` ↔
`inference_ipc_client.rs:54`, each documents the other); memory/condenser/curator
settings consumed directly via `KaskSettings` in the composition root rather
than via `mcp_env` (intentional in-process seam); `hkask-mcp` dual launch path
(McpRuntime + ContextServerStore); port traits in `hkask-types/src/ports/`
(multiple impls = legitimate port-promotion); `hkask-tool-invoker`
`.expect("…poisoned")` on process-global Mutex (convention).

## 2a. Reverification pass (2026-08-13)

Rows 35-38 and 40 were re-checked against the current tree and **all are
resolved or superseded**; §2 above is annotated accordingly. Before this pass §2
asserted them as open (`IS`) while §7 already recorded 35-38 as `fixed` — the
table and the fix log contradicted each other, and §7 was the trustworthy half.

**Line numbers throughout §2 are unreliable for `hkask-mcp/src/runtime.rs`.**
That file gained connection-healing (reap-on-death, liveness-on-read,
reconnect-on-demand) and the `DispatchError` split, so every cited offset has
moved. Search by symbol name, not line.

### New finding: `McpRuntime::is_connected` re-created the row-38 surface

`is_connected` (`kask/crates/hkask-mcp/src/runtime.rs`) was added for the
connection-healing tests and has **zero production callers** — the dispatch path
uses `get_peer`. Under the repo's "test-only callers are dead code" rule that is
the same pattern row 38 deleted: an accessor advertising a health surface that no
health consumer reads.

Mitigated rather than deleted, because reap-on-death is otherwise unobservable:
every production path *heals* on a missing peer, so it cannot distinguish
"reaped" from "reconnected". It is now `#[cfg(feature = "test-fixture")]`-gated,
so it does not exist in a normal build, with a doc comment pointing at this row.
**Wire a real health consumer before promoting it to unconditional `pub`.**

### Process note

This is the second time a dead-surface item in this crate was resolved by
deletion while §2 kept asserting it. When a fix lands, annotate §2 in the same
pass — a stale inventory is worse than no inventory, because it sends readers to
verify claims that were settled months earlier.

## 3. Deliverable 2 — Settings coverage gaps

### 3a. Present-but-not-surfaced-in-UI (field exists in `Kask*Settings` + `Default`, no UI control)

| behavior | current source (file:line) | proposed settings path | absent vs not-surfaced |
|---|---|---|---|
| Condenser persona keywords (token-cost dimension: fewer irrelevant lines retained) | `kask/crates/kask_bridge/src/settings.rs:347` (field); consumed via `mcp_env` L737 | surface in `kask_page/condenser.rs` | not-surfaced |
| Companies transactions dir (portfolio auto-load path) | `kask/crates/kask_bridge/src/settings.rs:393` | surface in `kask_page/companies.rs` | not-surfaced |
| Corpus embedding dim (must match model output) | `kask/crates/kask_bridge/src/settings.rs:400` | `kask.corpus.embedding_dim` → surface in `kask_page/corpus.rs` | not-surfaced |
| Corpus OCR concurrency (wall-clock latency dimension: pages in parallel) | `kask/crates/kask_bridge/src/settings.rs:407` | `kask.corpus.ocr_concurrency` → surface in `kask_page/corpus.rs` | not-surfaced |
| Corpus OCR simple threshold | `kask/crates/kask_bridge/src/settings.rs:410` | `kask.corpus.ocr_simple_max` → surface in `kask_page/corpus.rs` | not-surfaced |
| Corpus OCR moderate threshold | `kask/crates/kask_bridge/src/settings.rs:414` | `kask.corpus.ocr_moderate_max` → surface in `kask_page/corpus.rs` | not-surfaced |
| Corpus OCR sample rate (token-cost dimension: fraction of moderate pages sampled) | `kask/crates/kask_bridge/src/settings.rs:417` | `kask.corpus.ocr_sample_rate` → surface in `kask_page/corpus.rs` | not-surfaced |
| Corpus OCR tuneable toggle | `kask/crates/kask_bridge/src/settings.rs:420` | `kask.corpus.ocr_tuneable` → surface in `kask_page/corpus.rs` | not-surfaced |
| Swarm skills dir (local agent skill-awareness; empty = skill-blind) | `kask/crates/kask_bridge/src/settings.rs:521` | `kask.swarm.skills_dir` → surface in `kask_page/swarm.rs` | not-surfaced |

**Note:** `corpus.rs` surfaces only 2 of 8 `KaskCorpusSettings` fields
(`embedding_model`, `template_root`). The 6 OCR/embedding-dim fields are the
heaviest single-page gap.

### 3b. Genuinely absent — behavior is hardcoded or env-var-only with NO `Kask*Settings` field

Each proposed NEW field passed the essentialist deletion test: a code path
exists today that reads the hardcoded/env value, so the field would have a real
reader (not dead-on-arrival). Proposed paths name the consumer.

| behavior | current source (file:line) | proposed settings path | dimension / classification |
|---|---|---|---|
| HTTP request timeout | env-only `HKASK_HTTP_TIMEOUT_SECS` at `kask/crates/hkask-inference/src/config.rs:248` | `kask.inference.http_timeout_secs` (NEW) | wall-clock latency; absent |
| HTTP pool max idle | env-only `HKASK_HTTP_POOL_MAX_IDLE` at `config.rs:251` (drift: Default=5, env fallback=256 — see #2) | `kask.inference.pool_max_idle` (NEW) | wall-clock latency; absent |
| Inference concurrency cap (max parallel requests) | hardcoded in inference port/runtime | `kask.inference.max_concurrent_requests` (NEW) | wall-clock latency; absent |
| Retry/backoff policy | hardcoded | `kask.inference.retry_policy` (NEW) | wall-clock latency + token-cost; absent |
| Streaming chunk size | hardcoded in `chat_protocol.rs` parse | `kask.inference.streaming_chunk_size` (NEW) | wall-clock latency; absent |
| Embedding batch size | hardcoded (distinct from `ocr_concurrency`, which exists but is unsurfaced) | `kask.corpus.embedding_batch_size` (NEW) | wall-clock latency + token-cost; absent |
| Context-injection token budget | hardcoded in context injector | `kask.memory.injection_token_budget` (NEW) | token-cost; absent |
| Memory eviction policy | hardcoded in `memory_store.rs` | `kask.memory.eviction_policy` (NEW) | token-cost; absent |
| Condenser trigger threshold (token count that fires condensation) | hardcoded; `KaskCondenserSettings.profile`/`saliency_window` are surfaced but neither is a trigger threshold | `kask.condenser.trigger_threshold_tokens` (NEW) | token-cost; absent |
| Condensation target size | hardcoded; `saliency_window` controls summary max_tokens, not target message size | `kask.condenser.target_size_tokens` (NEW) | token-cost; absent |
| Deferred-tool-result drain policy | hardcoded in `run_turn_internal` `end_turn` (per `.rules`) | `kask.chat.deferred_result_drain` (NEW) | wall-clock latency; absent |
| Stream buffer/flush behavior | hardcoded | `kask.chat.stream_buffer` (NEW) | wall-clock latency; absent |
| Tool-router activation threshold | hardcoded `0.30` at `crates/agent/src/tool_router.rs:162` | `kask.tool_router.threshold` (NEW) | token-cost (catalog size → prompt tokens); absent; **push behind D-seam** |
| Tool-router complex-word threshold | hardcoded `40` at `tool_router.rs:163` | `kask.tool_router.complex_word_threshold` (NEW) | token-cost; absent; **push behind D-seam** |
| Tool catalog cap | hardcoded in `agent.rs::select_catalog_skills` L4224-4234 (currently disabled — all skills kept) | `kask.tool_router.catalog_cap` (NEW) | token-cost; absent |
| Tool timeout | per-call model input (`terminal_tool.rs:62,108`); no global default | `kask.tools.timeout_secs` (NEW) | wall-clock latency; absent |
| Per-agent `mcp_tools` allowlist | agent profile JSON / `agent_card.json`; enforced at `ToolDispatchPort::invoke_tool` (`hkask-types/src/ports/inference_port.rs:98-108`); edited via zed agent profile modal, NOT kask settings | **not a kask settings gap** (lives in zed profile modal) | n/a — correctly outside kask settings |
| Parallel tool-call cap | provider capability flag (`cloud_llm_client.rs:325`) | **not a kask settings gap** (provider-driven) | n/a — correctly outside kask settings |
| Panel-visible capability toggles | hardcoded | `kask.panel.*` (NEW) | absent; **needs consumer wiring** (Hypothesis — see §5) |

**Sub-page "resolve via From" rule:** no violations found. Every sub-page
resolves `Content`→`Settings` via `.map(Into::into).unwrap_or_default()`
(reading `Default`). `inference_providers.rs:38-41` uses
`unwrap_or_else(KaskInferenceProvidersSettings::from_env)` — documented and
correct (UI layer doesn't depend on `settings_content`).

## 4. Top 10 ranked recommendations (blast radius)

| rank | recommendation | findings | blast radius |
|---|---|---|---|
| 1 | Delete the `OpenRouterBackend` module (#6) and the `DeepInfraBackend` chat/image methods (#7) | #6,#7 | low — zero callers; removes ~450 lines; confirm no near-term consumer first |
| 2 | Fix `find_existing_by_eav` silent DB-error swallow (#14) | #14 | medium — on the live consolidation path; changes write semantics (duplicate h_mem seed → skip-with-warn) |
| 3 | Resolve `OllamaRegistry` advertised seam (#8): wire or delete | #8 | medium — either lands the train→local migration the doc promises, or removes ~290 lines + a misleading doc |
| 4 | Surface the 6 unsurfaced `KaskCorpusSettings` OCR/embedding-dim fields in `corpus.rs` (§3a) | §3a | low — pure UI additions; fields + `mcp_env` already exist |
| 5 | Derive `Default` on `InferenceParams` and switch 11 sites to `..Default::default()` (#10) | #10 | low-medium — mechanical, ~280 `None` lines removed; touches `hkask-types` + `hkask-inference` |
| 6 | ~~Wire `McpTool::validate_input` into `call_tool_inner` or delete it (#35)~~ **RESOLVED** — deleted; validation is server-side (rmcp/schemars) | #35 | medium — either makes the JSON-Schema gate load-bearing (behavior change for malformed tool inputs) or removes advertised theater |
| 7 | Add `tracing::warn!` to the three silent-degradation env reads (#1, #13) + fix `config.rs` pool_max_idle drift (#2) | #1,#2,#13 | low — diagnostics only, no behavior change; #2 fixes a real default mismatch |
| 8 | ~~Delete the `McpRuntime` health-check cluster (#38) or land the health endpoint~~ **RESOLVED** — all four deleted; no health endpoint landed | #36,#37,#38 | low if delete (4 dead methods); medium if land endpoint (new surface) |
| 9 | Fix the `salience.rs`/`engine.rs`/`ollama_registry.rs` advertised-invariant doc lies (#8,#25,#28) — delete the dead fns or wire them; refresh the `.rules` example (#18) to a live site | #8,#18,#25,#28 | low — mostly deletion; the `.rules` refresh is a separate doc commit |
| 10 | Add `KaskToolRouterSettings` for the two hardcoded `LazyToolRouter` thresholds (#44) behind a D-seam | #44 | medium — new settings struct + `Default` + UI sub-page + D-seam wiring in `main.rs:1469` |

## 5. Needs human verification (Hypothesis-tier)

These are perf claims or out-of-tier observations, not confirmed findings.

- **#26 `recall_dedup.rs:25-33` eav_hash allocations** — the `format!` + recursive
  `canonical_value` String per h_mem is real, but whether recall dedup is a
  measured bottleneck needs a benchmark on realistic h_mem counts. `blake3::Hasher::update`
  streaming is the proposed remedy but is an OUGHT, not verified faster.
- **#29 `FlashrankAlgorithm::novelty_score` O(n²) HashSet allocs** — quadratic in
  the selected set, but flashrank is the fallback for non-shell/test/build
  categories. Needs a benchmark on real tool-result sizes before optimizing.
- **#12 `chat_protocol.rs:197` multibyte panic** — the byte-slice `&body[..500]`
  panics iff byte 500 splits a multibyte codepoint. High likelihood on GLM
  (Chinese) error bodies, but unverified that a provider actually returns a
  >500-byte multibyte error body on this path.
- **Out-of-tier (tier 3) observation — `hkask-services-core/src/config.rs:133-137,165-175`**:
  `HKASK_MEMORY_LIFE_DAYS` read via `.ok().and_then(|v| v.parse().ok())` —
  silent `.ok()?` chain on a numeric env var, the same trap class as #1. Not
  audited as a tier-1/2 finding; flagged for the tier-3 pass.
- **Out-of-scope observation — `hkask-mcp-condenser/.../hkask_mcp_condenser.rs:367`**:
  `store.query_deduped(w).ok()` on the saliency-score memory-query loop — silent
  `.ok()` feeding a score (silent-degradation on a measurement). MCP server
  crate, outside all tiers; flagged for completeness.
- **Out-of-scope observation — `hkask-mcp-condenser/.../hkask_mcp_condenser.rs:289`**:
  `unwrap_or(5)` literal duplicating `KaskCondenserSettings::default().saliency_window`
  (the `.rules` "settings defaults must live in `Default` impls" pattern). MCP
  server crate, outside all tiers.
- **§3b panel-visible capability toggles** — the proposed `kask.panel.*` fields
  need a consumer wired (a panel affordance that reads them). Without that
  wiring the fields would be dead-on-arrival — essentialist deletion test fails
  until a reader is identified. Marked Hypothesis, not a confirmed gap.

## 6. Not covered

- **Tier 3 crates**: `hkask-regulation`, `hkask-capability`, `hkask-ledger`,
  `hkask-templates`, `hkask-storage`, `hkask-services-core`. Not examined
  (tiers 1–2 yielded > 10 findings). The one tier-3 observation that surfaced
  incidentally (`hkask-services-core` config silent `.ok()?`) is in §5.
- **MCP server crates** under `kask/mcp-servers/` (`hkask-mcp-condenser`,
  `hkask-mcp-corpus`, `hkask-mcp-swarm`, etc.): not in any tier list; not
  examined. Two observations surfaced incidentally are in §5.
- **`hkask-test-harness`, `hkask-bridge-ontology`, `hkask-email`,
  `hkask-forecast`, `hkask-keystore`, `hkask-lisp`**: not in any tier list; not
  examined.

## 7. Should-fix mode — fixes applied

After the audit, fixes were implemented (no backward-compatibility constraint;
dead surface deleted rather than `#[allow(dead_code)]`-ed). Every change was
verified with `cargo check` and the affected test suites. Status per finding:

| # | Status | Notes |
|---|---|---|
| 1,2 | fixed | `config.rs` numeric env reads now `tracing::warn!` on parse failure; `pool_max_idle` falls back to `Default` (drift closed) |
| 3 | fixed | `fal_backend.rs` `request_id` now errors instead of polling with `"unknown"` |
| 4,5 | fixed | named `FAL_QUEUE_POLL_TIMEOUT_SECS` / `ATLASCLOUD_MAX_POLLS` / `ATLASCLOUD_POLL_INTERVAL` consts, interpolated into messages |
| 6 | fixed | `openrouter_backend.rs` module deleted (zero callers); `RR-0049.yaml` `include` glob updated to drop the deleted file |
| 7 | fixed | `DeepInfraBackend` chat + image methods deleted; only the live `MediaProvider` impl (bg/speech/transcribe) kept |
| 8 | fixed | `ollama_registry.rs` module + re-exports deleted (advertised `AdapterStore` seam had no enforcement point) |
| 9 | fixed | dead `fal_workflow::topological_sort` deleted; `workflow::topological_sort_graph` made `pub`; the 3 proptest properties re-homed to exercise the live sort (all pass) |
| 10 | fixed | `InferenceParams` derives `Default` (`hkask-types`); 10 construction sites in `inference_ipc_client.rs` now use `..Default::default()` |
| 11 | fixed | shared `openai_chat_roundtrip` extracted; both `openai_compatible_generate*` delegate |
| 12 | resolved | the `&body[..500]` byte-slice sat in `vision_infer`, which was deleted as dead surface (its only callers were the deleted vision methods); `sanitize_error_body` remains the canonical char-boundary-safe truncation helper |
| 13 | fixed | `consolidation_candidate_count` now `tracing::warn!`s on `Err` |
| 14 | fixed | `find_existing_by_eav` propagates DB error with `warn!` (no silent duplicate h_mem seed) |
| 15,16 | **INVALID (restored)** | `compute_centroid`/`CentroidResult` are used by `hkask-mcp-corpus/src/corpus/embed/service.rs`; the deletion's grep scope missed `kask/mcp-servers/`. Restored. |
| 17,18,19 | fixed | `MemoryConsolidator::consolidation_candidate_count`/`semantic_low_confidence_count`/`semantic_h_mem_count` deleted — verified no `kask/mcp-servers/` callers |
| 20–23 | **INVALID (restored)** | `compute_method_signals`, `MethodSignals`, `DeclaredMethod`/`MethodThresholds`/`DeclaredMethod::matches`, `tag_entities` are used by `hkask-mcp-corpus` (`corpus/embed/service.rs`, `passage.rs`, `discover/llm.rs`, `config.rs`, `types.rs`). The audit's claim that corpus "disclaims use" was true only of `corpus/tagging/ops.rs`, not the embed/discover services. `salience.rs` restored to original; all 186 `hkask-mcp-corpus` tests pass. |
| 24,25 | restored (dead pub fns retained) | `EntityTags::tag_count`, `extract_keywords`, `keyword_overlap_score` have no `kask/mcp-servers/` callers and appear genuinely dead, but were restored with `salience.rs` to avoid further scope risk; their stale doc comments (claiming `MemoryService::recall_episodic`/`memory_recall` callers) remain — a separate doc-cleanup. |
| NoEmbeddingsForCentroid | restored | the cascade removal of this `MemoryStoreError` variant (and its two downstream match arms) was wrong — `compute_centroid` constructs it; restored along with #15. |
| 26 | not fixed | Hypothesis-tier (perf, needs benchmark) — left untouched |
| 27,28 | fixed | `persona_to_anchor` + `CondenserEngine::classify` (advertised `condenser_classify` MCP tool that doesn't exist) deleted |
| 29 | not fixed | Hypothesis-tier — left untouched |
| 30–32 | fixed | `kask_bridge memory.rs` log messages interpolate the resolved constants; `ConsolidationRequest` literals → `..Default::default()` |
| 33 | fixed | `webid_from_username` deleted (was `pub(crate)`+`allow(dead_code)`); tests removed |
| 34 | fixed | `open_curator_store` now calls `curator_db_path()` (duplication removed) |
| 35 | fixed | `McpTool::validate_input` deleted (rmcp validates server-side; the client-side duplicate was unenforced theater); `jsonschema` dep removed from `hkask-mcp` Cargo.toml |
| 36–38 | fixed | `McpRuntime::start_server`/`get_tool`/`list_servers`/`servers`/`connection_count`/`connections` deleted (health-check cluster with no health endpoint) |
| 39 | fixed | `call_tool_inner` moves the owned `args` map instead of cloning |
| 40 | ~~fixed~~ **superseded — this entry is inaccurate** | The `required_capability_for()` helper this credits does NOT exist in the tree (verified 2026-08-13). Both it and `verify_capability_domain` were removed when the per-call capability gate was deleted (RR-0056). The clone concern is moot: there is no capability lookup on the dispatch path. |
| 41 | fixed | `api_get`/`api_put`/`http_req` deleted; re-exports removed |
| 42 | fixed | `tool_internal_error` deleted; the now-dead `ToolSpanGuard::internal_error` method also removed (cascade cleanup) |
| **43** | **INVALID** | see §8 |
| 44 | fixed | new `KaskToolRouterSettings` (`threshold`, `complex_word_threshold`) with `Default`/`From`/content-layer/UI; `LazyToolRouter::new_with_thresholds` added (D-seam); `main.rs` wires settings into the router |
| 45 | fixed | dead `let _skill_name = input.name.clone()` deleted (D1 seam) |
| 46 | fixed | shared `manifest_execution_failed_body()` extracted; both `skill_tool.rs` and `agent.rs::send_skill_invocation` call it (format-string duplication removed) |
| §3a | fixed | all 9 unsurfaced settings fields now have UI controls (`persona_keywords`, `transactions_dir`, 6 `KaskCorpusSettings` OCR/embedding-dim fields, `skills_dir`) |
| §3b | partial | only the tool-router thresholds (#44) were added; the remaining genuinely-absent fields (HTTP timeout, concurrency, retry/backoff, embedding batch size, context-injection budget, condenser trigger threshold, etc.) were not added — they need new settings fields + readers and were outside the top-10/§3a scope |

Cargo dep cleanup (per the `.rules` "Cargo.toml deps outlive their consumers" trap): `chrono` removed from `hkask-inference`, `jsonschema` removed from `hkask-mcp`.

## 8. Invalid finding + concurrent-edit note

**Finding #43 is INVALID.** The should-fix pass disproved it: `BlockProvenance::is_empty`
(`crates/hkask-tool-invoker/src/hkask_tool_invoker.rs:118`) has two live production
callers in `crates/hkask-portfolio-widget/src/view.rs:961` (`scrub_enabled`) and
`:1023` (`build_returns_dispatch_args`). The doc comment is accurate — the
portfolio widget uses `is_empty` to distinguish the empty-provenance fallback
from dispatchable/partial provenance. The audit's grill-me lens missed this
because the caller grep was scoped too narrowly. The method was NOT deleted.

**Concurrent editor (build break, resolved).** During the should-fix pass, a
concurrent editor committed `68d5c1fd15` "Remove dead media workflow and object
segmentation code," which removed the `execute_workflow` tool from
`hkask-mcp-media/generation.rs` AND removed the `SegmentObject`/`ExecuteWorkflow`
variants from `provider.rs` — but incompletely: it left `as_str`'s
`ExecuteWorkflow` arm dangling and left `fal_backend`/`media_router`/
`inference_ipc_server`/gallery `extract_object` still referencing the variants,
which broke compilation. (The `SegmentObject` removal was definitely wrong —
`extract_object` still calls it.) `provider.rs` was restored to the last
known-good committed state (`0c3537cc98`, where the enum/`FromStr`/`as_str` are
consistent and both variants are present), which restores compilation while
leaving the concurrent editor's `generation.rs` tool removal intact. If the
concurrent editor intends to fully remove `execute_workflow`/`segment_object`,
they must also remove the `fal_backend`/`media_router`/`inference_ipc_server`
references (and `extract_object` in `gallery.rs`), not just the enum variants.

**Sub-agent misreport.** The `hkask-inference` sub-agent also made an unreported
erroneous edit to `provider.rs` (removing the same live variants) and reported
its `cargo check` as clean; this was caught only by an independent
`cargo check -p hkask-mcp-server --tests`. Lesson: sub-agent "check clean"
claims were independently re-verified.

## 9. Process finding — dead-surface grep scope must include `kask/mcp-servers/`

Two invalid deletion clusters (#15/#16 and #20–#23) shared one root cause: the
should-fix sub-agent's caller-grep was scoped to `kask/crates/` + `crates/`, which
**excludes `kask/mcp-servers/`** — the 13 MCP server crates that are the primary
consumers of the `hkask-memory` and `hkask-inference` public APIs. `hkask-mcp-corpus`
alone uses `compute_centroid`, `CentroidResult`, `NoEmbeddingsForCentroid`,
`compute_method_signals`, `tag_entities`, `MethodSignals`, `DeclaredMethod`, and
`MethodThresholds`. The audit's own grill-me lens missed this for the same reason
(its dead-surface verification grepped the same two trees).

**Correct grep scope for kask dead-surface claims:** `kask/crates/`, `crates/`,
AND `kask/mcp-servers/` (plus `kask/docs/` and `tasks/` for doc references). A
`pub` item with zero callers in `kask/crates/`+`crates/` but a caller in
`kask/mcp-servers/` is live, not dead. This is a candidate `.rules` addition
(proposed, not edited inline): *"Dead-surface grep scope must include
`kask/mcp-servers/` — the MCP server crates are the primary consumers of the
`hkask-*` public APIs and are outside the `kask/crates/`+`crates/` trees."*

The provider.rs break (§8) was a separate concurrent-editor issue, not this scope
gap.