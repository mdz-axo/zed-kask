# hkask-mcp-scenarios

Scenario-planning MCP server. It turns a framed decision into candidate events, calibrated probabilities, sensitivity rankings, and learning signals. The server is a thin MCP surface over `hkask-forecast` calculations and follows the standard hKask bootstrap and tool-outcome recording path.

## Configuration

| Variable | Purpose |
|---|---|
| `HKASK_WEBID` | Optional WebID identity for Regulation attribution (falls back to anonymous). |

## Tools

Nineteen registered tools (pinned against the live `scenario_router()` by
`tool_surface_is_exactly_19_registered_tools` in the crate's tests).

### Frame and explore

| Tool | Description |
|---|---|
| `scenario_status` | Return the current scenario-server state snapshot. |
| `scenario_full` | Run the Tetlock core batch (triage, quantify, sensitivity, calibrate, synthesize, assess) in one call. |
| `scenario_frame` | Start the seven-turn framing conversation. |
| `scenario_frame_document` | Convert framing answers into a typed document, persisted under the project record (id defaults to the subject). |
| `scenario_brainstorm` | Produce a four-round scenario brainstorming protocol. |
| `scenario_build` | Build an event-tree extraction scaffold from research context. |

### Quantify and update

| Tool | Description |
|---|---|
| `scenario_quantify` | Resolve event probabilities, dependency order, and sensitivity. |
| `scenario_propagate` | Update one event's prior and recompute the tree, with a per-node journal. |
| `scenario_update` | Apply a one-off Bayesian update (stateless — returns the posterior). |
| `scenario_calibrate` | Calibrate a forecast with Fermi and outside/inside views. |
| `scenario_synthesize` | Combine independent perspectives into one forecast (dragonfly-eye). |

### Resolve and learn

| Tool | Description |
|---|---|
| `scenario_score` | Score resolved forecasts with Brier scoring (the only journal writer). |
| `scenario_calibration` | Calculate a calibration curve from stored forecasts. |
| `scenario_triage` | Classify a question as well_specified, goldilocks, or needs_refinement. |
| `scenario_cross_validate` | Compare independent probability estimates. |
| `scenario_assess` | Assess a scenario project across five phases, anchored on its project record. |

### Market bridges

| Tool | Description |
|---|---|
| `scenario_from_markets_set` | Compose prediction-market records into a validated event tree. |
| `scenario_from_cmp_indices` | Compose CMP indices into a validated event tree. |
| `contract_price_coherence` | Compare a tree-implied probability with an observed contract price. |

## Operational boundaries

- A `requires_consent` pipeline step is refused by `PipelineRunner` until a separate approval mechanism supplies consent. This is the P2 affirmative-consent boundary.
- Scenario calculations are explicit inputs and outputs; external research remains an input to `scenario_build`'s `context` field, rather than hidden server-side collection.
- Each tool outcome is recorded through the MCP tool context, supporting P9 feedback and inspection.

## Related documentation

- [`docs/architecture/core/PRINCIPLES.md`](../../docs/architecture/core/PRINCIPLES.md) — P2, P4, and P9 constraints
- Forecasting stack (Schwartz, Tetlock, and Chermack integration) — `kask/docs/reference/mcp-servers/README.md` § The Forecasting Stack
- Companies-server bridge status — `kask/docs/reference/mcp-servers/companies.md` § Scenarios ↔ Companies Bridge
- [`docs/reference/mcp-servers/README.md`](../../docs/reference/mcp-servers/README.md) — built-in server registry
