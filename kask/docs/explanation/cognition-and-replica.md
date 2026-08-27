---
title: "Cognition and Replica — Scenario Forecasting, Nu-Event Semantics, Companies Server"
audience: [architects, developers, operators, agents]
last_updated: 2026-08-14
version: "0.38.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, lifecycle, curation]
---

# Cognition and Replica

This document consolidates three topics that share a single theme: how hKask represents, processes, and forecasts cognitive artifacts inside zed-kask. The scenario forecasting pipeline integrates three research frameworks to build, forecast, and evaluate futures. The ν-event semantics define the atomic unit of observability that feeds the Regulation. The Companies MCP server provides the investment research tooling that operationalizes forecasting and valuation. Together, they form the cognition layer — the mechanisms by which hKask agents perceive, reason about, and predict the world.

All three subsystems run inside zed-kask: the scenarios and companies MCP servers are registered as builtin context servers launched as child processes over stdio (D1–D3), and ν-events flow through the in-process `RegulationSink`. The standalone `kask` CLI, HTTP API server, and Matrix transport have been removed; the Curator is a native agent inside zed-kask (D2) that evaluates in-process agent events rather than Matrix messages. See the [zed-kask Host Architecture Plan](../architecture/zed-host-architecture-plan.md) for the D1–D28 integration seams.

---

## 1. Scenario Forecasting and Planning

### Statement

The `hkask-mcp-scenarios` server integrates three complementary research frameworks to build futures, forecast their likelihood, and measure whether the project improved decision quality. Schwartz builds compelling narratives; Tetlock measures accuracy; Chermack evaluates effectiveness.[^schwartz][^tetlock][^chermack] None of the three is sufficient alone — Schwartz without Tetlock is a good story that might be wrong; Tetlock without Schwartz is precision without imagination; Chermack without either is assessment without methodology.

### Evidence

#### Theoretical Foundations

| Framework                             | Author                                                       | Core Question                               | Tool Stage                                                                                               |
| ------------------------------------- | ------------------------------------------------------------ | ------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| **Schwartz Method**                   | Peter Schwartz, _The Art of the Long View_ (1991)            | "What could happen?"                        | `scenario-builder` skill's 2×2 pipeline; companies 2×2 bridge                                            |
| **Superforecasting**                  | Philip Tetlock, _Superforecasting_ (2015)                    | "How likely is each event?"                 | `scenario_calibrate`, `scenario_update`, `scenario_score`, `scenario_synthesize`, `scenario_calibration` |
| **Performance-Based Scenario System** | Thomas Chermack, _Scenario Planning in Organizations_ (2011) | "Did the project improve decision quality?" | `scenario_frame`, `scenario_assess`                                                                      |

Schwartz developed his method at Royal Dutch Shell, anticipating the 1973 oil crisis, 1986 price collapse, and Soviet collapse — not by predicting any of them, but by having strategies ready for worlds where they could happen. Key concepts: focal question, driving forces (STEEP: Society, Technology, Economy, Environment, Politics), critical uncertainties (importance × uncertainty → 2×2 matrix), scenario narratives, robust strategies, early-warning indicators.

Tetlock's Good Judgment Project identified what makes the top 2% of forecasters different: better process, not higher IQ. The Ten Commandments encode this process: triage (Goldilocks-zone questions), Fermi-ize (break into sub-questions), outside view first (base rates), incremental belief updating (0.05 at a time), dragonfly-eye (integrate perspectives), degrees of doubt (full 0–100% scale), under/overconfidence balance, postmortems, team management, error-balancing.

Chermack's contribution is the evaluation framework. Most scenario planning literature describes how to build scenarios. Chermack asks: "Did it work? How do you know?" His five-phase Performance-Based Scenario System: Project Preparation, Scenario Exploration, Scenario Development, Scenario Implementation, Project Assessment.

#### Conversational Design

The `scenario_frame` tool is designed to invite rather than interrogate. Most scenario projects fail at the framing stage — not because the questions are wrong, but because they are asked in the wrong way. Formal diagnostic questions create resistance. The 7-turn conversational protocol is built on three pillars:

**Improv Postures** (hKask improv skill): Plussing (default — accept, build on, silently let go), Yes And (accept and extend), Yes But (constrain without contradicting).

**Kata Coaching** (hKask kata-starter skill): The agent is a coach, not an interviewer. The user is the domain expert. Target: 15-20 minutes.

**Behavioral Psychology**: Foot-in-the-door (Cialdini), curiosity gap (Loewenstein), loss aversion (Kahneman), social proof (Cialdini), peak-end rule (Kahneman), processing fluency, IKEA effect (Norton, Mochon, Ariely).

| Turn | Opening                                                                 | Improv Mode | Psychology                | What it captures                   |
| ---- | ----------------------------------------------------------------------- | ----------- | ------------------------- | ---------------------------------- |
| 1    | "So — tell me a bit about what's on your mind."                         | Plussing    | Foot-in-the-door          | Subject, context, emotional stakes |
| 2    | "If you had a clearer picture, what would you actually do differently?" | Yes, And    | Curiosity gap             | Decision at stake, focal question  |
| 3    | "When do you actually need to make this call?"                          | Coaching    | Temporal anchoring        | Time horizon, action deadline      |
| 4    | "Let's start with what's definitely NOT on the table."                  | Yes, But    | Loss aversion             | Out-of-scope, then in-scope        |
| 5    | "Who else has skin in this game?"                                       | Plussing    | Social proof + contrarian | Stakeholders and perspectives      |
| 6    | "What does 'good enough' look like?"                                    | Yes, And    | Peak-end begins           | Success criteria, use case         |
| 7    | "What are we assuming that might be completely wrong?"                  | Yes, But    | Peak-end closes           | Assumptions, constraints           |

#### The Integrated Pipeline

**Phase 0: FRAME** — `scenario_frame` → 7-question Socratic interview protocol. Focal question + decision at stake (Schwartz Stage 1), time horizon + action deadline (Chermack Phase 1), scope boundaries, stakeholders, use case, success criteria, constraints.

**Phase 1: BRAINSTORM** — `scenario_brainstorm` → 4-round temperature-shifting protocol: DIVERGE (high temp, 4+ personas, 12+ candidate events), GROUND (medium temp, anchor in verified facts), LINK (low temp, causal chains), PRUNE (analytical, merge overlaps, converge to 4-8 events). Plus `scenario_research` (evidence-aware extraction from web search) and `scenario_triage` (classify as clocklike/goldilocks/cloudlike per Tetlock #1).

**Phase 2: QUANTIFY** — `scenario_quantify` (conditional probability tree, topological sort, marginals, joint, sensitivity ranking), `scenario_calibrate` (Fermi decomposition + outside-view base-rate blend per Tetlock #2-3), `scenario_update` (Bayesian evidence revision per Tetlock #4).

**Phase 3: SYNTHESIZE** — `scenario_synthesize` (dragonfly-eye aggregation per Tetlock #5 — Empirical-Bayes weighted average, disagreement scoring, dissent identification), `scenario_sensitivity` (variance contribution ranking per Tetlock #6).

**Phase 4: TRACK** — `scenario_score` (Brier scoring + auto-update suggestions per Tetlock #7-8), `scenario_calibration` (calibration curve with over/underconfidence detection per Tetlock #7).

**Phase 5: ASSESS** — `scenario_assess` → Five-phase evaluation (Chermack). Per-phase scores with strengths, gaps, recommendations. Answers: "Did this project improve decision quality?"

#### Key Design Decisions

1. **Events, not axes** (inspired by MAIA): Traditional Schwartz scenario planning crosses two critical uncertainties to produce four quadrant scenarios. The event-tree approach uses binomial events with conditional dependencies instead. This is more granular, more testable, and maps directly to Tetlock's Fermi decomposition.

2. **Computed certainty, not stored certainty**: The `certainty_tier` is derived from `probability` on access via `ScenarioEvent::certainty_tier()`. This prevents the stale-field divergence bug where a Bayesian update changes the probability but leaves the tier unchanged.

3. **Error types, not strings**: All error paths use the `ScenarioError` enum (via `thiserror`), following hKask conventions. The CI pipeline enforces this — `String` errors are prohibited by `scripts/check-string-errors.sh`.

4. **Journal-persisted forecast store with calibration tracking**: Forecasts are stored when scored and can be queried for calibration curves. The store uses append-only journal persistence (O(1) writes) with automatic snapshot compaction at 100 entries. Survives server restarts.

5. **Conditional independence assumption in event trees**: When an event has multiple parents, the server averages their single-parent conditional contributions for marginalization. This is an explicit heuristic, not conditional independence.

6. **Agent does research; server does math**: The scenarios server does not collect web research. The agent supplies raw research text to `scenario_research`; the server returns an extraction scaffold. The agent must create and review candidate events before quantification.

#### Architecture

```
mcp-servers/hkask-mcp-scenarios/
├── Cargo.toml
├── src/
│   ├── main.rs             # Binary entrypoint (bootstrap + run)
│   ├── hkask_mcp_scenarios.rs # Server struct, 21 MCP tools, request types
│   ├── types.rs            # Core data model (events, trees, forecasts, assessment)
│   ├── templates.rs        # Prompt/document templates
│   └── superforecast.rs    # Computation engine (Fermi, Bayes, Brier, trees, assessment)
```

Dependencies: `hkask-mcp`, `hkask-types`, `rmcp`, `thiserror`, `serde`/`serde_json`, `schemars`, `chrono`.

### Diagram

```mermaid
flowchart TD
    FRAME["Phase 0: FRAME\nscenario_frame\n7-turn interview"]
    BRAIN["Phase 1: BRAINSTORM\nscenario_brainstorm\nDIVERGE → GROUND → LINK → PRUNE"]
    QUANT["Phase 2: QUANTIFY\nscenario_quantify\ncalibrate, update"]
    SYNTH["Phase 3: SYNTHESIZE\nscenario_synthesize\ndragonfly-eye aggregation"]
    TRACK["Phase 4: TRACK\nscenario_score\nBrier scoring"]
    ASSESS["Phase 5: ASSESS\nscenario_assess\nChermack 5-phase evaluation"]

    FRAME --> BRAIN
    BRAIN --> QUANT
    QUANT --> SYNTH
    SYNTH --> TRACK
    TRACK --> ASSESS
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-COG-002
verified_date: 2026-08-05
verified_against: mcp-servers/hkask-mcp-scenarios/src/hkask_mcp_scenarios.rs (22 tool routers), mcp-servers/hkask-mcp-scenarios/src/superforecast.rs (engine functions)
status: VERIFIED (v3 — corrected tool count to 21 and directory sketch to the post-split layout per 2026-08-05 audit)
-->

### Implications

The scenario forecasting pipeline is the cognitive-forecasting analog of the Regulation homeostatic loop. Where the Regulation senses → compares → computes → acts → verifies on system metrics, the scenario pipeline frames → brainstorms → quantifies → synthesizes → tracks → assesses on future events. Both are PDCA cycles; both have convergence criteria (the scenario pipeline's convergence is the Brier score and calibration curve); both escalate when they cannot self-correct (the scenario pipeline's `on_not_reached: escalate` is the same mechanism as the Regulation's `Escalate` action). The integration of three frameworks (Schwartz, Tetlock, Chermack) is itself a dragonfly-eye synthesis — each framework provides a different perspective, and the pipeline integrates them into a unified process that is stronger than any alone.

The conversational design of `scenario_frame` is noteworthy — it applies hKask's own improv and kata-coaching skills to the problem of eliciting decision-relevant information from users. This is the system consuming its own thread: the improv postures (Plussing, Yes And, Yes But) and the kata coaching posture (coach, not interviewer) are skills defined in `.agents/skills/`, and the `scenario_frame` tool operationalizes them in its 7-turn protocol. The behavioral psychology layer (foot-in-the-door, curiosity gap, loss aversion, social proof, peak-end, IKEA effect) grounds the conversational design in established cognitive science rather than ad-hoc intuition.

---

## 2. Nu-Event Semantics

### Statement

A ν-event (nu-event) is a thin domain event — a timestamped, attributed, namespaced observation that enters the cybernetic nervous system. It is the atomic unit of observability in hKask. A ν-event is an **assertion**, not a trace. It says "at time T, observer O witnessed fact F in domain D during phase P." It is persisted, queryable, and replayable. It feeds the Regulation homeostatic loop. This design exists because the Regulation needs structured observations, not raw log lines — the event carries who, when, what, which phase, and what was the regulatory outcome, all in a typed, queryable format.

### Evidence

The `RegulationRecord` struct at `crates/hkask-types/src/event.rs` carries:

- `id: EventID` — unique identifier
- `timestamp: DateTime<Utc>` — when the event occurred
- `observer_webid: WebID` — who observed it (the agent or system component)
- `span: Span` — a (namespace, path) pair identifying where in the system
- `phase: CyclePhase` — which phase of the cybernetic cycle (Sense, Compute, Compare, Act, Verify)
- `observation: Value` — arbitrary JSON payload describing what was observed
- `regulation: Option<Value>` — optional regulatory metadata
- `outcome: Option<Value>` — optional outcome data
- `recursion_depth: u8` — for nested/recursive operations
- `parent_event: Option<EventID>` — causal chain link
- `visibility: String` — `"private"` by default

#### ObservableSpan vs RegulationRecord

This distinction is crucial. `ObservableSpan` (at `crates/hkask-types/src/observable_span.rs`) is a trait that typed span enums implement — it produces a canonical dot-separated namespace string like `"reg.tool.web_search"`. `RegulationSpan` is the primary implementor, but the trait is designed to be domain-extensible: `InfraSpan`, `QaSpan`, and other domain span enums can implement it. A span is a **trace** — it marks where in the system something happened.

A `RegulationRecord` contains a `Span`, but it adds: who, when, what, which phase, and what was the regulatory outcome. A span says "tool invoked"; a ν-event says "Agent A invoked the web_search tool in the Sense phase, observing {server, tool, estimated_cost}, with no regulation applied, at recursion depth 0."

The bridging function is `SpanNamespace::from_observable()` — it takes any `impl ObservableSpan`, validates against the canonical namespace set in `CANONICAL_NAMESPACES` (262 entries at v0.32.0), and produces a validated `SpanNamespace` for `RegulationRecord` construction. This design decouples domain span definitions from namespace validation: domain crates define their spans; the event system validates.

#### The Emission Contract

The emission contract has three participants:

- **Emitter** — Any system component that creates a `RegulationRecord`. `McpRuntime::invoke()` is the canonical emitter for tool invocations; `CyberneticsLoop` emits regulation spans; the Curator emits curation spans. The emitter constructs the event with `RegulationRecord::new(observer_webid, span, phase, observation, recursion_depth)`, optionally chaining `.with_outcome()`, `.with_regulation()`, `.with_parent()`, and `.with_visibility()`.

- **Sink** — `RegulationSink` is the persistence trait. It has a single method: `fn persist(&self, event: &RegulationRecord) -> Result<(), InfrastructureError>`. The production implementation is `RegulationArchive` in `hkask-storage`. The sink is the durable boundary — once persisted, the event is available for Regulation sensing, Curator review, and forensic audit.

- **Observer** — The Regulation itself. `CurationLoop::sense()` reads algedonic-significant events from the store using cursor-based review. `CyberneticsLoop::sense()` reads via sensor providers (`Sensor` trait). Events are also replayed with decay weighting via `RegulationArchive::replay_weighted()`.

#### Regulation Span Namespaces

The `RegulationSpan` enum at `crates/hkask-types/src/regulation.rs` defines the core span identifiers:

| Variant              | Namespace              | Purpose                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| -------------------- | ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `Tool { subsystem }` | `reg.tool.{subsystem}` | MCP subsystems for the 10 on-disk servers (companies, corpus, curator, kata-kanban, portfolio, prediction-markets, research, scenarios, swarm, training) plus legacy `ToolSubsystem` variants (`communication`, `filesystem`, `memory`, `registry`, `wallet`, `web_search`) retained in the enum for span-name stability. The deleted `communication`, `filesystem`, `memory`, `skill`, and `regulation` MCP servers no longer emit spans. `portfolio`, `prediction-markets`, `scenarios`, and `swarm` route through `ToolSubsystem::Other` (no dedicated variant). |
| `Inference`          | `reg.inference`        | LLM request/response                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `Curation`           | `reg.curation`         | Registry sync, pod sync, directive issuance                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `SelfHeal`           | `reg.heal`             | Self-healing operations                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `MemoryEncode`       | `reg.memory.encode`    | Memory encoding events                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |

The `SpanKind` enum at `event.rs` provides typed construction for common spans, eliminating string typos: `ToolInvoked`, `ToolCompleted`, `ToolError`, `CurationDirectiveAcknowledged`, `CurationEscalation`, `VarietyAlgedonicAlert`, `DepositCredited`, and the v0.31.0 regulation spans (`ImpactVerified`, `ActionSubstituted`, `ActionBlocked`, `RegulatoryPlateauDetected`, `LoopMetricsTelemetry`).

Beyond `RegulationSpan`, the `CANONICAL_NAMESPACES` array registers 262 namespace strings spanning architecture seams, chat, CI, classification, condenser, consent, consolidation, contracts, curation, cybernetics, deploy, guard, healing, inference, kata, MCP media, memory, multi-agent, platform metrics (11 spans for PaaP/DORA/SPACE/Loyalty), QA, regulation, semantic, skills, SLOs, sovereignty, specs, storage, tools, variety, wallet, well, pipeline, supply chain, runtime posture, attack taxonomy, LoRA training, template, and training providers.

#### How ν-Events Feed the Regulation Homeostatic Loop

The Regulation loop is sense → compare → compute → act → verify. ν-events enter at sense — they are the afferent signals. `CyberneticsLoop::sense()` reads via pluggable `Sensor` implementations: `EnergyBudgetSensor`, `VarietySensor`. Each sensor queries the ν-event store for relevant events and produces `Signal` values with metrics and set-points.

In the compare phase, signals are measured against set-points to produce `Deviation` values with direction (`AboveSetPoint` / `BelowSetPoint`). In compute, deviations map to `RegulatoryAction` with action types like `Calibrate`, `Escalate`, `Throttle`, `Notify`. In act, actions are executed. In verify_impact, the `ImpactReport` records whether actions were effective.

The `parent_event` field creates causal chains: the `reg.tool.completed` event has `parent_event` set to the `reg.tool.invoked` event's ID. This enables causality tracing through the event graph.

#### WeightedEvent and Decay

Events do not persist at full weight forever. `WeightedEvent` at `crates/hkask-types/src/ports/regulation.rs` pairs a `RegulationRecord` with a `weight: f64`. `DecayConfig` (same file) defines per-category exponential decay constants: cybernetics has a 5-minute half-life, curation 15 minutes, inference 2 minutes, episodic 10 minutes. Events below `weight_threshold` (default 0.001) are not replayed. This implements episodic memory — recent events matter more than ancient ones — and prevents the Regulation from drowning in historical noise.

The `RegulationArchive::replay_weighted()` method provides time-decayed event replay, enabling the Regulation to reconstruct system state from recent history without loading the entire event store. This is the computational expression of the least-action principle applied to observability: only the computationally cheapest (most recent, most salient) events factor into regulation decisions.

### Diagram

```mermaid
flowchart TD
    EMITTER["Emitter\n(McpRuntime::invoke, CyberneticsLoop,\nCurator)"]
    EVENT["RegulationRecord\nid, timestamp, observer_webid,\nspan, phase, observation,\nregulation, outcome, parent_event"]
    SINK["RegulationSink\npersist() trait"]
    STORE["RegulationArchive\n(hkask-storage)"]
    SENSORS["Sensor\n(EnergyBudget, Variety,\nToolReliability, WalletKeyHealth)"]
    Regulation["CyberneticsLoop\nsense → compare → compute → act → verify"]
    CURATOR["CurationLoop\ncursor-based review"]

    EMITTER -->|"RegulationRecord::new()"| EVENT
    EVENT --> SINK
    SINK --> STORE
    STORE -->|"query"| SENSORS
    SENSORS --> Regulation
    STORE -->|"algedonic events"| CURATOR
    STORE -->|"replay_weighted()"| Regulation
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-COG-003
verified_date: 2026-07-29
verified_against: crates/hkask-types/src/event.rs (RegulationRecord, Span, SpanNamespace, CANONICAL_NAMESPACES — 262 entries), crates/hkask-types/src/regulation.rs (RegulationSpan enum), crates/hkask-types/src/observable_span.rs (ObservableSpan trait), crates/hkask-regulation/src/cybernetics_loop.rs (CyberneticsLoop::sense, Sensor implementations)
status: VERIFIED (v3 — corrected CANONICAL_NAMESPACES count to 262 per 2026-08-04 audit; fixed WeightedEvent/DecayConfig file path to ports/regulation.rs; removed stale hkask-memory/src/lib.rs reference)
-->

### Implications

The ν-event design is the Good Regulator theorem applied to observability: the Regulation's internal model of the system is built from ν-events, and the model's fidelity depends on the events' structure. A raw log line ("tool invoked at 14:32") is a poor model — it lacks who, what phase, what outcome, and what regulation was applied. A ν-event is a rich model — it carries all of these in a typed, queryable, replayable format. The 262-entry `CANONICAL_NAMESPACES` registry ensures that every namespace is validated against a known set — a typo in a span namespace is caught at construction time, not discovered during debugging. The decay-weighted replay is the least-action principle in action: the Regulation does not need to load the entire event store to regulate the system; it needs only the recent, salient events. This is what makes cybernetic regulation computationally feasible — the regulator's model is bounded by decay, not by the full history of the system.

---

## 3. Companies MCP Server — Investment Research

### Statement

The `hkask-mcp-companies` server provides 54 tools for retrieving company data, calculating valuations, researching claims, maintaining durable forecast feedback, and managing a local investment ledger. It is the operational toolset that connects the scenario forecasting pipeline to real market data — the forecasting pipeline produces probabilities; the companies server provides the financial data that grounds those probabilities in reality.

### Evidence

The server requires both market-data credentials, configured through zed-kask's `CredentialsProvider` (D9) or the kask settings page. Research keys are optional:

```bash
# Via zed-kask credentials (preferred — D9 keyring crate adapter):
#   add HKASK_FMP_API_KEY, HKASK_EODHD_API_KEY, and any optional
#   HKASK_EXA_API_KEY / HKASK_TAVILY_API_KEY / HKASK_BRAVE_API_KEY
#   entries under the kask namespace in the editor's credentials store.
#
# The companies MCP server reads these at in-process startup via the
# keyring crate adapter; it does not spawn a separate process.
```

The `HKASK_FMP_API_KEY` and `HKASK_EODHD_API_KEY` values are required for financial-data calls; the research provider keys enable `research_search`.

#### Retrieve Company Data

Use an exchange-qualified symbol when the listing is not a plain US-style ticker:

```text
company_profile AAPL
stock_quote MSFT
income_statement VOD.L
symbol_search "Tesla"
```

Eligible financial-data calls prefer FMP for plain symbols and EODHD for exchange-qualified symbols, then fall back after a provider failure. `company_screener` is FMP-only. `research_search` uses optional Exa, Tavily, and Brave providers instead of the financial-data route.

#### Run a Valuation

A DCF valuation builds a history-calibrated, two-stage projection and persists an owner-scoped `forecast_id`:

```text
dcf_valuation AAPL
reverse_dcf AAPL
sensitivity_analysis AAPL
monte_carlo_dcf AAPL
```

The active DCF model uses a Gordon-growth terminal value and subtracts net debt from enterprise value. It does not use exit multiples, a separate SG&A line, or other non-operating assets. The MCP boundary rejects non-finite values, out-of-range assumptions, invalid horizons, and terminal growth at or above the discount rate. `scenario_analysis` executes a fixed revenue-growth × gross-margin matrix.

#### Record a Forecast Outcome

`forecast_record` requires forecast and outcome values, not a free-text actuals summary:

```json
{
  "symbol": "AAPL",
  "forecast_date": "2025-01-01",
  "horizon": "1yr",
  "forecast_multiple": 30.0,
  "forecast_price_change": 0.1,
  "outcome_date": "2026-01-01",
  "actual_multiple": 28.0,
  "actual_price_change": 0.03,
  "forecast_id": "the-id-returned-by-dcf-valuation"
}
```

The identifier persists across restarts. Use `forecast_get` to retrieve one record and outcomes, or `forecast_list AAPL` to review the owner's history. Pass `revision_of` to `dcf_valuation` or `calibrate_forecast` to create a same-symbol revision linked to its predecessor.

#### Manage a Portfolio Ledger

Import transactions as CSV or JSON, then query portfolio analysis or add research records:

```text
ledger_import my_portfolio csv "type,date,symbol,quantity,price,commission,amount\nbuy,2024-01-15,AAPL,10,150,1,"
portfolio_returns my_portfolio 2024-01-01 2024-12-31
portfolio_attribution my_portfolio 2024-01-01 2024-12-31
note_add my_portfolio AAPL 2024-06-15 "Earnings review" "Raised guidance" ["earnings"]
```

Portfolio state is stored locally under `~/.config/hkask/portfolios/<sanitized-webid>/`, so the authenticated MCP `WebID` determines each caller's database and attachment namespace. Imports are limited to 5 MiB and 10,000 transactions; attachments are limited to 10 MiB encoded and 6 MiB decoded.

The former shared `portfolios/master.db` is not auto-migrated because it has no reliable owner identity. Export legacy data from a trusted single-principal deployment and import it into the correct owner-scoped server.

#### Ontology Annotations

Derived outputs may provide a `fibo` object with compact FIBO identifiers. Raw provider payloads are not field-mapped, and the server does not emit a JSON-LD context or Dublin Core/PKO mapping. Treat these identifiers as application metadata, not self-resolving semantic-web URIs. This is the dual-axis ontology's domain supplement layer in action — FIBO supplements the companies server's state axis where DC+BIBO is not specific enough for financial concepts.

### Diagram

```mermaid
flowchart TD
    REQ["MCP Request\n(authenticated WebID)"]
    ROUTE{"Provider Routing"}
    FMP["FMP API\n(plain symbols)"]
    EODHD["EODHD API\n(exchange-qualified)"]
    RESEARCH["Research APIs\n(Exa, Tavily, Brave)"]
    VAL["Valuation Engine\nDCF, reverse DCF,\nsensitivity, Monte Carlo"]
    FORECAST["Forecast Store\nowner-scoped, persistent"]
    LEDGER["Portfolio Ledger\nlocal SQLite, owner-scoped"]

    REQ --> ROUTE
    ROUTE -->|"plain symbol"| FMP
    ROUTE -->|"exchange-qualified"| EODHD
    ROUTE -->|"research_search"| RESEARCH
    FMP --> VAL
    EODHD --> VAL
    VAL -->|"persist forecast_id"| FORECAST
    VAL -->|"record outcome"| FORECAST
    REQ -->|"ledger_import"| LEDGER
    LEDGER -->|"portfolio_returns,\nattribution"| REQ
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-COG-004
verified_date: 2026-08-05
verified_against: mcp-servers/hkask-mcp-companies/src/hkask_mcp_companies.rs (CompaniesServer struct, run factory), mcp-servers/hkask-mcp-companies/src/providers.rs (companies_get, emit_provider_reg), mcp-servers/hkask-mcp-companies/src/portfolio.rs (PortfolioManager), mcp-servers/hkask-mcp-companies/src/tools/ (42 tool methods across 7 tool modules)
status: VERIFIED (v3 — corrected tool count to 42 per 2026-08-05 audit)
-->

### Implications

The companies server is the bridge between the scenario forecasting pipeline and real market data. The forecasting pipeline produces probabilities for future events; the companies server provides the financial data (company profiles, income statements, valuations) that grounds those events in observable reality. The `forecast_record` tool closes the feedback loop: a forecast made today can be scored against actual outcomes tomorrow, and the Brier score from `scenario_score` can be computed using the actual multiples and price changes that `forecast_record` captures. This is the same PDCA cycle as the Regulation — Plan (forecast), Do (invest), Check (score the forecast against reality), Act (calibrate future forecasts based on Brier score feedback).

The owner-scoped portfolio storage is P1 (User Sovereignty) and P12 (Authenticated Host Mandate) applied to financial data: each user's portfolio is stored under their own WebID namespace, not in a shared database. The import limits (5 MiB, 10,000 transactions) are resource governance — bounded ingestion to protect the store. The FIBO ontology annotations are the dual-axis ontology's domain supplement layer — FIBO provides financial-specific vocabulary where DC+BIBO is not precise enough, following the same thin-bridge pattern as the core ontology crates.

---

## References

[^schwartz]: Schwartz, P. (1991). _The Art of the Long View: Planning for the Future in an Uncertain World_. Currency Doubleday.

[^tetlock]: Tetlock, P. E., & Gardner, D. (2015). _Superforecasting: The Art and Science of Prediction_. Crown.

[^chermack]: Chermack, T. J. (2011). _Scenario Planning in Organizations: How to Create, Use, and Assess Scenarios_. Berrett-Koehler.

- Brier, G. W. (1950). "Verification of forecasts expressed in terms of probability." _Monthly Weather Review_, 78(1), 1–3.
- Murphy, A. H. (1973). "A New Vector Partition of the Probability Score." _Journal of Applied Meteorology_, 12(4), 595–600.
- Cialdini, R. B. (2007). _Influence: The Psychology of Persuasion_. HarperCollins. (Foot-in-the-door, social proof)
- Loewenstein, G. (1994). "The Psychology of Curiosity: A Review and Reinterpretation." _Psychological Bulletin_, 116(1), 75–98.
- Kahneman, D. (2011). _Thinking, Fast and Slow_. Farrar, Straus and Giroux. (Loss aversion, peak-end rule)
- Norton, M. I., Mochon, D., & Ariely, D. (2012). "The IKEA effect: When labor leads to love." _Journal of Consumer Psychology_, 22(3), 453–460.
- Companies MCP Server Reference at `docs/reference/mcp-servers/README.md` (Companies MCP Server section)

---

## Embedding Architecture (Merged from EMBEDDING_ARCHITECTURE.md)

# Embedding Architecture — QA Pipeline

**Date:** 2026-07-10 | **Model:** Qwen3-Embedding-0.6B (1024-dim)

## Current State

| Component              | Uses Embeddings? | How                                 |
| ---------------------- | ---------------- | ----------------------------------- |
| `corpus_embed`         | ✅ Produces      | Embeds 20K+ chunks → EmbeddingStore |
| `corpus_salience`      | ❌ No            | Graph-centrality only               |
| `build-prompts`        | ❌ No            | Salience + concepts only            |
| `generate-qa`          | ❌ No            | Raw text → LLM                      |
| `ingest-qa`            | ✅ Produces      | Embeds QAs AFTER generation         |
| `corpus_compose`         | ✅ Produces      | Embeds corpus for style exemplar centroids |

## Gap: Embeddings Not Used in QA Generation

| Opportunity                                | Impact               | Difficulty                                |
| ------------------------------------------ | -------------------- | ----------------------------------------- |
| **Chunk dedup** (cosine >0.95)             | −15% inference cost  | Medium — needs DB access in build-prompts |
| **MMR selection** (salience + diversity)   | Fewer redundant QAs  | Medium — needs vec0 KNN queries           |
| **Semantic cross-ref** (embedding groups)  | Better synthesis QAs | Hard — O(n²) without index                |
| **QA-chunk alignment** (cosine validation) | Catch hallucinations | Easy — already have both embeddings       |

## Why Keep Qwen3-Embedding-0.6B

| Factor             | Analysis                                                                                     |
| ------------------ | -------------------------------------------------------------------------------------------- |
| MTEB retrieval     | ~60% (adequate for dedup at 0.95 threshold)                                                  |
| Upgrade cost       | Re-embed 20K chunks + 7K QAs → 4+ hours                                                      |
| Dim compatibility  | 1024 matches EMBEDDING_DIM hardcoded everywhere                                              |
| Academic consensus | Simple embeddings beat complex methods for data selection (Large-Scale Data Selection, 2025) |
| DEITA threshold    | 0.9-0.95 for Repr Filter on OpenHermes/Tulu3 pools                                           |

## Upgrade Path (when justified)

1. **Phase 1** (now): Use existing embeddings for concept coverage validation ✅
2. **Phase 2** (next): Add `--use-embeddings` flag to build-prompts for MMR selection
3. **Phase 3** (future): Evaluate bge-large-en-v1.5 vs Qwen3-Embedding-0.6B on investment lit
4. **Phase 4** (future): QA-chunk alignment validation in ingest-qa

## Ontological Anchoring

The `concepts` field in tagged_chunks.jsonl serves as a lightweight investment ontology:

- Competitive positioning: `competitive advantage`, `moat`, `barriers to entry`
- Valuation: `discounted cash flow`, `DCF`, `multiples`, `intrinsic value`
- Return analysis: `return on capital`, `ROIC`, `ROE`, `economic profit`
- Risk: `margin of safety`, `cost of capital`, `beta`, `uncertainty`
- Strategy: `capital allocation`, `reinvestment`, `growth`, `management quality`

Concept coverage is validated in the pipeline selection step — flags if critical concepts are missing from training QAs.

---

## Inlined Diagrams

The following Mermaid diagrams were inlined from the former `docs/diagrams/` directory per DOCUMENTATION_STANDARDS §1.

### Memory Pipeline — Current Architecture

The memory system is a **vector embedding + relational lookup** store — no
separate `EpisodicMemory`/`SemanticMemory` structs, no `ConsentManager`, no
`ConsolidationBridge`, no `generate_narrative` loop. Those abstractions were
removed when the standalone daemon was deleted. The episodic/semantic
distinction is carried by the `HMemOntology` blob on each `h_mem` row.

**Authoritative spec:** [Memory System Specification](../architecture/memory-system-specification.md).
**Write-side diagram:** [Memory Ingest Sequence](../diagrams/sequence-memory-ingest.md).
**Read-side diagram:** [Memory Recall Flow](../diagrams/flowchart-memory-recall.md).
**Storage schema:** [Memory Store ERD](../diagrams/erd-memory-store.md).

### Visibility and Perspective Rules (current)

| Store                       | `visibility` | `perspective`         | Who can read?              |
| --------------------------- | ------------ | --------------------- | -------------------------- |
| Episodic (user `memory.db`) | `Private`    | `Some(user_webid)`    | Only owning user           |
| Episodic (curator `curator.db`) | `Private`    | `Some(curator_webid)` | Only curator               |
| Semantic (curator `curator.db`) | `Shared`     | `Some(curator_webid)` | Curator + user recall path |

### Consolidation Rules (current)

| Scenario              | Action                     | Confidence                                        |
| --------------------- | -------------------------- | ------------------------------------------------- |
| EAV match in semantic | Bayesian combine           | `combine_confidences(existing, episodic_decayed)` |
| No EAV match          | Seed new semantic hMem     | Decayed episodic confidence                       |
| Episodic hMem expired | Soft-delete via `valid_to` | Source removed from episodic                      |

The consolidation runs on a background timer via `MemoryConsolidator`
(`kask/crates/hkask-memory/src/consolidation_service.rs`). It selects
episodic h_mems, applies Wozniak-Gorzelanczyk decay (`R(t) = exp(-t/S)`,
`S = memory_life_days` default 180), and either Bayesian-combines with an
existing semantic h_mem (EAV match) or seeds a new one. Budget enforcement
(confidence floor + max count pruning) is inside `MemoryConsolidator::consolidate`.

---

## Cross-Reference

| Reference                                                                                           | Description                                                                                                                               |
| --------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| [`MemoryStore`](kask/crates/hkask-memory/src/memory_store.rs:78)                                    | Vector + relational lookup store — `store`, `store_embedding`, recall                                                                      |
| [`MemoryConsolidator`](kask/crates/hkask-memory/src/consolidation_service.rs:45)                   | Episodic → semantic consolidation with Bayesian combine + budget pruning (renamed from `ConsolidationService`)                            |
| [`recall_dedup`](kask/crates/hkask-memory/src/recall_dedup.rs:25-70)                                | BLAKE3 EAV-hash deduplication layer (`eav_hash`, `dedup_h_mems`)                                                                          |
| [`ToolSpanGuard`](kask/crates/hkask-mcp-server/src/server/tool_span.rs:11)                         | Tool span guard for `reg.tool.*` emission (D6)                                                                                            |
| [Memory System Specification](../architecture/memory-system-specification.md)                       | The authoritative memory architecture spec                                                                                               |
| [Magna Carta P1](../architecture/core/magna-carta.md#principle-1-user-sovereignty)                  | User Sovereignty — episodic memory as sovereign first-person                                                                              |

### Classification-to-Memory Sequence

_Inlined from `docs/diagrams/sequence-classify-to-memory.md`_

# Classification-to-Memory Sequence

Full flow from source text through single-model classification, semantic
extraction, and shared memory storage.

Related: `mcp-servers/hkask-mcp-corpus/src/corpus/embed/service.rs`

```mermaid
sequenceDiagram
    participant S as Source
    participant M as Model (classifier)
    participant I as Integrator
    participant Regulation as Regulation Spans
    participant Memory as Shared Memory

    S->>M: classify_batch(texts)
    M-->>I: ClassifyResult (section_type)
    Note over I: normalize (dedup, annotate)
    I->>I: extract semantic h_mems
    I->>Regulation: reg.tool span (cost posted to ledger)
    I->>Memory: store_passage_h_mems()
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-COG-008
verified_date: 2026-08-14
verified_against: kask/mcp-servers/hkask-mcp-corpus/src/corpus/embed/service.rs (classify_and_extract, classify_batch, store_passage_h_mems)
status: VERIFIED
-->

### Classification Flow

_Inlined from `docs/diagrams/flowchart-algo-classification.md`_

# Classification Flow

How classification operates as a single-model extraction: the model's JSON
extraction is normalized (dedup, diverging fields annotated) before storage.

Related: `mcp-servers/hkask-mcp-corpus/src/corpus/embed/service.rs`

```mermaid
flowchart TD
    S([Source Text])
    P[Model classifier]
    R1[Response JSON]
    I[Normalize dedup annotate]
    ST[Store in Shared Memory]
    M[Memory]

    S --> P
    P --> R1
    R1 --> I
    I --> ST
    ST --> M

    subgraph "Single-Model Extraction"
        P
    end

    subgraph "Epistemic Integration"
        I
    end
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-COG-009
verified_date: 2026-08-14
verified_against: kask/mcp-servers/hkask-mcp-corpus/src/corpus/embed/service.rs (classify_and_extract, store_passage_h_mems)
status: VERIFIED
-->
