# Pre-Release Final Summary — Second Pass

Date: 2026-08-04. Second hardening pass over the pre-release continuation
prompt. First-pass verdict was READY; this pass audited that verdict, the five
fix commits that implemented the first pass's addendum, and the four commits
that landed after the first pass with no review at all.

## Release-readiness verdict: **READY**, with one open operator decision

No blockers. Everything actionable found by this pass has been fixed, tested,
and (mostly) committed. One decision needs the operator (D1 below).

## What this pass found that two prior passes missed

1. **The security regression gate was theater** (HIGH, fixed). Three
   independent vacuity classes (single-quote pattern corruption, dead include
   paths, inverted presence-semantics) made 20+ of the 39 "enforced" entries
   unenforceable, and RR-0044 was promoted to `enforced` while 17 production
   violations existed. The gate now: strips both quote styles, hard-fails on
   orphaned include paths (stale paths can never silently pass again),
   supports `semantics: presence`, and was negatively tested (synthetic
   orphan + synthetic presence-failure both fire). All entries repaired;
   17 RR-0044 sites triaged (6 mis-classifications fixed, 11 annotated with
   `rr0044-ok` markers). Gates green: kali 37 enforced/0 violations, lora
   1 enforced, unsafe-forbid 38/38.
2. **Corpus JSONL reads bypassed path containment** (MEDIUM, fixed). Six
   caller-supplied path sites (`read_jsonl`, `read_jsonl_lenient`,
   `corpus_prepare_training_dataset`) read arbitrary files uncapped. Now
   contained + size-capped at the helper level with escape tests.
3. **"Layer 3 instruction hierarchy" is advertised but not deployed**
   (MEDIUM, status corrected). RR-0010's include path never existed in this
   repo; no hierarchy text exists anywhere in kask source — only skill
   templates describing it. Downgraded to `pending` with provenance note.
4. **Two error-classification regressions in the unreviewed tail** (fixed):
   PDF-triage environment failures blamed on the caller; `JobStoreError::Storage`
   mislabeled retryable.
5. **Mapper-promotion discipline violated within the very commit that
   introduced it** (fixed): `map_semantic_memory_error` duplicated verbatim
   across two servers; `map_fs_error` re-implemented the shared `map_io_error`.
6. **`kask-ci.yml` was invalid YAML since `e37bef8a3b`** (HIGH, fixed): the
   new kali-regressions job's step name contained an unquoted `status:` colon,
   which spec-compliant parsers reject — GitHub would have refused the whole
   workflow file, disabling ALL kask CI jobs from that commit onward. Name
   quoted; file re-validated with a YAML parser (11 jobs parse). Additionally
   the test job carried dead steps (fixed): trace-collection + mutation steps
   referenced `kask/scripts/test` (deleted in `009b04066a`), silently
   no-op'ing behind `|| true`; cargo-llvm-cov installed and never invoked.
   Steps removed with a restoration note; `kali-regressions` job gained the
   missing toolchain/cache/deps steps.
7. **deny.toml drift** (fixed): stale `RUSTSEC-2026-0199` ignore removed;
   fictional `paste`-rationale corrected (real chain touches kask-owned
   `hkask-media-widget`); block re-review date added.

## What this pass re-verified as holding (first-pass fixes, all HOLD)

redact_spans merge-on-overlap (adversarially probed: adjacent, past-EOF,
empty, interleaved-scanner cases), GuardedStream cap + re-scan guard, IPC
timeout + guard-nulling on all four error branches, `debit_if_funds`
BEGIN-IMMEDIATE atomicity, memory data boundaries at all 4 injector paths,
OCAP Layer-4 fail-closed on all three paths, WebID redaction, swarm_panel
extraction byte-for-byte behavior preservation, unsafe-forbid 38/38,
cargo-machete clean.

## Open items

| # | Item | Owner |
|---|------|-------|
| D1 | Instruction hierarchy: deploy the text into a real system prompt (re-enforce RR-0010) **or** remove "Layer 3" claims from adversarial-red-team/kali-audit templates | Operator decision |
| D2 | `internal(e.to_string())` evasion spelling: 82 sites; sampled clean apart from the 2 fixed; widening RR-0044 requires full triage | Post-release |
| D3 | `swarm_panel.rs` still 4,112 lines; `render_swarm_detail`/`render_card` next extraction candidates | Post-release |
| D4 | Non-canonical warn targets (`hkask.mcp.cap`, `hkask.mcp`, `hkask.ledger`) on governance/ledger paths — same class as the fixed `reg.guard.redact` | Post-release |
| D5 | Marker-spoofable memory data boundaries — documented framing limit, consistent with Layer-7 posture | Accepted |
| D6 | Release version/changelog scope — still unconfirmed from the original prompt | User |

## Commit / working-tree state

- `c23f6f9661` — gate repairs + companies re-classification (committed by
  fix sub-agent).
- `61abea787f` — mapper consolidation + JSONL containment + annotations
  (recommitted by me after removing unrelated user WIP that the sub-agent's
  original `77eeaf70aa` had swept in).
- **Uncommitted (deliberately)**: `.github/workflows/kask-ci.yml` (dead-step
  removal + kali-regressions job hardening), `kask/deny.toml` (ignore
  refresh), and these four pass-2 QA reports — left for your review since no
  commit was requested. `tasks/plan.md`, `tasks/todo.md`, and
  `docs/reports/prediction-markets/*` contain your own in-progress work,
  untouched.
- Note: both sub-agents committed without being asked; flagged as a process
  observation.

## Validation run

730 lib tests green across the 8 touched crates; `cargo check` clean on all
touched crates; `cargo fmt --check` clean; `cargo deny advisories` ok; all
three regression gates green *after* demonstrating they can fail (synthetic
negative tests); `bash -n` on the modified script. Full-workspace clippy/nextest
not re-run (unchanged surfaces; CI covers them).

## Suggested .rules additions

1. **A CI gate must be shown to fail before its `status: enforced` is
   trusted.** The kali-regressions gate reported green for weeks while three
   independent defects (quote corruption, dead include paths, inverted
   semantics) made 20+ entries unenforceable — and a regression entry was
   promoted to `enforced` while 17 live violations existed. When adding or
   promoting a mechanically-enforced invariant, run the checker against a
   synthetic violation and confirm nonzero exit. (Non-obvious: the gate's
   *output* looked healthy — `39 enforced, 0 violations`; repeatedly
   encountered: three defect classes, 20+ entries; actionable: one negative
   test per new gate.)
2. **grep-based enforcement entries must fail on nonexistent include paths.**
   `grep … 2>/dev/null || true` over a stale path returns empty and passes
   forever; crate renames and repo merges silently orphan the gate. The
   checker now hard-errors on include paths that don't exist — when writing
   new RR entries, paths are relative to `kask/` (the script's cwd), never
   `kask/`-prefixed. (This generalizes "advertised invariants need
   enforcement points" to the enforcement mechanism itself.)

## Cross-cutting critic notes

- **Metacognition** (predictions scored): P1 "1–2 gaps in the post-report fix
  commits" (p=0.6) — correct in direction, magnitude underestimated (B1/B2 +
  M1/M2). P2 "≥1 finding in the four unreviewed commits" (p=0.7) — correct
  (CI gate vacuity is anchored in `e37bef8a3b`'s new job). P3 "test suite
  passes" (p=0.85) — correct. Brier ≈ 0.11; systematic bias: underestimating
  defect *depth* (found classes, not instances).
- **Essentialist**: candidate findings dropped at the gates: renaming
  `hkask.mcp.cap` targets (consumers unverified — deferred to D4), widening
  RR-0044 to `e.to_string()` (requires 82-site triage — deferred to D2),
  splitting corpus `helpers.rs` now (next-addition trigger suffices).
- **Pragmatic-semantics**: all fixed findings are IS-tier (verified against
  code + live gate runs). S2 (instruction hierarchy) provenance is IS-tier
  (git history checked, `--all`); whether it *should* be deployed is
  OUGHT-tier — hence D1. The first pass's F1 exploitability inference
  (llm-guard substring pairs) remains unresolved but moot post-fix.
- **Pragmatic-cybernetics**: the vacuous gate is a textbook broken feedback
  loop — sensor present, signal path severed, regulator (CI) reading a
  constant "healthy". The repair restores loop closure *and* adds
  self-monitoring (orphan detection = the loop now senses its own sensor
  failure). The dead CI trace/mutation steps were the same class: an
  effector wired to a deleted sensor, masked by `|| true`.
