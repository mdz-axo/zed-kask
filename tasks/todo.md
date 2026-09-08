# Kask reliability checklist

Source: [plan.md](plan.md). Baseline `2475305420`; created 2026-09-07.

**Implementation authorized 2026-09-07.** T01 and D01 are now verified (2026-09-07): RED observed pre-fix, GREEN post-fix, crate suites, companies compatibility, check and clippy all run — exact evidence in [plan.md](plan.md). T02–T15 are untouched. Technical closure owner: receiving coding agent. Policy owner: operator. Checked items below certify only the named activity, not the whole repair.

## Before implementation

- [x] Implementation authorized by "D01 yes. please proceed"; D01 retention explicitly ratified.
- [x] Recheck tree/HEAD; coordinate one editing process and build-directory availability. (No competing build process found; one adequate 30-minute bounded window replaced the 180-second attempts.)
- [x] Recover each selected task's spec/callers/test seam; confirm scope and policy gates.
- [x] Read the constraining architecture doc and name its invariant before editing. (Magna Carta P1 user sovereignty: failures must not silently destroy user data; IS/OUGHT: only claim verified enforcement.)

## Resume first — close existing work before broadening scope

- [x] T01 implementation and two public-tool regressions written.
- [x] D01 transactional ownership guard and five recovery tests written/adapted.
- [x] D01 recorded in both owning reference documents with date and superseded policy.
- [x] Independent static review completed; no definite new production defect found, one test weakness identified.
- [x] Fix T01 test weakness: assert recovered identity/probability/outcome BEFORE retry, and exact journal entries rather than only non-emptiness. (Read seam: `scenario_status` `recent_forecasts`; full two-entry journal comparison; Brier discriminator.)
- [x] Add T01 published-snapshot/uncleared-journal replay fixture; assert exact-once recovered state. (`snapshot_with_uncleared_journal_recovers_exactly_once`, production-written bytes for both files.)
- [x] Arrange a sufficient bounded build window. (30-minute window; dependency compilation completed; clippy wrapper run 35m49s clean.)
- [x] Execute focused regressions and controls; establish honest pre-fix failure evidence safely. (RED observed: 3 scenarios persistence failures + portfolio research-destruction probe, both against `git show HEAD:` worktree-only reverts with the index untouched; probe deleted after capture.)
- [x] Run affected crate suites/checks and `./script/clippy`; format/check only touched files. (18+5 focused, 44+2+5+2+18 crate suites, 68 companies, `cargo check` ×3 crates, clippy wrapper clean, presence ratchet 0 violations.)
- [x] Update scenarios persistence reference and D01 enforcement citations after validation; preserve ratified intent. (`scenarios.md` persistence section added; `portfolio.md`/`companies.md` enforcement citations updated with verified evidence; prior-snapshot-survival documented as design-reviewed, not test-pinnable.)
- [x] Record exact outcomes and remaining gaps in plan/todo. (Evidence table in plan.md; remaining: operator review at Checkpoint A.)

## Phase A — Protect boundaries and recoverable data

- [x] **T01 — Preserve scenario recovery on snapshot failure** (verified 2026-09-07; awaiting operator review at Checkpoint A)
  - [x] Failed snapshot/publication preserves journal and acknowledged recoverable records across reopen.
  - [x] Successful publication precedes cleanup; failure-then-retry remains recoverable. Publication/truncation crash window covered.
  - [x] Public response surfaces persistence failure; file-backed regression and control validated. (Prior-snapshot-survival across failed replacement is design-reviewed, not fixture-pinnable — see scenarios.md.)
- [x] **T02 — Enforce extraction destination policy across redirects** (core slice verified 2026-09-07; T02b follow-up open; awaiting operator review at Checkpoint A)
  - [x] Forbidden redirect sentinel receives no request (E2E over real sockets: loopback literal, 169.254.169.254, and localhost-name hops each reject with zero sentinel requests; multi-hop bound/cycle/permitted semantics pinned by the decision tests).
  - [x] Permitted redirects/direct extraction work (redirects remain followed under the same reqwest machinery, gated per hop; permitted public-literal hops pass the decision tests; direct extraction is behavior-identical for the no-redirect path); loops fail within a bound (10 hops, cycles refused, surfaced errors).
  - [x] Connection address matches validated destination (connect-time validating resolver closes the DNS-rebinding TOCTOU at this transport; address-family variants hard: 0.0.0.0/8, ::, NAT64 64:ff9b::/96, IPv4-compatible; proxy behavior made explicit: raw fetch ignores proxy env with a build-time warn — permissive RSS policy byte-identical).
  - [x] **T02b:** `rss_discover_feeds` fetched through the shared unvalidated `rss_client` after strict initial-URL validation — **fixed and verified 2026-09-07**: dedicated `discover_client` (shared `validated_fetch_client`) wired through `ResearchServer::new`; RED observed test-first (redirect followed into the loopback sentinel pre-fix); `discover_feeds_rejects_redirect_to_loopback`, `discover_feeds_direct_fetch_still_works`, `rss_discover_feeds_rejects_unspecified_destination` green; research suite 36 lib + 31 tool-behavior, clippy/check clean.
- [ ] **T03 — Restrict forgetting to distilled content** (no dependency)
  - [ ] Newer-than-watermark turns/embeddings remain semantically recallable.
  - [ ] Covered eligible content is deleted; watermark/never-distilled content is preserved; repetition is idempotent.
  - [ ] Insertion during eligibility/deletion cannot erase fresh content; mixed-age deletion policy is resolved.
- [ ] **Checkpoint A:** cumulative regressions, affected crate tests/checks/lints, scope/residue review, operator review. (T01 evidence complete; operator review remains.)

## Phase B — Make completion and retry states reliable

- [ ] **T04 — Revisit pending distillation work** (T03 for integrated safety checkpoint)
  - [ ] Active-at-scan work distills when later idle without a new turn or restart.
  - [ ] Transient pre-watermark inference failure retries; successful work is not unnecessarily replayed.
  - [ ] Watermark-before-insert/startup bounds preserved; pending-work policy resolved; real cursor progression tested.
- [ ] **T05 — Reserve session authorization before external dispatch** (no technical dependency; settlement policy gate)
  - [ ] Concurrent shared-store authorization cannot oversubscribe the session or dispatch beyond capacity.
  - [ ] Success settles once; proven pre-dispatch rejection releases only its reservation; existing ceilings/single-use controls pass.
  - [ ] Ambiguous post-acceptance/persistence failure retains durable evidence across restart; recovery policy ratified before coding.
- [ ] **T06 — Preserve replay protection after agent creation** (no dependency)
  - [ ] Post-spawn failure plus retry produces one agent and an explicit replay/pending/partial outcome.
  - [ ] Concurrent same-key attempts and reopened durable claims remain protected within the existing TTL.
  - [ ] Clean pre-effect rejection remains retryable; worktree/local failure paths and ephemeral-goal controls pass.
- [ ] **Checkpoint B:** cumulative memory/budget/spawn checks, scoped builds/lints, policy-gate review, operator review.

## Phase C — Make regulation acknowledgments truthful

- [ ] **T07 — Report actual directive outcomes** (no dependency)
  - [ ] Applied acknowledgments correspond to supported effects; failed/unsupported variants are distinct.
  - [ ] Dampening does not imply application; existing cap/threshold behavior remains intact.
  - [ ] No autonomous actuator or new authority; acknowledgment compatibility reviewed; inbox-to-event tests pass.
- [ ] **T08 — Deliver explicit domain escalations** (depends on T07)
  - [ ] Domain/severity/evidence reaches the existing human-review queue/channel; both paths tested.
  - [ ] Missing/broken sinks are visible; queued/attempted/confirmed persistence is distinguished despite the current best-effort sink.
  - [ ] Dampening/general alerts remain correct; explicit concerns are not fabricated sensor readings; routing gate resolved.
- [ ] **Checkpoint C:** cumulative regressions, affected integration build/lints, operator review before follow-up scheduling.

## Follow-up queue — elaborate before execution

Each item is owned by the coding agent for task elaboration. Completion requires a full scoped regression-first task, controls, actual validation evidence, and operator scheduling; these are not implementation-ready tickets.

- [ ] **T09 — Passage deletion ownership:** own passage text/vector removed, siblings retained, valid semantic neighbor recovered; reconcile T03 identity choices.
- [ ] **T10 — Exact harness comparison:** exact detected pair and metric retained; trailing/interleaved events do not suppress verification.
- [ ] **T11 — Market-identity calibration:** distinct markets count independently, rescans do not; legacy identity/migration gate resolved.
- [ ] **T12 — Cap-reset evidence:** exhausted-agent evidence preserved; replenishment alone earns no advice progress; dispatch bounds intact.
- [ ] **T13 — Retrain finalization:** existing placeholder gains verified durable metadata/comparison; repeated polling idempotent; failure control.
- [ ] **T14 — IPC discovery convention:** publish/discover works across XDG/UID cases with env precedence/private-directory protections.
- [ ] **T15 — Skill-feedback sensing:** recover intended production writer/spec; actual feedback reaches drift consumer with provenance; absence stays unobserved.
- [ ] Expand follow-up tasks and place cumulative review checkpoints after each group of two or three.

## Operator decision — D01

- [x] Operator ratified YES on 2026-09-07: companies notes/forecasts must survive portfolio recovery.
- [x] Decision/date/superseded whole-file-disposal scope recorded in portfolio/companies references.
- [x] Scoped implementation: compatible shared startup works; incompatible mixed DB refuses reset; portfolio-only recovery is transactional; no unlink/reopen.
- [x] Five schema-recovery tests written/adapted; portfolio explicit-file rustfmt/diff checks passed.
- [x] Execute all five recovery tests, portfolio suite/check/lint, and affected companies compatibility checks. (5/5 focused; 44+2+5+2 full crate suite; 68/68 companies lib; `cargo check` ×3; clippy wrapper clean — 2026-09-07.)
- [x] Confirm preserved research rows/parents/schema on refusal and rollback; verified via observed pre-fix RED and post-fix GREEN.

## Per-task closeout

- [ ] Observed behavioral RED before fix; regression and controls GREEN afterward.
- [ ] Targeted and affected crate tests/checks plus `./script/clippy` actually run; exact outputs recorded.
- [ ] MCP presence ratchet checked without representing it as sufficient behavior coverage.
- [ ] Scope, stale comments, dependencies, fixtures, documentation, and response compatibility reviewed.
- [ ] Unresolved decisions and blocked verification remain explicit; no unsupported completion claim.
- [ ] No commits/branches or parallel tree editing without authorization.
