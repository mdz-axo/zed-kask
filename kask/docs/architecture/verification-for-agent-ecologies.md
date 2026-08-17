---
title: "Verification for Agent Ecologies — Grounding Contract"
audience: [architects, developers]
last_updated: 2026-08-16
version: "1.1.0"
status: "Active"
domain: "Trust"
mds_categories: [trust, composition]
---

# Verification for Agent Ecologies — Grounding Contract

## Context

This document describes the implementation of the verification ladder from the
ABW team's paper *"Verification for Agent Ecologies: Why a declared contract is
not a contract, and what to do about it"* (working paper, 2026-08-15). The paper
identifies a class of defect in which agent declarations are well-formed,
well-typed, and false — and in which no existing check could have noticed. The
remedy is a ladder of four contracts (presence, truth, grounding, binding), each
cheap enough to run continuously, each required to demonstrate its own ability
to fail.

## The paper

**Citation:** ABW Team. *Verification for Agent Ecologies: Why a declared contract
is not a contract, and what to do about it.* Working paper, 2026-08-15.
Implements: `docs/ABW_VERIFICATION_RECONCILIATION.md`. Situates against: Trooskens
et al., *Compiled AI: Deterministic Code Generation for LLM-Based Workflow
Automation*, arXiv:2604.05150v2, 2026.

The paper's central claim: *a check that reasons about shape will pass while
content is wrong, and shape is what almost every check reasons about.* The
grounding rung is what stops a fully-typed agent ecology from being a
fully-fabricated one.

## The four-rung ladder in zed-kask

| Rung | Question | zed-kask substrate | Status |
|------|----------|---------------------|--------|
| **Presence** | Does the declared thing exist? | `validate_presence` in `local_registry.rs` — rejects cards with empty `agent_id`, `agent_type`, or `system_prompt` | Implemented |
| **Truth** | Does the stored value equal its source of truth? | Not yet implemented — requires aggregate queries against production data | Future |
| **Grounding** | Could this value have come from any available tool? | `enforce_grounding` in `grounding.rs` — checks field → tool sourcing per invocation | Implemented for `kanban-task-*` agents |
| **Binding** | Does the invocation match the declared interface? | `check_bind` in `local_runtime.rs` — classifies request, compares to `accepts` labels | Implemented |

Beneath the ladder sits a typing layer (`PortRegistry` in `port_registry.rs`)
that converts `accepts`/`produces` labels from free strings into type references.
Typing is necessary and nowhere near sufficient — a schema makes a wrong field
more credible, not less.

## The six-valued grounding vocabulary

The paper proposes a four-valued vocabulary (Sourced / Inferred / Narrative /
Unsourced). We extend it to six values because our target agents
(`kanban-task-*`) produce uncommissioned inferences, and because platform
code can derive values deterministically from sourced inputs. The `Derived`
variant (adopted from Fermi's implementation) distinguishes reproducible
computations from model inferences.

| Value | Meaning | Disposition |
|-------|---------|-------------|
| **Sourced** | A named tool returned it | Keep, mark verified (`tool_verified`) |
| **Inferred** | Judgement over sourced inputs, by design (commissioned) | Keep, mark as inference (`model_inference`) |
| **Derived** | Computed by platform code from a sourced value, deterministically | Keep, mark as derived (`platform_derived`) |
| **UncommissionedInference** | Model produced a judgment not explicitly commissioned but plausibly in scope | Keep, mark as uncommissioned (`uncommissioned_inference`) |
| **Narrative** | Prose | Keep, scan for claims it cannot support (`narrative`) |
| **Unsourced** | No tool could supply it | Null it, record what was removed (`unavailable_no_tool_source` or `tool_no_match`) |

The `Unsourced` variant carries a `tool_failed: bool` flag distinguishing
"tool was called but failed" (`tool_no_match` — transient, retry) from "no tool
was called" (`unavailable_no_tool_source` — capability gap, wire up the tool).
This distinction, adopted from Fermi's implementation, gives the operator
different remediation paths.

Provenance is stamped on the document as `<field>_provenance` keys, so
downstream consumers (the curator, ORIENT) can see grounding status without
parsing the `GroundingResult`.

The distinction: a file path is a fact sitting in a source the agent did not
consult (Unsourced); a threat level is a judgment the agent was commissioned to
make (Inferred); a file purpose the agent inferred without being asked is
UncommissionedInference.

## What is grounded

The grounding contract covers `kanban-task-*` agents — the agents spawned by
`kanban_task_spawn` that execute tasks via `swarm_delegate_local`. These agents
produce LLM output that may contain fabricated facts (file paths that don't
exist, test results that weren't run, code that wasn't written).

Grounding is a **system-level capability**, not a per-tool feature. The
`hkask-verification` crate provides the shared enforcement code
(`enforce_grounding`, `GroundingContract`, `GroundingResult`, `ProvenanceTag`,
`schema_validate`, `envelope`, `card_contract`, `rollup_trust`) and the central
grounding ledger (`VerificationStore`). Every MCP server that delegates to
agents calls `VerificationStore::enforce_for_agent()` on each delegation:

- `kanban_task_spawn` (kata-kanban) — source `"kanban_task_spawn"`
- `swarm_delegate_local` (swarm) — source `"swarm_delegate_local"`
- `swarm_execute_plan_local` (swarm) — source `"swarm_execute_plan_local"`

The central ledger is append-only, cross-tool, and cross-server. The curator
queries it via `curator_grounding_trend`, `curator_grounding_violations`, and
`curator_grounding_coverage` — closing the cybernetic feedback loop:
enforcement → ledger → curator → user → action → improved contracts → better
enforcement.

The contract is declared in `grounding::task_agent_contract()`:

| Field | Source tools | Disposition |
|-------|-------------|-------------|
| `deliverable_path` | `zed/edit_file`, `zed/write_file`, `zed/terminal` | Sourced if a file-writing tool succeeded; Unsourced (nulled) otherwise |
| `test_verdict` | `zed/terminal` | Sourced if terminal succeeded; Unsourced (nulled) otherwise |
| `summary` | (empty — commissioned judgment) | Inferred — kept |
| `approach` | (empty — commissioned judgment) | Inferred — kept |
| Any other field | (not in contract) | UncommissionedInference — kept, marked |

### Card-declared grounding (N1)

Third-party agents can self-declare their grounding contract in the agent
card's `capabilities.output_contract.grounding` field. The `card_contract`
module validates this at admission time:
- Every `status` must be one of the closed set: `sourced`, `inferred`,
  `narrative`, `unavailable`, `derived`.
- Every `sourced` entry must name at least one tool the agent declares in
  `mcp_tools`.
- `derived` entries must name `from` (the sourced field they derive from).
- `why` is mandatory (≥40 chars).

### Schema validation (N3)

A minimal JSON Schema validator (`schema_validate.rs`) with 7 supported
keywords runs AFTER grounding, BEFORE the payload is consumed. Unsupported
keywords are NOT a pass — a validator that silently ignores what it cannot
interpret returns `valid` for a document it never checked.

### Delegation-hop envelope (N2)

The `envelope.rs` module carries the enforced payload, provenance stamps,
and violations across agent-to-agent composition. Grounding survives the
hop — the composition path (the one that matters for a fleet) is the
protected one. The envelope is additive: existing keys are preserved
byte-for-byte, and the envelope is added under its own key.

### Truth rung (N4)

The `rollup_trust.rs` module documents denormalised fields and their
sources of truth. `cost` is capped at `credits_authorized`; `cost_uncapped`
is the source of truth. `balance: None` is not 0 — a failed read is not a
measured zero. The contract documents these relationships so a future code
path that writes them separately is caught.

### Narrative leak rules (N5)

The `LeakRule` enum provides two matching strategies: `Word` (plain
substring) and `Quantity` (requires a digit before the unit). The
`Quantity` rule prevents the paper's Rule 5.2 failure: a plain `" gb"`
needle matches "GBIF", so an honest summary citing its source was flagged
as fabricating a genome size. The `Quantity` rule requires a digit before
the unit, so "GBIF" does not match but "480 Mb" does.

## What is NOT grounded (stated plainly, per the paper's §6)

- **The curator agent.** The curator is a native in-process agent
  (`CuratorAgentServer`), not a local swarm agent. Its `HealthSnapshot` is
  produced by deterministic Rust code in `MetacognitionLoop::tick`, not by an
  LLM. The grounding contract has no insertion point for deterministic code.
- **Semantic hallucination inside a sourced field.** If a tool returns data and
  the model paraphrases it wrongly, every contract passes. This needs outcome
  scoring against ground truth (`validate_golden_outputs`), not grounding.
- **Prose output.** If the agent ignores the system prompt and produces prose
  instead of JSON, the grounding check is a no-op. The system prompt mitigates
  this by asking for JSON, but it is not enforced.
- **The contract is hand-declared and therefore incomplete.** It covers only the
  `"task"` agent_type. Coverage is itself a metric — the `curator_grounding_coverage`
  tool reports which agent types have delegations but no contract (the coverage
  gap). Extending to other agent types requires a new contract registered via
  `VerificationStore::register_contract()` in the same PR.

## The invocation lifecycle

The grounding check runs on the fast clock (per invocation), via
`VerificationStore::enforce_for_agent()` — called by every delegating tool
(`spawn_via_local_runtime`, `swarm_delegate_local`, `swarm_execute_plan_local`):

1. The delegating tool builds a task agent card (or reuses an expert agent).
2. The agent's system prompt instructs it to produce JSON with
   `deliverable_path`, `test_verdict`, `summary`, `approach`.
3. `runtime.delegate` runs the agent (skill cascade + tool loop + LLM call).
4. After delegation, `enforce_for_agent()` parses the response as JSON:
   - **Sourced** fields: keep, mark verified.
   - **Inferred** fields: keep, mark as inference.
   - **UncommissionedInference** fields: keep, mark.
   - **Unsourced** fields: null, retain truncated preview, scan narrative for
     leaked values.
5. A full `GroundingRecord` is written to the central ledger (append-only,
   cross-tool). When no contract exists for the agent_type, a coverage-gap
   record (`had_contract: false`) is written so the gap is visible.
6. The cleaned JSON (with unsourced fields nulled) replaces the raw response
   before it is recorded and commented.
7. Nulled fields and narrative leaks are logged at `warn!`.

The check runs **before** anything persists or renders — not as a report
afterward. The paper's Rule 4: *"the check that runs after the write is a
metric; the check that runs before it is a control."*

## The two clocks

| Clock | What runs | Where |
|-------|-----------|-------|
| **Authoring (slow)** | Presence check, typing check, card-declared grounding validation | `LocalAgentRegistry::load`, `write_card`, `hkask_verification::card_contract::validate` |
| **Invocation (fast)** | Bind check, grounding enforcement, ledger write | `VerificationStore::enforce_for_agent` (called by `swarm_delegate_local`, `swarm_execute_plan_local`, `spawn_via_local_runtime`) |
| **Cybernetic (feedback)** | `GroundingSensor` reads the central ledger and produces clean-rate, coverage-rate, and violation-delta signals | `hkask-regulation::sensor_provider::GroundingSensor` → `CyberneticsLoop` → `RegulationPolicy` |
| **Curator (query)** | Trend query, violations query, coverage query | `curator_grounding_trend`, `curator_grounding_violations`, `curator_grounding_coverage` |

The authoring gate is where enforcement by default lives: a new agent cannot
enter the catalogue with an untyped or ungrounded interface. The invocation gate
catches what the authoring gate cannot know — that this particular output, on
this particular run, contains something no tool could have supplied. The
curator gate closes the feedback loop: the operator sees trends and coverage
gaps and acts (adjusts contracts, adds tools, retires agents).

## Regulation loop integration (the cybernetic clock)

The `GroundingSensor` in `hkask-regulation` reads the central verification
ledger on every regulation tick and produces up to three signals:

- `GroundingCleanRate` — fraction of grounded delegations with zero nulled
  fields. Fires when below the floor. Encoded as -1.0 when no grounded
  delegations exist (absence ≠ 0 — paper Rule 5.3).
- `GroundingCoverageRate` — fraction of delegations with a grounding contract.
  Fires when below the floor. Encoded as -1.0 when no delegations exist.
- `GroundingViolationDelta` — change in `delegations_with_nulled` since the
  last tick. Fires only when positive (new violations).

A DB outage is NOT collapsed to "no signal" — the sensor logs a `warn!`
naming the failure and returns no signals for that tick (the `.rules`
broken-feedback-loop trap: the operator can distinguish "not configured" from
"configured but broken").

The signals flow through the regulation policy to produce alerts that name the
specific curator tool the operator should use to investigate
(`curator_grounding_violations`, `curator_grounding_coverage`).

## Curator integration (the feedback loop)

The curator MCP server (`hkask-mcp-curator`) exposes three tools that query
the central verification ledger:

- `curator_grounding_trend` — aggregates grounding records into a
  `GroundingTrendReport` (lead metric: `delegations_with_zero_nulled`).
  Supports scoping by agent or source tool.
- `curator_grounding_violations` — returns recent delegations with nulled
  fields or narrative leaks, sorted by timestamp descending.
- `curator_grounding_coverage` — reports the coverage gap: delegations with
  no grounding contract for their agent_type.

This closes the cybernetic feedback loop described in the paper's §4.1:
enforcement → ledger → curator → user → action → improved contracts → better
enforcement. Without the curator integration, grounding violations are logged
but invisible to the operator — the check gets quietly disabled.

## Design rules (from the paper, pinned in `.rules`)

1. **A check that has never been falsified is inert.** Every grounding contract
   clause has a test that breaks it and shows the check going red.
2. **Absence is not a verdict.** `task_success: None` means "not checked," not
   "failed." `bind_matched: None` means "no accepts declared," not "mismatch."
   `grounding: None` means "no contract," not "compliant."
3. **Port labels are type references.** Every `accepts`/`produces` label must
   resolve to a registered type in `PortRegistry`.
4. **The `classify_request` heuristic has no correct setting.** Its deletion is
   the success condition for the typing layer.
5. **The grounding contract is hand-declared and incomplete.** Coverage is a
   metric. Do not pretend it covers all agents.
6. **Distinguish retrieval from judgement.** Do not collapse
   `UncommissionedInference` into `Unsourced` — nulling an agent's legitimate
   reasoning removes its entire product.
7. **The lead trend metric is deletion-resistant.** `delegations_with_zero_nulled`
   is a raw count of clean delegations — it cannot be gamed by recording fewer
   delegations or retiring cards with violations. The derived `clean_rate` ratio
   IS gameable (removing violations from the denominator increases it). The
   trend report and `curator_grounding_trend` tool lead with the count, not the
   rate (paper Rule 5.4).

## References

- ABW Team. *Verification for Agent Ecologies.* Working paper, 2026-08-15.
- Trooskens, G., et al. *Compiled AI: Deterministic Code Generation for
  LLM-Based Workflow Automation.* arXiv:2604.05150v2, 2026.
- Cemri, M., et al. *Why Do Multi-Agent LLM Systems Fail?*
  arXiv:2503.13657, 2025. — 79% of failures are specification and coordination,
  not infrastructure.
- Dalrymple, D., et al. *Towards Guaranteed Safe AI.* arXiv:2405.06624, 2024. —
  the Safety Sandwich framing.

## Related

- [Swarm MCP Server Architecture](../diagrams/flowchart-swarm-architecture.md)
- [Swarm Steering Loop](../diagrams/sequence-swarm-steering-loop.md)
- [MCP Tool Call Sequence](../diagrams/sequence-mcp-tool-call.md)
- [Architecture Principles](./core/PRINCIPLES.md) — P8 Semantic Grounding
