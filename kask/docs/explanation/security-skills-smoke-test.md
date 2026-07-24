# Security Skills — Execution Smoke Test Procedure

This document describes how to manually smoke-test the security skills
against test fixtures. These tests require a running hKask instance with
LLM access — they are NOT CI gates.

## Prerequisites

1. hKask installed and initialized (`kask init` completed)
2. LLM inference configured (`HKASK_INFERENCE_*` env vars or keystore)
3. Regulation MCP server running (`kask mcp start regulation` or auto-started)
4. Working directory: hKask project root

## Test Fixtures

### Fixture 1: Supply Chain Audit (supply-chain-sentinel)

**Setup:** The hKask project itself is the test fixture — it has `Cargo.toml`,
`Cargo.lock`, and `deny.toml`.

**Procedure:**
```bash
# Run the supply-chain-sentinel skill against the hKask workspace
kask skill run supply-chain-sentinel --surface cargo --userpod-host test-auditor
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
5. `reg.supply_chain.*` spans are emitted (check via `reg_query_spans` MCP tool)
6. No synthetic findings — every finding references a real `Cargo.toml` line

### Fixture 2: Runtime Posture Monitor (runtime-posture-monitor)

**Setup:** Requires a running hKask instance with Regulation telemetry.

**Procedure:**
```bash
# 1. Generate some Regulation telemetry (run any kask command that emits spans)
kask skill audit  # emits hkask.* performative spans

# 2. Run the runtime-posture-monitor skill
kask skill run runtime-posture-monitor --signal all --userpod-host test-monitor
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
4. `reg.runtime.*` spans are emitted (check via `reg_query_spans` MCP tool)
5. No synthetic signals — every finding references a real span target + timestamp

### Fixture 3: Attack Taxonomy Mapper (attack-taxonomy-mapper)

**Setup:** Requires findings from `supply-chain-sentinel` (Fixture 1) to exist
in `security/regressions/` as `surface: supply-chain` entries.

**Procedure:**
```bash
# Run the attack-taxonomy-mapper skill
kask skill run attack-taxonomy-mapper --source all --userpod-host test-mapper
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
6. `reg.taxonomy.*` spans are emitted (check via `reg_query_spans` MCP tool)

### Fixture 4: Kali Audit (kali-audit)

**Setup:** The hKask project itself is the test fixture.

**Procedure:**
```bash
# Run the kali-audit skill against the hKask codebase
kask skill run kali-audit --surface code --userpod-host test-auditor
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

When the `kask skill run` command supports automated execution (rendering
templates + calling LLM + validating output), the above fixtures can be
automated as integration tests. The validation steps would become assertions
in a Rust test file.

Current limitation: `kask skill run` renders templates but does not
automatically call the LLM or validate output. The agent (or a human) must
read the rendered template, execute the instructions, and verify the output
matches the contract.

## Running the Smoke Tests

To run all smoke tests manually:

```bash
# 1. Supply chain audit
kask skill run supply-chain-sentinel --surface cargo --userpod-host smoke-test

# 2. Runtime posture monitor (requires running instance)
kask skill run runtime-posture-monitor --signal all --userpod-host smoke-test

# 3. Attack taxonomy mapper (requires supply-chain findings)
kask skill run attack-taxonomy-mapper --source all --userpod-host smoke-test

# 4. Kali audit
kask skill run kali-audit --surface code --userpod-host smoke-test
```

Check Regulation span emissions:
```bash
# Query Regulation spans emitted by the smoke tests
kask mcp call regulation reg_query_spans '{"namespace": "reg.supply_chain", "since_hours": 1.0, "limit": 50}'
kask mcp call regulation reg_query_spans '{"namespace": "reg.runtime", "since_hours": 1.0, "limit": 50}'
kask mcp call regulation reg_query_spans '{"namespace": "reg.taxonomy", "since_hours": 1.0, "limit": 50}'
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
LLM calls, a running instance, and manual output validation. They are
recommended as a pre-release checklist, not a CI gate.
