# hkask-mcp-skill — Skill Registry MCP Server

MCP server exposing the skill registry for *execution*: lists registered skills and runs a skill's Jinja2 template through the inference router. Skill *management* (discovery, publishing, auditing, bundle composition) lives in `hkask-services-skill`.

**Version:** v0.31.0 | **Crate:** `hkask-mcp-skill`

## Tools (3)

| Tool | Description |
|------|-------------|
| `skill_ping` | Liveness and profile info |
| `skill_list` | List available skill IDs with their descriptions |
| `skill_execute` | Execute a registered skill template with context variables. Renders the skill as a Jinja2 template and runs inference. Use `skill_list` first to discover available skill IDs. |

## Configuration

No environment variables required. Uses the hKask skill registry (`registry/`) for discovery and installation.

## Dependencies

Per `Cargo.toml` (the server intentionally does **not** depend on `hkask-services-skill`; skill execution lives in this surface, skill management lives in the service crate — see `src/lib.rs` architectural note):

- `hkask-mcp` — MCP runtime, `mcp_server!` macro, `execute_tool`, `CapabilityTier`
- `hkask-templates` — `Registry::bootstrap()`, template registry
- `hkask-ports` — `InferencePort`, `RegistryIndex` traits
- `hkask-inference` — `InferenceRouter` (production `InferencePort`)
- `hkask-types` — `LLMParameters`, `McpErrorKind`, `WebID`
- `minijinja` — Jinja2 template rendering
- `rmcp` — `#[tool]` macro, `tool_router`, `Parameters`

See [docs/reference/mcp-servers/skill-server.md](../../docs/reference/mcp-servers/skill-server.md) for the architectural reference with diagram.
