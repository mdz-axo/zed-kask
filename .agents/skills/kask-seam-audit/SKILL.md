---
name: kask-seam-audit
description: "Convergent multi-skill audit of the zed-kask Kask-Zed seam (every live D-seam in DIVERGENCE.md). Three tracks: security (self-contained), refactor-architecture (dead-surface removal), measured GPUI layout (its own layout loop, which also runs standalone before adding card or panel elements). Every finding cites file:line."
audience: platform
---

# Kask Seam Audit

Convergent multi-skill audit-and-refactor engagement over the zed-kask
Kask↔Zed seam. The implementation lives in the registry:

- **Templates**: `kask/registry/templates/kask-seam-audit/*.j2`

This skill has no manifest; it runs purely from this SKILL.md body + the
templates. Invoke the skill via the `skill` tool; the agent reads the SKILL.md
and calls `lisp_eval`, `render_template`, and MCP tools directly as the
methodology instructs.

## When to Use

- Reproducible security + architecture + UI audit of the Kask↔Zed seam
  (every live seam in the `DIVERGENCE.md` table; retired seams are read from the document, not assumed).
- Dead-surface removal + deepening candidates with grep-verified caller
  counts and the essentialist deletion test.
- GPUI measured-layout + Zed interaction-language gaps (Button/IconButton vs
  raw div, PopoverMenu, Tooltip, Toggle vs ToggleFocus, deploy-and-focus).
- Remediations ranked by MCDA (+-20% sensitivity) and applied only if
  essentialist-surviving and seam-scoped.

## When NOT to Use

- Upstream Zed files outside a D-seam — fixes belong in `kask/` behind a seam; this audit governs the seam surface only.
- General review of a change against its spec — use `code-review`.
- Remediation that fails the essentialist gate or requires an upstream non-D-seam edit — the Act phase hard-stops; do not force it through this skill.

## Ontological anchors

- **PKO** — the engagement is a Procedure (spec/execution split); Steps map
  to `pko:Step` with `pko:StepVerification` (the `lisp_eval` tool gates).
- **OWASP LLM Top-10 (2025) / MITRE ATLAS v5.1 / NIST SSDF SP 800-218A** —
  the security-track ontology.
- **de la Torre (2025, arXiv:2506.10021)** — symbolic-neural scaffolding via
  stateless `lisp_eval` tool gates (count / completeness / exclusivity invariants).
- **Ousterhout** — the deep-module deletion test (essentialist G1/G2/G3).

## D/P labelling

Audit findings, per-behavior adjudication, and remediation choice are P —
critiqued by the file:line citation enforcement (a finding without a location
is rejected), the essentialist gates, and the operator's ratification. The
gates are D: Gate A/B/C, Gate FA (functional authority), the Convergence count,
and the measured-layout gate (the five-gate + probe form in the layout loop)
run as `lisp_eval` forms over the template outputs — the convergence score is passed to
final-report, never supplied by the model. Layout measurements are D when
taken from the code (widths, counts); the remedy choice is P, critiqued by
the adversarial probes. Caller-count sweeps (grep) are D.

## Instructions

```
Plan:  enumerate live rows + reconcile the durable ledger; seam-map + prior verification -> Gate A
Do:    for each ranked batch, audit-security -> audit-architecture -> audit-ui -> Gate B
Check: per-behavior adjudication (essentialist + semantics) -> mcda (+ sensitivity)
Act:   record verdicts and evidence; remediate only after the applicable ratification -> Gate C
Converge: lisp open count (uncited/unadjudicated -> 0) within the batch; max 2 re-loops per track
Final: commit the ledger per completed batch; report remaining rows explicitly as pending
```

The Gate A, B, C, FA, Converge, `KSA-1` (measured-layout derived quantities +
5-gate check), `KSA-2` (MCDA weighted totals, top-3, and ±20% perturbation
stability), `KSA-3` (track/severity/defense-layer counts), and `KSA-4`
(probe-list aggregates: `all_probes_passed` fold and `failing_gates`
set-difference) forms live in
`kask/registry/templates/kask-seam-audit/gates.md` (read it with `read_file`);
each `KSA-*` form is also shown copyable with its env block at its call site in
this SKILL.md (`KSA-1` in the layout loop step 2; `KSA-2` and `KSA-3` in the
Registry Templates section). Run them with `lisp_eval` over the template
outputs. The Converge count is passed to `final-report` as
`convergence_score`; `KSA-1` derives `sum_min_widths`, `overflow_delta`,
`text_column_residual`, and `fitts_violations` from `layout-sense`'s primitives
(the model never computes them); `KSA-2` computes the weighted totals, top-3
ranking, and perturbation stability; and `KSA-3` computes the per-track,
per-severity, and defense-layer counts (Jinja-rendered into `final-report`'s
prompt over its own inputs) — the model never supplies them.

- **Initial condition:** the live and retired seams in `DIVERGENCE.md`, the operator's `prior_rules`, the audited commit, and `kask/docs/research/d-seam-audit.md` (if present).
- **Target condition per batch:** every selected behavior has an evidence-backed advisory verdict and a pin status, the ledger is committed, and unresolved decisions have an owner. Pending rows are not audited rows. Gate C applies to remediations, not record-only updates.

| Phase | Type | Oracle / critique |
|-------|------|-------------------|
| Plan, Do (seam-map, audits) | P | Gates A and B |
| Check (adjudicate, mcda) | P | Converge count; the operator on ranking |
| Act (remediate) | P | Gate C and the tests it pins |
| Gates, Converge | D | `lisp_eval` forms in `gates.md` |

## Resumable behavior-level pass

The ledger at `kask/docs/research/d-seam-audit.md` is the worklist, not a second authority for what is live. At each session start, extract rows from the `## The divergence surface` table of `DIVERGENCE.md` (never infer a range or reuse the retired paragraph), and run `bash kask/scripts/check-d-seam-audit-ledger.sh`. Add new rows as `pending`, remove retired rows only after verifying no residue, and rerun the check. Rank pending rows by functional authority first — the operator's current supported function and its revocation/deprecation status (a revoked function outranks every technical signal; a passing pin never justifies retaining one) — then files × changed lines, pin health, live-fire evidence and operator pain; write the next small batch and the ranking rationale to the ledger. Stop when that batch is complete or the context budget requires it; three no-new-state iterations per seam stop and escalate.

For **each behavior**, not merely each D-number:

1. Recover the purpose from the row, `.rules`, `git log --oneline -- <files>` and `git log -S '<identifier>'`; recall `seam:DXX` before updating memory. The row claims a contract; it is not proof. Preserve any uncommitted edits by other streams. **Functional-authority gate (before any git/test probe):** establish (a) the operator's current supported function for this behavior and its revocation/deprecation status (`.rules`, operator rulings, the ledger's decision history), (b) the user-visible loss if the behavior is deleted, and (c) the nearest non-divergent route (upstream or kask-owned) and its maintenance cost. A revoked or deprecated function routes to `retire` — only technical deletion and validation remain, and a passing pin is never retention evidence. If the authority is unknown, record `not_adjudicated` and ask the operator exactly one functional question (what does the user lose if this disappears?) — never a technical question.
2. Enumerate **every current pin named for this behavior**, locate its test target, then run each with `bash kask/scripts/cargo-test-nonzero.sh -p <crate> <filter>` (or the named non-Cargo script). Record missing/failed/unrun individually. Do not silently count a similarly named test or a zero-test run.
3. Look for its warn/error signature in available editor logs. Compare the specific behavior with local `upstream/main`: `git remote -v`, `git log -S '<identifier>' upstream/main`, and the upstream implementation. Record the ref and freshness; no ref or network means `unverified`, not absent. No log signature or no logging means `not observable`, not disproven.
4. Run essentialist in **advisory** mode, with coding-guidelines and refactor-architecture: G1 deletion test against the current wanted function — the question is whether the operator's supported function disappears under deletion, not whether code or tests disappear (a green pin on a deprecated behavior is not retention evidence; label the claim Evidence vs Hypothesis), G2 public surface (7-function rule), G3 pass-through contracts. Apply the existing security/architecture/UI tracks at the behavior's actual reach; UI layout checks are not applicable to a non-UI seam, and a security track outside the batch is not a claim of full-fleet coverage. Label each finding Prohibition / Guardrail / Guideline / Evidence / Hypothesis. Gate B checks citations, not correctness; record oracle strength separately.
5. Map to upstream-rebase vocabulary — the functional-authority gate's verdict leads: a revoked or deprecated function maps to `delete` = retire regardless of pin status, and only the technical deletion/validation work remains. Otherwise: `essential-keep` = retain, `upstreaming-candidate` = retain + issue draft, `demoted-defensive-compatibility` = simplify, `delete` = retire, `already-retired-verify-no-residue`. Update the row, pins and `seam:DXX` memory for keeps/demotions once verified. For deletions, present a red-first removal plan and wait for operator ratification; for upstreaming, draft summary, evidence, discriminator, fix and PR offer, and wait for the operator to send it. Never mark a proposal adopted upstream.
6. Record evidence links, pin and upstream status, date and **actual** pathspec-limited commit in the ledger; verify `git diff --cached --stat` before commit. A shared `DIVERGENCE.md` edit blocks row alignment, not the independent ledger commit: record its owner and leave the seam `partial` until aligned. Rerun the ledger check and Gate B/Converge for batch findings. Apply Gate C only to code remediations (and run red/green pin + residue sweep). Commit status stays `uncommitted` until the hash exists.

The row-by-row ledger is deliberately compact; detailed evidence and upstream issue drafts sit below the table under the same ID. A prior session's `asserted` result never upgrades itself to `oracle-verified`; re-run pins after a changed test or behavior. Close with verdicts, hashes, validation tiers, code red/green (or none), net lines, residue sweep and every open item's closure state and owner. Record evidence-grounded worker progress, then request native human closure when appropriate; never author a human outcome or treat optional forecast scoring as acceptance.

## Composed skills

| Skill | Role | When |
|-------|------|------|
| `refactor-architecture` | executor | architecture track (Do) |
| `coding-guidelines` | lens | adjudicate (Check) — run with essentialist per step 4 |
| `pragmatic-semantics` | lens | adjudicate (Check) |
| `pragmatic-cybernetics` | lens | adjudicate (Check) |
| `essentialist` | lens | adjudicate (Check) + remediation gate (Act) |
| `mcda` | decision | ranking + sensitivity (Check) |

The security track is self-contained in `audit-security.j2` (10 priority
surfaces, OWASP LLM Top-10 / MITRE ATLAS / NIST SSDF framing); it does not
delegate to a separate skill. The UI track uses this skill's own layout loop
(below), which also runs standalone before adding elements to any GPUI
card or panel.

## Measured-layout loop (UI track; formerly ui-layout-discipline)

Layout failures share one root cause: adding elements without measuring. Use
it before adding elements to a card/panel renderer, when a card has more than
two actions or a text column beside actions, when changing a shared card
container, or when a layout looks cramped — not for logic-only or test-only
changes. Measurements are D when taken from the code (widths, counts); the
remedy choice is P, critiqued by the adversarial probes.

1. **Measure & Orient** (`kask-seam-audit/layout-sense`) — render with `target`, `container_width` (dock ~300–400px, center ~600px+), and optional `codebase_context` to measure each child's minimum width (`child_min_widths`, each entry `{element, min_width_px, kind}` with kind fixed | text_column | flex), `container_available_width`, `total_gap_width`, `fitts_min_target` (32px mouse / 44px touch), `action_count` against `action_budget` (≤5 primary per Hick's Law), `sibling_patterns`, `consistency_violation`, `primary_action_visible`, and `on_grid_spacing`. The derived quantities (`sum_min_widths`, `overflow_delta`, `text_column_residual`, `fitts_violations`) are computed by the `KSA-1` gate, never by the model.
2. **Gate (`KSA-1`, D)** — derive the layout quantities and evaluate the five yes/no Fagan gates deterministically with `lisp_eval` over `layout-sense`'s primitive outputs (no LLM gate-decider template):
   - form: `(begin (define sum-all (lambda (cs) (if (is_null cs) 0 (+ (assoc "min_width_px" (car cs)) (sum-all (cdr cs)))))) (define sum-fixed (lambda (cs) (if (is_null cs) 0 (+ (if (string= (assoc "kind" (car cs)) "fixed") (assoc "min_width_px" (car cs)) 0) (sum-fixed (cdr cs)))))) (define fitts-bad (lambda (cs) (if (is_null cs) (list) (append (if (and (string= (assoc "kind" (car cs)) "fixed") (< (assoc "min_width_px" (car cs)) fitts_min_target)) (list (assoc "element" (car cs))) (list)) (fitts-bad (cdr cs)))))) (let ((sum_min_widths (+ (sum-all child_min_widths) total_gap_width))) (let ((overflow_delta (- sum_min_widths container_available_width))) (let ((text_column_residual (- (- container_available_width (sum-fixed child_min_widths)) total_gap_width))) (let ((fitts_violations (fitts-bad child_min_widths))) (let ((failed (append (if (<= overflow_delta 0) (list) (list "no_overflow")) (append (if primary_action_visible (list) (list "primary_action_visible")) (append (if (>= text_column_residual 160) (list) (list "text_column_min_width")) (append (if on_grid_spacing (list) (list "on_grid_spacing")) (if (<= action_count action_budget) (list) (list "action_count_budget")))))))) (list (list "all_gates_passed" (= (length failed) 0)) (list "failing_gates" failed) (list "sum_min_widths" sum_min_widths) (list "overflow_delta" overflow_delta) (list "text_column_residual" text_column_residual) (list "fitts_violations" fitts_violations))))))))`
   - env: `{ "container_available_width": <from layout-sense>, "child_min_widths": <from layout-sense, each {element, min_width_px, kind}>, "total_gap_width": <from layout-sense>, "fitts_min_target": <from layout-sense, 32 mouse / 44 touch>, "action_count": <from layout-sense>, "action_budget": <from layout-sense, 5 per Hick's Law>, "primary_action_visible": <from layout-sense>, "on_grid_spacing": <from layout-sense> }`
   The result object (`all_gates_passed`, `failing_gates`, `sum_min_widths`, `overflow_delta`, `text_column_residual`, `fitts_violations`) binds `decide_result` for steps 3–4. After running `layout-review`, verify overall completion with `lisp_eval` form `(and (= (length failed_gates) 0) (= (length probe_failures) 0))`, env `{ "failed_gates": <KSA-1 failing_gates>, "probe_failures": <layout-review's failing_gates> }` — where `layout-review`'s `failing_gates` is the `KSA-4` gate's output, not a model emission.
3. **Remedy** (`kask-seam-audit/layout-act`) — render with `target` and `decide_result` (the `KSA-1` gate result); for each failing gate, prescribe the canonical GPUI remedy: secondary actions behind `PopoverMenu` with an `IconName::Ellipsis` trigger, `.truncate()` on labels, `flex_shrink_0()` on fixed elements, `min_w_0()` on flexible text columns, or hide-secondary.
4. **Probe** (`kask-seam-audit/layout-review`) — render with `target`, `act_result`, and `decide_result`: a 40-character button label, a German string (~30% longer), a 300px container (the agent panel's minimum dock width, `MIN_PANEL_WIDTH` at `crates/agent_ui/src/agent_panel.rs:108`), 7 actions, and an empty description. Any broken probe rejects the layout and re-enters step 3. Bound: max 2 remedy rounds; a third failing gate set rejects the change — hide the secondary actions or defer, and say so.

Patterns checked: `min_w_0()` on flexible text columns; `flex_shrink_0()` on fixed-width elements; `.truncate()` on labels; `PopoverMenu::new(id).trigger_with_tooltip(IconButton::new(id, IconName::Ellipsis), Tooltip::text(...))` for overflow; `ContextMenu::build(...)` for menu items; `gap_1()`/`gap_2()` on the 4px/8px grid.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `seam-map.j2` | PLAN — read the live seams and the retired list from DIVERGENCE.md, grep `crates/` for each convention prior's artifact (live vs phantom), and derive the audit slices. Read-only. |
| `audit-security.j2` | DO — self-contained security review of the 10 priority surfaces (OWASP LLM Top-10, MITRE ATLAS, NIST SSDF, defense-layer coverage). Every finding cites file:line. |
| `audit-architecture.j2` | DO — find dead surface (trait-with-one-impl, helper-test-only, folded re-exports) and deepening candidates; apply the essentialist deletion test with grep-verified caller counts. |
| `audit-ui.j2` | DO — measured-layout discipline + Zed interaction-language gaps across kask-owned GPUI widgets; Toggle-vs-ToggleFocus and deploy-and-focus traps. |
| `layout-sense.j2` | Layout loop step 1: measure container and child minimum widths (`child_min_widths`, each entry with `kind`: fixed | text_column | flex), `total_gap_width`, `fitts_min_target`, count primary interactive elements against the ≤5 Hick's Law budget, compare sibling conventions, and record primary-action and grid-alignment observations — the primitives `KSA-1` derives the gate quantities from. |
| `layout-act.j2` | Layout loop step 3: the canonical GPUI remedy per failing gate (and per Fitts violation) from the `KSA-1` result object (`decide_result`). |
| `layout-review.j2` | Layout loop step 4: adversarial probes that reject a layout that breaks a gate. |
| `adjudicate.j2` | CHECK — classify each finding by constraint force, run the deletion test, and check the feedback loop. Produces annotated_findings (`task`, `security_findings`, `arch_findings`, `ui_findings`, `prior_verdicts`). |
| `mcda.j2` | CHECK — score remediation candidates on four weighted criteria plus `deepens_module`, and run the essentialist gate (`task`, `annotated_findings`, `operator_priority`). Each score traces to a finding; `KSA-2` (`lisp_eval`) computes the weighted totals, top-3 ranking, and the four ±20% perturbation top-3 stability sets — never the model. |
| `remediate.j2` | ACT — apply only mcda top-ranked remediations surviving essentialist (`task`, `top3`, `ranked_remediations`, `seam_map`); pin each with a test; declare within_kask per touched file; set hard_stop if any touch requires an upstream non-D-seam edit. |
| `final-report.j2` | Consolidate the three tracks, MCDA, applied remediations, hard-stop decision, and convergence score (`task`, `seam_map`, `security_track`, `arch_track`, `ui_track`, `annotated_findings`, `mcda`, `applied_remediations`, `hard_stop`, `convergence_score`, `iterations`); per-track/severity/defense-layer counts are Jinja-computed (`| length`/`selectattr`) over its own inputs and verified via `KSA-3` in `gates.md`. Cite file:line. |

Gate-defect repair is handled inline: if Gate A/B/C finds defects, re-run the
parent template (`seam-map.j2`, the audit template, or `remediate.j2`) with
`gate_defects` in context; there are no separate repair templates.

To render a template, call the `render_template` tool with the template ref (e.g., `kask-seam-audit/seam-map`) and a context object with the required variables.

Copyable `render_template` context blocks (variable names from each template's `[inference]` contract):

`kask-seam-audit/seam-map`:
```json
{
  "task": "<the audit task>",
  "divergence_doc": "DIVERGENCE.md",
  "seam_scope": "kask/",
  "prior_rules": [ { "prior": "<.rules trap name>", "artifact": "<symbol/path>", "expected": "live" } ]
}
```

`kask-seam-audit/audit-security`:
```json
{
  "task": "<the audit task>",
  "seam_scope": "kask/",
  "prior_verdicts": [ { "prior": "...", "artifact": "...", "verdict": "live|phantom", "evidence": "..." } ]
}
```

`kask-seam-audit/audit-architecture`:
```json
{ "task": "<the audit task>", "seam_scope": "kask/" }
```

`kask-seam-audit/audit-ui`:
```json
{ "task": "<the audit task>" }
```

`kask-seam-audit/layout-sense`:
```json
{
  "target": { "file": "crates/<widget>/src/<file>.rs", "function": "render_card", "element_tree": "<the child element tree>" },
  "container_width": "dock",
  "codebase_context": { "siblings": [ { "component": "...", "action_count": 3, "layout_approach": "..." } ] }
}
```

`kask-seam-audit/layout-act`:
```json
{
  "target": { "file": "...", "function": "render_card", "element_tree": "..." },
  "decide_result": { "all_gates_passed": false, "failing_gates": ["no_overflow"], "sum_min_widths": 0, "overflow_delta": 0, "text_column_residual": 0, "fitts_violations": [] }
}
```

`kask-seam-audit/layout-review`:
```json
{
  "target": { "file": "...", "function": "render_card", "element_tree": "..." },
  "act_result": { "remedies": [], "revised_layout": "...", "codebase_pattern_reference": "agent_panel.rs render_panel_options_menu" },
  "decide_result": { "all_gates_passed": false, "failing_gates": [], "sum_min_widths": 0, "overflow_delta": 0, "text_column_residual": 0, "fitts_violations": [] }
}
```

`kask-seam-audit/adjudicate`:
```json
{
  "task": "<the audit task>",
  "security_findings": [ { "id": "KS-01", "severity": "high", "file_line": "path:line", "evidence": "..." } ],
  "arch_findings": [ { "id": "RA-01", "severity": "medium", "file_line": "path:line", "evidence": "..." } ],
  "ui_findings": [ { "id": "UI-01", "severity": "low", "file_line": "path:line", "evidence": "..." } ],
  "defense_layer_coverage": { "layers": [ { "id": 1, "name": "...", "status": "...", "enforcement_point": "path:line" } ], "verdict": "Pass|Conditional|Fail" }
}
```

`kask-seam-audit/mcda`:
```json
{
  "task": "<the audit task>",
  "annotated_findings": [ { "id": "KS-01", "track": "security", "severity": "high", "deletion_test": "n/a", "remediation": "..." } ],
  "operator_priority": "security|codebase|ui"
}
```

`kask-seam-audit/remediate`:
```json
{
  "task": "<the audit task>",
  "ranked_remediations": [ { "id": "KS-01", "remediation": "...", "scores": [0.8, 0.2, 0.0, 0.5], "deepens_module": false, "essentialist_survives": true } ],
  "divergence_surface": [ "<D-seam file path>" ],
  "seam_scope": "kask/"
}
```

`kask-seam-audit/final-report`:
```json
{
  "task": "<the audit task>",
  "seam_map": { "divergence_surface": [], "prior_verdicts": [], "slices": [] },
  "security_track": { "findings": [], "defense_layer_coverage": { "layers": [], "verdict": "Pass" } },
  "arch_track": { "findings": [], "dead_surface_summary": {} },
  "ui_track": { "findings": [], "pattern_gap_summary": {} },
  "annotated_findings": [],
  "mcda": { "weight_vector": {}, "ranked_remediations": [], "weighted_totals": [], "baseline_top3": [], "perturbations": [], "stable_under_perturbation": true },
  "applied_remediations": [],
  "hard_stop": { "triggered": false },
  "convergence_score": 0,
  "iterations": 1
}
```

### `KSA-2` — run over `mcda`'s output (computes what the model never does)

- form: `(begin (define wt (lambda (s w) (+ (* (nth 0 s) (nth 0 w)) (* (nth 1 s) (nth 1 w)) (* (nth 2 s) (nth 2 w)) (* (nth 3 s) (nth 3 w))))) (define totals (lambda (cs w) (if (is_null cs) (list) (cons (wt (assoc "scores" (car cs)) w) (totals (cdr cs) w))))) (define find-max (lambda (cs w) (if (= (length cs) 1) (list (assoc "id" (car cs)) (wt (assoc "scores" (car cs)) w)) (let ((hd (list (assoc "id" (car cs)) (wt (assoc "scores" (car cs)) w))) (tl (find-max (cdr cs) w))) (if (>= (nth 1 tl) (nth 1 hd)) tl hd))))) (define remove-id (lambda (cs id) (if (is_null cs) (list) (if (string= (assoc "id" (car cs)) id) (remove-id (cdr cs) id) (cons (car cs) (remove-id (cdr cs) id)))))) (define top3 (lambda (cs w n) (if (or (= n 0) (is_null cs)) (list) (let ((m (find-max cs w))) (cons (nth 0 m) (top3 (remove-id cs (nth 0 m)) w (- n 1))))))) (define pert-w (lambda (w a i) (if (= i 4) (list) (cons (if (= i a) (* (nth i w) 1.2) (nth i w)) (pert-w w a (+ i 1)))))) (define pert-top3 (lambda (a) (top3 candidates (pert-w weights a 0) 3))) (define subset (lambda (xs ys) (if (is_null xs) t (and (member (car xs) ys) (subset (cdr xs) ys))))) (define same-set (lambda (a b) (and (= (length a) (length b)) (subset a b)))) (let ((base (top3 candidates weights 3))) (let ((p0 (pert-top3 0)) (p1 (pert-top3 1)) (p2 (pert-top3 2)) (p3 (pert-top3 3))) (list (list "weighted_totals" (totals candidates weights)) (list "baseline_top3" base) (list "perturbations" (list (list "security" p0 (not (same-set base p0))) (list "codebase" p1 (not (same-set base p1))) (list "ui" p2 (not (same-set base p2))) (list "cost" p3 (not (same-set base p3))))) (list "stable_under_perturbation" (and (same-set base p0) (same-set base p1) (same-set base p2) (same-set base p3)))))))`
- env: `{ "candidates": [ { "id": "KS-01", "scores": [0.8, 0.2, 0.0, 0.5] } ], "weights": [0.35, 0.25, 0.25, 0.15] }` — `candidates` from `mcda`'s `ranked_remediations` (`scores` in the order `[security, codebase, ui, cost_inverted]`), `weights` from `mcda`'s `weight_vector` in the same order. Merge the result into the `mcda` context for `remediate` (`top3`) and `final-report`.

### `KSA-3` — run over the three tracks' findings (binds the counts `final-report` never asks the model to count)

- form: `(begin (define count-sev (lambda (fs s) (if (is_null fs) 0 (+ (if (string= (assoc "severity" (car fs)) s) 1 0) (count-sev (cdr fs) s))))) (let ((all (append sec_findings (append arch_findings ui_findings)))) (list (list (length sec_findings) (length arch_findings) (length ui_findings)) (list (count-sev all "critical") (count-sev all "high") (count-sev all "medium") (count-sev all "low") (count-sev all "info")) (list (- total_layers (length missing_layers)) (length missing_layers)))))`
- env: `{ "sec_findings": <security_track.findings>, "arch_findings": <arch_track.findings>, "ui_findings": <ui_track.findings>, "total_layers": <security_track.defense_layer_coverage.layers | length>, "missing_layers": [<declared layers with no live enforcement point>] }`

### `KSA-4` — run over `layout-review`'s probes (folds the aggregates the model never computes)

- form: `(begin (define all-passed (lambda (ps) (if (is_null ps) t (and (assoc "overall_passed" (car ps)) (all-passed (cdr ps)))))) (define failed-in (lambda (grs) (if (is_null grs) (list) (if (assoc "passed" (car grs)) (failed-in (cdr grs)) (cons (assoc "gate_name" (car grs)) (failed-in (cdr grs))))))) (define failed-gates (lambda (ps) (if (is_null ps) (list) (append (failed-in (assoc "gate_results" (car ps))) (failed-gates (cdr ps)))))) (define dedup (lambda (xs) (if (is_null xs) (list) (cons (car xs) (dedup (remove-if-member (car xs) (cdr xs))))))) (define remove-if-member (lambda (x xs) (if (is_null xs) (list) (if (string= x (car xs)) (remove-if-member x (cdr xs)) (cons (car xs) (remove-if-member x (cdr xs))))))) (list (all-passed probes) (dedup (failed-gates probes))))`
- env: `{ "probes": [ { "probe_name": "long_label", "gate_results": [ {"gate_name": "overflow", "passed": false} ], "overall_passed": false } ] }` — `probes` is `layout-review`'s own output array, each entry carrying its per-gate `gate_results` and its `overall_passed` judgment. The gate returns `[all_probes_passed, failing_gates]`; `all_probes_passed` is the fold over every probe's `overall_passed`, and `failing_gates` is the deduplicated set-difference over every probe's `gate_results`. The model emits neither.

## Regression case

All receipts executed live through `lisp_eval` (2026-10-01, backfill pass),
over the convergence gate:

- Zero failed gates and zero probe failures → `true`.
- One failed gate → `false`.

The skill's forms are executed at use time, never anchored in code.

## Constraints

- Hard-stop on any remediation requiring an upstream non-D-seam edit.
- No fabrication: every finding cites `file:line` or is `deferred` with a
  reason; every MCDA score traces to a finding.
- `lisp_eval` tool gates are authoritative between phases; condition branches
  reference real step-N result keys, not LLM self-assessment.
- `ledger.span_namespace` is `reg.skill.kask-seam-audit` (CI-enforced).

## Example invocation

```
task: "Audit the Kask↔Zed seam for security, dead surface, and GPUI gaps."
operator_priority: "security"
prior_rules:
  - { prior: "McpRuntime::invoke OCAP gate", artifact: "McpRuntime::invoke", expected: "live" }
  - { prior: "propagate_taint_for_binding", artifact: "propagate_taint_for_binding", expected: "live" }
```

