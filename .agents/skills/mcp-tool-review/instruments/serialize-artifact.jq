# The findings-artifact template's body, as jq: same keys, same order, values passed through.
{
  capability: "mcp-tool-review",
  server: .server,
  run_date: .run_date,
  reconciliation: .reconciliation,
  efficiency_metric_space: .efficiency_metric_space,
  anchor_registry: .anchor_registry,
  tools: .tools,
  findings: .findings,
  gated_pending_rulings: .gated_pending,
  citation_check: .citation_check,
  ask_first: .ask_first,
  notes: .notes
}
