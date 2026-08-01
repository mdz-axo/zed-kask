# Minimalist Refactor — Todo

## Slice 1 — `EnergyEstimator` trait deletion test
- [x] Done. Verdict: **remove**. 92+10 tests green.

## Slice 2 — `EscalationPort` trait deletion test
- [x] Done. Verdict: **remove**. 72+79 tests green. Zero dyn-consumers.

## Slice 3 — `LedgerStoragePort` trait deletion test
- [x] Done. Verdict: **remove**. 72+79 tests green. Zero dyn-consumers.

## Slice 4 — `EmbeddingPort` trait deletion test
- [x] Done. Verdict: **remove**. 72+79 tests green. Zero dyn-consumers.

## Slice 5 — `WalletBudgetPort` + `WalletBackedBudget` dead path deletion test
- [x] Done. Verdict: **remove**. 91 tests green (1 test removed with deleted production code).
  `register_wallet_budget` had zero call sites; entire `WalletBackedBudget` →
  `wallet_budgets` map → sensor fallback chain was dead.

## Slice 6 — `SkillReader` trait deletion test
- [x] Done. Verdict: **remove**. 130 tests green. Single impl, no test mock
  despite doc claim.

## Slice 7 — `RuntimePolicy` trait deletion test
- [x] Done. Verdict: **remove**. 91+130 tests green. Consumer already depended
  on impl crate directly.

## Final report
- [x] `tasks/final-report.md` written with before/after code graph, edge delta,
      deletion-test verdicts, and suggested .rules additions.

---

# Bridge Seam Simplification — Open

## Operator hypothesis (2026-07-31)

The complexity of the kask_bridge seam surface (D1–D12 in DIVERGENCE.md)
caused an error in one of the seams that we have missed. The best way to find
it and clean it up is by simplifying the seams — reducing the number of
interfaces the bridge must manage. The OpenRouter 402 / "can only afford
7326 tokens" failure is the presenting symptom; the root cause is a seam
that resolves the wrong key, sends the wrong max_tokens, or shadows the
upstream provider with a compatible-provider entry that diverges in key
resolution.

## Open questions

1. **Sovereignty keys (D5) essentialist review.** Do `hkask-keystore`,
   `HKASK_OCAP_SECRET`, `HKASK_A2A_SECRET`, and the `keyring`-direct
   keychain path pass the essentialist deletion test? Preliminary verdict:
   **no** — hKask runs in-process under zed-kask; the `McpRuntime`
   governance membrane is the real authority and does not depend on the
   OCAP token. The sovereignty-key seam exists for a standalone-hKask
   deployment topology that zed-kask does not use. It adds keychain reads,
   env-var resolution, and a `OnceLock`-injected `keyring` — all surfaces
   where a key can fail to resolve silently. Run the full G1/G2/G3 essentialist
   loop to confirm before deleting.

2. **Bridge seam inventory + essentialist audit.** Enumerate every interface
   in `kask/crates/kask_bridge/` and every D-seam in `DIVERGENCE.md`. For each,
   run the deep-module deletion test: if the seam were removed, where would
   the complexity reappear? Seams that exist only to support a deployment
   topology zed-kask doesn't use (standalone hKask, daemon transport, OCAP
   membrane in front of `McpRuntime`) are candidates for removal.

3. **OpenRouter key resolution divergence.** The key in `kask/.env`
   (`sk-or-v1-6afb63472df...`) queries as `limit: 500, usage_weekly: 499.97,
   limit_remaining: 0.032` via the OpenRouter API — a $500/week key that is
   maxed out, not the $3000/week key the operator reports in the dashboard.
   Either (a) the file has the wrong key, or (b) zed-kask is sending a
   different key than the one in the file. Discriminating test: add a
   one-line `log::info!` in `OpenRouterLanguageModelProvider::stream_completion`
   that logs the key prefix (first 12 chars) + the key ID hash, then compare
   against the file. This isolates whether the divergence is in key storage
   or in the bridge's key-resolution seam.

4. **`max_tokens: null` divergence.** Upstream zed sends `max_tokens: null`
   for OpenRouter (because `max_output_tokens()` returns `None`), and
   OpenRouter applies its 65,536 default. zed-kask has not changed this path.
   But the 402 is triggered by the 65,536 default exceeding the key's
   remaining weekly budget. If the key-resolution seam is fixed (correct
   $3000/week key), the 65,536 default is fine. If not, a `max_output_tokens`
   override setting may be needed. Do not add it until the key seam is
    resolved — otherwise we mask the real bug.

## Essentialist audit — D5 sovereignty keys (completed)

### G1 — EXIST (deletion test): FAIL for OCAP/a2a seam, PASS for DB passphrase

The `hkask-keystore` crate contains two distinct subsystems:

1. **DB passphrase + encryption** (`resolve_db_passphrase_string`, `derive_key`,
   `EncryptionService`, `Keychain::store_by_key`). Consumed by `kask_bridge/src/identity.rs`,
   `kask_bridge/src/memory.rs`, `hkask-storage/src/core/database.rs`,
   `hkask-services-core/src/config.rs`. **Load-bearing** — deleting this would
   require reappearing keychain read + generation + Argon2 derivation in 4+
   consumers. PASSES G1.

2. **OCAP/a2a sovereignty secrets** (`resolve_a2a_secret`, `get_or_create_ocap_secret`,
   `resolve_secret_chain`, `resolve_treasury_key`, `resolve_wallet_seed`,
   `sign_wallet_bytes`, `derive_all_internal_secrets`, `derive_sub_key`,
   `InternalSecrets`). Consumed by `main.rs` (PanelToolInvoker token minting),
   `hkask-mcp-server/src/server/credentials.rs`, `hkask-services-core/src/config.rs`,
   `hkask-templates/src/executor.rs`, `kask_bridge/src/skill_executor.rs`.
   **FAILS G1** — the OCAP token is self-signed: `PanelToolInvoker` mints a
   token with a signing key derived from `a2a_secret`, and `McpRuntime::invoke`
   verifies the token's signature against the token's OWN embedded public key
   (`token.verify()` in `token_types.rs:273`). The verification does not check
   that the public key corresponds to a trusted authority. Anyone can mint a
   valid token with any signing key. The `a2a_secret` is not verified by the
   runtime — it's only used to sign, and the verification doesn't check the
   signer's identity. This is security theater ("Advertised invariants need
   enforcement points" trap from `.rules`).

### G2 — SURFACE: FAIL

`hkask-keystore` exposes 25+ public items. The sovereignty-secret subsystem
alone exposes 10+ public functions (`resolve_a2a_secret`, `get_or_create_ocap_secret`,
`resolve_secret_chain`, `resolve_treasury_key`, `resolve_wallet_seed`,
`sign_wallet_bytes`, `derive_all_internal_secrets`, `derive_sub_key`,
`derive_all_internal_secrets_with_version`, `InternalSecrets`). Most have zero
or one dynamic consumers in zed-kask.

### G3 — CONTRACT: FAIL

`resolve_a2a_secret` is a pass-through: reads env var or keychain, returns
bytes. The bytes are used to derive an Ed25519 signing key that signs tokens
which verify against themselves. The entire chain adds no security beyond
what `McpRuntime`'s gas/regulation membrane already provides.

### Verdict: REMOVE the OCAP/a2a sovereignty-secret surface

The OCAP/a2a seam is security theater. Remove:
- `resolve_a2a_secret`, `get_or_create_ocap_secret`, `resolve_secret_chain`,
  `resolve_treasury_key`, `resolve_wallet_seed`, `sign_wallet_bytes`
- `derive_all_internal_secrets`, `derive_sub_key`, `InternalSecrets`
- The `a2a_secret` field threaded through `PanelToolInvoker`,
  `BridgeManifestExecutor`, `hkask-services-core/src/config.rs`,
  `hkask-templates/src/executor.rs`, `kask_bridge/src/skill_executor.rs`
- The `HKASK_A2A_SECRET` / `HKASK_OCAP_SECRET` env var resolution

Keep:
- `resolve_db_passphrase_string`, `derive_key`, `Keychain`, `EncryptionService`
  (load-bearing for SQLCipher encryption)

## Next action

Begin removing the OCAP/a2a sovereignty-secret surface. Start with the
`a2a_secret` field threading through `main.rs` → `PanelToolInvoker` →
`BridgeManifestExecutor` → `hkask-templates/src/executor.rs` →
`kask_bridge/src/skill_executor.rs` → `hkask-services-core/src/config.rs`.
Replace the OCAP token minting in `PanelToolInvoker` with a no-op token
(since verification is theater anyway) or remove the token parameter entirely
if `McpRuntime::invoke` can be called without governance for the panel path.

## Completed — D5 OCAP/a2a seam removal

- [x] Added `panel_default_token` helper to `hkask-capability/src/auth.rs` —
      mints a `DelegationToken` with a static zeroed key (verification is
      self-referential, so the key doesn't matter).
- [x] Removed `a2a_secret` field from `PanelToolInvoker` in `main.rs` —
      replaced token minting with `panel_default_token`.
- [x] Removed `a2a_secret` resolution from `main.rs` startup (the
      `hkask_keystore::keychain::resolve_a2a_secret()` call + warn).
- [x] Removed `a2a_secret` field from `BridgeManifestExecutor` in
      `kask_bridge/src/skill_executor.rs`.
- [x] Removed `a2a_secret` field from `ManifestExecutor` in
      `hkask-templates/src/executor.rs` — replaced token minting with
      `panel_default_token`.
- [x] Removed `a2a_secret` field from `ServiceConfig` in
      `hkask-services-core/src/config.rs` (was stored but never read).
- [x] Removed `HKASK_OCAP_SECRET` / `HKASK_A2A_SECRET` resolution from
      `hkask-mcp-server/src/server/credentials.rs`.
- [x] Added diagnostic log to `open_router.rs` — logs key prefix (12 chars)
      + source (env_var vs keychain) at every `stream_completion` call.
- [x] All tests pass: hkask-capability (13), hkask-templates (30+115),
      kask_bridge (19), hkask-services-core (92), hkask-mcp-server (4).
- [x] Full workspace compiles clean.
- [x] `cargo build -p zed` succeeds.

### Remaining D5 cleanup (deferred)

The dead sovereignty-secret functions still exist in `hkask-keystore/src/keychain.rs`
(`resolve_a2a_secret`, `get_or_create_ocap_secret`, `resolve_secret_chain`,
`resolve_treasury_key`, `resolve_wallet_seed`, `sign_wallet_bytes`) and
`hkask-keystore/src/master_key.rs` (`derive_all_internal_secrets`,
`derive_sub_key`, `InternalSecrets`). These are now unreferenced from zed-kask
but still referenced from `hkask-keystore`'s own tests. Remove them in a
follow-up commit after verifying no MCP server still calls them.

### Next seam to audit

D8 (Bridge + adapters) — the largest seam. Enumerate every adapter in
`kask/crates/kask_bridge/` and run the essentialist deletion test on each.