---
name: kask-seam-audit
description: >-
  Convergent multi-skill audit of the zed-kask Kask↔Zed seam (DIVERGENCE.md
  D1-D24). Three tracks: kali-audit (security: OWASP LLM Top-10, MITRE ATLAS,
  NIST SSDF, 8-layer defense coverage), refactor-architecture (dead-surface
  removal + deepening + strangler-fig), ui-layout-discipline (GPUI measured
  layout + Zed interaction-language gaps). Findings adjudicated with
  pragmatic-semantics + pragmatic-cybernetics + essentialist; remediations
  ranked by mcda with +-20% sensitivity analysis and applied only if
  essentialist-surviving and seam-scoped. Stateless lisp.eval gates verify
  structural invariants (prior-verdict exclusivity, finding citation +
  severity exclusivity, remediation test-pinning + divergence-surface
  membership) between phases, not LLM self-assessment. Hard-stops on any
  upstream non-D-seam touch. Every finding cites file:line; missing evidence
  is deferred with a reason, never fabricated. Any userpod may invoke it.
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

## Constraints

- Hard-stop on any remediation requiring an upstream non-D-seam edit.
- No fabrication: every finding cites `file:line` or is `deferred` with a
  reason; every MCDA score traces to a finding.
- `lisp.eval` gates are authoritative between phases; `condition:` branches
  reference real `step_N_result` keys, not LLM self-assessment.
- `ledger.span_namespace` is `reg.skill.kask-seam-audit` (CI-enforced).

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