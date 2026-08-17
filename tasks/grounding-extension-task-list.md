# Grounding System Extension — Task List

## Priority 1: Regulation system integration (IN PROGRESS)

### Done
- [x] Add `hkask-verification` dependency to `hkask-regulation`
- [x] Add `SignalMetric` variants: `GroundingCleanRate`, `GroundingCoverageRate`, `GroundingViolationDelta`
- [x] Add `RegulationReason` variants: `GroundingCleanRateDegraded`, `GroundingCoverageDegraded`, `GroundingViolationDeltaIncreased`
- [x] Add `RegulationData` variants for the three grounding signals
- [x] Add policy rules mapping the new metrics → Escalate to Curation
- [x] Add substitution ladder entries (empty — observational, terminal Escalate)
- [x] Add `build_regulation_action` arms for the three new reasons
- [x] Add `GroundingSensor` in `sensor_provider.rs` (3 metric variants)
- [x] Add set-point constants and fields (`grounding_clean_rate_floor`, `grounding_coverage_rate_floor`)
- [x] Add env-var overrides with `log::warn!` on parse failure
- [x] Add `with_verification_store` / `set_verification_store` builder methods on `CyberneticsLoop`
- [x] Wire `VerificationStore::open()` in `main.rs`
- [x] 8 unit tests for `GroundingSensor` (all passing)

### TODO — tests for Priority 1
- [ ] **Policy rule tests** (`regulation_policy.rs`): verify the three new rules match on the correct `(metric, direction)` and produce `Escalate → Curation` with the correct reason. One test per rule (3 tests).
- [ ] **`build_regulation_action` tests** (`cybernetics_loop.rs`): verify each of the three new reason arms produces a `RegulatoryAction` with the correct `RegulationData` variant and `metric_name`. One test per reason (3 tests).
- [ ] **SetPoints validation tests** (`set_points.rs`): verify the new grounding floors are validated (in [0.0, 1.0]) and that `from_config` picks up the new fields. 2 tests.
- [ ] **Proptest: `GroundingSensor` never panics** across random delegation sequences. The sensor reads from a store that may have 0..N delegations of mixed clean/nulled/coverage-gap types. 1 proptest.
- [ ] **Proptest: clean_rate signal value matches `GroundingTrendReport::clean_rate()`** for any delegation sequence. The sensor is a thin wrapper — its signal value must equal the report's computed rate (or be absent). 1 proptest.
- [ ] **Proptest: violation_delta is monotonic** — the delta is always `current - previous`, never negative when fired, and the sensor never fires on the first tick. 1 proptest.
- [ ] **Integration test**: `CyberneticsLoop::with_verification_store` registers 3 sensors. Verify via `sensor_registry.len()` or `provider_names()`. 1 test.
- [ ] **Integration test**: a full `tick()` cycle with a store that has violations produces an `Escalate` action routed to Curation. This requires a `CapturingEscalationSink` or checking the alerts channel. 1 test.

### TODO — Priority 1 design review
- [ ] Verify the `route_action_as_alert` path correctly handles the grounding `RegulationData` variants (the `extract_deficit_threshold` function returns (0,0) for non-variety variants — the alert message should still be meaningful). Check whether `route_action_as_alert` needs a grounding-specific message branch.

## Priority 2: Algedonic alerts for grounding spikes

- [ ] Read the algedonic alert system in `hkask-mcp-curator` (the `curator_algedonic_log` tool and the `RuntimeAlert` struct)
- [ ] Verify the regulation decide phase (Priority 1) fires alerts via `route_action_as_alert` — the grounding reasons already route to `Escalate → Curation`, which `route_action_as_alert` converts to a `RuntimeAlert`. This may already be wired by Priority 1.
- [ ] Add a grounding-specific alert message in `route_action_as_alert` (or verify the generic efferent message is sufficient)
- [ ] Add env-configurable thresholds: `HKASK_GROUNDING_CLEAN_RATE_FLOOR` (done in Priority 1), `HKASK_GROUNDING_COVERAGE_RATE_FLOOR` (done in Priority 1)
- [ ] Tests: verify a grounding violation spike produces an alert in the escalation queue

## Priority 3: Gemba walk integration

- [ ] Read the gemba-walk skill manifest (it's a discovery-only entry — the manifest may not exist locally; check `.agents/skills/gemba-walk/`)
- [ ] Add `curator_grounding_trend` and `curator_grounding_coverage` execute steps to the gemba-walk manifest's Prepare phase
- [ ] Add a "Grounding Health" section to the briefing templates
- [ ] Add proposed refinement actions for grounding coverage gaps and clean rate degradation

## Priority 4: Expand the contract registry — more agent types

- [ ] Inventory agent types in `LocalAgentRegistry` — find all `agent_type` values in use
- [ ] Read agent cards' system prompts to understand output shapes per type
- [ ] Add `research_agent_contract()` — sources field, findings, summary
- [ ] Add `creative_agent_contract()` — content, summary (both inferred)
- [ ] Add `analysis_agent_contract()` — analysis, data_points, summary
- [ ] Register contracts in `KanbanServer::run()` and `SwarmServer::run()`
- [ ] Verify card-declared contract lookup in `enforce_for_agent()` (check before registry fallback)
- [ ] Tests: one per new contract (with falsification test per the "check that has never been falsified is inert" rule) — 3+ tests

## Priority 5: Skill cascade grounding

- [ ] Read `kask_bridge/src/skill_executor.rs` — `BridgeManifestExecutor::execute`
- [ ] Surface tool-call summaries from the cascade (check `step_machine.rs`, `step_actions.rs`)
- [ ] Add `skill_agent_contract()` — check for fabricated file paths in output
- [ ] Wire `VerificationStore::enforce_for_agent()` after cascade completion
- [ ] Tests: 10 tests (tool-call surfacing is the deep change)

## Priority 6: ABW cloud delegation grounding

- [ ] Read `cloud_tools.rs` — `swarm_delegate`, `swarm_delegate_and_wait`, `swarm_fanout`
- [ ] Add `cloud_agent_contract()` — narrative-focused, no tool_calls
- [ ] Wire grounding into cloud delegation after response returns
- [ ] Tests: 8 tests

## Priority 7: Grounding bypass via expert agent reuse

- [ ] After Priority 4, log a warning when an expert agent is reused and no contract exists
- [ ] Verify card-declared contracts for expert agents
- [ ] Tests: 3 tests

## Priority 8: PortRegistry schema validation

- [ ] Read `port_registry.rs` — current type registration and label resolution
- [ ] Add `schema: Option<serde_json::Value>` per registered type
- [ ] Validate output against schema after grounding in `enforce_for_agent()`
- [ ] Tests: 8 tests

## Priority 9: Co-evolution feedback loop

- [ ] **BLOCKED**: The co-evolution plan file (`kask/docs/plans/skill-mcp-coevolution.md`) does not exist. Only the continuation task (`tasks/skill-mcp-coevolution-continuation.md`) exists. This priority is deferred until the plan file is created or the continuation task is resolved.
- [ ] Read the co-evolution plan (when it exists)
- [ ] Add grounding feedback loop as the fourth loop
- [ ] Wire grounding violations into the skill-use reporting loop (if `curator_report_skill_use_issue` exists — it does)

## Cross-cutting: Proptest strategy

For each new sensor, contract, and wiring point, build proptests that verify:
1. **No panics** across random inputs (delegation sequences, tool-call combinations, output shapes)
2. **Consistency** — the sensor/contract output matches the pure function it wraps
3. **Monotonicity** — deltas go in the right direction, clean_rate is in [0,1] or absent
4. **Absence ≠ verdict** — None/absent inputs produce no signal, not a zero signal
5. **Falsifiability** — every contract clause has a test that breaks it and shows the check going red
