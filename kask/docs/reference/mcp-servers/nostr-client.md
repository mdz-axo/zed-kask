---
title: "Nostr Client MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-11-20
version: "0.1.0"
status: "Active"
domain: "Composition"
mds_categories: [domain, composition, trust, lifecycle]
---

# Nostr Client MCP Server Reference

**Crate:** `kask/mcp-servers/hkask-mcp-nostr-client`
**Tools:** 15 — `nostr_account`, `nostr_decode`, `nostr_fetch_events`, `nostr_thread`, `nostr_profile`, `nostr_author_relays`, `nostr_notifications`, `nostr_publish_deletion`, `nostr_publish_note`, `nostr_publish_reaction`, `nostr_publish_reply`, `nostr_prepare_zap`, `nostr_publish_nutzap`, `nostr_blossom_archive`, `nostr_blossom_mirror` (pinned by `tool_surface_is_exactly_15_registered_tools` at `kask/mcp-servers/hkask-mcp-nostr-client/src/hkask_mcp_nostr_client.rs:51-61@tool_surface_tests` and wire-matched to `hkask_types::nostr_wire::CLIENT_TOOL_NAMES`).
**Auto-start:** Yes by default with the built-in fleet (`id: "nostr-client"`, `binary: "hkask-mcp-nostr-client"`, `kask/crates/kask_bridge/src/mcp_servers/catalog.rs:384-410@BUILT_IN_MCP_SERVERS`). Disabled by default on privacy-marked threads via `DEFAULT_PRIVATE_THREAD_DISABLED_SERVERS` (`crates/settings_content/src/settings_content.rs:2230-2236`).

The Nostr client server is the agent's public-network Nostr instrument (D106): seven read-only inspection and query tools over the local relay (`hkask-mcp-nostr-relay`) and up to 8 operator-configured `wss://` seed relays, plus three step-execution tools (`nostr_publish_note`, `nostr_blossom_archive`, `nostr_blossom_mirror`) whose cryptographic signatures are produced exclusively inside the editor process over the inference IPC channel.[^nip-01][^blossom-bud02]

## Key-isolation & signing boundary

The child process receives **zero credentials** (`credentials: &[]`, `kask/crates/kask_bridge/src/mcp_servers/catalog.rs:394@BUILT_IN_MCP_SERVERS`), links `hkask-nostr-core` (public verification and NIP-19 decoding) and **never** links `hkask-nostr-signer` (`kask/mcp-servers/hkask-mcp-nostr-client/src/hkask_mcp_nostr_client.rs:7-15`). Because a Nostr secret key (`nsec`) cannot be rotated without abandoning the public identity, the key stays encrypted (NIP-49 `ncryptsec`) in the editor's keychain slots (`kask://credentials/nostr_ncryptsec` and `kask://credentials/nostr_password`, `kask/crates/kask_bridge/src/credentials.rs:131-160`).[^nip-49]

When `nostr_publish_note`, `nostr_blossom_archive`, or `nostr_blossom_mirror` requires a signature (`kind: 1` note or `kind: 24242` Blossom `BUD-11` upload authorization), the server sends a signing request over `HKASK_INFERENCE_SOCKET` (`kask/crates/kask_bridge/src/nostr_signing.rs`). The editor enforces the `host/sign_nostr_note` delegated-tool grant (`kask.mcp.delegated_tools["nostr-client"]`), checks `kask.nostr.autopublish` (default `false`), and prompts the operator in a native approval window before touching the keychain unless `autopublish` is explicitly enabled (`kask/crates/kask_bridge/src/settings.rs:366-370@KaskNostrSettings`).

## Configuration & relay perimeter

Startup reads four environment variables once via `Config::from_env` (`kask/mcp-servers/hkask-mcp-nostr-client/src/config.rs:12-46`), emitted for every settings state by `emit_nostr_client_env` so parent shell variables cannot shadow operator settings:

| Env var | Source setting | Role |
|---------|----------------|------|
| `HKASK_DATA_DIR` | `kask.data_dir` | Resolves `mcp/nostr-relay/listener.json` (`ws://127.0.0.1:<port>`) and `mcp/nostr-blossom/listener.json` (`http://127.0.0.1:<port>`) |
| `HKASK_NOSTR_CLIENT_NPUB` | `kask.nostr.npub` | Public identity (`npub1...`); secret keys (`nsec`/`ncryptsec`) are refused without echoing the value (`kask/mcp-servers/hkask-mcp-nostr-client/src/config.rs:51-67`) |
| `HKASK_NOSTR_CLIENT_SEED_RELAYS` | `kask.nostr.seed_relays` | Comma-joined public `wss://` relays (up to 8 contacted; literal loopback, private, link-local, unspecified, broadcast, or credentialed URLs are refused) |
| `HKASK_NOSTR_CLIENT_BLOSSOM_SERVERS` | `kask.nostr.blossom_servers` | Comma-joined public `https://` Blossom servers (up to 4 targets for `BUD-02` `PUT /upload`) |
| `HKASK_NOSTR_BLOSSOM_PUBLIC_URL` | `kask.nostr.blossom.public_base_url` | Optional public `https://` URL fronting the local Blossom sidecar |

Relays named inside untrusted events (NIP-65 `kind: 10002` relay lists or NIP-19 relay hints) are surfaced as data and **never contacted** (`kask/mcp-servers/hkask-mcp-nostr-client/src/server.rs:362-382`). Direct-message kinds (`4`, `13`, `14`, `1059`) are refused on every read path. Every event's ID and Schnorr signature are verified before return, and relay provenance is merged per event ID.

## Tool catalog (15 tools)

All tool handlers live on `NostrClientServer` in `kask/mcp-servers/hkask-mcp-nostr-client/src/server.rs`:

| Tool | Handler | PKO / DC-BIBO | Contract |
|------|---------|---------------|----------|
| `nostr_account` | `kask/mcp-servers/hkask-mcp-nostr-client/src/server.rs:352@NostrClientServer::nostr_account` | `pko:ProcedureExecution` / `bibo:Report` | Reports configured `npub` state (`configured`, `not_configured`, `invalid`), local relay announcement status, and 5 s WebSocket probe results across the local relay and up to 8 seed relays. |
| `nostr_decode` | `kask/mcp-servers/hkask-mcp-nostr-client/src/server.rs:407@NostrClientServer::nostr_decode` | `pko:ProcedureExecution` / `bibo:Report` | Pure local NIP-19 / 64-hex decoder (`npub`, `nprofile`, `note`, `nevent`, `naddr`, optional `nostr:` prefix). Refuses `nsec` and `ncryptsec` without repeating input.[^nip-19] |
| `nostr_fetch_events` | `kask/mcp-servers/hkask-mcp-nostr-client/src/server.rs:425@NostrClientServer::nostr_fetch_events` | `pko:ProcedureExecution` / `dcterms:Dataset` | Fetches verified events newest-first across local + seed relays (15 s timeout/relay; requires >= 1 author or event ID; `limit` default 20, max 100; refuses DM kinds; fails visibly if zero relays answer). |
| `nostr_thread` | `kask/mcp-servers/hkask-mcp-nostr-client/src/server.rs:512@NostrClientServer::nostr_thread` | `pko:ProcedureExecution` / `dcterms:Dataset` | Reconstructs a `kind: 1` (NIP-10) or `kind: 1111` (NIP-22, including addressable roots) conversation in reading order (max 200 events), surfacing missing referenced event IDs explicitly.[^nip-10][^nip-22] |
| `nostr_profile` | `kask/mcp-servers/hkask-mcp-nostr-client/src/server.rs:654@NostrClientServer::nostr_profile` | `pko:ProcedureExecution` / `bibo:Report` | Reads newest `kind: 0` metadata for 1–20 authors; NIP-05 addresses are reported as unverified claims. |
| `nostr_author_relays` | `kask/mcp-servers/hkask-mcp-nostr-client/src/server.rs:739@NostrClientServer::nostr_author_relays` | `pko:ProcedureExecution` / `bibo:Report` | Reads an author's NIP-65 (`kind: 10002`) read/write relay list without contacting listed relays.[^nip-65] |
| `nostr_notifications` | `kask/mcp-servers/hkask-mcp-nostr-client/src/server.rs:809@NostrClientServer::nostr_notifications` | `pko:ProcedureExecution` / `dcterms:Dataset` | Reads replies, mentions, reposts, reactions, and comments tagging the configured `npub` (`limit` default 20, max 100; fails when no identity is configured). |
| `nostr_publish_note` | `kask/mcp-servers/hkask-mcp-nostr-client/src/server.rs:869@NostrClientServer::nostr_publish_note` | `pko:StepExecution` / `dcterms:Dataset` | Publishes a `kind: 1` note (max 4,000 chars) with caller-supplied `created_at` and optional `media_paths` (<= 4 local files, <= 100 MiB each, uploaded via `BUD-06` `HEAD /upload` preflight + `BUD-02` `PUT /upload` with `NIP-92` `imeta` tags), signed by the editor over IPC, broadcast to local + usable seed relays, and verified by ID read-back.[^nip-92] |
| `nostr_publish_deletion` | `kask/mcp-servers/hkask-mcp-nostr-client/src/server.rs:1866@NostrClientServer::nostr_publish_deletion` | `pko:StepExecution` / `dcterms:Dataset` | Publishes a NIP-09 `kind: 5` event deletion request targeting 1–20 of the operator's own event IDs (optional `k` tag and reason); signed inside the editor over IPC and **always** requires native operator confirmation (never bypassed by `autopublish`). |
| `nostr_publish_reaction` | `kask/mcp-servers/hkask-mcp-nostr-client/src/server.rs:1681@NostrClientServer::nostr_publish_reaction` | `pko:StepExecution` / `dcterms:Dataset` | Publishes a NIP-25 `kind: 7` reaction (default `"+"`) targeting an event and author; signed inside the editor over IPC after operator approval (or `autopublish`).[^nip-25] |
| `nostr_publish_reply` | `kask/mcp-servers/hkask-mcp-nostr-client/src/server.rs:1765@NostrClientServer::nostr_publish_reply` | `pko:StepExecution` / `dcterms:Dataset` | Publishes a NIP-10 `kind: 1` reply (marked `root`/`reply` `e` tags) or NIP-22 `kind: 1111` comment (for non-`kind: 1` roots such as `kind: 30023` articles); signed inside the editor over IPC after operator approval (or `autopublish`).[^nip-10][^nip-22] |
| `nostr_prepare_zap` | `kask/mcp-servers/hkask-mcp-nostr-client/src/server.rs:1483@NostrClientServer::nostr_prepare_zap` | `pko:StepExecution` / `dcterms:Dataset` | Prepares a NIP-57 Lightning zap: resolves recipient `lud16` under SSRF checks, obtains an editor-signed `kind: 9734` zap request (`host/sign_nostr_zap`, never `autopublish`), fetches and verifies the BOLT11 `description_hash` and `amount_msat`, and returns the verified invoice for `wallet_pay_invoice`.[^nip-57] |
| `nostr_publish_nutzap` | `kask/mcp-servers/hkask-mcp-nostr-client/src/server.rs:1598@NostrClientServer::nostr_publish_nutzap` | `pko:StepExecution` / `dcterms:Dataset` | Publishes an editor-signed NIP-61 `kind: 9321` nutzap (`host/sign_nostr_zap`, never `autopublish`) carrying P2PK-locked Cashu proofs created by `wallet_send_ecash`.[^nip-61] |
| `nostr_blossom_archive` | `kask/mcp-servers/hkask-mcp-nostr-client/src/server.rs:1085@NostrClientServer::nostr_blossom_archive` | `pko:StepExecution` / `dcterms:Dataset` | Reads verified events, fetches referenced public `https://` NIP-92 `imeta` blobs (<= 100 MiB, SSRF public-host check, no redirects), verifies SHA-256 against the `imeta` `x` tag, and uploads verified blobs to the local Blossom sidecar using editor-signed `kind: 24242` auth. |
| `nostr_blossom_mirror` | `kask/mcp-servers/hkask-mcp-nostr-client/src/server.rs:1282@NostrClientServer::nostr_blossom_mirror` | `pko:StepExecution` / `dcterms:Dataset` | Mirrors blobs from the local Blossom sidecar out to configured public `kask.nostr.blossom_servers` via `BUD-02` `PUT /upload` using editor-signed `kind: 24242` auth. |

Dual-axis P5.4 ontology annotation is wired through `hkask_mcp_server::dual_axis_annotate!` in `kask/mcp-servers/hkask-mcp-nostr-client/src/ontology.rs:18-64`.

## Cross-links

- [MCP Server Registry](README.md) — fleet catalog and `tool_surface_pin!` summary
- [Nostr Relay MCP Server Reference](nostr-relay.md) — loopback event archive (`hkask-mcp-nostr-relay`)
- [Nostr Blossom MCP Server Reference](nostr-blossom.md) — local Blossom media sidecar (`hkask-mcp-nostr-blossom`)
- [Kask Settings Reference](../kask-settings.md#nostr-kasknostrsettings) — `kask.nostr` settings, NIP-49 keychain slots, and `host/sign_nostr_note` delegation
- [Curator Nostr Client Design](../../research/curator-nostr-client-design-2026-11-11.md) — threat model and D106 design record

## Footnotes

[^nip-01]: Nostr Implementation Possibilities. *NIP-01: Basic protocol flow description*. https://github.com/nostr-protocol/nips/blob/master/01.md
[^nip-10]: Nostr Implementation Possibilities. *NIP-10: Text Notes and Threads (`kind: 1` `e`/`p` tags)*. https://github.com/nostr-protocol/nips/blob/master/10.md
[^nip-19]: Nostr Implementation Possibilities. *NIP-19: bech32-encoded entities*. https://github.com/nostr-protocol/nips/blob/master/19.md
[^nip-22]: Nostr Implementation Possibilities. *NIP-22: Comment (`kind: 1111`)*. https://nostr-nips.com/nip-22
[^nip-25]: Nostr Implementation Possibilities. *NIP-25: Reactions (`kind: 7`)*. https://nostr-nips.com/nip-25
[^nip-49]: Nostr Implementation Possibilities. *NIP-49: Private Key Encryption (`ncryptsec`)*. https://nostr-nips.com/nip-49
[^nip-57]: Nostr Implementation Possibilities. *NIP-57: Lightning Zaps (`kind: 9734`)*. https://nostr-nips.com/nip-57
[^nip-61]: Nostr Implementation Possibilities. *NIP-61: Nutzaps (`kind: 9321`)*. https://nostr-nips.com/nip-61
[^nip-65]: Nostr Implementation Possibilities. *NIP-65: Relay List Metadata (`kind: 10002`)*. https://github.com/nostr-protocol/nips/blob/master/65.md
[^nip-92]: Nostr Implementation Possibilities. *NIP-92: Media Attachments (`imeta`)*. https://github.com/nostr-protocol/nips/blob/master/92.md
[^blossom-bud02]: Hzrd149. *Blossom Specifications — BUD-02 (Blob upload/delete) & BUD-11 (`kind: 24242` authorization)*. https://github.com/hzrd149/blossom
