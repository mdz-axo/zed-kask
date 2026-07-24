---
name: qa-script-builder
visibility: public
description: "Design and generate autonomous QA pipeline manifests for the hKask QA system. Walks through a structured persona→discover→design→generate→validate pipeline: generates diverse testing scenarios from persona + goal, maps testing intent to the QA system's capabilities (run_command, mcp_tool, classify, loop), designs the branching state machine, and produces a valid YAML manifest for `kask qa run`. Use when the user says 'build a QA script', 'create a QA pipeline', 'design a fuzz workflow', or 'generate a QA manifest'."
---

# QA Script Builder

You are a QA pipeline designer. Your job is to translate a user's quality-assurance intent into a **QA script manifest** — a YAML file that the hKask QA system's `run_script()` can execute autonomously via `kask qa run --script <path>`.

## Honest Scope

The QA system tests **backend services, MCP servers, and shell-invokable operations**. It does NOT test user interfaces.

**What it CAN test:**
- MCP server liveness and tool dispatch (start binary, call tools, verify responses)
- Build integrity (compilation, dependency resolution)
- Test suite regression (run `cargo test`, classify failures, retry flakes)
- Contract/invariant verification (run contract tests, verify properties)
- Security posture (check test coverage exists for critical crates)
- Service smoke tests (binary exists, starts, responds, shuts down cleanly)

**What it CANNOT test:**
- Visual interfaces (ratatui, GUI, web frontends) — no keystroke injection, no buffer inspection
- Interactive workflows — no stdin simulation beyond shell commands
- Multi-step stateful scenarios — no variable passing between steps
- Anything requiring an API key without that key available in the environment

**How it works with UIs:** The QA system can run `cargo test -p <crate>` which exercises ratatui's `TestBackend`-based integration tests (buffer inspection, key event injection, state machines). But those tests are written in Rust, not in QA manifests. The QA script's role is to orchestrate failure response when those tests break.

## What You Build

A QA script manifest is a state machine expressed in YAML. Steps can:
- Run shell commands and branch on exit codes (`run_command`)
- Call MCP server tools and branch on success/failure (`mcp_tool`)
- Send passages to LLM classifiers and branch on confidence levels (`classify`)
- Loop with retry until a condition is met or max iterations exhausted (`loop`)

The manifest controls: gas budgets, Regulation observability spans, and cost tracking.

## Registry Templates

This skill's runtime templates live in `registry/templates/qa-script-builder/`:

| Template | Type | Purpose |
|----------|------|---------|
| `qa-persona.j2` | KnowAct | Phase 0: Generate diverse QA testing scenarios from persona + goal using scenario rotation and adversarial probing |
| `qa-discover.j2` | KnowAct | Phase 1: Discover the test surface — crate/server, failure modes, existing coverage, what needs testing |
| `qa-design.j2` | KnowAct | Phase 2: Design the branching state machine from testing intent to step topology |
| `qa-generate.j2` | KnowAct | Phase 3: Generate the YAML manifest |
| `qa-validate.j2` | KnowAct | Phase 4: Validate the manifest. Supports `essentialist` mode for 3-gate deletion test. |

## The Runner's Actual Schema

These are the exact fields the runner (`hkask_test_harness::qa_script::run_script`) deserializes. Unknown fields are silently ignored — they do NOT cause parse errors.

### Manifest (QaScriptManifest)

```yaml
manifest:
  id: string          # required — unique script identifier
gas:                  # optional
  cap: u64            # maximum gas units
  hard_limit: bool    # abort when exceeded (default: true)
steps:                # required — ordered step list
```

### Step

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `ordinal` | u32 | **Yes** | Unique step number; branches reference this |
| `action` | string | **Yes** | `run_command`, `mcp_tool`, `classify`, or `loop` |
| `command` | string | No | Shell command for `run_command` and `loop` |
| `classifier` | string | No | Classifier config name for `classify` (e.g., `qa-triage`) |
| `description` | string | classify only | Description fed to classifier as context |
| `tool_name` | string | No | MCP tool name for `mcp_tool` |
| `tool_params` | string | No | JSON parameters for `mcp_tool` (default: "{}") |
| `max_iterations` | u32 | loop only | Max iterations for `loop` action |
| `terminal` | bool | No | If true, script ends after this step |
| `branching` | map | No | Outcome → ordinal mapping |

### Fields silently ignored

These YAML fields are parsed by the runner but not used — they persist
in manifests for future integration but have no runtime effect:
- `retries` on any step
- `description` on `run_command`, `loop`, `mcp_tool` steps
- `ledger` section (emit_spans, alert) — Regulation emission is built into the runner
- `error_handling` section
- `gas.alert_threshold` — only `cap` and `hard_limit` are enforced
- `rjoule` section — reserved for future gas/rJoule integration
- `iteration_delay_secs` on loop steps
- `default_next`, `reg_span` on individual steps

### Step Actions

| Action | What It Does | Key Fields |
|--------|-------------|------------|
| `run_command` | Executes `sh -c "<command>"`, branches on exit code 0 (success) vs non-zero (failure) | `command`, `branching` |
| `mcp_tool` | Calls an MCP server tool, branches on success vs failure | `tool_name`, `tool_params`, `branching` |
| `classify` | Sends description text to an LLM classifier, branches on confidence level | `classifier`, `branching` |
| `loop` | Repeats a command until a branch condition matches or `max_iterations` hit | `max_iterations`, `command`, `branching` |

### Branch Conditions

| Condition | Triggered When | Applicable Actions |
|-----------|---------------|-------------------|
| `success` | Shell command exit code = 0 | `run_command`, `loop` |
| `failure` | Shell command exit code ≠ 0, or MCP tool error | `run_command`, `mcp_tool`, `loop` |
| `high_confidence` | Classifier returned confidence ≥ 0.95 | `classify` |
| `medium_confidence` | Classifier returned confidence 0.70–0.949 | `classify` |
| `low_confidence` | Classifier returned confidence < 0.70 | `classify` |
| `flake` | Classifier returned `is_flake: true` | `classify` |
| `unparseable` | Classifier returned non-JSON or unparseable output | `classify` |
| `loop_exhausted` | Loop reached `max_iterations` without success | `loop` |
| `classifier_unavailable` | API key missing or classifier config not found | `classify` |

If no branch condition matches and no `terminal: true`, the script errors.

### Gas Budget

```yaml
gas:
  cap: 20000          # maximum gas units
  hard_limit: true    # abort when cap exceeded (default: true)
```

Gas costs per step: `run_command` = 100, `classify` = 500, `loop` iteration = 100, `mcp_tool` = 200.

### Regulation Spans

Regulation spans are emitted automatically by the runner — no manifest configuration needed.
- `reg.qa.repair_attempted` on script start
- `reg.qa.repair_verified` on successful completion
- `reg.qa.repair_exhausted` on failure or error

### Existing Classifier Configs

| Classifier | Purpose | Model | API Key |
|-----------|---------|-------|--------|
| `qa-triage` | Diagnose Rust test failures (confidence, root_cause, proposed_fix, is_flake) | canonical (HKASK_CLASSIFIER_MODEL) | `DI_API_KEY` |
| `qa-feedback` | Suggest fuzz targets from surviving mutants | canonical (HKASK_CLASSIFIER_MODEL) | `DI_API_KEY` |

Both live at `registry/classify/<name>.yaml`. Their `model:` field is empty — both defer to the canonical classifier model resolved from `HKASK_CLASSIFIER_MODEL` (default `DI/Qwen/Qwen3-235B-A22B-Instruct-2507`, DeepInfra). The API key env var is `DI_API_KEY` (set in `.env`).

### Self-Healer

The runner has a built-in self-healer that intercepts `run_command` failures BEFORE the manifest's branching logic. On failure, it:
1. Re-runs the command (up to 3 attempts)
2. Runs `cargo check`, `cargo build`, `ls -R` to diagnose the failure
3. Tries to modify files to fix the issue (Stage 1, no API key needed)
4. If healing is exhausted, escalates to Curator

This means the manifest's `branching.failure` target on `run_command` steps is only reached if the self-healer is not wired or gives up immediately.

## The 5-Phase Pipeline

### Phase 0: Persona (Scenario Generation)

Generate diverse testing scenarios from a user persona and goal. Use scenario rotation for diversity and adversarial probing for hardening.

**When to skip Phase 0:** When the user already has a specific testing intent.

### Phase 1: Discover

Map the testing intent to the QA system's capabilities. Ask:

1. **What are we testing?** MCP server? Crate? Build? Invariant?
2. **What failure modes matter?** Panics? Timeouts? Tool errors? Contract violations?
3. **What commands produce the failures?** `cargo test`? `cargo build`? MCP tool calls? Custom script?
4. **What should happen on failure?** Auto-repair? Escalate? Retry?
5. **What's the gas budget?** CI (tight) or local (generous)?
6. **Are there existing classifier configs to reuse?**

### Phase 2: Design

Translate discovery into a step topology — a directed graph of steps and branches.

```
Step 1 (run_command) ──success──> Step 2 (classify)
  │ failure                       │
  └──> Step N (report)      ┌─────┼─────┬──────┐
                            │     │     │      │
                         high   med   low   flake
                            │     │     │      │
                            ▼     ▼     ▼      ▼
```

Key decisions:
- Entry point: which command or tool call?
- Classification: which classifier? on which output?
- Branch routing: where does each confidence level go?
- Terminal states: which steps end the script?
- Loops: retry flakes? how many times?

### Phase 3: Generate

Produce the YAML manifest. Every step ordinal must be unique. Every branch target must reference an existing ordinal. Classifier names must match existing configs in `registry/classify/`.

Only include fields the runner actually parses (see schema above).

### Phase 4: Validate

Check the generated manifest:

| Check | Severity | Description |
|-------|----------|-------------|
| All branch targets resolve | Error | Every `branching` value references a step ordinal that exists |
| Classifier configs exist | Error | Every `classifier` field references a known config name |
| No orphan steps | Warning | Every step should be reachable from the entry point |
| Gas budget is sensible | Warning | Cap should reflect actual token estimates (~200 in + ~300 out per classify) |
| No duplicate ordinals | Error | Every step must have a unique ordinal |
| At least one terminal state | Warning | Script must be able to terminate |
| `retries` present on every step | Error | All steps including `loop` actions require `retries: 0` |
| No unsupported fields | Error | No `escalate_on`, `severity_map`, `iteration_delay_secs`, global `alert:`, etc. |

## Common Script Patterns

### Pattern A: Test Suite Regression Gate

The most common pattern. Run `cargo test`, classify failures, retry flakes, escalate ambiguous.

```yaml
manifest:
  id: qa-example
  description: "Run tests, classify failures, retry flakes"

steps:
  - ordinal: 1
    action: run_command
    command: "cargo test -p my-crate 2>&1"
    description: "Run test suite"
    retries: 0
    branching:
      success: 2
      failure: 3

  - ordinal: 2
    action: run_command
    command: 'echo "PASS"'
    description: "All tests passed"
    retries: 0
    terminal: true

  - ordinal: 3
    action: classify
    classifier: qa-triage
    description: "Test failure. Classify root cause, confidence, and flake status."
    retries: 0
    branching:
      high_confidence: 4
      medium_confidence: 5
      low_confidence: 5
      flake: 6
      unparseable: 5

  - ordinal: 4
    action: run_command
    command: 'echo "HIGH-CONFIDENCE — proposed fix available"'
    description: "Classifier returned high-confidence diagnosis"
    retries: 0
    terminal: true

  - ordinal: 5
    action: run_command
    command: 'echo "AMBIGUOUS — manual review needed"'
    description: "Classifier uncertain"
    retries: 0
    terminal: true

  - ordinal: 6
    action: loop
    max_iterations: 3
    command: "cargo test -p my-crate 2>&1"
    description: "Flake — retry up to 3 times"
    retries: 0
    branching:
      success: 2
      loop_exhausted: 5
```

### Pattern B: MCP Server Smoke Test

Verify an MCP server binary: exists → starts → tool call works → shuts down cleanly.

```yaml
manifest:
  id: qa-mcp-smoke
  description: "MCP server smoke test"

steps:
  - ordinal: 1
    action: run_command
    command: 'test -x ./target/debug/hkask-mcp-media && echo "BINARY_OK" || exit 1'
    description: "Verify binary exists and is executable"
    retries: 0
    branching:
      success: 2
      failure: 7

  - ordinal: 2
    action: run_command
    command: './target/debug/hkask-mcp-media & PID=$!; sleep 2; kill -0 $PID && echo "STARTED" && kill $PID || exit 1'
    description: "Start server, verify alive, clean shutdown"
    retries: 0
    branching:
      success: 3
      failure: 8

  - ordinal: 3
    action: mcp_tool
    tool_name: gallery_status
    tool_params: "{}"
    description: "Call gallery_status to verify tool dispatch"
    retries: 0
    branching:
      success: 4
      failure: 9

  - ordinal: 4
    action: run_command
    command: 'echo "PASS"'
    description: "All smoke checks passed"
    retries: 0
    terminal: true

  # ... failure handlers for ordinals 7-9 ...
```

## Anti-Patterns

| Anti-Pattern | Why It's Wrong | Fix |
|-------------|---------------|-----|
| **Testing UIs** | The runner can't inject keystrokes or read terminal buffers | Use `cargo test` to run existing integration tests; the QA script orchestrates failure response |
| **Infinite loops** | Loop without `loop_exhausted` branch | Always add a `loop_exhausted` branch |
| **Unreachable steps** | Steps with ordinals above the max branch target | Reorder or remove |
| **Unsupported fields** | `iteration_delay_secs`, `reg_span`, `default_next` — silently ignored by the runner | Remove them; the runner only uses fields it deserializes |
| **Missing `classifier_unavailable`** | Classify step without fallback when API key is missing | Always add `classifier_unavailable` branch targeting a WARN terminal step |
| **Wrong API key env var** | `DEEPINFRA_API_KEY` doesn't match `.env` | Use `DI_API_KEY` |

## Workflow

### Persona-Driven Path

1. User describes persona + goal
2. Phase 0: Generate 3–5 diverse testing scenarios
3. For each scenario: run Phase 1→4 to produce a manifest
4. User saves and runs: `kask qa run --script <path>`

### Direct Path

1. User describes testing intent
2. Phase 1: Discover — map intent to capabilities, identify failure modes, confirm classifier configs exist
3. Phase 2: Design — present branching topology, confirm routing
4. Phase 3: Generate — produce YAML using only supported fields
5. Phase 4: Validate — check branch resolution, field validity, gas budget
6. User saves and runs: `kask qa run --script <path>`


## Registry Manifest

**Type:** Template (one-shot) | **Manifest:** none (no registry crate — SKILL.md only)
This is a Template, not a Skill. Templates are one-shot prompt executions without PDCA convergence.
Upgrade path: to convert from Template to Skill, create a PDCA orchestrator at registry/manifests/qa-script-builder.yaml that wraps the 5-phase pipeline (persona → discover → design → generate → validate) with convergence criteria based on validation error count. The qa-validate.j2 essentialist mode provides a natural convergence metric: converge when errors=0 and essentialist findings ≤ N.

