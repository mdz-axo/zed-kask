# Divergence Slimming + Seam Simplification — Plan

Date: 2026-07-31 · Status: **COMPLETE** (all slices landed; tests green)

## Outcome summary

- **Phase A (reverts)**: all 4 non-essential `default.json` default groups reverted
  (rust-analyzer off, auto-install toml/csv/jinja, Jinja globs, bash/yaml/taplo LSPs);
  settings_store write-skip reverted (upstream candidate); 7 files restored
  byte-identical to merge-base (trusted_worktrees, acp_diff, alacritty, 2 CI
  actions, 2 CI workflows); OpenRouter key-prefix logging + visibility bumps
  reverted (kept only the D13 `max_output_tokens` plumbing).
- **Phase B (docs)**: DIVERGENCE.md updated — D3/D4/D5/D6 rewritten post-removal,
  D13 stale log reference removed, 11 previously undocumented seams listed.
- **Phase C (seam simplification)**: C1 self-referential `token.verify()` gate
  removed (runtime.rs); C2 partially descoped — `AuthContext`, `verify.rs`
  require_*_access, and `NoOpTokenRegistry` deleted, but `TokenRegistry`/
  `TokenRegistryStore` KEPT (live consumer: curator `list_tokens` consent audit
  — the audit's "zero consumers" claim was wrong); C3 `BridgeToolPort` collapsed
  (McpRuntime passed directly); C4 `LoggingMemoryPort` + early wiring deleted;
  C5 duplicate condenser/tool-invoker wiring in main.rs deleted; C6
  `direct_chat_strategy` knob + Guard settings page deleted.
- **Deferred**: LedgerObserver subscriber bus (needs operator decision);
  upstream PRs for trusted_worktrees/settings_store/head-tail fixes.

## Keystore / sovereignty-token verdicts (confirmed)

1. Sovereignty-token removal: complete (19c5ca5f80 + C1 residue removed).
2. Keystore duplication: absent — hkask-keystore uses `keyring` directly
   everywhere; no injection seam, no parallel CredentialsProvider path.
   D5 row rewritten to reflect this.

## Validation

- `cargo check`: zed, kask_bridge, hkask-mcp, hkask-capability, settings_content,
  settings_ui, language_models — all clean.
- `cargo nextest`: 762 agent + 148 bridge/capability/mcp/settings_content +
  71 settings/auto_update/kask_panel — all pass.
- Upstream diff audit: `default.json` shows only telemetry/auto_update/
  credentials_url; settings_store shows only the auto_update test pin.

## Methodology notes (grill-me self-challenge outcomes)

- The sub-agent audit claimed TokenRegistry had "zero production consumers" —
  grep proved it wrong (curator MCP `list_tokens`). Corrected before deletion.
- An E0004 in remote_connection during test builds was a stale-artifact flake;
  not reproducible after rebuild, unrelated to the changes.
- Working-tree edits were committed incrementally by the operator during the
  session (commits f5ed02574e…7edd0e7cc8); content verified equivalent to plan.

## Hypothesis (H1) and null (H0)

- **H1**: Removing the 12 identified non-essential upstream-file modifications
  and 7 theater/ritual seam surfaces loses no behavior zed-kask depends on.
- **H0**: At least one candidate removal silently breaks a kask behavior.
- **Discriminating test**: after each slice, `cargo check -p <touched crates>`
  + `cargo nextest run -p <touched crates>` must pass; Phase A additionally
  requires `git diff 5e1fd392f6 HEAD -- <file>` to show only sanctioned seams.

## Phase A — Revert non-essential upstream modifications (8 tasks)

Surgical reverts only; the pin tests for telemetry/auto_update defaults stay.

- **A1. `assets/settings/default.json` surgical revert.** Remove four blocks:
  rust-analyzer `enabled: false`; `auto_install_extensions` toml/csv/jinja;
  `Jinja` file-globs mapping; bash-language-server/yaml-language-server/taplo
  `enabled: true`. KEEP: telemetry off, auto_update off, `credentials_url`.
  AC: `git diff 5e1fd392f6 -- assets/settings/default.json` shows only the
  three kept blocks. Verify: `cargo nextest run -p auto_update -p settings`.
- **A2. `crates/settings/src/settings_store.rs` partial revert.** Remove the
  "skip write if unchanged" block (upstream candidate; kask call sites are
  user-driven, no observer loops exist). KEEP the `AutoUpdateSetting{false}`
  test assertion. Verify: `cargo nextest run -p settings`.
- **A3. Checkout reverts (7 files, one command).**
  `git checkout 5e1fd392f6 -- crates/project/src/trusted_worktrees.rs
  crates/acp_thread/src/diff.rs crates/terminal/src/alacritty.rs
  .github/workflows/run_tests.yml .github/workflows/extension_tests.yml
  .github/actions/run_tests/action.yml .github/actions/run_tests_windows/action.yml`
  AC: files byte-identical to merge-base. Verify: `cargo check -p project -p acp_thread -p terminal`.
- **A4. OpenRouter key-prefix logging removal** (`crates/language_models/src/provider/open_router.rs`):
  remove the `log::info!` key-prefix hunk AND the two `fn`→`pub(crate)` visibility
  bumps added with it (verify no other module uses them first). Verify: `cargo check -p language_models`.
- **Checkpoint A**: full `./script/clippy` clean on touched crates; upstream diff
  audit shows only sanctioned seams remain in those files.

## Phase B — DIVERGENCE.md gap closure (1 task)

- **B1. Document the ~10 discovered undocumented seams** so Phase A's retained
  edits stop being invisible merge-cost: `windows_resources` (D7 title),
  `gpui_tokio::handle_async`, `RuleFrontmatter`/conditional rules,
  terminal_tool tuning (spillover, head/tail fix, shell-subst wording),
  `tools.rs` u64 deserializer, `agent_panel` curator hunks,
  `language_model` ModelFilterFn/api_url, `open_ai/list_models`,
  `.env` loading + printenv, `zed_urls.rs` scheme. Each gets a one-line entry
  in the "Other zed-kask-modified files" section or its D-seam row.

## Phase C — Seam simplification: theater removal (6 tasks, from audit)

Each is an independent essentialist deletion with a cargo-verified blast radius.

- **C1. Remove self-referential `token.verify()` gate** in
  `kask/crates/hkask-mcp/src/runtime.rs:505` (signature checked against the
  token's own embedded key — denies nothing). Keep `is_valid_for` capability
  match. Fix the "OCAP" comment to state what's actually enforced.
  Verify: `cargo nextest run -p hkask-mcp -p hkask-capability`.
- **C2. Delete dead OCAP surface** (~500 lines): `AuthContext` (hkask-capability
  auth.rs — zero production constructors), `verify.rs` require_read/write_access
  (only self-test callers), `TokenRegistry` trait + `NoOpTokenRegistry` +
  `TokenRegistryStore` (~300 lines SQL, constructed only in its own test).
  AC: zero references remain; `cargo check --workspace` passes.
- **C3. Collapse `BridgeToolPort`** (pure pass-through to `McpRuntime`, which
  already implements `ToolPort`): main.rs passes `Arc<McpRuntime>` as the port;
  delete `kask/crates/kask_bridge/src/tool_port.rs`. Verify: `cargo check -p kask_bridge -p zed`.
- **C4. Delete `LoggingMemoryPort` + early wiring** in main.rs:760-776 (turns
  pre-login were silently dropped anyway; `thread.rs:2879` already no-ops on
  `None`). Verify: `cargo nextest run -p kask_bridge -p agent`.
- **C5. Delete duplicate hook wiring** in main.rs:1769-1789 (deferred
  `set_tool_invoker` + `set_thread_condenser` re-wiring identical to the
  pre-login block; neither is model/user-dependent). AC: single wiring site
  each. Verify: `cargo check -p zed`.
- **C6. Remove `direct_chat_strategy` setting + UI knob** (declared
  "buffer|incremental|cascade_only", default cascade_only, read nowhere — a
  settings page configuring a one-inhabitant enum). Touch: kask_bridge/settings.rs,
  settings_content.rs, kask_page/guard.rs, kask_page.rs, DIVERGENCE.md D4 text.
  Verify: `cargo nextest run -p kask_bridge -p settings_ui`.
- **Checkpoint C**: `cargo check --workspace`; hkask test suites green;
  DIVERGENCE.md updated for D3/D4/D6 wording.

## Loose-end sweep (2026-07-31, third round)

- **Docs sweep**: 12 files updated — mcp-servers/README + companies.md
  (governance section now says capability-match, not OCAP/signature),
  kask_bridge/reference.md (BridgeToolPort/a2a_secret/LoggingMemoryPort
  references removed), hkask-capability reference/explanation/tutorial/
  how-to rewritten for the post-collapse token model, hkask-storage/
  hkask-types/hkask-mcp-server references fixed, explanation/README index
  row updated. `.rules` GuardedStream trap deleted (resolved in D4).
- **Keystore dead surface**: `get_or_create_ocap_secret`, `resolve_a2a_secret`,
  `resolve_secret_chain`, `resolve_treasury_key`, `resolve_wallet_seed`,
  `sign_wallet_bytes`, `InternalSecrets` + `derive_all_internal_secrets*`,
  `MASTER_KEY_SALT`, `CURRENT_KEY_VERSION`, `KEY_A2A_SECRET`, `KEY_OCAP_SECRET`,
  `A2A_SECRET`/`OCAP_SECRET`/`TREASURY_HEDERA`/`WALLET_SEED` derivation
  contexts — all removed (zero production consumers). Kept: `resolve()`,
  `resolve_db_passphrase{,_string}`, `Keychain`, encryption, `derive_sub_key`
  (live via `SecretRef::Derived` in the DB provisioning path).
  `ed25519-dalek` dropped from hkask-keystore + hkask-templates Cargo.tomls.
- Validation: workspace check clean (0 errors); 311 + 89 tests pass across
  keystore/types/capability/storage/templates/regulation.
- RuleFrontmatter: kept (operator confirmed always used).

## Follow-up simplifications (2026-07-31, second round)

- **Token ceremony collapse (item #1): COMPLETE.** `DelegationToken` is now a
  plain in-process capability struct — `signature`/`public_key` fields,
  `verify()`/`verify_cryptographic()`, `derive_signing_key`, `TokenSignature`,
  the `SigningPayload`, base64 serialization, attenuation-chain crypto, and
  the Kani tamper-detection harnesses all deleted. `panel_default_token`
  mints directly (no static zeroed key). Registry persistence
  (`token_registry.rs`) writes empty strings to the legacy
  signature_hex/public_key_hex columns (schema unchanged — no migration).
  `with_heal_cb`/`HealCallback` on `RegulationLedger` deleted (zero prod
  callers). Cargo.toml: ed25519-dalek, base64, uuid deps dropped;
  crate description updated. Contract tests rewritten to pin the
  capability-match gate (is_valid_for, allows_*, deterministic id).
- **Hook-collapse (item #2): COMPLETE.** `set_tool_router` and
  `set_metacognition_provider` converted from OnceLock + "already set" warn
  ceremony to plain re-settable Mutex hooks, matching the other 4 kask hooks.
- **Docs (item #4): COMPLETE.** DIVERGENCE.md + .rules D1–D10 → D1–D13;
  `.rules` GuardedStream trap marked RESOLVED.
- Validation: 1084 tests pass across hkask-capability/hkask-storage/
  hkask-regulation/hkask-mcp/hkask-templates/agent; workspace check clean.
  Also landed: `mcp_server_in_scope` free function (operator's per-tab MCP
  scoping pin test needed the helper extracted).

## Deferred items — resolution

- **LedgerObserver subscriber bus: REMOVED.** Deleted `LedgerObserver` trait,
  `DepletionSignal`, `BackpressureSignal` (hkask-types), the `subscribers`
  field, `subscribe`/`subscribe_async`/`publish_event`/`emit_backpressure`,
  `LedgerSink`, and `emit_critical_depletion` (hkask-regulation), plus the
  `emit_backpressure` call in cybernetics_loop and 2 bus tests. The heal
  callback survived as `run_heal_cb` (called on critical alerts). In its
  place, the REAL observability path was closed: composition root now wires
  `NoopEventSink` at startup and upgrades both the CyberneticsLoop and
  McpRuntime governance sinks to `RegulationArchive` on the curator's pod.db
  in the deferred post-login task (new `set_event_sink` setters on both, new
  `open_curator_regulation_archive` bridge helper). The curator MCP server's
  `reg_query`/`curator_algedonic_log` tools — previously reading a DB nothing
  wrote to — now have a producer. Validation: 262 tests pass across
  hkask-types/hkask-regulation/hkask-mcp/kask_bridge; `cargo check -p zed`
  clean.
- **Upstream PRs**: declined by operator.

## Keystore / sovereignty-token question (resolved)

1. **Sovereignty token removal: CONFIRMED complete.** Commit 19c5ca5f80 removed
   a2a_secret threading; remaining self-referential verify gate is C1.
2. **Keystore duplication: CONFIRMED ABSENT.** The `keyring`-injection seam
   described in DIVERGENCE.md D5 no longer exists — `hkask-keystore/keychain.rs`
   uses the `keyring` crate directly everywhere; no OnceLock injection, no
   parallel zed `CredentialsProvider` path. D5 text is stale → fix in B1/C-slices.
   What remains in hkask-keystore (DB passphrase chain, encryption) passed the
   G1 deletion test in the prior audit and stays.

## Risks

| Risk | Mitigation |
|---|---|
| Reverting settings_store skip-if-unchanged reintroduces a re-render loop | Grep showed no observer-loop call sites; tests pin behavior |
| Removing LoggingMemoryPort changes pre-login UX | Call site already tolerates None; behavior identical (turns were dropped either way) |
| Upstream merge conflicts from retained undoc'd seams | B1 documents them before any upstream sync |
| C2 removes a registry a future revocation feature needs | Docs will state "revocation not yet enforced" per .rules; re-add with its consumer |

## Open questions for operator

1. LedgerObserver: register a real observer or delete the bus? (deferred above)
2. Upstream PRs for the three general bug fixes — do you want me to prepare them?
