# hkask-mcp-scenarios

Scenario-planning MCP server. It turns a framed decision into candidate events, explicit probability estimates, sensitivity rankings, and descriptive forecast history. The server is a thin MCP surface over `hkask-forecast` calculations and follows the standard hKask bootstrap and tool-outcome recording path.

## Configuration

| Variable | Purpose |
|---|---|
| `HKASK_WEBID` | Optional WebID identity for Regulation attribution (falls back to anonymous). |

## Tools

Twenty-two registered tools (pinned against the live `combined_router()` by
`tool_surface_is_exactly_22_registered_tools` in the crate's tests).

### Frame and explore

| Tool | Description |
|---|---|
| `scenario_status` | Return the current scenario-server state snapshot. |
| `scenario_frame` | Start the seven-turn framing conversation. |
| `scenario_frame_document` | Convert framing answers into a typed document, persisted under the project record (id defaults to the subject). |
| `scenario_brainstorm` | Produce a four-round scenario brainstorming protocol. |
| `scenario_build` | Build an event-tree extraction scaffold from research context. |

### Quantify and update

| Tool | Description |
|---|---|
| `scenario_quantify` | Resolve event probabilities, dependency order, and sensitivity. |
| `scenario_propagate` | Update one event's prior and recompute the tree, with a per-node journal. |
| `scenario_recompute_posteriors` | Recompute posteriors under evidence, in both directions (shared engine with the graph widget). |
| `scenario_update` | Apply a one-off Bayesian update (stateless — returns the posterior). |
| `scenario_calibrate` | Estimate probability with Fermi and outside/inside views; no automatic historical correction. |
| `scenario_synthesize` | Combine perspectives with explicit equal weights (dragonfly-eye); raw historical loss is context only. |

### Resolve and learn

| Tool | Description |
|---|---|
| `scenario_declare` | Freeze the complete event set, marginal probabilities, exact `resolution_at` timestamp per event ID, and named caller-reported resolver. Identical retries retain the original declaration; changed membership or terms conflict. |
| `scenario_score` | Resolve an existing declaration, scoring its frozen marginals. Reject duplicate outcomes, changed targets/resolvers, early resolution, and conflicting replays. |
| `scenario_calibration` | Calculate a calibration curve from stored forecasts. |
| `scenario_triage` | Classify a question as well_specified, goldilocks, or needs_refinement. |
| `scenario_cross_validate` | Compare independent probability estimates. |
| `scenario_assess` | Assess a scenario project across five phases, anchored on its project record. |

### Market bridges

| Tool | Description |
|---|---|
| `scenario_from_markets_set` | Compose prediction-market records into a validated event tree. |

## Forecast eligibility and interpretation

- Declaration requires `resolution_at: {event_id: RFC3339 timestamp}` with exactly the declared IDs. Its UTC date must match the event's descriptive `deadline`. Admission requires `declared_at < resolution_at`; resolution requires `now >= resolution_at`. Legitimate subsecond and same-day horizons are allowed. A server-owned clock timestamps admission; callers cannot backdate it.
- The complete declaration is synced as one journal event before any membership is published. Automatic compaction failure may return an error after that durable admission; identical same-ID retry finishes snapshot publication, even after cutoff, without changing the original timestamp, event set or probabilities. Changed membership, cutoff, resolver or probabilities conflict. Revisions use a new `forecast_id`; `scenario_update` remains a stateless calculation.
- Scenario resolution reads stored probabilities and checks the stored admission timestamp against the cutoff. Nonmatching outcomes are listed explicitly, not scored. Legacy records without precise cutoffs (including date-only v4 declarations) remain readable and unscored; no retrospective eligibility is invented. New records use schema version 5; historical single-record journal entries remain readable alongside complete-batch journal events.
- Equity `forecast_persist` admits scoring only with a declared probability on the forecast date. The return-accuracy proposition uses a frozen 20% relative tolerance (absolute return band for a zero target), calendar-month horizon, and caller-reported return at that horizon. Resolution must occur after that horizon and match the frozen target/date. No-id `forecast_record` still reports observed gaps, unscored.
- Resolver names document source rules; caller tools do not authenticate independent evidence. Eligibility is not proof the outcome is true, probabilities are calibrated, or forecasting is useful.
- Loss uses the binary convention `mean((p-y)^2)` on eligible resolved declarations, one weight per event/version. No matched reference is declared, so no skill comparison is claimed. History counts remain distinct from eligible sample counts.
- Calibration bins retain counts, mean probabilities and observed frequencies. `signed_probability_error` is `mean(p) - frequency`, not overconfidence: complementing an event flips its sign without changing confidence. The overall value averages only bins with at least five observations; `signed_probability_error_sample_count`, `signed_probability_error_qualifying_bins` and `signed_probability_error_bin_min_count` disclose that scope. Empty bins and absent qualifying means are explicitly insufficient. Zero can reflect cancellation and is not a calibration certificate. These are computed outputs, not stored historical score fields; there is no obsolete-key alias or data migration. No universal raw-loss quality labels or automatic historical correction is deployed.

## Operational boundaries

- A `requires_consent` pipeline step is refused by `PipelineRunner` until a separate approval mechanism supplies consent. This is the P2 affirmative-consent boundary.
- Scenario calculations are explicit inputs and outputs; external research remains an input to `scenario_build`'s `context` field, rather than hidden server-side collection.
- Framing guidance does not require session-order tracking: supplied answers can go directly to `scenario_frame_document`, including after a restart or for another project. Input validation and persisted-state requirements still apply.
- Each tool outcome is recorded through the MCP tool context, supporting P9 feedback and inspection.

## Related documentation

- [`docs/architecture/core/PRINCIPLES.md`](../../docs/architecture/core/PRINCIPLES.md) — P2, P4, and P9 constraints
- Forecasting stack (Schwartz, Tetlock, and Chermack integration) — `kask/docs/reference/mcp-servers/README.md` § The Forecasting Stack
- Equity forecast loop (ported from the retired companies server) — `forecast_persist`, `forecast_record`, `forecast_list`, and `scenario_risk_measure` over caller-supplied branches
- [`docs/reference/mcp-servers/README.md`](../../docs/reference/mcp-servers/README.md) — built-in server registry
