---
title: "Agent Loop Guardrails — Implementation Plan (Runaway Protection, Repetition Stop-Loss, lisp_eval Teaching Fix)"
audience: [architects, developers, product]
last_updated: 2026-10-02
version: "0.1.0"
status: "Proposed"
domain: "Composition"
mds_categories: [composition, lifecycle, trust]
---

# Agent Loop Guardrails — Implementation Plan

## 1. Purpose and status

This is the implementation plan for the two adoption candidates from the
2026-10-02 FrontierAgent comparative architecture review (verdict: 2
candidates — C1 LLM-call runaway protection, C2 cross-turn repetition
stop-loss), plus the `lisp_eval` teaching-error fix that the review's own
process incident surfaced and reproduced.

It is a **plan, not an implementation record**. No implementation is
authorized by this document alone; the operator charters each workstream
(§13).

**Execution record.** Chartered 2026-10-02 ("proceed with plan
execution"). Slice 0 (lisp_eval TypeError teaching) and Slice 1 (C2
repetition stop-loss) landed the same day — RED→GREEN receipts in §9, one
design revision recorded in §7 (committed as `1d4bff6fb2`). Slice 2 (C1a
reasoning-runaway watchdog) landed the same day: both bounds checked per
delta (no timer race needed while reasoning flows — the stalled-stream
case is the deferred stall bound), state-machine unit suite + a
source-structure wiring pin green, clippy `-D warnings` clean. Deferred
from Slice 2, recorded in DIVERGENCE.md: the first-chunk/stall bounds and
the settings keys (the behavioral turn-level test landed 2026-10-02 after
the rebuild as `test_reasoning_runaway_watchdog_aborts_thinking_only_stream`).
Slice 3 (C1b recovery ladder) — charter dropped 2026-10-02 by the plan audit below (zero live occurrences of its motivating signature since D42 removed its root cause).

**C1a removed 2026-10-02 (operator directive, the same day it landed).**
Three live false positives killed working turns in one day — a legitimate
sub-agent's 123-second reasoning phase (the 120 s time bound, removed
separately first), the operator's working thread (token bound), and the
analysis turn examining that abort (token bound). The token bound
(16384 estimated reasoning tokens) sits inside the legitimate operating
range of this platform's own reasoning models: a frontier model working a
hard problem emits 16k+ reasoning tokens before its first visible
output, so reasoning length cannot distinguish "stuck" from "working" —
the §11 falsifier fired in the harmful direction, and the grill-me
answer ("fires only when reasoning is flowing and nothing else arrives")
had conflated the mechanism's window with its legitimacy. The abort was
also fatal with no recovery path (C1b was never built) and no settings
knob, so a misfire lost the whole turn. Removed in full: `kask_runaway.rs`,
the thread.rs wiring and `CompletionError::ReasoningRunaway`, the
behavioral pin, and the DIVERGENCE.md entry; the deferred
first-chunk/stall bounds and settings keys are void with it. The floor
that remains: the model's own `max_tokens` cap ends a true runaway, and
D43 names the zero-content MaxTokens signature in the log. C1b's charter
was dropped the same day by the audit below. Do not re-land C1a without a
content-level discriminator (looping vs. progressing), a non-fatal
recovery path, and a settings knob.

**Plan audit (2026-10-02, operator-directed, post-C1a-removal).** Every
remaining component was re-evaluated on the axes a guard's danger class
turns on: what a misfire costs, whether the trigger discriminates the
failure mode from legitimate work, and whether it has fired live.
Findings: (1) **C2's shipped code did not implement its documented
contract** — the docs, hint/refusal texts, and the Lean spec all say
"consecutive", but the call-level revision kept a per-key map with no
reset on a different dispatch, so interleaved identical calls
accumulated: the edit → verify → edit → verify loop would be refused at
its 6th verification as a "zero-gain loop". Fixed the same day —
`check_repetition` now clears the key's streak when a different key was
dispatched since its last success (reset-on-distinct; the Lean spec's
`nextStreak_distinct_resets` now describes the shipped behavior),
pinned by `interleaved_identical_successes_do_not_accumulate` and
`intervening_dispatch_resets_the_streak`. Zero live firings before the
fix (the guard had landed hours earlier). (2) **C1b's charter is
dropped**: its motivating failure (zero-content MaxTokens) had its root
cause removed by D42, and the observable logs carry zero D43
`silent-stop` lines — no observed failure, no mechanism. Re-charter bar:
a live D43 zero-content line. (3) **lisp_eval teaching kept**:
diagnostic-only, locally motivated (three live receipts),
discriminated, negative controls pinned. The systemic lesson, on record:
the components built from borrowed evidence (C1a's bounds, C2's 71%
motivation) were the miscalibrated ones; the one component built from
local evidence is the one that works.

**Provenance.** Patterns are translated at the architecture level from
FrontierAgent (Apache-2.0, v0.1.0 unreleased, active — last commit 2026-10-02)
and its pinned engine `apodex-agent-core==0.12.2` (PyPI; engine mechanisms
attributed to the library, composition choices to the repo). No code is
copied. The review's full evidence record is the session log; load-bearing
citations are repeated here.

## 2. Direction (kata step 1)

- The operator's own ruling is on record (DIVERGENCE.md D42, 2026-09-04):
  *"timeouts are the enforcement/kill mechanism for runaway or looping
  processes"* — the named mechanism does not exist on the LLM completion
  path.
- The review adopted C1 and C2 as proposals on the axis of failure-mode
  coverage (measured failure modes on the source side, grep-verified
  absence on the zed-kask side).
- Direction: bound the two measured failure modes of long-horizon agent
  loops — runaway generation and identical-call death spirals — without
  breaking the interactive contract (user cancel remains the human remedy;
  deliberately no tight sub-agent wall-time ceilings, per the source's own
  52.8%-killed counter-evidence).

## 3. Current condition (kata step 2 — measured, cited)

### C1 gaps

- **Zero timeouts on the completion path.** `crates/language_model/src`
  has no `timeout` matches; `crates/language_models/src/provider/bedrock.rs:744-748`
  explicitly disables both `TimeoutConfig` and stalled-stream protection;
  the only provider timeouts are model-discovery fetches
  (`opencode.rs:80-84`). Tool side is covered (terminal `timeout_ms`,
  context-server request timeouts) — the LLM stream is the one unbounded
  wait.
- **Detection without recovery.** `crates/agent/src/thread.rs:4370-4379`:
  `Stop(StopReason::MaxTokens)` → `on_completion_truncated()` (D25 flag) →
  `Err(CompletionError::MaxTokens)` — the turn dies. D43 added the
  zero-content *warning* (`max_tokens_turn_end_warning` distinguishes
  "prompt → silence → nothing") but the arm remains warn-and-fail.
- **Source evidence.** `agent_core/loop_types.py:142-154` (LoopConfig:
  `first_chunk_timeout`, `reasoning_only_timeout_s`,
  `reasoning_only_max_tokens`, `logical_call_timeout_s`); the stream
  watchdog raising `LLMReasoningRunaway { elapsed_s, estimated_tokens,
  trigger, partial_response }` and closing the HTTP stream
  (`agent_core/runtime/loop/_streaming.py:452-485`); the capped-empty
  ladder with transient user-turn reminders that never enter durable
  history (`agent_core/runtime/loop/_runaway.py:18-72`); armed in all six
  shipped profiles at 120 s / 16384 tokens / 900 s
  (`workflows/agent_team/profiles/benchmark.yaml:69-71` and the five
  siblings). First-chunk default is **0.0 = off**
  (`_streaming.py:77-78`) — cold local-model loads are legitimate.

### C2 gaps

- `crates/agent/src/tool_retry_tracker.rs` tracks **failures** only:
  `check` (L144-184) consults per-input and per-tool failure counters;
  `record_success` (L239-249) resets both. Identical *successful* calls are
  never flagged anywhere (verified by `repetition|duplicate|consecutive`
  sweeps over `crates/agent/src`).
- Source measurement: **71% of sub-agents that exhausted their turn budget
  did so inside a run of ten or more consecutive byte-identical calls,
  median 87, worst case 198 of 200**
  (`agent_core/components/observers/repetition_guard.py:33-38`).

### lisp_eval

- **Three live receipts in one session (2026-10-02)**: `lisp_eval` calls
  with every scalar env value wrapped as an object (`{"n": 0}`, `{"b":
  true}`, `{"list": []}`) instead of the bare scalar; each failed with
  `type error: expected number, got list` (`LispError::TypeError`,
  `kask/crates/hkask-lisp/src/hkask_lisp.rs:86-91`, raised per-value at
  `as_f64` L907-914). Two hard-cap refusals (5 identical calls each), and
  the third receipt occurred **under explicit written deliberation to emit
  bare scalars** — the wrapping is deterministic for this emitter and
  deliberation does not override it.
- The tool's error path gives `TypeError` no teaching
  (`crates/agent/src/tools/lisp_eval_tool.rs:253-269`): only
  `UnboundSymbol` gets the sorted-bindings + flatten-fix treatment; the
  type error falls through `other => other.to_string()`.
- **Elevated blast radius**: scalar-env `lisp_eval` forms are the
  deterministic gates of at least six skills (grill-me feedback gate,
  kata-improvement check forms, region-routing invariant, coding-guidelines
  audit gate, essentialist score, metacognition gap arithmetic). The
  unteaching error blocks the skill system's D-layer for this emitter
  class, not just one call. Filed to the curator:
  `skill_use_issue:hypothesis-framer` and `skill_use_issue:region-routing`.

## 4. Target condition (kata step 3)

Observable criteria — each is a test or a live check, not an intention:

1. **[C1a]** A fake-model stream emitting only thinking deltas is aborted
   within the reasoning-only bound; a typed error carries
   elapsed/tokens/trigger; the stream is dropped; no hang (clock-advance
   test, the `test_terminal_tool_timeout_expires` pattern).
2. **[C1b]** A zero-visible-content MaxTokens stop triggers one bounded
   recovery ladder per turn: retry with expanded thinking budget + a
   transient reminder that exists on the request copy only (pinned absent
   from `thread.messages`), then thinking-off; exhaustion falls through to
   the existing fatal path with the D43 warning intact.
3. **[C2]** Consecutive identical tool-call batches: teaching hint injected
   into tool results at streak 3, refusal at 5, any distinct batch resets,
   `[a,b] ≠ [a,c]`, and a distinct first dispatch is never refused
   (Lean-pinned, §12).
4. **[lisp_eval]** A wrapped-scalar env emission fails on call 1 with an
   error naming the object-carrying bindings and the flatten fix; the
   emission-matrix test pins it; a bare `TypeError` without object
   bindings keeps the plain message.
5. `./script/clippy` and `cargo check -p zed` green; DIVERGENCE.md carries
   the new seam entries with pins in the same change.

**Obstacles parking lot:** (a) GPUI timer-race correctness on the
foreground — solved pattern (`.rules` + the clock-advance test precedent);
(b) the transient reminder must not enter durable history — the one novel
mechanism (request-copy seam); (c) one prefix-cache miss per recovery retry
— accepted; (d) false positives on cold local models — defaults keep
first-chunk/stall off; (e) C2 false positives on legitimate polling —
hint-first ladder plus the existing prompt guidance against busy-waiting.
**Focus obstacle:** (b) — everything else follows existing precedents
(D8 rescue shape, tracker ladder, teaching-error pattern).

## 5. Region routing (D/P decomposition)

The plan's generating steps are decomposed per the
syntax-semantic × deterministic-probabilistic regions
(`kask/docs/research/syntax-semantic-probabilistic-deterministic-space.md`).
Every probabilistic (model-facing) surface carries a named deterministic
gate.

| # | Step | Region | Vehicle | Deterministic gate | Role |
|---|------|--------|---------|--------------------|------|
| S1 | `lisp_eval` TypeError teaching arm + matrix test | Sem-P content in an SD container | Rust match arm (`lisp_eval_tool.rs:253-269`); the error text is model-facing | Emission-matrix test: the enriched error names the wrapped binding; bare TypeError unchanged | Executing agent |
| S2 | C2 signature + streak + verdicts | SD | Pure functions in `tool_retry_tracker.rs` | Tracker unit tests + Lean spec pin (§12) | Executing agent |
| S3 | C2 hint/refusal texts | Sem-P | Rust constants (the `format_warning` precedent) | Content pins: text names the streak and the alternatives | Executing agent |
| S4 | C2 wiring at dispatch | SD | Batch signature once per assistant message; batch guard before per-call failure checks | Thread integration tests (hint at 3, refuse at 5, reset on distinct) | Executing agent |
| S5 | C1 watchdogs | SD | Stream wrapper + timer race + typed `CompletionError` variant + settings keys | Clock-advance tests (abort within bound, stream dropped) | Executing agent |
| S6 | C1 ladder machinery | SD | D8-rescue shape + `transient_retry_addendum` seam | Request-copy pin (reminder absent from durable history); one-ladder-per-turn pin | Executing agent |
| S7 | C1 reminder texts | Sem-P | Three Rust constants adapted from the source's battle-tested set | Content pins (budget-reserve / short-reasoning / emit-now instructions) | Executing agent |
| S8 | DIVERGENCE.md seams + pins + docs | SD | One seam entry per workstream, pins in the same change | `check-*` scripts; full build | Executing agent |
| S9 | Plan authorization + settings defaults | Sem-P | — | — (asked question, not assumed consent) | **Operator** |
| S10 | Post-landing falsifier review (log study) | Sem-D | Named method: log sweep for stalled streams, zero-content MaxTokens stops, identical-batch streaks | The study's own count assertions | **Operator** |

**Structural invariant** (region-routing step 3): unclassified 0, ungated 0
(every Sem-P step names its D gate), bad_region 0, operator_roles 2 (S9,
S10), other_roles 8 (S1–S8) — the conjunction is **adjudicated true;
mechanically unconfirmed**: the `lisp_eval` confirmation is blocked by the
very emission pathology this plan fixes (three receipts, §3). The counts
are model-performed over the delivered table and are checkable by any
consumer's recomputation.

## 6. Workstream A — lisp_eval teaching fix (Slice 0)

**Diagnosis (root cause, cited).** The emitter wraps scalars in typed
objects when filling the env parameter (whose per-value JSON Schema is the
empty any-schema, pinned by `test_env_schema_has_type_object`,
`lisp_eval_tool.rs:296-310`); objects become association lists
(`evaluate_lisp` L242-246), so numeric forms receive a list and fail. The
error names the type mismatch but not the offending binding, the received
shape, or the fix — the `.rules`-named failure class ("an error that does
not teach produces identical retries"), and the receipts show even a
teaching-adjacent error is identically retried when the fix is not named.
The failure-side tracker eventually refuses (by design) — the cost is five
wasted dispatches; the fix targets call 1.

**Fix spec.**
- **A1** — in `evaluate_lisp` (`lisp_eval_tool.rs:253-269`), carve a
  `LispError::TypeError { .. }` arm out of the bare catch-all: when any env
  binding carries a JSON object (detected like `nested_env` at L230-233),
  append the sorted object-carrying bindings and the flatten fix —
  mirroring the `UnboundSymbol` treatment exactly (same sorted-bindings
  discipline, L228-229): *"env bindings carrying objects (objects become
  association lists): {name}={…} — if you wrapped a scalar, pass it
  directly: {\"a\": 1}, not {\"a\": {\"n\": 1}}"*.
- **A2** — emission-matrix test (`lisp_eval_emission_matrix.rs`): the
  wrapped-scalar env variant produces the enriched error; a bare TypeError
  with no object-carrying bindings keeps the plain message (no false
  teaching).
- **A3** — one doc-comment example line on the `env` field (L66) showing
  flat scalars.

**Essentialist.** Exist: three live receipts; the unteaching error blocks
six skills' D-gates. Surface: one match arm + one test + one doc line,
reusing the existing sorted-bindings and teaching patterns. Contract:
replaces the `other => other.to_string()` fallthrough for this variant —
the arm is carved out of the existing catch-all, not added beside it.

**Grill-me (recorded).** *Why the tool boundary, not hkask-lisp?* The
TypeError is per-value (`as_f64`, `hkask_lisp.rs:907-914`) and cannot know
the binding name; the tool sees the whole env shape. *Why not accept
wrapped scalars?* Silent tolerance is the forbidden-fallback trap — it
would mask the emission defect and keep the matrix unpinned. *Why not fix
the schema?* The per-value schema is generated from the Rust type and
pinned by test; A3's description example is the teachable surface that
does not fight the pin. Effort: ~0.5 day.

## 7. Workstream B — C2 repetition stop-loss (Slice 1)

**Design.**
- `ToolRetryTracker` gains `success_streak: Mutex<StreakState>` with
  `StreakState { last_batch_sig: Option<u64>, streak: u32 }`.
- `batch_signature(calls) -> u64`: per call
  `format!("{}:{}", name, input_hash(input))` (reusing `input_hash`,
  L254-260), joined with `|` in call order, hashed. Order-preserving; the
  load-bearing property is `[a,b] ≠ [a,c]` (batch granularity — a model
  refining a batch incrementally never trips).
- `note_batch_dispatch(sig) -> RepeatVerdict`: streak + 1 if equal to
  last, else 1; `Allow` / `Hint(streak)` / `Refuse(streak)` at the shared
  `WARN_THRESHOLD` (3) / `HARD_CAP` (5) — one consistent contract with the
  failure ladder.
- **Wiring (S4):** the signature is computed once per assistant message at
  dispatch start (the tool-gather point, `thread.rs:3380` region); the
  batch guard runs **before** per-call failure checks (the coarser outer
  guard first); `Refuse` short-circuits the batch with the teaching text as
  each call's result; `Hint` appends to each result via the existing
  injection path (`thread.rs:4815-4825`).
- **Texts (S3, P):** hint — *"NOTE: this is turn {streak} dispatching a
  byte-identical tool batch; the previous results are already in context.
  Change the arguments, use a different tool, or proceed with what you
  have."* Refusal — mirroring `format_refusal`'s teaching shape (L289-296):
  names the streak, names the alternatives, ends the zero-gain loop.
- **Not adopted (recorded, with reasons):** the turn-pop/rollback variant
  (no turn-pop primitive; `Arc` message discipline makes one expensive),
  `TextRepetitionGuard` (prose repetition is rarer in tool-driven editor
  agents), the search-specific rollback (the generic guard covers the
  class).

**Design revision (2026-10-02, during Slice 1 execution).** The
batch-signature design above was revised before landing: zed-kask
dispatches tool calls as they stream in (`handle_tool_use_event` fires per
event; tools start during streaming), so there is no pre-execution point
where the whole batch is known. The landed guard is call-level — a
per-`(tool, input)` consecutive-success streak that mirrors the failure
side exactly (same per-message dedup via `record_success_for_message`,
same check-before-call point, same warn-3/refuse-5 ladder via `hard_cap_for`,
failure resets streak symmetric to success resetting failures).
Consequence accepted and recorded: a call that stays identical across
evolving batches (e.g., re-reading one unchanged file while varying a
sibling grep) accumulates its own streak and is hinted at 3 — which
mechanically enforces the D26 prompt guidance against re-reading after
success.

**Essentialist.** Exist: measured class (71%/87/198) plus verified
absence. Surface: extends one module, reuses `input_hash`, the verdict
ladder, and the injection path; no new file. Contract: extends the
tracker's scope from failure-retries to all identical-input repeats — the
replaced behavior is "identical successful calls re-execute and re-bill
indefinitely"; the failure-side contract is unchanged (`record_success`
still resets the failure counters).

**Grill-me (recorded).** *Parallel batches?* The signature is computed
from the request side (the message's calls), not results —
`FuturesUnordered` out-of-order arrival is irrelevant. *Legitimate
re-grep after an edit?* The edit turn's batch differs → reset; only N
consecutive identical batches trip. *Polling?* Hint-then-refuse
mechanically enforces the existing prompt guidance ("don't busy-spin …
let the turn end"). *Failing-call repeats?* Both trackers engage; the batch
guard runs first and the failure check covers allowed batches — the two
texts teach the same lesson from different angles; accepted.
Effort: 1–2 days.

**Second design revision (2026-10-02, plan audit).** The call-level
revision above kept a per-key streak map with no reset on a different
dispatch, so "consecutive" in this section's contract was not what
shipped: interleaved identical calls accumulated (edit → verify → edit →
verify — the verification call's streak survived every intervening
edit, and the 6th verification would be refused as a "zero-gain loop").
The fix restores the spec'd semantics at call granularity:
`check_repetition` clears the key's streak when a different key was
dispatched since its last success (reset-on-distinct —
`nextStreak_distinct_resets`), so only back-to-back identical repeats
accumulate. The Lean spec is unchanged — it already specified this; the
code now matches it.

## 8. Workstream C — C1 runaway protection (Slices 2–3)

### C1a — stream watchdogs (S5) — REMOVED 2026-10-02

**Removed the same day it landed** (operator directive, after two live
false-positive aborts killed working turns — see the execution record in
§1). The text below is the historical design, not a shipped surface.

- New module `crates/agent/src/kask_runaway.rs` (kask-owned addition in
  the upstream crate — the `kask_compaction.rs`/`tool_retry_tracker.rs`
  precedent): a stream wrapper racing the completion stream against
  `cx.background_executor().timer()` + `futures::select` — the
  `.rules`-documented GPUI-safe pattern (tokio timers panic on the
  foreground thread).
- Three bounds: **first-chunk** (default OFF — cold local-model loads are
  legitimate; the source's own default is 0.0), **inter-chunk stall**
  (default OFF initially), **reasoning-only** (default ON: 16384
  estimated tokens primary — load-invariant; 120 s secondary; the source's
  tuning note: trust the token cap over wall clock under concurrency).
- Trigger action: drop the stream (cancels the request), raise a new
  `CompletionError::ReasoningRunaway { elapsed_s, estimated_tokens,
  trigger }` — a variant on the thread-local enum (`thread.rs:1459-1466`,
  no upstream crate touch) — carrying accumulated visible content; it
  flows the existing stream-error boundary. Reasoning runaway is a typed
  error, **not** truncation: the D25 flag is not set.
- Settings (`KaskSettings`, D9; defaults in `Default` impls only, per
  `.rules`): `kask.loop_watchdog.{reasoning_only_max_tokens,
  reasoning_only_timeout_s, first_chunk_timeout_s, stall_timeout_s}`.

### C1b — capped-empty recovery ladder (S6, S7) — CHARTER DROPPED 2026-10-02

**Charter dropped** (plan audit, same day as C1a's removal): the
motivating failure's root cause was already removed by D42 (hidden
reasoning budgets), and the observable logs carry zero D43 zero-content
`silent-stop` lines — no observed failure, no mechanism. Re-charter
bar: a live D43 zero-content line. The text below is the historical
design.

- Trigger: `Stop(StopReason::MaxTokens)` with zero visible content (no
  text, no tool calls) — the D43-detectable signature, at
  `handle_completion_event` (`thread.rs:4370-4379`).
- Shape: the `perform_prompt_too_large_rescue` precedent
  (`thread.rs:3770-3845`): guard conditions → build a modified request →
  bounded attempts → `Continue` or fall through. **One ladder per turn**
  (a per-turn flag, the D8 pattern).
- Ladder: attempt 1 — retry with expanded thinking budget (×1.5, ceiling
  8192) + reminder *"larger thinking budget; reserve output for one tool
  call or a visible answer"*; attempt 2 — thinking disabled + reminder
  *"short bounded reasoning; emit a tool call or answer now"*. Exhaustion
  → the existing fatal path; the D43 warning stays as telemetry.
- **Transient reminder seam (the focus obstacle):**
  `KaskThreadState::transient_retry_addendum: Option<String>`;
  `build_request_messages` (`thread.rs:5656`) appends it as a final
  **user** message when set (the source's provider note: a user turn,
  never a non-leading system message — providers disagree on those,
  `_runaway.py:39-44`); cleared at turn end; never enters `self.messages`.
  Pinned: the reminder is absent from durable history.
- Request-level knobs (`max_tokens`, thinking config) are
  `LanguageModelRequest` fields — set per retry; one prefix-cache miss per
  recovery, accepted.

**Essentialist.** Exist: the D42 ruling names the mechanism; zero
completion-path timeouts verified. Surface: one module + one enum variant
+ one state field + four settings keys; the ladder reuses the D8 rescue
shape. Contract: C1b replaces the D43 warn-only zero-content arm's fatal
end with detect-and-recover (the warning stays; the `Err` path remains as
the exhaustion fallback); C1a replaces unbounded waits with bounded ones —
honestly a gap fill, no code deleted.

**Grill-me (recorded).** *Mid-stream abort vs the pending message?* The
abort is a stream error; the existing stream-error boundary plus the
D25/D36 pending-flush machinery handles partial content. *Summarizer
streams?* Out of scope — `stream_compaction` has its own bounded retry
machinery; noted as a follow-up only if the failure mode appears there.
*Subagents?* Same `Thread` machinery — covered for free. *False
positives?* The only default-on bound is the token cap, which fires only
when reasoning is flowing and nothing else arrives. Effort: ~1 week
total (C1a 2–3 days incl. clock-advance tests; C1b 2–3 days).

## 9. Sequencing (kata step 4 — PDCA slices)

| Slice | Content | Prediction (the theory under test) | Effort |
|-------|---------|-----------------------------------|--------|
| 0 | Workstream A (lisp_eval) | The enriched error turns the first failure into a correction; matrix test observed RED before the fix | ~0.5 day |
| 1 | Workstream B (C2) | Streak tests + Lean pin align; thread integration passes; a distinct batch never refused | 1–2 days |
| 2 | C1a watchdogs | A clock-advance test aborts a thinking-only stream within the bound, stream dropped, typed error | 2–3 days |
| 3 | C1b ladder | A zero-content MaxTokens fake recovers; the reminder is pinned absent from durable history; one ladder per turn | 2–3 days |
| 4 | DIVERGENCE.md seams (one entry per workstream) + pins + this doc's status update | Same-change discipline holds; `check-*` scripts green | 0.5 day |

Each slice: red test → implement → green → pin. `./script/clippy` and
`cargo check -p zed` before any green claim (full-repo build, per
`.rules`). Slice 0 is independent and unblocks the skill system's
D-gates; it proceeds first regardless of the C1/C2 charter decision.

## 10. Replacement ledger (P5.5)

- **A1** carves the `TypeError` arm out of the bare catch-all (replaces
  `other => other.to_string()` for that variant).
- **B** extends the tracker; no deletion — the replaced behavior is
  named (unbounded identical re-execution).
- **C1b** replaces the warn-only zero-content arm's fatal end (the arm
  gains a rescue branch; the `Err` path remains as the exhaustion
  fallback).
- **C1a** is additive (bounds unbounded waits) — a gap fill; no code
  deleted.
- Net: mostly additive with two behavior replacements; net lines
  reported per change at implementation time.

## 11. Risks and falsifiers

- **C1 falsifier:** a log study over the operator's fleet (ollama / GLM /
  OpenRouter) showing zero stalled streams and zero zero-content MaxTokens
  stops → the watchdogs ship default-off and the ladder is dropped.
- **C2 falsifier:** a workload study showing identical consecutive
  successful batches are common and legitimate → the ladder stays
  hint-only.
- **lisp_eval falsifier:** the enriched error does not change model
  behavior on the next call (log study) → the matrix test still pins the
  teaching contract; the emission-side lesson is already filed to the
  curator.
- **Risks:** false-positive aborts (defaults chosen against), prefix-cache
  misses per recovery (accepted), C2 double-messaging with the failure
  tracker (accepted — both texts teach), reminder leaking into history
  (pinned against), deprecation lints in the Lean file (recorded,
  non-blocking).

## 12. Formalization record (lean-prover)

`kask/lean/repetition_guard_spec.lean` — **machine-checked 2026-10-02**:
Lean 4.34.0 (pinned toolchain), exit 0, five theorems —
`first_dispatch_streak_one`, `nextStreak_distinct_resets` (reset-on-change),
`nextStreak_shape` (the conservation property: every dispatch resets to 1
or increments by exactly 1 — the streak can never jump),
`nextStreak_distinct_below_cap` (no starvation: a distinct first dispatch
is never refused), `verdict_refuse_implies_streak` (refusal certifies the
threshold streak). Axiom audit on `nextStreak_distinct_resets`:
`[propext]` only — Lean's core axiom via `simp`; no `sorry`, no
`native_decide`; two deprecation lints (`if_neg`/`if_pos`) recorded. The
model is the guard's contract; the Rust implementation is pinned by the
Slice 1 tracker tests. C1's ladder termination is a bounded loop by
construction — unit tests pin it; no Lean formalization (a proof would
verify the spec formalization, not the Rust; consistent with the review's
Phase 7 reasoning).

**Implementation mapping (2026-10-02).** The spec's `step` models the
recorded streak (advance on identical, reset on distinct); the Rust
implementation checks the PRIOR streak before dispatch — the failure
side's check-before-call contract — so the hint lands on the dispatch
after the 3rd identical success and the refusal after the hard cap.
`record_failure_inner`'s streak reset is an additional reset-to-1 path the
spec does not model; it preserves all four theorems (a reset only ever
makes streaks smaller). The spec's `Batch`/signature corresponds to the
implementation's `(tool_name, input_hash)` key. **Corrected 2026-10-02
(plan audit):** the first implementation did NOT reset on distinct — a
per-key map let interleaved identical calls accumulate, so
`nextStreak_distinct_resets` described the spec, not the code.
`check_repetition` now clears the key's streak when a different key was
dispatched since its last success; the mapping above is accurate as of
that fix.

## 13. Open questions for the operator

1. **Charter:** authorize Slice 0 (lisp_eval) standalone? It is
   independent, smallest, and unblocks the skill system's deterministic
   gates.
2. **Defaults:** reasoning-only watchdog ON at 16384 tokens / 120 s;
   first-chunk and stall OFF — confirm or adjust.
3. **C2 thresholds:** share `WARN_THRESHOLD` (3) / `HARD_CAP` (5) with the
   failure ladder (one consistent contract), or separate knobs?
4. **Post-landing:** the falsifier log study (S10) — who runs it, over
   what window, and against which log sources?
