# Reflection — metacognition Kata + refactor-architecture plan

## Part A — Metacognition Improvement Kata (run on the review itself)

### Step 1 — Grasp current condition

**Object-space (Dublin Core artifacts produced):**
- `findings.md` — 10 findings, 9 crates reviewed, file:line on every finding.
- `canonical-patterns.md` — 3 proposed patterns (P1, P2, P3), 2 rejected (R1, R2).
- This file.

**Process-space (PKO procedure followed):**
- bug-hunt: 3 charters/crate max, static baseline + dynamic expansion, no-fiction
  citation gate.
- graph-audit: 2 query passes (duplication grep, dead-surface grep).
- idiomatic-rust: lens applied per-finding (type-driven fix proposals).
- pragmatic-cybernetics: each finding tagged with exactly one impedance category.
- essentialist: 3-gate run on every proposed pattern; rejections recorded.
- grill-me: top 5 interrogated; 0 retractions.

**Obstacles encountered:**
- The `grep` tool requires the `zed-kask/` prefix in `include_pattern`; initial
  `kask/crates/` searches returned false negatives. Corrected after 2 probes.
- `propagate_taint_for_binding` (a `.rules` prior) does not exist in the
  codebase → F8 (stale rule). This is itself a Good-Regulator finding.
- No `unwrap()` in `kask/crates/` library code (only in `#[cfg(test)]`) — the
  `.rules` "no unwrap" trap is honored. Reduced the formal-bug surface.

**Grounded claims:** 10/10 findings cite a verbatim code snippet + file:line
verified via `read_file` or `grep`. 0 unverified claims.

### Step 2 — Establish target condition

**Target:** Every proposed canonical pattern has (a) ≥2 production callers
grep-verified, (b) a falsifiable deletion test, (c) essentialist 3-gate PASS.
Additionally: every cybernetic impedance is tagged with exactly one of the 5
operational categories; grill-me verdict recorded for the top 5; no upstream
file edited.

### Step 3 — Prediction

**Prediction:** Of the 10 findings, the top 5 (F1, F2, F4, F3, F5) will all
survive grill-me's Mechanism and Rationale levels. F8 (stale `.rules`) will
survive but at LOW severity. F9 and F10 will be deferred (survive Mechanism but
are low-impact). No finding will be retracted.

**Confidence:** 0.85 — high because every finding is grounded in a verbatim
snippet and the `.rules` traps are explicit; residual uncertainty is whether
grill-me's Edge-Cases level surfaces a counterexample I missed for F4 (the
relative-path divergence).

### Step 4 — Experiment (run grill-me on the top 5)

Grill-me executed across Recall → Mechanism → Rationale → Edge Cases →
Synthesis. Results recorded in `findings.md` "grill-me verdict" table.

**Outcome:** All 5 survived. F4's edge cases (relative path, empty string,
symlink, unset var) were checked and held. 0 retractions.

### Step 5 — Measure gap + Brier score

**Object-space gap:** 0/3 patterns missing callers (P1: 2, P2: 3+4, P3: 2
existing + 3 adopters). 0/3 patterns missing deletion tests. 0/3 patterns
failed essentialist. → object gap = 0.0.

**Process-space gap:** 0/10 findings missing file:line. 0/10 findings missing
impedance tag (formal findings tagged `formal`). 1/10 (F8) is a `.rules` edit
that cannot be applied inline per `.rules` hygiene → residual process gap =
0.1 (the proposal is recorded but not actioned). 0 upstream files edited. →
process gap = 0.1.

**Hypotenuse:** sqrt(0.0² + 0.1²) = 0.1.

**Brier score for the prediction:**
- Predicted P(survive) = 0.85 for the top 5; actual = 1.0 (all survived).
- Predicted P(retraction) = 0.0; actual = 0.0.
- Predicted P(F9/F10 deferred) = 1.0; actual = 1.0.
- Brier = mean((0.85-1.0)², (0.0-0.0)², (1.0-1.0)²) = mean(0.0225, 0, 0) = 0.0075.

A Brier score of 0.0075 is well-calibrated (lower is better; 0 = perfect). The
prediction was slightly underconfident — I predicted 0.85 survival probability
and all 5 survived, indicating I could have predicted 0.95. The underconfidence
is conservative-safe for a review (better to under-predict survival and be
surprised than over-predict and retract).

### Step 6 — Convergence verdict

**gap (0.1) < epsilon (0.15)** → converged. The residual 0.1 is the F8
`.rules`-hygiene constraint (cannot be actioned inline), not a knowledge gap.
Brier is calibrated (0.0075). Cauchy-stable: a re-run would produce the same
findings (all grounded in verbatim snippets).

**Kata iterations used:** 1 (converged on first experiment — the review was
grounded enough that no re-probe was needed).

---

## Part B — Refactor-architecture plan (plan phase only; no execution)

### Friction discovered (ra-explore)

1. **Duplicated sensor locator** (`sensor_provider.rs`) — 2 byte-identical fns,
   both carrying the same broken-sensor bug. Friction signal: a fix must be
   applied twice; divergent evolution already latent.
2. **Duplicated path-resolution fallback chain** (`agent_paths.rs`) — 2 fns,
   divergent rules, 7 production callers across 4 crates. Friction signal:
   callers must know which helper to call to get the right `HKASK_DATA_DIR`
   semantics; the divergence is silent.
3. **Inconsistent sense-input error handling** (`registry_sqlite.rs` vs
   `tool_stats.rs`) — the canonical warn-then-fallback pattern exists in one
   crate but is absent in another. Friction signal: no shared convention;
   each author re-decides whether to warn.

### Deepening candidates (ra-candidates), ranked

| Rank | Candidate | Files | Problem | Solution | Strength |
| --- | --- | --- | --- | --- | --- |
| 1 | `latest_run_metrics` extraction | `sensor_provider.rs` | F1/F2/F3 | P1 — extract locator fn returning `Result` | Strong — fixes 2 HIGH bugs + duplication, 2 callers, low blast radius |
| 2 | `resolve_under_data_dir` delegation | `agent_paths.rs` | F4 | P2 — delegate to `resolve_data_dir` | Strong — fixes 1 HIGH bug, 7 callers, single crate |
| 3 | Sense-input warn convention adoption | `registry_sqlite.rs` | F5/F6/F7 | P3 — apply `read_count_field` pattern inline | Worth exploring — 3 MEDIUM bugs, no new abstraction |

**Top recommendation:** Candidate 1. It fixes two HIGH-severity broken-sensor
impedances (the regulation loop going blind on DB outage) *and* eliminates the
duplication in one move. The blast radius is a single file, and the deletion
test is falsifiable (inline → bug reappears in both sensors).

### Strangler-fig migration plan (ra-strangle, plan only)

**Domain:** regulation-loop metric sensing. One domain per commit.

**Commit 1 — Extract `latest_run_metrics` (P1).**
1. Write a failing test: `latest_run_metrics_returns_err_on_unreadable_dir`
   (chmod 000 the trace dir; assert `Err(MetricsLocateError::TraceDirInaccessible)`).
2. Write a failing test: `latest_run_metrics_returns_err_on_metadata_failure`
   (mock or fixture; assert `Err(MetadataUnavailable)`).
3. Implement `latest_run_metrics` returning `Result<Option<PathBuf>, MetricsLocateError>`.
4. Refactor `TestCoverageSensor::sense` and `MutationScoreSensor::sense` to call
   it; on `Err`, `tracing::warn!` and propagate `None` (or change `Sensor::sense`
   signature if the trait is widened — deferred per R1).
5. Delete the two duplicated `latest_metrics_path` bodies.
6. Verify: `./script/clippy`, `cargo test -p hkask-regulation`.

**Commit 2 — Delegate `resolve_under_data_dir` (P2).**
1. Write a failing test: `resolve_under_data_dir_honors_relative_hkask_data_dir`
   — set `HKASK_DATA_DIR=foo`, assert `resolve_under_data_dir(Path::new("x"))`
   returns `foo/x` (matching `resolve_data_dir`'s rule) *or* both return
   `$XDG/hkask/foo/x` — the test pins whichever rule is chosen, eliminating the
   divergence. (Decision: keep `resolve_data_dir`'s "absolute-or-dot" rule as
   the single regulator; a relative `HKASK_DATA_DIR` is likely a misconfig.)
2. Implement `resolve_under_data_dir(relative) = resolve_data_dir().join(relative)`.
3. Move the CWD-fallback `warn!` into `resolve_data_dir`'s CWD arm.
4. Verify: `./script/clippy`, `cargo test -p hkask-types`, and the
   `kata_kanban_allowlist_matches_actual_reads` test in `kask_bridge` (it
   references `resolve_under_data_dir` semantics).
5. Run `bash kask/scripts/check-hkask-no-zed-deps.sh` (§13.1 invariant).

**Commit 3 — Adopt the warn-on-malformed-sense-field convention (P3).**
1. In `registry_sqlite.rs`, add `tracing::warn!` to the three early-return arms
   in `count` (L246), `query_skills` (L517, L521, L541), and `get_skill_owned`
   (L507, L511 — split `NotFound` from `Io`).
2. Write tests pinning the warn: `count_warns_on_query_row_failure`,
   `query_skills_warns_on_prepare_failure`, `get_skill_owned_distinguishes_not_found_from_io`.
3. Verify: `./script/clippy`, `cargo test -p hkask-templates`.

**Not planned (deferred):**
- R1 (trait-shape change to `Sensor`) — defer until a third broken-sensor appears.
- F8 (`.rules` staleness) — propose the strike in the PR description for
  Commit 1 or 2; do not edit `.rules` inline.
- F9/F10 — low-impact; the existing `warn!` already closes the observability loop.

### Verification (ra-verify, to run after execution — not in this task)

- Dependency direction: `kask/scripts/check-hkask-no-zed-deps.sh` (P2 touches
  `hkask-types`, the foundation crate).
- Depth test: inline each extracted fn → complexity reappears (P1, P2).
- P6/P7/P8: no stubs, no deprecation, tests verify behavior (the failing tests
  in each commit pin the fix).
- `./script/clippy` (per `.rules` — not `cargo clippy`).
- `cargo test -p hkask-regulation -p hkask-types -p hkask-templates -p kask_bridge`.

---

## Change log

**Skills run (in declared order):**
1. bug-hunt — primary; 3 charters/crate max; static baseline + dynamic expansion;
   no-fiction citation gate enforced (every finding has file:line + verbatim snippet).
2. graph-audit — dual mode; 2 query passes (duplication grep, dead-surface grep);
   seeded F3 (latest_metrics_path duplication) and F4 (resolve_*_data_dir duplication).
3. idiomatic-rust — lens; type-driven fix proposals per finding (Result-returning
   sensors, delegation, warn-then-fallback convention). Compiler grounding via
   `read_file`/`grep` (no LSP code-action tool available in this session).
4. pragmatic-cybernetics — lens; each impedance tagged with exactly one of the 5
   operational categories (broken-sensor ×5, silent-failure ×1, variety-deficit ×1,
   loop-not-closed ×1, good-regulator ×2).
5. essentialist — gate; 3 patterns PASS (P1, P2, P3), 2 rejected (R1, R2).
6. grill-me — adversarial; top 5 interrogated; 0 retractions.
7. refactor-architecture — plan phase only; 3 deepening candidates ranked,
   strangler-fig plan for 3 commits, no execution.
8. metacognition — Kata run on the review; gap 0.1 < epsilon 0.15; Brier 0.0075;
   converged on iteration 1.

**Gas consumed:** ~62,000 of the 1,200,000 hard cap (analysis-only; no manifest
cascade executed). Well within bounds.

**grill-me verdict:** All top 5 (F1, F2, F4, F3, F5) survived Mechanism and
Rationale. 0 retractions.

**Retractions:** None from the top 5. F9 and F10 were deferred (not retracted —
they survive Mechanism but are low-impact; the existing `warn!` closes the
observability loop).

**Residual risks:**
- F8 (stale `.rules` `propagate_taint_for_binding` entry) cannot be fixed inline
  per `.rules` hygiene; requires a PR-description proposal.
- The `kask_bridge` `generate_stream` `model_override: None` hardcode (L582) is
  documented and pinned but a future caller of `generate_stream_with_model` on
  this port will silently lose the override via the default trait impl — worth a
  follow-up test.
- No `./script/clippy` or `cargo test` was run (analysis + planning task, not
  execution). The proposed code in `canonical-patterns.md` is illustrative; the
  actual commits must run clippy + tests per the verification section.

**Upstream files edited:** None. No D-seam file was modified. F8 proposes a
`.rules` edit via PR description only (per `.rules` hygiene).
