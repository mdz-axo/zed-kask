# hkask-mcp

MCP runtime and dispatch for hKask.

Core MCP (Model Context Protocol) implementation — server dispatch, tool routing, security membrane.

## Key Concepts

| Concept | Description |
|---------|-------------|
| **Dispatch** | Route tool invocations to MCP servers |
| **Security** | OCAP membrane — capability-gated tool access |
| **Runtime** | MCP server lifecycle management |
| **Transport** | stdio + child-process transport for MCP servers |
