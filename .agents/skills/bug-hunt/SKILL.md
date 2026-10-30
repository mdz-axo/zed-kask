---
name: bug-hunt
core: true
description: "Exploratory bug hunting: explores a target crate for threats to user-defined quality using heuristic test strategy and exploratory testing charters."
audience: user
---

# Bug Hunt

Bug hunting: explores a target crate for threats to user-defined quality. Applies Weinberg's quality definition ("value to some person who matters"), Beizer's bug taxonomy, Bach/Bolton's heuristic test strategy model, and Hendrickson's exploratory testing charters. Decomposed into phased templates: Charter (with Good Regulator crate modeling + prior-expedition feedback) → Probe (with prediction-then-probing calibration + dynamic pattern expansion + missing-tests detection + algedonic escalation) → Oracle (with reproducibility separated from confidence, file:line citation enforcement, and Beizer taxonomy/severity/pattern_signature/fix_suggestion folded in, followed by `BH-1` `lisp_eval` summary aggregation) → Report (with prediction reconciliation + lessons_learned + pattern_signatures + next_charter_focus for loop closure) → Convergence (new findings by location across expeditions, computed by `lisp_eval`). Reasoning patterns from pragmatic-semantics (IS/OUGHT + epistemic classification + an evidence-type provenance adaptation) and grill-me are embedded as inline prompt instructions in the oracle phase; pragmatic-cybernetics (Good Regulator crate modeling in the charter; Ashby variety expansion + VSM algedonic escalation in the probe) and diagnose (reproduce before diagnosing, isolate one variable — probe) in their own phases.

## Reference models

Weinberg, *Quality Software Management, Vol. 1: Systems Thinking* (1992); Beizer, *Software Testing Techniques*, 2nd ed. (1990); Bach & Bolton, *Heuristic Test Strategy Model* (2006); Hendrickson, *Explore It!* (2013); Jia & Harman, "An Analysis and Survey of the Development of Mutation Testing", *IEEE TSE* 37(5) (2011); Barr et al., "The Oracle Problem in Software Testing: A Survey", *IEEE TSE* 41(5) (2015). `onto_anchor` → derived `exploratory_testing`, `test_oracle`, `mutation_testing`, `bug_taxonomy`, `good_regulator_theorem`.

## Initial and target condition

- **Initial condition:** the target crate, the user-defined quality criteria, the observed crate model and static baseline, plus any prior expedition or mutation report supplied for this run.
- **Target condition:** after at least two and at most ten expeditions, the convergence form reports no new confirmed-or-potential file:line locations and zero malformed contract/location findings. A nonzero defect count returns those findings to the oracle; new locations feed the next charter. Bound exhaustion reports the remaining coverage and escalations instead of implying bug absence.

## When to Use

- When exploring a target crate for threats to user-defined quality criteria.
- When applying Weinberg's quality definition, Beizer's bug taxonomy, Bach/Bolton's HTSM, and Hendrickson's exploratory testing charters.
- When decomposing bug hunting into phased templates: Charter → Probe → Oracle → Report → Convergence.
- When needing a Good Regulator crate model to drive Beizer category selection (rather than generic prevalence).
- When closing the feedback loop across expeditions: charter consumes prior_expedition (lessons_learned, pattern_signatures, next_charter_focus).
- When needing dynamic pattern expansion (Ashby's Law) beyond a static bug-pattern baseline.
- When needing missing-tests detection (Weinberg: absent tests = quality threat) and algedonic escalation for critical findings.
- When needing oracle verdicts that separate reproducibility from confidence and enforce file:line citation (no-fiction).
- When needing an honest convergence signal: new findings by location across expeditions, with next_charter_focus emission.

## When NOT to Use

- Verifying a specific known bug — use `diagnose` (reproduce → hypothesize → instrument → fix); bug-hunt is exploratory and charter-driven.
- Reviewing a change against its stated spec — use `code-review`.
- Proving the absence of bugs — an expedition reports what it found with honest coverage estimates; a clean expedition is a coverage statement, not a proof.

## D/P labelling

Charter design, crate-model description, the probe step-0 prediction (`predicted_findings` + `predicted_category`), probe strategy, and oracle classification (Weinberg quality, IS/OUGHT, epistemic mode, provenance, Beizer taxonomy `beizer_category`, `severity`, `pattern_signature`, and `fix_suggestion`) are P — judgment, critiqued by the operator and by the grill-me self-challenge at the oracle gate (the reproducibility axis is a label, not a critic — it does not downgrade confidence). Tool outputs are D: `cargo test --no-run` and missing-tests detection, BugStalker runtime probes, and the file:line no-fiction check (a finding without a location is rejected, and the convergence form's location channel counts it). The oracle summary aggregation (`BH-1` `lisp_eval` computing `confirmed_bugs`, `potential_bugs`, `contract_gaps`, `rejected_findings` count, and `by_category` / `by_severity` counts), the report's prediction reconciliation (`lisp_eval` count gap, report step 0), and the convergence computation (`lisp_eval`: new confirmed-or-potential locations vs the prior expedition by file:line, plus the verdict/contract/location defect count) are D.

## Instructions

### bug-hunt-charter

> **Pre-compute (step 0, use `grep` and `read_file` — no template):** before the charter, use `grep` to scan for dead-code patterns (unused `pub` items, functions with no callers) and complexity hotspots (deeply nested conditionals, long functions). The `precomputed_analysis` feeds this step's Beizer category selection and probe planning.

1. Build a lightweight `crate_model` first (Good Regulator compliance — Conant-Ashby): read `Cargo.toml`, `lib.rs`/`main.rs`, and module structure; describe architecture, data_flow, critical_paths, dependency_surface, and observed_characteristics (async, unsafe, trait_objects, concurrency, ffi, macros, proc_macros).
2. If `prior_expedition` is present, consume it: distill `lessons_learned` into 1-3 probe-strategy adjustments, extend the probe pattern list with `pattern_signatures`, and (if present) make `next_charter_focus` the primary `target_area` unless already exhausted.
3. If `mutation_report` is present (e.g. from a `cargo mutants` or `cargo-mutants`-style run supplied by the operator), prioritize `target_area` toward functions with surviving mutants — those are the concrete locations where the test suite is blind. Mutation testing finds syntactic blind spots (the suite doesn't notice `+` → `-`); bug-hunt finds semantic blind spots (the suite doesn't test the error path, the race condition). Mutation-guided chartering focuses exploratory probing exactly where the suite is weakest.
4. Generate a Hendrickson-format charter: "Explore [target] using [strategy] to discover [quality threat]."
5. Pick the most promising strategy from Bach's HTSM (Project Environment, Product Elements, Quality Criteria), justified against the crate model — not generic categories.
6. Select 2-3 Beizer categories given the crate model and quality criteria (e.g., heavy async usage → `timing` overrides `requirements` regardless of generic prevalence).
7. Emit probe_instructions that are actionable with available MCP tools.
8. Respond with a JSON object containing `charter_statement`, `target_area`, `strategy`, `expected_category`, `beizer_focus`, `crate_model`, `probe_instructions`, and `prior_feedback_consumed`.

### bug-hunt-probe

0. **Predict before probing (calibration, P).** From the charter's `crate_model` alone, state the expected number of confirmed findings and the expected top Beizer category, with one sentence of basis. Record them before the first probe; the report reconciles them.
1. Format probe execution results. Do NOT generate fictional findings — only report what the agent actually discovered through MCP tool usage.
2. If no `probe_findings` are provided, explore the target with available MCP tools (`read_file`, `grep`, `terminal`) using the charter's `probe_instructions`.
3. Run the static baseline pattern search (floor, not ceiling): `.unwrap()`/`.expect()` in library code, public functions without contracts, `unsafe` without documented safety invariants, integer arithmetic without overflow protection, `clone()` hiding ownership confusion, mutable state without synchronization, `panic!`/`todo!()` in non-startup code.
4. Apply dynamic pattern expansion (Ashby's Law): use `crate_model.observed_characteristics` to generate crate-specific patterns (async lock patterns, unsafe soundness, trait-object assumptions, concurrency ordering, FFI boundary, macro hygiene, proc-macro error emission). If `crate_model` is sparse, infer characteristics from the source read in step 3.
5. Detect missing tests (Weinberg): run `cargo test --no-run`, search for test modules, and check whether each `crate_model.critical_paths` function has a corresponding test. Record absence as a finding and shift strategy toward static analysis if dynamic probing has nothing to run.
6. Emit `probe_escalation` entries (VSM S1→S5 short-circuit) for critical findings: `unsafe` with likely UB, `panic!`/`unwrap`/`expect` in `fn main()` or `Drop` impls, security-relevant contract violations, data-loss paths without recovery. Each entry includes `finding_summary`, `location` (file:line), `severity` (CRITICAL), and `reason`. The agent surfaces these entries to the user IMMEDIATELY after the probe phase — before oracle/report run — which is the short-circuit; the `[ESCALATION]`-prefixed findings still flow into the oracle as ordinary findings.
7. Apply diagnose pattern (reproduce before diagnosing, isolate one variable), adversarial pattern (unexpected input orders, boundary values), and cybernetic pattern (trace feedback loops, Good Regulator check, variety check).
8. If `probe_depth` is `dynamic` or `full` and the charter targets timing/integration/structural categories, use BugStalker (`bs`) on Linux x86-64 debug builds (with `--oracle tokio` for async). Probe patterns: async deadlock, race condition, state machine violation, memory corruption, poisoned mutex, unexpected polling. Record binary path, breakpoint locations, observed states, and whether behavior matches expectations.
9. Collect all findings as structured text (`probe_findings`), alongside `predicted_findings`, `predicted_category`, and `probe_escalation`. Prefix escalations with `[ESCALATION] <summary> at <file:line> — <reason>` so downstream phases can route them.

### bug-hunt-oracle

1. Evaluate raw probe findings against user-defined quality criteria using the Weinberg oracle (bug = threat to user-defined quality).
2. Assign a tier, confidence, AND reproducibility to each finding: Tier 1 BUG (0.90–1.00), Tier 2 POTENTIAL_BUG (0.60–0.89), Tier 3 OBSERVATION (<0.60).
3. Label reproducibility as `reproduced`, `reproducible`, `hard_to_reproduce`, or `not_reproducible`. Reproducibility does NOT downgrade confidence — a `hard_to_reproduce` finding with confidence 0.85 stays POTENTIAL_BUG (this corrects the prior conflation that biased against timing/concurrency/UB classes).
4. Apply IS vs OUGHT classification: describe what the code DOES (IS) vs what it SHOULD do (OUGHT). Never present OUGHT as IS.
5. Label the epistemic mode for every finding (Declarative, Probabilistic, Subjunctive). A subjunctive statement presented as declarative is a false positive.
6. Identify the provenance of the finding (Direct measurement, Inference, Assessment). Flag LLM-assessed conclusions.
7. Challenge your own verdict using grill-me self-challenge: Could this be intentional? Is there an edge case where this is correct? Would a reviewer dismiss this? If confidence < 0.80, state what would increase it.
8. Classify each evaluated finding into exactly one Beizer category (`requirements`, `structural`, `data`, `coding`, `interface`, `integration`, `timing`, `configuration`), assign `severity` (`CRITICAL`, `HIGH`, `MEDIUM`, `LOW`) justified by the evidence, and emit a concrete `pattern_signature` (grep-able or structural) and `fix_suggestion` (`"needs investigation"` when not obvious).
9. Enforce no-fiction at the gate: every finding MUST include `location.file`, `location.line_approx`, and a verbatim `evidence` snippet (≤5 lines). Findings without citations are REJECTED and recorded in `rejected_findings` as `{"id": "...", "reason": "missing_citation"}`.
10. Respond from the template with `evaluated_findings` and `rejected_findings` (array of `{id, reason}`).
11. **Deterministic oracle summary (`BH-1`, D — `lisp_eval`):** after `bug-hunt-oracle.j2` returns `evaluated_findings` and `rejected_findings`, compute `confirmed_bugs`, `potential_bugs`, `contract_gaps`, `rejected_findings` count, and the `by_category` and `by_severity` counts (`oracle_summary`) with `lisp_eval`:
    - form: `(begin (define count-field (lambda (fs k v) (if (is_null fs) 0 (+ (if (string= (assoc k (car fs)) v) 1 0) (count-field (cdr fs) k v))))) (list (list "confirmed_bugs" (count-field evaluated_findings "verdict" "BUG")) (list "potential_bugs" (count-field evaluated_findings "verdict" "POTENTIAL_BUG")) (list "contract_gaps" (count-field evaluated_findings "verdict" "OBSERVATION")) (list "rejected_findings" (length rejected_findings)) (list "by_category" (list (list "requirements" (count-field evaluated_findings "beizer_category" "requirements")) (list "structural" (count-field evaluated_findings "beizer_category" "structural")) (list "data" (count-field evaluated_findings "beizer_category" "data")) (list "coding" (count-field evaluated_findings "beizer_category" "coding")) (list "interface" (count-field evaluated_findings "beizer_category" "interface")) (list "integration" (count-field evaluated_findings "beizer_category" "integration")) (list "timing" (count-field evaluated_findings "beizer_category" "timing")) (list "configuration" (count-field evaluated_findings "beizer_category" "configuration")))) (list "by_severity" (list (list "CRITICAL" (count-field evaluated_findings "severity" "CRITICAL")) (list "HIGH" (count-field evaluated_findings "severity" "HIGH")) (list "MEDIUM" (count-field evaluated_findings "severity" "MEDIUM")) (list "LOW" (count-field evaluated_findings "severity" "LOW"))))))`
    - env: `{ "evaluated_findings": <array>, "rejected_findings": <array> }`

### bug-hunt-report

0. **Reconcile the prediction (D).** Report `predicted_findings` / `confirmed_findings` (`oracle_summary.confirmed_bugs + oracle_summary.potential_bugs`) and the predicted vs observed top Beizer category from probe step 0; compute the count gap with `lisp_eval` `(- confirmed_findings predicted_findings)`. The gap is the charter's calibration signal for the next expedition, not a finding.
1. Compile `charter`, `evaluated_findings`, and `oracle_summary` into a structured JSON bug report.
2. Carry `evaluated_findings` directly into `findings`; each finding already includes `id`, `summary`, `location`, `verdict`, `confidence`, `reproducibility`, `beizer_category`, `severity`, `evidence`, `pattern_signature`, and `fix_suggestion`.
3. Populate `summary` (`confirmed_bugs`, `potential_bugs`, `contract_gaps`, `rejected_findings`, and `taxonomy_coverage`) directly from `oracle_summary` (`BH-1`).
4. Emit `lessons_learned` — concrete, actionable lessons the next expedition's charter should consume (e.g., "async lock held across .await at 3 sites; next charter should target all .await points in lock scope"). Not generic platitudes.
5. Emit `pattern_signatures` — derived from actual findings (signature, beizer_category, derived_from, notes on how to apply in the next probe). Not fabricated. Emit `next_charter_focus` — the one-sentence directive the next expedition's charter consumes as its primary target_area.
6. Note whether this is a first pass or iteration N+1 (consumed `prior_expedition`).
7. Write the expedition report to `~/Documents/zk-data/skills/bug-hunt/{date}-{run}/bug-hunt-report.json` via `terminal` (built-in file tools cannot write under `~/Documents/zk-data`). The report is the durable record of the expedition — later expeditions consume it via `prior_expedition`, and an operator (or a future suite-level proposer) can read it to target test gaps.
8. Respond with the complete expedition report as JSON.

## Relationship to Other Skills

- **tdd**: builds or fixes one behavior test-first at an agreed seam. Bug-hunt explores for unknown bugs across a crate. A confirmed finding returns to tdd as a new red test.
- **diagnose**: when bug-hunt finds a confirmed bug, the `evidence` and `location` fields are a pre-minimized reproducer for diagnose's Phase 2.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `bug-hunt-charter.j2` | Generate a focused bug-hunt charter using Hendrickson format and Bach's Heuristic Test Strategy Model (HTSM). Builds a lightweight crate_model first (Good Regulator compliance) and consumes prior_expedition when present (feedback loop closure). Selects Beizer taxonomy categories based on the crate model rather than generic prevalence. |
| `bug-hunt-probe.j2` | PROBE phase — agent-coordinated MCP tool execution. Reads target source files, searches for bug patterns, runs cargo check/test/clippy, and when probe_depth is `dynamic` or `full`, executes BugStalker (`bs`) runtime probes (breakpoints, thread inspection, Tokio oracle) for timing, integration, and structural Beizer categories. Dynamic pattern expansion based on crate_model.observed_characteristics (Ashby's Law), missing-tests detection (Weinberg), and algedonic escalation for critical findings (VSM S1→S5 short-circuit). This template provides guidance and collects results; the actual probing is performed by the agent using MCP tools between template calls. |
| `bug-hunt-oracle.j2` | Apply Weinberg oracle (bug = threat to user-defined quality), pragmatic-semantics IS/OUGHT classification, epistemic mode labeling, provenance tracing, grill-me self-challenge, Beizer taxonomy classification (`beizer_category`), severity rating (`CRITICAL`/`HIGH`/`MEDIUM`/`LOW`), `pattern_signature`, and `fix_suggestion` to raw probe findings in one pass. Reproducibility is a separate axis from confidence (a high-confidence low-reproducibility finding stays POTENTIAL_BUG, not OBSERVATION), and findings without file:line citation are rejected into `rejected_findings` (`{id, reason}`). Aggregate counts and taxonomy summaries (`oracle_summary`) are computed deterministically by `BH-1` `lisp_eval`. |
| `bug-hunt-report.j2` | Compile charter, `evaluated_findings`, and `oracle_summary` (`BH-1`) into a structured JSON bug report written to `~/Documents/zk-data/skills/bug-hunt/{date}-{run}/bug-hunt-report.json` via `terminal`. Emits `lessons_learned`, `pattern_signatures`, and `next_charter_focus` fields that the next expedition's charter consumes to close the feedback loop. |


To render a template, call the `render_template` tool with the template ref (e.g., `bug-hunt/bug-hunt-charter`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `bug-hunt-charter.j2`: `target`, `quality_criteria`, `prior_expedition`, `mutation_report`, `precomputed_analysis`, `crate_model`
- `bug-hunt-probe.j2`: `charter`, `probe_findings`, `probe_depth`, `files_read`, `tests_run`, `probes_executed`, `crates_examined`
- `bug-hunt-oracle.j2`: `charter`, `probe_findings`, `quality_criteria`
- `bug-hunt-report.j2`: `charter`, `evaluated_findings`, `oracle_summary`, `quality_criteria`, `probes_executed`, `files_read`, `tests_run`, `crates_examined`, `prior_expedition`, `predicted_findings`, `predicted_category`

## Regression case

All receipts executed live through `lisp_eval` (2026-10-01, backfill pass),
over the convergence form:

- Clean: one prior location, one current location, one well-formed BUG
  finding → `[[], 0]` — no new locations, no malformed findings.
- Defect: a new location ("cache") plus a BUG finding missing its
  location → `[["cache"], 1]` — both defect channels (new unexplored
  locations; findings missing contract or location) named in one pass.

The skill's forms are executed at use time, never anchored in code.

## Constraints

- `bug-hunt-charter.j2`: Beizer category selection must be justified against the `crate_model`, not generic prevalence. When `prior_expedition` is present, the charter MUST consume it — silent ignoring is a feedback-loop violation.
- `bug-hunt-probe.j2`: Do not generate fictional findings. The static pattern list is a floor, not a ceiling — dynamic expansion is required when `crate_model.observed_characteristics` are present. Missing tests must be recorded as a finding, not a silent no-op. Critical findings must emit `probe_escalation` entries.
- `bug-hunt-oracle.j2`: Every finding must carry IS/OUGHT, epistemic mode, provenance, reproducibility, `beizer_category`, `severity`, `pattern_signature`, AND `fix_suggestion` fields. Every finding has exactly one Beizer category; severity must be justified by evidence; pattern signatures must be concrete (grep-able or structural), not vague; do not fabricate fix suggestions (`"needs investigation"` is valid). Every finding must cite a concrete file:line and include a verbatim code snippet (≤5 lines); uncited findings are rejected into `rejected_findings` (`{id, reason}`), not silently dropped. Reproducibility does NOT downgrade confidence. If confidence < 0.60, the finding is an OBSERVATION. `BH-1` `lisp_eval` computes `oracle_summary`.
- `bug-hunt-report.j2`: Each finding must include all required fields. Summary counts come from `oracle_summary` (`BH-1`), including `rejected_findings`. `lessons_learned` must be concrete and actionable; `pattern_signatures` must be derived from actual findings, not fabricated. Write the report to `~/Documents/zk-data/skills/bug-hunt/{date}-{run}/bug-hunt-report.json` via `terminal`.
- **Convergence (D over P findings):** after each expedition, compute with `lisp_eval` (1) the new confirmed or potential findings, compared by `location` (file:line) against the prior expedition's — not by `pattern_signature`, which is free text and changes with rewording; and (2) the no-fiction and tier-consistency defects — a Tier 1 BUG without a cited contract, or any finding without a location:
  - form: `(begin (define new-locs (lambda (cur prior) (if (is_null cur) (quote ()) (if (member (car cur) prior) (new-locs (cdr cur) prior) (cons (car cur) (new-locs (cdr cur) prior)))))) (define bad (lambda (fs) (if (is_null fs) 0 (+ (if (and (string= (assoc "verdict" (car fs)) "BUG") (is_null (assoc "contract" (car fs)))) 1 0) (if (is_null (assoc "location" (car fs))) 1 0) (bad (cdr fs)))))) (list (new-locs current_locations prior_locations) (bad findings)))`
  - env: `{ "current_locations": [<file:line of this expedition's CONFIRMED or POTENTIAL findings only — OBSERVATIONs are excluded from the convergence signal>], "prior_locations": [<file:line from prior_expedition>], "findings": [<oracle findings with verdict, contract, location>] }`
  A defect count above 0 sends the offending findings back to the oracle. Converged when an expedition after the first adds no new locations. The process metrics once named here (`process_stabilization`, `coverage_estimate`) are not computed by any step; report `taxonomy_coverage` from the report instead. Bound: minimum 2 expeditions, maximum 10 (the operator can set a lower cost budget); escalate if not converged. The tier boundaries follow the oracle's categorical criteria (a cited contract for Tier 1); the confidence number is P and is not re-checked.
