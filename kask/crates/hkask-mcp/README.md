# hkask-mcp

MCP runtime and dispatch for hKask.

Core MCP (Model Context Protocol) implementation — server dispatch, tool routing, security membrane.

## Key Concepts

| Concept | Description |
|---------|-------------|
| **Dispatch** | Route tool invocations to MCP servers |
| **Security** | Layered allowlists (per-server env + swarm-card `mcp_tools` + inference-IPC `tool_allowlist`) — no per-call OCAP gate (RR-0056) |
| **Runtime** | MCP server lifecycle management |
| **Transport** | stdio + child-process transport for MCP servers |
