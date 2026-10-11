---
title: "Test-suite Refresh Review — Working Plan (kask/ subtree and hkask-mcp-* servers)"
audience: [developers, architects, agents, operators]
last_updated: 2026-10-11
version: "0.40.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle]
---

# Test-suite refresh review — working plan

**Status:** Working plan, composed 2026-10-11. **Not executed.** No test, build,
clippy, or review gate has been run at composition time — every check named below
is *proposed*. This file is a governed docs-tree artifact with an explicit
lifecycle: execution amendments are logged in §9, and at termination (§5) it is
deleted — stale plans are deleted, not archived (DOCUMENTATION_STANDARDS.md §3) —
with its successor recorded in the README lifecycle ledger and corpus-size
parity restored in the same change.

**Constraint:** refresh-only. This review aligns *existing* tests to current
code. It adds no coverage, no features, no production-code changes. Zed-side
upstream files are out of scope except the fork-seam test mini-batch (§3.6).

## 0. Failure mode and categories

The suite may measure fossils: tests asserting behavior removed months ago, or
hardcoding patterns the codebase has since generalized past. Every suspect test
is classified against:

**Stale pin — remove or rewrite:**

1. *Removed-behavior pin* — asserts a symbol, function, error string, file, or
   behavior with zero live references in the current tree. Evidence: zero-hit
   grep over code AND docs, plus the removal commit in git log.
2. *Evolving-pattern pin* — hardcodes a manifestation of a pattern the codebase
   has generalized (hardcoded file-name lists where coverage is now
   filesystem-derived; byte-identical error-string assertions where errors are
   now typed variants; literals that now live in a shared constants module).
   Evidence: the pinned literal diverges from what current code derives, and a
   canonical replacement form exists in the tree.
3. *Wrong-premise setup* — the test's purpose is right but its setup pins an
   outgrown premise. Fix the setup; keep the test.

**Deliberate invariant pin — keep, never delete (§7):**

- Pins behavior disabled by a `// zed-kask:` comment (D-seam divergence
  protection; DIVERGENCE.md D1–D89).
- Pins the legitimate case a restricting fix must not break (regression pin).
- Asserts a category invariant derived from the tree at test time (filesystem
  walks, registry enumerations — never hardcoded name lists).
- Surfaces degradation (asserts the reason is named — never that an empty
  result equals success).

When a correctness fix breaks a test: read the test's intent before its
assertion. Fix the setup, keep the intent.

## 1. Phase 1 — Scope enumeration (once, at execution start)

**Entry:** execution approved; working-tree state recorded
(`git status --porcelain` snapshot into the batch notes).

**Steps** (from the repo root — `kask/` is not a standalone Cargo workspace;
all its crates are root-workspace members per DIVERGENCE.md, so every cargo
command runs from the root with `-p <crate>`):

1. Crates: `find kask -name Cargo.toml -not -path '*/target/*' | sort`
   — 32 expected at composition time (19 under `kask/crates/`, 13 under
   `kask/mcp-servers/`). Re-count; never assume.
2. Integration test files per crate:
   `find kask/crates kask/mcp-servers -path '*/tests/*.rs' -not -path '*/target/*' | cut -d/ -f1-3 | sort | uniq -c`
3. Inline test-bearing files per crate:
   `grep -rl --include='*.rs' --exclude-dir=target '#\[test\]' kask/crates kask/mcp-servers | grep -v '/tests/' | grep -v '/benches/' | cut -d/ -f1-3 | sort | uniq -c`
4. Zed-side fork-seam candidates:
   `grep -rln --include='*.rs' --exclude-dir=target 'zed-kask' crates | xargs grep -l -e '#\[test\]' -e '#\[cfg(test)\]'`
5. Create the tracking board: kanban board `test-suite-refresh`, one task per
   crate plus one `zed-side fork-seam mini-batch` task; WIP limit 1 on
   in-progress. Each task's criteria = the §4 gate.

**Exit:** §10 manifest refreshed with execution-time numbers (those are
authoritative, not this file's composition-time tables); board exists; every
crate has a task.

## 2. Phase 2 — Triage funnel (per crate, at that crate's batch start)

Cheap and mechanical; it bounds the expensive deep review. The suite run
doubles as the crate's pre-modification baseline for the §4 gate.

**Entry:** the crate's kanban task claimed;
`git status --porcelain -- <crate paths>` shows no other stream's in-flight
edits (defer the crate if non-empty and foreign).

**Signals, in precedence order:**

1. **(c) Non-green → suspect.** `cargo test -p <crate>` (live-mutation probe
   crates: append `--test-threads=1`). Record every failing and ignored test
   per file; `grep -rn '#\[ignore' <crate>/tests <crate>/src` catches the
   skipped. *Attribution before routing:* each non-green test is attributed
   known-pre-existing / newly-broken / stale-pin-suspect from this fresh run.
   Session history records corpus-crate test failures attributed to a stash
   event — that is context to re-establish, never a fact to carry forward.
2. **(a) Dead identifier → suspect.** `cargo test -p <crate> --no-run` — a test
   target that fails to compile pins identifiers that no longer exist.
3. **(b) Hardcoded-what-code-derives → suspect.** For identifier-like literals
   in assertions (error strings, file names, paths — *not* numeric expected
   values, which are legitimate pins): `grep -rn "<literal>" <crate>/src` —
   zero hits, including the shared constants modules, means divergence.
4. **(d) Drift → needs-intent-reading.** `git log -1 --format=%cs -- <test
   file>` vs `git log -1 --format=%cs -- <crate>/src` — src newer by more than
   90 days.
5. **Keep-signature → deliberate-pin-candidate** (skips deep review unless
   non-green): the file contains `zed-kask`, names a DIVERGENCE.md seam, or
   self-describes as pin / invariant / regression.
6. Else **healthy** — no deep review.

**Exit:** every test file in the crate bucketed (suspect / needs-intent-reading
/ deliberate-pin-candidate / healthy); bucket counts and baseline failures
recorded on the kanban task.

## 3. Phase 3 — Deep review (per crate)

**Entry:** triage complete for the crate. Only suspect and needs-intent-reading
files get deep review; deliberate-pin-candidates get a one-minute marker/seam
validity confirmation, nothing more.

**3.1 Pilot.** The first batch is one of the single-file crates
(hkask-email, hkask-event-store, hkask-steer-core, or hkask-services-core) —
it validates the full procedure end-to-end (triage → review → gate → commit →
board) at minimum cost. Exit the pilot with a funnel-precision note in §9
(false positives/negatives observed, threshold adjustments). Then treat the
first large crate (hkask-mcp-corpus) as scale calibration before committing to
the full order. Do not scale a miscalibrated funnel.

**3.2 Ordering.** After the pilot: descending test-file count (§10), re-checked
at each batch start. Sub-batch rule: more than 15 suspect+needs-intent files in
one crate → split by file group, one commit per sub-batch.

**3.3 Per-file procedure.**

1. Read the test and the code it exercises. Write the intent reading: one or
   two sentences naming the behavior pinned.
2. Categorize against §0 (removed-behavior pin / evolving-pattern pin /
   wrong-premise setup / legitimate pin).
3. Verdict: **delete / fix setup / rewrite as category invariant / keep**.
4. Record file:line, category, evidence, verdict — as a comment on the crate's
   kanban task.

**3.4 Deletion evidence standard — all three, or no deletion:**

- **(i) Zero-reference sweep.** For each load-bearing identifier (asserted
  symbol, error string, file name, helper):

  ```
  grep -rn "<id>" kask/ --include='*.rs' --include='*.md' --include='*.sh' --include='*.toml' --exclude-dir=target > /tmp/sweep.txt
  wc -l /tmp/sweep.txt && cat /tmp/sweep.txt
  ```

  plus the zed-side and governance pass:

  ```
  grep -rn "<id>" crates .rules DIVERGENCE.md --include='*.rs' --exclude-dir=target
  ```

  Consume the full list — never report a sweep as clean from capped output.
  Zero hits outside the test itself, this plan, and the findings = pass. Hits
  in `kask/docs` → the doc sweep (§4 step 5) fixes them in the same change.
  Hits in `.rules` or `DIVERGENCE.md` → the deletion is blocked until the
  governance reference is updated in a dedicated, operator-approved rules-only
  commit; never edited inline.
- **(ii) The removal or generalization commit.**
  `git log --oneline -S"<id>" -- kask/` names it — or the documented absence
  is itself recorded in the findings.
- **(iii) The intent reading** from 3.3 step 1.

**3.5 Rewrite standards** (for evolving-pattern rewrites — the repo's
test-protocol traps, restated as standards):

- Category invariants are derived from the tree at test time: filesystem walks
  and registry enumerations, never hardcoded name lists.
- MCP tool tests construct the server with the capability under test and
  invoke tools through the public `Parameters<T>` seam —
  `kask/scripts/check-mcp-tool-tests.sh` keys on that contract and fails a
  server that loses it.
- Degradation is surfaced (a reason/note/status field is asserted), never
  empty-result-equals-success.
- Memory-writer variants get recall-path round-trip tests (the entity_ref JOIN,
  not just the write).
- GPUI tests wait via `cx.background_executor().timer(...)`, never
  `smol::Timer`.

**3.6 Zed-side fork-seam mini-batch** (the one zed-side surface in scope).
Filter the 67 candidates (§10) to test modules that pin zed-kask behavior —
D-seam pins, and tests in fork-owned zed-side crates (the hkask-*-widget
crates, the agent crate's kask modules). Same procedure, plus: every zed-side
edit carries its DIVERGENCE.md seam verification in the same pass — the seam
entry exists and still matches; update it if the pin changes. *Scope judgment
to confirm at execution start (operator call):* whether fork-owned zed-side
crates' tests are reviewed in this mini-batch or deferred.

**3.7 Scope boundary.** Production-code findings — dead surface discovered
under a test (trait-with-one-impl, unwired helper) — are recorded and filed as
follow-up kanban tasks with evidence. They are not fixed in-batch; batches stay
test-side only.

**Exit:** findings list complete with verdicts; modifications applied; every
deletion carries (i) + (ii) + (iii).

## 4. Phase 4 — Verification gate (per batch, before commit)

**Entry:** §3 modifications applied.

1. `cargo test -p <crate>` — green against the §2 baseline: no new failures;
   targeted stale tests gone or fixed.
2. MCP-server crates: `bash kask/scripts/check-mcp-tool-tests.sh` exits 0 — the
   per-server `Parameters<T>` contract survived the batch.
3. `./script/clippy` — zero new warnings in the touched scope; action any
   machete finding in the touched crate (verify zero `use <dep>` hits in
   `src/` and `tests/`, then remove the dep line).
4. Orphan sweep: helpers and fixtures deleted with their consumers; full-repo
   symbol sweep for every removed identifier (code + docs + scripts).
5. Doc sweep: `grep -rn` removed identifiers over `kask/docs` — zero stale
   mentions. If any docs file changed:
   `bash kask/scripts/check-docs-count-gate.sh` exits 0 (README corpus-size
   parity intact).
6. `cargo check -p zed` if anything outside `tests/` changed.
7. Net lines: `git diff --shortstat` against the batch base, recorded in the
   findings.

**Commit:** stage exactly the batch's files; review
`git diff --cached --stat`; then
`git commit -m "<plain text, no fences>" -- <batch paths>` (message shape:
`Refresh <crate> tests: <N> deleted, <M> setups fixed, <K> rewritten; net <±L> lines`).
One kanban state move per batch.

**Exit:** all checks green; findings and evidence on the kanban task; commit
hash recorded on the task.

## 5. Phase 5 — Termination and final report

**Termination condition:** every crate in the §10 manifest — including
enumerated zero-test crates (hkask-tool-port at composition time) — and the
zed-side mini-batch has a recorded verdict on its kanban task. "No findings"
is a verdict.

**Final report** (delivered in-thread and saved via `report_save`, kind
`report`, name `test-suite-refresh-final-report`):

- Per-crate verdict table (deleted / fixed / rewritten / kept / no findings).
- Deletions with their three-part evidence and commit hashes.
- Setups fixed; rewrites to category invariants; invariant pins confirmed
  preserved (the §7 list checked off).
- Net lines, total and per crate.
- Proposed `.rules` additions discovered en route (dedicated rules-only
  commit; operator-approved).
- Production-code findings filed as follow-ups (§3.7).
- Known-failure attribution outcomes (which non-green tests were
  infrastructure vs stale).

**Plan-file lifecycle:** mark executed in §9; then delete this file in the
closing change — recording the successor (the saved final report) in the
README lifecycle ledger and restoring corpus-size parity (the README line
bumped back down) in the same change.

## 6. Shared-tree discipline (all phases)

- Pathspec-limited commits only; `git diff --cached --stat` before every
  commit (the index check is necessary, not sufficient).
- No working-tree-wide mutations at any point: no bare `git stash`, no
  `git restore .`, `git checkout .`, `git reset --hard`.
- Never read a file in the same parallel tool batch that edits it.
- One kanban state move per task per batch (parallel state moves race).
- Commit messages are plain text — no markdown fences (the
  `kask/githooks/commit-msg` hook rejects them).
- `.rules` edits are never inline in feature work — proposals go in the change
  report or a dedicated rules-only commit.

## 7. Keep-list — never delete

1. D-seam pins: tests pinning behavior disabled by a `// zed-kask:` comment
   (DIVERGENCE.md D1–D89; 68 active seams at composition time).
2. Regression pins: the legitimate case a restricting fix must not break.
3. Category invariants derived from the tree at test time (walks and
   enumerations; never hardcoded name lists).
4. Degradation-surfacing tests (reason named; never empty == success).
5. CI-gate contract tests: the per-server `Parameters<T>` tool-behavior tests
   (`check-mcp-tool-tests.sh`; ratchet completed 2026-08-26, allowlist empty).
6. Tests named in `.rules` or DIVERGENCE.md (e.g. the O_NOFOLLOW
   inside-root-symlink boundary pin, the `collect_seedable_files`
   filesystem walk) — governance-referenced pins; deletion requires the
   governance reference updated in a dedicated change first.

## 8. Out of scope

New coverage or features; benches; production-code changes (§3.7 files them);
`kask/scripts` bash selftests (reference-sweep targets, not review targets);
upstream zed files outside the §3.6 mini-batch; `evaluate-test-evidence.sh`
harness runs.

## 9. Execution log

**2026-10-11 — execution started.** Phase 1 re-enumeration: 32 crates / 17
integration test files / 229 inline test-bearing files / 67 zed-side
candidates — unchanged from §10; the composition-time tables stand. Plan
committed at aee548085a. Board `test-suite-refresh` created with default
columns (Backlog / Ready / In Progress / Review / Done); the §1 WIP-1 limit
is enforced procedurally (default columns carry no per-column limits). 33
tasks created. Foreign in-flight edits noted at start: hkask-memory
(federated_recall.rs, tests/federated_source.rs) and hkask-mcp-media
(assets.rs, hkask_mcp_media.rs, tools/educt.rs, tools/processing.rs) — those
batches defer per §2 entry criteria while the edits persist.

**2026-10-11 — pilot (hkask-email): KEEP, no findings.** 1 file, all signals
clear (green 8/0/0; typed-variant error assertion form; Display strings live
in src; no drift — tests and production share one file last touched
together 2026-08-29, e1d2312711; no keep-signature). No modifications → the
commit path remains unvalidated; the first batch with findings validates it.

**2026-10-11 — scale calibration (hkask-mcp-corpus): KEEP ALL, no findings.**
25 files → 22 healthy + 3 confirmed keep-list pins (property_tests.rs
invariant tests; gather.rs D-seam F4 pin territory; hkask_mcp_corpus.rs
registry-derived count/schema pins). 0 suspect, 0 needs-intent-reading →
deep-review scope zero. Suite green 243/0/0 — the six stash-attributed
corpus failures in session history are gone (re-established from the fresh
run, not carried forward). check-mcp-tool-tests.sh exit 0.

**2026-10-11 — hkask-tool-port: no findings by enumeration** (zero test
files in both the composition-time and execution-time enumerations).

**Funnel refinements (calibration output).** (1) For green tests, signal (b)
is subsumed by signal (c): a passing assertion cannot pin a divergent
literal — the (b) literal-grep runs only on non-green files, as an
attribution aid. (2) The largest crates are the freshest (corpus: every test
file touched within 30 days) — funnel yield concentrates where drift exists,
not where file count is high; the §3.2 descending-count order stands (triage
is cheap), with findings expected from the drifted tail. (3) The funnel
certifies "not stale," not "minimal" — green tests exercising dead production
surface are §3.7 follow-up territory, invisible to the funnel by design.

**Procedure corrections (observed live).** (1) The board rejects direct
backlog→in_progress moves; the §2 claim is two sequenced moves
(backlog→ready→in_progress), and closure is in_progress→review→
verify(evidence)→done — kanban_task_verify performs the review→done
transition. (2) kanban_task_comment + kanban_task_move on the same task in
one parallel batch: the comment's row write reverts the move's status write
(observed: the move returned new_status=ready, the subsequent board listing
showed backlog with the comment present; reported to the curator as a
tool-implementation issue). Rule: one row-mutating kanban call per task per
batch — comments included. Proposed .rules addition, operator call; not
edited inline.

**2026-10-11 — batches 4–9 (hkask-bridge-ontology, kask_bridge,
hkask-mcp-swarm, hkask-mcp-prediction-markets, hkask-mcp-companies,
hkask-types): KEEP ALL, no findings.** All six green (61+4 / 272 / 223 /
85 / 167+25 / 88+1 passed; 0 failed, 0 ignored anywhere), all drift under
45 days (oldest: swarm contract.rs and task_board.rs, 2026-08-29, 43
days), no ignored tests. 43 green pin-candidate files kept across the six
crates (incl. swarm's port_registry.rs — the .rules-named PortRegistry
surface). 0 suspect, 0 needs-intent-reading across 96 test files →
deep-review scope zero. No modifications → no crate commits; the commit
path remains unvalidated.

**Funnel simplification (green pin-candidates).** For green suites,
pin-candidate confirmation reduces to marker-present + suite-green:
whether the marker sits in test code or a production comment routes to the
same verdict (keep), so the distinction is skipped.

## 10. Composition-time scope manifest (2026-10-11)

Sizing input only, captured mechanically at composition time. Phase 1 re-runs
the enumeration at execution start; execution-time numbers are authoritative.

**kask/crates/ (19 crates — 5 integration test files, 99 inline test-bearing
files):**

| crate                    | integration | inline |
| ------------------------ | ----------- | ------ |
| kask_bridge              | 0           | 17     |
| hkask-bridge-ontology    | 1           | 17     |
| hkask-types              | 0           | 14     |
| hkask-regulation         | 0           | 9      |
| hkask-storage            | 2           | 8      |
| hkask-inference          | 0           | 7      |
| hkask-condenser          | 0           | 5      |
| hkask-memory             | 1           | 4      |
| hkask-mcp-server         | 0           | 4      |
| hkask-forecast           | 0           | 3      |
| hkask-spreadsheet        | 0           | 2      |
| hkask-lisp               | 0           | 2      |
| hkask-keystore           | 0           | 2      |
| hkask-steer-core         | 0           | 1      |
| hkask-services-core      | 0           | 1      |
| hkask-mcp                | 1           | 1      |
| hkask-event-store        | 0           | 1      |
| hkask-email              | 0           | 1      |
| hkask-tool-port          | 0           | 0      |

**kask/mcp-servers/ (13 crates — 12 integration test files, 130 inline
test-bearing files):**

| crate                        | integration | inline |
| ---------------------------- | ----------- | ------ |
| hkask-mcp-corpus             | 0           | 25     |
| hkask-mcp-media              | 1           | 23     |
| hkask-mcp-swarm              | 0           | 17     |
| hkask-mcp-prediction-markets | 0           | 15     |
| hkask-mcp-companies          | 1           | 14     |
| hkask-mcp-research           | 1           | 11     |
| hkask-mcp-training           | 0           | 7      |
| hkask-mcp-curator            | 1           | 6      |
| hkask-mcp-kata-kanban        | 3           | 5      |
| hkask-mcp-portfolio         | 2           | 3      |
| hkask-mcp-scenarios          | 1           | 2      |
| hkask-mcp-experimentation    | 1           | 2      |
| hkask-mcp-spreadsheet        | 1           | 0      |

**Totals:** 32 crates; 31 test-bearing; 17 integration test files; 229 inline
test-bearing files; 246 test files. Zed-side fork-seam candidates: 67 files
carrying both a `zed-kask` marker and test attributes (filtered to test modules
at §3.6). Batch estimate: 32 crate batches + 1 zed-side mini-batch, with the
>15-file sub-batch rule likely splitting hkask-mcp-corpus, hkask-mcp-media,
hkask-bridge-ontology, kask_bridge, and hkask-mcp-swarm — a multi-session
effort, which is why board state is durable (§1 step 5).
