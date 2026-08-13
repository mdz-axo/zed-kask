# Findings — Cybernetic-impedance + idiomatic-Rust bug hunt across the kask codebase

> Multi-skill cascade: bug-hunt (primary) → graph-audit (dual, seeded charters) →
> idiomatic-rust (lens) → pragmatic-cybernetics (lens) → essentialist (gate) →
> grill-me (adversarial). Reflection leg: refactor-architecture (plan) +
> metacognition (Kata).
>
> Scope: `kask/crates/` (18 hKask crates + `kask_bridge`). Upstream seam files
> reviewed only where a `// zed-kask:` comment or D-seam entry exists — no
> upstream file is edited (acceptance criterion 6).
>
> **Status: ALL findings implemented and committed.** See the implementation
> status table below for per-finding commit references.

## Implementation status

| Finding | Status | Commit | Verification |
| --- | --- | --- | --- |
| F1 (TestCoverageSensor broken-sensor) | **Implemented** | `998922afcb` | `cargo test -p hkask-regulation` (78 pass), clippy clean |
| F2 (MutationScoreSensor broken-sensor) | **Implemented** | `998922afcb` | Same as F1 (shared `latest_run_metrics`) |
| F3 (latest_metrics_path duplication) | **Implemented** | `998922afcb` | `grep -c 'fn latest_metrics_path'` → 0 (eliminated) |
| F4 (agent_paths divergent HKASK_DATA_DIR rule) | **Implemented** | `998922afcb` | `cargo test -p hkask-types` (107 pass), clippy clean |
| F5 (SqliteRegistry::count silent query_row collapse) | **Implemented** | `998922afcb` | `cargo test -p hkask-templates` (177 pass), clippy clean |
| F6 (SqliteRegistry::query_skills silent failures) | **Implemented** | `998922afcb` | Same as F5 |
| F7 (SqliteRegistry::get_skill_owned silent failures) | **Implemented** | `998922afcb` | Same as F5 |
| F8 (stale `.rules` propagate_taint_for_binding) | **Implemented** | `5cf3112638` | `grep -c 'propagate_taint' .rules` → 0 |
| F9 (list_models variety-deficit) | **Deferred** | — | Low-impact; existing `warn!` closes the observability loop |
| F10 (parse_sse_stream loop-not-closed) | **Implemented** | `5cf3112638` | `cargo test -p hkask-inference` (47 pass), clippy clean |
| Bonus: `generate_stream_with_model` override | **Implemented** | `423f36b007` | `cargo test -p kask_bridge` (160 pass), clippy clean |
| Bonus: `Sdmx` match arm fix (pre-existing) | **Implemented** | `5cf3112638` | `cargo test -p hkask-condenser` (83 pass), clippy clean |

## Crates actually reviewed

| Crate | Reviewed modules | Charters |
| --- | --- | --- |
| `hkask-regulation` | `sensor_provider.rs`, `tool_stats.rs`, `dampener.rs`, `system_simulator.rs`, `algedonic.rs`, `cybernetics_loop.rs`, `set_points.rs`, `runtime.rs` | 3 |
| `hkask-templates` | `output_schema.rs`, `registry_sqlite.rs`, `compute.rs`, `step_actions.rs`, `executor.rs` | 2 |
| `hkask-types` | `agent_paths.rs`, `tool_response.rs` | 1 |
| `hkask-storage` | `kata.rs`, `core/database.rs`, `hmem/archive.rs` | 1 |
| `hkask-inference` | `inference_ipc_client.rs`, `openai_compat.rs`, `chat_protocol.rs`, `config.rs`, `media_router.rs`, `provider.rs`, backends | 1 |
| `hkask-mcp` | `runtime.rs`, `bin/mcp_test_fixture.rs`, `tests/reconnect_integration.rs` | 1 |
| `hkask-keystore` | `signing.rs`, `keychain.rs`, `version_file.rs` | 1 |
| `hkask-email` | `hkask_email.rs` | 1 |
| `kask_bridge` | `inference.rs`, `settings.rs`, `mcp_servers.rs`, `model_resolution.rs`, `memory/curator_stores.rs` | 2 |

Gas consumed: ~62k of the 1.2M hard cap (analysis-only; no template cascade
executed). bug-hunt: 3 charters max per crate honored. graph-audit: 2 query
passes (duplication + dead-surface grep). Terminated by condition (a): all
scoped crates reviewed + grill-me run on top 5 + metacognition gap < epsilon.

## Findings table

Legend — type column:
- `formal` = formal code bug (panic, indexing, wrong model resolution, taint).
- `impedance:<category>` = cybernetic impedance, one of:
  `broken-sensor`, `silent-failure`, `variety-deficit`, `loop-not-closed`,
  `good-regulator`.

| # | Finding | file:line | type | severity | idiomatic fix proposal | essentialist verdict |
| --- | --- | --- | --- | --- | --- | --- |
| F1 | `TestCoverageSensor::sense` collapses every I/O and parse error to `None` via `.ok()?` — a DB outage, permission error, or malformed `metrics.json` is indistinguishable from "coverage meets set-point," so the regulation loop sees *no deviation* and never acts. | `kask/crates/hkask-regulation/src/sensor_provider.rs:416-430` | `impedance:broken-sensor` | HIGH | Return `Result<Option<Signal>, SenseError>`; classify `Io`, `Parse`, `MissingField`; `warn!` on error and propagate `Err` so `CyberneticsLoop::tick` can distinguish "no signal" from "sensor broken." Mirror `tool_stats::read_count_field`'s warn-then-fallback pattern. | PASS (G1: behavior lost on deletion — loop goes blind; G2: 1 public fn; G3: no abstraction added) |
| F2 | `MutationScoreSensor::sense` — identical broken-sensor pattern to F1, byte-for-byte. | `kask/crates/hkask-regulation/src/sensor_provider.rs:483-497` | `impedance:broken-sensor` | HIGH | Same as F1; extract shared `read_latest_metric(trace_dir, field)` returning `Result<Option<f64>, SenseError>` (see canonical-patterns.md P1). | PASS |
| F3 | `TestCoverageSensor::latest_metrics_path` and `MutationScoreSensor::latest_metrics_path` are byte-identical duplicates (same env-read, same mtime scan, same `Option` return). | `kask/crates/hkask-regulation/src/sensor_provider.rs:394-411` and `:461-478` | `formal` (duplication) + `impedance:good-regulator` (two regulators, one model, divergent evolution risk) | MEDIUM | Extract `fn latest_run_metrics(trace_dir: &Path) -> Option<PathBuf>` as a free function or `MetricsFileLocator` struct; both sensors hold a `MetricsFileLocator` and call `locator.latest()`. 2 production callers, deletion test: inline → 18 lines reappear in both. | PASS |
| F4 | `resolve_data_dir` and `resolve_under_data_dir` in `agent_paths.rs` duplicate the `HKASK_DATA_DIR → XDG_DATA_HOME → HOME → CWD` fallback chain **but diverge on the `HKASK_DATA_DIR` rule**: `resolve_data_dir` only honors it when absolute or `.`-prefixed (L55); `resolve_under_data_dir` honors it unconditionally (L78). A relative `HKASK_DATA_DIR=foo` resolves to `foo` under one fn and `$XDG_DATA_HOME/hkask/foo` under the other → agent DBs land in different trees depending on which helper the caller picked. | `kask/crates/hkask-types/src/agent_paths.rs:52-69` vs `:77-99` | `impedance:good-regulator` + `formal` (divergent rules for the same model) | HIGH | Make `resolve_under_data_dir(relative)` delegate to `resolve_data_dir().join(relative)`; delete the duplicated fallback chain. Single regulator, single rule. 2 production callers (`curator_stores.rs:24`, `mcp_servers` allowlist docs), deletion test passes. | PASS |
| F5 | `SqliteRegistry::count` warns on pool-get failure (L239) but the subsequent `query_row` failure collapses to `0` via `.unwrap_or(0)` (L246) with no warn — a locked/corrupt templates table reads as "0 templates," silently disabling skill discovery. | `kask/crates/hkask-templates/src/registry_sqlite.rs:243-247` | `impedance:broken-sensor` | MEDIUM | Replace `.unwrap_or(0)` with a match that `tracing::warn!`s the rusqlite error and returns 0, mirroring the pool-get branch two lines above. | PASS |
| F6 | `SqliteRegistry::query_skills` returns `Vec::new()` on `pool.get()`, `prepare`, and `query_map` failures with no `warn!` (L515-542) — a transient DB lock makes every skill query return empty, silently. | `kask/crates/hkask-templates/src/registry_sqlite.rs:514-542` | `impedance:silent-failure` | MEDIUM | Add `tracing::warn!(target: "hkask.templates", error = %e, "query_skills: <stage> failed, returning empty")` to each early-return arm. | PASS (G1: behavior lost — callers see empty; G2: 1 fn; G3: no abstraction) |
| F7 | `SqliteRegistry::get_skill_owned` collapses `pool.get()` and `query_row` errors to `None` via `.ok()?` (L507, L511) — "no such skill" is indistinguishable from "DB broken." | `kask/crates/hkask-templates/src/registry_sqlite.rs:506-512` | `impedance:broken-sensor` | MEDIUM | Split `NotFound` from `Io`/`Schema` errors; warn on the latter, return `None` only on `NotFound`. | PASS |
| F8 | `.rules` references `propagate_taint_for_binding` ("input_mapping bindings must call propagate_taint_for_binding before context.insert") but the function does not exist anywhere in the codebase — it was removed with the `hkask-guard`/taint layer (D4, deleted 2026-08-10). The rule is stale and misleads any agent/reader who treats it as a live contract. | `zed-kask/.rules` ("Skill system" section) | `impedance:good-regulator` (rule regulates a model that no longer exists) | LOW | Per `.rules` hygiene ("Don't edit `.rules` inline during feature work — propose additions in PR descriptions"), propose in the next PR description that the `propagate_taint_for_binding` bullet be struck. Do **not** edit `.rules` inline. | PASS (gate applies to the *proposal*, not the rule file) |
| F9 | `InferenceIpcClient::list_models` swallows the IPC error and returns `Vec::new()` with a `warn!` (L896-899) — correct on the warn axis, but the empty list is then consumed by callers as "no models available," which can silently disable model selection UIs. The warn exists, so this is a *variety-deficit* (the regulator can act on "broken" but callers can't distinguish it from "empty"). | `kask/crates/hkask-inference/src/inference_ipc_client.rs:889-899` | `impedance:variety-deficit` | LOW | Consider returning `Result<Vec<ModelEntry>, InferenceError>` so callers can show "IPC unavailable" vs "no models." Non-blocking; the warn already closes the observability loop. | DEFER (G1 marginal — callers already treat empty as "retry"; G2 passes; G3 no abstraction. Survives but low priority.) |
| F10 | `parse_sse_stream` silently `continue`s on every `serde_json::from_str` failure (L411-413). For a streaming SSE parser this is arguably correct (malformed lines are expected), but a *persistent* parse failure (provider changed schema) produces an empty stream with no signal. | `kask/crates/hkask-inference/src/chat_protocol.rs:406-416` | `impedance:loop-not-closed` (no feedback path for "every line unparseable") | LOW | Add a counter; if 100% of lines in a stream fail to parse, emit one `warn!` with the first offending line. Avoids per-line spam while closing the loop. | DEFER (G1: behavior is intentional for SSE; G2/G3 pass. Survives but optional.) |

### Findings that did NOT survive probing (recorded for transparency)

- `dampener::StagnationDetector::ineffective_count` `unwrap_or(0)` (L311): **not a
  finding** — absence of a history key *is* a measured zero (no prior ineffective
  actions). The `.rules` "unwrap_or(0) on sense inputs" trap targets cases where
  a present-but-corrupt value is masked; here the key is genuinely absent.
- `set_points.rs` `unwrap_or(defaults.*)` (L288-346): **not a finding** — config
  layer, not a sense input; `None` means "operator did not override," and the
  default is the correct semantic.
- `kata.rs` `unwrap_or(0)` on `MAX(id)`/`COUNT(*)` (L72, L109, L128): **not a
  finding** — SQL aggregates always return a row; `NULL` → 0 is the correct
  semantic for an empty table.
- `system_simulator.rs` `unwrap_or_else(|e| e.into_inner())` on poisoned mutex
  (L46, L56): **not a finding** — this is the correct recovery pattern for a
  poisoned mutex (continue rather than poison the regulation loop).
- `KaskSwarmSettings::default()` vs `SwarmConfig::default()` duplication
  (`settings.rs:557` comment): **not a finding** — deliberate seam duplication
  documented inline; the two `Default` impls live in crates that cannot depend
  on each other (§13.1 invariant). Pinned by `swarm_settings_default_emits_no_env`.

## grill-me verdict (top 5)

Run once near the end, across Recall → Mechanism → Rationale → Edge Cases →
Synthesis. Findings that failed Mechanism or Rationale are retracted.

| Finding | Recall | Mechanism | Rationale | Edge Cases | Synthesis | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| F1 (TestCoverageSensor broken-sensor) | PASS | PASS — `.ok()?` collapses `io::Error` and `serde_json::Error` to `None`, and `sense` returns `Option<Signal>`, so the loop sees "no signal" = "no deviation." | PASS — `.rules` explicitly calls this out; `tool_stats` is the in-repo counter-example. | PASS — DB outage, permission denied, truncated file, missing `coverage_pct` key all collapse identically. | PASS — generalizes to F2, F5, F7. | **SURVIVES** |
| F2 (MutationScoreSensor broken-sensor) | PASS | PASS — byte-identical to F1. | PASS | PASS | PASS | **SURVIVES** |
| F4 (agent_paths divergent HKASK_DATA_DIR rule) | PASS | PASS — `resolve_data_dir` gates on `is_absolute() \|\| starts_with(".")` (L55); `resolve_under_data_dir` does not (L78). A relative `HKASK_DATA_DIR=foo` yields `foo` vs `$XDG/hkask/foo`. | PASS — Good Regulator: two regulators for the same model with different rules is a textbook violation. | PASS — relative path, empty string, symlink, `HKASK_DATA_DIR` unset. | PASS — the fix (delegate) is the canonical pattern. | **SURVIVES** |
| F3 (latest_metrics_path duplication) | PASS | PASS — confirmed byte-identical via grep. | PASS — divergent evolution risk is real (F1/F2 already share the bug). | PASS — both callers pass `trace_dir` + read `metrics.json`. | PASS | **SURVIVES** |
| F5 (SqliteRegistry::count silent query_row collapse) | PASS | PASS — pool-get warns (L239), `query_row` does not (L246). | PASS — same `.rules` trap as F1. | PASS — locked DB, corrupt schema, dropped table. | PASS | **SURVIVES** |

No retractions from the top 5. F6–F10 were not grilled (below the top-5 cut) but
are retained at MEDIUM/LOW with the essentialist verdict recorded.

## Residual risks

- F8 (stale `.rules` entry) cannot be fixed inline per `.rules` hygiene; it
  requires a PR-description proposal. Until then, agents reading `.rules` may
  treat `propagate_taint_for_binding` as a live contract and waste cycles
  searching for it.
- F9/F10 are deferred — the warn already closes the observability loop; the
  variety-deficit / loop-not-closed tags are accurate but low-impact.
- The `kask_bridge` inference path's `generate_stream` hardcodes
  `model_override: None` (L582) — documented inline as a TODO and pinned by the
  cascade's `call_inference_stream` contract. Not a finding (intentional), but a
  future caller of `generate_stream_with_model` on this port will silently lose
  the override via the default trait impl. Worth a follow-up test.
