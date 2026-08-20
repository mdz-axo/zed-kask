---
name: code-review
core: true
description: "Convergent code review of a change against its stated spec. Multi-axis detection, IS/OUGHT adjudication, falsifier, file:line no-fiction. Optional implement phase via fix_mode context. Grounded in Fagan, PERFECT, Ousterhout."
---

# Code Review

Convergent code review of a change against its stated spec. Grounded in Fagan formal inspection (Planning → defect detection → defect collection → follow-up), modern code review (Bacchelli & Bird 2013; Sadowski & Stolee 2015), the PERFECT framework (Bastrich), and Ousterhout's "A Philosophy of Software Design". Decomposed into phased templates: Scope (real diff + Fagan sizing + critical-path identification + change model via Good Regulator + prior-review feedback) → Perspectives (multi-axis DETECTION across PERFECT-ordered axes intersected with the addyosmani five-axis, with optional delegation to kali-audit / bug-hunt / refactor-architecture / deep-module / essentialist) → Adjudicate (defect COLLECTION with pragmatic-semantics IS/OUGHT + epistemic mode + provenance + constraint-force severity + falsifier + grill-me self-challenge + file:line no-fiction citation) → Report (verdict + named structural remedies + coverage honesty + lessons_learned / next_review_focus loop closure) → Implement (optional, caller-gated Act phase via fix_mode). Reasoning patterns from pragmatic-semantics, pragmatic-cybernetics, falsifiability, hypothesis-framer, grill-me, and essentialist are embedded as inline prompt instructions in the adjudicate and perspectives phases. Comprehensive-by-default; variety via delegation, not toggleable modes (essentialist deletion test). Emits Regulation spans (`reg.codereview.*`) for observability. Capability-gated.

## When to Use

- Before merging any PR or change — review-first, no exceptions.
- After implementing a feature or fixing a bug (review the fix and the regression test).
- When evaluating self-authored, AI-generated, or another agent's code (AI code needs more scrutiny, not less).
- When you need severity grounded in constraint force (Prohibition / Guideline / Preference / Informational), not ad-hoc importance.
- When you need every finding falsifiable and cited with file:line + verbatim evidence (no-fiction; anti-hallucination for AI review).
- When you want optional delegation to kali-audit (security), bug-hunt (deep defects), or refactor-architecture / deep-module / essentialist (architecture) instead of reimplementing those lenses.
- When you want an optional, consent-gated implement phase (`fix_mode`) that applies the reviewed fixes.
- When iterating a review to convergence (blocker_delta stabilization) with `next_review_focus` feedback-loop closure across passes.

## Context parameters (skill-tool `context`)

The skill is steered by passing keys in the `context` map of the `skill` tool invocation (these map 1:1 to the manifest's declared `inputs`):

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `change_spec` | string | (required) | The stated spec/intent the change implements; the review judges the change against this. |
| `diff_base` | string | (required) | Git ref to diff against (`main`, `origin/main`, a SHA). Scope computes `git diff <diff_base>...HEAD` from real output. |
| `focus` | array | `[]` (comprehensive) | Axes to restrict the review to (empty = all). Security always gets a basic pass. |
| `delegate_security` | bool | `false` | Emit a delegation instruction for kali-audit (security depth). |
| `delegate_bug_hunt` | bool | `false` | Emit a delegation instruction for bug-hunt (deep defects). |
| `delegate_architecture` | bool | `false` | Emit a delegation instruction for refactor-architecture / deep-module / essentialist (architecture). |
| `fix_mode` | string | `"none"` | `none` = review-only. `blockers` / `should_fix` / `all` enable the implement phase (Act); a non-`none` value is consent to modify code. |
| `prior_review` | object | absent | Output of a previous pass; closes the feedback loop (`next_review_focus`, `lessons_learned`, `blockers`). |
| `probe_findings` | string | absent | Pre-populated delegate-skill findings (FlowDef path); in Zed sessions the agent folds these between perspectives and adjudicate. |

`task` (the user's natural-language request) is injected after `context`, so a `context["task"]` entry never clobbers the user's actual request.

**Enforced at the boundary (`enforce_inputs: true`).** This skill opts in to input validation: an invocation that omits a required input (`change_spec` / `diff_base`) or passes a wrong-typed `context` value (e.g. `fix_mode` as a bool instead of a string) is rejected with a structured error *before* the cascade runs — instead of silently running on empty/wrong inputs. Unknown keys are warned (not rejected). See `hkask_templates::validate_inputs`.

## Instructions

### code-review-scope

1. Compute the real diff from git output (`git --no-pager diff <diff_base>...HEAD --stat` / `--name-only`, `git --no-pager log <diff_base>..HEAD --oneline`); never estimate sizes from the spec. If the diff is empty, emit `size_class` "empty" and stop.
2. Classify change size (Fagan): `trivial` (<~20, non-critical), `good` (~100), `acceptable` (~300, single logical change), `too_large` (>~1000 → request a split). Also flag files the change materially grows past ~1000 total lines.
3. Identify critical paths touching auth, payments, data writes, concurrency, `unsafe`, FFI, secrets/credentials, external data boundaries, or anything `change_spec` names as load-bearing — these get deeper scrutiny.
4. Build the change model (Good Regulator): `what_changed`, `intent_vs_spec` (does the diff match the stated spec? flag mismatches for the Purpose axis), `module_boundaries_crossed`, `observed_characteristics` (async, unsafe, trait objects, concurrency, FFI, macros — derived from actually reading the diff), `prior_blockers` (`prior_review.blockers` if present, else 0).
5. Resolve focus: if `focus` is non-empty, restrict detection to those axes (security always gets a basic pass); if empty, comprehensive. Merge `prior_review.next_review_focus` as the primary emphasis (do not drop the user's explicit `focus`).
6. Do NOT judge findings here — only model the change (detection/collection is later). Respond with the JSON object (`change_model`, `scope_summary`, `size_class`, `critical_paths`, `focus_axes`, `prior_feedback_consumed`).

### code-review-perspectives

1. Read surrounding code with `read_file` / `grep` for context around each hunk — diffs alone miss issues. Do not opine on regions you did not read.
2. Walk the diff top-to-bottom across the PERFECT-ordered axes (Purpose → Edge cases → Reliability → Form → Evidence → Clarity → Taste) intersected with the addyosmani five-axis (correctness, readability, architecture, security, performance). If `focus` is non-empty, run only those (plus a basic security pass); if empty, comprehensive.
3. DETECTION ONLY — do NOT assign verdicts, severity, confidence, or falsifiers (that is the adjudicate phase; Sauer detection/collection separation).
4. For each raw finding, record `axis`, `location.file`, `location.line_approx`, a verbatim `evidence` snippet (≤5 lines) read from the cited location, a one-line `observation` (no verdict), and `source`. Uncited observations are DROPPED, not recorded (no-fiction).
5. For each enabled delegate flag, emit a delegation instruction (kali-audit / bug-hunt / refactor-architecture / deep-module / essentialist) for the agent to run between this step and adjudicate; the inline pass always covers the basics, delegation adds depth. When `probe_findings` is present, fold those returned findings into `raw_findings` with `source` set (do not double-count or re-verdict them).
6. Lead with leverage (purpose/security/structural before cosmetic nits). Respond with `raw_findings`, `delegated_axes`, `delegate_instructions`.

### code-review-adjudicate

1. Do NOT re-scan the code — adjudicate the `raw_findings` given (Sauer detection/collection separation).
2. Classify each finding (pragmatic-semantics): IS vs OUGHT (never present an OUGHT as an IS), `epistemic_mode` (declarative / probabilistic / subjunctive — a subjunctive presented as declarative is a false positive), `provenance` (direct_measurement / inference / assessment; assessment never exceeds Should-fix).
3. Frame each as a falsifiable hypothesis (falsifiability + hypothesis-framer): H0 (null: the code is correct/intentional), H1 (the claim), and a `falsifier` (what would prove H1 wrong / catch it). A finding with no falsifier is a preference → Nit/FYI, not a defect.
4. Derive severity deterministically from constraint force: Prohibition → Blocker, Guideline → Should-fix, Preference → Nit, Informational → FYI. Modifiers: confidence < 0.60 downgrades one tier; subjunctive or assessment provenance never exceeds Should-fix; a taste-only finding is never a Blocker; a finding on a critical path weighs heavier (a Prohibition there stays Blocker even at lower confidence).
5. Run grill-me self-challenge (Recall → Mechanism → Rationale → Edge cases → Synthesis): could this be intentional? Is there an edge case where it is correct? Is there a convention (`.rules`, surrounding code) that makes it acceptable? If confidence < 0.80, state what would raise it. Resolve and record; if the self-challenge invalidates the finding, downgrade or reject it.
6. No-fiction gate: every adjudicated finding must cite `location.file`, `location.line_approx`, and a verbatim `evidence` snippet (carried from raw_findings). Missing any → REJECT, counted in `rejected_findings` with reason `"missing_citation"` (not silently dropped).
7. Quantify where possible ("this N+1 adds ~50ms per item"); if you cannot quantify, say so — never fabricate a number. An unquantified performance finding is Should-fix ≤ 0.6, not Blocker.
8. Be honest / anti-sycophantic: do not soften a real issue, do not rubber-stamp, do not block on taste alone. If `raw_findings` is empty, return all-zero counts — a CLEAN PASS is valid; do not fabricate findings to look thorough. Corroborated ≠ confirmed — use "upheld"/"withstood", never "proven".
9. Compute `blocker_delta` = Blocker count this pass − `prior_review.blockers` (0 if absent). Respond with `adjudicated_findings`, `severity_counts`, `rejected_findings`, `blocker_delta`.

### code-review-report

1. Produce a verdict driven by Blocker presence, NOT nit count: **Approve** (zero Blockers AND the change improves overall code health, even if imperfect — don't block because it isn't how you'd write it), **Request changes** (one or more Blockers, or a structural regression that makes the system worse), **Comment** (observations only, nothing blocking).
2. Group findings by severity (Blocker → Should-fix → Nit → FYI); lead with what matters; never bury a Blocker under nits. If you have one structural problem and ten nits, the structural problem IS the review.
3. Attach a NAMED structural remedy to every architectural/structural (Blocker/Should-fix) finding — propose the move, not just the problem: replace a conditional chain with a typed model/dispatcher; collapse duplicate branches; separate orchestration from business logic; move feature-specific logic out of a shared module; reuse the canonical helper; make a type boundary explicit; delete a pass-through wrapper; extract a helper / split a large file. Prefer the remedy that REMOVES moving pieces over one that relocates the same complexity.
4. Rank the 3 highest-leverage top fixes with estimated effort (XS/S/M/L). If `size_class == "too_large"`, the TOP recommendation is to split before merging, with a concrete split plan (stack / by-file-group / horizontal / vertical).
5. Coverage honesty is mandatory: state `checked`, `not_checked`, and `residual_risk`. A clean review MUST say so explicitly and name what it did NOT verify; never output a bare "LGTM" (anti-sycophancy).
6. Loop closure: emit `lessons_learned` (concrete, derived from THIS review's findings — not platitudes) and `next_review_focus` (what the next pass should concentrate on; EMPTY string if the review converged clean: zero Blockers, stable `blocker_delta`).
7. Review-first: do not implement changes here — implementation is the separate, `fix_mode`-gated implement phase. Respond with `review`, `lessons_learned`, `next_review_focus`.

### code-review-implement

1. This step runs ONLY when `fix_mode` is one of `blockers` / `should_fix` / `all` (gated by `step.condition`, default-deny; absence and `"none"` both skip). Map `fix_mode` to the tier: `blockers` → Blocker only; `should_fix` → Blocker + Should-fix; `all` → every actionable finding (skip pure taste/FYI with no concrete remedy).
2. For each in-tier finding WITH a concrete remedy, READ the actual file first (`read_file` on `location.file`) and produce ONE surgical edit: `file`, `location`, the EXACT verbatim `old_text` (copied from the read), `new_text`, `finding_id`, and `remedy` (reuse the report's structural remedies). NEVER fabricate `old_text` — a fabricated `old_text` fails the fuzzy match and silently drops the fix.
3. Prefer the remedy that removes moving pieces (Ousterhout/addyosmani). One finding → one surgical edit (plus its directly-required sibling, e.g., a call site the edit forces). Do NOT bundle unrelated refactors into a requested fix.
4. Honor project conventions and the repo `.rules` (Rust/GPUI: `?` over `unwrap()`/`expect()`; never `let _ =` on fallible ops; no panicking indexing; full variable names; GPUI constraints where applicable). For other languages, follow the surrounding code's idioms.
5. Skip findings you cannot ground: no actionable remedy → `"no_remedy"`; cannot locate `old_text` → `"cannot_locate"`; taste/FYI with no remedy → skip even under `"all"`. The agent applies each `fix_plan` edit via `edit_file`; `applied_count` reflects what was ACTUALLY applied (a failed fuzzy match drops that fix into `fixes_skipped` with reason `"edit_failed"`).
6. Respond with `fix_plan`, `fixes_generated`, `applied_count`, `fixes_skipped`. The convergence re-review should see a reduced `blocker_delta`.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `code-review-scope.j2` | KnowAct | Compute the diff against diff_base from real git output, classify change size (Fagan sizing: ~100 good, ~300 acceptable, ~1000 too large), identify critical paths (auth/payments/data writes/concurrency/unsafe/FFI/secrets/ external data boundaries), and build a lightweight change model (Good Regulator). Consumes prior_review.next_review_focus as the primary emphasis for this pass. Does not judge findings — only models the change. |
| `code-review-perspectives.j2` | KnowAct | Walk the diff through PERFECT-ordered axes (Purpose → Edge cases → Reliability → Form → Evidence → Clarity → Taste) intersected with the addyosmani five-axis (correctness, readability, architecture, security, performance). DETECTION only — records raw, unverdicted findings with file:line + verbatim evidence (no-fiction). Respects focus. For each enabled delegate flag, emits a delegation instruction for the agent to invoke the specialist skill between this step and adjudicate. Accepts pre-populated probe_findings for the FlowDef path. Detection/collection separation (Sauer) — no verdicts here. |
| `code-review-adjudicate.j2` | KnowAct | Turn raw observations into tiered, evidence-backed verdicts (defect COLLECTION). Applies pragmatic-semantics IS/OUGHT + epistemic mode + provenance, frames each as a falsifiable hypothesis (H0/H1 + falsifier), derives severity from constraint force (Prohibition→Blocker, Guideline→Should-fix, Preference→Nit, Informational→FYI), runs grill-me self-challenge, and enforces file:line no-fiction citation (uncited findings rejected, not dropped). Computes blocker_delta vs prior_review. Does NOT re-scan the code (Sauer detection/collection separation). |
| `code-review-report.j2` | KnowAct | Produce a verdict driven by Blocker presence (not nit count): Approve / Request changes / Comment. Group findings by severity, lead with what matters, attach a NAMED structural remedy to every architectural/structural finding, rank the 3 highest-leverage top fixes, and emit coverage honesty (checked / not_checked / residual_risk — never a bare "LGTM"). Emits lessons_learned and next_review_focus for loop closure. |
| `code-review-implement.j2` | KnowAct | Act phase (Fagan rework). SKIPPED when fix_mode == 'none' (the default) via step.condition — the review is review-first; setting fix_mode to a non-`none` value IS the user's consent to modify code. For each finding at/above the requested tier with a concrete remedy, produce ONE surgical edit (file, location, exact verbatim old_text read from the actual file, new_text). Reuses the report's structural remedies. Honors project conventions and .rules. Never fabricates old_text — skips findings it cannot ground. |

To render a template, call the `render_template` tool with the template ref (e.g., `essentialist/essentialist-flow`) and a context object with the required variables.

## Constraints

- All templates are `KnowAct`, `Public`
- `code-review-scope.j2`: compute the diff from real git output; never estimate from the spec. When `prior_review.next_review_focus` is present it MUST be consumed (silently ignoring prior feedback is a feedback-loop violation). Do not judge findings here — only model the change.
- `code-review-perspectives.j2`: DETECTION ONLY — no verdicts, severity, confidence, or falsifiers. Every raw finding cites file:line + a verbatim evidence snippet; uncited observations are dropped, not recorded. Respect `focus` (security always gets a basic pass). Delegation adds depth but does not remove the inline basics pass.
- `code-review-adjudicate.j2`: do NOT re-scan the code; adjudicate the `raw_findings` given. Every finding carries IS/OUGHT, epistemic mode, provenance, constraint force, confidence, a falsifier, a grill-me resolution, and a verbatim citation — missing any → reject (not silently drop). Severity is derived from constraint force; a taste finding is never a Blocker; subjunctive/assessment never exceeds Should-fix; confidence < 0.60 downgrades one tier. An empty `raw_findings` list yields all-zero counts (a clean pass is valid) — do not fabricate. Corroborated ≠ confirmed; use "upheld"/"withstood".
- `code-review-report.j2`: verdict driven by Blocker presence, not nit count. Every Blocker/Should-fix finding carries a concrete remedy AND a falsifier. Coverage honesty is mandatory (checked / not_checked / residual_risk); never a bare "LGTM". `lessons_learned` must be concrete; `next_review_focus` is empty when converged clean. Review-first — no implementation here.
- `code-review-implement.j2`: runs ONLY when `fix_mode` ∈ {blockers, should_fix, all} (`step.condition`, default-deny; absence and `"none"` skip). Read the actual file before emitting `old_text` — never fabricate; prefer to skip than guess. One finding → one surgical edit; do not bundle unrelated refactors. Honor project conventions and `.rules`. `applied_count` reflects what the agent actually applied, not what was generated.
- **Convergence:** evaluate `blocker_delta` (new blockers per pass) after each full iteration. Converged when blocker_delta is zero or stable across 3 iterations. Maximum 10 iterations; escalate if not converged by then. Minimum 2 iterations before declaring convergence, guaranteeing at least one grill-me self-challenge re-pass.
- **Modes:** comprehensive-by-default. The four lenses (adversarial, multi-perspective, refactoring, generative) are inline reasoning, not separate invocations. "Generative" is the `fix_mode`-gated implement phase; a non-`none` `fix_mode` is the caller's consent to modify code.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.