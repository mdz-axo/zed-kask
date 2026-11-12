# Curator Nostr client — design proposal (2026-11-11)

**Status: proposal for operator ratification (algedonic review). Not implemented.**

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

## 2. What "stored in Settings → Kask → Data Services" means on this platform

Data Services is a real settings sub-page
(`crates/settings_ui/src/pages/kask_page/data_services.rs`) whose rows come
from the single-source-of-truth registry `kask_bridge::DATA_SERVICES`
(`kask/crates/kask_bridge/src/inference_providers.rs:181`), each entry a
`DataServiceDescriptor { env_var, credential_key, label, dashboard_url,
shows_in_ui }`. The page's own contract (kask_page.rs, Data Services
description): **"Keys are stored in the system keychain, not in
settings.json."** Every credential resolves through `credential_url_for_key`
to one canonical keychain URL (`kask://credentials/<credential_key>`) — the
one-key-one-location invariant (DIVERGENCE.md D28/D29 patterns).

This proposal therefore maps the operator's ask to a **Nostr row in Data
Services** whose credential lands in the system keychain — the platform's
enforced storage for every credential it manages. The raw nsec never lands
in `settings.json`: that is the page's own contract, the operator's adopted
hygiene rule (never in a synced file or git repo), and the OWASP LLM06
sensitive-disclosure path this repo's security-audit skill targets (agent
sessions read settings as ambient context).

What may live in settings JSON: public data only — the npub, the relay
list, tool toggles.

## 3. Architecture (v1)

New MCP server `kask/mcp-servers/hkask-mcp-nostr-client/` (renamed from `hkask-mcp-nostr`,
operator ruling 2026-11-11; relay: `curator-nostr-relay-design-2026-11-11.md`), surfaced to
the agent panel through the existing MCP server registration path. Tools:

- `nostr_read_notes { author | hashtag | following, limit }` — anonymous
  relay reads. **Must resolve the author's outbox (NIP-65) first**: observed
  live this session, the operator's events are absent from generic default
  relay sets while present on their published relays — reading without
  outbox resolution returns false empties.
- `nostr_read_profile { user }`, `nostr_read_thread { event }` — anonymous.
- `nostr_publish_note { text }` — reads the nsec from
  `kask://credentials/nostr` at sign time only, holds it in process memory
  for the signature, never writes it anywhere, and surfaces the drafted
  note to the operator for confirmation before broadcasting.
- `nostr_account {}` — public introspection: npub, relays, follow counts.

Read-kind note (observed live): clients post replies as NIP-22 kind:1111
comments and top-level posts as kind:1 — a reader limited to kind:1 misses
real activity. The read tools must query kind sets (1, 6, 1111, 30023, …)
and render author activity across kinds.

Settings additions (public): `kask.nostr = { npub, read_relays[],
write_relays[], publish_requires_confirmation: true }`.

Data Services row: `credential_key: "nostr"`, `label: "Nostr"`,
`env_var: ""` (no consumer reads it as an env var — the MCP server reads the
keychain directly; the RunPod precedent in DIVERGENCE D29 already keeps
credentials out of MCP child env), `dashboard_url` →
`https://nostr.com`, `shows_in_ui: true`. Paste validation: decode the
entered nsec server-side (NIP-19) and show the npub it corresponds to
before saving, so the operator sees which identity is being registered.

## 4. Signing implementation options (decision requested)

- **Option A (recommended): in-process signing with `nostr-sdk`.** The
  server reads `kask://credentials/nostr` at sign time. One key location
  (the Data Services slot); no subprocess key-passing; no env injection.
  Cost: a new crypto dependency in-repo.
- **Option B: amy-bridge.** Shell to the official `amy` CLI (installed at
  `~/Applications/amy`, v1.21.0) with an account provisioned once; amy
  holds the key in its own OS-keyring slot (secret-tool on the session
  D-Bus). Least new code, but two keychain stores to keep aligned and a
  binary dependency outside the repo.
- **Option C (v2 evolution): NIP-46 remote signer.** `amy bunker` or the
  Amethyst desktop app holds the key; the panel stores only a `bunker://`
  URI; every publish is a consented signing request (`--interactive`).
  Strongest custody posture (per-event human veto), most moving parts.
  Natural v2 once A is proven.

## 5. Threat model (short)

- settings exposure: eliminated (keychain; settings hold public data only).
- process memory: the signing process transiently holds the key — same as
  every Nostr client; documented and accepted.
- prompt injection via publish content: publish requires the operator's
  confirmation of the final text; no tool auto-embeds file contents; note
  size caps.
- public broadcast: published content is public and effectively
  undeletable — the publish tool's description says so.
- DMs (NIP-17/44/59): out of scope for v1 — encrypt/decrypt with the same
  key is a separate decision.

## 6. Charter test

The human holds the key (system keychain, unlock-gated), the publish veto
(confirmation), and the identity; the agent gains read instruments and a
draft-and-confirm publish loop. Deepens the seat at the bench.

## 7. Decisions requested from the operator

1. Signing option A / B / C (recommend A, evolving to C).
2. v1 verb scope (recommend: read notes/profile/thread + publish + account).
3. Default relays: resolve from the operator's published outbox
   (`nostr.mom`, `nos.lol`, `nostr.bitcoiner.social`).
4. Row input: accept raw nsec paste into the keychain (with npub preview),
   or ncryptsec only.

Ratification happens in the algedonic review; implementation is delegated
and verified only after the operator accepts.
