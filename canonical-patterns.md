# Canonical patterns — shared helpers for duplicated/near-duplicated kask functions

> Each pattern survived the essentialist 3-gate (Exist → Surface → Contract),
> has ≥2 production callers (grep-verified), and carries a falsifiable deletion
> test. Patterns that failed the gate are listed under "Rejected" for
> transparency.
>
> **Status: ALL 3 patterns implemented and committed** in `998922afcb`.
> The rejected patterns (R1, R2) remain correctly rejected — no speculative
> abstractions were added.

## P1 — `latest_run_metrics(trace_dir) -> Result<Option<PathBuf>, MetricsLocateError>`

**Problem.** `TestCoverageSensor::latest_metrics_path` and
`MutationScoreSensor::latest_metrics_path` are byte-identical
(`sensor_provider.rs:394-411` and `:461-478`). Both collapse I/O errors to
`None` via `.ok()?`, which is the F1/F2 broken-sensor finding. Extracting the
locator closes the duplication *and* gives one place to fix the error handling.

**Proposed signature.**
```rust
enum MetricsLocateError {
    TraceDirInaccessible { path: PathBuf, error: io::Error },
    MetadataUnavailable { path: PathBuf, error: io::Error },
}

fn latest_run_metrics(trace_dir: &Path) -> Result<Option<PathBuf>, MetricsLocateError>
```
Returns `Ok(None)` only when the dir exists but contains no `metrics.json`;
returns `Err` for I/O failures so the sensor can `warn!` and propagate
distinguishable signal to `CyberneticsLoop::tick`.

**Current duplicate sites.**
- `kask/crates/hkask-regulation/src/sensor_provider.rs:394-411` (`TestCoverageSensor`)
- `kask/crates/hkask-regulation/src/sensor_provider.rs:461-478` (`MutationScoreSensor`)

**Proposed canonical location.** `kask/crates/hkask-regulation/src/sensor_provider.rs`
as a free `pub(crate) fn` (both sensors live in this file; no cross-crate callers
exist today). If a third consumer appears, promote to `hkask-types::trace_fs`.

**Deletion test.** Inline the body back into both sensors → 18 lines reappear in
each *and* the F1/F2 error-classification fix must be applied twice. The
duplication is load-bearing because the bug is duplicated.

**Caller count.** 2 production callers (the two sensors). Meets the ≥2 threshold.

**Essentialist verdict.** PASS all three gates.
- G1 (Exist): behavior lost on deletion (no locator); complexity reappears in both sensors.
- G2 (Surface): 1 public fn, 1 error enum — ≤7.
- G3 (Contract): no trait, no wrapper, no generic — pure function over `&Path`.

## P2 — `resolve_under_data_dir` delegates to `resolve_data_dir` (eliminate the divergent fallback chain)

**Problem.** `resolve_data_dir` and `resolve_under_data_dir` in `agent_paths.rs`
duplicate the `HKASK_DATA_DIR → XDG_DATA_HOME → HOME → CWD` fallback chain but
diverge on the `HKASK_DATA_DIR` rule (F4): `resolve_data_dir` honors it only when
absolute or `.`-prefixed (L55); `resolve_under_data_dir` honors it unconditionally
(L78). A relative `HKASK_DATA_DIR=foo` resolves to `foo` under one and
`$XDG/hkask/foo` under the other.

**Proposed canonical form.**
```rust
pub fn resolve_under_data_dir(relative: &Path) -> PathBuf {
    resolve_data_dir().join(relative)
}
```
Delete the duplicated fallback chain in `resolve_under_data_dir` (L77-99). The
`tracing::warn!` on the CWD fallback (L91-97) moves into `resolve_data_dir`'s
CWD arm so it fires once for both callers.

**Current duplicate sites.**
- `kask/crates/hkask-types/src/agent_paths.rs:52-69` (`resolve_data_dir`)
- `kask/crates/hkask-types/src/agent_paths.rs:77-99` (`resolve_under_data_dir`)

**Proposed canonical location.** `kask/crates/hkask-types/src/agent_paths.rs`
(both fns already live here; this is a delegation refactor, not a move).

**Deletion test.** Restore the duplicated chain → the F4 divergence reappears
and a relative `HKASK_DATA_DIR` silently splits agent DBs across two trees again.

**Caller count.**
- `resolve_under_data_dir`: 3 production callers — `crates/zed/src/main.rs:735`
  (threads DB), `crates/zed/src/main.rs:1420` (curator DB),
  `kask/crates/kask_bridge/src/memory/curator_stores.rs:24`.
- `resolve_data_dir`: 4 production callers —
  `crates/settings_ui/src/pages/kask_page/general.rs:21`,
  `kask/crates/hkask-services-core/src/config.rs:111` (`from_env`),
  `kask/crates/hkask-services-core/src/config.rs:165` (`from_secrets`),
  `kask/crates/kask_bridge/src/identity.rs:226`.

Both well above the ≥2 threshold.

**Essentialist verdict.** PASS all three gates.
- G1: behavior lost on deletion (no path resolution); complexity reappears.
- G2: 2 public fns, 0 types added — ≤7.
- G3: no abstraction added; a delegation call replaces a duplicated chain. The
  fix *removes* an abstraction (the divergent second regulator).

## P3 — `read_count_field`-style "warn-on-malformed-sense-field" template (advisory, not extraction)

**Problem.** F5, F6, F7 are three sites in `registry_sqlite.rs` that collapse DB
errors to a default with no `warn!`, while `tool_stats::read_count_field`
(`tool_stats.rs:215-234`) is the in-repo exemplar of the correct pattern: warn
naming the field/tool/error, then fall back. This is not a function-extraction
pattern (the call sites have different types — `usize`, `Vec<Skill>`,
`Option<Skill>`) but a *convention* pattern.

**Proposed canonical form (convention, not fn).**
Every regulation-loop sense input that falls back to a default on error must:
1. `tracing::warn!` with `target`, the offending field/key, the expected type,
   and the actual error/value.
2. Return the default only after the warn.
3. Distinguish `NotFound`/missing (no warn — measured zero) from `Io`/`Schema`
   (warn — broken sensor).

Reference implementation: `read_count_field` at
`kask/crates/hkask-regulation/src/tool_stats.rs:215-234`.

**Current violating sites.**
- `kask/crates/hkask-templates/src/registry_sqlite.rs:243-247` (`count` query_row)
- `kask/crates/hkask-templates/src/registry_sqlite.rs:514-542` (`query_skills`)
- `kask/crates/hkask-templates/src/registry_sqlite.rs:506-512` (`get_skill_owned`)

**Proposed canonical location.** No new function. Document the convention in
`kask/crates/hkask-regulation/src/tool_stats.rs` as a `///` reference comment on
`read_count_field` ("this is the canonical pattern for sense-input fallback;
see `.rules`") and apply it at the three violating sites.

**Deletion test.** Remove the convention → F5/F6/F7 regress: a locked templates
table silently reads as "0 templates" / "no skills" with no operator signal.

**Caller count.** The *pattern* has 2 production callers already
(`read_count_field` is called twice in `load_state`); the three violating sites
would become 3 more adopters.

**Essentialist verdict.** PASS (as a convention, not a function).
- G1: the convention is load-bearing (it's the `.rules` "broken feedback loop"
  trap's mitigation).
- G2: 0 new public items.
- G3: no abstraction — it's a documented discipline, not a wrapper.

## Rejected patterns (failed essentialist)

### R1 — Extract a shared `Sensor::sense_with_error` trait method

**Why proposed.** F1/F2 suggest sensors should return `Result<Option<Signal>,
SenseError>` instead of `Option<Signal>`.

**Why rejected.** The `Sensor` trait (`sensor_provider.rs`) is consumed by
`SensorBus::sense_all` which fans out to N sensors and aggregates `Option<Signal>`
into a `Vec<Signal>`. Changing the return type is a trait-shape change, not a
duplication fix — it touches every implementor and the bus. The essentialist G3
gate flags this as speculative generality: today only 2 sensors share the bug,
and the fix is local (P1 + warn). A trait change is warranted only when a *third*
sensor with the same broken-sensor pattern appears. Defer.

### R2 — Extract `SqliteRegistry::with_conn` helper for the pool-get-then-warn pattern

**Why proposed.** F5/F6/F7 all do `let conn = match self.pool.get() { Ok(c) => c,
Err(e) => { warn; return default } }`.

**Why rejected.** The three sites return different types (`usize`, `Vec<Skill>`,
`Option<Skill>`) and different defaults. A generic helper would need a
`Default` bound or a closure producing the default — adding abstraction for 3
callers with divergent shapes. G1 fails: inlining the 4-line match back is
trivial. Apply P3 (the convention) inline instead.
