---
name: bug-hunt
visibility: public
description: "Bug hunting: explores a target crate for threats to user-defined quality. Applies Weinberg's quality definition, Beizer's bug taxonomy, Bach/Bolton's heuristic test strategy model, and Hendrickson's exploratory testing charters. Decomposed into phased templates (Charter → Probe → Oracle → Taxonomize → Report → Convergence) with inline reasoning patterns from pragmatic-semantics, pragmatic-cybernetics, diagnose, grill-me, and adversarial-red-team. Emits reg.bughunt.* spans. OCAP-gated: requires Tool:test:Execute and Tool:regulation:Read."
---

# Bug Hunt

Bug hunting: explores a target crate for threats to user-defined quality. Applies Weinberg's quality definition ("value to some person who matters"), Beizer's bug taxonomy, Bach/Bolton's heuristic test strategy model, and Hendrickson's exploratory testing charters. Decomposed into phased templates: Charter (with Good Regulator crate modeling + prior-expedition feedback) → Probe (with dynamic pattern expansion + missing-tests detection + algedonic escalation) → Oracle (with reproducibility separated from confidence + file:line citation enforcement) → Taxonomize → Report (with lessons_learned + pattern_signatures for loop closure) → Convergence (with honest process_stabilization + coverage_estimate sub-metrics + next_charter_focus). Reasoning patterns from pragmatic-semantics (IS/OUGHT + epistemic classification + provenance), pragmatic-cybernetics (feedback loop analysis + Good Regulator checks + variety engineering), diagnose, grill-me, and adversarial-red-team are embedded as inline prompt instructions in the oracle phase. Emits Regulation spans (reg.bughunt.*) for observability (P12). OCAP-gated: requires Tool:test:Execute and Tool:regulation:Read.


## When to Use

- When exploring a target crate for threats to user-defined quality criteria.
- When applying Weinberg's quality definition, Beizer's bug taxonomy, Bach/Bolton's HTSM, and Hendrickson's exploratory testing charters.
- When decomposing bug hunting into phased templates: Charter → Probe → Oracle → Taxonomize → Report → Convergence.
- When needing a Good Regulator crate model to drive Beizer category selection (rather than generic prevalence).
- When closing the feedback loop across expeditions: charter consumes prior_expedition (lessons_learned, pattern_signatures, next_charter_focus).
- When needing dynamic pattern expansion (Ashby's Law) beyond a static bug-pattern baseline.
- When needing missing-tests detection (Weinberg: absent tests = quality threat) and algedonic escalation for critical findings.
- When needing oracle verdicts that separate reproducibility from confidence and enforce file:line citation (no-fiction).
- When needing honest convergence metrics: process_stabilization + coverage_estimate, with next_charter_focus emission.
- When needing to run a legacy monolithic expedition template (v0.30.0 backward compatibility).

## Instructions

### bug-hunt-charter

1. Build a lightweight `crate_model` first (Good Regulator compliance — Conant-Ashby): read `Cargo.toml`, `lib.rs`/`main.rs`, and module structure; describe architecture, data_flow, critical_paths, dependency_surface, and observed_characteristics (async, unsafe, trait_objects, concurrency, ffi, macros, proc_macros).
2. If `prior_expedition` is present, consume it: distill `lessons_learned` into 1-3 probe-strategy adjustments, extend the probe pattern list with `pattern_signatures`, and (if present) make `next_charter_focus` the primary `target_area` unless already exhausted.
3. If `mutation_report` is present (from the `harness-optimize` skill's mutation testing output in the trace filesystem), prioritize `target_area` toward functions with surviving mutants — those are the concrete locations where the test suite is blind. Mutation testing finds syntactic blind spots (the suite doesn't notice `+` → `-`); bug-hunt finds semantic blind spots (the suite doesn't test the error path, the race condition). Mutation-guided chartering focuses exploratory probing exactly where the suite is weakest.
3. Generate a Hendrickson-format charter: "Explore [target] using [strategy] to discover [quality threat]."
4. Pick the most promising strategy from Bach's HTSM (Project Environment, Product Elements, Quality Criteria), justified against the crate model — not generic categories.
5. Select 2-3 Beizer categories given the crate model and quality criteria (e.g., heavy async usage → `timing` overrides `requirements` regardless of generic prevalence).
6. Emit probe_instructions that are actionable with available MCP tools.
7. Respond with a JSON object containing `charter_statement`, `target_area`, `strategy`, `expected_category`, `beizer_focus`, `crate_model`, `probe_instructions`, and `prior_feedback_consumed`.

### bug-hunt-probe

1. Format probe execution results. Do NOT generate fictional findings — only report what the agent actually discovered through MCP tool usage.
2. If no `probe_findings` are provided, explore the target with available MCP tools (`file:read`, `code:search`, `terminal`) using the charter's `probe_instructions`.
3. Run the static baseline pattern search (floor, not ceiling): `.unwrap()`/`.expect()` in library code, public functions without contracts, `unsafe` without documented safety invariants, integer arithmetic without overflow protection, `clone()` hiding ownership confusion, mutable state without synchronization, `panic!`/`todo!()` in non-startup code.
4. Apply dynamic pattern expansion (Ashby's Law): use `crate_model.observed_characteristics` to generate crate-specific patterns (async lock patterns, unsafe soundness, trait-object assumptions, concurrency ordering, FFI boundary, macro hygiene, proc-macro error emission). If `crate_model` is sparse, infer characteristics from the source read in step 3.
5. Detect missing tests (Weinberg): run `cargo test --no-run`, search for test modules, and check whether each `crate_model.critical_paths` function has a corresponding test. Record absence as a finding and shift strategy toward static analysis if dynamic probing has nothing to run.
6. Emit `probe_escalation` entries (VSM S1→S5 short-circuit) for critical findings: `unsafe` with likely UB, `panic!`/`unwrap`/`expect` in `fn main()` or `Drop` impls, security-relevant contract violations, data-loss paths without recovery. Each entry includes `finding_summary`, `location` (file:line), `severity` (CRITICAL), and `reason`.
7. Apply diagnose pattern (reproduce before diagnosing, isolate one variable), adversarial pattern (unexpected input orders, boundary values), and cybernetic pattern (trace feedback loops, Good Regulator check, variety check).
8. If `probe_depth` includes `dynamic` or `full` and the charter targets timing/integration/structural categories, use BugStalker (`bs`) on Linux x86-64 debug builds (with `--oracle tokio` for async). Probe patterns: async deadlock, race condition, state machine violation, memory corruption, poisoned mutex, unexpected polling. Record binary path, breakpoint locations, observed states, and whether behavior matches expectations.
9. Collect all findings as structured text. Prefix escalations with `[ESCALATION] <summary> at <file:line> — <reason>` so downstream phases can route them.

### bug-hunt-oracle

1. Evaluate raw probe findings against user-defined quality criteria using the Weinberg oracle (bug = threat to user-defined quality).
2. Assign a tier, confidence, AND reproducibility to each finding: Tier 1 BUG (0.90–1.00), Tier 2 POTENTIAL_BUG (0.60–0.89), Tier 3 OBSERVATION (<0.60).
3. Label reproducibility as `reproduced`, `reproducible`, `hard_to_reproduce`, or `not_reproducible`. Reproducibility does NOT downgrade confidence — a `hard_to_reproduce` finding with confidence 0.85 stays POTENTIAL_BUG (this corrects the prior conflation that biased against timing/concurrency/UB classes).
4. Apply IS vs OUGHT classification: describe what the code DOES (IS) vs what it SHOULD do (OUGHT). Never present OUGHT as IS.
5. Label the epistemic mode for every finding (Declarative, Probabilistic, Subjunctive). A subjunctive statement presented as declarative is a false positive.
6. Identify the provenance of the finding (Direct measurement, Inference, Assessment). Flag LLM-assessed conclusions.
7. Challenge your own verdict using grill-me self-challenge: Could this be intentional? Is there an edge case where this is correct? Would a reviewer dismiss this? If confidence < 0.80, state what would increase it.
8. Enforce no-fiction at the gate: every finding MUST include `location.file`, `location.line_approx`, and a verbatim `evidence` snippet (≤5 lines). Findings without citations are REJECTED and counted in `rejected_findings` with reason `"missing_citation"`.
9. Respond with a JSON object containing `evaluated_findings`, `confirmed_bugs`, `potential_bugs`, `contract_gaps`, and `rejected_findings`.

### bug-hunt-taxonomize

1. Classify each evaluated finding into exactly one Beizer category (requirements, structural, data, coding, interface, integration, timing, configuration).
2. Assign severity (CRITICAL, HIGH, MEDIUM, LOW) justified by the evidence in the finding.
3. Produce a `pattern_signature` for each finding — concrete enough for the next expedition's probe to consume (grep-able string or structural description). Vague signatures like "look for similar patterns" are not acceptable.
4. Preserve the `reproducibility` field from the oracle output — do not drop it.
5. Do not fabricate fix suggestions — use "needs investigation" when not obvious.
6. Respond with a JSON object containing `classified_findings` and `taxonomy_summary` (by_category and by_severity counts).

### bug-hunt-report

1. Compile charter, oracle, and taxonomy results into a structured JSON bug report.
2. Consolidate findings from oracle + taxonomy into a single `findings` array; each finding includes id, summary, location, verdict, confidence, reproducibility, beizer_category, severity, evidence, pattern_signature, and fix_suggestion.
3. Compute accurate summary statistics, including `rejected_findings` from the oracle.
4. Emit `lessons_learned` — concrete, actionable lessons the next expedition's charter should consume (e.g., "async lock held across .await at 3 sites; next charter should target all .await points in lock scope"). Not generic platitudes.
5. Emit `pattern_signatures` — derived from actual findings (signature, beizer_category, derived_from, notes on how to apply in the next probe). Not fabricated.
6. Note whether this is a first pass or iteration N+1 (consumed `prior_expedition`).
7. Write the expedition report to the trace filesystem (`kask/traces/<run-id>/bug-hunt-report.json`) via `hkask_test_harness::write_trace` so the findings are visible to the `harness-optimize` skill (the suite-level proposer). This closes the loop: bug-hunt finds bugs → traces → harness-optimize proposes tests → CI evaluates → mutation score improves.
8. Respond with the complete expedition report as JSON.

### bug-hunt-expedition (legacy)

1. Legacy monolithic expedition template (v0.30.0). Retained for backward compatibility — prefer the decomposed pipeline via the bug-hunt manifest.
2. Divergence from the decomposed pipeline (documented in-place): no `crate_model` (Good Regulator missing), no `prior_expedition` consumption (feedback loop not closed), no dynamic pattern expansion (Ashby deficit), no missing-tests detection, no algedonic escalation, no reproducibility axis (confidence conflated with reproducibility), no file:line citation enforcement (no-fiction is voluntary), no `lessons_learned`/`pattern_signatures` outputs, no composite convergence metric (saturation-only).
3. Use only when a single-call monolithic expedition is explicitly required.
4. Phases: Charter (Hendrickson + Bach HTSM) → Probe (file:read, code:search, terminal) → Oracle (Weinberg + pragmatic-semantics + grill-me) → Taxonomize (Beizer + severity + pattern signature) → Report (JSON schema).
5. Do not fabricate bugs; read real code and run real commands.

## Relationship to Other Skills

- **proptest**: systematically verifies known properties of a single function. Bug-hunt explores for unknown bugs across a crate. Bug-hunt's `pattern_signatures` feed into proptest's Identify phase; bug-hunt's trace emissions feed into `harness-optimize` which dispatches to proptest for specific under-tested functions.
- **harness-optimize**: the suite-level proposer. Reads bug-hunt's trace emissions and proposes tests for the bugs found. Bug-hunt runs independently (with `terminal` enabled — it can run tests); `harness-optimize` runs as a proposer (with `terminal` disabled). They communicate asynchronously through the trace filesystem.
- **diagnose**: when bug-hunt finds a confirmed bug, the `evidence` and `location` fields are a pre-minimized reproducer for diagnose's Phase 2.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `bug-hunt-charter.j2` | KnowAct | Generate a focused bug-hunt charter using Hendrickson format and Bach's HTSM. v0.31.0: builds a lightweight `crate_model` first (Good Regulator compliance) and consumes `prior_expedition` when present (feedback loop closure). Selects Beizer categories based on the crate model rather than generic prevalence. |
| `bug-hunt-probe.j2` | KnowAct | PROBE phase — agent-coordinated MCP tool execution. Reads target source files, searches for bug patterns, runs cargo check/test/clippy, and when `probe_depth` includes `dynamic`, executes BugStalker (`bs`) runtime probes for timing/integration/structural categories. v0.31.0: dynamic pattern expansion (Ashby), missing-tests detection (Weinberg), and algedonic escalation (VSM S1→S5). This template provides guidance and collects results; the actual probing is performed by the agent using MCP tools between template calls. |
| `bug-hunt-oracle.j2` | KnowAct | Apply Weinberg oracle, pragmatic-semantics IS/OUGHT classification, epistemic mode labeling, provenance tracing, and grill-me self-challenge to raw probe findings. v0.31.0: reproducibility is a separate axis from confidence (high-confidence low-reproducibility stays POTENTIAL_BUG), and findings without file:line citation are rejected (no-fiction enforcement). Produces tiered verdicts with confidence and reproducibility scores. |
| `bug-hunt-taxonomize.j2` | KnowAct | Classify evaluated findings into Beizer taxonomy (8 categories) and assign severity ratings (CRITICAL/HIGH/MEDIUM/LOW). Produces pattern signatures concrete enough for the next expedition's probe to consume. Preserves the `reproducibility` field from the oracle. |
| `bug-hunt-report.j2` | KnowAct | Compile charter, oracle, and taxonomy results into a structured JSON bug report. v0.31.0: emits `lessons_learned` and `pattern_signatures` fields that the next expedition's charter consumes to close the feedback loop. |
| `bug-hunt-expedition.j2` | KnowAct | Legacy monolithic expedition template (v0.30.0). Retained for backward compatibility. Prefer the decomposed pipeline. v0.31.0: divergence from the decomposed pipeline is documented in-place (missing crate_model, prior_expedition consumption, dynamic pattern expansion, missing-tests detection, algedonic escalation, reproducibility axis, citation enforcement, lessons_learned/pattern_signatures outputs, composite convergence metric). |

## Constraints

- `bug-hunt-charter.j2`: Public. Beizer category selection must be justified against the `crate_model`, not generic prevalence. When `prior_expedition` is present, the charter MUST consume it — silent ignoring is a feedback-loop violation.
- `bug-hunt-probe.j2`: Public. Do not generate fictional findings. The static pattern list is a floor, not a ceiling — dynamic expansion is required when `crate_model.observed_characteristics` are present. Missing tests must be recorded as a finding, not a silent no-op. Critical findings must emit `probe_escalation` entries.
- `bug-hunt-oracle.j2`: Public. Every finding must carry IS/OUGHT, epistemic mode, provenance, AND reproducibility labels. Every finding must cite a concrete file:line and include a verbatim code snippet (≤5 lines); uncited findings are rejected, not silently dropped. Reproducibility does NOT downgrade confidence. If confidence < 0.60, the finding is an OBSERVATION.
- `bug-hunt-taxonomize.j2`: Public. Every finding has exactly one Beizer category. Severity must be justified by evidence. Pattern signatures must be concrete (grep-able or structural), not vague. The `reproducibility` field must be preserved from the oracle.
- `bug-hunt-report.j2`: Public. Each finding must include all required fields. Summary counts must be accurate, including `rejected_findings`. `lessons_learned` must be concrete and actionable; `pattern_signatures` must be derived from actual findings, not fabricated.
- `bug-hunt-expedition.j2`: Public. Legacy v0.30.0 — divergence from the decomposed pipeline is documented in-place. Use only when a single-call monolithic expedition is explicitly required.
- **Convergence:** Convergence is detected deterministically via the Cauchy criterion — the iterates have stopped moving. `max_iterations: 10`, `min_iterations: 2`, `on_not_reached: escalate`. No LLM convergence-check template is used.
- **OCAP:** requires `Tool:test:Execute` and `Tool:regulation:Read`; delegation chain required; template-scoped; capability expiry 3600s; signature algorithm ed25519.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
