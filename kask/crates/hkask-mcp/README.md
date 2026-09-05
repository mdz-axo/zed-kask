# hkask-mcp

MCP runtime and dispatch for hKask.

Core MCP (Model Context Protocol) implementation — server dispatch, tool routing, security membrane.

## Lifecycle guarantees

Start, publication, and stop are coordinated by a desired launch generation.
Stop/replacement cancels that generation; an older discovery result cannot
publish tools or replace the new configuration. Discovery failures and cancelled
starts retain a cleanup guard, including before a child is registered.

Reconnect uses the configured Tokio runtime's worker pool and does not park the
calling executor. `HKASK_MCP_STARTUP_TIMEOUT_SECS` bounds each handshake and
`tools/list` phase (default 60 seconds, the existing health-check interval).
Zero gives an immediate deadline, not an unbounded startup. Cancellation begins
rmcp's shutdown; its three-second graceful-close period precedes forced killing.
A delivered tool call with an unknown outcome is still never automatically replayed.

## Key Concepts

| Concept | Description |
|---------|-------------|
| **Dispatch** | Route tool invocations to MCP servers |
| **Security** | Layered allowlists (per-server env + swarm-card `mcp_tools` + inference-IPC `tool_allowlist`) — no per-call OCAP gate (RR-0056) |
| **Runtime** | MCP server lifecycle management |
| **Transport** | stdio + child-process transport for MCP servers |
