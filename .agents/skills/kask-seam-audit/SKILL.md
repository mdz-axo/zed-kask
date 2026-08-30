---
name: kask-seam-audit
description: "Convergent multi-skill audit of the zed-kask Kask-Zed seam (DIVERGENCE.md D1–D33, D17/D19 retired). Three tracks: security (self-contained), refactor-architecture (dead-surface removal), ui-layout-discipline (GPUI layout). Every finding cites file:line."
---

# Kask Seam Audit

Convergent multi-skill audit-and-refactor engagement over the zed-kask
Kask↔Zed seam. The implementation lives in the registry:

- **Templates**: `kask/registry/templates/kask-seam-audit/*.j2`

This skill has no manifest; it runs purely from this SKILL.md body + the
templates. Invoke the skill via the `skill` tool; the agent reads the SKILL.md
and calls `lisp_eval`, `render_template`, and MCP tools directly as the
methodology instructs.

## When to use

- Reproducible security + architecture + UI audit of the Kask↔Zed seam
  (D1–D33 in `DIVERGENCE.md`; D17 and D19 are retired).
- Dead-surface removal + deepening candidates with grep-verified caller
  counts and the essentialist deletion test.
- GPUI measured-layout + Zed interaction-language gaps (Button/IconButton vs
  raw div, PopoverMenu, Tooltip, Toggle vs ToggleFocus, deploy-and-focus).
- Remediations ranked by MCDA (+-20% sensitivity) and applied only if
  essentialist-surviving and seam-scoped.

## Ontological anchors

- **PKO** — the engagement is a Procedure (spec/execution split); Steps map
  to `pko:Step` with `pko:StepVerification` (the `lisp_eval` tool gates).
- **OWASP LLM Top-10 (2025) / MITRE ATLAS v5.1 / NIST SSDF SP 800-218A** —
  the security-track ontology.
- **de la Torre (2025, arXiv:2506.10021)** — symbolic-neural scaffolding via
  stateless `lisp_eval` tool gates (count / completeness / exclusivity invariants).
- **Ousterhout** — the deep-module deletion test (essentialist G1/G2/G3).

## PDCA shape

```
Plan:  seam-map + prior verification  ->  Gate A (lisp: prior exclusivity)
Do:    audit-security -> audit-architecture -> audit-ui -> Gate B (lisp: citation + severity)
Check: adjudicate (semantics + cybernetics + essentialist) -> mcda (+ sensitivity)
Act:   remediate -> Gate C (lisp: test-pinning + divergence membership, hard-stop)
Converge: lisp score (uncited/unadjudicated -> 0) -> loop to Do
Final: report
```

## Composed skills

| Skill | Role | When |
|-------|------|------|
| `refactor-architecture` | executor | architecture track (Do) |
| `ui-layout-discipline` | executor | UI track (Do) |
| `pragmatic-semantics` | lens | adjudicate (Check) |
| `pragmatic-cybernetics` | lens | adjudicate (Check) |
| `essentialist` | lens | adjudicate (Check) + remediation gate (Act) |
| `mcda` | decision | ranking + sensitivity (Check) |

The security track is self-contained in `audit-security.j2` (10 priority
surfaces, OWASP LLM Top-10 / MITRE ATLAS / NIST SSDF framing); it does not
delegate to a separate skill.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `seam-map.j2` | PLAN — read DIVERGENCE.md D1–D33 (D17, D19 retired), grep `crates/` for each convention prior's artifact (live vs phantom), and derive the audit slices. Read-only. |
| `audit-security.j2` | DO — self-contained security review of the 10 priority surfaces (OWASP LLM Top-10, MITRE ATLAS, NIST SSDF, defense-layer coverage). Every finding cites file:line. |
| `audit-architecture.j2` | DO — find dead surface (trait-with-one-impl, helper-test-only, folded re-exports) and deepening candidates; apply the essentialist deletion test with grep-verified caller counts. |
| `audit-ui.j2` | DO — measured-layout discipline + Zed interaction-language gaps across kask-owned GPUI widgets; Toggle-vs-ToggleFocus and deploy-and-focus traps. |
| `adjudicate.j2` | CHECK — classify each finding by constraint force, run the deletion test, and check the feedback loop. Produces annotated_findings. |
| `mcda.j2` | CHECK — rank remediation candidates against four weighted criteria and run a ±20% sensitivity analysis. Each score traces to a finding. |
| `remediate.j2` | ACT — apply only mcda top-ranked remediations surviving essentialist; pin each with a test; declare within_kask per touched file; set hard_stop if any touch requires an upstream non-D-seam edit. |
| `final-report.j2` | Consolidate the three tracks, defense-layer coverage, MCDA, applied remediations, hard-stop decision, and convergence score. Cite file:line. |

Gate-defect repair is handled inline: if Gate A/B/C finds defects, re-run the
parent template (`seam-map.j2`, the audit template, or `remediate.j2`) with
`gate_defects` in context; there are no separate repair templates.

To render a template, call the `render_template` tool with the template ref (e.g., `essentialist/essentialist-flow`) and a context object with the required variables.

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

This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.