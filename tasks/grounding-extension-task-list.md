# Grounding System Extension — Task List

## Priority 1: Regulation system integration (DONE)

### Done
- [x] Add `hkask-verification` dependency to `hkask-regulation`
- [x] Add `SignalMetric` variants: `GroundingCleanRate`, `GroundingCoverageRate`, `GroundingViolationDelta`
- [x] Add `RegulationReason` variants: `GroundingCleanRateDegraded`, `GroundingCoverageDegraded`, `GroundingViolationDeltaIncreased`
- [x] Add `RegulationData` variants for the three grounding signals
- [x] Add policy rules mapping the new metrics → Escalate to Curation
- [x] Add substitution ladder entries (empty — terminal Escalate)
- [x] Add `build_regulation_action` arms for the three new reasons
- [x] Add grounding-specific alert messages in `route_action_as_alert`
- [x] Add `GroundingSensor` in `sensor_provider.rs` (3 metric variants)
- [x] Add set-point constants and fields (`grounding_clean_rate_floor`, `grounding_coverage_rate_floor`)
- [x] Add env-var overrides with `log::warn!` on parse failure
- [x] Add `with_verification_store` / `set_verification_store` builder methods on `CyberneticsLoop`
- [x] Wire `VerificationStore::open()` in `main.rs`
- [x] 8 unit tests for `GroundingSensor` (all passing)
- [x] 3 policy rule tests (one per new metric)
- [x] 3 `build_regulation_action` tests (one per new reason)
- [x] 3 SetPoints validation tests (reject out-of-range, from_config picks up new fields)
- [x] 3 proptests (never panics, clean_rate matches report, violation_delta never fires on first tick)
- [x] 1 integration test (full tick cycle with grounding violations produces escalate alert)
- [x] Updated `policy_no_match_for_unregistered_metric` to include new metrics
- [x] Updated `default_substitution_ladders_empty_for_observational_metrics` to include grounding metrics
- [x] Clippy clean across all affected crates
- [x] All 112 tests in `hkask-regulation` pass

### Adversarial review fixes (DONE)
- [x] #1: `verify_impact` now handles grounding actions — re-senses from `VerificationStore`, classifies Accept/Stage/Block, feeds stagnation detector
- [x] #2: Fixed doc comment referencing non-existent `for_metric` → `new`
- [x] #3: Fixed `read_trend` doc — clarified 3x DB query per tick
- [x] #4: Removed hardcoded `current_nulled`/`previous_nulled` from `GroundingViolationDeltaIncreased` — delta is the actionable signal
- [x] #5: Removed `coverage_rate: -1.0` sentinel from `GroundingCleanRateDegraded` — coverage is a separate signal
- [x] #6: Extended `extract_deficit_threshold` for grounding variants — meaningful deficit/threshold values in escalation queue
- [x] #7: Added `grounding_clean_rate`/`grounding_coverage_rate` to `HealthSnapshot`; wired `VerificationStore` into `MetacognitionLoop`; surfaced in `BridgeMetacognitionProvider` JSON
- [x] #8: Documented 3x DB query as known scaling limit with suggested future optimizations
- [x] #9: Documented `SensorBus` vs `SensorRegistry` pattern as consistent with existing sensors
- [x] #10: Added 3 `route_action_as_alert` grounding message tests (one per variant)
- [x] #11: Fixed misleading substitution ladder comment (grounding = Escalate terminal, not Notify)
- [x] #12: Fixed by removing the `coverage_rate` field (Finding #5)
- [x] Added `verification_store` field to `CyberneticsLoop` struct for `verify_impact` re-sensing
- [x] Added 2 `verify_impact` grounding tests (handles action, skips when store not wired)
- [x] Added 4 `extract_deficit_threshold` grounding tests (clean_rate, coverage, delta, clamps negative)
- [x] Wired `VerificationStore` into `MetacognitionLoop` via `with_verification_store` builder
- [x] Updated `BridgeMetacognitionProvider` JSON to include grounding fields
- [x] All 543 tests pass across all affected crates; clippy clean

## Priority 2: Algedonic alerts for grounding spikes (DONE — wired by Priority 1)

The grounding reasons route to `Escalate → Curation`, which `route_action_as_alert` converts to a `RuntimeAlert` with grounding-specific messages. The alert reaches:
- The escalation queue (`persist_alert_to_queue`) — reviewable via `curator_escalations`
- The live alerts channel (`alerts_tx`) → `MetacognitionLoop` → toast sink → user
- The `RegulationArchive` fallback when the live channel is down

The `extract_deficit_threshold` function now produces meaningful deficit/threshold values for grounding variants (not 0,0). The `error_context` JSON in the escalation queue carries these values.

Env-configurable thresholds (`HKASK_GROUNDING_CLEAN_RATE_FLOOR`, `HKASK_GROUNDING_COVERAGE_RATE_FLOOR`) are wired in Priority 1 with `log::warn!` on parse failure.

Tests: `tick_with_grounding_violations_produces_escalate_alert` + 3 `route_action_as_alert` message tests verify the full alert path.

## Priority 3: Gemba walk integration

- [ ] Read the gemba-walk skill manifest (it's a discovery-only entry — the manifest may not exist locally; check `.agents/skills/gemba-walk/`)
- [ ] Add `curator_grounding_trend` and `curator_grounding_coverage` execute steps to the gemba-walk manifest's Prepare phase
- [ ] Add a "Grounding Health" section to the briefing templates
- [ ] Add proposed refinement actions for grounding coverage gaps and clean rate degradation

## Priority 4: Expand the contract registry — more agent types (DONE)

### Done
- [x] Inventoried agent types: `"task"` (kanban), `"research"` (swarm default), `"narrator"` (local agent card), `"sentiment"` (test-only)
- [x] Added `research_agent_contract()` — `sources` field sourced from search tools; `findings`/`summary` inferred
- [x] Added `narrator_agent_contract()` — `content`/`summary` both inferred (commissioned judgment)
- [x] Registered both contracts in `VerificationStore::new()` (auto-registered at construction)
- [x] Re-exported `research_agent_contract` and `narrator_agent_contract` from crate root
- [x] 7 new tests: `why` validation (2), falsification (sources nulled without tool, sources kept with tool, findings/summary inferred, content/summary inferred, uncommissioned file_path kept not nulled)
- [x] 1 registration test: `research_and_narrator_contracts_are_registered`
- [x] Fixed existing tests that used `"research"` as a coverage-gap agent_type (now has a contract)
- [x] All 139 tests in `hkask-verification` pass; 551 total across all affected crates; clippy clean

### Not done (deferred)
- [ ] `creative_agent_contract()` — low priority (no creative agents in use locally)
- [ ] `analysis_agent_contract()` — low priority (no analysis agents in use locally)
- [ ] Card-declared contract lookup in `enforce_for_agent()` — verify before registry fallback (may already be wired via `card_contract::validate`)

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
