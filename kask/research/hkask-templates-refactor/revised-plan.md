# Revised Refactor Plan — `hkask-templates`

> Multi-perspective review of the `refactor-architecture` pass against
> `kask/crates/hkask-templates/`. Six review skills ran in sequence
> (essentialist → pragmatic-semantics → pragmatic-cybernetics →
> idiomatic-rust → metacognition → grill-me). This is the synthesized
> revised plan after applying the minimal-satisfiability rule: the smallest
> set of revisions that addresses all surviving Prohibition findings. No
> gold-plating; the larger CAND-* candidates from the prior draft remain
> deferred for operator verification.

## 1. Revised refactor plan (applied findings)

| ID | Constraint force | File:line | Change | Rationale |
|----|------------------|-----------|--------|-----------|
| A1 | Prohibition | `kask/crates/hkask-templates/src/registry_sqlite.rs:333-348` (`SkillRegistryIndex::register_skill`) | Replaced `if let Err(e) = conn.execute(...) { tracing::error!(...); } Ok(())` with `conn.execute(...).map_err(\|e\| { tracing::error!(...); RegistryError::Other(...) })?; Ok(())` | The silent `Ok(())` on INSERT failure was a broken feedback loop: the sole caller (`skill_loader.rs:136-140`) already had an `if let Err(e)` arm that pushed a warning, but that arm never fired because the SQL error was swallowed inside the registry. Callers cannot distinguish "succeeded" from "failed silently" — the `.rules` `unwrap_or(0)` trap generalized to write paths. |
| A2 | Prohibition | `kask/crates/hkask-templates/src/registry_sqlite.rs:369-380` (`SkillRegistryIndex::remove_skill`) | Replaced `if let Err(e) = conn.execute("DELETE ...") { tracing::error!(...); } Ok(skill)` with `conn.execute(...).map_err(\|e\| { tracing::error!(...); RegistryError::Other(...) })?; Ok(skill)` | Same broken-loop pattern as A1. A failed DELETE returned `Ok(Some(skill))`, lying to the caller that the skill was removed when it wasn't. |
| A3 | Prohibition | `kask/crates/hkask-templates/src/registry_sqlite.rs:385-418` (`BundleRegistryIndex::register_bundle`) | Propagated all four failure paths: (1) `serde_json::to_string` serialize failure → `TemplateError::Manifest`; (2) bundles INSERT failure → `TemplateError::Manifest`; (3) `bundle_skills` DELETE failure → `TemplateError::Manifest`; (4) per-skill `bundle_skills` INSERT failure → `TemplateError::Manifest`. Each `.map_err` retains the `tracing::error!` for operator visibility. | Four silent `Ok(())` returns on write failure. A bundle that failed to serialize, failed to INSERT, or failed to link its skills reported success — the registry drift would only surface later as a missing bundle. The `.rules` "advertised invariants must point to the enforcement line" — the `BundleRegistryIndex` trait's `Result<()>` return was a lie. |
| A4 | Prohibition | `kask/crates/hkask-templates/src/registry_sqlite.rs:491-511` (`BundleRegistryIndex::remove_bundle`) | Propagated both DELETE failures (`bundle_skills`, `bundles`) as `TemplateError::Manifest` with the `tracing::error!` retained. | Same broken-loop pattern. A failed DELETE returned `Ok(Some(bundle))`, claiming the bundle was removed when it wasn't. |
| A5 | Guardrail | `kask/crates/hkask-templates/src/registry_sqlite.rs:560-562` (`row_to_skill` visibility/zone parse) | Replaced `Visibility::parse_str(&visibility_str).unwrap_or(Visibility::Private)` with `.unwrap_or_else(\|\| { tracing::warn!(... skill_id, visibility_str, "unknown visibility string — defaulting to Private. A corrupted visibility column reads as Private; the operator cannot distinguish 'intentionally Private' from 'corrupted' without this warn."); Visibility::Private })`. Same for `SkillZone::parse_str`. | Security-relevant silent downgrade. A corrupted/malformed `visibility` column silently became `Private` instead of surfacing the parse failure. Per `.rules`: silent fallbacks on failure paths are broken feedback loops — the operator cannot distinguish "intentionally Private" from "corrupted data." The `warn!` closes the loop without changing the default (Private is still the safe fallback). |
| A6 | Prohibition | `kask/crates/hkask-templates/src/step_machine.rs:736-757` (`classify_failure_mode`) | Deleted the 16-arm `fn classify_failure_mode` and its `#[allow(dead_code)]` attribute. | Zero callers (not even tests). A 16-arm match retained for a future that hasn't arrived. The `#[allow(dead_code)]` + "in-process work — not yet wired" comment was honest about it being unwired, but the `.rules` "convention helpers with only test callers are dead code" generalizes to "helpers with zero callers are dead code." The essentialist deletion test: deleting it removes complexity that does not reappear across N callers (N=0). The `failure_mode` field that this classifier was meant to populate is already emitted as literal strings (`failure_mode = "timeout"`, etc.) at the `tracing::warn!` call sites in `step_actions.rs` and `step_machine.rs`. |

### Verification

- `cargo build --package hkask-templates` — clean.
- `cargo test --package hkask-templates` — all tests pass except one **pre-existing** failure (`all_mcp_references_point_to_known_tools`, a `gemba-walk` manifest-vs-registry MCP drift unrelated to this change; verified failing on `main` before these edits via `git stash`).
- `./script/clippy --package hkask-templates` — clean, no unused deps.
- `cargo build --package hkask-mcp-kata-kanban` and `--package kask_bridge` — consumer crates build clean (the `SqliteRegistry` write-path signature changes are trait-compatible: the trait already declared `Result<(), RegistryError>` / `Result<(), TemplateError>`; the impls now honor those signatures instead of lying).

## 2. Rejected findings

| Finding (from prior draft) | Skill that rejected | Gate / reason |
|---------------------------|---------------------|---------------|
| B3 — `SqliteRegistry::count` returns 0 on error | pragmatic-cybernetics | `count` already emits `tracing::warn!` (SELECT path) and `tracing::error!` (pool-get path) on both failure branches. Per `.rules`, the `unwrap_or(0)` trap applies to *regulation-loop sense inputs*; `count` has zero production callers (test-only). The warn/error already satisfy the "emit `tracing::warn!`" alternative. Reclassified from Prohibition to non-finding. |
| D6 — `Effect::ConsumedGas(amount)` "silently dropped" | pragmatic-semantics | The prior draft's own metacognition pass corrected this: the `let _ = amount;` at `step_machine.rs:627` is a *documented no-op* (gas is charged per iteration by `charge_iteration` in the pass loop, not per-step via the effect). The variant is intentionally a no-op at the effect site. The dead-code question is whether the variant is ever *produced* — and it is not, but that's a separate finding (deferred to CAND-10 dead-surface removal, not a Prohibition). |
| CAND-1 through CAND-11 (the 11 candidate refactor packages) | essentialist (Exist gate) | Every candidate fails the Exist gate for *this pass*: the task scoped a single-pass review producing a structured findings list, with the larger candidates explicitly deferred to "Deferred for operator verification." Applying CAND-1..CAND-11 would be gold-plating beyond the minimal-satisfiability rule. They remain in §3. |

## 3. Deferred findings (Hypothesis-tier, for operator verification)

| ID | Finding | Deferral reason |
|----|---------|-----------------|
| H1 | **CAND-1** — Collapse the registry trait layer (`Registry` in-memory + 3 index traits → inherent `SqliteRegistry` methods). `Registry` has zero external consumers (test-only). | Hypothesis: the prior seam-audit (RA-01/09/10) deferred this and the deferral rationale was not re-checked. Operator must verify the deferral reasons are no longer load-bearing before deletion. Blast radius: in-crate tests using `Registry` as a fast in-memory mock. |
| H2 | **CAND-2** — Extract `run_sub_cascade(manifest, parent_context, infra) -> CascadeOutcome` to deduplicate the triplicated sub-cascade orchestration in `executor.rs:214-240`, `execute_flowdef`, `execute_parallel`. The `MAX_STEPS` hard gate is applied only in the executor path; the two sub-cascade paths get only the advisory warn. | Hypothesis: the three call sites have subtle differences (e.g. `parallel` constructs per-branch trackers) that the extraction must parameterize, not flatten. Operator must verify the parameterization is faithful before applying. |
| H3 | **CAND-3** — Split `step_actions.rs` (3,124 lines, largest module by 2×) along action seams into per-action modules behind a `step_actions/` facade. | Hypothesis: the split is mechanical but touches every `use crate::step_actions::*` site. Operator must verify the `impl StepMachine` split-across-files pattern (already a fact) is made navigable rather than accidental. |
| H4 | **CAND-4** — Decompose `dispatch_compute` god-function (830-line match, 17 arms) into sibling modules behind a `compute/` facade. | Hypothesis: the arms share JSON-destructuring helpers redefined per arm; extracting a shared `compute_input` helper is part of the work. Prior draft's line-count claim (1,270 prod lines) was unverified (actual: 3,379 total). Operator must re-measure before sizing. |
| H5 | **CAND-5** — Collapse `ManifestFile` into `BundleManifest` via `#[serde(flatten)]`. | Hypothesis: `#[serde(flatten)]` has known interactions with `deny_unknown_fields` and `serde_yaml_neo`. Needs a spike across all 30+ manifests before applying. Unfalsifiable until the spike runs. |
| H6 | **CAND-6** — Extract shared frontmatter/Jinja strippers (C1: byte-equivalent Jinja-comment stripper in `output_schema` + `inputs`; C2: 5-step frontmatter parser duplicated in two sub-keys). | Low-risk dedup; deferred only because it's not a Prohibition. Operator can apply independently. |
| H7 | **CAND-7** — Type-discriminate `BundleManifestStep` (18-field god-struct) into enum-of-structs. | Highest-risk candidate. Blast radius: every step construction site, every `StepNode` reader, every manifest YAML parse. Operator must measure blast radius (construction-site count + reader count) before deciding. |
| H8 | **CAND-8** — Split `ConvergenceTracker` into Kata + legacy variants. | Hypothesis: `ConvergenceConfig` carries both models' fields today; splitting the tracker may require splitting the config too (C8 duplication). Operator must verify the config split is tractable. |
| H9 | **CAND-9** — Fix remaining broken feedback loops in `SqliteRegistry` (B5-B9: `input_mapping` silent fallback, `output_schema` silent narrow, `skill_loader` default-to-KnowAct, `Registry::reload` partial clear, `template_renderer::load` doc contradiction). | These are Guardrail-tier (not Prohibition — they don't break regulation-loop sense inputs). Deferred because each is a behavior change that may need its own release note. Operator should triage per-finding. |
| H10 | **CAND-10** — Remove dead public surface (22 of 32 crate-root re-exports have zero external consumers; `PromptStrategy` module; `render_input_param_spec`/`extract_contract_input_keys`; `BudgetTracker::from_remaining`/`last_rjoule_cost`; `Effect::ConsumedGas` variant). | Hypothesis: some zero-consumer re-exports are exercised by in-crate contract tests that import via the crate root. Demoting them breaks those tests; deleting them removes the contract. Operator must run the per-item deletion test before applying. |
| H11 | **CAND-11** — Inline pass-through wrappers (`concurrency.rs` 13-line `pub use`; `bundle/cascade.rs` 23-line file; `render_step_template` 7-line pass-through; `apply_input_mapping` 12-line pass-through; `phase_str` one-line delegate; three `*_str()` methods on `BundleConflict`/`BundleComplementarity`; `execute_manifest` borrowed→owned pass-through). | Low-risk individually; collectively a large diff. Deferred only because it's not a Prohibition. Operator can bundle into one "pass-through sweep" commit. |
| H12 | **B10** — `step_graph::MAX_STEPS` advisory warn + executor hard gate is a dual-write (two enforcement points for one invariant). | Hypothesis: the dual-write is intentional defense-in-depth (the warn is for the flowdef sub-cascade path that doesn't hit the hard gate). Operator must decide whether to consolidate (one enforcement point) or document the intent. |

## 4. Change log

### Summary

Applied 6 findings (5 Prohibition, 1 Guardrail) across 2 files
(`registry_sqlite.rs`, `step_machine.rs`). All changes propagate errors that
were previously swallowed with `Ok(())` or silent `unwrap_or` defaults, closing
broken feedback loops in the `SqliteRegistry` write paths and the `row_to_skill`
parse path. Deleted one dead helper (`classify_failure_mode`). No public API
changes — the trait signatures already declared `Result<...>`; the impls now
honor them. No behavior change for callers that handled `Err` correctly (the
sole `register_skill` caller already had an `if let Err(e)` arm that now fires
on real failures instead of never firing).

### Per-skill findings count

| Skill | Findings emitted | Applied | Rejected | Deferred |
|-------|-----------------|---------|----------|----------|
| refactor-architecture (base) | 12 candidates + 11 friction items | — | — | 12 (all CAND-* + B10) |
| essentialist | 6 (one per applied finding, Exist gate) | 6 | 11 (CAND-* fail Exist for this pass) | 0 |
| pragmatic-semantics | 6 IS-verified, 1 correction (D6 reclassified) | 6 | 1 (D6) | 0 |
| pragmatic-cybernetics | 5 Prohibition (B1×4 write paths + B2 visibility), 1 non-finding (B3) | 5 | 1 (B3) | 0 |
| idiomatic-rust | advisory only — no overrides | 0 | 0 | 0 |
| metacognition | 6 calibrated (each applied finding has a measurable target: error propagates to caller's existing `Err` arm / `warn!` fires on corrupted data) | 6 | 0 | 0 |
| grill-me | 6 `pass`, 0 `rewrite_needed`, 0 `fail` | 6 | 0 | 0 |

### grill-me verdict tally

| Finding | Verdict | Notes |
|---------|---------|-------|
| A1 (`register_skill` error propagation) | `pass` | Caller's existing `if let Err(e)` arm now fires; no signature change. |
| A2 (`remove_skill` error propagation) | `pass` | Same pattern; trait signature unchanged. |
| A3 (`register_bundle` error propagation) | `pass` | Four failure paths now propagate; each `.map_err` retains the `tracing::error!`. |
| A4 (`remove_bundle` error propagation) | `pass` | Both DELETE failures now propagate. |
| A5 (`row_to_skill` visibility/zone warn) | `pass` | `warn!` closes the loop without changing the safe default (Private). |
| A6 (`classify_failure_mode` deletion) | `pass` | Zero callers; deletion removes complexity that does not reappear. |

### Residual risks

1. **`gemba-walk` MCP drift (pre-existing, unrelated).** `tests/manifest_load_validation.rs::all_mcp_references_point_to_known_tools` fails on `main` because the `gemba-walk` manifest references `curator_grounding_trend` and `curator_grounding_coverage` MCP tools that aren't in the known set. Verified via `git stash` that this predates these changes. Not caused by this revision; flagged for the operator.
2. **Caller behavior change for `register_bundle`/`remove_bundle`/`remove_skill`.** These trait methods now return `Err` on SQL failure where they previously returned `Ok(())`. No production callers exist outside the crate (verified: `grep` for `.register_bundle(`, `.remove_bundle(`, `.remove_skill(` finds only the trait definitions and the `skill_loader.rs` `register_skill` caller). If a future caller assumes `Ok(())`, it will now see the error — the desired behavior.
3. **Deferred CAND-* candidates remain unaddressed.** The 11 larger refactor candidates (registry trait collapse, sub-cascade extraction, `step_actions.rs` split, `dispatch_compute` decomposition, `ManifestFile` collapse, frontmatter dedup, `BundleManifestStep` type-discrimination, `ConvergenceTracker` split, remaining feedback-loop fixes, dead-surface removal, pass-through inlining) are documented in §3 but not applied. Each has a deferral reason; the operator should triage per-finding.
4. **`Effect::ConsumedGas` variant.** Confirmed produced by zero actions (grep for `Effect::ConsumedGas` finds only the variant definition and the `apply_effect` arm). Deferred to H10 (CAND-10 dead-surface removal) rather than applied here because removing it touches the `Effect` enum and the `apply_effect` match — a larger blast radius than the minimal-satisfiability rule permits for this pass.
