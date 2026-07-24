# hkask-mcp-scenarios

Scenario-planning MCP server. It turns a framed decision into candidate events, calibrated probabilities, sensitivity rankings, and learning signals. The server is a thin MCP surface over `hkask-forecast` calculations and follows the standard hKask bootstrap and tool-outcome recording path.

## Configuration

| Variable | Purpose |
|---|---|
| `HKASK_MCP_HOST` | Optional host identity used during MCP bootstrap |

## Tools

### Frame and explore

| Tool | Description |
|---|---|
| `scenario_status` | Return the current scenario-server state snapshot. |
| `scenario_full` | Run the complete scenario pipeline in one call. |
| `scenario_from_companies` | Convert company data into forecast events. |
| `scenario_frame` | Start the seven-turn framing conversation. |
| `scenario_frame_document` | Convert framing answers into a typed document. |
| `scenario_brainstorm` | Produce a four-round scenario brainstorming protocol. |
| `scenario_build` | Build an event-tree scaffold from research. |

### Quantify and calibrate

| Tool | Description |
|---|---|
| `scenario_research` | Extract candidate events from research text. |
| `scenario_quantify` | Resolve event probabilities and dependency order. |
| `scenario_update` | Apply a Bayesian probability update. |
| `scenario_score` | Score resolved forecasts with Brier scoring. |
| `scenario_calibrate` | Calibrate a forecast with Fermi and outside/inside views. |
| `scenario_sensitivity` | Rank events by uncertainty contribution. |
| `scenario_synthesize` | Combine independent perspectives into one forecast. |

### Learn and assess

| Tool | Description |
|---|---|
| `scenario_calibration` | Calculate a calibration curve from stored forecasts. |
| `scenario_triage` | Classify a question as clocklike, Goldilocks, or cloudlike. |
| `scenario_cross_validate` | Compare independent probability estimates. |
| `scenario_assess` | Assess a scenario project across five performance phases. |

## Operational boundaries

- A `requires_consent` pipeline step is refused by `PipelineRunner` until a separate approval mechanism supplies consent. This is the P2 affirmative-consent boundary.
- Scenario calculations are explicit inputs and outputs; external research remains an input to `scenario_research`, rather than hidden server-side collection.
- Each tool outcome is recorded through the MCP tool context, supporting P9 feedback and inspection.

## Related documentation

- [`docs/architecture/core/PRINCIPLES.md`](../../docs/architecture/core/PRINCIPLES.md) — P2, P4, and P9 constraints
- [`docs/explanation/forecasting-and-scenarios.md`](../../docs/explanation/forecasting-and-scenarios.md) — Schwartz, Tetlock, and Chermack integration
- [`docs/architecture/scenarios-companies-bridge.md`](../../docs/architecture/scenarios-companies-bridge.md) — companies-server bridge status
- [`docs/reference/mcp-servers/README.md`](../../docs/reference/mcp-servers/README.md) — built-in server registry
