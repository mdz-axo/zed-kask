---
name: kali-audit
core: true
visibility: public
description: "Security review skill for hKask. Audits Rust code, Jinja2 templates, YAML manifests, supply chain, MCP surfaces, and LLM I/O boundaries for vulnerabilities. Forward-adaptable: consumes the security regression library at runtime. Runs a declared probe wave of MCP tool calls before the audit step."
---

# Kali Audit

Security review skill for hKask. Audits Rust code, Jinja2 templates, YAML manifests, supply chain, MCP surfaces, and LLM I/O boundaries. Anchored to OWASP LLM Top 10 (2025), MITRE ATLAS v5.1, NIST SSDF SP 800-218A. Forward-adaptable: consumes the security regression library at runtime, runs a declared probe wave of concurrent MCP tool calls (`codegraph_query`, `codegraph_analysis`, `codegraph_stats`) recorded in the grounding ledger before the audit step interprets them, and discovers surfaces dynamically.

## When to Use

- When you need to audit a crate, template directory, MCP server, or the supply chain for security vulnerabilities.
- When you need to verify defense-in-depth layer coverage (4 live layers: capability separation, runtime monitoring, provider-side safety, deception detection). The former input-filtering, output-filtering, IFC, and per-call token-gate layers were de-advertised after their machinery was deleted as inert (RR-0001/0030/0053/0056).
- When you need to propose regression entries for confirmed findings so CI catches re-introductions.
- When you need to consume the existing regression library to avoid re-finding known issues.
- When you need to compute a security coverage metric (defense layers present, CWE classes checked, OWASP risks covered).

## Instructions

### kali-audit/select-surface

1. If `target_surface` is "auto", discover surfaces by scanning the codebase (crates, templates, mcp-servers, deny.toml, scripts).
2. Map the surface to the 8-layer defense-in-depth catalog. Each layer is a parameter — new layers can be added without template changes.
3. Read the regression library (`security/regressions/RR-*.yaml`) to identify already-enforced checks — skip them.
4. Return the selected surface, checks to run, known regressions, and defense layers to verify.

### Probe wave (step 2, execute — no template)

1. Step 2 is an `mcp_batch` execute step (no template_ref): runs three concurrent MCP tool calls — `codegraph_query` (symbol search), `codegraph_analysis` (dead-code), `codegraph_stats` (index stats).
2. Partial failures preserved via `allSettled` — a failed probe is a recorded `ok:false`, not silence.
3. If the entire wave fails, `on_failure: report` halts the cascade: an audit with no recorded tool calls is narration, not evidence.

### kali-audit/audit (step 3)

1. Interpret the recorded probe wave (step 2 results). Findings MUST cite a specific probe result or be marked `deferred` with a reason.
2. Check for evidence-backed patterns: `#![forbid(unsafe_code)]`, `subtle::ConstantTimeEq`, `secrecy::Secret<T>`, `deny_unknown_fields`, path containment, etc.
3. Classify each finding by CWE, OWASP LLM (2025), ATLAS tactic, NIST SSDF practice, severity, confidence, constraint force, and missing defense layer.
4. For each finding with severity >= medium, propose a regression entry with a concrete, testable detection pattern and source citation.
5. Track coverage: defense layers present/missing, CWE classes covered, OWASP risks covered.

### kali-audit/report

1. Synthesize findings into a structured report grouped by severity.
2. Produce a verdict: Pass (no critical/high, >= 6 layers), Conditional (medium or 4-5 layers), or Fail (critical/high or < 4 layers).
3. For each finding, provide a concrete remediation recommendation citing the source.
4. Produce proposed regression entries in YAML format with OWASP 2025 numbering and source citations.
5. Identify defense-layer gaps and top 3 highest-priority fixes.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `select-surface.j2` | KnowAct | Select the security audit target surface (code, template, supply-chain, mcp, config) and map it to the applicable check catalog. Reads the regression library to know what has already been found. |
| `audit.j2` | KnowAct | Run security checks for the selected surface. For code: unsafe blocks, panics, auth bypass, crypto misuse, deserialization, path traversal. For templates: SSTI, sandbox violations, untrusted input rendering. For supply-chain: cargo-deny, cargo-audit, RustSec advisories. For mcp: tool-behavior contracts, indirect_tool_output injection paths. |
| `report.j2` | KnowAct | Synthesize audit findings into a structured report. For each confirmed finding, propose a regression entry (RR-NNNN.yaml) for human review. Classify by CWE, OWASP LLM, severity, and surface. |
| `taxonomy-map.j2` | KnowAct | Map supply-chain audit findings to the OSC&R attack taxonomy (Open Software Supply Chain Attack Reference). Folded from the standalone attack-taxonomy-mapper skill. Only runs for surface == 'supply-chain'. Emits reg.taxonomy.map spans. |

## Defense-in-Depth Layer Catalog

| Layer | Name | Source |
|-------|------|--------|
| 1 | Capability separation (allowlists) | OWASP LLM06:2025, OCAP literature |
| 2 | Runtime monitoring (Regulation, action distribution) | AgentGuard arXiv:2509.23864, NIST AI RMF |
| 3 | Provider-side safety (model refusal, safety training) | OWASP LLM01:2025 |
| 4 | Deception detection (decoy tools) | MITRE Engage, Cobalt Honey-AI |

**De-advertised layers** (do NOT report as present or missing — the machinery
was deleted as inert):

- *Information flow control (FIDES taint labels)* — deleted 2026-08-12 (RR-0053); both gate inputs were constants.
- *Per-call capability gating via `DelegationToken`* — deleted 2026-08-12 (RR-0056); compared a caller-supplied value against itself.
- *Input/output content scanning, spotlighting, canary tokens* — `hkask-guard` deleted 2026-08-10 (RR-0001/RR-0030).

New layers can be added as research advances — the skill structure does not change.

## Relationship to the Regression Library

The `security/regressions/` directory is the **deep artifact** — it compounds value over time. The skill consumes it as input (to avoid re-finding known issues) and proposes new entries as output (for human review). The "evolving" property comes from the library growing, not from the skill mutating its own prompts.

**Honest framing:** this is a human-curated ratcheted checklist with CI enforcement, not autonomous learning. The skill proposes entries; humans curate them; CI enforces them.

## Relationship to adversarial-red-team

`kali-audit` covers **code and infrastructure** security (Rust, templates, manifests, supply chain, MCP, LLM I/O). `adversarial-red-team` covers **LLM I/O robustness** (prompt injection, exfiltration). They are complementary — `kali-audit` checks the static surface and defense-layer presence; `adversarial-red-team` probes the dynamic LLM boundary.

## Constraints

- rJoule cap: 2 per invocation. Maximum 10 iterations.
- `select-surface.j2`: Public.
- `audit.j2`: Public.
- `report.j2`: Public.
- `taxonomy-map.j2`: Public. Only runs for surface == 'supply-chain'. Every mapping requires concrete evidence: finding reference, CWE category, OSC&R tactic + technique.
- Do NOT fabricate findings — only report what was actually discovered through tool usage.
- Every finding must include concrete evidence (file path, line number, code snippet) and a source citation.
- Every proposed regression must use OWASP LLM 2025 numbering (not 2023).
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
