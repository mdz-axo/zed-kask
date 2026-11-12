# Curator Nostr client — design proposal (2026-11-11)

**Status: revised 2026-11-11 to state the design as the operator ruled it
(§2, §7). Four decisions remain open (§7). Not implemented; nothing here was
compiled or resolved with cargo.**

Reviewed in `curator-nostr-client-review-2026-11-11.md`; its findings are
cited below as F1–F12 and were not re-verified for this revision. The
original text of this note is in git at `d2c8c18754`.

**Operator ask (2026-11-11):** use the curator agent panel as a Nostr client —
read and publish to a Nostr account, registered with the key stored under
Settings → Kask → Data Services.

## 1. Identity model and current state

A Nostr identity is a secp256k1 keypair (NIP-01). `npub1…` is the public
address; `nsec1…` is the private key — password-equivalent, with no rotation
and no recovery: whoever holds it is the person, forever. The operator's
identity, created in Amethyst on 2026-11-11:

- npub `npub17p3778geepw8kq499uq2gdk5kg8zk9scpljlxk35klmny4uz6cvq9snak2`
  (hex `f063ef1d19c85c7b02a52f00a436d4b20e2b16180fe5f35a34b7f7325782d618`)
- Verified live on relays this session, machine-side via the official
  Amethyst CLI `amy` v1.21.0: 11 events published under this key, including
  the kind:3 contact list (`4440dedf…`), the NIP-65 relay list (read/write:
  `nostr.mom`, `nos.lol`, `nostr.bitcoiner.social`), and a public NIP-22
  kind:1111 comment replying to another user. The identity works
  cross-client (Amethyst desktop + Primal web, same npub).

## 2. What "stored in Settings → Kask → Data Services" means

Data Services is a settings sub-page
(`crates/settings_ui/src/pages/kask_page/data_services.rs`) whose rows come
from the registry `kask_bridge::DATA_SERVICES`
(`kask/crates/kask_bridge/src/inference_providers.rs:181`). The page's own
contract: **"Keys are stored in the system keychain, not in
settings.json."** A data-service credential resolves through
`credential_url_for_key` to `kask://credentials/<credential_key>`;
provider-backed keys resolve to the provider's own slot. One key, one
location (DIVERGENCE.md D5 and D29).

Operator rulings (2026-11-11), verbatim:

1. "it means that like the other service api keys the nsec private key for
   the active nostr identity in zed-kask is stored or input into the nsec
   key field in the nostr settings in the data services section of the kask
   settings."
2. "please proceed as proposed with the ncryptsec"
3. "nsec plus encryption password in nostr settings."

Read together: the Nostr row takes two inputs, the nsec and an encryption
password. The editor encrypts the nsec under that password (NIP-49) and
writes only the resulting `ncryptsec` to the keychain at
`kask://credentials/nostr`. The raw nsec is never written anywhere.

**To confirm with the operator before the row is built.** This note reads
ruling 3 as: the password is typed into the row and used, not saved. That
matches the proposal ruling 2 accepted, in which the password is asked for
again at each publish. If the password were saved beside the blob, whoever
can read the keychain would hold the key, and the encryption would protect
nothing.

What may live in settings JSON: public data only — the npub, seed relays,
the kind list.

## 3. Architecture (v1)

Three parts. The identity key exists in one of them, and only for the moment
of signing.

### The relay instrument (MCP server)

New MCP server `kask/mcp-servers/hkask-mcp-nostr-client/` (renamed from
`hkask-mcp-nostr`, operator ruling 2026-11-11; the relay server is a separate
note, `curator-nostr-relay-design-2026-11-11.md`). Registered like the other
built-in servers, with `credentials: &[]`. It holds no key and cannot sign.

Tools:

- `nostr_read_notes { author | hashtag | following, limit }`
- `nostr_read_profile { user }`
- `nostr_read_thread { event }`
- `nostr_account {}` — npub, published relay list, follow count.
- `nostr_draft_note { text }` — stores an unsigned draft and returns its id.
  Drafting and withdrawing are the agent's only write verbs.
- `nostr_list_drafts {}` — drafts and their state, for the approval prompt.
- `nostr_discard_draft { id }` — withdraws a draft that is not yet published.
- `nostr_broadcast_event { event }` — sends an already-signed event to the
  write relays and returns each relay's answer. It cannot create a signature.

The agent can call the broadcast tool, so its refusals are the control. It
sends an event only if all of these hold: the event verifies; it is kind 1;
the configured identity signed it; its text matches a pending draft; and it
was signed after that draft was created. The last condition stops an old note
the operator once signed from being re-sent against a new draft with the same
text. A draft moves from pending to signed to published; a later call may
re-send only the event already recorded for that draft.

Read rules (F3, F4, F5):

- Every event's id and signature are checked before the event reaches tool
  output.
- Event text is returned marked as untrusted third-party content, within
  count and size bounds.
- Relays are resolved at use time from the author's published relay list
  (NIP-65: read an author from that author's write relays). Observed live:
  the operator's events are absent from generic default relays and present on
  their published ones. Settings hold seed relays only.
- Every result carries a per-relay outcome. An empty read says which it is:
  no events on N reachable relays, or M of N relays unreachable.
- Kinds are a closed list. Replies to notes are kind 1 (NIP-10); kind 1111
  (NIP-22) carries replies to other kinds. A reader needs both.
- Relay addresses taken from events are validated before use.

Drafts and relay receipts are stored under `mcp/nostr-client/` (D28). They
are not secret.

### The signer (editor process)

Lives in the editor process, in kask-side code, and is not reachable from any
tool. Given the password, it reads the `ncryptsec` from
`kask://credentials/nostr`, derives the key off the foreground thread, checks
that the derived public key equals the recorded npub, signs one kind 1
event, and wipes the password and key. It refuses every other kind.

This follows the tree's host-held-key pattern (D9: "the zed side holds the
OpenRouter key via the credential channel, the child never sees it") and the
RunPod row, whose key no MCP child receives (D29).

### The approval prompt (editor UI)

An editor prompt, opened by the operator, that lists pending drafts. For the
chosen draft it shows the full text, the signing npub, the target relays and
a notice that publication is public and effectively permanent. It has one
masked password field. Approving here is the only path to a signature.

The prompt cannot be a chat turn or the existing question form: chat turns
are ingested into memory, and that form is unmasked and returns its answer to
the model (review §9). A panel can host the same prompt later (review §4).

### Publish flow

1. The agent calls `nostr_draft_note`; the draft is stored unsigned.
2. The operator opens the approval prompt and reads the draft.
3. The operator types the password and approves.
4. The editor signs exactly the text it displayed.
5. The editor hands the signed event to `nostr_broadcast_event`.
6. The prompt shows each relay's answer, then reads the event back from the
   relays. A retry re-sends the same signed event; it never signs again.

### Settings and the Data Services row

Settings (public): `kask.nostr = { npub, seed_relays[] }`. The row writes the
npub when the key is saved. There is no confirmation toggle: publishing has
no unconfirmed mode.

Data Services row: `credential_key: "nostr"`, label "Nostr identity". Inputs:
nsec, encryption password, password again — all masked. On save the editor
validates the nsec (NIP-19), shows the npub it belongs to, encrypts
(NIP-49), writes the `ncryptsec` to the keychain, records the npub, and
discards the nsec and the password. A mismatch between the two password
entries stops the save before anything is written.

The row needs changes to the shared row code (F7): identity-key wording in
place of "API key", no environment-variable hint, masked inputs, and a
presence check that does not decrypt. No MCP child consumes this credential.

## 4. Signing (ruled)

Ruled: a local key, held as an `ncryptsec`, signed in the editor process.

The options this note first offered, for the record: A, in-process signing
in the MCP server with `nostr-sdk`; B, shelling to the `amy` CLI, which keeps
the key in its own keyring slot; C, a NIP-46 remote signer. The ruled design
is A's local key with two changes: the signing moves from the MCP server to
the editor, and the stored form is encrypted. B is not taken. C remains
possible later as a second signer; nothing here blocks it.

Dependencies. None of the crates this needs are in `Cargo.lock` today: no
Schnorr signer, no bech32 codec, no scrypt, no XChaCha20-Poly1305 (F10,
review §9). The choice between the `nostr` / `nostr-sdk` crate family and a
smaller set is the first task in the plan. The relay note already plans on
`nostr-sdk`.

The plan puts the code in two library crates: one for public operations
(event ids, verification, identifiers) and one for secret operations (key
encryption, signing, key derivation). The MCP server depends only on the
first, so it cannot link signing code under any build. Names proposed in the
plan: `hkask-nostr-core` and `hkask-nostr-signer`.

## 5. Threat model

- **Settings exposure:** none. Settings hold public data only.
- **Keychain exposure:** the login keyring is unlocked for the whole session
  and reachable from an agent terminal (F2, observed). It now holds an
  encrypted blob. The password is the protection, and guessing against a
  copied blob is offline and unlimited.
- **Entering the nsec:** it is pasted once into a masked field. The clipboard
  holds it at that moment. NIP-49's security byte records that the key was
  handled unencrypted.
- **Process memory:** the editor holds the key for the moment of signing and
  wipes it. The MCP server never holds it.
- **Hostile relay content:** events are verified, marked untrusted and
  bounded before the model sees them (F3). The publish veto does not depend
  on the model: a signature needs the password.
- **Publish content:** the operator reads the full final text in the approval
  prompt; the signer signs exactly that text; notes have a size cap.
- **Password handling:** never stored, cached, logged, placed in a thread or
  returned to a tool. There is no "remember" option; one would undo F1's fix.
- **Public broadcast:** public and effectively permanent; the prompt says so.
- **DMs (NIP-17/44/59):** out of v1. The signer signs kind 1 only, and the
  read kind list excludes DM kinds (F8).
- **Privacy-marked threads:** undecided (§7).
- **Not covered:** a weak password; approving without reading; the key cannot
  be rotated if it leaks.

## 6. Charter test

The human holds the key (encrypted under a password only they know), the
veto (a signature needs that password, typed at each publish) and the policy
(settings the agent cannot change). The agent gains read instruments and a
draft verb. No tool can sign. This deepens the seat at the bench for as long
as the password is never cached.

## 7. Decisions

Ruled by the operator (2026-11-11):

- **Signing:** local key, in the editor process, stored as an `ncryptsec`
  (rulings 1 and 2).
- **Row input:** nsec plus an encryption password (ruling 3).
- **Naming:** `hkask-mcp-nostr-client`; the relay is `hkask-mcp-nostr-relay`.

Open, each with this note's recommendation:

1. **Password not saved** — confirm the reading in §2.
2. **v1 scope and order** — recommend reads first (no key involved), then
   draft, approve and publish kind 1. Sub-question: replies in v1? The
   operator's own public activity so far is a reply, and `nostr_draft_note
   { text }` cannot express one.
3. **Relays** — recommend resolving from the published relay list at use
   time, with seed relays only in settings.
4. **Privacy-marked threads** — recommend that drafting is refused on them;
   whether reads are allowed is the operator's call (F9).

The plan carries its own open questions as well (crate names, build order,
task count); they are listed in `plan.md` and come up at its first stop.

Two further points need a ruling before parts of the build can finish:

- **Docs cap.** `kask/docs` holds 84 files against a cap of fewer than 85. A
  reference page for the new server needs the cap raised or a document
  folded.
- **Panel.** The approval prompt is an editor prompt in v1. A Nostr panel is
  a separate decision (review §4 recommends a narrow one).

## 8. Plan and handoff

- Plan and checklist:
  `/home/mdz-axolotl/Documents/zk-data/skills/task-breakdown/2026-11-11-curator-nostr-client/`
  (`plan.md`, `todo.md`).
- Continuation prompt for the implementing agent: `continuation-prompt.md` in
  the same directory.

Implementation is delegated, and nothing is done until it is verified.
