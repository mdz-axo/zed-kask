---
name: kask-seam-audit
description: "Convergent multi-skill audit of the zed-kask Kask-Zed seam (DIVERGENCE.md D1-D24). Three tracks: kali-audit (security), refactor-architecture (dead-surface removal), ui-layout-discipline (GPUI layout). Every finding cites file:line."
---

# Kask Seam Audit

Convergent multi-skill audit-and-refactor engagement over the zed-kask
Kask↔Zed seam. The implementation lives in the registry:

- **Process manifest**: `kask/registry/manifests/kask-seam-audit.yaml`
- **Templates**: `kask/registry/templates/kask-seam-audit/*.j2`

This SKILL.md is a discovery-only catalog entry. Invoke the skill via the
`skill` tool; the ManifestExecutor runs the registry cascade.

## When to use

- Reproducible security + architecture + UI audit of the Kask↔Zed seam
  (D1-D24 in `DIVERGENCE.md`).
- Dead-surface removal + deepening candidates with grep-verified caller
  counts and the essentialist deletion test.
- GPUI measured-layout + Zed interaction-language gaps (Button/IconButton vs
  raw div, PopoverMenu, Tooltip, Toggle vs ToggleFocus, deploy-and-focus).
- Remediations ranked by MCDA (+-20% sensitivity) and applied only if
  essentialist-surviving and seam-scoped.

## Ontological anchors

- **PKO** — the engagement is a Procedure (spec/execution split); Steps map
  to `pko:Step` with `pko:StepVerification` (the lisp.eval gates).
- **ESO** — each finding is an Event with pre/post situations; the
  divergence-surface membership check is the Situation boundary.
- **OWASP LLM Top-10 (2025) / MITRE ATLAS v5.1 / NIST SSDF SP 800-218A** —
  the security-track ontology.
- **de la Torre (2025, arXiv:2506.10021)** — symbolic-neural scaffolding via
  stateless `lisp.eval` gates (count / completeness / exclusivity invariants).
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
| `kali-audit` | executor | security track (step 4) |
| `refactor-architecture` | executor | architecture track (step 5) |
| `ui-layout-discipline` | executor | UI track (step 6) |
| `pragmatic-semantics` | lens | adjudicate (step 9) |
| `pragmatic-cybernetics` | lens | adjudicate (step 9) |
| `essentialist` | lens | adjudicate + remediation gate (steps 9, 11) |
| `mcda` | decision | ranking + sensitivity (step 10) |

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `seam-map.j2` | KnowAct | PLAN — read DIVERGENCE.md D1–D24, grep `crates/` for each convention prior's artifact (live vs phantom), and derive the audit slices. Read-only. |
| `refine-seam-map.j2` | KnowAct | Repair Gate A structural defects (phantom prior, invalid verdict, no slices). Only runs when Gate A found defects. |
| `audit-security.j2` | KnowAct | DO — security review of the 8 priority surfaces (OWASP LLM Top-10, MITRE ATLAS, NIST SSDF, 8-layer defense coverage). Every finding cites file:line. |
| `audit-architecture.j2` | KnowAct | DO — find dead surface (trait-with-one-impl, helper-test-only, folded re-exports) and deepening candidates; apply the essentialist deletion test with grep-verified caller counts. |
| `audit-ui.j2` | KnowAct | DO — measured-layout discipline + Zed interaction-language gaps across kask-owned GPUI widgets; Toggle-vs-ToggleFocus and deploy-and-focus traps. |
| `refine-findings.j2` | KnowAct | Re-cite or re-classify findings Gate B flagged (missing file_line, invalid severity, empty track). Never fabricates evidence; marks deferred with a reason. |
| `adjudicate.j2` | KnowAct | CHECK — classify each finding by constraint force, run the deletion test, and check the feedback loop. Produces annotated_findings. |
| `mcda.j2` | KnowAct | CHECK — rank remediation candidates against four weighted criteria and run a ±20% sensitivity analysis. Each score traces to a finding. |
| `remediate.j2` | KnowAct | ACT — apply only mcda top-ranked remediations surviving essentialist; pin each with a test; declare within_kask per touched file; set hard_stop if any touch requires an upstream non-D-seam edit. |
| `remediation-repair.j2` | KnowAct | Repair Gate C defects (missing test, out-of-seam touch) or emit the hard-stop with the offending file and a proposed D-seam entry. |
| `final-report.j2` | KnowAct | Consolidate the three tracks, defense-layer coverage, MCDA, applied remediations, hard-stop decision, and convergence score. Cite file:line. |

## Constraints

- Hard-stop on any remediation requiring an upstream non-D-seam edit.
- No fabrication: every finding cites `file:line` or is `deferred` with a
  reason; every MCDA score traces to a finding.
- `lisp.eval` gates are authoritative between phases; `condition:` branches
  reference real `step_N_result` keys, not LLM self-assessment.
- `ledger.span_namespace` is `reg.skill.kask-seam-audit` (CI-enforced).
- rJoule cap: 3 per invocation. Maximum 10 iterations.

## Example invocation

```
task: "Audit the Kask↔Zed seam for security, dead surface, and GPUI gaps."
operator_priority: "security"
prior_rules:
  - { prior: "McpRuntime::invoke OCAP gate", artifact: "McpRuntime::invoke", expected: "live" }
  - { prior: "propagate_taint_for_binding", artifact: "propagate_taint_for_binding", expected: "live" }
```

Registry is authoritative — when this SKILL.md disagrees with the registry
templates, the registry wins.