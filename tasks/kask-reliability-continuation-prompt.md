# Continuation: Kask regression-first reliability work

## Operator instruction and immediate objective

You are taking over an authorized implementation, not starting another audit.

The operator requested a six-skill audit (refactor-architecture, bug-hunt, diagnose, hypothesis-framer, pragmatic-cybernetics, metacognition), accepted a regression-first plan, then said **"D01 yes. please proceed"**. On 2026-09-07 they asked: **"please compose a continuation prompt and update the plan and tasks for where the work is now and handoff to another agent"**.

**D01 is ratified:** companies research notes and forecasts must survive portfolio schema recovery. Do not ask this question again. Broader plan execution was authorized, but task-specific unresolved policies and checkpoints still apply. No commits, branches, destructive production probes, or accepted risk deferrals were authorized.

Your first job is to finish and verify the existing **T01 scenario persistence** and **D01 portfolio recovery guard** changes. Do not begin the remaining fixes until the present tree is understood and validated. The outgoing agent stopped implementation for this handoff; all delegated agents returned. No further code changes were made during handoff preparation.

## Read first

1. `tasks/plan.md` — full plan, updated state, acceptance criteria, policy gates.
2. `tasks/todo.md` — activity versus completion checklist.
3. Project `.rules`; load `program-manager` and `tdd` skills.
4. `kask/docs/architecture/core/magna-carta.md` — user-data sovereignty; parent-held authorization; intended versus implemented behavior.
5. `kask/docs/reference/mcp-servers/portfolio.md` and `companies.md` — D01 ratification, date, superseded disposal scope.
6. Current diff, relevant crate sources, and tests. Before editing, name one architecture doc and its constraining invariant.

Later memory work must preserve `kask/docs/architecture/memory-system-specification.md`: single-copy chunks, hard deletion, additive-only idle-thread distillation, watermark-before-lesson insertion, bounded startup discovery. Regulation work must preserve ratified advisory D4 in `kask/docs/diataxis/hkask-regulation/explanation.md`; no autonomous actuator.

## Worktree baseline and preservation

Root: `/home/mdz-axolotl/Clones/zed-kask`.

HEAD at handoff: `2475305420ae065b5d1792c0f25cea471e558ae3` — `Extract editor-free Steer prompt helpers`.

No commits/branches were created. At handoff all implementation changes are unstaged; task documents are untracked. Run `git --no-pager --no-optional-locks status --short` and compare before editing. Preserve existing work; do not reset, clean, stash, or overwrite it blindly. Only one agent process may edit at a time, including disjoint file scopes. Read-only reviews may run concurrently.

Expected changed files:

- `Cargo.lock` — one direct dependency edge for scenarios → existing workspace `tempfile`.
- `kask/mcp-servers/hkask-mcp-scenarios/Cargo.toml`
- `kask/mcp-servers/hkask-mcp-scenarios/src/superforecast/store.rs`
- `kask/mcp-servers/hkask-mcp-scenarios/src/hkask_mcp_scenarios.rs`
- `kask/mcp-servers/hkask-mcp-scenarios/tests/tool_behavior.rs`
- `kask/mcp-servers/hkask-mcp-portfolio/src/store.rs`
- `kask/mcp-servers/hkask-mcp-portfolio/src/tests.rs`
- `kask/docs/reference/mcp-servers/portfolio.md`
- `kask/docs/reference/mcp-servers/companies.md`
- Untracked `tasks/plan.md`, `tasks/todo.md`, this continuation prompt.

No upstream production code is changed. `kask/docs/reference/mcp-servers/scenarios.md` has been read but **not updated** for the persistence changes.

## T01 — implemented, not validated

Original defect: `ForecastStore::compact` warned on failed snapshot write then truncated the recovery journal anyway. Journal append errors could also be silently ignored while admitting in-memory state and reporting successful scoring.

Current changes:

- `save_entry`, `insert`, and `persist` return `std::io::Result`.
- Journal writes and file sync must succeed before the record enters memory.
- Snapshot serialization writes a `tempfile::NamedTempFile` in the destination directory, syncs it, then publishes it with `persist`.
- On Unix the parent directory is synced after publication and before journal truncation. Journal cleanup is synced; successful compaction resets `journal_count`.
- Snapshot publication failure leaves the journal available for replay; tempfile RAII cleans failed temporary snapshots.
- `scenario_score` propagates persistence failures through `McpToolError`, explicitly allowing already-journaled partial progress rather than claiming request rollback.
- No general multi-process store locking or power-loss proof was added. The store contract assumes exclusive writes. Do not claim those guarantees from these tests.

Written public-tool tests in `tests/tool_behavior.rs`:

1. `snapshot_failure_preserves_journal_for_recovery`
2. `journal_failure_is_surfaced_before_memory_changes`

They use temporary file-backed servers. A directory at the snapshot/journal destination creates a deterministic failure even for a privileged test runner. The first test exercises failure→reopen→retry→successful compaction; the second checks surfaced journal failure and unchanged memory.

### Important review issue to fix first

The first test currently checks only counts after reopen, then calls `scenario_score` again **before** asserting the final outcome. That retry can repair a wrong replayed outcome, hiding a regression.

- Assert recovered identity, probability, and outcome through an actual read seam **before re-scoring**. Inspect `scenario_status`'s returned fields before assuming it exposes the entire record; use another faithful public read seam if necessary.
- Compare the retained journal's full entries/expected resolved record, not just `!journal.is_empty()`.
- Add coverage for **a published snapshot with its overlapping journal still present**: reopening must preserve the exact last-write-wins record without duplicates. The existing test empties the journal before its final reopen and does not cover this boundary.
- Confirm the previous snapshot remains readable across failed replacement; add a narrowly scoped control if existing fixtures do not prove it.

Independent source/diff review found no definite newly introduced production bug, but compilation was not completed. Do not treat the review as a green build. Preexisting tolerant-load/torn-journal behavior was not included as a new defect; avoid unrelated expansion.

## D01 — policy resolved; code/tests unverified

Ratification is recorded in both owning reference docs. Historical portfolio-only disposability (`804cf441a8` / `f28789fc81`) remains; extending whole-file deletion to companies research is superseded.

Important FK constraint: companies `notes.portfolio_name` and `files.portfolio_name` reference `portfolios(name) ON DELETE CASCADE`. Dropping portfolio parents in a mixed DB would erase research or leave invalid relationships. Simply dropping the portfolio tables is not a safe shared-database repair.

Implemented in `open_with_schema_recovery`:

- Acquire an IMMEDIATE transaction before schema initialization, ownership inspection, or reset.
- After DDL failure, inspect `sqlite_schema` for tables outside the five owned names: `portfolios`, `transactions`, `price_cache`, `daily_holdings`, `daily_returns`.
- Exclude internal `sqlite_*` names using GLOB, not an unescaped LIKE underscore.
- Any other table, or inspection error, refuses reset with context; rollback preserves prior data/schema.
- Portfolio-only DB: drop child tables before parents and rebuild inside the transaction. Reset failure rolls back.
- Removed database unlink/reopen. `pub(super)` helper supports crate-local error assertions; no new public API or dependency.

**Intentional user-visible behavior:** compatible shared databases still open. Incompatible shared databases now error rather than silently destroy research. This trade-off was communicated; do not invent a research migration merely to avoid the error.

Five tests in portfolio `src/tests.rs`:

- `schema_recovery_discards_stale_portfolio_data` — adapted existing portfolio-only control.
- `schema_recovery_preserves_mixed_database` — populated notes/files/forecasts, revision/outcome data, parent rows, schema, and FK checks.
- `schema_recovery_refuses_unknown_non_portfolio_table` — also catches `sqlite_%` wildcard exclusion mistakes.
- `schema_recovery_rolls_back_failed_portfolio_reset` — partial reset rolls back.
- `schema_recovery_opens_compatible_mixed_database` — compatible shared startup retains research.

Portfolio explicit-file formatting and whitespace checks passed. Test execution remains outstanding. The docs deliberately separate the policy from proof of enforcement; update enforcement citations/results only after validation.

## Validation history — do not relabel timeouts as passes

All commands ran from the repository root. No Rust test reached execution; no behavioral RED or GREEN has been observed.

| Attempt | Result |
|---|---|
| Earlier audit: `cargo test --offline --locked -p hkask-mcp-scenarios --lib --no-run -j 2` | 120-second timeout waiting for another build's directory lock |
| Baseline: `cargo test --offline --locked -p hkask-mcp-scenarios --test tool_behavior scenario_status_reports_empty_state -j 2` | 180-second timeout compiling dependencies, about 130/365 units |
| T01 pre-fix: `cargo test --offline -p hkask-mcp-scenarios --test tool_behavior snapshot_failure_preserves_journal_for_recovery -j 2` | 180-second timeout compiling, about 199/366 units; updated lockfile dependency edge |
| D01 pre-fix: `cargo test --offline --locked -p hkask-mcp-portfolio schema_recovery -j 2` | 180-second timeout compiling, about 203/356 units |
| D01 post-fix: `cargo test --offline --locked -p hkask-mcp-portfolio --lib schema_recovery -j 2` | 180-second timeout compiling, about 237/351 units |
| Combined post-fix: `cargo test --offline --locked -p hkask-mcp-scenarios -p hkask-mcp-portfolio -j 2` | 180-second timeout compiling, about 256/375 units |

The last observed blocker was **slow dependency compilation**, not a lock or reported compiler error. The commands use somewhat different feature/target sets, so build counters are not one precise progress meter. Incremental artifacts remain in `target/`; do not clean them.

- `git --no-pager diff --check` passed across tracked changes at handoff.
- Portfolio `rustfmt --check --config skip_children=true` on explicit files passed (delegate report).
- The audit's `bash kask/scripts/check-mcp-tool-tests.sh` passed with zero violations/allowlisted gaps. It only checks test presence and predates implementation.
- `cargo check` and `./script/clippy` have **not** been run on these changes.
- No claim of workspace health, crash durability, successful regression execution, or completed checkpoint is supported.

## Resume sequence

1. Recheck tree and build processes without killing anything. Preserve the exact pending implementation scope.
2. Fix the T01 test weakness above; add the missing recovery boundary control. Format/check only touched Rust files; do not run workspace-wide formatting.
3. Coordinate an adequate bounded Cargo runtime with the operator. Repeating three-minute builds is not a validation strategy. Do not silently change global profiles/jobs, delete locks, clean caches, or bypass the existing wrapper rules.
4. With an agreed runtime, execute targeted tests, then affected suites. Suggested commands (not yet successful):
   - `cargo test --offline --locked -p hkask-mcp-scenarios --test tool_behavior -j 2`
   - `cargo test --offline --locked -p hkask-mcp-portfolio --lib schema_recovery -j 2`
   - `cargo test --offline --locked -p hkask-mcp-scenarios -p hkask-mcp-portfolio -j 2`
   - Recover the companies compatibility test targets before selecting the dependent-crate check.
5. Address actual compiler/test errors surgically. Establish pre-fix regression failure with a safe isolated/reversible approach that preserves all current work. Never claim test-first RED was seen merely because test code preceded the fix.
6. Run affected `cargo check` and **`./script/clippy`**, not direct `cargo clippy`; inspect the wrapper before choosing runtime/jobs because it uses release/all-targets/all-features. Preserve resource limits.
7. Update scenarios persistence documentation, completed enforcement references, exact validation evidence, and unchecked criteria in plan/todo. Stop on policy conflicts; do not mark blocked work complete.
8. Once current slices are closed, continue T02 (redirect SSRF), then T03 (safe forgetting), respecting Checkpoint A. T04–T08 and the later backlog are not implemented; their descriptions in the plan are proposals, not current code.

## Remaining program state

- **T02:** research redirect SSRF — untouched. Validate every hop and actual destination; preserve permitted redirects; recover proxy/DNS behavior before coding.
- **T03:** forgetting coverage/concurrent insertion — untouched.
- **T04:** distillation cursor pending-work retry — untouched, preserve ratified watermark/startup rules.
- **T05:** session reservation before dispatch — untouched; unknown-outcome reconciliation remains a policy gate.
- **T06:** post-spawn idempotency — untouched.
- **T07/T08:** truthful directive outcomes / explicit domain escalation — untouched; current best-effort void alert sink cannot prove durable success.
- **T09–T15:** passage/vector lifecycle, exact harness pair, market identity dedup, cap feedback, retrain finalization, IPC discovery, skill-feedback producer — tracked but require elaboration before execution.

No forecast of completion time, fabricated calibration, or new requirements should be inferred from this handoff. Close the existing evidence gaps before expanding the diff.
