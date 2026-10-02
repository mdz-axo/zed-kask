---
name: code-review
core: true
description: "Convergent code review of a change against its stated spec. Multi-axis detection, IS/OUGHT adjudication, falsifier, file:line no-fiction. Optional implement phase via fix_mode context. Grounded in Fagan, PERFECT, Ousterhout."
---

# Code Review

Convergent code review of a change against its stated spec. Grounded in Fagan formal inspection (Planning → defect detection → defect collection → follow-up), modern code review (Bacchelli & Bird 2013; Sadowski & Stolee 2015), the PERFECT framework (Bastrich), and Ousterhout's "A Philosophy of Software Design". Decomposed into phased templates: Scope (real diff + Fagan sizing + critical-path identification + change model via Good Regulator + prior-review feedback) → Perspectives (prediction-then-detection: pre-registered finding and Blocker counts, then multi-axis DETECTION across PERFECT-ordered axes intersected with the addyosmani five-axis, with optional delegation to bug-hunt / refactor-architecture / deep-module / essentialist) → Adjudicate (defect COLLECTION with pragmatic-semantics IS/OUGHT + epistemic mode + provenance + constraint-force severity + falsifier + grill-me self-challenge + file:line no-fiction citation) → Report (prediction reconciliation + verdict + named structural remedies + coverage honesty + lessons_learned / next_review_focus / prior_review_handoff loop closure) → Implement (optional, caller-gated Act phase via fix_mode). Reasoning patterns from pragmatic-semantics, falsifiability, hypothesis-framer, and grill-me are embedded as inline prompt instructions in the adjudicate phase; pragmatic-cybernetics (Good Regulator change model, Ashby variety-via-delegation) in the scope and perspectives phases; essentialist's deletion test flavors the perspectives architecture axis and the report's remove-moving-pieces remedy preference. Comprehensive-by-default; variety via delegation, not toggleable modes (essentialist deletion test). Capability-gated.

## Reference models

Fagan (1976) formal inspection and Ousterhout (2018) — `onto_anchor` → derived `fagan_inspection` (operator ruling 2026-09-25).

## D/P labelling

Findings are P (detection and adjudication judgments), critiqued by the grill-me self-challenge and each finding's falsifier. Severity derivation is D (`lisp_eval` over the classified finding — the fixed base → confidence-downgrade → provenance-ceiling → taste-ceiling order in `code-review-adjudicate.j2` Step 3); the severity tier order is D (the form's `tiers` list). The report's prediction reconciliation is D (`lisp_eval` gap form over predicted vs found counts, report step 0). The constraint-force classification feeding the severity form is P. The file:line no-fiction rule and the Blocker-set stop rule are exceptionless P-region rules — no machine oracle checks them; their violations surface in `rejected_findings` (reason `missing_citation`) and the open-Blocker-set tracking (Constraints), and `severity_counts` / `blocker_delta` are P-emitted arithmetic audited by the report's coverage honesty.

## When to Use

- Before merging any PR or change — review-first, no exceptions.
- After implementing a feature or fixing a bug (review the fix and the regression test).
- When evaluating self-authored, AI-generated, or another agent's code (AI code needs more scrutiny, not less).
- When you need severity grounded in constraint force (the pragmatic-semantics forces, with Guardrails waivable by the operator), not ad-hoc importance.
- When you need every finding falsifiable and cited with file:line + verbatim evidence (no-fiction; anti-hallucination for AI review).
- When you want optional delegation to bug-hunt (deep defects) or refactor-architecture / deep-module / essentialist (architecture) instead of reimplementing those lenses.
- When you want an optional, consent-gated implement phase (`fix_mode`) that applies the reviewed fixes.
- When iterating a review to convergence (blocker_delta stabilization) with `next_review_focus` feedback-loop closure across passes.

## When NOT to Use

- A change with no recoverable spec — first recover the user's stated intent from the task, relevant docs, and history; if it remains unclear, ask the user before judging the change. The skill tool does not validate `change_spec`.
- Security-only deep audits — the inline pass covers the security basics; there is no dedicated security delegate (the retired kali-audit's surface is inline now).
- Style-only preference feedback — severity derives from constraint force; taste findings never reach Blocker.
- Verifying that the code runs — this is static review of a diff; it does not execute the change.

## Review inputs (agent-resolved, not skill-tool parameters)

The `skill` tool accepts `name` and `task` only. Read the user's task and gather the following inputs before rendering the relevant templates; do not treat them as runtime-validated skill inputs:

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `change_spec` | string | (required for review) | Stated or recovered intent; if unavailable, ask instead of inventing it. |
| `diff_base` | string | (agent selects) | Verified git ref for committed changes (`main`, `origin/main`, a SHA); for uncommitted changes use the actual working-tree/staged diff, not `...HEAD`. |
| `focus` | array | `[]` (comprehensive) | Axes to restrict the review to (empty = all). Security always gets a basic pass. |
| `delegate_bug_hunt` | bool | `false` | Emit a delegation instruction for bug-hunt (deep defects). |
| `delegate_architecture` | bool | `false` | Emit a delegation instruction for refactor-architecture / deep-module / essentialist (architecture). |
| `fix_mode` | string | `"none"` | Review-only unless the user's actual request explicitly authorizes `blockers`, `should_fix`, or `all` fixes. Model-inferred values and template output are not consent. |
| `prior_review` | object | absent | The previous pass's `prior_review_handoff` (next_review_focus, lessons_learned, `blockers` as an integer count); closes the feedback loop. |
| `probe_findings` | string | absent | Pre-populated delegate-skill findings. Skill path: pass them at the perspectives render and the model folds them into `raw_findings`; Zed sessions: the agent folds the returned findings into `raw_findings` between perspectives and adjudicate. One owner per path, never both (no double-counting). |

`task` is the user's natural-language request, not a validated context map. There is no manifest/enforce_inputs gate. The agent must verify the spec, diff target, and explicit edit authorization itself. When any is absent or ambiguous, default to read-only review; ask for the missing spec before a spec-based verdict and ask for consent before any edit.

## Instructions

### code-review-scope

1. Identify the real change target first: for committed changes use a verified base and `git --no-pager diff <diff_base>...HEAD`; for staged/working-tree changes inspect those diffs too. Do not use `...HEAD` alone to review an uncommitted change. Derive size and paths from real git output; if the selected diff is empty, emit `size_class` "empty" and stop. Recover `change_spec` from the user's task/docs/history; if unavailable, ask before a spec-based verdict.
2. Classify change size (Fagan): `trivial` (<~20, non-critical), `good` (~100), `acceptable` (~300, single logical change), `too_large` (>~1000 → request a split). Also flag files the change materially grows past ~1000 total lines.
3. Identify critical paths touching auth, payments, data writes, concurrency, `unsafe`, FFI, secrets/credentials, external data boundaries, or anything `change_spec` names as load-bearing — these get deeper scrutiny.
4. Build the change model (Good Regulator): `what_changed`, `intent_vs_spec` (does the diff match the stated spec? flag mismatches for the Purpose axis), `module_boundaries_crossed`, `observed_characteristics` (async, unsafe, trait objects, concurrency, FFI, macros — derived from actually reading the diff), `prior_blockers` (`prior_review.blockers` if present, else 0).
5. Resolve focus: if `focus` is non-empty, restrict detection to those axes (security always gets a basic pass); if empty, comprehensive. Merge `prior_review.next_review_focus` as the primary emphasis (do not drop the user's explicit `focus`).
6. Do NOT judge findings here — only model the change (detection/collection is later). Respond with the JSON object (`change_model`, `scope_summary`, `size_class`, `critical_paths`, `focus_axes`, `prior_feedback_consumed`).

### code-review-perspectives

0. **Predict before detecting (calibration, P).** From the scope model alone, state the expected number of raw findings and the expected number of Blockers, with one sentence of basis (size class, critical paths, prior review). Record both numbers before reading a hunk; the report reconciles them.
1. Read surrounding code with `read_file` / `grep` for context around each hunk — diffs alone miss issues. Do not opine on regions you did not read.
2. Walk the diff top-to-bottom across the PERFECT-ordered axes (Purpose → Edge cases → Reliability → Form → Evidence → Clarity → Taste) intersected with the addyosmani five-axis (correctness, readability, architecture, security, performance). Run the scope-resolved `focus_axes`: if the user's `focus` was non-empty, these are restricted to it (plus a basic security pass); if comprehensive, run all — and when a prior pass's `next_review_focus` was merged into `focus_axes`, walk that emphasis FIRST and give it the deepest pass (scope put it there as the primary emphasis; do not demote it to a passive header).
3. DETECTION ONLY — do NOT assign verdicts, severity, confidence, or falsifiers (that is the adjudicate phase; Sauer detection/collection separation).
4. For each raw finding, record `axis`, `location.file`, `location.line_approx`, a verbatim `evidence` snippet (≤5 lines) read from the cited location, a one-line `observation` (no verdict), and `source`. Uncited observations are DROPPED, not recorded (no-fiction).
5. For each enabled delegate flag, emit a delegation instruction (bug-hunt / refactor-architecture / deep-module / essentialist) for the agent to run between this step and adjudicate; the inline pass always covers the basics, delegation adds depth. When `probe_findings` is present (skill path — the delegates ran after this step's instructions were emitted), fold those returned findings into `raw_findings` with `source` set (do not double-count or re-verdict them).
6. Lead with leverage (purpose/security/structural before cosmetic nits). Respond with `predicted_findings`, `predicted_blockers`, `raw_findings`, `delegated_axes`, `delegate_instructions`.

### code-review-adjudicate

1. Do NOT re-scan the code — adjudicate the `raw_findings` given (Sauer detection/collection separation).
2. Classify each finding (pragmatic-semantics): IS vs OUGHT (never present an OUGHT as an IS), `epistemic_mode` (declarative / probabilistic / subjunctive — a subjunctive presented as declarative is a false positive), `provenance` (an evidence-type adaptation of pragmatic-semantics' provenance axis to findings: direct_measurement / inference / assessment; assessment never exceeds Should-fix).
3. Frame each as a falsifiable hypothesis (falsifiability + hypothesis-framer): H0 (null: the code is correct/intentional), H1 (the claim), and a `falsifier` (what would prove H1 wrong / catch it). A finding with no falsifier is a preference → Nit/FYI, not a defect.
4. Derive severity deterministically from constraint force — pragmatic-semantics' five forces plus two review-only categories: Prohibition → Blocker (fix only); Guardrail → Blocker, waivable by the operator with a recorded reason; Guideline → Should-fix; Evidence → the severity of the rule it violates (`violated_force`); Hypothesis → as Evidence, capped at Should-fix; Preference → Nit; Informational → FYI. Compute it with `lisp_eval` (D) over the classified finding (the classification is P) — form: `(begin (define tiers (list "FYI" "Nit" "Should-fix" "Blocker")) (define base (lambda (f v) (cond ((string= f "Prohibition") 3) ((string= f "Guardrail") 3) ((string= f "Guideline") 2) ((or (string= f "Evidence") (string= f "Hypothesis")) (base v "none")) ((string= f "Preference") 1) (t 0)))) (let ((b (base force violated))) (let ((d (if (and (< conf 0.6) (not (and crit (string= force "Prohibition")))) (max 0 (- b 1)) b))) (let ((c (if (or capped (string= force "Hypothesis")) (min d 2) d))) (nth (if taste (min c 1) c) tiers)))))`, env `{ "force", "violated": <violated_force or "none">, "conf", "crit": <on a critical path>, "capped": <subjunctive or assessment provenance>, "taste" }`. Modifiers: confidence < 0.60 downgrades one tier; subjunctive or assessment provenance never exceeds Should-fix; a taste-only finding is never a Blocker; a finding on a critical path weighs heavier (a Prohibition there stays Blocker even at lower confidence). Apply them in the fixed order stated in `code-review-adjudicate.j2` Step 3 (base → confidence downgrade → provenance ceiling → taste ceiling); the order changes outcomes, so it is not left to judgment.
5. Run grill-me self-challenge (Recall → Mechanism → Rationale → Edge cases → Synthesis): could this be intentional? Is there an edge case where it is correct? Is there a convention (`.rules`, surrounding code) that makes it acceptable? If confidence < 0.80, state what would raise it. Resolve and record; if the self-challenge invalidates the finding, downgrade or reject it.
6. No-fiction gate: every adjudicated finding must cite `location.file`, `location.line_approx`, and a verbatim `evidence` snippet (carried from raw_findings). Missing any → REJECT, counted in `rejected_findings` with reason `"missing_citation"` (not silently dropped).
7. Quantify where possible ("this N+1 adds ~50ms per item"); if you cannot quantify, say so — never fabricate a number. An unquantified performance finding is Should-fix ≤ 0.6, not Blocker.
8. Be honest / anti-sycophantic: do not soften a real issue, do not rubber-stamp, do not block on taste alone. If `raw_findings` is empty, return all-zero counts — a CLEAN PASS is valid; do not fabricate findings to look thorough. Corroborated ≠ confirmed — use "upheld"/"withstood", never "proven".
9. Compute `blocker_delta` = Blocker count this pass − `prior_review.blockers` (0 if absent). Respond with `adjudicated_findings`, `severity_counts`, `rejected_findings`, `blocker_delta`.

### code-review-report

0. **Reconcile the prediction (D).** Report `predicted_findings` / `found_findings` and `predicted_blockers` / `found_blockers` from step 0 of perspectives, with the gap computed by `lisp_eval` `(list (- found_findings predicted_findings) (- found_blockers predicted_blockers))`. A gap is not a defect; an unreported gap is — it is the only signal that the reviewer's model of the change was wrong.
1. **Guardrail waivers (the operator's decision).** Present every waivable Blocker to the operator with the rule it breaks and ask: fix it, or waive it with a reason. Record an accepted waiver in the finding's `waiver` (`{reason, by: "operator", date}`). Never waive on the operator's behalf, and never offer a waiver for a Prohibition. A waived Guardrail is reported under its own heading, not removed.
2. Produce a verdict driven by open Blockers (unfixed and unwaived), NOT nit count: **Approve** (zero open Blockers AND the change improves overall code health, even if imperfect — don't block because it isn't how you'd write it), **Request changes** (one or more Blockers, or a structural regression that makes the system worse), **Comment** (observations only, nothing blocking).
3. Group findings by severity (Blocker → Should-fix → Nit → FYI); lead with what matters; never bury a Blocker under nits. If you have one structural problem and ten nits, the structural problem IS the review.
4. Attach a NAMED structural remedy to every architectural/structural (Blocker/Should-fix) finding — propose the move, not just the problem: replace a conditional chain with a typed model/dispatcher; collapse duplicate branches; separate orchestration from business logic; move feature-specific logic out of a shared module; reuse the canonical helper; make a type boundary explicit; delete a pass-through wrapper; extract a helper / split a large file. Prefer the remedy that REMOVES moving pieces over one that relocates the same complexity.
5. Rank the 3 highest-leverage top fixes with estimated effort (XS/S/M/L). If `size_class == "too_large"`, the TOP recommendation is to split before merging, with a concrete split plan (stack / by-file-group / horizontal / vertical).
6. Coverage honesty is mandatory: state `checked`, `not_checked`, and `residual_risk`. A clean review MUST say so explicitly and name what it did NOT verify; never output a bare "LGTM" (anti-sycophancy).
7. Loop closure: emit `lessons_learned` (concrete, derived from THIS review's findings — not platitudes) and `next_review_focus` (what the next pass should concentrate on; EMPTY string if the review converged clean: zero Blockers, stable `blocker_delta`).
8. Review-first: do not implement changes here — implementation is the separate, `fix_mode`-gated implement phase. Respond with `review`, `lessons_learned`, `next_review_focus`, `prior_review_handoff`.

### code-review-implement

1. This step runs ONLY after the agent confirms the user's actual request explicitly authorized edits and resolves `fix_mode` to one of `blockers` / `should_fix` / `all`. No manifest or skill-tool condition gates this. Missing, model-inferred, or `"none"` values skip implementation. Map an authorized `fix_mode` to the tier: `blockers` → Blocker only; `should_fix` → Blocker + Should-fix; `all` → every actionable finding (skip pure taste/FYI with no concrete remedy).
2. For each in-tier finding WITH a concrete remedy, READ the actual file first (`read_file` on `location.file`) and produce ONE surgical edit: `file`, `location`, the EXACT verbatim `old_text` (copied from the read), `new_text`, `finding_id`, and `remedy` (reuse the report's named remedies). NEVER fabricate `old_text` — a fabricated `old_text` fails the fuzzy match and silently drops the fix.
3. Prefer the remedy that removes moving pieces (Ousterhout/addyosmani). One finding → one surgical edit (plus its directly-required sibling, e.g., a call site the edit forces). Do NOT bundle unrelated refactors into a requested fix.
4. Honor project conventions and the repo `.rules` (Rust/GPUI: `?` over `unwrap()`/`expect()`; never `let _ =` on fallible ops; no panicking indexing; full variable names; GPUI constraints where applicable). For other languages, follow the surrounding code's idioms.
5. Skip findings you cannot ground: no actionable remedy → `"no_remedy"`; cannot locate `old_text` → `"cannot_locate"`; taste/FYI with no remedy → skip even under `"all"`. The agent applies each `fix_plan` edit via `edit_file`; `applied_count` reflects what was ACTUALLY applied (a failed fuzzy match drops that fix into `fixes_skipped` with reason `"edit_failed"`).
6. Respond with `fix_plan`, `fixes_generated`, `applied_count`, `fixes_skipped`. The convergence re-review should see a reduced `blocker_delta`.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `code-review-scope.j2` | Compute the diff against diff_base from real git output, classify change size (Fagan sizing: ~100 good, ~300 acceptable, ~1000 too large), identify critical paths (auth/payments/data writes/concurrency/unsafe/FFI/secrets/ external data boundaries), and build a lightweight change model (Good Regulator). Consumes prior_review.next_review_focus as the primary emphasis for this pass. Does not judge findings — only models the change. |
| `code-review-perspectives.j2` | Records the pre-registered prediction (predicted_findings / predicted_blockers) before detection, then walks the diff through PERFECT-ordered axes (Purpose → Edge cases → Reliability → Form → Evidence → Clarity → Taste) intersected with the addyosmani five-axis (correctness, readability, architecture, security, performance). DETECTION only — records raw, unverdicted findings with file:line + verbatim evidence (no-fiction). Respects focus. For each enabled delegate flag, emits a delegation instruction for the agent to invoke the specialist skill between this step and adjudicate. Accepts pre-populated probe_findings for the skill path. Detection/collection separation (Sauer) — no verdicts here. |
| `code-review-adjudicate.j2` | Turn raw observations into tiered, evidence-backed verdicts (defect COLLECTION). Applies pragmatic-semantics IS/OUGHT + epistemic mode + provenance, frames each as a falsifiable hypothesis (H0/H1 + falsifier), derives severity from constraint force (Prohibition→Blocker, Guideline→Should-fix, Preference→Nit, Informational→FYI), runs grill-me self-challenge, and enforces file:line no-fiction citation (uncited findings rejected, not dropped). Computes blocker_delta vs prior_review. Does NOT re-scan the code (Sauer detection/collection separation). |
| `code-review-report.j2` | Produce a verdict driven by Blocker presence (not nit count): Approve / Request changes / Comment. Group findings by severity, lead with what matters, attach a NAMED structural remedy to every architectural/structural finding, rank the 3 highest-leverage top fixes, and emit coverage honesty (checked / not_checked / residual_risk — never a bare "LGTM"). Reconciles the perspectives step-0 prediction against the found counts (lisp_eval gap form). Emits lessons_learned, next_review_focus, and prior_review_handoff (the typed prior_review object — blockers as an integer count — for the next pass) for loop closure. |
| `code-review-implement.j2` | Act phase (Fagan rework). Skip by default; only render and apply fixes after verifying explicit user authorization for the requested fix tier. A model-supplied `fix_mode` is not consent. For each finding at/above the requested tier with a concrete remedy, produce ONE surgical edit (file, location, exact verbatim old_text read from the actual file, new_text). Reuses the report's structural remedies. Honors project conventions and .rules. Never fabricates old_text — skips findings it cannot ground. |

To render a template, call the `render_template` tool with the template ref (e.g., `code-review/code-review-scope`) and a context object with the required variables.

## Regression case

All receipts executed live through `lisp_eval` (2026-10-01, backfill pass):

- Severity form, Prohibition violated at conf 0.9 → `"Blocker"` (base 3,
  no demotion).
- Severity form, Guideline violated at conf 0.5 → `"Nit"` (base 2,
  low-confidence demotion to 1).
- Gap form, found [8, 5] vs predicted [8, 5] → `[0, 0]`.

The skill's forms are executed at use time, never anchored in code.

## Constraints

- `code-review-scope.j2`: compute the diff from real git output; never estimate from the spec. When `prior_review.next_review_focus` is present it MUST be consumed (silently ignoring prior feedback is a feedback-loop violation). Do not judge findings here — only model the change.
- `code-review-perspectives.j2`: DETECTION ONLY — no verdicts, severity, confidence, or falsifiers. Every raw finding cites file:line + a verbatim evidence snippet; uncited observations are dropped, not recorded. Respect `focus` (security always gets a basic pass). Delegation adds depth but does not remove the inline basics pass.
- `code-review-adjudicate.j2`: do NOT re-scan the code; adjudicate the `raw_findings` given. Every finding carries IS/OUGHT, epistemic mode, provenance, constraint force, confidence, a falsifier, a grill-me resolution, and a verbatim citation — missing any → reject (not silently drop). Severity is derived from constraint force; a taste finding is never a Blocker; subjunctive/assessment never exceeds Should-fix; confidence < 0.60 downgrades one tier. An empty `raw_findings` list yields all-zero counts (a clean pass is valid) — do not fabricate. Corroborated ≠ confirmed; use "upheld"/"withstood".
- `code-review-report.j2`: verdict driven by Blocker presence, not nit count. Every Blocker/Should-fix finding carries a concrete remedy AND a falsifier. Coverage honesty is mandatory (checked / not_checked / residual_risk); never a bare "LGTM". `lessons_learned` must be concrete; `next_review_focus` is empty when converged clean. Review-first — no implementation here.
- `code-review-implement.j2`: agent-gated, not manifest-gated. Run ONLY when the user explicitly authorized the selected `fix_mode` ∈ {blockers, should_fix, all}; otherwise skip, including when absent, inferred, or `"none"`. Read the actual file before emitting `old_text` — never fabricate; prefer to skip than guess. One finding → one surgical edit; do not bundle unrelated refactors. Honor project conventions and `.rules`. `applied_count` reflects what the agent actually applied, not what was generated.
- **Convergence:** track the SET of open Blocker findings (by file:line and claim) after each full iteration, not only the count — `blocker_delta` is a net figure, so fixing two and introducing two reads as 0. A Blocker leaves the open set when it is fixed, or — for a Guardrail only — when the operator has recorded a waiver; a Prohibition leaves it only when fixed. Converged clean when the open Blocker set is empty. If the set is unchanged for 3 consecutive passes with Blockers still open, stop and escalate: that is a stall, not convergence. Maximum 10 iterations; escalate if not converged by then. Minimum 2 iterations before declaring convergence on a non-empty open set, guaranteeing at least one grill-me self-challenge re-pass; a first pass with zero open Blockers is converged clean (the empty set needs no stabilization).
- **Modes:** comprehensive-by-default. The four lenses (adversarial, multi-perspective, refactoring, generative) are inline reasoning, not separate invocations. "Generative" is the explicitly user-authorized implement phase, never enabled by the model's own `fix_mode` choice.
