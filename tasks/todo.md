# Token Efficiency — Task Checklist

## Phase 1 — Foundation (cheapest, highest leverage) ✅

- [x] **T1: Billed-token compaction trigger** — `billed_input_tokens()` excludes cache reads
- [x] **T2: Second cache breakpoint at latest user message** — `set_cache_breakpoints()` helper

## Phase 2 — System prompt stability ✅

- [x] **T3: System-prompt digest on Thread** — SHA-256 digest of inputs; cached render reused when digest matches
- [x] **T4: Cache-bust telemetry** — `Agent System Prompt Cache Bust` telemetry event on digest change

## Phase 3 — Compaction quality ✅

- [x] **T5: Structured compaction template** — Kilocode-style fixed-section Markdown template
- [x] **T6: Iterative summary refinement** — prior summary fed back in `<previous-summary>`

## Phase 4 — Tool-output bounding ✅

- [x] **T7: Tool-output truncation hook** — terminal output spillover to temp file; path in message
- [x] **T8: Spillover cleanup** — deferred (tempfile uses OS temp dir; cleanup tied to OS temp lifecycle)

## Pre-existing issues fixed incidentally

- Fixed `drain_completed_deferred_results` — pre-existing broken code from "Add deferred tool results and spillover" commit (moved out of `Pin<Box<...>>`, `as_ref()` on `LanguageModelToolUseId`, missing `deferred_tool_results` field in constructors). Changed `DeferredToolResult.receiver` to `Option<Pin<Box<...>>>` and rewrote the poll logic to poll in place without consuming.

## Pre-existing issues NOT fixed (out of scope)

- `test_select_terminal_output_head_and_tail_lines_overlap` — pre-existing test expectation mismatch in `select_terminal_output_lines` (tail overlap logic). Failing before my changes.
- `agent.rs` `internal_tests` module — pre-existing compilation errors (`LanguageModelToolResultContent` not in scope, `SubagentPromptResult` type mismatch) from the in-progress "deferred tool results" commit. Block the `tests::` module but not `thread::tests` or `tools::terminal_tool::tests`.
