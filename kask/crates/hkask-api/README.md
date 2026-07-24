# hkask-api

HTTP API with utoipa OpenAPI for hKask.

26 route groups — CLI/MCP/API surface equivalence (every CLI command has an API endpoint).

## Route Groups

| Group | Description |
|-------|-------------|
| `chat` | Chat session management |
| `agent`, `bots`, `pods`, `userpod` | Agent lifecycle |
| `templates`, `bundles` | Template and bundle management |
| `mcp` | MCP server management |
| `regulation`, `consolidation` | Regulation and memory |
| `sovereignty`, `auth` | Sovereignty and authentication |
| `backup`, `git` | Storage and backup |
| `spec` | Specification management |
| `wallet` | Multi-chain wallet |
| `settings`, `admin` | System configuration |
| `a2a`, `models`, `episodic`, `export`, `landing`, `curator`, `goal`, `terminal` | Additional endpoints |

Built with `axum` + `utoipa` (OpenAPI 3.1, auto-generated from `#[derive(ToSchema)]`).
