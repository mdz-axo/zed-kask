---
name: kali-audit
visibility: public
description: "Security review skill for hKask. Audits Rust code, Jinja2 templates, YAML manifests, supply chain, MCP surfaces, and LLM I/O boundaries for security vulnerabilities. Anchored to OWASP LLM Top 10 (2025), MITRE ATLAS v5.1, NIST SSDF SP 800-218A. Forward-adaptable: consumes the security regression library at runtime, checks defense-layer coverage (8 layers), and discovers surfaces dynamically. Single skill with a surface parameter — not a bundle.
"
---

# Kali Audit

Security review skill for hKask. Audits Rust code, Jinja2 templates, YAML manifests, supply chain, MCP surfaces, and LLM I/O boundaries. Anchored to OWASP LLM Top 10 (2025), MITRE ATLAS v5.1, NIST SSDF SP 800-218A. Forward-adaptable: consumes the security regression library at runtime, checks defense-layer coverage (8 layers), and discovers surfaces dynamically.

## When to Use

- When you need to audit a crate, template directory, MCP server, or the supply chain for security vulnerabilities.
- When you need to verify defense-in-depth layer coverage (8 layers: input filtering, data/instruction separation, instruction hierarchy, capability gating, IFC, runtime monitoring, output filtering, deception detection).
- When you need to propose regression entries for confirmed findings so CI catches re-introductions.
- When you need to consume the existing regression library to avoid re-finding known issues.
- When you need to compute a security coverage metric (defense layers present, CWE classes checked, OWASP risks covered).

## Instructions

### kali-audit/select-surface

1. If `target_surface` is "auto", discover surfaces by scanning the codebase (crates, templates, mcp-servers, deny.toml, scripts).
2. Map the surface to the 8-layer defense-in-depth catalog. Each layer is a parameter — new layers can be added without template changes.
3. Read the regression library (`security/regressions/RR-*.yaml`) to identify already-enforced checks — skip them.
4. Return the selected surface, checks to run, known regressions, and defense layers to verify.

### kali-audit/audit

1. For each check and defense layer, use available MCP tools (`file:read`, `code:search`, `terminal`) to probe the target.
2. Check for evidence-backed patterns: `#![forbid(unsafe_code)]`, `subtle::ConstantTimeEq`, `secrecy::Secret<T>`, `deny_unknown_fields`, path containment, spotlighting, canary tokens, etc.
3. Classify each finding by CWE, OWASP LLM (2025), ATLAS tactic, NIST SSDF practice, severity, confidence, constraint force, and missing defense layer.
4. For each finding with severity >= medium, propose a regression entry with a concrete, testable detection pattern and source citation.
5. Track coverage: defense layers present/missing, CWE classes covered, OWASP risks covered.

### kali-audit/report

1. Synthesize findings into a structured report grouped by severity.
2. Produce a verdict: Pass (no critical/high, >= 6 layers), Conditional (medium or 4-5 layers), or Fail (critical/high or < 4 layers).
3. For each finding, provide a concrete remediation recommendation citing the source.
4. Produce proposed regression entries in YAML format with OWASP 2025 numbering and source citations.
5. Identify defense-layer gaps and top 3 highest-priority fixes.

### kali-audit/convergence-check

1. Compute the convergence metric on [0, 1] where 0 = converged.
2. Score five dimensions: critical/high findings (0.40), medium findings (0.15), defense-layer coverage (0.25), CWE coverage (0.10), regression library growth (0.10).
3. Converged when metric ≤ 0.10 with minimum 5% relative improvement from previous cycle.
4. Identify specific blockers (missing defense layers, unfixed findings).

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `select-surface.j2` | KnowAct | Select target surface, discover defense layers, read regression library. Forward-adaptable: surfaces discovered dynamically, layers are parameters. |
| `audit.j2` | KnowAct | Run security checks consuming the regression library at runtime. Checks for evidence-backed patterns (OWASP, ANSSI, RustSec, Microsoft Research). |
| `report.j2` | KnowAct | Synthesize findings with OWASP 2025 numbering, ATLAS tactics, NIST SSDF practices, and source citations. Proposes regression entries. |
| `convergence-check.j2` | KnowAct | Convergence metric including defense-layer coverage (8 layers), CWE coverage, and regression library growth. |

## Defense-in-Depth Layer Catalog

| Layer | Name | Source |
|-------|------|--------|
| 1 | Input filtering | OWASP LLM01:2025 |
| 2 | Data/instruction separation (spotlighting) | Microsoft Research arXiv:2403.14720 |
| 3 | Instruction hierarchy | OpenAI arXiv:2404.13208 |
| 4 | Capability gating (OCAP) | OWASP LLM06:2025, OCAP literature |
| 5 | Information flow control (taint labels) | Microsoft Research arXiv:2505.23643 (FIDES) |
| 6 | Runtime monitoring (Regulation, action distribution) | AgentGuard arXiv:2509.23864, NIST AI RMF |
| 7 | Output filtering (secrets, canaries) | OWASP LLM02:2025, Thinkst canarytokens |
| 8 | Deception detection (decoy tools, canary tokens) | MITRE Engage, Cobalt Honey-AI |

New layers can be added as research advances — the skill structure does not change.

## Relationship to the Regression Library

The `security/regressions/` directory is the **deep artifact** — it compounds value over time. The skill consumes it as input (to avoid re-finding known issues) and proposes new entries as output (for human review). The "evolving" property comes from the library growing, not from the skill mutating its own prompts.

**Honest framing:** this is a human-curated ratcheted checklist with CI enforcement, not autonomous learning. The skill proposes entries; humans curate them; CI enforces them.

## Relationship to adversarial-red-team

`kali-audit` covers **code and infrastructure** security (Rust, templates, manifests, supply chain, MCP, LLM I/O). `adversarial-red-team` covers **LLM I/O robustness** (prompt injection, exfiltration). They are complementary — `kali-audit` checks the static surface and defense-layer presence; `adversarial-red-team` probes the dynamic LLM boundary.

## Constraints

- `select-surface.j2`: Public.
- `audit.j2`: Public.
- `report.j2`: Public.
- `convergence-check.j2`: Public.
- Do NOT fabricate findings — only report what was actually discovered through tool usage.
- Every finding must include concrete evidence (file path, line number, code snippet) and a source citation.
- Every proposed regression must use OWASP LLM 2025 numbering (not 2023).
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
