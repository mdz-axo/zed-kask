# zed-kask Debugging & Code-Improvement Plan

**Date:** 2026-08-06
**Reviewer:** Lead Rust/GPUI engineer (orchestrated hKask skill pass)
**Scope:** `kask/`, the D1–D21 seams in `DIVERGENCE.md`, and kask UI on the Zed side
**Deliverable:** plan only — no code modified in this task

---

## 1. Executive summary

The zed-kask fork is in **strong posture**. The `.rules` traps that historically
caused silent breakage — `set_*` hooks without startup-failure signals,
`background_spawn` of tokio futures, `AsyncApp` captures in `Send + Sync`
traits, kask settings defaults drift, `unwrap_or(0)` on regulation sense
inputs, MCP envelope extraction, `extract_final_step_result` ordering — are
all defended in the current tree. Six parallel evidence-gathering audits
across the scoped surface returned **zero hits** for the highest-severity
mechanical traps and a small, bounded set of lower-severity findings. The
D-seam deviations (D1–D21) are uniformly test-pinned; the named pinning tests
in `DIVERGENCE.md` all resolve to real, compiling test functions.

**Top 3 risks (none blocker-tier):**
1. **Stale `ocap_secret`/`a2a_secret` doc references** in `hkask-keystore` and
   `hkask-inference` advertise removed sovereignty keys — a security-flavored
   advertised-invariant violation that could mislead an operator or auditor
   into believing a security control exists where none does (medium).
2. **`mcp_env()` `embedding_model` `is_empty()` check** in
   `kask_bridge/src/settings.rs:754` violates the "defaults live in `Default`
   impls" rule — default settings always emit `HKASK_EMBEDDING_MODEL`,
   breaking the `mcp_env()` contract and the
   `mcp_env_emits_nothing_for_default_settings` test's intent (medium).
3. **3 unpinned disabling deviations** in supporting files
   (`agent.rs::run_skills_scan` migration, `collab/main.rs` bootstrap gate,
   `kask_extensions_ui` refetch/banner disables) — these are the
   "tests must pin deviations" trap in its live form (medium).

**Top 3 opportunities:**
1. **Delete the `ChunkingStrategy` trait** in `hkask-mcp-corpus` — the only
   remaining single-impl trait in the scoped surface (~40 lines of dead
   abstraction; the second advertised impl never landed).
2. **Consolidate the 6 test-only MCP envelope re-implementations** onto
   `hkask_types::tool_response::parse_tool_response` — mechanical, removes
   the drift risk the 2026-08-02 `extract_workspace_id` panic taught.
3. **Land the `superforecast.rs` `domain_bias_delta` calibration feedback**
   or rewrite the doc to "not yet implemented" — the advertised
   "replaced by data-derived estimates" replacement never landed.

---

## 2. Seam inventory

Every D-seam touched by this review, with the `// zed-kask:` deviations
found and whether each is test-pinned. Verified by grepping for the named
pinning tests in `DIVERGENCE.md`.

| D | Surface | File paths | `// zed-kask:` deviation | Test-pinned? |
|---|---------|------------|--------------------------|-------------|
| D1 | Skill execution | `crates/agent/src/tools/skill_tool.rs`, `crates/agent/src/agent.rs`, `crates/agent_skills/agent_skills.rs`, `crates/zed/src/main.rs`, `kask/crates/hkask-templates/`, `kask/crates/kask_bridge/src/skill_executor.rs` | Manifest cascade instead of body injection; catalog budget disabled; description-length warnings disabled; `SkillSource::Public` marketplace; global skills dir isolated; visibility default `Private` | ✅ all pinned (`test_select_catalog_skills_*`, `test_parse_description_too_long_loads_with_warning`, `test_skill_visibility_defaults_to_private`, `test_adding_remote_skill`, `test_system_prompt_skills_section_describes_manifest_cascade`). **One comment-accuracy issue:** `agent.rs:1325` says "We skip that check entirely" but the size check moved to `extract_skill_frontmatter` (pinned there). |
| D2 | Curator agent | `crates/agent/src/agent.rs`, `crates/agent_ui/src/agent_ui.rs` | `Agent::Curator` variant; `CURATOR_AGENT_ID` | ✅ (addition, not a disable) |
| D3 | hKask tools in-process | `crates/zed/src/main.rs` | `McpRuntime` implements `ToolPort` directly; capability-match gate + gas budgeting | ✅ enforcement point verified at `kask/crates/hkask-mcp/src/runtime.rs:508` (`invoke` → `token.is_valid_for` + `verify_capability_domain` + `charge_call` fail-closed) |
| D4 | Guard layer | `kask/crates/hkask-guard`, `kask/crates/kask_bridge/src/inference.rs` | `GuardedInferencePort` wraps the cascade path; output scanning is post-hoc | ✅ doc at `kask/crates/hkask-guard/src/guarded_inference.rs:15` explicitly says "scanned post-hoc via `GuardedStream` on stream end" |
| D5 | Keychain access | `kask/crates/hkask-keystore` | Uses `keyring` directly; no zed-side seam; a2a/OCAP secret threading removed | ⚠️ **doc drift** — module doc at `hkask_keystore.rs:9` still advertises `a2a_secret` and `ocap_secret` as sovereignty keys (see Finding F-3) |
| D6 | Thread → memory | `crates/agent/src/thread.rs`, `crates/agent/src/agent.rs`, `kask/crates/hkask-types`, `kask/crates/kask_bridge/src/memory.rs` | `MemoryPort` trait; `BridgeMemoryPort` + `RealMemoryPort`; `set_memory_port` (Mutex, re-settable) | ✅ (addition) |
| D7 | App-identity | `crates/paths/src/paths.rs`, `crates/release_channel/src/lib.rs`, `crates/zed/src/zed/mac_only_instance.rs`, `crates/zed/Cargo.toml`, `script/install.sh`, `script/uninstall.sh`, `script/bundle-linux` | `APP_NAME`→`Zed-Kask`, bundle IDs, URL scheme `zed-kask://` | ✅ (addition) |
| D8 | Bridge + adapters | `kask/crates/kask_bridge/` | All ports over zed-kask facilities; channel pattern for GPUI/tokio `Send`+`Sync` | ✅ channel pattern verified — `ToastAlertSink` holds `mpsc::UnboundedSender`, not `AsyncApp` |
| D9 | Settings + credentials | `kask/crates/kask_bridge/src/settings.rs`, `crates/settings_content/src/settings_content.rs`, `crates/settings_ui/src/pages/kask_page.rs`, `crates/settings_ui/src/page_data.rs` | `KaskSettings` struct + `"kask"` section; credentials in keychain under `kask://credentials/<key>` | ⚠️ **one drift** — `mcp_env()` `embedding_model` `is_empty()` check at `settings.rs:754` (see Finding F-1) |
| D10 | (removed) Kask panel | — | `crates/kask_panel/` deleted; `ToolInvoker` moved to `swarm_panel` | ✅ (removal) |
| D11 | `time::format_description::parse` deprecation allow | `crates/git_ui/src/git_graph.rs` | `#[allow(deprecated)]` on two call sites | ✅ (lint suppression, not behavior disable) |
| D12 | OpenAI/Anthropic env var name | `crates/language_models/src/provider/api_compatible.rs` | Strip non-alphanumerics + uppercase instead of `convert_case` | ✅ `test_api_key_env_var_name_kask_contract` |
| D13 | OpenRouter output budget | `crates/open_router/src/open_router.rs`, `crates/language_models/src/provider/open_router.rs`, `crates/open_router/Cargo.toml` | Parse `top_provider.max_completion_tokens` into `Model::max_output_tokens`; send explicit `max_tokens` | ✅ `test_max_completion_tokens_from_api_becomes_request_budget` |
| D14 | Streaming text reveal timer | `crates/acp_thread/src/acp_thread.rs` | `TASK_UPDATE_MS` 16→50 (60fps→20fps) | ✅ `test_streaming_reveal_timer_interval_kask_contract` |
| D15 | Bounded cursor-blink timers | `crates/editor/src/blink_manager.rs` | One restartable resume task; invalidate pending callbacks on disable; only (re)start on disabled→enabled | ✅ `test_pause_blinking_restarts_single_resume_deadline`, `test_disable_cancels_pending_resume`, `test_settings_updates_do_not_accumulate_blink_timers` |
| D16 | App menu rename + Update Zed-Kask | `crates/zed/src/zed/app_menus.rs` | Leftmost menu `name` `"Zed"`→`"z-k"`; `Update Zed-Kask` action | ✅ `test_leftmost_menu_name_is_zk`, `test_leftmost_menu_has_update_zed_kask_item` |
| D17 | GitHub-backed update feed | `crates/auto_update/src/auto_update.rs`, `crates/auto_update/Cargo.toml`, `kask/crates/kask_bridge/src/github_update.rs` | `UpdateZedKask` action; `UpdateFeed::Github`; auto-update defaults false | ✅ `test_update_zed_kask_action_exists`, `test_github_feed_update_flow`, `test_github_feed_no_releases_returns_idle`, `test_auto_update_defaults_to_false`, `test_match_asset_*`, `test_default_github_repo_is_kask` |
| D18 | Media + graph + kanban + portfolio + scenarios block renderer | `crates/markdown/src/markdown.rs`, `crates/agent_ui/src/conversation_view.rs`, `crates/hkask-{media,graph,kanban,portfolio,scenarios}-widget/`, `crates/hkask-viz-core/` | `media_block_renderer: Option<MediaBlockRendererFn>` field; widget impls in zed-kask-side crates | ✅ `test_media_block_renderer_intercepts_media_blocks`, `test_media_block_renderer_falls_through_for_non_media_blocks`, `test_media_block_renderer_intercepts_all_viz_fence_languages`, `selects_event_tree_body`, `falls_through_non_graph_bodies` |
| D19 | Update-progress popup | `crates/auto_update_ui/src/auto_update_ui.rs` | `UpdateProgressNotification` view over D17 status machine | ✅ `progress_popup_gating` |
| D20 | Observed per-call USD cost in `TokenUsage` | `crates/language_model_core/src/language_model_core.rs`, `crates/open_ai/src/open_ai.rs`, `crates/open_ai/src/completion.rs`, `kask/crates/kask_bridge/src/inference.rs` | `cost: Option<f64>` on `TokenUsage`; OpenAI-compatible parses `usage.cost`/`estimated_cost`/`market_cost`; bridge reads into `InferenceResult.cost_usd` | ✅ `test_token_usage_cost_round_trips`, `test_token_usage_cost_adds_and_subs`, `test_map_event_populates_cost_from_usage` |
| D21 | Widget→agent compose-back seam | `crates/agent_ui/src/conversation_view.rs`, `crates/hkask-conversation-injector/` | `ConversationInjector` trait + process-global accessor; `ThreadConversationInjector` pre-fills message editor | ✅ `publish_injector_wires_global_on_activation_and_clears_on_disconnect`, `shared_injector_returns_none_by_default`, `set_then_shared_returns_some`, `set_none_clears_a_prior_injector` |

**Supporting files (carry `// zed-kask:` but not primary seams):**
- `crates/agent/src/tool_router.rs` — `LazyToolRouter` filters MCP only ✅
  (`test_router_only_sees_mcp_candidates_when_built_ins_filtered`,
  `test_apply_router_retains_all_built_ins_unconditionally`)
- `crates/agent/src/thread.rs:4849` — built-in tools bypass router ✅ (same tests)
- `crates/agent_ui/src/conversation_view/thread_view.rs:11502` — disables
  `DescriptionTooLong`/`CatalogBudgetExceeded` render arms ✅ (transitive —
  producers pinned-disabled)
- `crates/collab/src/api/kask_skills.rs` + `db/queries/kask_skills.rs` —
  marketplace API + Ed25519 signing + 120-day expiry ✅
  (`verification_*`, `test_kask_skill_expiry_*`,
  `kask_skill_table_statements_are_idempotent`)
- `crates/kask_extensions_ui/src/publish.rs` — install-time signature +
  expiry verification ✅
  (`install_manifest_verification_*`,
  `manifest_signature_verifies_over_canonical_bytes`)
- `crates/collab/src/main.rs:189` — bootstrap schema gate ⚠️ **unpinned**
  (see Finding F-5)
- `crates/kask_extensions_ui/src/kask_extensions_ui.rs:1171,1364` —
  refetch/banner disables ⚠️ **unpinned** (see Finding F-6)
- `crates/settings_ui/src/settings_ui.rs:4413` — **phantom pin**: comment
  names `test_drain_fires_on_skills_page_leave` which does not exist
  (see Finding F-7)

---

## 3. Findings

Sorted by severity. Constraint force: **Prohibition** (must not do),
**Guardrail** (must do unless documented), **Guideline** (should do),
**Evidence** (test-pinning / verification). IS vs OUGHT per
`pragmatic-semantics`.

| ID | Sev | Force | IS/OUGHT | file:line | Skill | Proposed remedy sketch |
|----|-----|-------|----------|-----------|-------|------------------------|
| F-1 | medium | Guardrail | IS (the code does `is_empty()`; OUGHT is `!= default.field`) | `kask/crates/kask_bridge/src/settings.rs:754` | pragmatic-semantics + code-review | Change `!self.corpus.embedding_model.is_empty()` to `self.corpus.embedding_model != corpus_default.embedding_model` (the `corpus_default` binding exists at L703). Add `assert!(!env.contains_key("HKASK_EMBEDDING_MODEL"))` to `mcp_env_emits_nothing_for_default_settings`. |
| F-2 | medium | Evidence | IS (doc advertises 3 sovereignty keys; OUGHT is 1) | `kask/crates/hkask-keystore/src/hkask_keystore.rs:9` | pragmatic-semantics + kali-audit | Update module doc from `sovereignty keys (a2a_secret, db_passphrase, ocap_secret)` to `sovereignty keys (db_passphrase)`. `a2a_secret`/`ocap_secret` were removed (Appendix A.3). |
| F-3 | medium | Evidence | IS (test doc names removed key); OUGHT is removed | `kask/crates/hkask-inference/src/config.rs:313` and `:548` | pragmatic-semantics + kali-audit | Delete `ocap_secret` from both doc comments (the keystore contract now covers only `db_passphrase`). |
| F-4 | medium | Evidence | IS (unpinned disable); OUGHT is pinned | `crates/agent/src/agent.rs:694` (`run_skills_scan`) | code-review + bug-hunt | Add a test asserting `run_skills_scan` moves an old `~/.agents/skills/<name>` into `global_skills_dir()` and writes the `.migrated` marker, and that a second run is a no-op. |
| F-5 | medium | Evidence | IS (unpinned disable); OUGHT is pinned | `crates/collab/src/main.rs:189` (`setup_app_database` bootstrap gate) | code-review + bug-hunt | Add an integration test asserting a second `setup_app_database` call on an already-bootstrapped SQLite DB is a no-op (no crash, no re-`CREATE TABLE "users"`). |
| F-6 | medium | Evidence | IS (unpinned disables, no test module); OUGHT is pinned | `crates/kask_extensions_ui/src/kask_extensions_ui.rs:1171` (refetch disable) and `:1364` (banner disable) | code-review + bug-hunt | Add a `#[cfg(test)]` module to `kask_extensions_ui`. Assert `refresh_search` does not spawn a network fetch on keystroke; assert the `provides` filter row and upsell banners are absent from `KaskExtensionsPage::render`. |
| F-7 | medium | Evidence | IS (phantom pin); OUGHT is real test or corrected comment | `crates/settings_ui/src/settings_ui.rs:4413` | code-review | Either add `test_drain_fires_on_skills_page_leave` or correct the comment to reference `test_spawn_drain_phase2_noop_clears_queue` in `skills_visibility.rs` (which covers the queue drain, not the page-leave trigger). |
| F-8 | medium | Guardrail | IS (advertised replacement never landed); OUGHT is landed or doc says "not yet" | `kask/mcp-servers/hkask-mcp-scenarios/src/superforecast.rs:1483` (`domain_bias_delta`) | pragmatic-semantics + diagnose | Either wire `compute_calibration_curve`'s bias into `domain_bias_delta`, or rewrite the doc to "Hardcoded δ; data-derived replacement not yet implemented." |
| F-9 | low | Guardrail | IS (trait has 1 impl, never used polymorphically); OUGHT is deleted or 2nd impl landed | `kask/mcp-servers/hkask-mcp-corpus/src/corpus/embed/strategies.rs:14` (`ChunkingStrategy`) | essentialist + refactor-architecture | Delete the trait; inline `WordCountChunker::chunk` as an inherent method (or call `chunk_text` directly in `EmbedService`). The `corpus_chunk` path already bypasses the trait. |
| F-10 | low | Evidence | IS (doc advertises 2nd impl that doesn't exist); OUGHT is corrected | `kask/mcp-servers/hkask-mcp-corpus/src/corpus/embed/strategies.rs:11` (`TokenCountChunker` doc) and `:3` (module doc) and `src/tools/persona/mod.rs:264` | pragmatic-semantics | Delete the `TokenCountChunker` line from the doc, or land the struct + impl. Rewrite the module doc to say `corpus_chunk` uses the `chunk_text` free function. |
| F-11 | low | Evidence | IS (advertised override doesn't exist); OUGHT is landed or doc says "no server overrides" | `kask/mcp-servers/hkask-mcp-server/src/server/tool_span.rs:183` (`record_tool_outcome` doc) | pragmatic-semantics | Either land the condenser override (impl `ToolContext` for `CondenserServer` delegating to `record_experience`), or rewrite the doc to "no server currently overrides this; the macro-generated debug log is the production path." |
| F-12 | low | Evidence | IS (6 test-only re-implementations of envelope unwrap); OUGHT is canonical seam | `kask/mcp-servers/hkask-mcp-codegraph/src/hkask_mcp_codegraph.rs:665,686,700`; `kask/mcp-servers/hkask-mcp-curator/tests/qa_contract.rs:138`; `kask/mcp-servers/hkask-mcp-prediction-markets/src/hkask_mcp_prediction_markets.rs:1846`; `kask/mcp-servers/hkask-mcp-research/tests/research_contract.rs:172` | code-review | Replace each `value.get("content")` / `value["content"]` with `hkask_types::tool_response::parse_tool_response(out).expect(...)`. Mechanical; matches the pattern in `hkask-mcp-kata-kanban/tests/qa_contract.rs:55` and `hkask-mcp-scenarios/tests/scenarios_contract.rs:46`. |
| F-13 | low | Evidence | IS (comment says "skip entirely"; OUGHT is "moved to parse layer") | `crates/agent/src/agent.rs:1325` | code-review | Fix the comment to say "the check is moved to `extract_skill_frontmatter`/`load_skill_frontmatter`; this call site no longer repeats it" instead of "We skip that check entirely." The size check is pinned at the parse layer (`test_oversized_project_skill_reports_error`, `test_load_oversized_skill_file_short_circuits`). |
| F-14 | low | Guardrail | IS (out-of-scope but same trap shape); OUGHT is warn-on-error | `kask/mcp-servers/hkask-mcp-corpus/src/runtime/provider_intel.rs:400,536` (`ledger.transaction_count(&account).unwrap_or(0)`) | pragmatic-cybernetics | Out of strict scope (corpus MCP, not a regulation loop) but same broken-feedback-loop shape. The outer `Ledger::from_driver` match warns, but the inner `transaction_count().unwrap_or(0)` silently swallows a `LedgerError` from the SQL query. Add `tracing::warn!` on the `Err` arm or propagate. Flagged for a follow-up audit if provider-usage counts feed back into any regulation loop. |

**Findings that did NOT surface (traps verified defended):**
- `set_*` hooks without startup-failure signals — all `OnceLock` hooks
  (`set_manifest_executor`, `set_context_injector`, `set_curator_context_injector`)
  have `log::warn!` on the `Err` branch; the `auto_inject` else branch warns
  naming both unwired injectors; the no-default-model else branch warns naming
  all 5 model-dependent hooks.
- `background_spawn` of tokio-dependent futures — zero hits in scope; all kask
  async work routes through `gpui_tokio::Tokio::spawn` (7 sites in `main.rs`).
- `AsyncApp` captures in `Send + Sync` traits — zero hits; `ToastAlertSink`
  holds an `mpsc::UnboundedSender` (the correct pattern).
- `unwrap_or(0)` on regulation sense inputs — zero hits in scope; the
  previously-cited sites (`consolidation_service.rs:142,160`,
  `runtime.rs:309`, `tool_stats.rs:215`) all use explicit `match` +
  `tracing::warn!` with doc comments documenting degradation.
- MCP envelope extraction in production code — zero hits; all production
  callers route through `unwrap_tool_envelope`/`parse_tool_response`. The 6
  re-implementations are all in test code (F-12).
- `OcapConfig` / `ocap:` manifest blocks / `required_capabilities` — all
  removed; only docs/regressions reference them (RR-0040, RR-0041 pin the
  removal). The real gate (`McpRuntime::invoke` at `runtime.rs:508`) is
  verified to enforce `token.is_valid_for` + `verify_capability_domain` +
  `charge_call` fail-closed.
- `GuardedStream` real-time blocking — doc at `guarded_inference.rs:15`
  explicitly says "scanned post-hoc via `GuardedStream` on stream end."
- `extract_final_step_result` HashMap ordering — defended with regression
  test `extract_final_step_result_picks_highest_ordinal` at
  `kask_bridge/src/skill_executor.rs:486`.

---

## 4. Debugging plan

Per non-trivial defect, the cybernetic debugging loop
(reproduce → hypothesize → instrument → fix-sketch → regression-test).
Capped at the defects that warrant a full loop; the rest are mechanical
(F-12, F-13) or doc-only (F-2, F-3, F-10, F-11) and don't need a loop.

### D-1: `mcp_env()` always emits `HKASK_EMBEDDING_MODEL` for default settings (F-1)

- **Reproduction:** construct `KaskSettings::default()`, call `mcp_env()`,
  assert `env.contains_key("HKASK_EMBEDDING_MODEL")` — it will (the
  `is_empty()` check is false because the default is non-empty
  `"DeepInfra/Qwen/Qwen3-Embedding-0.6B"`). The existing
  `mcp_env_emits_nothing_for_default_settings` test passes only because it
  doesn't assert on `HKASK_EMBEDDING_MODEL`.
- **Hypothesis:** the `is_empty()` check was missed when the surrounding
  `embedding_dim`/`ocr_concurrency`/`template_root` checks were updated to
  `!= default.field` during the `Default`-as-source-of-truth refactor.
- **Instrumentation:** add `println!` in `mcp_env()` showing the
  `embedding_model` value and the `is_empty()` result for default settings;
  add `assert!(!env.contains_key("HKASK_EMBEDDING_MODEL"))` to the existing
  test (it will fail).
- **Fix sketch:** change `!self.corpus.embedding_model.is_empty()` to
  `self.corpus.embedding_model != corpus_default.embedding_model` (the
  `corpus_default` binding exists at L703).
- **Regression test:** the updated
  `mcp_env_emits_nothing_for_default_settings` (now asserting
  `HKASK_EMBEDDING_MODEL` is absent for defaults) plus a new test
  `mcp_env_emits_embedding_model_when_overridden` asserting it IS present
  when the user sets a non-default model.

### D-2: Stale `ocap_secret`/`a2a_secret` doc references (F-2, F-3)

- **Reproduction:** `grep -rn "ocap_secret\|a2a_secret" kask/crates/` —
  returns only the 3 doc lines, no `std::env::var` or keychain read.
- **Hypothesis:** the secrets were removed as "self-referential security
  theater" (Appendix A.3) but the doc comments were not updated.
- **Instrumentation:** none needed — this is a doc/advertised-invariant
  defect, not a runtime behavior defect.
- **Fix sketch:** delete `ocap_secret` and `a2a_secret` from the 3 doc
  comments; the keystore contract now covers only `db_passphrase`.
- **Regression test:** add a `grep`-based test (or a doc-test) asserting no
  `kask/crates/` source file references `ocap_secret` or `a2a_secret`
  outside `kask/security/regressions/` and `kask/docs/`. This pins the
  advertised-invariant rule forward.

### D-3: `superforecast.rs` `domain_bias_delta` calibration feedback never landed (F-8)

- **Reproduction:** call `domain_bias_delta("politics")` — returns hardcoded
  `0.3`. Call `compute_calibration_curve` with a series of outcomes showing
  systematic over/under-estimation — the bias is computed but
  `domain_bias_delta` still returns `0.3`.
- **Hypothesis:** the calibration loop computes Brier/bias but does not feed
  back into `domain_bias_delta`; the doc comment advertising "Replaced by
  data-derived estimates when the calibration loop accrues outcomes"
  describes an unimplemented replacement.
- **Instrumentation:** add a `tracing::info!` in `domain_bias_delta`
  showing the hardcoded value vs the computed bias from
  `compute_calibration_curve`; confirm they diverge.
- **Fix sketch (option A — land it):** add a `ForecastStore` lookup in
  `domain_bias_delta` that reads the latest computed bias for the domain
  and returns it if the sample size is above a threshold (e.g., 10
  outcomes); fall back to the hardcoded value otherwise.
- **Fix sketch (option B — doc honesty):** rewrite the doc to "Hardcoded
  δ; data-derived replacement not yet implemented. The calibration loop
  computes bias but does not feed back into this function."
- **Regression test:** if option A, add a test asserting
  `domain_bias_delta` returns the data-derived value after N outcomes;
  if option B, add a doc-test asserting the doc says "not yet implemented."

### D-4: `ChunkingStrategy` single-impl dead abstraction (F-9, F-10)

- **Reproduction:** `grep -rn "dyn ChunkingStrategy\|Box<dyn ChunkingStrategy>\|Arc<dyn ChunkingStrategy>\|: ChunkingStrategy" kask/` — returns only the `impl` line. `EmbedService` imports the trait but constructs `WordCountChunker` concretely and calls `.chunk()`. `corpus_chunk` calls `chunk_text` directly, bypassing the trait.
- **Hypothesis:** the trait was introduced in anticipation of a
  `TokenCountChunker` second impl that never landed; the `corpus_chunk`
  path bypassed it from the start.
- **Instrumentation:** none — this is a dead-code defect.
- **Fix sketch:** delete the `ChunkingStrategy` trait; make
  `WordCountChunker::chunk` an inherent method; update `EmbedService` to
  call it directly (or call `chunk_text` directly, matching `corpus_chunk`).
  Delete the `TokenCountChunker` doc line and fix the module doc + the
  `persona/mod.rs:264` doc.
- **Regression test:** the existing `WordCountChunker` tests still pass
  (they test the method, not the trait). Add a `grep`-based test asserting
  no `kask/` source file defines `ChunkingStrategy` or `TokenCountChunker`.

### D-5: Unpinned `run_skills_scan` migration (F-4)

- **Reproduction:** set up a fake FS with `~/.agents/skills/old-skill/`
  and no `data_dir()/agents/skills/`; call `run_skills_scan`; assert the
  skill moved and `.migrated` exists. Call again; assert no-op.
- **Hypothesis:** the migration is gated by a marker file but has no test
  pinning the gate, so a regression that re-runs the migration (or skips
  it) would be silent.
- **Instrumentation:** add a test-only `Fs` impl; log the migration
  decisions.
- **Fix sketch:** add `test_run_skills_scan_migrates_old_dir_once` and
  `test_run_skills_scan_noop_when_marker_exists` to `agent.rs`'s test
  module.
- **Regression test:** the two new tests.

### D-6: Unpinned `setup_app_database` bootstrap gate (F-5)

- **Reproduction:** call `setup_app_database` twice on an in-memory SQLite
  DB; assert the second call does not crash and does not re-run
  `CREATE TABLE "users"`.
- **Hypothesis:** the gate checks for the `users` table to avoid
  "table already exists" on second run, but no test pins the gate.
- **Instrumentation:** add a `tracing::debug!` in the gate showing the
  table-presence check result.
- **Fix sketch:** add `test_setup_app_database_idempotent_on_second_run`
  to `collab`'s test module.
- **Regression test:** the new test.

### D-7: Unpinned `kask_extensions_ui` disables (F-6)

- **Reproduction:** the crate has no `#[cfg(test)]` module. Render
  `KaskExtensionsPage` and assert the `provides` filter row and upsell
  banners are absent. Type into the search box and assert no network
  fetch task is spawned.
- **Hypothesis:** the disables were applied without tests because the
  crate had no test harness.
- **Instrumentation:** add a `#[cfg(test)]` module with a fake client.
- **Fix sketch:** add `test_render_omits_provides_filter_and_upsell_banners`
  and `test_refresh_search_does_not_fetch_on_keystroke`.
- **Regression test:** the two new tests.

### D-8: Phantom pin `test_drain_fires_on_skills_page_leave` (F-7)

- **Reproduction:** `grep -rn "test_drain_fires_on_skills_page_leave"` —
  returns only the comment at `settings_ui.rs:4413`, no test definition.
- **Hypothesis:** the test was planned (Phase 7) but never landed; the
  comment was left as a placeholder.
- **Instrumentation:** none.
- **Fix sketch (option A):** add the test asserting the queue drains
  when navigating off the Skills sub-page. **(option B):** correct the
  comment to reference `test_spawn_drain_phase2_noop_clears_queue` in
  `skills_visibility.rs` (which covers the queue drain, not the
  page-leave trigger) and note the page-leave trigger is not separately
  pinned.
- **Regression test:** option A — the new test; option B — a grep test
  asserting no comment references a nonexistent test name.

---

## 5. Improvement plan

Per refactor candidate: deletion-test verdict, deepening rationale,
strangler-fig migration steps, validation command.

### R-1: Delete `ChunkingStrategy` trait (F-9, F-10)

- **Deletion-test verdict (essentialist G1):** delete the trait — does
  complexity reappear? No. `WordCountChunker::chunk` becomes an inherent
  method; `EmbedService` calls it directly; `corpus_chunk` already
  bypasses the trait. The trait provides no polymorphism (1 impl, never
  used as `dyn`). **Survives G1.**
- **Deletion-test verdict (essentialist G2):** delete the trait — does
  the surface contract shrink? Yes: removes a public trait + its doc
  contract (which currently advertises a nonexistent 2nd impl). **Survives G2.**
- **Deepening rationale (deep-module):** the trait is a shallow
  abstraction (1 method, 1 impl) with a misleading interface (advertises
  extensibility that doesn't exist). Inlining deepens the
  `WordCountChunker` module by removing the indirection.
- **Strangler-fig migration:** no migration needed — single-step deletion.
  1. Make `WordCountChunker::chunk` an inherent method.
  2. Update `EmbedService` (`service.rs:522`) to call it directly.
  3. Delete the `ChunkingStrategy` trait + the `TokenCountChunker` doc line.
  4. Fix the module doc (`strategies.rs:3`) and `persona/mod.rs:264` doc.
- **Validation:** `cargo test -p hkask-mcp-corpus` and
  `./script/clippy` (per `.rules` build guidelines).

### R-2: Consolidate test-only MCP envelope re-implementations (F-12)

- **Deletion-test verdict (G1):** delete the 6 local
  `value.get("content")`/`value["content"]` re-implementations — does
  complexity reappear? No. The canonical
  `hkask_types::tool_response::parse_tool_response` already provides the
  unwrap. **Survives G1.**
- **Deletion-test verdict (G2):** delete — does the surface contract
  shrink? Yes: removes 6 drift-prone re-implementations; consolidates on
  the single seam. **Survives G2.**
- **Deepening rationale:** the canonical seam is already deep (handles
  the envelope, error cases, and is the single place to fix if the
  envelope format changes). The re-implementations are shallow copies
  that would each need updating independently.
- **Strangler-fig migration:** mechanical, per-file:
  1. `hkask-mcp-codegraph/src/hkask_mcp_codegraph.rs:665,686,700` —
     replace `&v["content"]` with `parse_tool_response(&out).expect(...)`.
  2. `hkask-mcp-curator/tests/qa_contract.rs:138` — replace the
     `v.get("content")` clone-or-fallback with
     `parse_tool_response(out).expect("tool output must be valid JSON")`.
  3. `hkask-mcp-prediction-markets/src/hkask_mcp_prediction_markets.rs:1846`
     — replace `parsed.get("content").expect(...)` with
     `parse_tool_response(&response).expect(...)`.
  4. `hkask-mcp-research/tests/research_contract.rs:172` — replace the
     `parse_content` helper body with `parse_tool_response(out).expect(...)`.
- **Validation:** `cargo test -p hkask-mcp-codegraph -p hkask-mcp-curator -p hkask-mcp-prediction-markets -p hkask-mcp-research` and `./script/clippy`.

### R-3: `mcp_env()` `embedding_model` default-source-of-truth fix (F-1)

- **Deletion-test verdict (G1):** delete the `is_empty()` check — does
  complexity reappear? No. The `!= corpus_default.embedding_model` check
  is the same shape as the surrounding `embedding_dim`/`template_root`
  checks. **Survives G1.**
- **Deepening rationale:** aligns the `mcp_env()` contract ("only
  non-default values are included") with the `Default`-as-source-of-truth
  rule, removing the silent drift where default settings emit env vars.
- **Strangler-fig migration:** single-step:
  1. Change the check at `settings.rs:754`.
  2. Add `assert!(!env.contains_key("HKASK_EMBEDDING_MODEL"))` to
     `mcp_env_emits_nothing_for_default_settings`.
  3. Add `mcp_env_emits_embedding_model_when_overridden`.
- **Validation:** `cargo test -p kask_bridge` and `./script/clippy`.

### R-4: Stale sovereignty-key doc cleanup (F-2, F-3)

- **Deletion-test verdict (G1):** delete the stale references — does
  complexity reappear? No. The keys don't exist. **Survives G1.**
- **Deepening rationale:** removes advertised-invariant theater (the
  `.rules` trap: a doc comment claiming a security property must point
  to the enforcement point; here it points to a removed enforcement
  point).
- **Strangler-fig migration:** single-step doc edits + a grep-based
  regression test pinning the removal forward.
- **Validation:** `cargo test -p hkask-keystore -p hkask-inference` and
  `./script/clippy`. The grep test (D-2 regression test) runs as a
  doc-test or a `cargo test` with a `grep` assertion.

### R-5: `superforecast.rs` `domain_bias_delta` — land or document (F-8)

- **Deletion-test verdict (G1):** delete the "Replaced by data-derived
  estimates" claim — does complexity reappear? No (option B). Or land
  the replacement (option A). Both survive G1.
- **Deepening rationale:** option A deepens the calibration loop by
  closing the feedback (bias → delta → forecast → bias). Option B
  removes a misleading advertised invariant.
- **Strangler-fig migration:**
  - **Option A:** add a `ForecastStore` lookup in `domain_bias_delta`
    with a sample-size threshold; fall back to hardcoded. Add a test.
  - **Option B:** rewrite the doc to "not yet implemented." Add a
    doc-test.
- **Validation:** `cargo test -p hkask-mcp-scenarios` and
  `./script/clippy`.

### R-6: Pin the 3 unpinned disabling deviations (F-4, F-5, F-6)

- **Deletion-test verdict:** N/A — these are test additions, not
  deletions. The deletion test applies to the production code (already
  verified to survive — the disables are deliberate).
- **Deepening rationale:** pins the `.rules` "tests must pin deviations"
  trap forward; prevents silent regression of the disables.
- **Strangler-fig migration:** per-file test additions (see D-5, D-6,
  D-7 above).
- **Validation:** `cargo test -p agent -p collab -p kask_extensions_ui`
  and `./script/clippy`.

### R-7: Fix the phantom pin comment (F-7)

- **Deletion-test verdict (G1):** delete the phantom pin comment — does
  complexity reappear? No. Either the test lands (option A) or the
  comment is corrected (option B). **Survives G1.**
- **Deepening rationale:** removes a misleading comment that names a
  nonexistent test (an advertised-invariant violation at the test level).
- **Strangler-fig migration:** single-step.
- **Validation:** `cargo test -p settings_ui` and `./script/clippy`.

### R-8: Fix the `agent.rs:1325` comment accuracy (F-13)

- **Deletion-test verdict (G1):** delete the "skip entirely" claim —
  does complexity reappear? No. The check moved to the parse layer
  (pinned there). **Survives G1.**
- **Deepening rationale:** removes a comment that contradicts the code
  (the size check IS enforced, just at a different layer).
- **Strangler-fig migration:** single-step comment edit.
- **Validation:** `cargo test -p agent` (the existing
  `test_oversized_project_skill_reports_error` and
  `test_load_oversized_skill_file_short_circuits` pin the moved check).

### R-9: `record_tool_outcome` doc — land or document (F-11)

- **Deletion-test verdict (G1):** delete the condenser example — does
  complexity reappear? No (option B). Or land the override (option A).
  Both survive G1.
- **Deepening rationale:** option A deepens the `ToolContext` trait by
  adding a real override (the condenser's `record_experience`). Option B
  removes an advertised override that doesn't exist.
- **Strangler-fig migration:**
  - **Option A:** impl `ToolContext` for `CondenserServer` with
    `record_tool_outcome` delegating to `record_experience`. Add a test.
  - **Option B:** rewrite the doc to "no server currently overrides
    this; the macro-generated debug log is the production path."
- **Validation:** `cargo test -p hkask-mcp-server -p hkask-mcp-condenser`
  and `./script/clippy`.

---

## 6. Deferred / hypothesis-tier

Findings that need user verification before any action. **Do not mutate
on speculation.**

| ID | Item | Why deferred |
|----|------|--------------|
| H-1 | F-14 (`provider_intel.rs:400,536` `unwrap_or(0)`) | Out of strict scope (`hkask-mcp-corpus`, not a regulation loop). Needs verification: do provider-usage counts feed back into any regulation loop (`CyberneticsLoop`, `MetacognitionLoop`, `ConsolidationService`)? If yes, this is the same broken-feedback-loop trap and needs the `tracing::warn!` + doc treatment. If no, it's a non-regulation sense input and the `unwrap_or(0)` is acceptable with a doc comment. **Action:** user/curator to confirm whether `transaction_count` feeds a regulation loop. |
| H-2 | F-8 (option A vs B for `domain_bias_delta`) | Landing the calibration feedback (option A) is a behavior change to the superforecast pipeline; documenting (option B) is a doc change. The choice depends on whether the calibration loop is trusted enough to override the hardcoded δ. **Action:** user to decide A or B based on calibration-loop maturity. |
| H-3 | F-11 (option A vs B for `record_tool_outcome`) | Landing the condenser override (option A) is a real wiring change; documenting (option B) is a doc change. **Action:** user to decide whether the condenser should override `record_tool_outcome` or keep its separate `record_experience` path. |
| H-4 | F-7 (option A vs B for phantom pin) | Landing the test (option A) is the right fix if the page-leave drain behavior is load-bearing; correcting the comment (option B) is right if the queue drain is already covered by `test_spawn_drain_phase2_noop_clears_queue`. **Action:** user to confirm whether the page-leave trigger is separately load-bearing. |
| H-5 | `kask_extensions_ui` test harness (F-6) | Adding a `#[cfg(test)]` module to a crate with no tests is a scaffolding decision. The crate renders GPUI elements, so the tests need a `TestAppContext` / `VisualTestContext` harness. **Action:** user to confirm the test harness approach (and whether `kask_extensions_ui` should depend on `gpui`'s test helpers). |

---

## 7. Validation

The exact commands to confirm the plan is sound. **Honesty note:** this
was a review-only task (`fix_mode = none`); no code was modified, so the
"did it pass" question doesn't apply. The commands below are what to run
**after** implementing each finding, plus the commands I ran during the
review to verify the traps are defended.

### Commands run during this review (evidence-gathering, read-only)

| Command | Purpose | Result |
|---------|---------|--------|
| `sed -n '1,102p' DIVERGENCE.md` | Read the seam inventory | ✅ D1–D21 + supporting files |
| `grep -rn "// zed-kask:" crates/ kask/` | Find all deviations | ✅ full list (paginated) |
| `grep -rn "set_manifest_executor\|set_context_injector\|..." crates/agent/src/agent.rs crates/zed/src/main.rs kask/crates/kask_bridge/src/` | Audit `set_*` hooks for startup-failure signals | ✅ all `OnceLock` hooks have `log::warn!` on `Err` branch |
| `sed -n '1200,2050p' crates/zed/src/main.rs` | Read the composition root wiring | ✅ `auto_inject` else branch warns naming both injectors; no-default-model else branch warns naming all 5 hooks |
| `grep -rn "ocap:\|OcapConfig\|required_capabilities" kask/ crates/` | Verify `ocap:` removal | ✅ only docs/regressions reference them (RR-0040, RR-0041) |
| `grep -rn "fn invoke" kask/crates/hkask-mcp/src/runtime.rs` + `sed -n '508,580p'` | Verify `McpRuntime::invoke` enforcement point | ✅ `token.is_valid_for` + `verify_capability_domain` + `charge_call` fail-closed |
| `grep -rn "post-hoc\|GuardedStream" kask/crates/hkask-guard/src/` | Verify `GuardedStream` doc | ✅ `guarded_inference.rs:15` says "scanned post-hoc" |
| `grep -rn "extract_final_step_result\|values().last()" kask/crates/kask_bridge/src/skill_executor.rs kask/crates/hkask-templates/src/executor.rs` | Verify ordinal-keyed extraction | ✅ defended with regression test |
| 6 parallel sub-agent audits | `background_spawn`/tokio, settings defaults, trait-with-one-impl, test-pinning, `unwrap_or(0)`, MCP envelope | ✅ results synthesized into §3 |

### Commands NOT run during this review (and why)

| Command | Why not |
|---------|--------|
| `./script/clippy` | No code modified — clippy on the current tree is the baseline, not a validation of this plan. Run after implementing. |
| `cargo test -p <crate>` (any) | Same — no code modified. Run after implementing. |
| `cargo check -p kask_bridge -p hkask-types -p hkask-mcp-server` | Same. |
| `bash kask/scripts/check-hkask-no-zed-deps.sh` | Same — verifies the §13.1 invariant after a change; no change made. |

### Commands to run AFTER implementing each finding

| Finding(s) | Command |
|------------|---------|
| F-1 (mcp_env embedding_model) | `cargo test -p kask_bridge mcp_env` + `./script/clippy` |
| F-2, F-3 (stale sovereignty-key docs) | `cargo test -p hkask-keystore -p hkask-inference` + `./script/clippy` + the new grep-based regression test |
| F-4 (run_skills_scan pin) | `cargo test -p agent run_skills_scan` + `./script/clippy` |
| F-5 (setup_app_database pin) | `cargo test -p collab setup_app_database` + `./script/clippy` |
| F-6 (kask_extensions_ui pins) | `cargo test -p kask_extensions_ui` + `./script/clippy` |
| F-7 (phantom pin) | `cargo test -p settings_ui` + `./script/clippy` |
| F-8 (domain_bias_delta) | `cargo test -p hkask-mcp-scenarios domain_bias` + `./script/clippy` |
| F-9, F-10 (ChunkingStrategy delete) | `cargo test -p hkask-mcp-corpus` + `./script/clippy` |
| F-11 (record_tool_outcome) | `cargo test -p hkask-mcp-server -p hkask-mcp-condenser` + `./script/clippy` |
| F-12 (test envelope consolidation) | `cargo test -p hkask-mcp-codegraph -p hkask-mcp-curator -p hkask-mcp-prediction-markets -p hkask-mcp-research` + `./script/clippy` |
| F-13 (agent.rs:1325 comment) | `cargo test -p agent oversized` (existing tests pin the moved check) + `./script/clippy` |
| F-14 (provider_intel — deferred H-1) | `cargo test -p hkask-mcp-corpus provider_intel` (after user confirms the regulation-loop feedback question) |
| All findings (final gate) | `./script/clippy` (per `.rules` build guidelines — prefer over `cargo clippy`) + `bash kask/scripts/check-hkask-no-zed-deps.sh` (verify §13.1 invariant) + `cargo check -p kask_bridge -p hkask-types -p hkask-mcp-server` (the DIVERGENCE.md upstream-sync runbook step 5) |

---

## 8. Suggested `.rules` additions

Only for non-obvious, repeatedly-hit, specific patterns discovered during
this review. **Do not edit `.rules` inline** — reviewers decide what gets
merged. These are proposals for the PR description's "Suggested .rules
additions" section.

### S-R1: `mcp_env()` string-field checks must compare against `Default`, not `is_empty()`

**Proposed text:**

> `mcp_env()` string-field inclusion checks must compare against
> `Default::default().field`, not `!field.is_empty()`. A string field
> whose `Default` is non-empty (e.g. `corpus.embedding_model` =
> `"DeepInfra/Qwen/Qwen3-Embedding-0.6B"`) will always pass `!is_empty()`
> for default settings, emitting an env var the `mcp_env()` contract
> promises to omit. The numeric/bool fields already follow this rule
> (they compare against `Default::default().field`); the string fields
> must match. The `mcp_env_emits_nothing_for_default_settings` test must
> assert every `HKASK_*` env var is absent for default settings, not just
> the numeric ones. Found in `kask_bridge/src/settings.rs:754`
> (`embedding_model`); the surrounding `embedding_dim`/`ocr_concurrency`/
> `template_root` checks were already correct, so the drift was a
> single-field miss during the `Default`-as-source-of-truth refactor.

**Meets the 3 criteria:**
1. **Non-obvious:** the `is_empty()` check looks correct in isolation;
   the bug is that the `Default` is non-empty.
2. **Repeatedly encountered:** the surrounding fields were already fixed
   for the same trap; this one was missed. The pattern recurs for any
   string field with a non-empty default.
3. **Specific:** names the file, the field, the test, and the fix shape.

### S-R2: Removed security controls need doc-comment + grep-test cleanup in the same commit

**Proposed text:**

> When a security control is removed (e.g. `ocap_secret`/`a2a_secret`
> threading, `OcapConfig`, `required_capabilities`), grep the crate's
> doc comments for the removed symbol in the same commit. A doc comment
> that advertises a removed security control is an advertised-invariant
> violation (the `.rules` "Advertised invariants need enforcement points"
> trap) — an operator or auditor reading the doc believes the control
> exists. The cleanup is mechanical: `grep -rn "<removed-symbol>"
> <crate>/src/` and delete or rewrite each hit. Add a grep-based
> regression test (or doc-test) asserting no `src/` file references the
> removed symbol outside `security/regressions/` and `docs/`. Found in
> `hkask-keystore/src/hkask_keystore.rs:9` and
> `hkask-inference/src/config.rs:313,548` — all 3 still advertise
> `ocap_secret`/`a2a_secret` as sovereignty keys after the
> Appendix A.3 removal.

**Meets the 3 criteria:**
1. **Non-obvious:** the removal commit removed the code but not the docs;
   the docs are not enforced by the compiler.
2. **Repeatedly encountered:** 3 doc comments in 2 crates, all
   referencing the same removed symbols.
3. **Specific:** names the symbols, the files, the grep command, and the
   regression-test shape.

### S-R3: Test-pin comments must reference tests that exist

**Proposed text:**

> A `// Pinned by <test_name>` comment must reference a test that
> exists. A phantom pin (the comment names a test that was never landed
> or was renamed) is worse than no comment — it asserts a pin that
> doesn't exist, so a regression of the pinned behavior is silent. When
> adding a `// Pinned by ...` comment, add the test in the same commit.
> When renaming or deleting a test, grep for `Pinned by <old_name>` and
> update the comments in the same commit. Found in
> `crates/settings_ui/src/settings_ui.rs:4413` — names
> `test_drain_fires_on_skills_page_leave` (Phase 7 placeholder) which
> does not exist; the actual drain coverage is
> `test_spawn_drain_phase2_noop_clears_queue` in `skills_visibility.rs`.
> A grep-based CI check (`grep -rn "Pinned by " crates/ kask/ | awk
> '{print $3}' | sort -u | xargs -I{} grep -l "fn {}" crates/ kask/`)
> would catch phantom pins mechanically.

**Meets the 3 criteria:**
1. **Non-obvious:** the comment looks authoritative; the test name is
   plausible.
2. **Repeatedly encountered:** this is the "tests must pin deviations"
   trap's doc-level analog; the same shape would recur for any
   placeholder test name.
3. **Specific:** names the file, the phantom test, the real test, and a
   mechanical CI check.

---

**End of plan.** No code was modified. Implementation is a separate,
consented step. The 8 sections are complete; the validation section
honestly reports run vs not-run.
