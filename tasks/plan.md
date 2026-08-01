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

## Deferred (needs operator decision — NOT in this plan)

- **LedgerObserver subscriber bus** (~100 lines in hkask-regulation): main.rs:620
  comment claims registered observers "actually observe" ledger events; zero
  production registrations exist. Either register one (curator status surface)
  or delete `LedgerObserver` + bus + `LedgerSink`. Requires product decision.
- **Upstreaming**: trusted_worktrees warn-and-skip, settings_store skip-if-unchanged,
  terminal head/tail overlap fix — file upstream PRs; revert landed here in A2/A3.

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
