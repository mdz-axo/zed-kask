# Continuation Prompt: Grounding System Extension — Regulation, Curator, and Feedback Loops

## Context

You are continuing work on the verification ladder for the zed-kask agent ecology. The grounding refactor is complete: grounding has been extracted from a per-tool feature of the kanban tool into a system-level capability. The `hkask-verification` crate exists with `VerificationStore` (central append-only ledger), `GroundingRecord` (full provenance per delegation), `GroundingTrendReport` (deletion-resistant scoreboard), and `TrendScope` (Global / ByAgent / BySource). Grounding is wired into both `spawn_via_local_runtime` (kata-kanban) and `swarm_delegate_local` (swarm). The curator has three MCP tools: `curator_grounding_trend`, `curator_grounding_violations`, `curator_grounding_coverage`.

**What is NOT yet done** — the subject of this continuation — is wiring the grounding system into the regulation loop, the gemba walk, the algedonic alert system, and expanding grounding coverage to the remaining delegation paths (skill cascades, ABW cloud, expert agent reuse). The cybernetic feedback loop is architecturally present but not closed: grounding data flows into the ledger, but the regulation system doesn't sense it, the gemba walk doesn't surface it, and the curator doesn't act on it.

## What exists (post-refactor — the foundation you're building on)

### The `hkask-verification` crate

**Location:** `kask/crates/hkask-verification/`

| Module | Contents |
|--------|----------|
| `grounding.rs` | `enforce_grounding()`, `GroundingContract`, `GroundingResult`, `ProvenanceTag`, `FieldSpec`, `LeakRule`, `NARRATIVE_LEAK_RULES`, `task_agent_contract()` |
| `card_contract.rs` | `validate()` for card-declared contracts at admission time |
| `schema_validate.rs` | `validate()` minimal JSON Schema validator (7 keywords) |
| `envelope.rs` | `build()` delegation-hop envelope |
| `rollup_trust.rs` | `ROLLUP_CONTRACTS` static contracts |
| `ledger.rs` | `VerificationStore` — central append-only store with `enforce_for_agent()`, `grounding_trend()`, `grounding_violations()`, contract registry keyed by `agent_type` |
| `trend.rs` | `GroundingTrendReport`, `TrendScope` (Global / ByAgent / BySource) |
| `types.rs` | `GroundingRecord` (full provenance, append-only, cross-tool) |
| `error.rs` | `VerificationError` |

### The central grounding ledger

`VerificationStore` wraps an `HMemStore` at `mcp/verification/grounding.db`. Every grounded delegation writes a `GroundingRecord` as an h_mem under entity `verification:grounding`. Records are append-only (time-series). The store holds a contract registry (`HashMap<String, GroundingContract>` keyed by `agent_type`), initialized with `task_agent_contract()` and extensible via `register_contract()`.

### Wiring points (where grounding runs today)

| Path | Source label | File | What happens |
|------|-------------|------|-------------|
| `kanban_task_spawn` → `spawn_via_local_runtime` | `"kanban_task_spawn"` | `hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs` | `verification_store.enforce_for_agent()` called after delegation, before response is recorded |
| `swarm_delegate_local` | `"swarm_delegate_local"` | `hkask-mcp-swarm/src/local_tools.rs` | `verification_store.enforce_for_agent()` called after delegation, before stigmergy write |
| `swarm_execute_plan_local` | `"swarm_execute_plan_local"` | `hkask-mcp-swarm/src/local_tools.rs` | Same, per delegation in the plan |

### Curator MCP tools (read-only queries on the ledger)

| Tool | Parameters | Returns |
|------|-----------|---------|
| `curator_grounding_trend` | `scope` (global/by_agent/by_source), `agent_name`, `source` | `GroundingTrendReport` with `clean_rate`, `coverage_rate` |
| `curator_grounding_violations` | `since` (ISO 8601), `scope` | `Vec<GroundingRecord>` with nulled fields or narrative leaks |
| `curator_grounding_coverage` | (none) | Coverage report: which agent types have contracts vs. delegations without contracts |

### The `LocalDelegateResult` struct (post-refactor)

The `grounding_summary` field has been removed. The `raw_response` field is retained (raw LLM output before grounding). Grounding data lives in the central ledger, not on the per-delegation result. The `bind_matched` and `task_success` fields remain.

## The remaining gaps (ordered by priority)

### 1. Regulation system integration — grounding as a sense input

**The gap:** The `hkask-regulation` crate's sense phase reads latency, cost, and task-success as quality signals. It does not read grounding violation counts. A tool that was clean yesterday and is producing ungrounded output today is a regulation signal — something changed (a tool broke, an agent's prompt drifted, a model was swapped) — but the regulation loop can't see it.

**What to do:**

1. **Read the regulation sense phase.** Find where `hkask-regulation` reads its sense inputs (latency, cost, task_success). This is likely in `kask/crates/hkask-regulation/src/` — look for the sense/orient/decide/act loop structure.

2. **Add a grounding sense input.** The regulation system should query `VerificationStore::grounding_trend(TrendScope::Global)` on each sense tick (or at a configurable interval — grounding trend queries are cheap but not free). The key signal is the **delta**: did `delegations_with_nulled` increase since the last tick? A spike in nulled fields is the regulation signal.

3. **The sense input shape.** Grounding violations are `Option<usize>` (per the `.rules` no-`unwrap_or(0)` rule). The sense input should carry:
   - `grounding_clean_rate: Option<f64>` — from `GroundingTrendReport::clean_rate()`. `None` = no grounded delegations (absence ≠ 0).
   - `grounding_coverage_rate: Option<f64>` — from `coverage_rate()`. `None` = no delegations at all.
   - `grounding_violation_delta: Option<i64>` — change in `delegations_with_nulled` since last tick. `None` = first tick (no baseline). Positive = getting worse. Negative = getting better. Zero = stable.

4. **The orient phase.** The orient phase classifies the deviation. A `grounding_violation_delta > 0` is a quality regression — the orient phase should classify it as a deviation that requires action (not just logging).

5. **The decide phase.** The decide phase proposes an action. For grounding regressions, the action is: surface to the curator (algedonic alert), who surfaces to the user. The regulation system does not auto-fix grounding contracts — that's a human decision.

**Insertion points:**
- `kask/crates/hkask-regulation/src/` — the sense/orient/decide/act loop
- The regulation system needs a reference to `VerificationStore` (or a query interface). Check how it currently gets its sense inputs — does it hold references to the stores it queries, or does it receive data via a channel?

**Estimated cost:** ~150 lines + 8 tests. Requires understanding the regulation loop's sense input mechanism.

### 2. Algedonic alerts for grounding spikes

**The gap:** The curator's `curator_algedonic_log` tool records alerts that surface to the user. There is no alert fired when grounding violations spike. A tool that was clean and is now producing fabricated file paths is exactly the kind of signal the algedonic system should catch — it's a quality regression that the user needs to know about.

**What to do:**

1. **Read the algedonic alert system.** Find `curator_algedonic_log` in `hkask-mcp-curator` and understand the alert shape (severity, category, message, data).

2. **Add a grounding alert category.** The alert should fire when:
   - `grounding_violation_delta > 0` (new violations since last tick)
   - `clean_rate` drops below a configurable threshold (default: 0.8 — if more than 20% of grounded delegations have nulled fields, alert)
   - `coverage_rate` drops below a configurable threshold (default: 0.5 — if more than half of delegations have no grounding contract, alert — this is a coverage gap, not a quality regression, but the user needs to know)

3. **The alert message.** Should name the specific tool/agent if scoped: "Grounding violations increased for agent 'task_agent' (source: kanban_task_spawn): 3 new nulled fields in the last 10 delegations. Clean rate dropped from 1.0 to 0.7."

4. **Wire from regulation decide phase.** The regulation system's decide phase fires the alert via the curator's algedonic API. This is the `decide → act` step for grounding regressions.

**Insertion points:**
- `kask/mcp-servers/hkask-mcp-curator/src/` — the algedonic alert system
- `kask/crates/hkask-regulation/src/` — the decide phase

**Estimated cost:** ~100 lines + 5 tests.

### 3. Gemba walk integration

**The gap:** The `gemba-walk` skill queries algedonic alerts, pending escalations, and the curator's memory for skill performance patterns. It does not query grounding trends. The gemba walk is the human-in-the-loop review — the user's opportunity to see system health and act. Without grounding data, the user can't see "is this getting better?" during a gemba walk.

**What to do:**

1. **Read the gemba-walk skill.** The skill is at `.agents/skills/gemba-walk/`. Read the manifest and templates to understand what data it queries and how it structures the briefing.

2. **Add grounding to the briefing.** The gemba walk should include a "Grounding Health" section in its per-tool digest:
   - Current `clean_rate` and `coverage_rate` (global scope)
   - Trend direction (improving/stable/degrading — from the violation delta)
   - Top violating agents (by_source or by_agent scope, sorted by nulled count)
   - Coverage gaps (agent types with delegations but no contract)

3. **The skill should call `curator_grounding_trend` and `curator_grounding_coverage`** as part of its Prepare phase. These are existing MCP tools — no new tools needed. The skill's templates need to be updated to include the grounding data in the briefing.

4. **Proposed refinement actions.** The gemba walk's Present phase proposes refinement actions for operator approval. For grounding, the proposed actions are:
   - "Agent type 'research' has 15 delegations but no grounding contract. Register a contract?"
   - "Agent 'task_agent' clean rate dropped from 0.9 to 0.6. Review recent delegations for a pattern?"
   - "3 narrative leaks detected in the last 10 delegations. The agent may be restating unsourced values in its summary."

**Insertion points:**
- `.agents/skills/gemba-walk/manifest.yaml` — add `execute` steps for `curator_grounding_trend` and `curator_grounding_coverage`
- `.agents/skills/gemba-walk/templates/*.j2` — add grounding health section

**Estimated cost:** ~50 lines of manifest + template changes. No new Rust code (uses existing curator tools).

### 4. Expand the contract registry — more agent types

**The gap:** The contract registry is initialized with only `task_agent_contract()` (covers `agent_type: "task"`). Every other agent type (`"research"`, `"creative"`, `"meta"`, `"analysis"`, etc.) has no grounding contract — they all show up as coverage gaps. The paper's §6: "The grounding contract is hand-declared and therefore incomplete. Coverage is itself a metric."

**What to do:**

1. **Inventory agent types.** Query the local agent registry (`LocalAgentRegistry`) to find all `agent_type` values in use. Check `kask/mcp-servers/hkask-mcp-swarm/src/local_registry.rs` for how agent cards are stored and what types exist.

2. **For each agent type, determine what fields its output contains and what tools could source them.** This requires reading the agent cards' system prompts to understand what output shape they produce. The contract is hand-declared (paper §6) — there's no automatic inference.

3. **Register contracts at server startup.** Both `KanbanServer` and `SwarmServer` should register contracts for the agent types they spawn. The `VerificationStore::register_contract()` method exists for this. The registration should happen in the server's `run()` function, after the store is created.

4. **Start with the highest-value contracts.** Based on the agent inventory:
   - `"research"` — likely produces `findings`, `sources`, `summary`. `sources` should be sourced from `research_search` / `web_search` tools. `findings` and `summary` are inferred.
   - `"creative"` — likely produces `content`, `summary`. Both inferred (commissioned judgment).
   - `"analysis"` — likely produces `analysis`, `data_points`, `summary`. `data_points` should be sourced from specific tools depending on the analysis domain.

5. **Card-declared contracts.** Third-party agents can self-declare contracts via `capabilities.output_contract.grounding` (validated by `card_contract::validate`). The `VerificationStore` should check for a card-declared contract before falling back to the registry. This may already be wired — verify.

**Insertion points:**
- `kask/crates/hkask-verification/src/grounding.rs` — add new contract functions (`research_agent_contract()`, etc.)
- `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs` — register contracts in `run()`
- `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs` — register contracts in `run()`

**Estimated cost:** ~200 lines (contracts + registration) + 10 tests (one per new contract, each with a falsification test per the "check that has never been falsified is inert" rule).

### 5. Skill cascade grounding

**The gap:** Skill cascades produce LLM-synthesized text from Jinja2 templates. Some skills (e.g., `diataxis-diagram`, `sankey-flow`) produce structured output (Mermaid diagrams) with potentially fabricated content. The `golden_outputs` mechanism exists for deterministic validation but is on-demand only, not wired into the invocation path. Skill cascades don't go through `VerificationStore::enforce_for_agent()`.

**The challenge:** Skill cascades don't expose a `tool_calls` summary — the cascade runs inside the `ManifestExecutor`, and tool calls are not surfaced to the caller in the same shape as `LocalDelegateResult.tool_calls`. Grounding needs this summary to check whether a tool was actually called.

**What to do:**

1. **Read the skill executor.** `kask/crates/kask_bridge/src/skill_executor.rs` — `BridgeManifestExecutor::execute`. Understand how the cascade runs and what it returns.

2. **Surface tool-call summaries from the cascade.** The `ManifestExecutor` already tracks tool calls internally (for the `execute` action dispatch). The summary needs to be exposed on the cascade result. Check `kask/crates/hkask-templates/src/step_machine.rs` and `step_actions.rs` for where tool calls are recorded.

3. **Add a grounding contract for diagram-producing skills.** Skills like `diataxis-diagram` and `sankey-flow` produce Mermaid diagrams that may contain fabricated node names or flow values. The contract should check:
   - Node names that reference files/entities — must be sourced from a tool that returned them.
   - Flow values (in Sankey) — must be sourced from a tool that returned the data.

4. **Wire grounding into the skill executor.** After the cascade completes, call `VerificationStore::enforce_for_agent()` with `source: "skill_cascade"`, the skill name as `agent_id`, `"skill"` as `agent_type`, and the cascade output + tool-call summary.

5. **Register a `skill_agent_contract()`.** The contract for skill cascades is different from task agents — it depends on what the skill produces. Start with a minimal contract that checks for fabricated file paths in the output (same `deliverable_path` / `test_verdict` pattern as task agents, since skills that produce code often claim to have written files).

**Insertion points:**
- `kask/crates/kask_bridge/src/skill_executor.rs` — after cascade completion
- `kask/crates/hkask-templates/src/step_machine.rs` — surface tool-call summary
- `kask/crates/hkask-verification/src/grounding.rs` — `skill_agent_contract()`

**Estimated cost:** ~300 lines (tool-call surfacing is the deep change) + 10 tests. Medium-high complexity.

### 6. ABW cloud delegation grounding

**The gap:** `swarm_delegate`, `swarm_delegate_and_wait`, and `swarm_fanout` in `hkask-mcp-swarm/src/cloud_tools.rs` delegate to ABW cloud agents. The response is free prose from an uncontrolled model. A fabricated file path or test result from a cloud agent is the same defect class as from a local agent, but the operator has even less visibility (no local tool-call log).

**The challenge:** ABW agents don't produce structured JSON by default, and the `tool_calls` summary is not available for ABW delegations (the tool loop runs on ABW's side). The grounding check is limited to narrative-leak scanning — checking whether the response restates values that no tool could have sourced.

**What to do:**

1. **Read the cloud delegation tools.** `kask/mcp-servers/hkask-mcp-swarm/src/cloud_tools.rs` — `swarm_delegate` (L606), `swarm_delegate_and_wait` (L669), `swarm_fanout`.

2. **Add a cloud-agent grounding contract.** The contract for cloud agents is narrative-only — no `tool_calls` summary, so no field can be "sourced." The contract checks:
   - The response for narrative leaks (claims that look like facts but couldn't have come from any tool the cloud agent has access to).
   - File paths mentioned in the response — if the cloud agent doesn't have file-writing tools, any file path is unsourced.

3. **Wire grounding into cloud delegation.** After the cloud delegation returns, call `VerificationStore::enforce_for_agent()` with `source: "swarm_delegate"` (or `"swarm_fanout"`), the cloud agent name, `"cloud"` as `agent_type`, and the response. The `tool_calls` parameter will be empty (or contain the ABW-side tool summary if available — check whether ABW returns a tool-call log).

4. **Register a `cloud_agent_contract()`.** The contract is narrative-focused. The `LeakRule` mechanism (Word / Quantity) is the primary tool. The contract should define which leak rules apply to cloud agent output.

**Insertion points:**
- `kask/mcp-servers/hkask-mcp-swarm/src/cloud_tools.rs` — after cloud delegation returns
- `kask/crates/hkask-verification/src/grounding.rs` — `cloud_agent_contract()`

**Estimated cost:** ~150 lines + 8 tests.

### 7. Grounding bypass via expert agent reuse

**The gap:** The grounding check in `spawn_via_local_runtime` runs via `VerificationStore::enforce_for_agent()`, which checks the contract registry by `agent_type`. If `kanban_task_spawn` reuses an expert agent from the local registry whose `agent_type` is not `"task"` (e.g., `"research"`), and no contract is registered for `"research"`, grounding is bypassed entirely — a coverage-gap record is written, but the output is not grounded.

**Post-refactor status:** This is partially fixed by the refactor — `enforce_for_agent()` always writes a record (either a grounding record or a coverage-gap record), so the bypass is visible. But the output is still not grounded.

**What to do:**

1. **After Priority 4 (expand contract registry) is done**, this gap shrinks — more agent types have contracts. But expert agents may have types that don't match any registered contract.

2. **Log a warning when an expert agent is reused and no contract exists.** This surfaces the coverage gap at spawn time, not just in the trend query. The warning should name the agent_type and suggest registering a contract.

3. **Consider card-declared contracts for expert agents.** Expert agents are registered via `swarm_create_local_agent` or loaded from disk. If the agent card declares an `output_contract.grounding`, the `VerificationStore` should use it. Verify that `enforce_for_agent()` checks for card-declared contracts before falling back to the registry.

**Insertion points:**
- `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs` — the expert agent reuse path in `kanban_task_spawn`
- `kask/crates/hkask-verification/src/ledger.rs` — `enforce_for_agent()` card-declared contract lookup

**Estimated cost:** ~30 lines + 3 tests.

### 8. PortRegistry schema validation

**The gap:** The current `PortRegistry` checks label resolution (does `"task_result"` exist?) but not schema validation (does the actual output match the `task_result` schema?). The paper's §3.1 describes the target state: every `accepts`/`produces` entry is a reference to a registered type with a JSON Schema. One artifact, two uses — composition checkability + output validation.

**What to do:**

1. **Read the PortRegistry.** `kask/mcp-servers/hkask-mcp-swarm/src/port_registry.rs` — understand the current type registration and label resolution.

2. **Add `schema: Option<serde_json::Value>` per registered type.** When a type is registered, an optional JSON Schema can be provided. The schema is stored alongside the type.

3. **Validate output against the schema after grounding.** In `VerificationStore::enforce_for_agent()`, after grounding runs (and before the cleaned JSON is returned), validate the cleaned output against the schema for the agent's `produces` type. Use the existing `schema_validate::validate()` from the verification crate.

4. **Unsupported keywords are NOT a pass.** The existing `schema_validate` module already handles this — unsupported keywords return `unverified_unsupported_schema`, not `valid`.

**Insertion points:**
- `kask/mcp-servers/hkask-mcp-swarm/src/port_registry.rs` — add schema field
- `kask/crates/hkask-verification/src/ledger.rs` — validate after grounding

**Estimated cost:** ~150 lines + 8 tests. Requires `schemars` (already a dependency).

### 9. Co-evolution: grounding → skill-MCP feedback loop

**The gap:** The skill-MCP co-evolution plan (`kask/docs/plans/skill-mcp-coevolution.md`) describes three curated feedback loops. The grounding system is the missing fourth loop: grounding violations reveal where skills produce ungrounded output, which reveals either (a) the skill needs to call an MCP tool it isn't calling, or (b) the MCP tool's output shape doesn't match what the skill expects.

**What to do:**

1. **Read the co-evolution plan.** `kask/docs/plans/skill-mcp-coevolution.md` — understand the three existing loops (calibration, skill-use reporting, persistence-grounded learning).

2. **Add a grounding feedback loop.** When grounding nulls a field in a skill cascade's output, that's a signal that the skill is producing a value no tool could have sourced. The signal should flow to:
   - The Curator (via the existing `curator_grounding_violations` tool) — for trend analysis.
   - The skill-use reporting loop (if built) — as a "skill produced ungrounded output" issue.
   - The co-evolution orchestrator — as a signal that the skill needs to call an MCP tool or the MCP tool's output shape needs adjustment.

3. **Wire grounding violations into the skill-use reporting loop.** If the `curator_report_skill_use_issue` MCP tool exists (from the co-evolution plan's Phase 2), grounding violations should be reported automatically. If it doesn't exist yet, this is deferred until the co-evolution plan's Phase 2 is implemented.

4. **Update the co-evolution plan.** Add the grounding feedback loop as a fourth loop in the plan document.

**Insertion points:**
- `kask/docs/plans/skill-mcp-coevolution.md` — add grounding loop
- `kask/crates/kask_bridge/src/skill_executor.rs` — report grounding violations (after Priority 5 is done)

**Estimated cost:** ~50 lines of doc + ~30 lines of wiring (if the skill-use reporting tool exists). Deferred if the co-evolution plan's Phase 2 is not yet implemented.

## The cybernetic feedback loop (the target state after all priorities)

```
Every delegation (kanban, swarm, cloud, skill cascade)
        │
        ▼
  VerificationStore::enforce_for_agent()
        │
        ├──► Returns (GroundingResult, cleaned_json) to caller
        │     └──► Caller uses cleaned_json (unsourced fields nulled)
        │
        └──► Writes GroundingRecord to central ledger
                    │
                    ├──► hkask-regulation sense phase
                    │       └──► Detects violation delta (spike/stable/improving)
                    │              └──► Orient classifies as quality regression
                    │                     └──► Decide fires algedonic alert
                    │                            └──► Curator surfaces to user
                    │                                   └──► User acts:
                    │                                         ├── Register new contract
                    │                                         ├── Fix agent prompt
                    │                                         ├── Retire agent
                    │                                         └── Add missing tool
                    │
                    ├──► Gemba walk skill
                    │       └──► Queries curator_grounding_trend + _coverage
                    │              └──► Surfaces grounding health in briefing
                    │                     └──► Proposes refinement actions
                    │
                    ├──► Curator tools (curator_grounding_trend/violations/coverage)
                    │       └──► Any agent or skill can query grounding status
                    │
                    └──► Co-evolution loop (future)
                            └──► Grounding violations reveal skill/MCP design issues
                                   └──► Skills add tool calls / MCP tools adjust output shapes
```

## Key design rules to follow

- **MCP tool failures must not collapse to `None`** — grounding violations are logged at `warn!`, not silently skipped. The `VerificationStore` returns `Err` on DB failures, not an empty trend.
- **Absence is not a verdict** — `had_contract: false` means "no contract," not "compliant." `clean_rate: None` means "no measured delegations," not "0% clean." `grounding_violation_delta: None` means "no baseline," not "no change."
- **A check that has never been falsified is inert** — every new grounding contract clause has a test that breaks it and shows the check going red.
- **The scoreboard must not reward deletion** — lead with `delegations_with_zero_nulled`, not `nulled_fields_count` falling.
- **No `unwrap_or(0)` on regulation signals** — grounding violation counts and deltas are `Option`, not numeric with a default.
- **Stale diagnostics after bulk edits** — the crate's lib root is authoritative, not individual-file diagnostics.
- **No `mod.rs` files** — use `src/module.rs` instead.
- **Build: use `./script/clippy` instead of `cargo clippy`.**
- **Numeric env vars that fail to parse must `log::warn!` naming the malformed value, not silently fall back.** Thresholds for algedonic alerts (clean_rate threshold, coverage_rate threshold) are env-configurable and must warn on parse failure.
- **Opt-in `HKASK_USE_*` features that fail must log the failure classification**, not collapse to `None` via `.ok()?`.

## Before starting

1. **Read the architecture doc**: `zed-kask/kask/docs/architecture/verification-for-agent-ecologies.md` — the full reference for the verification ladder, six-valued vocabulary, and design rules. Verify it reflects the post-refactor state (central ledger, `VerificationStore`, curator tools).

2. **Read the `hkask-verification` crate**: `kask/crates/hkask-verification/src/ledger.rs` — the `VerificationStore` API. `types.rs` — the `GroundingRecord` struct. `trend.rs` — the `GroundingTrendReport` and `TrendScope`.

3. **Read the regulation system**: `kask/crates/hkask-regulation/src/` — the sense/orient/decide/act loop. Understand how sense inputs are currently provided (latency, cost, task_success) and how to add a new sense input.

4. **Read the curator**: `kask/mcp-servers/hkask-mcp-curator/src/` — the algedonic alert system (`curator_algedonic_log`), the three grounding tools, and how the curator surfaces issues to the user.

5. **Read the gemba-walk skill**: `.agents/skills/gemba-walk/` — the manifest and templates. Understand what data it queries and how it structures the briefing.

6. **Read the co-evolution plan**: `zed-kask/kask/docs/plans/skill-mcp-coevolution.md` — the three existing feedback loops and where the grounding loop fits.

7. **Verify the build**: `./script/clippy -p hkask-verification -p hkask-mcp-swarm -p hkask-mcp-kata-kanban -p hkask-mcp-curator` and `cargo test -p hkask-verification -p hkask-mcp-swarm -p hkask-mcp-kata-kanban -p hkask-mcp-curator --lib` — confirm all tests pass before making changes.

Then begin with Priority 1 (regulation system integration), since it closes the highest-value feedback loop — the regulation system sensing grounding violations and acting on them.
