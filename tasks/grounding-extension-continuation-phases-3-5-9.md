# Continuation Prompt: Grounding System Extension — Phases 3, 5–9

## Context

You are continuing work on the verification ladder for the zed-kask agent ecology. Phases 1, 2, and 4 are complete. The grounding system is wired into the regulation loop (sense → orient → decide → act), algedonic alerts fire on grounding violations, the contract registry covers `"task"`, `"research"`, and `"narrator"` agent types, and the `HealthSnapshot` includes grounding clean/coverage rates.

**What is done (Phases 1, 2, 4):**

- **Phase 1 (Regulation integration):** `GroundingSensor` (3 metric variants) registered in `CyberneticsLoop`. Three new `SignalMetric` variants (`GroundingCleanRate`, `GroundingCoverageRate`, `GroundingViolationDelta`). Three new `RegulationReason` + `RegulationData` variants. Policy rules map grounding deviations → `Escalate → Curation`. `verify_impact` re-senses grounding metrics from the `VerificationStore` and classifies Accept/Stage/Block. `route_action_as_alert` produces grounding-specific alert messages. `extract_deficit_threshold` produces meaningful deficit/threshold values for grounding variants. Env-configurable thresholds (`HKASK_GROUNDING_CLEAN_RATE_FLOOR`, `HKASK_GROUNDING_COVERAGE_RATE_FLOOR`) with `log::warn!` on parse failure. 112 tests in `hkask-regulation`, all passing.
- **Phase 2 (Algedonic alerts):** Wired by Phase 1. Grounding alerts reach the escalation queue (`persist_alert_to_queue`), the live alerts channel (`alerts_tx` → `MetacognitionLoop` → toast sink → user), and the `RegulationArchive` fallback. 4 alert message tests verify the full path.
- **Phase 4 (Contract registry):** `research_agent_contract()` (sources field sourced from search tools; findings/summary inferred) and `narrator_agent_contract()` (content/summary both inferred) registered in `VerificationStore::new()`. 8 falsification tests. 139 tests in `hkask-verification`, all passing.

**What is NOT yet done — the subject of this continuation:**

- **Phase 3 (Gemba walk integration):** The gemba-walk skill manifest and templates exist but don't query grounding data. The skill needs `curator_grounding_trend` and `curator_grounding_coverage` execute steps and a "Grounding Health" section in the briefing.
- **Phase 5 (Skill cascade grounding):** Skill cascades produce LLM-synthesized text from Jinja2 templates. They don't go through `VerificationStore::enforce_for_agent()`. The cascade needs tool-call summaries surfaced and a `skill_agent_contract()`.
- **Phase 6 (ABW cloud delegation grounding):** **ALREADY DONE** — `enforce_narrative` is wired into `swarm_delegate`, `swarm_delegate_and_wait`, and `swarm_fanout` in `cloud_swarm_tools.rs`. The cloud agent contract is narrative-focused (leak scanning). No further work needed.
- **Phase 7 (Expert agent reuse):** **PARTIALLY DONE** — card-declared grounding contracts are validated at admission time in `kanban_task_spawn` (line 1118–1132). The `enforce_and_stamp` call at line 1318 uses `agent.agent_type`, so expert agents with types `"task"`, `"research"`, or `"narrator"` are now grounded. The remaining gap is logging a warning when an expert agent is reused and its `agent_type` has no contract.
- **Phase 8 (PortRegistry schema validation):** **ALREADY DONE** — `PortRegistry` has `schema: Option<serde_json::Value>` per registered type, `validate_output()` method, and is wired into `swarm_delegate_local` (line 132). The kata-kanban path also has inline schema validation after grounding (line 1331). No further work needed.
- **Phase 9 (Co-evolution feedback loop):** **BLOCKED** — the co-evolution plan file (`kask/docs/plans/skill-mcp-coevolution.md`) does not exist. The `curator_report_skill_use_issue` tool exists and is wired into skill `on_failure` configs. Grounding violations could be reported via this tool, but the co-evolution plan needs to be created first.

## What exists (the foundation you're building on)

### The `hkask-verification` crate

**Location:** `kask/crates/hkask-verification/`

The central grounding ledger (`VerificationStore`) wraps an `HMemStore` at `mcp/verification/grounding.db`. Every grounded delegation writes a `GroundingRecord`. The store holds a contract registry (`HashMap<String, GroundingContract>` keyed by `agent_type`), initialized with `task_agent_contract()`, `research_agent_contract()`, and `narrator_agent_contract()`.

Key API:
- `enforce_for_agent(source, agent_id, agent_type, output_json, tool_calls, response)` → `(Option<GroundingResult>, Value)` — runs grounding, writes a record, returns cleaned JSON.
- `enforce_and_stamp(source, agent_id, agent_type, response, tool_calls)` → `EnforcementOutcome` — higher-level wrapper that parses, enforces, warns, and returns the outcome for stamping.
- `enforce_narrative(source, agent_id, agent_type, narrative)` → `GroundingResult` — narrative-only grounding for cloud agents (no tool_calls, leak scanning only).
- `grounding_trend(scope)` → `Result<GroundingTrendReport, VerificationError>` — aggregates records.
- `grounding_violations(since, scope)` → `Result<Vec<GroundingRecord>, VerificationError>` — recent violations.
- `grounding_coverage()` → `Result<Vec<CoverageEntry>, VerificationError>` — coverage gap report.

### Wiring points (where grounding runs today)

| Path | Source label | File | What happens |
|------|-------------|------|-------------|
| `kanban_task_spawn` → `spawn_via_local_runtime` | `"kanban_task_spawn"` | `hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs:1318` | `enforce_and_stamp()` called after delegation, before response is recorded. Card-declared contracts validated at admission (line 1118). Schema validation after grounding (line 1331). |
| `swarm_delegate_local` | `"swarm_delegate_local"` | `hkask-mcp-swarm/src/local_tools.rs:107` | `enforce_and_stamp()` called after delegation, before stigmergy write. PortRegistry schema validation at line 132. |
| `swarm_execute_plan_local` | `"swarm_execute_plan_local"` | `hkask-mcp-swarm/src/local_tools.rs` | Same, per delegation in the plan. |
| `swarm_delegate` (cloud) | `"swarm_delegate"` | `hkask-mcp-swarm/src/cloud_swarm_tools.rs:708` | `enforce_narrative()` called after cloud delegation returns. |
| `swarm_delegate_and_wait` (cloud) | `"swarm_delegate_and_wait"` | `hkask-mcp-swarm/src/cloud_swarm_tools.rs:846` | Same. |
| `swarm_fanout` (cloud) | `"swarm_fanout"` | `hkask-mcp-swarm/src/cloud_swarm_tools.rs:1450` | Same, per delegation. |

### The regulation system (Phase 1 wiring)

**Location:** `kask/crates/hkask-regulation/src/`

The `CyberneticsLoop` holds an `Option<Arc<VerificationStore>>` (wired via `with_verification_store`). Three `GroundingSensor` instances (one per metric: `CleanRate`, `CoverageRate`, `ViolationDelta`) are registered in the `SensorBus`. Each tick, the sensors query `grounding_trend(Global)` and produce signals when metrics deviate from set-points.

The `verify_impact` function re-senses grounding metrics from the `VerificationStore` and classifies actions as Accept/Stage/Block. The `MetacognitionLoop` also holds an `Option<Arc<VerificationStore>>` (wired via `with_verification_store`) and populates `grounding_clean_rate`/`grounding_coverage_rate` on the `HealthSnapshot`.

### The gemba-walk skill (Phase 3 target)

**Location:** `.agents/skills/gemba-walk/SKILL.md` (discovery-only), `kask/registry/manifests/gemba-walk.yaml` (manifest), `kask/registry/templates/gemba-walk/*.j2` (templates)

The manifest has 8 steps:
1. `execute` `curator_algedonic_log` — query algedonic alerts
2. `execute` `curator_escalations` — query pending escalations
3. `execute` `curator_consult` — query curator memory for skill performance
4. `select` `synthesize-briefing` — structure the three signal channels into a briefing
5. `select` `present-briefing` — render the briefing as markdown
6. `select` `recommend-actions` — propose refinement actions
7. `compute` `lisp.eval` — convergence signal
8. `loop` — re-enter if not converged

The briefing has four sections: system health summary, algedonic alert digest, escalation backlog digest, per-skill performance digest. Grounding health is NOT included.

### The skill executor (Phase 5 target)

**Location:** `kask/crates/kask_bridge/src/skill_executor.rs`

`BridgeManifestExecutor::execute_skill` resolves a skill name to its YAML manifest, loads it as a `BundleManifest`, and runs `ManifestExecutor::execute_manifest()`. The `CascadeOutcome` carries `context` (a `StepContext` = `HashMap<String, Value>`), `iterations`, `exit_kind`, `last_result_step`, `budget_snapshot`, and `resume_text`.

The `StepMachine` in `kask/crates/hkask-templates/src/step_machine.rs` tracks tool calls internally (for the `execute` action dispatch), but the tool-call summary is NOT surfaced on the `CascadeOutcome`. The `step_actions.rs` file (line 339) shows that `execute_tool_invoke` records tool calls, but they're not aggregated into a summary that the caller can access.

### The PortRegistry (Phase 8 — already done)

**Location:** `kask/mcp-servers/hkask-mcp-swarm/src/port_registry.rs`

`PortTypeEntry` has `schema: Option<serde_json::Value>`. `register_type(label, schema)` registers a type with an optional JSON Schema. `validate_output(produces, output)` validates an agent's output against the schema for its `produces` type. `schema_for(label)` returns the schema for a registered type. Already wired into `swarm_delegate_local` at line 132.

## The remaining work (ordered by priority)

### Phase 3: Gemba walk integration

**The gap:** The gemba-walk skill queries algedonic alerts, pending escalations, and curator memory, but not grounding trends. The operator can't see "is grounding getting better?" during a gemba walk. The `curator_grounding_trend` and `curator_grounding_coverage` MCP tools exist but are not called by the skill.

**What to do:**

1. **Read the gemba-walk manifest** at `kask/registry/manifests/gemba-walk.yaml`. Understand the step structure and `input_mapping` pattern.

2. **Add two new `execute` steps** after step 3 (curator_consult):
   - Step 3a: `execute` `curator_grounding_trend` with `scope: "global"`. Query the global grounding trend (clean_rate, coverage_rate, violation counts).
   - Step 3b: `execute` `curator_grounding_coverage`. Query the coverage report (which agent types have contracts vs. coverage gaps).
   Both steps should use `on_failure: { action: report, resume: "..." }` per the `.rules` pattern.

3. **Update the `input_mapping`** of step 4 (synthesize-briefing) to include `grounding_trend_result: "{{ step_3a_result }}"` and `grounding_coverage_result: "{{ step_3b_result }}"`. **Note:** adding steps shifts ordinals — all downstream `step_N_result` references must be updated. The existing steps 4–8 become steps 5–10 (or use non-sequential ordinals like 3a/3b to avoid renumbering — check whether the manifest parser supports non-sequential ordinals).

4. **Update the `synthesize-briefing.j2` template** to include a "Grounding Health" section:
   - Current `clean_rate` and `coverage_rate` (from the trend result)
   - Trend direction (improving/stable/degrading — from the violation delta, if available)
   - Top coverage gaps (agent types with delegations but no contract, from the coverage result)
   - If the trend query failed or returned empty, note it explicitly — do not silently omit the section.

5. **Update the `recommend-actions.j2` template** to propose grounding-specific refinement actions:
   - "Agent type 'X' has N delegations but no grounding contract. Register a contract?"
   - "Clean rate dropped from X% to Y%. Review recent violations via curator_grounding_violations?"
   - "N narrative leaks detected. The agent may be restating unsourced values in its summary."

6. **Update the manifest compliance tests** in `kask/crates/hkask-templates/tests/yaml_schema_validation.rs` to pin the new step count and execute step ordinals.

**Estimated cost:** ~80 lines of manifest + template changes. No new Rust code (uses existing curator tools).

### Phase 5: Skill cascade grounding

**The gap:** Skill cascades produce LLM-synthesized text from Jinja2 templates. Some skills (e.g., `diataxis-diagram`, `sankey-flow`) produce structured output (Mermaid diagrams) with potentially fabricated content. Skill cascades don't go through `VerificationStore::enforce_for_agent()`.

**The challenge:** Skill cascades don't expose a `tool_calls` summary — the cascade runs inside the `ManifestExecutor`, and tool calls are not surfaced to the caller in the same shape as `LocalDelegateResult.tool_calls`. Grounding needs this summary to check whether a tool was actually called.

**What to do:**

1. **Read the skill executor.** `kask/crates/kask_bridge/src/skill_executor.rs` — `BridgeManifestExecutor::execute_skill`. Understand how the cascade runs and what it returns (`CascadeOutcome`).

2. **Read the step machine.** `kask/crates/hkask-templates/src/step_machine.rs` — `StepMachine`, `CascadeOutcome`, `StepContext`. Read `step_actions.rs` — `execute_tool_invoke` (line 443), `invoke_tool` (line 1292). Understand where tool calls are recorded.

3. **Surface tool-call summaries from the cascade.** The `StepMachine` already tracks tool calls internally (for the `execute` action dispatch). Add a `tool_calls: Vec<Value>` field to `CascadeOutcome` (or a `tool_call_summary` method on `StepContext`). The summary should have the same shape as `LocalDelegateResult.tool_calls`: `[{"tool": "server/tool_name", "ok": true/false}]`.

4. **Add a `skill_agent_contract()`.** The contract for skill cascades checks for fabricated file paths in the output (same `deliverable_path` / `test_verdict` pattern as task agents, since skills that produce code often claim to have written files). Register it for `agent_type: "skill"`.

5. **Wire grounding into the skill executor.** After the cascade completes, call `VerificationStore::enforce_for_agent()` with `source: "skill_cascade"`, the skill name as `agent_id`, `"skill"` as `agent_type`, and the cascade output + tool-call summary. The `BridgeManifestExecutor` needs a reference to the `VerificationStore` — add it as a constructor parameter or a builder method.

6. **Tests:**
   - Tool-call summary is correctly aggregated from the cascade (1 test)
   - Skill output with fabricated file path is nulled (1 falsification test)
   - Skill output with sourced file path is kept (1 test)
   - Skill output with no tool calls — inferred fields kept, unsourced fields nulled (1 test)
   - Grounding record written to the ledger (1 test)
   - `enforce_for_agent` with `"skill"` agent_type (1 test)
   - `skill_agent_contract()` has `why` for every field (1 test)
   - Proptest: grounding never panics across random skill outputs (1 proptest)
   - Proptest: tool-call summary is consistent with the cascade's internal tracking (1 proptest)
   - Integration: `BridgeManifestExecutor` with a `VerificationStore` grounds the cascade output (1 integration test)

**Estimated cost:** ~300 lines (tool-call surfacing is the deep change) + 10 tests. Medium-high complexity.

### Phase 7: Grounding bypass via expert agent reuse (partially done)

**The gap:** When `kanban_task_spawn` reuses an expert agent whose `agent_type` is not `"task"`, `"research"`, or `"narrator"` (e.g., `"sentiment"`), grounding is bypassed — a coverage-gap record is written, but the output is not grounded.

**Post-Phase-4 status:** This gap has shrunk — `"task"`, `"research"`, and `"narrator"` now have contracts. But expert agents may have types that don't match any registered contract.

**What to do:**

1. **Log a warning when an expert agent is reused and no contract exists.** In `kanban_task_spawn` (line 1098–1110), after the agent is resolved, check if the `agent_type` has a contract in the `VerificationStore`. If not, log a `warn!` naming the `agent_type` and suggesting registering a contract. This surfaces the coverage gap at spawn time, not just in the trend query.

2. **Verify card-declared contracts for expert agents.** The card-declared contract validation at line 1118–1132 validates the contract's structure (sourced entries name declared tools, `why` is mandatory). Verify that `enforce_for_agent()` checks for a card-declared contract before falling back to the registry. If it doesn't, add the lookup.

3. **Tests:**
   - Expert agent with no contract logs a warning (1 test)
   - Expert agent with a contract is grounded (1 test)
   - Card-declared contract is used when the registry has no contract for the agent_type (1 test)

**Estimated cost:** ~30 lines + 3 tests.

### Phase 9: Co-evolution feedback loop (blocked)

**The gap:** The co-evolution plan file (`kask/docs/plans/skill-mcp-coevolution.md`) does not exist. The `curator_report_skill_use_issue` tool exists and is wired into skill `on_failure` configs. Grounding violations could be reported via this tool, but the co-evolution plan needs to be created first.

**What to do:**

1. **Create the co-evolution plan file** at `kask/docs/plans/skill-mcp-coevolution.md`. Document the three existing feedback loops (calibration, skill-use reporting, persistence-grounded learning) and the proposed fourth loop (grounding feedback).

2. **Add a grounding feedback loop.** When grounding nulls a field in a skill cascade's output (Phase 5), that's a signal that the skill is producing a value no tool could have sourced. The signal should flow to:
   - The Curator (via the existing `curator_grounding_violations` tool) — for trend analysis.
   - The skill-use reporting loop — as a "skill produced ungrounded output" issue. Wire `enforce_for_agent` to call `curator_report_skill_use_issue` when grounding nulls a field in a skill cascade.

3. **Update the co-evolution plan** with the grounding feedback loop as the fourth loop.

**Estimated cost:** ~50 lines of doc + ~30 lines of wiring (if Phase 5 is done). Blocked on Phase 5.

## Key design rules to follow

- **MCP tool failures must not collapse to `None`** — grounding violations are logged at `warn!`, not silently skipped. The `VerificationStore` returns `Err` on DB failures, not an empty trend.
- **Absence is not a verdict** — `had_contract: false` means "no contract," not "compliant." `clean_rate: None` means "no measured delegations," not "0% clean." `grounding_violation_delta: None` means "no baseline," not "no change."
- **A check that has never been falsified is inert** — every new grounding contract clause has a test that breaks it and shows the check going red.
- **The scoreboard must not reward deletion** — lead with `delegations_with_zero_nulled`, not `nulled_fields_count` falling.
- **No `unwrap_or(0)` on regulation signals** — grounding violation counts and deltas are `Option`, not numeric with a default.
- **Stale diagnostics after bulk edits** — the crate's lib root is authoritative, not individual-file diagnostics.
- **No `mod.rs` files** — use `src/module.rs` instead.
- **Build: use `./script/clippy` instead of `cargo clippy`.**
- **Numeric env vars that fail to parse must `log::warn!` naming the malformed value, not silently fall back.**
- **Opt-in `HKASK_USE_*` features that fail must log the failure classification**, not collapse to `None` via `.ok()?`.
- **Renumber carefully** — when adding `execute` steps to a manifest, all downstream `input_mapping` references (`step_N_result`) must be updated. A missed renumber will silently bind to the wrong step's output.
- **Update manifest compliance tests** in the same PR as the manifest change.

## Before starting

1. **Verify the build:** `./script/clippy -p hkask-verification -p hkask-regulation -p hkask-mcp-swarm -p hkask-mcp-curator -p hkask-mcp-kata-kanban -p kask_bridge -p zed` and `cargo test -p hkask-verification -p hkask-regulation -p hkask-mcp-swarm -p hkask-mcp-curator -p hkask-mcp-kata-kanban --lib` — confirm all tests pass before making changes.

2. **Read the gemba-walk manifest:** `kask/registry/manifests/gemba-walk.yaml` — understand the step structure, `input_mapping` pattern, and `on_failure` config.

3. **Read the gemba-walk templates:** `kask/registry/templates/gemba-walk/synthesize-briefing.j2`, `present-briefing.j2`, `recommend-actions.j2` — understand the briefing structure and where grounding health fits.

4. **Read the skill executor:** `kask/crates/kask_bridge/src/skill_executor.rs` — `BridgeManifestExecutor::execute_skill`. Understand how the cascade runs and what `CascadeOutcome` carries.

5. **Read the step machine:** `kask/crates/hkask-templates/src/step_machine.rs` — `CascadeOutcome`, `StepContext`. Read `step_actions.rs` — `execute_tool_invoke`, `invoke_tool`. Understand where tool calls are recorded.

6. **Read the manifest compliance tests:** `kask/crates/hkask-templates/tests/yaml_schema_validation.rs` — understand how step counts and execute step ordinals are pinned.

Then begin with Phase 3 (gemba walk integration), since it's the lowest-risk change (no new Rust code, just manifest + template edits) and directly closes the human-in-the-loop feedback loop.
