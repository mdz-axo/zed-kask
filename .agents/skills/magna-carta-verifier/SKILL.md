---
name: magna-carta-verifier
visibility: public
description: "Verifies that hKask's four Magna Carta principles (User Sovereignty, Affirmative Consent, Generative Space, Clear Boundaries) are correctly implemented and enforced. Uses YAML manifests to declare assertions per principle and Jinja2 templates to render verification procedures, reports, and test cases.
"
---

# Magna Carta Verifier

Verifies that hKask's four Magna Carta principles (User Sovereignty, Affirmative Consent, Generative Space, Clear Boundaries) are correctly implemented and enforced. Uses YAML manifests to declare assertions per principle and Jinja2 templates to render verification procedures, reports, and test cases.


## When to Use

- When you need to generate step-by-step verification procedures for Magna Carta principle assertions, including methods, targets, and pass/fail criteria.
- When you need to synthesize verification results into a structured compliance report with per-principle status, findings, gap identification, and an overall compliance score.
- When you need to render compilable Rust test case skeletons from Magna Carta assertion definitions targeting specific crates, modules, and methods.
- When you need to compute a normalized convergence metric for Magna Carta verification PDCA cycles to determine if compliance analysis is stable.

## Instructions

### mc-verify-procedure

1. Identify the method from the assertion's `method` field.
2. Map to targets — the crate, module, and methods listed in the assertion's `targets`.
3. Define pass criteria — what constitutes a successful verification (gate exists, access denied, resource categorized correctly, prohibited construct absent).
4. Define fail criteria — what constitutes a verification failure (gate missing, access granted, wrong categorization, prohibited construct found).
5. Specify execution — exact CLI command, MCP tool call, or code path to exercise.

### mc-verify-report

1. Score each assertion based on its `status` (Pass = 1.0, Warning = 0.5, Fail = 0.0).
2. Calculate the overall compliance score as the mean of all assertion scores.
3. Include a header with principle name, version, timestamp, and overall status.
4. Provide a one-line summary verdict (Compliant / Partially Compliant / Non-Compliant) and overall score.
5. List per-assertion findings in a table with assertion ID, name, method, status, and finding summary.
6. Identify gaps including untested or unimplemented assertions with recommended remediation.
7. Escalate any `fail` results that require Curator or human attention.

### mc-verify-testcase

1. Name the function `magna_carta_<principle>_<assertion_id>`.
2. Add a doc comment `// Verify: MC-<PRINCIPLE>-<ASSERTION_ID> — <claim summary>`.
3. Map the method to test logic (structural_audit, behavioral_probe, resource_verification, absence_check).
4. Render a compilable test body with `todo!()` where actual assertion logic needs implementation, including a comment describing what the test must verify.

### mc-convergence-check

1. Measure convergence on [0,1] where 0 means compliance analysis is stable and no critical principle blockers remain unresolved.
2. Score how much work remains based on procedure, report, and testcase results.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `mc-verify-procedure.j2` | KnowAct | Render a step-by-step verification procedure for each assertion in a Magna Carta principle manifest. For each assertion, produce the method (structural_audit, behavioral_probe, resource_verification, absence_check), targets, and expected pass/fail criteria.  |
| `mc-verify-report.j2` | KnowAct | Synthesize verification results into a structured report with per-principle status, per-assertion findings (pass/fail/warning), gap identification, and an overall compliance score.  |
| `mc-verify-testcase.j2` | KnowAct | Render Rust test case code blocks from assertion definitions. Each assertion produces a compilable test skeleton targeting the declared crate, module, and method with the appropriate verification method.  |
| `mc-convergence-check.j2` | KnowAct | Compute normalized convergence metric for Magna Carta verification PDCA cycles.  |

## Constraints

- `mc-verify-procedure.j2`: Public.
- `mc-verify-report.j2`: Public.
- `mc-verify-testcase.j2`: Public.
- `mc-convergence-check.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
