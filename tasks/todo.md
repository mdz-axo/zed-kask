# Token Efficiency — Task Checklist

## Phase 1 — Foundation (cheapest, highest leverage) ✅

- [x] **T1: Billed-token compaction trigger**
  - Added `billed_input_tokens(usage) = input_tokens + cache_creation_input_tokens`
  - Used it in `compaction_message_target_ix`
  - Kept `total_input_tokens` for context-window-overflow warning
  - AC met: `test_compaction_ignores_cache_read_tokens` passes — high cache reads do NOT trigger compaction
  - Files: `crates/agent/src/thread.rs`
  - Scope: S

- [x] **T2: Second cache breakpoint at latest user message**
  - Extracted `set_cache_breakpoints()` helper; called from `build_request_messages` AFTER pending-message extension
  - Marks latest user message + last message
  - AC met: `test_prompt_caching` + `test_building_request_with_pending_tools` updated and pass
  - Files: `crates/agent/src/thread.rs`, `crates/agent/src/tests/mod.rs`
  - Scope: S

**Checkpoint 1**: ✅ `./script/clippy` clean; 4 prompt/caching tests + 13 compaction tests pass.

## Phase 2 — System prompt stability (deferred)

- [ ] **T3: System-prompt digest on Thread** — deferred; lower leverage than Phase 3
- [ ] **T4: Cache-bust telemetry** — deferred with T3

## Phase 3 — Compaction quality ✅

- [x] **T5: Structured compaction template**
  - Replaced `compaction_prompt.txt` with Kilocode-style fixed-section Markdown template (Goal / Constraints / Progress / Key Decisions / Next Steps / Critical Context / Relevant Files)
  - AC met: existing compaction tests pass with new template
  - Files: `crates/agent_settings/src/prompts/compaction_prompt.txt`
  - Scope: S

- [x] **T6: Iterative summary refinement**
  - `build_compaction_request` now finds prior `CompactionInfo::Summary` before `insertion_ix`
  - Feeds it back wrapped in `<previous-summary>` with "Preserve still-true, remove stale, merge new" instruction
  - AC met: `test_compaction_refines_prior_summary` passes
  - Files: `crates/agent/src/thread.rs`
  - Scope: M

**Checkpoint 3**: ✅ `./script/clippy` clean; 13 compaction tests pass (including 2 new ones).

## Phase 4 — Tool-output bounding (deferred)

- [ ] **T7: Tool-output truncation hook** — deferred; requires workspace-boundary check for `hkask-condenser` dep
- [ ] **T8: Spillover cleanup** — deferred with T7
