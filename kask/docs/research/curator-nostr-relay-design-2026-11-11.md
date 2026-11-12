# Curator Nostr relay — design proposal (2026-11-11)

**Status: proposal. Nine decisions ruled by the operator (§2); the open items
are in §7. Not implemented. A dependency and behaviour spike ran on
2026-11-12 outside the workspace (§1a); this note has changed nothing in the
root `Cargo.toml` or `Cargo.lock`. The relay has a known access-control
deficiency, documented with a plan in §5a.**

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

## 1a. Spike results (2026-11-12)

A standalone cargo project (gitignored, `target/zk-nostr-relay-spike/`;
manifest, lockfile and sources kept, build output deleted),
pinned to `nostr-sdk =0.45.6` with `local-relay`, `nostr-lmdb =0.45.3` and
the workspace's `heed =0.21.0`. It built on rustc 1.98.1 and ran on loopback
only. Observed:

| Probe | Result |
| --- | --- |
| Resolution | `heed` 0.20.5 and 0.21.0 both resolve onto one `lmdb-master-sys` (0.2.6 in the spike; the root lock has 0.2.5) |
| Both `heed` versions in one process | `heed` 0.21 environment opened beside the relay's LMDB store |
| Allowlisted author publishes | accepted by 1 of 1 relays |
| Other author publishes | rejected: `blocked: author not on the local allowlist`; 0 of its events stored |
| Stored events after both | 1 |
| Reopen the store after dropping the relay | 1 event still present (same process) |
| Websocket upgrade with `Origin: https://evil.example` | `101 Switching Protocols` — accepted |
| Plain HTTP GET with `Accept: application/nostr+json` | connection closed, no response |
| Second relay on the same port | `Address already in use (os error 98)` |
| `LocalRelay::sync_with` from a source relay holding 2 events | reported 2 received; 0 stored |
| A `Client` on the relay's LMDB handle, open filter | 2 stored, 1 of them by a non-allowlisted author |
| The same, filter restricted to the allowlisted author | 1 stored, 0 by the other author |
| Fetch with an open filter, admit by author, store with `add_event` | fetched 2, refused 1, stored 1; 0 by the other author |
| Own accept layer: SDK client publishes as the allowlisted author | accepted |
| Own accept layer: upgrade with no `Origin` | `101 Switching Protocols` |
| Own accept layer: upgrade with `Origin` foreign, `null`, or the relay's own address | `403 Forbidden`, all three |
| Own accept layer: upgrade with `Host: attacker.example` | `421 Misdirected Request` |
| Own accept layer: NIP-11 request without and with `Origin` | `200` with the document; `403` |
| Own accept layer: `Access-Control-Allow-Origin` on any response | absent |
| Gift wrap (kind 1059) published over the websocket, kind blacklisted | rejected, "blocked: kind `1059` is not accepted by this relay"; 0 stored |
| Gift wrap passed to `LocalRelay::add_event`, kind blacklisted | stored |
| 50 events acknowledged, process killed with SIGKILL, store reopened | 50 present |

The accept-layer probe put a `hyper` listener in front of
`LocalRelay::take_connection` and never called `LocalRelay::run()`, so the
SDK's own listener did not exist. It added `hyper`, `hyper-util`, `sha1` and
`base64`; all four crate names are already in the root lock.

Read from the SDK source (`nostr-sdk-0.45.6/src/local_relay/`):

- The default bind address is loopback (`local/inner.rs`, line 87). With no port
  set, the relay picks a free one.
- NIP-42 gates on "this connection authenticated", with any key. It does not
  admit a named set of keys. Two narrower rules exist: a protected event
  (NIP-70) must be authored by the authenticated key, and with `auth_dm` a
  gift-wrap query must name the authenticated key.
- The default kind blacklist is Seal, ZapRequest, Authentication,
  BlossomAuth and HttpAuth (`builder.rs`, lines 326-368). Gift wraps (kind 1059) are
  accepted by default.
- `NostrLmdb` reports: persistent, full-text search and request-to-vanish
  supported; event expiration not handled by the store.

Dependency footprint, measured on the spike's own resolution (an upper bound;
the workspace would unify some versions):

- 138 dependency crates in the normal tree for this host; 24 of those crate
  names are not in the root `Cargo.lock` at any version.
- New native code: `secp256k1-sys`. `lmdb-master-sys` is already present.
- crates.io, read 2026-11-12: `nostr-sdk` 0.45.0 through 0.45.5 are yanked
  and 0.45.6 is the only live 0.45 release.

The operator's three relays, one HTTPS request each for the NIP-11 document
(2026-11-12): `nostr.bitcoiner.social` (strfry 1.1.0) lists NIP-77;
`nos.lol` (strfry 1.1.3) does not; no document was obtained from
`nostr.mom`, cause not determined.

Not covered by the spike: any sync against a public relay, a real browser
against either listener, a kill in the middle of a write or a power loss,
the server running under the MCP runtime, and a build inside the workspace.

## 2. Operator rulings (2026-11-11 and 2026-11-12)

1. **Lifetime:** the relay runs while the editor runs — the same lifetime as
   the rest of the built-in MCP fleet. No daemon.
2. **Storage:** LMDB (`nostr-lmdb`).
3. **Naming:** `hkask-mcp-nostr-client` and `hkask-mcp-nostr-relay`. The
   client note's `hkask-mcp-nostr` is renamed accordingly.
4. **`nostr-sdk` in the relay** (2026-11-12): accepted. Verbatim:
   "nostr-sdk is fine for the relay - yes."
5. **Private threads** (2026-11-12): both servers are blocked. Verbatim:
   "the nostr relay and client would be blocked from the private thread
   mode." `nostr-relay` joins the default disabled-server set
   (`crates/settings_content/src/settings_content.rs:2044`, D100).
6. **Client and local relay** (2026-11-12): the client is unchanged in v1
   and the relay stands alone (§3a). Verbatim: "yes - confirmed".
7. **Own accept layer that refuses `Origin`, in v1** (2026-11-12). Verbatim:
   "yes as recommended - document deficiency and sketch future plan for
   resolution". The deficiency and the plan are §5a.
8. **Gift wraps** (2026-11-12): rejected. Verbatim: "reject gift wrap - yes."
9. **Start and stop** (2026-11-12). Verbatim: "relay should be started and
   shut down with the built in mcp server fleet for now in v1." The
   mechanism is in §6.

Consequences of these rulings:

- The relay is an archive, a cache and a test fixture for the client server.
  It is not reachable by other apps when the editor is closed.
- LMDB is unencrypted on disk, unlike the platform's SQLCipher databases.
  Gift wraps (kind 1059) are rejected (ruling 8), so no DM envelopes land in
  the store. The SDK accepts them by default, and its kind blacklist covers
  only the websocket path (§1a), so the rejection lives in the relay's own
  admission function (§4).

## 3. Architecture (v1)

New MCP server `kask/mcp-servers/hkask-mcp-nostr-relay/`, laid out like
`hkask-mcp-prediction-markets` (lib + thin `main.rs`, `run_server` over
stdio).

Two planes:

- **Protocol plane:** a loopback websocket for Nostr apps on this machine,
  behind the relay's own accept layer (§5a). The client server does not
  connect to it (ruling 6). The two servers share no database.
- **Control plane:** MCP tools observe and operate the relay. They do not
  carry Nostr protocol traffic.

Tools:

- `nostr_relay_status {}` — listener address and state, event counts, policy
  summary, and the access limits in force (§5a), stated in the output.
- `nostr_relay_query { filter, limit }` — stored events, read from the store.
- `nostr_relay_sync { relays? }` — pull the allowlisted authors' events from
  their outbox relays into the store. Pull only in v1. It cannot use
  `LocalRelay::sync_with`, which stores nothing (§4).
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
the allowlist, defaulting to the operator's npub. The spike ran this policy
(§1a).

## 3a. Where this design meets the client plan

The client was re-planned after this note was first written (design note at
`7a1472b6cb`; plan under
`~/Documents/zk-data/skills/task-breakdown/2026-11-11-curator-nostr-client/`).
That plan puts the relay server out of scope. Its build is under way: t1 is
committed (`81f390c2b8`).

### Ruled: the client is unchanged in v1

The client plan's task t8 refuses a relay address before connecting unless it
is `wss://` and its host is not `localhost` or an IP literal in a loopback,
private, link-local or unspecified range. `ws://` and loopback are accepted
only under a test-only feature, and a build without it refuses them
"including when they come from settings". The plan's t12 puts relay hints
taken from events through the same check.

Why the check exists (client review F3): events carry relay addresses chosen
by their authors, and a read tool that follows them "connects to hosts
chosen by event authors". Without the check, an event could point the
client at a service on this machine or network.

The effect on this design: `ws://127.0.0.1:<port>` fails the check twice, on
scheme and on host. The client cannot connect to the local relay, so the
first version of this note ("the client lists the local relay as its first
read relay and mirrors there") cannot be built as the client is planned.

Ruled (§2 ruling 6): **no change to the client in v1.** The relay fills its
own store with `nostr_relay_sync`, and the agent reads the archive through
`nostr_relay_query`. Reasons:

- Nothing in the client plan needs the local relay. Its tests use their own
  test relay (t7).
- The agent reaches the archive either way. The control-plane tools read the
  store directly, in the relay's own process.
- Mirroring would store other authors' events. That contradicts the author
  allowlist, and what the agent reads would decide what the archive holds.
- The client's check stays one rule with no exception, and its build is not
  disturbed.

If the operator later wants the client to read from the archive (offline
reads, or a second place to confirm that a publish landed), the change to
the client should be this and no more:

- One address, taken from settings only, compared for exact equality. It is
  not a relaxation of the check, and loopback addresses found in events stay
  refused.
- The client reads from it and writes nothing to it by default.
- Whether a publish is also sent to it is a separate operator choice.
- Events read from it are verified like any other relay's.

### Also ruled

- **`nostr-sdk` in the relay** (§2 ruling 4). For the record: the client's
  signing set is `k256`; the relay's tree carries `secp256k1` 0.30.0 through
  the SDK, so the workspace holds two Schnorr implementations. The two sets
  share only `bech32`, at the same version (0.12.0), so the client's exact
  pins do not clash with the relay's tree. The relay holds no key.
- **Private threads** (§2 ruling 5).

## 4. Storage: LMDB

`nostr-lmdb` 0.45.3 (2026-09-11; 0.45.0 and 0.45.1 were yanked), store under
`HKASK_DATA_DIR`.

Compatibility, confirmed by the spike (§1a) outside the workspace:

- `nostr-lmdb` 0.45.3 requires `heed ^0.20`. The workspace pins `heed` 0.21
  (the root `Cargo.toml` `heed =` line).
- The two `heed` versions resolve onto one `lmdb-master-sys` and run in one
  process.
- Still to do: the same resolution inside the workspace, where the lock pins
  `lmdb-master-sys` 0.2.5.

Open: whether an LMDB environment must be registered with the database
inventory (`hkask_storage::DATABASE_CATALOG_ENV`) that `run_stdio_server`
configures at startup.

### How sync fills the store

Three facts from the spike (§1a) and the SDK source:

- `LocalRelay::sync_with` does not fill the store. It builds a fresh client
  with no handle on the relay's database (`local/inner.rs`, line 229) and only
  notifies live subscribers. Observed: 2 received, 0 stored.
- A `Client` given the relay's database handle does store what it syncs.
- The write policy is not applied on that path. It runs only where a
  websocket client publishes (`local/inner.rs`, line 743). Observed: with an open
  filter, an event by a non-allowlisted author was stored.

So the allowlist has to be enforced by the sync code itself. Design:

- One admission function, used by both the websocket `WritePolicy` and the
  sync path. It checks the author allowlist and the kind rules, including
  the gift-wrap rejection: `add_event` applies neither the write policy nor
  the builder's kind blacklist (§1a). The builder's blacklist is set as
  well, as a second check on the websocket path.
- Sync fetches with a plain subscription into a client that has no handle on
  the store, runs each event through that function, and stores admitted
  events with `LocalRelay::add_event`, which verifies the event first. The
  spike ran this shape: fetched 2, refused 1, stored 1.
- The fetch filter names the allowlisted authors, to limit what is
  transferred. The admission function is the control; the filter is not.

This gives up negentropy for v1. The cost is small for one identity's
events, and only one of the operator's two answering relays lists NIP-77
(§1a). Negentropy can return later if its receive path can be put behind
the admission function.

## 5. Threat model (short)

- **Loopback is not an authorization boundary.** This is the relay's known
  deficiency. §5a states it, what v1 does about it, and the plan.
- **Signed drafts must not be stored.** A signed event in a readable relay
  can be broadcast by any reader. Signing stays at confirmation time, as the
  client note specifies.
- **Stored events are untrusted third-party text.** Tool output marks event
  content as data and caps its size.
- **Unencrypted at rest** (LMDB, §2). In the spike LMDB created `data.mdb`
  and `lock.mdb` with mode `0600`. The server creates the store directory
  itself with mode `0700`.

## 5a. Known deficiency: no access control on the listener

### The deficiency

The relay cannot tell who is connecting.

- A TCP connection on loopback carries no identity. Any process on this
  machine, under any user account, can connect.
- The SDK offers no way to admit named keys. Both policy traits receive a
  socket address, and its NIP-42 support admits any authenticated key
  (§1a).
- The SDK's own listener also accepts a websocket upgrade from a web page:
  the spike's request with a foreign `Origin` was accepted (§1a).

### What v1 does (ruling 7)

The server runs its own accept layer and never starts the SDK's listener.
The spike ran this shape (§1a). The layer:

- binds loopback only;
- refuses any request that carries an `Origin` header, whatever its value.
  There is no allowlist of origins;
- refuses any request whose `Host` is not the bound loopback address. A
  page that reaches the relay through DNS rebinding may send no `Origin` on
  a plain GET; this check is for that case;
- sends no `Access-Control-Allow-Origin` header on any response;
- serves a NIP-11 document that states the relay's limits;
- hands every other websocket upgrade to `LocalRelay::take_connection`.

### What remains open after v1

- **Reads.** Any local process can read the whole store.
- **Writes.** Any local process can submit an event, and it is stored if an
  allowlisted author signed it. A process cannot forge one. It can replay
  one, including that author's own deletion requests.
- **Capacity.** Any local process can hold connections up to the limit.
- **The `Origin` refusal stops browsers and nothing else.** It relies on the
  browser sending the header. A program that is not a browser can leave it
  out. The refusal has not been tested against a real browser.

### Why this is accepted for v1

The store holds only events that allowlisted authors already published to
public relays, and no gift wraps. A reader gains nothing that is not already
public, apart from learning which authors the operator archives.

That makes two settings load-bearing. If either changes, the deficiency has
to be closed first:

- the kinds admitted stay public kinds (no gift wraps, no other private
  material);
- the store never holds an unpublished or draft event.

`nostr_relay_status` and the NIP-11 document both state these limits, so
the operator and any connecting app can see them.

### Plan for resolution (sketch, not designed in detail)

In order of cost. Each step names what it closes and what it does not.

1. **A secret in the relay address.** The server creates a random token,
   keeps it in a `0600` file beside the store, and accepts only
   `ws://127.0.0.1:<port>/<token>`. The operator pastes that address into a
   Nostr app once. Closes: other user accounts and anything that cannot read
   the file. Does not close: processes running as the operator, which can
   read the file, and could read the store directly anyway. To check first:
   that the apps the operator uses accept a relay address with a path.
2. **A Unix socket for fleet-internal readers.** `take_connection` accepts
   any byte stream, so a socket with file permissions can carry the same
   protocol (read from the signature; not run). Ordinary Nostr apps cannot
   use it. Its use is the case §3a leaves for later: the client server
   reading the archive without a loopback exception in its address check.
3. **Access tied to a Nostr key.** NIP-42 with a named set of keys. This
   needs the SDK to pass the authenticated key to the policy traits, or
   the relay to handle the AUTH exchange itself. Note the limit on this
   platform: the operator's key signs only in the editor, with the
   password, so this serves apps that hold their own key.
4. **Encryption at rest.** Required before any private kind is admitted. The
   route is a `NostrDatabase` over the platform's SQLCipher store, the
   option set aside when LMDB was chosen.

Triggers. Step 1 before the machine has a second user account, or before
the relay is offered to an app outside the fleet as more than a public
archive. Steps 3 and 4 before any non-public kind is admitted.

## 6. Implementation notes

### Start and stop with the fleet (ruling 9)

How the fleet works today, read from the tree:

- One runtime owns every built-in server process (`hkask_mcp::McpRuntime`,
  the "single spawn authority"). Servers are not started per project or per
  window. Observed: one editor process, eight server children.
- A server is in the load set when `kask.mcp.load_default` is on and its
  entry in `kask.mcp.overrides` is not `false`.
- On a settings change the runtime stops and starts servers to match
  (`McpRuntime::stop_server`, `start_server_with_env`). It also restarts a
  server that dies.
- On editor quit, `wire_kask_mcp_shutdown` calls
  `McpRuntime::shutdown_all`.
- Both stop paths drop the connection and then kill the child
  (`stop_children`). The server cannot count on running shutdown code.

What the relay server does with that:

- **Start.** The listener and the store open when the server process
  starts, before the first tool call. Registering the server in
  `BUILT_IN_MCP_SERVERS` is all that ties it to the fleet.
- **Stop.** Nothing is required at stop. Every event the relay has
  acknowledged is already committed: `nostr-lmdb` opens the environment
  without the flags that defer syncing. The spike killed a process holding
  50 acknowledged events and found all 50 on reopening (§1a).
- **Off switch.** `kask.mcp.overrides["nostr-relay"] = false` stops the
  relay without quitting the editor.
- **Private threads.** Ruling 5 is read here as: the relay's tools are
  refused on a private thread, as the corpus and media tools are. Inference
  from the fleet being per editor and not per thread: the process and its
  listener keep running while a private thread is open, and nothing from a
  thread reaches the listener. If the operator means the relay should stop
  while a private thread is open, that is a different mechanism and needs
  saying.
- **Bind failure.** If the port is taken, the server still starts and
  serves its tools; `nostr_relay_status` reports the listener as down, with
  the error. A second relay on the same port fails with "Address already
  in use" (§1a).
- **Port.** A stable address for other apps needs a configured port. With
  none set, a free port is picked at each start.

To test in the build: the listener rebinding at once after a kill and
restart; and two server processes opening one store.

### Other notes

- `run_server`'s factory is synchronous. The server's own `run()` is async
  and opens the store and the listener before it calls `run_server`.
- `LocalRelay::run()` is never called (§5a). The server struct holds the
  `LocalRelay` for the life of the process.
- `MockRelay` gives this server hermetic tests.
- Pin exact `nostr-sdk` and `nostr-lmdb` versions; keep SDK types behind one
  module.

## 7. Open items

1. When the build starts. Not ruled. Recommendation: after the client build
   has registered its server (its tasks t4 and t5), because both builds edit
   the registry, the installer list and the root manifest in one tree; and
   from a task plan like the client's.
2. `nostr_relay_delete` in v1 or not (§3): it has no human gate yet.
3. Whether an LMDB store needs a database-inventory entry (§4).
4. The steps in §5a's plan, each at its trigger.
5. `nostr-sdk` has one live 0.45 release and a single publisher (§1a).
   Re-read crates.io at pin time.
6. Tests the spike did not run: a real browser against the accept layer; a
   sync against a public relay; the two items at the end of §6's first
   part; a build inside the workspace.
7. Closed by the spike or by reading the tree: dependency resolution, the
   allowlist on the websocket path, whether NIP-42 can name keys (it
   cannot), how sync must store (§4), the accept layer's behaviour (§1a),
   survival of a kill (§6), and one server child per editor (§6).
8. Closed by ruling: §2, rulings 4 to 9.
9. Ontology anchor for "relay": `onto_anchor` returns `sumo:Relay`, an
   electro-mechanical switch, which is the wrong sense. The term is used
   here in the NIP-01 protocol-role sense and needs a ruling.

## 8. Charter test

The operator keeps the key (the relay holds none), the admission policy
(settings, not agent-writable) and the off switch. The relay gives the
operator a local copy of events that today exist only on third-party relays.
Its access limits are stated where the operator and connecting apps can read
them (§5a). Deepens the seat at the bench.
