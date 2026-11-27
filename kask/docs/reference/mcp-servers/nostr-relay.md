---
title: "Nostr Relay MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-11-18
version: "0.1.0"
status: "Active"
domain: "Composition"
mds_categories: [domain, composition, trust, lifecycle]
---

# Nostr Relay MCP Server Reference

**Crate:** `kask/mcp-servers/hkask-mcp-nostr-relay`
**Tools:** 4 — `nostr_relay_status`, `nostr_relay_query`, `nostr_relay_sync`, `nostr_relay_log` (pinned by `tool_surface_is_exactly_4_registered_tools` at `kask/mcp-servers/hkask-mcp-nostr-relay/src/hkask_mcp_nostr_relay.rs:36-45@tool_surface_tests` and wire-matched to `hkask_types::nostr_wire::RELAY_TOOL_NAMES`).
**Auto-start:** Yes by default with the built-in fleet (`id: "nostr-relay"`, `binary: "hkask-mcp-nostr-relay"`, `kask/crates/kask_bridge/src/mcp_servers/catalog.rs:363-383@BUILT_IN_MCP_SERVERS`). Disabled by default on privacy-marked threads via `DEFAULT_PRIVATE_THREAD_DISABLED_SERVERS` (`crates/settings_content/src/settings_content.rs:2230-2236`).

The Nostr relay server (D105) runs an in-process loopback WebSocket relay (`127.0.0.1:<port>`) backed by an unencrypted LMDB event archive under `{data_dir}/mcp/nostr-relay/events`, admitting only verified public events authored by public keys on the operator's allowlist.[^nip-01][^nip-11] It holds no Nostr key, receives no credentials (`credentials: &[]`), and never signs or pushes events out to public relays.

## Architecture & admission perimeter

Three internal modules enforce the loopback archive boundary (`kask/mcp-servers/hkask-mcp-nostr-relay/src/hkask_mcp_nostr_relay.rs:9-27`):

1. **HTTP/WebSocket listener (`accept.rs`):** Binds `127.0.0.1` (`port = 0` selects an OS-assigned port unless `kask.nostr.relay.port` fixes one), serves NIP-11 relay metadata (`SUPPORTED_NIPS = [1, 9, 11, 62]`, `kask/mcp-servers/hkask-mcp-nostr-relay/src/server.rs:51@SUPPORTED_NIPS`), and rejects browser-originated requests carrying an `Origin` header before WebSocket upgrade (`kask/mcp-servers/hkask-mcp-nostr-relay/src/accept.rs:197@serve`).[^nip-09][^nip-62]
2. **Single admission rule (`admission.rs`):** Both WebSocket `EVENT` frames and `nostr_relay_sync` pulls pass through the same allowlist and kind gate (`allowed_authors` only; empty allowlist admits nothing; direct-message kinds refused; every decision recorded in a bounded ring buffer).
3. **Listener announcement (`AnnouncementGuard`):** On bind, writes `mcp/nostr-relay/listener.json` under `HKASK_DATA_DIR` so `hkask-mcp-nostr-client` discovers `ws://127.0.0.1:<port>` without manual configuration, removing the file on shutdown (`kask/mcp-servers/hkask-mcp-nostr-relay/src/server.rs:129@AnnouncementGuard`, `:598@run`).

**Known access limit:** The loopback listener does not authenticate local OS processes — any same-machine process can connect and read stored events, so the archive stores public events only (disclosed in `ACCESS_LIMITS` at `kask/mcp-servers/hkask-mcp-nostr-relay/src/server.rs:37@ACCESS_LIMITS` and in NIP-11).

## Configuration

Startup reads operator policy once via `Config::from_env` (`kask/mcp-servers/hkask-mcp-nostr-relay/src/config.rs:15-25@Config`), unconditionally emitted from `kask.nostr.relay` by `emit_nostr_relay_env`:

| Env var | Source setting | Default / validation |
|---------|----------------|----------------------|
| `HKASK_DATA_DIR` | `kask.data_dir` | Resolves `mcp/nostr-relay/events` and `mcp/nostr-relay/listener.json` |
| `HKASK_NOSTR_RELAY_PORT` | `kask.nostr.relay.port` | `0` (ephemeral loopback port) |
| `HKASK_NOSTR_RELAY_AUTHORS` | `kask.nostr.relay.allowed_authors` | Comma-joined `npub` or 64-hex public keys; empty admits zero events |
| `HKASK_NOSTR_RELAY_SYNC_RELAYS` | `kask.nostr.relay.sync_relays` | Comma-joined public `wss://` relays (`validate_sync_relay` rejects `ws://`, `localhost`, and private/loopback/link-local IP literals, surfacing skipped entries in `configuration_warnings`, `kask/mcp-servers/hkask-mcp-nostr-relay/src/config.rs:98-117@validate_sync_relay`) |

## Tool catalog (4 tools)

All tool handlers live on `NostrRelayServer` in `kask/mcp-servers/hkask-mcp-nostr-relay/src/server.rs`:

| Tool | Handler | PKO / DC-BIBO | Contract |
|------|---------|---------------|----------|
| `nostr_relay_status` | `kask/mcp-servers/hkask-mcp-nostr-relay/src/server.rs:334@NostrRelayServer::nostr_relay_status` | `pko:ProcedureExecution` / `bibo:Report` | Read-only status: listener state (`listening` or `down`), bound loopback URL, archived event count, allowlisted author count, configured sync relays, `configuration_warnings`, and `ACCESS_LIMITS`. |
| `nostr_relay_query` | `kask/mcp-servers/hkask-mcp-nostr-relay/src/server.rs:382@NostrRelayServer::nostr_relay_query` | `pko:ProcedureExecution` / `dcterms:Dataset` | Queries local LMDB archive newest-first by optional `authors`, `kinds`, `references` (`e` tags), `search` text, `since`/`until`, and `limit` (`DEFAULT_QUERY_EVENTS = 20`, `MAX_QUERY_EVENTS = 100`). |
| `nostr_relay_sync` | `kask/mcp-servers/hkask-mcp-nostr-relay/src/server.rs:445@NostrRelayServer::nostr_relay_sync` | `pko:StepExecution` / `dcterms:Dataset` | Pull-only sync copying allowlisted authors' events from configured public `sync_relays` (or a named subset) into the local archive (`CONNECT_TIMEOUT = 10 s`, `FETCH_TIMEOUT = 30 s`, `MAX_EVENTS_PER_SYNC = 5,000`, `kask/mcp-servers/hkask-mcp-nostr-relay/src/sync.rs:20-24`). |
| `nostr_relay_log` | `kask/mcp-servers/hkask-mcp-nostr-relay/src/server.rs:484@NostrRelayServer::nostr_relay_log` | `pko:ProcedureExecution` / `bibo:Report` | Reads recent WebSocket and sync admission/refusal decisions newest-first (`DEFAULT_LOG_DECISIONS = 50`, max `200`). |

Dual-axis P5.4 ontology annotation is wired in `kask/mcp-servers/hkask-mcp-nostr-relay/src/ontology.rs:18-47`.

## Cross-links

- [MCP Server Registry](README.md) — fleet catalog
- [Nostr Client MCP Server Reference](nostr-client.md) — reads from and publishes to this local relay
- [Nostr Blossom MCP Server Reference](nostr-blossom.md) — companion loopback Blossom media sidecar
- [Kask Settings Reference](../kask-settings.md#nostr-kasknostrsettings) — `kask.nostr.relay` settings
- [Curator Nostr Relay Design](../../research/curator-nostr-relay-design-2026-11-11.md) — D105 design record

## Footnotes

[^nip-01]: Nostr Implementation Possibilities. *NIP-01: Basic protocol flow description*. https://github.com/nostr-protocol/nips/blob/master/01.md
[^nip-09]: Nostr Implementation Possibilities. *NIP-09: Event Deletion Request*. https://github.com/nostr-protocol/nips/blob/master/09.md
[^nip-11]: Nostr Implementation Possibilities. *NIP-11: Relay Information Document*. https://github.com/nostr-protocol/nips/blob/master/11.md
[^nip-62]: Nostr Implementation Possibilities. *NIP-62: Request to Vanish*. https://github.com/nostr-protocol/nips/blob/master/62.md
