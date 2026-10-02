---
name: kask-seam-audit
description: "Convergent multi-skill audit of the zed-kask Kask-Zed seam (every live D-seam in DIVERGENCE.md). Three tracks: security (self-contained), refactor-architecture (dead-surface removal), measured GPUI layout (its own layout loop, which also runs standalone before adding card or panel elements). Every finding cites file:line."
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
gates are D: Gate A/B/C, the Convergence count,
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

The Gate A, B, C and Converge forms live in
`kask/registry/templates/kask-seam-audit/gates.md` (read it with `read_file`);
the measured-layout gate form lives in this SKILL.md (the layout loop,
step 3). Run them with `lisp_eval` over the template outputs. The Converge count is
passed to `final-report` as `convergence_score`; the model never supplies it.

- **Initial condition:** the live and retired seams in `DIVERGENCE.md`, the operator's `prior_rules`, the audited commit, and `kask/docs/research/d-seam-audit.md` (if present).
- **Target condition per batch:** every selected behavior has an evidence-backed advisory verdict and a pin status, the ledger is committed, and unresolved decisions have an owner. Pending rows are not audited rows. Gate C applies to remediations, not record-only updates.

| Phase | Type | Oracle / critique |
|-------|------|-------------------|
| Plan, Do (seam-map, audits) | P | Gates A and B |
| Check (adjudicate, mcda) | P | Converge count; the operator on ranking |
| Act (remediate) | P | Gate C and the tests it pins |
| Gates, Converge | D | `lisp_eval` forms in `gates.md` |

## Resumable behavior-level pass

The ledger at `kask/docs/research/d-seam-audit.md` is the worklist, not a second authority for what is live. At each session start, extract rows from the `## The divergence surface` table of `DIVERGENCE.md` (never infer a range or reuse the retired paragraph), and run `bash kask/scripts/check-d-seam-audit-ledger.sh`. Add new rows as `pending`, remove retired rows only after verifying no residue, and rerun the check. Rank pending rows by upstreamability first, then files × changed lines, pin health, live-fire evidence and operator pain; write the next small batch and the ranking rationale to the ledger. Stop when that batch is complete or the context budget requires it; three no-new-state iterations per seam stop and escalate.

For **each behavior**, not merely each D-number:

1. Recover the purpose from the row, `.rules`, `git log --oneline -- <files>` and `git log -S '<identifier>'`; recall `seam:DXX` before updating memory. The row claims a contract; it is not proof. Preserve any uncommitted edits by other streams.
2. Enumerate **every current pin named for this behavior**, locate its test target, then run each with `bash kask/scripts/cargo-test-nonzero.sh -p <crate> <filter>` (or the named non-Cargo script). Record missing/failed/unrun individually. Do not silently count a similarly named test or a zero-test run.
3. Look for its warn/error signature in available editor logs. Compare the specific behavior with local `upstream/main`: `git remote -v`, `git log -S '<identifier>' upstream/main`, and the upstream implementation. Record the ref and freshness; no ref or network means `unverified`, not absent. No log signature or no logging means `not observable`, not disproven.
4. Run essentialist in **advisory** mode, with coding-guidelines and refactor-architecture: G1 deletion test (Evidence vs Hypothesis), G2 public surface (7-function rule), G3 pass-through contracts. Apply the existing security/architecture/UI tracks at the behavior's actual reach; UI layout checks are not applicable to a non-UI seam, and a security track outside the batch is not a claim of full-fleet coverage. Label each finding Prohibition / Guardrail / Guideline / Evidence / Hypothesis. Gate B checks citations, not correctness; record oracle strength separately.
5. Map to upstream-rebase vocabulary: `essential-keep` = retain, `upstreaming-candidate` = retain + issue draft, `demoted-defensive-compatibility` = simplify, `delete` = retire, `already-retired-verify-no-residue`. Update the row, pins and `seam:DXX` memory for keeps/demotions once verified. For deletions, present a red-first removal plan and wait for operator ratification; for upstreaming, draft summary, evidence, discriminator, fix and PR offer, and wait for the operator to send it. Never mark a proposal adopted upstream.
6. Record evidence links, pin and upstream status, date and **actual** pathspec-limited commit in the ledger; verify `git diff --cached --stat` before commit. A shared `DIVERGENCE.md` edit blocks row alignment, not the independent ledger commit: record its owner and leave the seam `partial` until aligned. Rerun the ledger check and Gate B/Converge for batch findings. Apply Gate C only to code remediations (and run red/green pin + residue sweep). Commit status stays `uncommitted` until the hash exists.

The row-by-row ledger is deliberately compact; detailed evidence and upstream issue drafts sit below the table under the same ID. A prior session's `asserted` result never upgrades itself to `oracle-verified`; re-run pins after a changed test or behavior. Close with verdicts, hashes, validation tiers, code red/green (or none), net lines, residue sweep and every open item's closure state and owner. Do not score a goal until the operator confirms the outcome.

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

1. **Measure** (`kask-seam-audit/layout-sense`) — container width (dock ~300–400px, center ~600px+), each child's minimum width, the text column's residual width.
2. **Count** (`kask-seam-audit/layout-orient`) — interactive elements against the ≤5 primary budget (Hick's Law); sibling card conventions. The Rosenholtz congestion score is a model estimate reported for context; no gate reads it.
3. **Gate** (`kask-seam-audit/layout-decide`) — five yes/no gates: no overflow, primary action visible, text column ≥ min width, on-grid spacing, action count ≤ budget. Call `lisp_eval` with form `(and (= (length failed_gates) 0) (= (length probe_failures) 0))`, env `{ "failed_gates": <layout-decide's `failing_gates`>, "probe_failures": <layout-review's `failing_gates` — the gates broken under probes> }` (the templates output `failing_gates`; the form env binds `failed_gates`/`probe_failures` — pass the mapped names).
4. **Remedy** (`kask-seam-audit/layout-act`) — for each failing gate, the canonical GPUI remedy: secondary actions behind `PopoverMenu` with an `IconName::Ellipsis` trigger, `.truncate()` on labels, `flex_shrink_0()` on fixed elements, `min_w_0()` on flexible text columns, or hide-secondary.
5. **Probe** (`kask-seam-audit/layout-review`) — a 40-character button label, a German string (~30% longer), a 300px container (the agent panel's minimum dock width, `MIN_PANEL_WIDTH` at `crates/agent_ui/src/agent_panel.rs:108`), 7 actions, and an empty description. Any broken probe rejects the layout and re-enters step 4. Bound: max 2 remedy rounds; a third failing gate set rejects the change — hide the secondary actions or defer, and say so.

Patterns checked: `min_w_0()` on flexible text columns; `flex_shrink_0()` on fixed-width elements; `.truncate()` on labels; `PopoverMenu::new(id).trigger_with_tooltip(IconButton::new(id, IconName::Ellipsis), Tooltip::text(...))` for overflow; `ContextMenu::build(...)` for menu items; `gap_1()`/`gap_2()` on the 4px/8px grid.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `seam-map.j2` | PLAN — read the live seams and the retired list from DIVERGENCE.md, grep `crates/` for each convention prior's artifact (live vs phantom), and derive the audit slices. Read-only. |
| `audit-security.j2` | DO — self-contained security review of the 10 priority surfaces (OWASP LLM Top-10, MITRE ATLAS, NIST SSDF, defense-layer coverage). Every finding cites file:line. |
| `audit-architecture.j2` | DO — find dead surface (trait-with-one-impl, helper-test-only, folded re-exports) and deepening candidates; apply the essentialist deletion test with grep-verified caller counts. |
| `audit-ui.j2` | DO — measured-layout discipline + Zed interaction-language gaps across kask-owned GPUI widgets; Toggle-vs-ToggleFocus and deploy-and-focus traps. |
| `layout-sense.j2` | Layout loop step 1: measure container and child minimum widths; flag overflow and the text column's residual width (Fitts's Law, flexbox overflow). |
| `layout-orient.j2` | Layout loop step 2: count interactive elements against the ≤5 budget and compare sibling conventions (Hick's Law, Nielsen consistency). |
| `layout-decide.j2` | Layout loop step 3: the five yes/no layout gates and a pass/fail decision. |
| `layout-act.j2` | Layout loop step 4: the canonical GPUI remedy per failing gate. |
| `layout-review.j2` | Layout loop step 5: adversarial probes that reject a layout that breaks a gate. |
| `adjudicate.j2` | CHECK — classify each finding by constraint force, run the deletion test, and check the feedback loop. Produces annotated_findings. |
| `mcda.j2` | CHECK — rank remediation candidates against four weighted criteria and run a ±20% sensitivity analysis. Each score traces to a finding. |
| `remediate.j2` | ACT — apply only mcda top-ranked remediations surviving essentialist; pin each with a test; declare within_kask per touched file; set hard_stop if any touch requires an upstream non-D-seam edit. |
| `final-report.j2` | Consolidate the three tracks, defense-layer coverage, MCDA, applied remediations, hard-stop decision, and convergence score. Cite file:line. |

Gate-defect repair is handled inline: if Gate A/B/C finds defects, re-run the
parent template (`seam-map.j2`, the audit template, or `remediate.j2`) with
`gate_defects` in context; there are no separate repair templates.

To render a template, call the `render_template` tool with the template ref (e.g., `kask-seam-audit/seam-map`) and a context object with the required variables.

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

