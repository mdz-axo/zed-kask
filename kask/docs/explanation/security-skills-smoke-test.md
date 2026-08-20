---
title: "Security Skills — Execution Smoke Test Procedure"
audience: [operators, developers, security-engineers]
last_updated: 2026-08-20
version: "0.37.0"
status: "Active"
domain: "Security"
mds_categories: [domain, trust, lifecycle, curation]
---

# Security Skills — Execution Smoke Test Procedure

This document describes how to manually smoke-test the security skills
against test fixtures. These tests require a running zed-kask editor
session with hKask compiled in-process and LLM access — they are NOT CI
gates.

## Prerequisites

1. zed-kask built and running (`cargo build --release` then launch the
   editor). hKask is compiled in-process; there is no `kask init` step.
2. LLM inference configured through zed-kask's `CredentialsProvider`
   (D9) — inference requests route to the configured provider. (The former
   guard layer, D4, was removed 2026-08-10; provider-side safety and refusal
   fallbacks remain.)
3. The Regulation layer runs in-process (`hkask-regulation`); the
   former `kask mcp start regulation` standalone CLI has been removed.
   Regulation spans are emitted and queried in-process.
4. Working directory: zed-kask project root (the hKask workspace under
   `kask/`).[^owasp-llm][^mitre-atlas]

## Test Fixtures

The fixtures below follow an exploratory security-testing approach: each skill
is exercised against a real target with expected output contracts that the
operator validates manually.[^nist-ssdf][^bach-bolton]

### Fixture 1: Supply Chain Audit (supply-chain-sentinel)

**Setup:** The zed-kask project itself is the test fixture — it has
`Cargo.toml`, `Cargo.lock`, and `deny.toml`.

**Procedure:**

Invoke the `supply-chain-sentinel` skill from the zed-kask agent panel
(native agent, D2). The skill executes
in-process through upstream-Zed body injection (D1): `SkillTool::run` reads
the `SKILL.md` body from disk and injects it via `render_skill_envelope`;
there is no `kask skill
run` CLI. Supply the surface and target path as context:

```
skill: supply-chain-sentinel
manifest_path: <path-to-Cargo.toml-or-workspace-root>
```

**Expected output:**

- `select-surface` step (ordinal 1): discovers `Cargo.toml`, `Cargo.lock`, `deny.toml`
- `probe` step (ordinal 2): reads manifest entries, checks version pinning, registry trust
- `report` step (ordinal 3): proposes `surface: supply-chain` regression entries (if any findings)
- `loop` step (ordinal 4): re-enters the audit cycle if convergence is not met; the `convergence_signal:` binding (finding count from `step_3_result`) feeds the `ConvergenceTracker`'s Cauchy check

**Validation:**

1. The skill produces JSON output (not an error)
2. `manifest_paths` includes `Cargo.toml` and `Cargo.lock`
3. `defense_layers_present` includes at least `dependency_pinning` and `sbom_presence`
4. `reg.skill.supply-chain-sentinel` spans are emitted (query via the in-process `reg_query_spans` tool exposed through the agent panel)
5. No synthetic findings — every finding references a real `Cargo.toml` line

### Fixture 2: Runtime Posture Monitor (runtime-posture-monitor)

**Setup:** Requires a running zed-kask session with Regulation telemetry.

**Procedure:**

1. Generate some Regulation telemetry by running any agent task that
   emits spans (e.g., invoke a skill or run an agent panel session —
   these emit `hkask.*` performative spans in-process).

2. Invoke the `runtime-posture-monitor` skill from the agent panel:

```
skill: runtime-posture-monitor
telemetry_stream: <optional-preloaded-spans>
workspace_context: <optional-workspace-path>
```

**Expected output:**

- `select-signal` step (ordinal 1): discovers `hkask.*` and `reg.*` span sources
- `classify-threat` step (ordinal 2): classifies observed signals (may find zero threats if baseline is clean)
- `emit-regulation` step (ordinal 3): proposes `surface: runtime` regression entries (if any threats)
- `convergence-check` step (ordinal 4, `compute` action): computes the Cauchy convergence metric

**Validation:**

1. The skill produces JSON output (not an error)
2. `signal_sources` includes at least one `reg.*` or `hkask.*` target
3. `reg.skill.runtime-posture-monitor` spans are emitted (query via the in-process `reg_query_spans` tool)
4. No synthetic signals — every finding references a real span target + timestamp

### Fixture 3: Kali Audit (kali-audit)

**Setup:** The zed-kask project itself is the test fixture.

**Procedure:**

Invoke the `kali-audit` skill from the agent panel:

```
skill: kali-audit
target_surface: code
target_path: <crate-or-workspace-path>
```

**Expected output:**

- `select-surface` step (ordinal 1): discovers Rust source files, maps defense-layer coverage
- `audit` step (ordinal 2): agent-coordinated MCP tool execution — checks for unsafe blocks, panics, auth bypass, crypto misuse
- `report` step (ordinal 3): synthesizes findings into a structured report with verdict (Pass/Conditional/Fail)
- `taxonomy-map` step (ordinal 4, conditional — only when `target_surface == 'supply-chain'`): maps supply-chain findings to OSC&R tactic + technique (folded from the former `attack-taxonomy-mapper` skill)
- `convergence-check` step (ordinal 5, `compute` action): computes the Cauchy convergence metric
- `loop` step (ordinal 6): re-enters the audit cycle if convergence is not met

**Validation:**

1. The skill produces JSON output (not an error)
2. `defense_layers` includes at least 4 of the 7 layers
3. Every finding includes concrete evidence (file path, line number, code snippet)
4. No fabricated findings — every finding is verifiable by reading the cited file
5. `reg.skill.kali-audit` spans are emitted (query via the in-process `reg_query_spans` tool). The `taxonomy-map` step emits its own sub-span when `target_surface == 'supply-chain'`.

## Automated Smoke Test (Future)

When skill execution supports automated invocation (rendering templates +
calling the LLM + validating output) from a test harness, the above
fixtures can be automated as integration tests. The validation steps
would become assertions in a Rust test file.[^hendrickson-explore]

Current limitation: in-process skill execution renders templates and
calls the LLM, but does not automatically validate output against the
contract. The agent (or a human) must read the rendered output and
verify it matches the contract.

## Running the Smoke Tests

To run all smoke tests, invoke each skill from the zed-kask agent panel
with the context below. There is no standalone `kask
skill run` CLI — skills execute in-process through upstream-Zed body
injection (D1): `SkillTool::run` → `render_skill_envelope`.[^mcp-spec]

```
# 1. Supply chain audit
skill: supply-chain-sentinel
manifest_path: <workspace-root>

# 2. Runtime posture monitor (requires running session)
skill: runtime-posture-monitor
telemetry_stream: <optional-preloaded-spans>

# 3. Kali audit (includes taxonomy-map step when surface=supply-chain)
skill: kali-audit
target_surface: code
target_path: <workspace-root>
```

Check Regulation span emissions by querying the in-process
`reg_query_spans` tool (exposed through the agent panel):

```
tool: reg_query_spans
arguments: {"namespace": "reg.skill.supply-chain-sentinel", "since_hours": 1.0, "limit": 50}

tool: reg_query_spans
arguments: {"namespace": "reg.skill.runtime-posture-monitor", "since_hours": 1.0, "limit": 50}

tool: reg_query_spans
arguments: {"namespace": "reg.skill.kali-audit", "since_hours": 1.0, "limit": 50}
```

## What the Smoke Tests Catch

These smoke tests catch the issues that mechanical validation (Layers 1-4)
cannot:[^bach-bolton-smoke]

1. **Template rendering with real inputs** — the template might render
   differently with real data than with empty context
2. **LLM output quality** — the LLM might produce invalid JSON, miss fields,
   or hallucinate findings
3. **Pipeline data flow** — the agent might not correctly pass outputs from
   one phase to the next
4. **Regulation span emission** — the agent might not emit the expected spans
5. **MCP tool integration** — the skill might not correctly use MCP tools
   (e.g., `reg_query_spans` for runtime-posture-monitor)

These are the most valuable tests but also the most expensive — they require
LLM calls, a running editor session, and manual output validation. They are
recommended as a pre-release checklist, not a CI gate.

## Footnotes

[^owasp-llm]:
    OWASP. (2025). _OWASP Top 10 for Large Language Model Applications_. OWASP Foundation. https://owasp.org/www-project-top-10-for-large-language-model-applications/
    Cited for the security-threat taxonomy the security skills are designed to probe.

[^mitre-atlas]:
    MITRE. (2024). _MITRE ATLAS (Adversarial Threat Landscape for AI Systems)_. MITRE Corporation. https://atlas.mitre.org/
    Cited for the adversarial-attack taxonomy underlying the kali-audit and runtime-posture-monitor skills.

[^nist-ssdf]:
    NIST. (2022). _Secure Software Development Framework (SSDF) — SP 800-218A_. National Institute of Standards and Technology. https://csrc.nist.gov/pubs/sp/800/218/a/final
    Cited as the supply-chain and secure-development framework the supply-chain-sentinel fixture validates against.

[^bach-bolton]:
    Bach, J., & Bolton, M. (2019). _Rapid Software Testing_. Satisfice, Inc. https://www.satisfice.com/rapid-software-testing
    Cited for the exploratory-testing methodology guiding manual smoke-test fixture validation.

[^hendrickson-explore]:
    Hendrickson, E. (2013). _Explore It!: Reduce Risk and Increase Confidence with Exploratory Testing_. Pragmatic Bookshelf. https://pragprog.com/titles/ehxta/explore-it/
    Cited for the automation-from-exploratory-charters design choice in the future smoke-test harness.

[^mcp-spec]:
    Anthropic. (2024). _Model Context Protocol Specification_. Anthropic PBC. https://modelcontextprotocol.io/specification
    Cited for the MCP protocol the in-process skill execution path (D1) interoperates with.

[^bach-bolton-smoke]:
    Bach, J., & Bolton, M. (2019). _Rapid Software Testing_. Satisfice, Inc. https://www.satisfice.com/rapid-software-testing
    Cited for the rationale that smoke testing catches integration issues mechanical layer validation cannot.
