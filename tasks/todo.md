# Token Efficiency — Task Checklist

## Phase 1 — Foundation (cheapest, highest leverage)

- [ ] **T1: Billed-token compaction trigger**
  - Add `billed_input_tokens(usage) = input_tokens + cache_creation_input_tokens`
  - Use it in `compaction_message_target_ix` (thread.rs ~L4450)
  - Keep `total_input_tokens` for the context-window-overflow warning
  - AC: high `cache_read_input_tokens` does NOT trigger compaction; billed tokens do
  - Files: `crates/agent/src/thread.rs`
  - Scope: S

- [ ] **T2: Second cache breakpoint at latest user message**
  - In `build_request_messages_until`, set `cache: true` on the latest user message (not just the last message)
  - AC: `test_prompt_caching` updated; latest user message carries `cache: true` even when followed by tool results
  - Files: `crates/agent/src/thread.rs`, `crates/agent/src/tests/mod.rs`
  - Scope: S

**Checkpoint 1**: `./script/clippy` clean; `test_prompt_caching` + compaction tests pass; new test for billed-token trigger.

## Phase 2 — System prompt stability

- [ ] **T3: System-prompt digest on Thread**
  - Hash rendered system prompt + sorted available_tools; store digest on `Thread`
  - Skip re-render (reuse cached string) when digest matches
  - AC: digest stable across turns with no changes; busts on tool/skill/rules/date change
  - Files: `crates/agent/src/thread.rs`, `crates/agent/src/templates.rs`
  - Scope: M

- [ ] **T4: Cache-bust telemetry**
  - Emit telemetry event when system-prompt digest changes across turns
  - AC: telemetry event fires on tool add; does not fire on no-op turn
  - Files: `crates/agent/src/thread.rs`
  - Scope: S

**Checkpoint 2**: `./script/clippy` clean; system prompt tests pass; new digest-stability test.

## Phase 3 — Compaction quality

- [ ] **T5: Structured compaction template**
  - Replace `COMPACTION_PROMPT` with fixed-section Markdown template (Goal / Constraints / Progress / Key Decisions / Next Steps / Critical Context / Relevant Files)
  - AC: compaction request uses new template; existing compaction tests updated
  - Files: `crates/agent/src/thread.rs` (or a new const/template)
  - Scope: S

- [ ] **T6: Iterative summary refinement**
  - When prior `CompactionInfo::Summary` exists, feed it back in `<previous-summary>` with "preserve still-true, remove stale, merge new"
  - Carry prior `recent` tail into new compaction context
  - AC: second compaction refines first; `recent` tail preserved verbatim
  - Files: `crates/agent/src/thread.rs`
  - Scope: M

**Checkpoint 3**: `./script/clippy` clean; compaction tests pass with new template + refinement.

## Phase 4 — Tool-output bounding (complementary condenser)

- [ ] **T7: Tool-output truncation hook**
  - Add bounded-preview for terminal results exceeding byte/line budget
  - Use `hkask-condenser` `RtkStyleAlgorithm` logic (vendor if workspace boundary blocks dep)
  - Write full output to per-thread temp file; replace in-message with head/tail + marker path
  - AC: 100KB terminal output truncated to budget with valid spillover path; full output recoverable
  - Files: `crates/agent/src/tools/` (terminal tool), possibly `crates/agent/src/thread.rs`
  - Scope: M

- [ ] **T8: Spillover cleanup**
  - Retention/cleanup for spillover files (thread-drop sweep or hourly 7-day TTL)
  - AC: no leaked spillover files after thread drop
  - Files: `crates/agent/src/thread.rs` or terminal tool
  - Scope: S

**Checkpoint 4**: `./script/clippy` clean; new truncation test passes; spillover path readable.
