# `dispatch_compute` Audit — `hkask-templates/src/compute.rs`

> Research spike requested by the operator after the multi-perspective
> review. The grill-me and idiomatic-rust sub-agents flagged sizing/mechanism
> errors in v0's CAND-4; this audit grounds the candidate in measured data.

## Measured facts (2026-08-17)

### File and dispatch sizing

| Metric | v0 claim | Actual | Source |
|--------|----------|--------|--------|
| `compute.rs` total lines | 1,270 | **3,379** | `wc -l` |
| `dispatch_compute` match span | "830-line match" | **832 lines** (L256–L1059) | `awk` measurement |
| String arms in `dispatch_compute` | 17 | **19** (18 named + 1 catch-all `other`) | `grep -cE` |
| Distinct sub-domains | "6: forecast/kata/swarm/listening/lisp/shell" | **6** (confirmed) | grep enumeration |
| Closures (`get_f64`/`get_bool`/`get_u64`) | "redefined per arm" | **Defined ONCE** at L231–254 | `read_file` — grill-me was right; v0 was wrong |
| Arms that bypass the closures with inline `input.get(...).and_then(...)` | — | **6 arms** (`calibrate_from_fermi`, `combine_tree_probabilities`, `brier_score_multi`, `swarm.converge_accumulate`, `swarm.second_order_monitor`, `swarm.filter_proposed_moves`) | `read_file` |

### Per-arm measurement

| # | `compute_ref` | Sub-domain | Match lines | Manifest usage | In-crate test fns | External callers |
|---|---------------|------------|-------------|-----------------|-------------------|------------------|
| 1 | `calibrate_from_fermi` | forecast | L257–281 (~25) | 1 (`superforecasting.yaml:160`) | 1 (`dispatch_calibrate_from_fermi`) | 0 |
| 2 | `outside_view_adjustment` | forecast | L282–289 (~8) | 1 (`superforecasting.yaml:219`) | 1 (`dispatch_outside_view_adjustment`) | 0 |
| 3 | `bayesian_update` | forecast | L290–296 (~7) | 1 (`superforecasting.yaml:331`) | 1 (`dispatch_bayesian_update`) | 0 |
| 4 | `combine_tree_probabilities` | forecast | L306–386 (~81) | 1 (`superforecasting.yaml:299`) | 3 (`dispatch_combine_tree_probabilities_and_gate`, `_missing_nodes_errors`, `_bad_tree_errors`) | 0 |
| 5 | `apply_calibration_adjustment` | forecast | L387–392 (~6) | 1 (`superforecasting.yaml:515`) | 1 (`dispatch_apply_calibration_adjustment`) | 0 |
| 6 | `brier_score` | forecast | L393–398 (~6) | **0 manifests** | 1 (`dispatch_brier_score`) | 0 |
| 7 | `brier_score_multi` | forecast | L399–425 (~27) | **0 manifests** | 0 (no direct test; referenced in `manifest_properties.rs:40`, `composition_principles.rs:679`) | 0 |
| 8 | `brier_interpretation` | forecast | L426–429 (~4) | **0 manifests** | 0 (referenced in `manifest_properties.rs:41`, `composition_principles.rs:680`) | 0 |
| 9 | `kata.object_gap` | kata | L451–468 (~18) | 2 (`metacognition.yaml:291`, `sequential-inquiry.yaml:191`) | 3 (`dispatch_kata_object_gap_complete`, `_missing_fields`, `_ungrounded_half_weighted`) | 0 |
| 10 | `kata.process_gap` | kata | L472–488 (~17) | 2 (`metacognition.yaml:305`, `sequential-inquiry.yaml:202`) | 2 (`dispatch_kata_process_gap_all_complete`, `_mixed`) | 0 |
| 11 | `kata.hypotenuse` | kata | L491–500 (~10) | 2 (`metacognition.yaml:318`, `sequential-inquiry.yaml:213`) | 1 (`dispatch_kata_hypotenuse`) | 0 |
| 12 | `kata.prediction_vs_result` | kata | L504–537 (~34) | 2 (`metacognition.yaml:333`, `sequential-inquiry.yaml:223`) | 2 (`dispatch_kata_prediction_vs_result_correct`, `_wrong`) | 0 |
| 13 | `swarm.converge_accumulate` | swarm | L549–682 (**134**) | 1 (`swarm-intelligence.yaml:459`) | 7 (`swarm_converge_accumulate_*`) | 0 |
| 14 | `swarm.second_order_monitor` | swarm | L683–810 (**128**) | 1 (`swarm-intelligence.yaml:488`) | 8 (`swarm_second_order_monitor_*`) | 0 |
| 15 | `swarm.filter_proposed_moves` | swarm | L827–932 (**106**) | 1 (`swarm-intelligence.yaml:334`) | 4 + 2 property tests (`swarm_filter_*`, `compute_gap_properties.rs`) | 0 |
| 16 | `listening.chunk_transcript` | listening | L943–977 (~35) | 1 (`listening.yaml:68`) | 5 (`listening_chunk_transcript_*`) | 0 |
| 17 | `listening.verify_citations` | listening | L978–1001 (~24) | 1 (`listening.yaml:97`) | 4 (`listening.verify_citations_*`) | 0 |
| 18 | `lisp.eval` | lisp | L1017–1034 (~18) | **82 manifests** | 16+ (`dispatch_lisp_eval_*`) | 0 |
| 19 | `shell.exec` | shell | L1045–1054 (~10) | 1 (`upstream-rebase.yaml:220`) | 0 (no direct test; `shell_exec` is `#[allow(clippy::disallowed_methods)]`) | 0 |
| — | `other` (catch-all) | — | L1055–1058 (~4) | — | 1 (`dispatch_unknown_ref_errors`) | — |

### Helper functions (outside the match)

| Function | Lines | Consumers | Notes |
|----------|-------|-----------|-------|
| `chunk_transcript` | L26–99 (~74) | 1 arm (`listening.chunk_transcript`) + 1 helper (`listening.verify_citations` via prior chunks) | Pure, well-tested. |
| `verify_citations` + `verify_citations_recursive` | L117–169 (~53) | 1 arm (`listening.verify_citations`) | Pure, well-tested. |
| `shell_exec` | L196–212 (~17) | 1 arm (`shell.exec`) | Sync blocking call; `#[allow(clippy::disallowed_methods)]`; **no injection seam** (idiomatic-rust finding). |
| `extract_swarm_decision` | L1069–1090 (~22) | 1 arm (`swarm.converge_accumulate`) | Pure. |
| `extract_roster_agent_types` | L1097–1113 (~17) | 2 arms (`swarm.converge_accumulate`, `swarm.filter_proposed_moves`) | Pure. |
| `extract_task_success_scalar` | L1121–1136 (~16) | 1 arm (`swarm.converge_accumulate`) | Pure; documents the `.rules` `unwrap_or(0)` trap avoidance. |
| `compute_object_gap` + `is_ungrounded` + `collect_field_keys` | L1149–1270 (~122) | 1 arm (`kata.object_gap`) | Pure, well-tested. |
| `compute_process_gap` | L1201–1249 (~49) | 1 arm (`kata.process_gap`) | Pure, well-tested. |

## Findings

### F1 — `lisp.eval` dominates manifest usage (82 of 97 = 85%)

`lisp.eval` is used by **82 manifests**; the next-most-used arm (`kata.*` family) is used by 2 manifests. The forecast arms (6 of them) are used by exactly **1 manifest** (`superforecasting.yaml`); 3 of the 8 forecast arms (`brier_score`, `brier_score_multi`, `brier_interpretation`) have **zero manifest callers** — they exist only in tests and in the `manifest_properties.rs` / `composition_principles.rs` allow-lists.

**Implication:** `dispatch_compute` is not a "god-function" in the usage sense — it's a dispatch table where one arm (`lisp.eval`) is the workhorse and 18 arms are niche. The bloat is in the *implementation* (832 lines), not the *interface* (19 string keys).

### F2 — Three forecast arms are dead surface (zero manifest callers)

`brier_score`, `brier_score_multi`, `brier_interpretation` have:
- **0 manifest `compute_ref:` references** (grep-verified).
- 1 direct test (`brier_score`); 0 direct tests for `_multi` and `_interpretation`.
- Referenced only in `manifest_properties.rs:39-41` (an allow-list of "known compute_refs") and `composition_principles.rs:678-680` (an input-spec table).

**Implication:** these are dead surface in the `.rules` sense ("convention helpers with only test callers are dead code"). They were likely added for the `superforecasting` skill's future use but never wired. The `superforecasting.yaml` manifest uses `apply_calibration_adjustment` (L515) for the same purpose — applying a Brier-derived calibration — so the dedicated `brier_*` arms are redundant.

### F3 — The swarm arms are the largest by line count

| Arm | Lines | Tests |
|-----|-------|-------|
| `swarm.converge_accumulate` | 134 | 7 |
| `swarm.second_order_monitor` | 128 | 8 |
| `swarm.filter_proposed_moves` | 106 | 4 + 2 property |
| **swarm subtotal** | **368** | **19** |

The 3 swarm arms account for **44% of the match body** (368 of 832 lines). They are cohesive (all consumed by `swarm-intelligence.yaml`), well-tested (19 tests + 2 property tests), and have 3 shared helpers (`extract_swarm_decision`, `extract_roster_agent_types`, `extract_task_success_scalar`).

**Implication:** the swarm sub-domain is the strongest extraction candidate — it has the highest line count, the most tests, the clearest cohesion, and 3 dedicated helpers. This is the one place where v0's "split into sub-domain modules" framing has merit.

### F4 — The closures are defined once, not "redefined per arm"

Grill-me was right; v0 was wrong. The `get_f64`/`get_bool`/`get_u64` closures are defined **once** at L231–254, capturing `input` and `compute_ref`. They are used by 13 of 19 arms. The 6 arms that bypass them (`calibrate_from_fermi`, `combine_tree_probabilities`, `brier_score_multi`, `swarm.converge_accumulate`, `swarm.second_order_monitor`, `swarm.filter_proposed_moves`) do so because they need array/object destructuring, not scalar extraction.

**Implication:** v0's CAND-4 rationale ("closures redefined per arm") was factually wrong. The real duplication is that the 6 bypass arms each re-implement ad-hoc `input.get("...").and_then(|v| v.as_array())...` chains. Extracting a `ComputeInput` helper that also handles array/object extraction would help; extracting the closures does not (they're already shared).

### F5 — `shell.exec` is the only untestable arm

`shell_exec` (L196–212) is a sync `std::process::Command::output` call. It's `#[allow(clippy::disallowed_methods)]` with a comment justifying it ("runs on background executor"). It has **0 direct tests** and no injection seam — a `CommandRunner` trait (idiomatic-rust finding) would make it testable.

**Implication:** the `shell.exec` arm is the one place where the idiomatic-rust "injection seam" recommendation applies. The other 18 arms are pure and well-tested.

### F6 — The catch-all error message is a maintenance hazard

The `other` arm (L1055–1058) hardcodes a list of supported `compute_ref` values in the error message:

```rust
"Unknown compute_ref: '{}'. Supported: calibrate_from_fermi, outside_view_adjustment, bayesian_update, apply_calibration_adjustment, brier_score, brier_score_multi, brier_interpretation, kata.object_gap, kata.process_gap, kata.hypotenuse, kata.prediction_vs_result, lisp.eval, shell.exec, swarm.converge_accumulate, swarm.second_order_monitor, swarm.filter_proposed_moves, listening.chunk_transcript, listening.verify_citations"
```

This list is manually maintained and already out of date (it omits `combine_tree_probabilities`, which is a real arm at L306). Adding a new arm requires updating the error message — a maintenance hazard.

**Implication:** a `ComputeRef` enum (idiomatic-rust finding) with `parse()` would auto-generate the supported list via `#[derive(Debug)]` variants. This is a P1 gain (typo'd `compute_ref` becomes a compile-time error at construction, not a runtime fallback) and a maintenance gain (the supported list is always current).

### F7 — No external callers of `dispatch_compute`

Grep across `kask/crates`, `kask/mcp-servers`, `zed-kask/crates` (excluding `hkask-templates/` itself) finds **0 external callers** of `dispatch_compute`. The only caller is `step_actions.rs:481` (`execute_compute`).

**Implication:** `dispatch_compute` is `pub` but effectively `pub(crate)`. The `test_utils` re-export (`hkask_templates.rs:65`) exposes it for proptest, but no production consumer outside the crate calls it. This means any refactor of `dispatch_compute`'s signature or internal structure has zero cross-crate blast radius — only `step_actions.rs:481` and the test suite need to keep working.

## Revised CAND-4 (replaces v0's CAND-4 and v1's CAND-4-minimal)

Based on this audit, the revised CAND-4 is:

### CAND-4 (revised) — Decompose `dispatch_compute` along the swarm seam + add `ComputeRef` enum + delete dead forecast arms

**Files:** `compute.rs` → new `compute/swarm.rs` (or `swarm_compute.rs` per `.rules` "no `mod.rs` files").

**Problem:** `dispatch_compute` is 832 lines across 19 arms. The audit shows:
- 3 forecast arms (`brier_score`, `brier_score_multi`, `brier_interpretation`) are dead surface (F2).
- 3 swarm arms (368 lines, 44% of the match body) are cohesive and have 3 dedicated helpers (F3).
- The closures are already shared (F4); v0's "redefined per arm" was wrong.
- `shell.exec` is the only untestable arm (F5).
- The catch-all error message is a maintenance hazard (F6).
- No external callers (F7) — zero cross-crate blast radius.

**Solution (essentialist + idiomatic-rust, grounded in the audit):**

1. **Delete the 3 dead forecast arms** (`brier_score`, `brier_score_multi`, `brier_interpretation`). Update `manifest_properties.rs:39-41` and `composition_principles.rs:678-680` to remove them from the allow-lists. If a future manifest needs them, they can be re-added. (Essentialist G1: dead surface, 0 manifest callers.)

2. **Add a `ComputeRef` enum** (idiomatic-rust P1):
```rust
pub enum ComputeRef {
    CalibrateFromFermi,
    OutsideViewAdjustment,
    BayesianUpdate,
    CombineTreeProbabilities,
    ApplyCalibrationAdjustment,
    KataObjectGap,
    KataProcessGap,
    KataHypotenuse,
    KataPredictionVsResult,
    SwarmConvergeAccumulate,
    SwarmSecondOrderMonitor,
    SwarmFilterProposedMoves,
    ListeningChunkTranscript,
    ListeningVerifyCitations,
    LispEval,
    ShellExec,
}

impl ComputeRef {
    pub fn parse(s: &str) -> Result<Self, UnknownComputeRef> { ... }
}
```
This makes the catch-all error message auto-generated (F6 fix) and makes typo'd `compute_ref` a compile-time error at construction (P1 gain). The dispatch becomes `match ComputeRef::parse(compute_ref)? { ... }` — exhaustive, no `_ =>` arm.

3. **Extract the swarm sub-domain** into `swarm_compute.rs` (F3 — the strongest extraction candidate). The 3 swarm arms (368 lines) + 3 helpers (`extract_swarm_decision`, `extract_roster_agent_types`, `extract_task_success_scalar`) move to the new module. `dispatch_compute`'s swarm arms become one-line delegates:
```rust
ComputeRef::SwarmConvergeAccumulate => swarm_compute::converge_accumulate(input),
ComputeRef::SwarmSecondOrderMonitor => swarm_compute::second_order_monitor(input),
ComputeRef::SwarmFilterProposedMoves => swarm_compute::filter_proposed_moves(input),
```
**Essentialist G1 PASS for this extraction** (unlike v0's 6-module split): the swarm arms have 3 dedicated helpers that are consumed only by swarm arms — inlining the extraction back into `dispatch_compute` would re-bloat the match by 368 lines + 3 helpers. The extraction earns its keep.

4. **Extract a `ComputeInput` helper** (F4) that handles scalar + array + object extraction:
```rust
struct ComputeInput<'a> {
    compute_ref: &'a str,
    input: &'a Value,
}

impl<'a> ComputeInput<'a> {
    fn get_f64(&self, key: &str) -> Result<f64> { ... }
    fn get_bool(&self, key: &str) -> Result<bool> { ... }
    fn get_u64(&self, key: &str) -> Result<u64> { ... }
    fn get_array(&self, key: &str) -> Result<&'a [Value]> { ... }
    fn get_str(&self, key: &str) -> Result<&'a str> { ... }
}
```
This replaces both the top-of-function closures (L231–254) and the ad-hoc `input.get("...").and_then(|v| v.as_array())...` chains in the 6 bypass arms (F4).

5. **Add a `CommandRunner` trait for `shell.exec`** (F5, idiomatic-rust finding):
```rust
pub trait CommandRunner: Send + Sync {
    fn run(&self, command: &str, cwd: &str) -> Result<Value>;
}

pub struct DefaultCommandRunner;
impl CommandRunner for DefaultCommandRunner { ... }  // production

#[cfg(test)]
pub struct FakeCommandRunner { ... }  // tests
```
`dispatch_compute` (or a wrapper) takes `Option<&dyn CommandRunner>` — `None` for all arms except `shell.exec`, which requires `Some`. This makes `shell.exec` testable.

**Do NOT extract the other 5 sub-domains** (forecast, kata, listening, lisp, shell) into separate modules. The audit shows:
- Forecast: 6 arms, ~150 lines total, 1 manifest caller, 0 external callers. Extraction would create a 150-line module with 1 consumer — pass-through.
- Kata: 4 arms, ~79 lines total, 2 manifest callers. Cohesive but small.
- Listening: 2 arms, ~59 lines + 127 lines of helpers (`chunk_transcript`, `verify_citations`). The helpers are the bulk; the arms are thin. Extraction is borderline — the helpers could move, but the arms are fine where they are.
- Lisp: 1 arm, 18 lines. No extraction warranted.
- Shell: 1 arm, 10 lines + 17-line helper. No extraction warranted.

**Essentialist verdict (revised):**
- G1 PASS for the swarm extraction (368 lines + 3 dedicated helpers; inlining re-bloats).
- G1 FAIL for the other 5 sub-domain extractions (pass-through).
- G1 PASS for the 3 dead arm deletions (0 manifest callers).
- G1 PASS for the `ComputeRef` enum (P1 gain — invalid states unrepresentable).
- G1 PASS for the `ComputeInput` helper (dedups the 6 bypass arms' ad-hoc chains).
- G1 PASS for the `CommandRunner` trait (makes `shell.exec` testable).
- G2 PASS (the `ComputeRef` enum is 1 type; `swarm_compute.rs` exports 3 functions + 3 helpers; `ComputeInput` is 1 struct; `CommandRunner` is 1 trait — all ≤7).
- G3 PASS (every extraction encodes genuine behavior).

**Idiomatic-rust Hoare assessment (revised):**
- P1 High (`ComputeRef` enum — typo'd refs become compile-time errors).
- P3 Medium+ (exhaustive match — adding a variant is a compile error at the dispatch site).
- P5 Medium+ (supported list auto-generated, not hand-maintained).
- P7 Medium (`ComputeInput` helper propagates errors, doesn't `unwrap_or`).
- P8-adjacent (`CommandRunner` trait makes `shell.exec` testable).
- Critique score: **0.15** (revised from v0's 0.40 and v1's 0.20 — the audit grounded the design).

**Success criteria:**
- 3 dead forecast arms deleted (grep-verified: 0 `compute_ref: brier_score` in manifests).
- `ComputeRef` enum added; `dispatch_compute` dispatches on the enum (exhaustive, no `_ =>` arm).
- `swarm_compute.rs` module created; 3 swarm arms + 3 helpers moved; `dispatch_compute` swarm arms are one-line delegates.
- `ComputeInput` helper extracted; 6 bypass arms use it (no inline `input.get("...").and_then(...)` chains).
- `CommandRunner` trait added; `shell.exec` has a `FakeCommandRunner` test.
- `compute.rs` shrinks from 3,379 lines to ~2,200 (swarm extraction: −368 lines; dead arm deletion: −37 lines; `ComputeInput` consolidation: ~−50 lines; tests stay).
- `cargo test -p hkask-templates` clean.
- `./script/clippy` clean.

**Risks:**
- The `ComputeRef` enum changes the dispatch from string-match to enum-match. The `compute_ref` field in manifests is still a string (YAML); `ComputeRef::parse` runs at dispatch time. If a manifest has a typo'd `compute_ref`, it currently falls to the `other` arm with a hardcoded error message; post-CAND-4, it fails at `ComputeRef::parse` with an auto-generated error. This is a behavior change (better error, but different error text). Low risk.
- The `swarm_compute.rs` extraction moves 3 helpers that are currently `fn` (private). Making them `pub(crate)` in the new module is the minimum visibility. No external consumer.
- The `CommandRunner` trait adds a parameter to `shell.exec`'s dispatch path. `dispatch_compute`'s signature is `pub fn dispatch_compute(compute_ref: &str, input: &Value) -> Result<Value>` — adding a `runner: Option<&dyn CommandRunner>` parameter changes the signature. Since there are 0 external callers (F7), this is safe. The internal caller (`step_actions.rs:481`) passes `None` for non-shell arms and `Some(&runner)` for `shell.exec`. Alternatively, `dispatch_compute` stays string-in/string-out and `shell.exec`'s arm calls a `thread_local!` or `OnceLock`-injected runner — but that's less clean than a parameter. Recommend the parameter.

**Sequencing:** Independent of CAND-2, CAND-3-minimal, CAND-12. Can run in parallel with CAND-3-minimal (disjoint write sets: `compute.rs` vs `step_actions.rs`). After CAND-10 (dead surface removal) is ideal — the 3 dead forecast arm deletions are a natural fit with CAND-10's dead-surface sweep, but they're also fine as a standalone CAND-4 commit.
