# Continuation: finish the complexity teardown, then resume the reliability program

## Mandate and mode (this supersedes prior continuation prompts)

Continue the authorized reliability program in `tasks/plan.md` and
`tasks/todo.md` — now under **Phase E, the complexity teardown** (operator
ruling 2026-09-08). The operator's correction, recorded verbatim in intent:

> "we keep adding code and complexity and not cleaning anything up... I do
> not believe we are actually improving any reliability by just ratcheting
> up complexity... You have missed the rjoule and budget and other code
> that you were supposed to clean — so you have been building on
> unreliable code you never cleaned."

And on roles (also recorded in plan.md Phase E):

> "the product manager doesn't define every variable, he defines the
> required ones and the required functionality and then the rest is up to
> the technical program manager to implement efficiently."

**Standing definition of done (applies to every slice from 2026-09-08
forward):** every change must leave the tree simpler than it found it.
Net-LOC accounting, a simplification review of every file touched, and
residue removal ride with the change. Test-green on a more-complex base is
not done. Additive-only slices are failures.

**Phase C (T07/T08) is HALTED until the teardown tranche (C1–C3) lands.**

## Where things stand (verified, do not redo)

- **T01–T03, D01, T02b:** verified 2026-09-07; Checkpoint A RATIFIED
  2026-09-08 (all recommendations; record in plan.md §Checkpoint A).
- **T04 (pending distillation revisit), T05 (session reservation +
  settlement), T06 (spawn replay protection):** implemented and verified
  2026-09-08. RED observed for each against pre-fix semantics; suites,
  clippy, rustfmt clean. Evidence in plan.md task sections. Checkpoint B
  operator review was opened but SUPERSEDED by the teardown ruling — the
  operator's mode-change message interrupted it. Treat Checkpoint B as
  evidence-complete, operator-review-pending, to be re-presented after the
  teardown.
- **T16–T19 (Phase D):** automated checks complete; release builds and
  live executable identity verified (editor PID 90198 + 11 children match
  by ELF build ID; evidence in plan.md §Release build). Live T16 emission
  AND decay verified on the running build (curator env
  `HKASK_MEMORY_LIFE_DAYS=30`; recall confidence decayed per S=30).
- **C1 — local budget system DELETED (2026-09-08, ratified "delete it
  all"):** the `hkask-ledger` crate, the runtime's cost/debit machinery,
  `cost/cost_uncapped/balance` from `LocalDelegateResult`, the three
  ledger tools (surface 85→82), `credits_authorized` from all LOCAL
  request types (cloud types KEEP theirs — ABW credits are real and
  consent-gated), kata-kanban's ceiling read + cost theater, the
  `HKASK_SWARM_LEDGER_PATH` emission/allowlists/settings row,
  `total_cost_credits`, and every doc section. Net −1,421 lines in scope.
  Commits `29d3330507` + `a4f82b3deb`.
- **C2 core — spend-gate consolidation (2026-09-09):** `Settlement` is now
  THE authorization type; the `HireAuthorization`/`DelegateAuthorization`
  wrappers and the `DispatchSettlement` trait are deleted;
  `settle_dispatch_failure` is concrete. Verified green. This was in the
  worktree at handoff time (some staged by the external process's sweep —
  see below); if it is already committed, do not redo it.

## Immediate work (in order)

1. **C2 leftovers (small):**
   - The `test_client` + `sqlite_store` helpers are duplicated between
     `spend_gate.rs` tests and `cloud_swarm/curator.rs` tests — extract
     into the `test_http` fixture module in `abw_client.rs`.
   - Review T04's `DistillationCursor::merge` eviction block (~40 lines
     for a bounded map): assess whether a simpler structure (e.g. a
     VecDeque keyed by first-seen time, or an explicit evict-oldest scan
     over a small map) states the same invariant with less machinery.
     Only change it if it is genuinely simpler AND the T04 tests still
     pin the behavior (bounded set, newest survives, survivors distilled).
   - Confirm no other T06 response-format residue beyond what C1 cleaned
     (the spawn message is already credits-free).
2. **C3 — repo-wide residue sweep:** every remaining mention of the
   deprecated budget concept (rJoule, local credits, ledger) in code,
   docs, scripts, comments — including the rJoule spec doc links that
   survive in the ledger-crate README's absence, the training-types
   rjoule mention, and any `.rules`/skill references to per-call budgets.
   Also sweep for the count drift class: tool-count pins (the swarm 82
   pin, the panel's KANBAN_TOOLS 25 pin) after any further tool removals.
3. **Re-present Checkpoint B** (T04–T06 + C1/C2 evidence, net-LOC
   accounting included) for operator review. Then Phase C (T07 → T08)
   resumes under the new definition of done — with T07/T08 themselves
   held to net-simplification: prefer deleting the false-acknowledgment
   paths over adding new ones.

## T09–T15 (unchanged queue)

Passage deletion ownership, exact harness comparison, market-identity
calibration, cap-reset evidence, retrain finalization, IPC discovery,
skill-feedback sensing. Require task elaboration + operator scheduling
before execution. Note the recorded T09-class observation: watermark/turn
reads are entity-PREFIX queries; production thread ids are UUIDs so it is
unreachable today, but any future non-UUID id source needs exact-match
reads.

## Live quit test (still open, operator-controlled)

The quit watcher EXPIRED unused at 2026-09-09T01:04Z (no quit happened).
The watcher script, README (with the one-command re-arm), and self-test
evidence are at `target/kask-quit-watch-20260908T225830Z/`. Re-arm with
the CURRENT editor PID when the operator is ready to quit normally; the
agent never closes the editor autonomously. Confirm the quit manner with
the operator afterward (disappearance ≠ normal quit).

## Standing constraints (all still in force)

- One build at a time; check `ps -eo pid,etime,args | grep -E
  'rustc|cargo|clippy-driver' | grep -v grep` before every Cargo
  invocation; operator/parallel builds win the lock; 8 jobs max for
  builds (4 was used during memory pressure); `./script/clippy` (dev
  profile default, D52) with `HKASK_BUILD_JOBS` + `CARGO_NET_OFFLINE=true`
  for lints; bounded timeouts; logs under `target/`.
- An external process stages AND commits/pushes during sessions (it
  committed T04/T05 as `dd5c5a4dea`, T06 as `6a24ed82aa`, the teardown as
  `29d3330507`+`a4f82b3deb`, sweeping agent worktree edits mid-flight).
  Never unstage, reset, or revert its actions. If the tree diverges from
  what you wrote, read the fresh state before editing (the
  `read_file`-stale trap in `.rules`).
- A parallel agent session is ACTIVE in this editor (media/gallery work:
  `hkask-mcp-media`, `media_panel`, `hkask-storage/gallery.rs`). Its write
  scope is disjoint from the reliability program's — stay out of media/
  gallery/panel files unless a gate (like the KANBAN_TOOLS pin breakage in
  C1) forces a fix, and record any such fix.
- The original program baseline for behavioral RED is
  `2475305420ae065b5d1792c0f25cea471e558ae3`. Current HEAD contains all
  repairs.
- Read `.rules`, `program-manager`, `tdd`, and the crate README before
  editing; name the architecture doc + invariant before the first edit
  (Magna Carta P1/P4; for memory work the memory-system spec).
- Do not create commits/branches/pushes unless asked. Do not unstage the
  external process's sweeps. One editing agent; no half-refactors left in
  the tree — if a consolidation cannot finish in one pass, gate or revert
  it before yielding.

## Definition of done for this phase

- C2 leftovers closed with the same evidence standard (RED where behavior
  changes; suites/clippy/rustfmt clean; net-LOC recorded in plan.md).
- C3 sweep: zero remaining deprecated-budget mentions in kask/ (code,
  docs, scripts, comments), with a grep-able negative claim recorded.
- plan.md Phase E and todo.md current, including per-slice net-LOC
  accounting.
- Checkpoint B re-presented to the operator with the teardown accounting.
- The closure ledger names every open item with an owner and state.
