# hkask-mcp-curator — Curator Daemon MCP Server

MCP server exposing Curator daemon tools: algedonic monitoring, escalation management, Regulation health, metacognition, and semantic memory recall.

**Version:** v0.31.0 | **Crate:** `hkask-mcp-curator`

## Tools (11)

| Tool | Description |
|------|-------------|
| `curator_health` | Liveness check |
| `curator_list_escalations` | List all pending escalations requiring review |
| `curator_resolve_escalation` | Resolve an escalation by ID |
| `curator_dismiss_escalation` | Dismiss an escalation as not actionable |
| `curator_metacognition` | Run metacognition cycle — requires live daemon for Regulation data |
| `curator_reg_status` | Live Regulation status — variety per domain |
| `curator_bot_health` | Per-bot health — gas consumption vs. energy budget |
| `curator_spec_drift` | Check specs for drift from registered verbs |
| `curator_semantic_search` | Query the Curator's semantic memory by entity name |
| `curator_episodic_recall` | Recall the Curator's episodic and semantic memory about an entity |
| `curator_algedonic_log` | Read algedonic event log for a time window |

## Configuration

No environment variables required. Connects to the hKask daemon via `McpRuntime` when running in server mode.

## Dependencies

- `hkask-mcp` — MCP runtime and dispatch
- `hkask-services-context` — AgentService context
- `hkask-regulation` — Cybernetic Nervous System spans
