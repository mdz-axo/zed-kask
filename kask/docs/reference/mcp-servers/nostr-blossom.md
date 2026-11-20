---
title: "Nostr Blossom MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-11-18
version: "0.1.0"
status: "Active"
domain: "Composition"
mds_categories: [domain, composition, trust, lifecycle]
---

# Nostr Blossom MCP Server Reference

**Crate:** `kask/mcp-servers/hkask-mcp-nostr-blossom`
**Tools:** 3 — `nostr_blossom_status`, `nostr_blossom_list`, `nostr_blossom_log` (pinned by `tool_surface_is_exactly_3_registered_tools` at `kask/mcp-servers/hkask-mcp-nostr-blossom/src/hkask_mcp_nostr_blossom.rs:94-103@tool_surface_tests` and wire-matched to `hkask_types::nostr_wire::BLOSSOM_TOOL_NAMES`).
**Auto-start:** Yes by default with the built-in fleet (`id: "nostr-blossom"`, `binary: "hkask-mcp-nostr-blossom"`, `kask/crates/kask_bridge/src/mcp_servers/catalog.rs:411-424@BUILT_IN_MCP_SERVERS`). Disabled by default on privacy-marked threads via `DEFAULT_PRIVATE_THREAD_DISABLED_SERVERS` (`crates/settings_content/src/settings_content.rs:2230-2236`, pinned by `nostr_blossom_is_disabled_on_private_threads_by_default` at `kask/crates/kask_bridge/src/mcp_servers/tests.rs:118`).

The Nostr Blossom server (D109) is a local content-addressed media sidecar implementing Blossom `BUD-01` (`GET`/`HEAD /<sha256>[.<ext>]`), `BUD-02` (`PUT /upload`, `DELETE /<sha256>`, `GET /list/<pubkey>`), `BUD-06` (`HEAD /upload` preflight), and `BUD-11` (`kind: 24242` Schnorr-signed Nostr authorization events with `CLOCK_SKEW_SECS = 60` at `kask/mcp-servers/hkask-mcp-nostr-blossom/src/auth.rs:11@CLOCK_SKEW_SECS`).[^blossom-spec] Its MCP tool surface is strictly read-only; blob ingestion and mirroring are driven over HTTP by `hkask-mcp-nostr-client` (`nostr_blossom_archive`, `nostr_blossom_mirror`, `nostr_publish_note`) and the Nostr panel.

## Storage, HTTP perimeter & announcement

1. **Content-addressed store (`BlobStore`):** Stores verified blobs under `{data_dir}/mcp/nostr-blossom/blobs/<aa>/<sha256>` alongside a SQLite metadata index at `{data_dir}/mcp/nostr-blossom/index.db` (`kask/mcp-servers/hkask-mcp-nostr-blossom/src/storage.rs:74@BlobStore`, `kask/mcp-servers/hkask-mcp-nostr-blossom/src/config.rs:51-59@default_store_dir`). Uploads stream to disk while hashing so digests are verified before commit; `DELETE /<sha256>` is reference-counted per owning author (`DeleteOutcome` removes bytes from disk only when the last owner deletes the blob).
2. **Loopback HTTP listener & `Host` check (`http.rs`):** Binds `127.0.0.1:<port>` (`kask/mcp-servers/hkask-mcp-nostr-blossom/src/http.rs:901@bind`, `:919@serve`), enforces browser/DNS-rebinding `Host` header checks (accepting loopback or the configured `public_host`), and requires allowlisted `kind: 24242` Nostr signatures for uploads and deletes.
3. **Listener announcement (`AnnouncementGuard`):** Writes `mcp/nostr-blossom/listener.json` under `HKASK_DATA_DIR` on bind so `hkask-mcp-nostr-client` discovers the active loopback endpoint (`kask/mcp-servers/hkask-mcp-nostr-blossom/src/hkask_mcp_nostr_blossom.rs:30-66@start_with`, `kask/mcp-servers/hkask-mcp-nostr-blossom/src/server.rs:145@AnnouncementGuard`).
4. **Zero credentials:** Receives `credentials: &[]` — verifies `kind: 24242` signatures and holds no secret key or SQLCipher passphrase (`index.db` indexes public Nostr blobs only).

## Configuration

Startup reads five environment variables via `Config::from_env` (`kask/mcp-servers/hkask-mcp-nostr-blossom/src/config.rs:22-35@Config`, `:115-183`), emitted by `emit_nostr_blossom_env` (`kask/crates/kask_bridge/src/mcp_env.rs:259@emit_nostr_blossom_env`):

| Env var | Source setting | Default / validation |
|---------|----------------|----------------------|
| `HKASK_DATA_DIR` | `kask.data_dir` | Resolves `mcp/nostr-blossom/{index.db,blobs,listener.json}` |
| `HKASK_NOSTR_BLOSSOM_PORT` | `kask.nostr.blossom.port` | `0` (OS-assigned loopback port) |
| `HKASK_NOSTR_BLOSSOM_MAX_BYTES` | `kask.nostr.blossom.max_bytes` | `10,737,418,240` (`DEFAULT_BLOSSOM_MAX_BYTES` = 10 GiB; `0` disables quota, `kask/mcp-servers/hkask-mcp-nostr-blossom/src/config.rs:10@DEFAULT_BLOSSOM_MAX_BYTES`) |
| `HKASK_NOSTR_BLOSSOM_PUBLIC_URL` | `kask.nostr.blossom.public_base_url` | `""` (when set, `validate_public_base_url` requires `https://` on a non-loopback host with no credentials, query, or fragment, `kask/mcp-servers/hkask-mcp-nostr-blossom/src/config.rs:75-112@validate_public_base_url`) |
| `HKASK_NOSTR_BLOSSOM_AUTHORS` | `kask.nostr.relay.allowed_authors` (+ `kask.nostr.npub`) | Comma-joined `npub` or 64-hex public keys permitted to upload/delete; malformed entries are recorded in `refused` without echoing raw text |

## Tool catalog (3 tools)

All tool handlers live on `NostrBlossomServer` in `kask/mcp-servers/hkask-mcp-nostr-blossom/src/server.rs`:

| Tool | Handler | PKO / DC-BIBO | Contract |
|------|---------|---------------|----------|
| `nostr_blossom_status` | `kask/mcp-servers/hkask-mcp-nostr-blossom/src/server.rs:262@NostrBlossomServer::nostr_blossom_status` | `pko:ProcedureExecution` / `bibo:Report` | Reports HTTP listener state/URL, blob count, disk usage vs `max_bytes` quota, optional `public_base_url`, allowlisted author count, refused config entries, and `ACCESS_LIMITS`. |
| `nostr_blossom_list` | `kask/mcp-servers/hkask-mcp-nostr-blossom/src/server.rs:315@NostrBlossomServer::nostr_blossom_list` | `pko:ProcedureExecution` / `dcterms:Dataset` | Lists stored blobs newest-first (`DEFAULT_LIST_BLOBS = 50`, `MAX_LIST_BLOBS = 200`), optionally filtered by owning author (`npub` or 64-hex). |
| `nostr_blossom_log` | `kask/mcp-servers/hkask-mcp-nostr-blossom/src/server.rs:358@NostrBlossomServer::nostr_blossom_log` | `pko:ProcedureExecution` / `bibo:Report` | Reads recent in-memory HTTP admission, upload, and delete decisions newest-first (`DEFAULT_LOG_DECISIONS = 50`, `DECISION_LOG_CAPACITY = 200`). |

Dual-axis P5.4 ontology annotation is wired in `kask/mcp-servers/hkask-mcp-nostr-blossom/src/ontology.rs:10-35`.

## Cross-links

- [MCP Server Registry](README.md) — fleet catalog
- [Nostr Client MCP Server Reference](nostr-client.md) — `nostr_blossom_archive`, `nostr_blossom_mirror`, and `nostr_publish_note` (`media_paths`)
- [Nostr Relay MCP Server Reference](nostr-relay.md) — companion loopback event relay sharing `allowed_authors`
- [Kask Settings Reference](../kask-settings.md#nostr-kasknostrsettings) — `kask.nostr.blossom` settings

## Footnotes

[^blossom-spec]: Hzrd149. *Blossom Specifications (BUD-01, BUD-02, BUD-06, BUD-11)*. https://github.com/hzrd149/blossom
