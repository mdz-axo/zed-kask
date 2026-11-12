# Curator Nostr relay — design proposal (2026-11-11)

**Status: proposal. Three decisions ruled by the operator (§2); the rest is
open for ratification (algedonic review). Not implemented. Nothing here was
compiled or resolved with cargo.**

**Operator ask (2026-11-11):** a second Nostr MCP server that functions as a
local relay, beside the client server proposed in
`curator-nostr-client-design-2026-11-11.md`. Both in Rust.

**Related:** `curator-nostr-client-review-2026-11-11.md` reviews the client
note and returns "rework, targeted". This relay note has not been through
that review. Where its findings bear on the relay they are cited below as
the review's, not as re-verified here.

## 1. Evidence base

Research run `b4884234f0753633` (docs.rs, RustSec, crates.io metadata) plus
the repo.

- `nostr-relay-builder` is retired: RUSTSEC-2026-0237 (issued 2026-08-09)
  marks it unmaintained and says its functionality moved into `nostr-sdk`
  from 0.45.0.
- `nostr-sdk` 0.45.6 (2026-11-03) carries the relay behind the `local-relay`
  feature, off by default: `LocalRelay`, `LocalRelayBuilder`, `MockRelay`,
  and the `WritePolicy` / `QueryPolicy` traits.
- Supported NIPs per the crate docs: 01, 09, 17, 42, 70, 77. NIPs 40, 50 and
  62 depend on the database backend. NIP-11 is not supported.
- The SDK docs state the library is alpha and its API will break. Single
  publisher on crates.io.

## 2. Operator rulings (2026-11-11)

1. **Lifetime:** the relay runs while the editor runs — the same lifetime as
   the rest of the built-in MCP fleet. No daemon.
2. **Storage:** LMDB (`nostr-lmdb`).
3. **Naming:** `hkask-mcp-nostr-client` and `hkask-mcp-nostr-relay`. The
   client note's `hkask-mcp-nostr` is renamed accordingly.

Consequences of these rulings:

- The relay is an archive, a cache and a test fixture for the client server.
  It is not reachable by other apps when the editor is closed.
- LMDB is unencrypted on disk, unlike the platform's SQLCipher databases.
  Recommendation (not ruled): reject kind 1059 gift wraps in v1 with
  `blacklist_kinds`, so no DM envelopes land in an unencrypted store.

## 3. Architecture (v1)

New MCP server `kask/mcp-servers/hkask-mcp-nostr-relay/`, laid out like
`hkask-mcp-prediction-markets` (lib + thin `main.rs`, `run_server` over
stdio).

Two planes:

- **Protocol plane:** `LocalRelay` listens on a loopback websocket. The
  client server lists `ws://127.0.0.1:<port>` as its first read relay and
  mirrors what it reads and publishes there. The two processes share no
  database.
- **Control plane:** MCP tools observe and operate the relay. They do not
  carry Nostr protocol traffic.

Tools:

- `nostr_relay_status {}` — URL, running state, event counts, policy summary.
- `nostr_relay_query { filter, limit }` — stored events, read from the store.
- `nostr_relay_sync { relays?, filter? }` — pull the operator's events from
  their outbox relays via `LocalRelay::sync_with` (NIP-77). Pull only in v1.
- `nostr_relay_log { limit }` — recent admit and reject decisions, bounded.
- `nostr_relay_delete { ids }` — remove events. A tool-level "operator
  confirmation" has no enforcing mechanism yet: the client review's F1
  reports that an MCP child has no channel to the human and that the
  permission prompt can be set to always-allow. Either this tool uses the
  host-side gate the client's publish ends up with, or it is left out of v1.

Registry entry, appended to `BUILT_IN_MCP_SERVERS`
(`kask/crates/kask_bridge/src/mcp_servers.rs`; the panel selects by index):

- `id: "nostr-relay"`, `binary: "hkask-mcp-nostr-relay"`.
- `credentials: &[]` — the relay verifies signatures and never signs.
- `config_env`: `HKASK_DATA_DIR` plus the port and the author allowlist.

A new built-in server also needs, per the D86 row (`DIVERGENCE.md` L99): a
root `Cargo.toml` workspace member and dependency, a line in
`kask/scripts/build/mcp-servers.txt` (checked against the registry by
`kask/scripts/check-mcp-servers.sh`), an entry in
`kask/docs/reference/mcp-servers/README.md`, and its own `DIVERGENCE.md`
row. The client review's F12 adds a tool-surface pin and stale "8 servers"
statements in `PRINCIPLES.md`; not re-verified here.

Settings additions (public data only): `kask.nostr.relay = { port,
allowed_authors[], allowed_kinds[] }`. Policy is the operator's; the agent
can read it through `nostr_relay_status` and cannot change it.

Write admission: a `WritePolicy` that accepts only events whose author is on
the allowlist, defaulting to the operator's npub.

## 4. Storage: LMDB

`nostr-lmdb` 0.45.3 (2026-09-11; 0.45.0 and 0.45.1 were yanked), store under
`HKASK_DATA_DIR`.

Compatibility read from metadata, not resolved:

- `nostr-lmdb` 0.45.3 requires `heed ^0.20`. The workspace pins `heed` 0.21
  (`Cargo.toml:704`).
- `heed` 0.20.5 requires `lmdb-master-sys ^0.2.4`. `Cargo.lock` already has
  `heed` 0.21.0 on `lmdb-master-sys` 0.2.5.
- Expectation: two `heed` versions coexist over one `lmdb-master-sys`. Only
  `cargo tree` confirms this.

Open: whether an LMDB environment must be registered with the database
inventory (`hkask_storage::DATABASE_CATALOG_ENV`) that `run_stdio_server`
configures at startup.

## 5. Threat model (short)

- **Loopback is not an authorization boundary.** Any local process can
  connect. A web page may be able to as well unless the server checks
  `Origin`; no `Origin` option was found on the builder. Mitigation to
  evaluate: own the accept layer and hand streams to
  `LocalRelay::take_connection`, which would also allow serving NIP-11.
- **Read access cannot be tied to identity through `QueryPolicy`.** Both
  policy traits receive a socket address, not the authenticated pubkey.
- **Signed drafts must not be stored.** A signed event in a readable relay
  can be broadcast by any reader. Signing stays at confirmation time, as the
  client note specifies.
- **Stored events are untrusted third-party text.** Tool output marks event
  content as data and caps its size.
- **Unencrypted at rest** (LMDB, §2).

## 6. Implementation notes

- `run_server`'s factory is synchronous; `LocalRelay::run()` is async. Spawn
  the relay from the factory or start it on the first tool call.
- `LocalRelay` shuts down when all clones are dropped; the server struct
  holds it for the process lifetime.
- Set the bind address explicitly to loopback; the builder's default was not
  checked.
- A second instance of the server fails to bind the port. It should report
  the conflict and serve the control tools in a degraded state.
- `MockRelay` gives the client server hermetic tests.
- Pin exact `nostr-sdk` and `nostr-lmdb` versions; keep SDK types behind one
  module.

## 7. Open items

1. Spike: add `nostr-sdk` (`local-relay`) and `nostr-lmdb`, run `cargo tree`
   and `cargo check`. Gates everything in §4.
2. Whether NIP-42 can restrict access to specific pubkeys.
3. `Origin` handling and NIP-11 via `take_connection`.
4. Whether the editor spawns one server child per window.
5. `nostr-sdk` release stability: the client review's F10 reads crates.io
   as 0.45.0 through 0.45.5 all yanked and 0.45.6 published 2026-11-03.
   Re-read at pin time.
6. Reject kind 1059 in v1 (§2 recommendation).
7. Private-thread perimeter (D100): the default disabled set is
   `["media", "corpus"]` (`crates/settings_content/src/settings_content.rs:2044`).
   `nostr_relay_sync` sends queries to public relays; decide whether
   `nostr-relay` joins the set.
8. Ontology anchor for "relay": `onto_anchor` returns `sumo:Relay`, an
   electro-mechanical switch, which is the wrong sense. The term is used
   here in the NIP-01 protocol-role sense and needs a ruling.

## 8. Charter test

The operator keeps the key (the relay holds none), the admission policy
(settings, not agent-writable) and the delete veto. The relay gives the
operator a local copy of events that today exist only on third-party relays.
Deepens the seat at the bench.
