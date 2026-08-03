# MCP Runtime Invoke — Simplified Gate Flow

Flowchart of the `McpRuntime::invoke` tool-governance gate after the 2026-08-02 OCAP removal. The gate is a pure capability-match + gas gate — no token expiry, no signature verification, no attenuation. Tokens are minted in-process and never expire.

```mermaid
flowchart TD
    A["invoke(server, tool, args, token)"] --> B{"governance.is_some()?"}
    B -- "Yes" --> C{"token.is_valid_for(Tool, tool, Execute) or verify_capability_domain(token, tool)"}
    C -- "No" --> D["Return CapabilityDenied"]
    C -- "Yes" --> E["Estimate gas cost"]
    E --> F["Reserve gas"]
    F --> G["Call tool inner"]
    G --> H["Settle gas actual cost"]
    H --> I["Emit regulation span"]
    I --> J["Return result"]
    B -- "No" --> G
```

## What changed (2026-08-02)

- **Removed:** Token expiry check (`is_valid_for_at` with `now` parameter + `is_expired`), `new_with_expiry` token minting, `ocap.capability_expiry_seconds` manifest config, `OcapConfig` struct, `ocap:` manifest blocks (59 files)
- **Simplified:** The gate now does a pure capability-match (`is_valid_for` — triple match of resource/resource_id/action) OR `verify_capability_domain` (action hierarchy: Execute ⊇ Write ⊇ Read). No expiry branch. No signature verification (tokens are minted and consumed in-process).
- **Unchanged:** Gas reserve/settle pattern (hold-settle via `CyberneticsLoop`), regulation span emission, tool dispatch

## Related

- [hKask Capability Class Diagram](./class-hkask-capability.md) — the type system
- [Architecture Principles](../architecture/core/PRINCIPLES.md) — P4 Clear Boundaries
- [MDS](../architecture/core/MDS.md) — Trust category