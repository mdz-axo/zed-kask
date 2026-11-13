---
name: kask-seam-audit
description: "Convergent multi-skill audit of the zed-kask Kask-Zed seam (every live D-seam in DIVERGENCE.md). Three tracks: security (self-contained), refactor-architecture (dead-surface removal), measured GPUI layout (its own layout loop, which also runs standalone before adding card or panel elements). Every finding cites file:line."
audience: platform
---

# Kask Seam Audit

Convergent multi-skill audit-and-refactor engagement over the zed-kask
Kask↔Zed seam. The implementation lives in the registry:

- **Templates**: `kask/registry/templates/kask-seam-audit/*.j2`
- **Seam map script**: `kask/scripts/audit/seam-map.sh`

This skill has no manifest; it runs purely from this SKILL.md body + the
templates. Invoke the skill via the `skill` tool; the agent reads the SKILL.md
and calls `lisp_eval`, `render_template`, `terminal` and MCP tools directly as the
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
gates are D: Gate B, Gate FA (functional authority and adjudication coverage),
`KSA-2`, Gate C and the measured-layout gates `KSA-1` and `KSA-4` run as
`lisp_eval` forms over the template outputs. The seam map — the divergence
surface and each prior's live/phantom verdict — is D: a script computes it.
Layout measurements are D when taken from the code (widths, gaps); the remedy
choice is P, critiqued by the adversarial probes. Caller-count sweeps (grep)
are D.

## Instructions

```
Plan:  enumerate live rows + reconcile the durable ledger; seam-map.sh (surface + prior verdicts)
Do:    for each ranked batch, the three audit tracks together -> Gate B
Check: per-behavior adjudication (essentialist + semantics) -> Gate FA -> mcda -> KSA-2
Act:   record verdicts and evidence; remediate only after the applicable ratification -> Gate C
Converge: Gate FA convergence_score 0 within the batch; max 2 repair re-runs per gate
Final: commit the ledger per completed batch; report remaining rows explicitly as pending
```

1. **Seam map (D).** Write the operator's `prior_rules` to a JSON file and run `bash kask/scripts/audit/seam-map.sh <prior_rules.json>` via `terminal`. It returns `divergence_surface` (the existing paths of the live rows of `DIVERGENCE.md`'s numbered table; an entry ending in `/` covers that directory), `unresolved` (path-shaped entries that no longer exist — removal records) and `prior_verdicts` (each prior `live` when its artifact is an existing path or a code match under `crates/` or `kask/`, else `phantom`, with the evidence). A phantom prior drawn from `.rules` is itself a finding; report it, do not edit `.rules`. No model reads the register.
2. **Audit tracks (P).** Render `kask-seam-audit/audit-security`, `kask-seam-audit/audit-architecture` and `kask-seam-audit/audit-ui`. None reads another's output: issue the three renders together and run the three tracks side by side. The UI track uses the layout loop below.
3. **Gate B (D).** Run Gate B over the three tracks' findings. `passed: false` re-renders the owning track with `gate_defects` set to that track's `[id, code]` pairs. Keep `per_track` and `per_severity` for the report.
4. **Adjudicate (P).** Render `kask-seam-audit/adjudicate` with the three findings lists and the security track's `defense_layer_coverage`.
5. **Gate FA (D).** Run Gate FA over all findings and `annotated_findings`. `passed: false` re-renders adjudicate with `gate_defects` set to the `defects` pairs plus the `unadjudicated` ids. Its `convergence_score` is the batch's convergence count.
6. **Rank (P scores, D ranking).** Render `kask-seam-audit/mcda` with `annotated_findings`, then run `KSA-2` with `operator_priority` and the returned `candidates`. The model scores; the gate sets the weights, excludes candidates the deletion test rules out, ranks the top 3 and tests ±20% stability. An `error` result means `operator_priority` is not one of `security`, `codebase`, `ui`: ask the operator.
7. **Remediate (P, after ratification).** Render `kask-seam-audit/remediate` with the `KSA-2` `top3`, the `candidates`, and the seam map's `divergence_surface`.
8. **Gate C (D).** Run Gate C over `applied_remediations`. It computes which touched files are inside the seam from the reported paths. `hard_stop_required: true` stops the engagement and surfaces to the operator. `passed: false` otherwise re-renders remediate with `gate_defects`.
9. **Report (P).** Render `kask-seam-audit/final-report`. Its computed-facts block carries the counts, the top 3, stability and the unremediated blocking findings; the model writes the verdict, the deferred list and the narrative. Report the block beside the model's output.

Each gate allows at most two repair re-runs; defects that remain after the
second stop that phase and are recorded in the ledger with an owner, and the
batch is reported as incomplete.

The six gate forms are in the Gates section below: each entry gives the form,
its env and what its result fields mean. This is the only copy of each form.

- **Initial condition:** the live and retired seams in `DIVERGENCE.md`, the operator's `prior_rules`, the audited commit, and `kask/docs/research/d-seam-audit.md` (if present).
- **Target condition per batch:** every selected behavior has an evidence-backed advisory verdict and a pin status, the ledger is committed, and unresolved decisions have an owner. Pending rows are not audited rows. Gate C applies to remediations, not record-only updates.

| Phase | Type | Oracle / critique |
|-------|------|-------------------|
| Plan (seam map) | D | `kask/scripts/audit/seam-map.sh` |
| Do (audits) | P | Gate B |
| Check (adjudicate, mcda) | P | Gate FA; `KSA-2`; the operator on ranking |
| Act (remediate) | P | Gate C and the tests it pins |
| Gates | D | the `lisp_eval` forms in the Gates section |

## Resumable behavior-level pass

The ledger at `kask/docs/research/d-seam-audit.md` is the worklist, not a second authority for what is live. At each session start, extract rows from the `## The divergence surface` table of `DIVERGENCE.md` (never infer a range or reuse the retired paragraph), and run `bash kask/scripts/check-d-seam-audit-ledger.sh`. Add new rows as `pending`, remove retired rows only after verifying no residue, and rerun the check. Rank pending rows by functional authority first — the operator's current supported function and its revocation/deprecation status (a revoked function outranks every technical signal; a passing pin never justifies retaining one) — then files × changed lines, pin health, live-fire evidence and operator pain; write the next small batch and the ranking rationale to the ledger. Stop when that batch is complete or the context budget requires it; three no-new-state iterations per seam stop and escalate.

For **each behavior**, not merely each D-number:

1. Recover the purpose from the row, `.rules`, `git log --oneline -- <files>` and `git log -S '<identifier>'`; recall `seam:DXX` before updating memory. The row claims a contract; it is not proof. Preserve any uncommitted edits by other streams. **Functional-authority gate (before any git/test probe):** establish (a) the operator's current supported function for this behavior and its revocation/deprecation status (`.rules`, operator rulings, the ledger's decision history), (b) the user-visible loss if the behavior is deleted, and (c) the nearest non-divergent route (upstream or kask-owned) and its maintenance cost. A revoked or deprecated function routes to `retire` — only technical deletion and validation remain, and a passing pin is never retention evidence. If the authority is unknown, record `not_adjudicated` and ask the operator exactly one functional question (what does the user lose if this disappears?) — never a technical question.
2. Enumerate **every current pin named for this behavior**, locate its test target, then run each with `bash kask/scripts/cargo-test-nonzero.sh -p <crate> <filter>` (or the named non-Cargo script). Record missing/failed/unrun individually. Do not silently count a similarly named test or a zero-test run.
3. Look for its warn/error signature in available editor logs. Compare the specific behavior with local `upstream/main`: `git remote -v`, `git log -S '<identifier>' upstream/main`, and the upstream implementation. Record the ref and freshness; no ref or network means `unverified`, not absent. No log signature or no logging means `not observable`, not disproven.
4. Run essentialist in **advisory** mode, with coding-guidelines and refactor-architecture: G1 deletion test against the current wanted function — the question is whether the operator's supported function disappears under deletion, not whether code or tests disappear (a green pin on a deprecated behavior is not retention evidence; label the claim Evidence vs Hypothesis), G2 public surface (7-function rule), G3 pass-through contracts. Apply the existing security/architecture/UI tracks at the behavior's actual reach; UI layout checks are not applicable to a non-UI seam, and a security track outside the batch is not a claim of full-fleet coverage. Label each finding Prohibition / Guardrail / Guideline / Evidence / Hypothesis. Gate B checks citations, not correctness; record oracle strength separately.
5. Map to upstream-rebase vocabulary — the functional-authority gate's verdict leads: a revoked or deprecated function maps to `delete` = retire regardless of pin status, and only the technical deletion/validation work remains. Otherwise: `essential-keep` = retain, `upstreaming-candidate` = retain + issue draft, `demoted-defensive-compatibility` = simplify, `delete` = retire, `already-retired-verify-no-residue`. Update the row, pins and `seam:DXX` memory for keeps/demotions once verified. For deletions, present a red-first removal plan and wait for operator ratification; for upstreaming, draft summary, evidence, discriminator, fix and PR offer, and wait for the operator to send it. Never mark a proposal adopted upstream.
6. Record evidence links, pin and upstream status, date and **actual** pathspec-limited commit in the ledger; verify `git diff --cached --stat` before commit. A shared `DIVERGENCE.md` edit blocks row alignment, not the independent ledger commit: record its owner and leave the seam `partial` until aligned. Rerun the ledger check, Gate B and Gate FA for batch findings. Apply Gate C only to code remediations (and run red/green pin + residue sweep). Commit status stays `uncommitted` until the hash exists.

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
changes. Measurements are D when taken from the code (widths, gaps); the
remedy choice is P, critiqued by the adversarial probes.

1. **Measure** (`kask-seam-audit/layout-sense`, P reading of the code) — render with `target`, `container_width` (dock ~300–400px, center ~600px+), and optional `codebase_context`. It returns primitives only: `container_outer_width`, `padding_px`, `border_px`, `gap_px` (one entry per gap), `child_min_widths` (each `{element, min_width_px, kind, primary_action, is_label}`), `fitts_min_target` (32px mouse / 44px touch), `primary_action_visible`, `sibling_patterns` and `consistency_violation`. Report a `consistency_violation` as a UI finding.
2. **Gate (`KSA-1`, D)** — run `KSA-1` over those primitives. It derives the available width, the totals, the action count (≤5 primary per Hick's Law) and the grid check, evaluates the five gates, and re-runs them for a 300px container and for labels 30% longer. The result binds `decide_result`. All gates and both probes passing with no remedy needed skips to step 4.
3. **Remedy** (`kask-seam-audit/layout-act`, P) — render with `target` and `decide_result`; for each failing gate or failing probe, prescribe the canonical GPUI remedy: secondary actions behind `PopoverMenu` with an `IconName::Ellipsis` trigger, `.truncate()` on labels, `flex_shrink_0()` on fixed elements, `min_w_0()` on flexible text columns, or hide-secondary. Then re-measure the revised layout (step 1) and re-run `KSA-1` (step 2).
4. **Probe** (`kask-seam-audit/layout-review`, P) — render with `target`, `act_result` and the latest `decide_result`: a 40-character button label, 7 actions, an empty description, and the squint test. Then run `KSA-4` over the latest `KSA-1` result and the review's output. `accepted: false` re-enters step 3 with `failing_gates`. Bound: max 2 remedy rounds; a third failing result rejects the change — hide the secondary actions or defer, and say so.

Patterns checked: `min_w_0()` on flexible text columns; `flex_shrink_0()` on fixed-width elements; `.truncate()` on labels; `PopoverMenu::new(id).trigger_with_tooltip(IconButton::new(id, IconName::Ellipsis), Tooltip::text(...))` for overflow; `ContextMenu::build(...)` for menu items; `gap_1()`/`gap_2()` on the 4px/8px grid.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `audit-security.j2` | DO — self-contained security review of the 10 priority surfaces (OWASP LLM Top-10, MITRE ATLAS, NIST SSDF, defense-layer coverage). Every finding cites file:line. |
| `audit-architecture.j2` | DO — find dead surface (trait-with-one-impl, helper-test-only, folded re-exports) and deepening candidates; apply the essentialist deletion test with grep-verified caller counts. |
| `audit-ui.j2` | DO — measured-layout discipline + Zed interaction-language gaps across kask-owned GPUI widgets; Toggle-vs-ToggleFocus and deploy-and-focus traps. |
| `layout-sense.j2` | Layout loop step 1: measure the container, its padding, border and gaps, and each child's minimum width; flag primary actions and labels. Primitives only — `KSA-1` derives the rest. |
| `layout-act.j2` | Layout loop step 3: the canonical GPUI remedy per failing gate, failing probe and Fitts violation from the `KSA-1` result (`decide_result`). |
| `layout-review.j2` | Layout loop step 4: the three judgment probes (long label, 7 actions, empty description) and the squint test. |
| `adjudicate.j2` | CHECK — per finding: functional authority and `retire`, constraint force, the deletion test and the feedback-loop verdict. |
| `mcda.j2` | CHECK — score each remediation candidate on four criteria and judge `deepens_module`. `KSA-2` computes everything else. |
| `remediate.j2` | ACT — apply only the `KSA-2` top-ranked remediations; pin each with a test; report every touched path; set the hard stop when a remediation needs a file outside the seam. |
| `final-report.j2` | Verdict, deferred remediations and a cited narrative, written beside a computed-facts block. |

To render a template, call the `render_template` tool with the template ref (e.g., `kask-seam-audit/audit-security`) and a context object with the required variables. `gate_defects` is omitted on a first render and set on a repair re-run.

Copyable `render_template` context blocks (variable names from each template's `[inference]` contract):

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
  "decide_result": { "<the whole KSA-1 result object>": "" }
}
```

`kask-seam-audit/layout-review`:
```json
{
  "target": { "file": "...", "function": "render_card", "element_tree": "..." },
  "act_result": { "remedies": [], "revised_layout": "...", "codebase_pattern_reference": "agent_panel.rs render_panel_options_menu" },
  "decide_result": { "<the whole KSA-1 result object for the revised layout>": "" }
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
  "annotated_findings": [ { "id": "KS-01", "deletion_test": "n/a", "remediation": "..." } ]
}
```

`kask-seam-audit/remediate`:
```json
{
  "task": "<the audit task>",
  "top3": [ "KS-01" ],
  "candidates": [ { "id": "KS-01", "remediation": "...", "scores": [0.8, 0.2, 0.0, 0.5], "deepens_module": false, "deletion_test": "n/a" } ],
  "divergence_surface": [ "<path from seam-map.sh>" ],
  "seam_scope": "kask/"
}
```

`kask-seam-audit/final-report`:
```json
{
  "task": "<the audit task>",
  "security_track": { "findings": [], "defense_layer_coverage": { "layers": [], "verdict": "Pass" } },
  "arch_track": { "findings": [], "dead_surface_summary": {} },
  "ui_track": { "findings": [], "pattern_gap_summary": {} },
  "annotated_findings": [],
  "gate_b": { "<the whole Gate B result>": "" },
  "ksa_2": { "<the whole KSA-2 result>": "" },
  "applied_remediations": [],
  "hard_stop": { "triggered": false },
  "convergence_score": 0
}
```

## Gates

Six `lisp_eval` forms. Each returns an object; the pass condition is its
`passed`, `all_gates_passed` or `accepted` field. This is the only copy of
each form.

### Gate B — the three tracks' findings

env: `{ "sec_findings": <audit-security findings>, "arch_findings": <audit-architecture findings>, "ui_findings": <audit-ui findings> }`

Every finding has an `id`; a severity in `critical|high|medium|low|info`
(`high|medium|low` on the UI track); and either a non-empty `file_line`, or
`deferred: true` with a non-empty `deferred_reason`. Ids are unique across the
three tracks. `defects` lists `[id, code]` pairs: `finding_without_id`,
`invalid_severity`, `uncited_and_not_deferred`, `duplicate_id`. `per_track`
(`[security, architecture, ui]`) and `per_severity`
(`[critical, high, medium, low, info]`) are the counts for the final report.

```lisp
(begin (define has-text (lambda (v) (and (stringp v) (not (string= v ""))))) (define track-defects (lambda (fs sevs) (if (is_null fs) (list) (let ((f (car fs))) (append (if (has-text (assoc "id" f)) (list) (list (list "?" "finding_without_id"))) (append (if (member (assoc "severity" f) sevs) (list) (list (list (assoc "id" f) "invalid_severity"))) (append (if (or (has-text (assoc "file_line" f)) (and (eq (assoc "deferred" f) t) (has-text (assoc "deferred_reason" f)))) (list) (list (list (assoc "id" f) "uncited_and_not_deferred"))) (track-defects (cdr fs) sevs)))))))) (define ids-of (lambda (fs) (if (is_null fs) (list) (cons (assoc "id" (car fs)) (ids-of (cdr fs)))))) (define dups (lambda (xs) (if (is_null xs) (list) (if (member (car xs) (cdr xs)) (cons (list (car xs) "duplicate_id") (dups (cdr xs))) (dups (cdr xs)))))) (define count-sev (lambda (fs s) (if (is_null fs) 0 (+ (if (string= (assoc "severity" (car fs)) s) 1 0) (count-sev (cdr fs) s))))) (let ((full (list "critical" "high" "medium" "low" "info")) (all (append sec_findings (append arch_findings ui_findings)))) (let ((defects (append (track-defects sec_findings full) (append (track-defects arch_findings full) (append (track-defects ui_findings (list "high" "medium" "low")) (dups (ids-of all))))))) (list (list "passed" (is_null defects)) (list "defects" defects) (list "per_track" (list (length sec_findings) (length arch_findings) (length ui_findings))) (list "per_severity" (list (count-sev all "critical") (count-sev all "high") (count-sev all "medium") (count-sev all "low") (count-sev all "info")))))))
```

### Gate FA — adjudication coverage and functional authority

env: `{ "findings": <the three tracks' findings, concatenated>, "annotated_findings": <adjudicate's output> }`

Every annotated finding has a non-empty `current_function` and an
`authority_status` in `supported|revoked|deprecated|not_adjudicated`; a
`revoked` or `deprecated` status has `retire: true` (a passing pin is never
retention evidence for a revoked function). Every finding has an annotated
entry. `defects` lists `[id, code]` pairs: `no_current_function`,
`invalid_authority_status`, `revoked_function_not_retired`. `unadjudicated`
lists finding ids with no annotated entry. `convergence_score` is the number
of defects plus unadjudicated findings; 0 is clean. The authority verdict
itself is the operator's judgment; this gate checks that each finding carries
one and that its consequence was applied.

```lisp
(begin (define has-text (lambda (v) (and (stringp v) (not (string= v ""))))) (define defects-of (lambda (fs) (if (is_null fs) (list) (let ((f (car fs))) (append (if (has-text (assoc "current_function" f)) (list) (list (list (assoc "id" f) "no_current_function"))) (append (if (member (assoc "authority_status" f) (list "supported" "revoked" "deprecated" "not_adjudicated")) (list) (list (list (assoc "id" f) "invalid_authority_status"))) (append (if (and (member (assoc "authority_status" f) (list "revoked" "deprecated")) (not (eq (assoc "retire" f) t))) (list (list (assoc "id" f) "revoked_function_not_retired")) (list)) (defects-of (cdr fs))))))))) (define ids-of (lambda (fs) (if (is_null fs) (list) (cons (assoc "id" (car fs)) (ids-of (cdr fs)))))) (define open-ids (lambda (fs adj) (if (is_null fs) (list) (if (member (assoc "id" (car fs)) adj) (open-ids (cdr fs) adj) (cons (assoc "id" (car fs)) (open-ids (cdr fs) adj)))))) (let ((defects (defects-of annotated_findings)) (open (open-ids findings (ids-of annotated_findings)))) (list (list "passed" (and (is_null defects) (is_null open))) (list "defects" defects) (list "unadjudicated" open) (list "convergence_score" (+ (length defects) (length open))))))
```

### KSA-2 — MCDA weights, deletion-test eligibility, top-3 and ±20% stability

env: `{ "operator_priority": "security|codebase|ui", "candidates": [ { "id": <finding id>, "scores": [security, codebase, ui, cost_inverted], "deepens_module": <bool>, "deletion_test": <from the annotated finding> } ] }`

`weights` come from `operator_priority` (security `[0.35, 0.25, 0.25, 0.15]`,
codebase `[0.25, 0.4, 0.2, 0.15]`, ui `[0.25, 0.2, 0.4, 0.15]`; any other value
returns `error`). A candidate is eligible when its `deletion_test` is
`complexity_vanishes` or `n/a`, or `complexity_reappears` with
`deepens_module: true`; `unclear` is never eligible. `excluded` lists the
ineligible ids. `weighted_totals` covers every candidate; `top3` ranks the
eligible ones (ties keep the earlier candidate). `perturbations` recomputes
the top-3 under each single-axis +20% weight; `stable_under_perturbation` is
true when the top-3 set is unchanged under all four.

```lisp
(begin (define weights (cond ((string= operator_priority "security") (list 0.35 0.25 0.25 0.15)) ((string= operator_priority "codebase") (list 0.25 0.4 0.2 0.15)) ((string= operator_priority "ui") (list 0.25 0.2 0.4 0.15)) (t (list)))) (define wt (lambda (s w) (+ (* (nth 0 s) (nth 0 w)) (* (nth 1 s) (nth 1 w)) (* (nth 2 s) (nth 2 w)) (* (nth 3 s) (nth 3 w))))) (define survives (lambda (c) (let ((d (assoc "deletion_test" c))) (cond ((string= d "complexity_vanishes") t) ((string= d "n/a") t) ((string= d "complexity_reappears") (eq (assoc "deepens_module" c) t)) (t nil))))) (define keep (lambda (cs) (if (is_null cs) (list) (if (survives (car cs)) (cons (car cs) (keep (cdr cs))) (keep (cdr cs)))))) (define drop-ids (lambda (cs) (if (is_null cs) (list) (if (survives (car cs)) (drop-ids (cdr cs)) (cons (assoc "id" (car cs)) (drop-ids (cdr cs))))))) (define totals (lambda (cs w) (if (is_null cs) (list) (cons (list (assoc "id" (car cs)) (wt (assoc "scores" (car cs)) w)) (totals (cdr cs) w))))) (define find-max (lambda (cs w) (if (= (length cs) 1) (list (assoc "id" (car cs)) (wt (assoc "scores" (car cs)) w)) (let ((hd (list (assoc "id" (car cs)) (wt (assoc "scores" (car cs)) w))) (tl (find-max (cdr cs) w))) (if (> (nth 1 tl) (nth 1 hd)) tl hd))))) (define remove-id (lambda (cs id) (if (is_null cs) (list) (if (string= (assoc "id" (car cs)) id) (remove-id (cdr cs) id) (cons (car cs) (remove-id (cdr cs) id)))))) (define top3 (lambda (cs w n) (if (or (= n 0) (is_null cs)) (list) (let ((m (find-max cs w))) (cons (nth 0 m) (top3 (remove-id cs (nth 0 m)) w (- n 1))))))) (define pert-w (lambda (w a i) (if (= i 4) (list) (cons (if (= i a) (* (nth i w) 1.2) (nth i w)) (pert-w w a (+ i 1)))))) (define subset (lambda (xs ys) (if (is_null xs) t (and (member (car xs) ys) (subset (cdr xs) ys))))) (define same-set (lambda (a b) (and (= (length a) (length b)) (subset a b)))) (if (is_null weights) (list (list "error" "unknown_operator_priority")) (let ((eligible (keep candidates))) (let ((base (top3 eligible weights 3))) (let ((p0 (top3 eligible (pert-w weights 0 0) 3)) (p1 (top3 eligible (pert-w weights 1 0) 3)) (p2 (top3 eligible (pert-w weights 2 0) 3)) (p3 (top3 eligible (pert-w weights 3 0) 3))) (list (list "weights" weights) (list "weighted_totals" (totals candidates weights)) (list "excluded" (drop-ids candidates)) (list "top3" base) (list "perturbations" (list (list "security" p0 (not (same-set base p0))) (list "codebase" p1 (not (same-set base p1))) (list "ui" p2 (not (same-set base p2))) (list "cost" p3 (not (same-set base p3))))) (list "stable_under_perturbation" (and (same-set base p0) (same-set base p1) (same-set base p2) (same-set base p3)))))))))
```

### Gate C — applied remediations

env: `{ "applied_remediations": <remediate's output, each {id, touched_files: [path], test_added: <bool>}>, "top3": <KSA-2 top3>, "divergence_surface": <seam-map.sh divergence_surface>, "seam_scope": "kask/", "hard_stop_triggered": <remediate's hard_stop.triggered> }`

Every applied remediation is in `top3`, has `test_added: true`, and touches
only allowed files. A path is allowed when it starts with `seam_scope`, equals
a `divergence_surface` entry, or sits under a `divergence_surface` entry that
ends with `/`. `defects` lists `[id, code]` pairs: `not_top_ranked`,
`remediation_missing_test`, `touched_file_outside_seam`. `outside_seam` lists
the offending paths; `hard_stop_required` is true when there is one. `passed`
needs no defects and no declared hard stop.

```lisp
(begin (define allowed (lambda (p surface) (if (starts-with seam_scope p) t (if (is_null surface) nil (let ((s (car surface))) (if (or (string= s p) (and (ends-with "/" s) (starts-with s p))) t (allowed p (cdr surface)))))))) (define outside (lambda (ps) (if (is_null ps) (list) (if (allowed (car ps) divergence_surface) (outside (cdr ps)) (cons (car ps) (outside (cdr ps))))))) (define defects-of (lambda (rs) (if (is_null rs) (list) (let ((r (car rs))) (append (if (member (assoc "id" r) top3) (list) (list (list (assoc "id" r) "not_top_ranked"))) (append (if (eq (assoc "test_added" r) t) (list) (list (list (assoc "id" r) "remediation_missing_test"))) (append (if (is_null (outside (assoc "touched_files" r))) (list) (list (list (assoc "id" r) "touched_file_outside_seam"))) (defects-of (cdr rs))))))))) (define all-outside (lambda (rs) (if (is_null rs) (list) (append (outside (assoc "touched_files" (car rs))) (all-outside (cdr rs)))))) (let ((defects (defects-of applied_remediations)) (out (all-outside applied_remediations))) (list (list "passed" (and (is_null defects) (not hard_stop_triggered))) (list "defects" defects) (list "outside_seam" out) (list "hard_stop_required" (not (is_null out))))))
```

### KSA-1 — measured layout: derived quantities, five gates, two probes

env: `{ "container_outer_width": <px>, "padding_px": [<each horizontal padding, px>], "border_px": <total horizontal border, px>, "gap_px": [<each gap between children, px>], "child_min_widths": [ { "element": <name>, "min_width_px": <px>, "kind": "fixed|text_column|flex", "primary_action": <bool>, "is_label": <bool> } ], "fitts_min_target": <32 mouse | 44 touch>, "primary_action_visible": <bool> }`

Derives `container_available_width` (outer − padding − border),
`sum_min_widths`, `overflow_delta`, `text_column_residual` (available − fixed
children − gaps), `action_count` (children with `primary_action: true`) and
`fitts_violations` (fixed children below the floor). The five gates:
`no_overflow` (`overflow_delta <= 0`), `primary_action_visible`,
`text_column_min_width` (`text_column_residual >= 160`), `on_grid_spacing`
(every gap and padding in `0 2 4 6 8 12 16 20 24 32`), `action_count_budget`
(`action_count <= 5`, Hick's Law). `failing_gates` names the failures.
`probe_narrow_300` re-runs the gates at a 300px outer width, and
`probe_labels_plus_30pct` re-runs them with every fixed `is_label` child 30%
wider; each is the list of gates that fail under that probe.

```lisp
(begin (define width-of (lambda (c scale) (if (string= (assoc "kind" c) "fixed") (if (eq (assoc "is_label" c) t) (* (assoc "min_width_px" c) scale) (assoc "min_width_px" c)) (assoc "min_width_px" c)))) (define sum-all (lambda (cs scale) (if (is_null cs) 0 (+ (width-of (car cs) scale) (sum-all (cdr cs) scale))))) (define sum-fixed (lambda (cs scale) (if (is_null cs) 0 (+ (if (string= (assoc "kind" (car cs)) "fixed") (width-of (car cs) scale) 0) (sum-fixed (cdr cs) scale))))) (define fitts-bad (lambda (cs) (if (is_null cs) (list) (append (if (and (string= (assoc "kind" (car cs)) "fixed") (< (assoc "min_width_px" (car cs)) fitts_min_target)) (list (assoc "element" (car cs))) (list)) (fitts-bad (cdr cs)))))) (define count-primary (lambda (cs) (if (is_null cs) 0 (+ (if (eq (assoc "primary_action" (car cs)) t) 1 0) (count-primary (cdr cs)))))) (define count-off-grid (lambda (vs) (if (is_null vs) 0 (+ (if (member (car vs) (list 0 2 4 6 8 12 16 20 24 32)) 0 1) (count-off-grid (cdr vs)))))) (define sum-list (lambda (vs) (if (is_null vs) 0 (+ (car vs) (sum-list (cdr vs)))))) (define gates-at (lambda (available scale) (let ((gaps (sum-list gap_px))) (let ((overflow (- (+ (sum-all child_min_widths scale) gaps) available)) (residual (- (- available (sum-fixed child_min_widths scale)) gaps))) (append (if (<= overflow 0) (list) (list "no_overflow")) (append (if primary_action_visible (list) (list "primary_action_visible")) (append (if (>= residual 160) (list) (list "text_column_min_width")) (append (if (= (count-off-grid (append gap_px padding_px)) 0) (list) (list "on_grid_spacing")) (if (<= (count-primary child_min_widths) 5) (list) (list "action_count_budget")))))))))) (let ((available (- (- container_outer_width (sum-list padding_px)) border_px)) (gaps (sum-list gap_px))) (let ((failed (gates-at available 1))) (list (list "all_gates_passed" (is_null failed)) (list "failing_gates" failed) (list "container_available_width" available) (list "sum_min_widths" (+ (sum-all child_min_widths 1) gaps)) (list "overflow_delta" (- (+ (sum-all child_min_widths 1) gaps) available)) (list "text_column_residual" (- (- available (sum-fixed child_min_widths 1)) gaps)) (list "action_count" (count-primary child_min_widths)) (list "fitts_violations" (fitts-bad child_min_widths)) (list "probe_narrow_300" (gates-at (- (- 300 (sum-list padding_px)) border_px) 1)) (list "probe_labels_plus_30pct" (gates-at available 1.3))))))
```

### KSA-4 — layout acceptance: the gates, the computed probes and the judgment probes

env: `{ "failing_gates": <KSA-1 failing_gates>, "probe_narrow_300": <KSA-1 probe_narrow_300>, "probe_labels_plus_30pct": <KSA-1 probe_labels_plus_30pct>, "probes": <layout-review probes>, "hierarchy_present": <layout-review squint_test_result.hierarchy_present> }`

Bind the `KSA-1` fields from the gate run over the re-measured revised layout.
`accepted` needs no failing gate, no failing computed probe, all three
judgment probes (`long_label`, `high_action_count`, `empty_description`)
present and passed, and a visual hierarchy. `missing_probes` and
`failing_probes` name the judgment probes; `failing_gates` is every gate that
failed or that a failing probe broke, without duplicates.

```lisp
(begin (define names (list "long_label" "high_action_count" "empty_description")) (define names-of (lambda (ps) (if (is_null ps) (list) (cons (assoc "probe_name" (car ps)) (names-of (cdr ps)))))) (define missing (lambda (ns have) (if (is_null ns) (list) (if (member (car ns) have) (missing (cdr ns) have) (cons (car ns) (missing (cdr ns) have)))))) (define failed (lambda (ps) (if (is_null ps) (list) (if (eq (assoc "passed" (car ps)) t) (failed (cdr ps)) (cons (assoc "probe_name" (car ps)) (failed (cdr ps))))))) (define broken (lambda (ps) (if (is_null ps) (list) (append (if (eq (assoc "passed" (car ps)) t) (list) (assoc "broken_gates" (car ps))) (broken (cdr ps)))))) (define dedup (lambda (xs) (if (is_null xs) (list) (if (member (car xs) (cdr xs)) (dedup (cdr xs)) (cons (car xs) (dedup (cdr xs))))))) (let ((absent (missing names (names-of probes))) (bad (failed probes)) (computed (dedup (append failing_gates (append probe_narrow_300 probe_labels_plus_30pct))))) (list (list "accepted" (and (is_null absent) (is_null bad) (is_null computed) (eq hierarchy_present t))) (list "missing_probes" absent) (list "failing_probes" bad) (list "failing_gates" (dedup (append computed (broken probes)))) (list "hierarchy_present" (eq hierarchy_present t)))))
```

## Regression case

The six gate forms in the Gates section are evaluated on passing and failing cases by the
`scientific_gate_ksa_` tests
(`bash kask/scripts/check-skill-lisp-forms.sh`): Gate B rejects a deferral
with no reason and a `critical` UI finding; Gate FA rejects a revoked function
that is not retired and a finding with no annotation; `KSA-2` never ranks an
`unclear` deletion and its three weight vectors each sum to 1; Gate C rejects
an upstream file and accepts a documented D-seam file; `KSA-1` returns its
derived widths and both probes; `KSA-4` accepts an all-passing probe list and
rejects a missing probe. The seam map is exercised by
`bash kask/scripts/test-seam-map.sh`.

## Constraints

- Hard-stop on any remediation requiring an upstream non-D-seam edit.
- No fabrication: every finding cites `file:line` or is `deferred` with a
  reason; every MCDA score traces to a finding.
- `lisp_eval` tool gates are authoritative between phases; condition branches
  reference real gate result fields, not LLM self-assessment.

## Example invocation

```
task: "Audit the Kask↔Zed seam for security, dead surface, and GPUI gaps."
operator_priority: "security"
prior_rules:
  - { prior: "McpRuntime::invoke OCAP gate", artifact: "McpRuntime::invoke", expected: "live" }
  - { prior: "propagate_taint_for_binding", artifact: "propagate_taint_for_binding", expected: "live" }
```
