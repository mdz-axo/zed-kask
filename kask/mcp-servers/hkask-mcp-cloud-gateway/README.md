# hkask-mcp-cloud-gateway — Cloud Gateway Transport Adapter

mTLS + DelegationToken reverse proxy for remote MCP server access to the hKask daemon. Enables cloud deployments, IDE integrations, and multi-machine setups where MCP servers run outside the local machine.

**Version:** v0.31.0 | **Crate:** `hkask-mcp-cloud-gateway`

## Security Model (Three Layers)

| Layer | Mechanism | Description |
|-------|-----------|-------------|
| 1. Transport | mTLS 1.3 | Client and server present X.509 certificates. Client cert CN maps to userpod WebID |
| 2. Authorization | Ed25519 DelegationToken | Per-request signed token. `delegated_to` must match mTLS CN |
| 3. Capability | Per-tool gating | `token.resource_id` must match the requested tool name |

## Architecture

```
Remote MCP Client ──[mTLS]──▶ Gateway ──[Unix socket]──▶ DaemonHandler
                                   │
                                   ├── Verify client cert CN
                                   ├── Verify DelegationToken signature
                                   ├── Verify resource_id matches tool
                                   └── Forward to DaemonHandler::dispatch_tool
```

## Configuration

| Variable | Required | Description |
|----------|----------|-------------|
| `HKASK_GATEWAY_BIND` | No | Address to bind (default: `0.0.0.0:9443`) |
| `HKASK_GATEWAY_SERVER_CERT` | Yes | Path to server TLS certificate (PEM) |
| `HKASK_GATEWAY_SERVER_KEY` | Yes | Path to server private key (PEM) |
| `HKASK_GATEWAY_CLIENT_CA` | Yes | Path to client CA certificate (PEM) |

## Identity Binding

mTLS certificates carry human-readable Common Names (e.g., "alice"). hKask WebIDs are UUIDs derived from persona bytes via v5 UUID. The gateway derives the expected WebID from the cert CN and compares it against the token's `delegated_to` field.

## Token Provisioning

DelegationTokens are issued via `kask token issue` and must be regenerated periodically. Expired tokens produce `AuthError::Expired`. Mismatched identity produces `AuthError::IdentityMismatch`. Tool mismatch produces `AuthError::ToolMismatch`.

## Dependencies

- `hkask-mcp` — Daemon client and dispatch
- `hkask-capability` — DelegationToken verification
- `hkask-types` — WebID derivation
- `rustls` / `tokio-rustls` — mTLS 1.3 transport
