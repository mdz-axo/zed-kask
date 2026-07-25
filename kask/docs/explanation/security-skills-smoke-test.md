---
title: "Security Skills — Execution Smoke Test Procedure"
audience: [operators, developers, security-engineers]
last_updated: 2026-07-24
version: "0.31.0"
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
   (D9) — the guard layer (D4) routes inference requests to the
   configured provider.
3. The Regulation layer runs in-process (`hkask-regulation`); the
   former `kask mcp start regulation` standalone CLI has been removed.
   Regulation spans are emitted and queried in-process.
4. Working directory: zed-kask project root (the hKask workspace under
   `kask/`).

## Test Fixtures

### Fixture 1: Supply Chain Audit (supply-chain-sentinel)

**Setup:** The zed-kask project itself is the test fixture — it has
`Cargo.toml`, `Cargo.lock`, and `deny.toml`.

**Procedure:**

Invoke the `supply-chain-sentinel` skill from the zed-kask agent panel
(native agent, D2) or via the kask panel (D10). The skill executes
in-process through the `ManifestExecutor` (D1); there is no `kask skill
run` CLI. Supply the surface and userpod-host as context:

```
skill: supply-chain-sentinel
surface: cargo
userpod_host: test-auditor
```

**Expected output:**
- `select-surface` phase: discovers `Cargo.toml`, `Cargo.lock`, `deny.toml`
- `probe` phase: reads manifest entries, checks version pinning, registry trust
- `report` phase: proposes `surface: supply-chain` regression entries (if any findings)
- `convergence-check` phase: computes convergence metric

**Validation:**
1. The skill produces JSON output (not an error)
2. `manifest_paths` includes `Cargo.toml` and `Cargo.lock`
3. `defense_layers_present` includes at least `dependency_pinning` and `sbom_presence`
4. `userpod_host` is present in all outputs
5. `reg.supply_chain.*` spans are emitted (query via the in-process `reg_query_spans` tool exposed through the kask panel or agent panel)
6. No synthetic findings — every finding references a real `Cargo.toml` line

### Fixture 2: Runtime Posture Monitor (runtime-posture-monitor)

**Setup:** Requires a running zed-kask session with Regulation telemetry.

**Procedure:**

1. Generate some Regulation telemetry by running any agent task that
   emits spans (e.g., invoke a skill or run an agent panel session —
   these emit `hkask.*` performative spans in-process).

2. Invoke the `runtime-posture-monitor` skill from the agent panel:

```
skill: runtime-posture-monitor
signal: all
userpod_host: test-monitor
```

**Expected output:**
- `select-signal` phase: discovers `hkask.*` and `reg.*` span sources
- `classify-threat` phase: classifies observed signals (may find zero threats if baseline is clean)
- `emit-regulation` phase: proposes `surface: runtime` regression entries (if any threats)
- `convergence-check` phase: computes convergence metric

**Validation:**
1. The skill produces JSON output (not an error)
2. `signal_sources` includes at least one `reg.*` or `hkask.*` target
3. `userpod_host` is present in all outputs
4. `reg.runtime.*` spans are emitted (query via the in-process `reg_query_spans` tool)
5. No synthetic signals — every finding references a real span target + timestamp

### Fixture 3: Attack Taxonomy Mapper (attack-taxonomy-mapper)

**Setup:** Requires findings from `supply-chain-sentinel` (Fixture 1) to exist
in `security/regressions/` as `surface: supply-chain` entries.

**Procedure:**

Invoke the `attack-taxonomy-mapper` skill from the agent panel:

```
skill: attack-taxonomy-mapper
source: all
userpod_host: test-mapper
```

**Expected output:**
- `select-evidence` phase: discovers `surface: supply-chain` regression entries
- `map-taxonomy` phase: maps each finding to OSC&R tactic + technique
- `taxonomize` phase: proposes `taxonomy_mapping` field additions
- `convergence-check` phase: computes convergence metric

**Validation:**
1. The skill produces JSON output (not an error)
2. `findings_to_map` includes at least one finding (if regressions exist)
3. Each mapping includes `osc_r_tactic` and `osc_r_technique` (verified names)
4. No invented OSC&R categories — all mapped to existing entries in `github.com/pbom-dev/OSCAR`
5. `userpod_host` is present in all outputs
6. `reg.taxonomy.*` spans are emitted (query via the in-process `reg_query_spans` tool)

### Fixture 4: Kali Audit (kali-audit)

**Setup:** The zed-kask project itself is the test fixture.

**Procedure:**

Invoke the `kali-audit` skill from the agent panel:

```
skill: kali-audit
surface: code
userpod_host: test-auditor
```

**Expected output:**
- `select-surface` phase: discovers Rust source files
- `audit` phase: checks for unsafe blocks, panics, auth bypass, crypto misuse
- `report` phase: proposes regression entries (if any findings)
- `convergence-check` phase: computes convergence metric

**Validation:**
1. The skill produces JSON output (not an error)
2. `defense_layers` includes at least 4 of the 8 layers
3. `userpod_host` is present in all outputs
4. Every finding includes concrete evidence (file path, line number, code snippet)
5. No fabricated findings — every finding is verifiable by reading the cited file

## Automated Smoke Test (Future)

When skill execution supports automated invocation (rendering templates +
calling the LLM + validating output) from a test harness, the above
fixtures can be automated as integration tests. The validation steps
would become assertions in a Rust test file.

Current limitation: in-process skill execution renders templates and
calls the LLM, but does not automatically validate output against the
contract. The agent (or a human) must read the rendered output and
verify it matches the contract.

## Running the Smoke Tests

To run all smoke tests, invoke each skill from the zed-kask agent panel
or kask panel (D10) with the context below. There is no standalone `kask
skill run` CLI — skills execute in-process through the `ManifestExecutor`
(D1).

```
# 1. Supply chain audit
skill: supply-chain-sentinel
surface: cargo
userpod_host: smoke-test

# 2. Runtime posture monitor (requires running session)
skill: runtime-posture-monitor
signal: all
userpod_host: smoke-test

# 3. Attack taxonomy mapper (requires supply-chain findings)
skill: attack-taxonomy-mapper
source: all
userpod_host: smoke-test

# 4. Kali audit
skill: kali-audit
surface: code
userpod_host: smoke-test
```

Check Regulation span emissions by querying the in-process
`reg_query_spans` tool (exposed through the kask panel or agent panel):

```
tool: reg_query_spans
arguments: {"namespace": "reg.supply_chain", "since_hours": 1.0, "limit": 50}

tool: reg_query_spans
arguments: {"namespace": "reg.runtime", "since_hours": 1.0, "limit": 50}

tool: reg_query_spans
arguments: {"namespace": "reg.taxonomy", "since_hours": 1.0, "limit": 50}
```

## What the Smoke Tests Catch

These smoke tests catch the issues that mechanical validation (Layers 1-4)
cannot:

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
