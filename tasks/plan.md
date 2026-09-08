---
title: "Kask regression-first reliability plan"
creator: "Zed coding agent"
date: "2026-09-07"
type: "bibo:Document"
status: "Implementation authorized; T01 and D01 code present, verification blocked; handoff requested"
baseline: "2475305420ae065b5d1792c0f25cea471e558ae3"
---

# Kask regression-first reliability plan

## Target condition

Authorization history: the operator first requested this regression-first plan, then on 2026-09-07 said **"D01 yes. please proceed"**, ratifying research retention and authorizing implementation. The latest instruction requests a continuation prompt, current plan/checklist, and handoff to another agent. Implementation is paused at that boundary. No commits, branches, unrelated policy changes, or acceptance of deferred risks were authorized.

The target is to prevent the identified loss of recoverable data, private-network boundary bypass, over-authorized external dispatch, duplicate agent creation, and false completion evidence. Fix one observable failure path at a time; deepen existing modules only after behavioral tests pass.

Audit findings are source-backed hypotheses with concrete triggers, not reproduced incidents. **T01 and the D01 retention guard have implementation and tests in the working tree, but are not verified or complete. T02–T15 are untouched.** Owner for technical closure is the receiving coding agent; unresolved policy decisions remain with the operator. Later-wave tasks remain tracked, not silently abandoned or declared safe.

## Current handoff — 2026-09-07

Read [kask-reliability-continuation-prompt.md](kask-reliability-continuation-prompt.md) first for the exact resume sequence and build evidence. HEAD remains `2475305420ae065b5d1792c0f25cea471e558ae3`; no commits or branches were created. Preserve all current changes.

**Out-of-band index mutation (observed 2026-09-07):** an external process staged the full working tree at least twice during the receiving agent's session — once before any receiving edits (outgoing agent's work + untracked task docs) and once mid-session (capturing the strengthened scenarios tests before the final fixture reorder). The receiving agent did not stage, unstage, or reset anything; worktree content was authoritative throughout and RED/restore steps used worktree-only `git show HEAD:path > path` redirection so index state was never mutated. The operator should identify what stages this tree; if it is another agent process, the single-editor rule is already being violated.

| Work | State | Next action / owner |
|---|---|---|
| T01 scenario persistence | **Verified 2026-09-07** — RED observed pre-fix, 18/18 tool-behavior tests GREEN post-fix, clippy/check clean; three persistence regressions including the new publication/truncation crash-window boundary | Receiving agent: operator review at Checkpoint A; then T02 |
| D01 policy | **Ratified YES**, recorded in both owning references; enforcement citations updated with verified evidence | Closed pending operator confirmation |
| D01 retention guard | **Verified 2026-09-07** — RED observed pre-fix (research rows destroyed), 5/5 recovery tests GREEN, 68/68 companies suite GREEN, transactional refusal/rollback confirmed | Receiving agent: operator review at Checkpoint A |
| T02 extraction destination policy | **Core slice verified 2026-09-07** — redirect-per-hop gate, connect-time validating resolver, no_proxy explicitness, address-family hardening; RED reproduced the full SSRF pre-fix | Receiving agent: operator review at Checkpoint A |
| T02b discover-path transport | **Verified 2026-09-07 (test-first RED)** — `rss_discover_feeds` now fetches through the same validated client; sentinel-redirect regression, direct-fetch control, and tool-seam pin green; clippy/check clean | Receiving agent: operator review at Checkpoint A |
| T03–T08 | Not started | Resume after Checkpoint A operator review; retain checkpoint and task-specific policy gates |
| T09–T15 | Not started, require elaboration | Follow-up queue remains open; no accepted deferral |

**T01 test weakness (resolved):** the flagged weakness — counts checked after reopen, then re-scoring before asserting the stored outcome — is fixed as described in the T01 section. The publication/truncation crash-window boundary the review asked for is covered by `snapshot_with_uncleared_journal_recovers_exactly_once`.

**Build state (resolved):** the five 180-second timeouts were a window problem, not a code problem. With the incremental cache preserved and a 30-minute bounded window, the same command shape completed: dependency compilation finished and every test suite reached execution. `./script/clippy -p hkask-mcp-scenarios -p hkask-mcp-portfolio` (release/all-targets/all-features, `--deny warnings`, machete/typos/buf) completed clean in 35m49s. Do not return to 180-second windows for these crates.

**RED and GREEN evidence (2026-09-07):** pre-fix RED was observed against worktree-only reverts of the production files (`git show HEAD:path > path`, index untouched): all three scenarios persistence regressions failed for the intended reasons (persistence failures not surfaced to the caller), and a throwaway portfolio probe showed the unlink/recreate recovery destroying seeded research rows (`d01_red_evidence.rs`, deleted after capture). Post-fix GREEN: exact commands and outputs are recorded in the Definition-of-done evidence section below. No compilation-failure RED was claimed.

## Architecture constraints and provenance

- [Magna Carta](../kask/docs/architecture/core/magna-carta.md), lines 47–98: preserve user sovereignty and parent-held authority; distinguish intended capabilities from live enforcement. Do not add a parallel capability system.
- [Memory specification](../kask/docs/architecture/memory-system-specification.md), lines 599–675: idle-thread distillation; additive-only learning; watermark-before-lesson insertion; bounded six-hour startup discovery; single-copy, distillation-gated hard deletion. Do not turn forgetting into soft expiry or silently reverse watermark ordering.
- [Regulation D4](../kask/docs/diataxis/hkask-regulation/explanation.md), lines 22–54: human-applied advice, fresh evidence, original-threshold recovery, unverified causality, no autonomous actuator.
- [MDS](../kask/docs/architecture/core/MDS.md), sections 2 and 7: name the actual entity and reaching flow; express user-facing expectations and observable postconditions. Attach contract annotations to tests/production seams during implementation, not fictional enforcement claims in this plan.
- Existing idempotency contract: [idempotency.rs](../kask/mcp-servers/hkask-mcp-kata-kanban/src/idempotency.rs), lines 26–41, retains uncertain effects as Pending. Preserve its documented TTL rather than promising eternal exactly-once execution.
- Portfolio-only schema reset remains deliberate. D01 now explicitly forbids deleting shared research: see the ratified [portfolio recovery decision](../kask/docs/reference/mcp-servers/portfolio.md) and [companies ownership reference](../kask/docs/reference/mcp-servers/companies.md). The current guard refuses incompatible mixed databases rather than migrate or cascade-delete their research parents.
- Audit history anchors include `4dfcb76464` (hard deletion), `87652e946f` (memory deletion hardening), and `661c8e09e0` (tool behavior tests). Recover full task-specific commit intent before each implementation; the audit is not a replacement specification.

No upstream edits, generic transaction framework, or new service crate are presumed. T01 adds the existing workspace `tempfile` dependency to scenarios for same-directory atomic snapshot publication and temporary test fixtures; `Cargo.lock` records that one direct dependency edge. If an upstream change is necessary, stop to define its D-seam and regression pin. Only one agent edits the tree at a time. Read-only review may run in parallel.

## Execution protocol

Each task is a vertical slice: **contract → representative failing regression → minimal fix → control/failure variants → scoped validation**. One regression first is not a one-test ceiling. Tests must reach the real production behavior through its supported seam, with real temporary stores and controlled external-effect fixtures. No live providers, production DBs, or paid dispatches.

Before starting each task:

1. Recheck HEAD/worktree and recover callers, tests, spec history, and any applicable local rules.
2. Confirm the listed file boundary; locate an existing test seam. If scope exceeds one focused session or approximately five files, split into independently useful behavioral slices before editing. T02, T04, T05, and T11 particularly require this sizing check.
3. State the user expectation, motivating principle, applicable constraints, preconditions, postconditions, and measurable falsifier. Unknown user-visible behavior goes to an operator gate, not an implicit default.

T01 and D01 test names in the handoff now exist but have not executed. Other test names/fixtures below remain planned. Sizes S/M are provisional, not time promises.

## First tranche: concrete regressions

### Phase A — Protect boundaries and recoverable data

### T01 — Preserve scenario recovery on snapshot failure

**Scope:** M; lifecycle; `hkask-mcp-scenarios`. **Depends on:** none.
**Likely files:** `kask/mcp-servers/hkask-mcp-scenarios/src/superforecast/store.rs`, `src/hkask_mcp_scenarios.rs`, `tests/tool_behavior.rs` (latter paths relative to that crate).
**Anchor:** `ForecastStore::compact`, `scenario_score` → journal → snapshot → reopen.
**Status: verified 2026-09-07.** `save_entry`, `insert`, and `persist` propagate I/O errors; memory changes follow successful journal append; a synced same-directory `NamedTempFile` snapshot is published before journal cleanup, with Unix directory sync. The tool surfaces partial-progress/persistence errors. The review-flagged test weakness is fixed: `snapshot_failure_preserves_journal_for_recovery` now asserts recovered identity/probability/outcome through `scenario_status`'s `recent_forecasts` read seam BEFORE any re-scoring, and compares the retained journal's full two entries (pending insert + resolved update) instead of non-emptiness; it also asserts the failed publication leaves no temporary-file residue. New boundary regression `snapshot_with_uncleared_journal_recovers_exactly_once` covers the crash window between snapshot publication and journal truncation: a published snapshot with an uncleared overlapping journal recovers exactly once, with the journal's last write winning over a stale snapshot version (fixture uses production-written bytes for both files). The prior-snapshot-survives-failed-replacement property is design-reviewed but not test-pinnable — see the scenarios reference persistence section.

Acceptance:
- A failed snapshot write/publication leaves the prior recovery journal intact; reopening recovers every acknowledged durable record exactly once.
- Successful compaction publishes a complete replacement before journal cleanup; failure at publication and failure-then-retry preserve recoverability.
- The public scoring response surfaces persistence failure/degradation, rather than ordinary durable success; successful scoring behavior remains unchanged.

**Verification:** file-backed public-tool round trip with injected write/publication failures and a successful control. Use deterministic filesystem/driver faults, not permission assumptions that disappear when run as root. Assert exact recovered records and journal bytes. Explicitly distinguish tested I/O failure recovery from untested power-loss guarantees.
**Refused shortcut:** log snapshot failure then clear the journal, or add a test over an in-memory-only store.
**Skill match query:** persistence fault isolation and regression-first recovery testing.

### T02 — Enforce extraction destination policy across redirects

**Scope:** M; trust; `hkask-mcp-research`. **Depends on:** none.
**Likely files:** `kask/mcp-servers/hkask-mcp-research/src/research/providers.rs`, `src/research/providers/raw_fetch.rs`, `tests/tool_behavior.rs`; shared `hkask-mcp-server/src/security.rs` only if required by the real connection-validation seam.
**Anchor:** `web_extract` → `ProviderPool::extract_with_fallback` → RawFetch transport.
**Status: core slice verified 2026-09-07; follow-up slice T02b open.** Implemented at the real connection-validation seam, as the likely-files list anticipated:

- `RawFetchProvider` now owns a validated client (`raw_fetch_http_client`): a custom reqwest redirect policy re-runs the strict literal checks on **every redirect hop**, bounds the chain (10 hops) and refuses cycles; a validating DNS resolver (`reqwest::dns::Resolve` over `hkask_mcp_server::server::validate_resolved_addresses`) gates every connect-time resolution, closing the DNS-rebinding TOCTOU at this transport; `.no_proxy()` makes the connected destination always the validated one (surfaced with a build-time warn when proxy env vars are present). Redirect-fetched content is labeled with the final URL. RawFetch errors now carry the full reqwest cause chain (`reqwest_error_detail`) so a rejected hop names the reason.
- `hkask-mcp-server/src/security.rs` (shared seam, required): address policy extended with unspecified destinations (`0.0.0.0/8`, `::` — connecting to 0.0.0.0 routes to loopback on Linux) under the private gate, and NAT64 (`64:ff9b::/96`) / IPv4-compatible unmasking so neither family can re-spell a forbidden IPv4. The resolved-address loop was extracted (`validate_resolved_addresses`) and two public entries added (`validate_tool_url_literal` for the sync redirect-policy hop check, `validate_resolved_addresses` for the resolver), re-exported via `server.rs`. **The permissive (user-curated RSS) policy is byte-identical** for all new checks.

Acceptance:
- A public-to-private/loopback redirect never reaches the forbidden sentinel, including a multi-hop chain.
- Permitted redirects and direct public extraction still work; loops/redirect limits produce bounded, surfaced failure.
- The validated destination is the connected destination; resolver changes, address-family variants, and configured proxy behavior cannot silently bypass the gate. Unsupported transport configurations must be explicit, not falsely certified secure.

Coverage against acceptance (see research.md "Extraction destination policy" for the full boundary statements): forbidden single hops (loopback literal, 169.254.169.254, `localhost` name) are E2E-proven to make zero sentinel requests; chain bound, cycles, scheme/credential redirects, and permitted public-literal hops are pinned by the redirect-decision tests (a permitted multi-hop chain through locally-servable hops is not constructible under the strict gate — every local address is forbidden by policy); per-hop enforcement applies uniformly at every chain position via the same closure. Third-party provider APIs fetch target URLs server-side from their own network position — that fetch is not this transport and is documented as out of this gate.

**Verification:** real extraction transport with deterministic DNS/connection fixtures preserving production validation. Do not weaken the validator to make a loopback fixture pass. Check the exact pinned reqwest fork. No live attacker endpoint is needed. — **Executed 2026-09-07**: fork APIs confirmed (`redirect::Policy::custom`, `ClientBuilder::dns_resolver`/`no_proxy` — the builder method is `redirect_policy` on the zed fork); fixtures are loopback listeners with request counters, zero network. RED observed against worktree-only reverts (index untouched): the transport probe reproduced the full SSRF — pre-fix RawFetch followed the forbidden redirect and returned the sentinel's content (`Ok("sentinel-body")`) — and the three new tool-seam gate tests failed because `0.0.0.0`/NAT64 destinations passed validation. GREEN: research lib 34/34, tool_behavior 30/30, hkask-mcp-server lib 22/22; `cargo check` and `./script/clippy -p hkask-mcp-server -p hkask-mcp-research` clean.
**Refused shortcut:** initial-URL-only validation, disabling all redirects without reviewing compatibility, or mocking out the HTTP path under test. All refused as designed; redirects remain followed (validated per hop), the real transport is exercised over real sockets.
**Skill match query:** SSRF boundary testing through redirect and DNS transport behavior.

**T02b — follow-up slice (same class, separate wiring): verified 2026-09-07.** `rss_discover_feeds` validated its initial URL strictly but fetched through the shared `rss_client` (default redirect following). RED was observed test-first before the fix (a throwaway probe in `feed.rs` showed `discover_feeds` with the handler's client class following a redirect into a loopback sentinel and returning its content). Fix: the validated client construction is shared (`validated_fetch_client`, now `pub(crate)` from `raw_fetch.rs`, re-exported through `providers.rs`/`research.rs`), a `discover_client` field is wired through `ResearchServer::new` (all five call sites updated — production `run()` plus four test fixtures), the discover handler fetches through it, and its reqwest cause chain is rendered so a rejected hop names its reason. Permanent regressions: `discover_feeds_rejects_redirect_to_loopback` (sentinel counter zero, reason named) and `discover_feeds_direct_fetch_still_works` (validated client does not change direct fetches) in `feed.rs`, plus the tool-seam pin `rss_discover_feeds_rejects_unspecified_destination`. The wiring itself (handler passes `discover_client`, not the permissive `rss_client`) is a distinct typed field pinned by construction and review — a full tool-seam redirect E2E is not locally constructible (the strict outer gate rejects every locally-bindable initial URL). `rss_subscribe`/`rss_fetch`/`rss_synthesize` fetch with the permissive policy by ratified design; their redirects to local destinations are policy-permitted, not a defect.

### T03 — Restrict forgetting to distilled content

**Scope:** M; lifecycle; `hkask-mcp-curator`, storage seam if necessary. **Depends on:** none.
**Likely files:** `kask/mcp-servers/hkask-mcp-curator/src/forgetting.rs`, `src/distillation.rs`; existing memory/storage deletion implementation only if atomic eligibility/deletion needs it.
**Anchor:** distillation timer → forgetting → stored chunks/embeddings → semantic recall.

Acceptance:
- With an aged watermark and newer turns, newer turns and their embeddings remain recallable after forgetting.
- Eligible covered content is still hard-deleted; never-distilled threads and watermark records remain protected; repeating forgetting is idempotent.
- A controlled insertion between eligibility evaluation and deletion cannot erase the newly inserted content. Do not claim safety from a test of sequential checks only.

**Verification:** capability-complete memory store, mixed-age fixtures, bounded concurrent insertion, and post-forgetting semantic recall. Prefer the smallest safe existing lifecycle operation; conservative whole-thread deferral versus selective deletion must be checked against the retention contract before choosing.
**Refused shortcut:** replacing hard deletion with expiry or independently checking then unconditionally deleting the entity.
**Skill match query:** watermark coverage and concurrent memory-lifecycle regression testing.

**Checkpoint A:** T01–T03 regression evidence, affected crate tests/checks/lints, residue review, and operator review. These three tasks are technically independent; listed order is scheduling, not an artificial dependency.

### Phase B — Make completion and retry states reliable

### T04 — Revisit pending distillation work

**Scope:** M; curation/lifecycle; `hkask-mcp-curator`. **Depends on:** T03 for the combined memory safety regression checkpoint; cursor repair itself is independently implementable.
**Likely files:** `kask/mcp-servers/hkask-mcp-curator/src/distillation.rs`, `src/thread_turns.rs`, existing inline tests.
**Anchor:** `last_pass` → shared-turn discovery → idle check → inference → watermark.

Acceptance:
- A turn skipped as active becomes eligible and is distilled on a later production-shaped pass without requiring a new turn or restart.
- A transient inference failure before watermark advancement is retried; successful work is not replayed unnecessarily.
- Ratified watermark-before-lesson ordering and bounded startup discovery remain intact. Pending-work bounds/overflow must be explicit and non-silent; if the contracts conflict, stop for a decision rather than silently broadening retention or replay.

**Verification:** successive passes driven through the production orchestration/cursor, scripted inference, and controlled time; include success→repeat and failure→success. Preserve T03 integration coverage. Do not test only the helper with an epoch-wide scan.
**Refused shortcut:** moving the cursor without tracking unresolved work, or silently rescanning all historical memory.
**Skill match query:** asynchronous discovery progress versus completion-state test design.

### T05 — Reserve session authorization before external dispatch

**Scope:** M, re-slice if necessary; trust; `hkask-mcp-swarm`. **Depends on:** none. **Policy gate:** unknown-outcome reconciliation/refund behavior must be recovered or ratified before implementation.
**Likely files:** `kask/mcp-servers/hkask-mcp-swarm/src/spend_gate.rs`, `src/consent.rs`, existing server/consent tests.
**Anchor:** `authorize_delegate` → external POST → `complete_delegate` → durable consent state.

Acceptance:
- Two overlapping 10-credit authorizations against one 10-credit session admit at most one POST, including separate server instances sharing the consent DB.
- Success settles once; a proven pre-dispatch rejection releases only its own reservation; per-dispatch ceilings and single-use consent behavior remain intact.
- External acceptance followed by transport/local-persistence failure retains durable reservation evidence across reopen and reports uncertainty. A retry cannot reuse the same reserved capacity; no unsupported promise of external exactly-once delivery is made.

**Verification:** real SQLite consent store, barrier-controlled bounded HTTP fixture, failure before send/after acceptance/before local finalization, and reopen tests. Pin a reviewed reconciliation policy, not an invented timeout refund.
**Refused shortcut:** atomic deduction after the POST or automatically refunding every transport error.
**Skill match query:** atomic consent reservation, ambiguous external outcomes, concurrency tests.

### T06 — Preserve replay protection after agent creation

**Scope:** M; composition/lifecycle; `hkask-mcp-kata-kanban`. **Depends on:** none.
**Likely files:** `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs`, `src/idempotency.rs`, `tests/idempotent_creates.rs`.
**Anchor:** `with_idempotency` → successful spawn → fallible task comment → same-key retry.

Acceptance:
- Post-spawn bookkeeping failure followed by same-key retry creates exactly one agent; result is replayable or explicitly pending/partial.
- Concurrent same-key requests and retries after reopening durable state preserve the claim within the existing TTL, including failure after external acceptance before response recording.
- Proven clean pre-effect validation failures release the claim and can be retried successfully; ephemeral-goal semantics remain unchanged.

**Verification:** counting successful spawn port, real persistent replay store, one-shot post-spawn database fault, simultaneous retry and reopen controls. Exercise both worktree/local paths where their error classification differs.
**Refused shortcut:** classify all errors as clean, or test only an unavailable spawn port.
**Skill match query:** idempotent effectful tool retries and partial-success failure injection.

**Checkpoint B:** cumulative memory, authorization, and spawn regression checks; scoped build/lint evidence; explicit resolution of T05's policy gate; operator review. Serial editing is a resource constraint, not a T04→T05→T06 technical dependency.

### Phase C — Make regulation acknowledgments truthful

### T07 — Report actual directive outcomes

**Scope:** M; curation; `hkask-regulation`. **Depends on:** none.
**Likely files:** `kask/crates/hkask-regulation/src/cybernetics_loop/directive.rs`, existing regulation tests; acknowledgment consumers only if the recovered schema requires it.
**Anchor:** directive inbox → application handler → acknowledgment/event sink.

Acceptance:
- Applied acknowledgment requires evidence of the actual supported effect; failures and unsupported/log-only variants are explicitly distinguished.
- Dampened input is not represented as applied; working cap/threshold operations preserve their behavior.
- No capability authority or autonomous actuator is added to satisfy a formerly misleading acknowledgment; incompatible acknowledgment schema changes require review.

**Verification:** table of existing directive variants through inbox processing with real state assertions and a recording event sink; handler failures must not become success.
**Refused shortcut:** rename a log message while continuing to persist false completion.
**Skill match query:** typed command outcomes and production dispatch contract tests.

### T08 — Deliver explicit domain escalations

**Scope:** M; curation; regulation/bridge seam. **Depends on:** T07.
**Likely files:** `kask/crates/hkask-regulation/src/cybernetics_loop/directive.rs`, `src/cybernetics_loop/cycle.rs`, `src/algedonic.rs`, `kask/crates/kask_bridge/src/directive_bridge.rs`, existing bridge alert tests.
**Anchor:** `EscalateDomain` → inbox → existing `AlertEscalationSink` / `BridgeAlertEscalationSink` durable queue and existing alert channel.

Acceptance:
- An undampened explicit escalation retains domain, severity, and evidence in the human-review path; test queue persistence and alert delivery independently.
- Missing/broken sinks are surfaced; queued, attempted, and confirmed durable delivery are not conflated. The existing void/best-effort sink does not prove persistence success—recover or narrowly adapt its contract before claiming durable acknowledgment.
- Existing dampening/general alerts remain functional, without treating a requested escalation as a fabricated measured threshold breach.

**Verification:** bridge→inbox→real temporary escalation store plus channel receiver, missing-sink control and write-failure variant. If the existing sinks cannot faithfully carry an explicit concern, return the routing decision to the operator rather than invent policy.
**Refused shortcut:** record only the directive type as applied or invent autonomous remediation.
**Skill match query:** human escalation delivery with truthful persistence and channel outcomes.

**Checkpoint C:** cumulative directive and memory/budget regressions, build/lint evidence, and operator review before scheduling the follow-up queue.

## Follow-up queue: tracked, not execution-ready

These are not accepted deferrals or implementation authorization. Each remains owned by the coding agent for elaboration at Checkpoint C (or earlier if the operator reprioritizes). Before scheduling, expand each into the same three-criterion/failure-control format as T01–T08; re-slice anything larger than M. Source anchors and minimum completion evidence follow.

| ID | Target / source anchor | Required evidence before completion | Dependency / gate | Skill match query |
|---|---|---|---|---|
| T09 | Passage deletion ownership — `kask/crates/hkask-memory/src/memory_store.rs:596` | Delete/prune one passage; its stored text/embedding disappears, sibling chunks survive, and the next valid semantic match remains retrievable. | Reconcile with T03's actual deletion identity; no mandatory architectural dependency on T03. | Memory identity and relational/vector lifecycle consistency |
| T10 | Exact harness comparison — `kask/crates/kask_bridge/src/rollout_event_bridge.rs:132` | Production detector→verification preserves the exact 0.9→0.6 pair in a 0.4→0.9→0.6 history; interleaved verdicts do not suppress evidence; metric identity is retained. | None; recover both summary identities, not merely reverse an iterator. | Event history selection and regression feedback fidelity |
| T11 | Market-identity calibration — `kask/mcp-servers/hkask-mcp-prediction-markets/src/calibration.rs:103` | Five distinct 0.9/no markets yield five samples and Brier 0.81; rescans add zero; old stored observations are handled without fabricated identities. | Operator gate if legacy-data migration changes retained evidence; re-slice migration separately. | Calibration identity, deduplication and compatible persistence |
| T12 | Cap-reset evidence — `kask/crates/hkask-regulation/src/cybernetics_loop/cycle.rs:420` | Exhaustion is observed before replenishment; reset alone earns no advice-progress credit; dispatch limits still hold. | Reconcile T07/T08 edits to shared regulation code; no semantic dependency assumed. | Feedback measurement isolation across resource replenishment |
| T13 | Retrain finalization — `kask/mcp-servers/hkask-mcp-training/src/tools/status.rs:134` | Real pre-registration followed by fixture-backed successful manifest updates durable metrics/artifact information and comparison; repeated poll is idempotent; failure does not finalize. | Recover completion-manifest test seam first. | Adapter lifecycle finalization and external completion fixtures |
| T14 | IPC discovery convention — `kask/crates/kask_bridge/src/inference_socket.rs:49` | Publication→discovery works for absent/empty/populated XDG and non-1000 UID; env precedence/private directory protections remain intact. | No new upstream edits without a D-seam decision. | Secure runtime path publication and discovery round trips |
| T15 | Skill-feedback sensing — `kask/crates/hkask-regulation/src/runtime.rs:776` | Actual skill completion/operator feedback reaches the shared ledger and drift consumer with provenance; absent input remains explicitly unobserved. | Spec/writer ownership recovery before implementation; no deleting intended capability, invented skill identity, or substitute LLM success labels. | Spec-preserving feedback producer integration |

After elaboration, schedule follow-up checkpoints in groups of two or three tasks, each with cumulative tests/builds and human review. Do not treat this table as sufficient implementation design.

## D01 — Shared-database retention decision

**Policy owner:** operator. **Decision:** YES, ratified 2026-09-07 by "D01 yes. please proceed". **Implementation/verification owner:** receiving coding agent. Policy is resolved; the code is present but not yet verified.

Companies research notes and forecasts must survive portfolio schema recovery. The decision and superseded whole-file-disposal scope are recorded in `kask/docs/reference/mcp-servers/portfolio.md` and `companies.md`. Preserve attachment metadata and referenced portfolio parents too. Portfolio-only legacy data remains disposable; no research migration is authorized by this decision.

**Implemented approach:** `open_with_schema_recovery` in `kask/mcp-servers/hkask-mcp-portfolio/src/store.rs` uses an IMMEDIATE transaction for DDL, ownership inspection, and recovery. Any non-internal table outside the five portfolio tables prevents a destructive reset. Portfolio-only recovery drops owned tables child-first and rebuilds them transactionally; failures roll back. No database unlink/reopen remains. Incompatible shared databases return an explicit error; compatible shared databases open normally. This availability trade-off was communicated to the operator; do not silently add migration or cascade deletion to avoid the error.

**Five tests in `src/tests.rs`:** `schema_recovery_discards_stale_portfolio_data` (existing control adapted), `schema_recovery_preserves_mixed_database`, `schema_recovery_refuses_unknown_non_portfolio_table`, `schema_recovery_rolls_back_failed_portfolio_reset`, `schema_recovery_opens_compatible_mixed_database`. Mixed fixtures preserve notes/files/forecasts, revisions/outcomes, parent rows, schema, and FK integrity. The helper is `pub(super)` for the crate-local seam; no new public API/dependency was added.

**Validation (executed 2026-09-07):** pre-fix RED observed via a throwaway probe (the HEAD unlink/recreate recovery destroyed seeded research rows in a mixed DB; probe deleted after capture). Post-fix: `cargo test --offline --locked -p hkask-mcp-portfolio --lib schema_recovery` → 5/5 passed; full crate suite → 44 lib + 2 + 5 + 2 across targets, 0 failed; companies compatibility → `--lib` 68/68 passed; `cargo check` and `./script/clippy -p hkask-mcp-scenarios -p hkask-mcp-portfolio` clean (clippy release/all-targets/all-features `--deny warnings`, machete/typos/buf clean). Enforcement citations updated in `portfolio.md` and `companies.md`. The user-visible trade-off (incompatible shared DB errors at startup) remains as communicated; no research migration was added.

## Risks and open gates

Probabilities/frequencies are unmeasured. Severity is conditional on reaching the audited path, not a claim of incident frequency.

| Risk if outstanding | Severity | Owner / closure path |
|---|---|---|
| Private-network extraction through redirects | High | Coding agent, T02 regression and transport gate |
| Lost recovery journal or undistilled turns | High | Coding agent, T01/T03; T04 prevents skipped learning |
| Over-authorized spending or duplicate spawned work | High | Coding agent, T05/T06; operator ratifies any unresolved settlement policy |
| False compliance or lost explicit escalations | High | Coding agent, T07/T08; no false durable-success claims |
| Cross-domain research loss during schema recovery | High | D01 policy ratified; receiving agent must validate the implemented guard before closure |
| Stale vectors, biased calibration, misleading harness/cap feedback | Medium–High | Coding agent, elaborate T09–T12 at Checkpoint C |
| Incomplete adapter metadata, unavailable fallback IPC, unsensed skill drift | Medium | Coding agent, elaborate T13–T15; recover interfaces/spec first |
| Missing production-shaped test seams increase task size | Medium | Coding agent, mandatory pre-task sizing/design gate |
| Dependency compilation exceeds the current 180-second window | Verification blocker | Receiving agent coordinates a longer bounded window; preserve incremental cache; no repeated timeouts, lock deletion, or killing other builds |

Unresolved details are gates, not permission to invent behavior: mixed-age deletion policy (T03), bounded pending-work behavior (T04), unknown-outcome credit reconciliation (T05), acknowledgment/delivery semantics (T07/T08), legacy calibration evidence (T11), skill-feedback ownership (T15). Research retention (D01) is resolved; only its implementation verification remains open.

## Definition of done and verification

For each implemented slice:

1. Observe the new regression fail on the pre-fix path for the intended reason. Compilation failure is not RED evidence of the behavioral defect.
2. Observe the regression and controls pass after the fix. Use capability-complete constructors; no empty-result degradation-as-success assertions.
3. Run targeted tests first, then affected crate test suites and `cargo check`, followed by `./script/clippy -p` for affected packages. Use bounded timeouts and limited parallelism. GPUI tests use its tracked executor; live-mutation suites, if separately authorized, run single-threaded.
4. Run `bash kask/scripts/check-mcp-tool-tests.sh` as a presence ratchet, never as proof of transition coverage. At checkpoints build the affected application integration where touched; any blocked build is reported as blocked, not waived.
5. Review diff scope, stale comments, obsolete dependencies, temporary fixtures, changed error/schema behavior, and documentation against recovered intent. No commits/branches without explicit instruction.
6. Record exact commands, observed output, test names, and residual risks. A task is not complete when verification is blocked.

Planning baseline: earlier audit's MCP test-presence ratchet passed. Its test build timed out waiting for a lock; implementation-phase builds instead progressed through dependency compilation. Exact attempted commands and outcomes are in the continuation prompt. Neither crate had been compiled to completion or tested at handoff; that blocker is now resolved — see the evidence record below.

### Verification evidence — 2026-09-07 (T01 + D01, all commands from the repository root)

| Command | Observed result |
|---|---|
| `cargo test --offline --locked -p hkask-mcp-scenarios --test tool_behavior -j 2` | 18 passed, 0 failed (persistence regressions included) |
| `cargo test --offline --locked -p hkask-mcp-portfolio --lib schema_recovery -j 2` | 5 passed, 0 failed |
| Pre-fix RED (worktree-only `git show HEAD:… > …` reverts; index untouched): scenarios `--test tool_behavior` | `snapshot_failure_preserves_journal_for_recovery`, `journal_failure_is_surfaced_before_memory_changes`, `snapshot_with_uncleared_journal_recovers_exactly_once` each FAILED for the intended reason ("persistence/journal failure must reach the caller" / score succeeded where a persistence failure should surface); 15 passed, 3 failed |
| Pre-fix RED, portfolio: throwaway `tests/d01_red_evidence.rs` probe (deleted after capture) | FAILED: research row read back as `"DESTROYED"` vs `"Durable research"` — HEAD's unlink/recreate recovery destroyed it |
| Post-restore GREEN re-run: scenarios `--test tool_behavior` | 18 passed, 0 failed |
| `cargo test --offline --locked -p hkask-mcp-scenarios -p hkask-mcp-portfolio -j 2` | 44 + 2 + 5 + 2 + 18 passed across targets, 0 failed |
| `cargo test --offline --locked -p hkask-mcp-companies --lib -j 2` | 68 passed, 0 failed (shared-DB compatibility) |
| `cargo check --offline --locked -p hkask-mcp-scenarios -p hkask-mcp-portfolio -p hkask-mcp-companies -j 2` | Finished, no errors |
| `./script/clippy -p hkask-mcp-scenarios -p hkask-mcp-portfolio` | clean: release/all-targets/all-features `--deny warnings`, plus machete (scoped to `kask/`), typos, buf lint/format; 35m49s wall |
| `bash kask/scripts/check-mcp-tool-tests.sh` | 0 violations, 0 allowlisted gaps (presence ratchet only) |
| `git diff --check` / `git diff --cached --check` | clean |
| `rustfmt --edition 2021 --config skip_children=true --check` on touched scenarios test file | clean |

### Verification evidence — 2026-09-07 (T02 core slice, all commands from the repository root)

| Command | Observed result |
|---|---|
| `cargo test --offline --locked -p hkask-mcp-research -j 2` | lib 34 passed (6 inline redirect/resolver regressions: loopback-literal sentinel zero, 169.254.169.254 rejection, localhost-name resolver rejection with zero sentinel requests, browse variant, decision bound/cycle/forbidden/permitted, resolver unit), tool_behavior 30 passed (4 outer-gate tests), 0 failed |
| `cargo test --offline --locked -p hkask-mcp-server --lib -j 2` | 22 passed including the 3 new validator tests (unspecified, NAT64/compatible, resolved-address list), 0 failed |
| Pre-fix RED (worktree-only `git show HEAD:… > …` reverts; index untouched): throwaway transport probe | FAILED as intended — pre-fix RawFetch followed the redirect to the loopback sentinel and returned its content `Ok("sentinel-body")`; probe deleted after capture |
| Pre-fix RED: `--test tool_behavior` (kept new, reverted production) | `web_extract_rejects_unspecified_destination`, `web_extract_rejects_nat64_loopback_destination`, `web_browse_rejects_unspecified_destination` FAILED (validator admitted them; pool returned PermissionDenied, not InvalidArgument); loopback control passed |
| `cargo check --offline --locked -p hkask-mcp-server -p hkask-mcp-research -j 2` | Finished, no errors |
| `cargo test --offline --locked -p hkask-mcp-media -j 2` (consumer compatibility: the media tools validate remote media URLs through the same shared strict validator that T02 tightened) | 210 + 61 + 1 passed, 0 failed |
| `./script/clippy -p hkask-mcp-server -p hkask-mcp-research` | clean: release/all-targets/all-features `--deny warnings`, machete/typos/buf clean (3 test-code lints fixed: slice-from-ref, redundant clone) |
| `rustfmt --check` on the three touched Rust files | clean |

### Verification evidence — 2026-09-07 (T02b discover-path slice)

| Command | Observed result |
|---|---|
| Test-first RED (before the fix, current tree): throwaway probe `feed.rs` `t02b_red_probe` driving `discover_feeds` with the handler's client class (plain default) against a loopback redirect fixture | FAILED as intended — the redirect was followed and the sentinel fetched (`discover returned: Ok([])`); probe deleted and replaced by the permanent tests |
| `cargo test --offline --locked -p hkask-mcp-research -j 2` (post-fix) | lib 36 passed (new: `discover_feeds_rejects_redirect_to_loopback` with named reason + zero sentinel requests, `discover_feeds_direct_fetch_still_works` control), tool_behavior 31 passed (new: `rss_discover_feeds_rejects_unspecified_destination`), 0 failed |
| `cargo check --offline --locked -p hkask-mcp-research -j 2` | Finished, no errors |
| `./script/clippy -p hkask-mcp-research` | clean (release/all-targets/all-features `--deny warnings`, machete/typos/buf) |
| `rustfmt --edition 2024 --check` on the six touched research-crate files | clean |

T02 scope/residue: files touched are the four declared ones (`security.rs`, `server.rs` re-export line, `raw_fetch.rs`, `tests/tool_behavior.rs`) plus `research.md` and these task docs; the throwaway probe and its `mod` wiring were removed (the out-of-band staging process had captured the probe mid-RED; it was verified absent from index and worktree afterward); stale-comment sweep done — the security.rs "future hardening" TOCTOU comment was rewritten in the same change to state that the raw-fetch transport closes the gap at connect time and other consumers keep the documented gap. T02b (`rss_discover_feeds` shared-client redirect hole) is recorded as an open follow-up slice.

Scope/residue review 2026-09-07: no files changed outside the declared boundary (two scenarios production files + their test file + Cargo.toml/Cargo.lock edge, portfolio store + tests, three reference docs, plan/todo); the throwaway RED probe was deleted; `/tmp/zed-kask-red-backup/` scratch backups are outside the tree; no stale comments found — the old warn-and-truncate/warn-and-admit comment blocks were updated with the implementation; machete confirms the new `tempfile` dependency is used. Operator review at Checkpoint A remains open.

## Refinement history

Independent proposal review scored deficiency 0.1625 (gate 0.15): failure boundaries and sizing needed clarification. Revised T05/T06 to cover external acceptance, persistence failure, concurrent retries, and restart; constrained T04 retry to preserve watermark/startup contracts; separated scheduling from technical dependencies; named T08 sinks and their best-effort limitation; explicitly marked follow-up headings not execution-ready. Added the unresolved policy gates and cumulative checkpoint requirements. Final plan review is recorded separately in the delivery message; no implementation correctness is implied by plan quality.

## Process anchors

- `pko:Procedure`: this regression-first program; `pko:ProcedureTarget`: the target condition above.
- T01–T15: `pko:Step`; each acceptance/verification field is its `pko:StepVerification`. T09–T15 require elaboration before execution.
- Phases A–C: `pko:MultiStep`; Checkpoints A–C: `pko:UserFeedbackOccurrence`.
- Risk rows: `pko:IssueOccurrence`; D01 and unresolved policy gates: `pko:UserQuestionOccurrence`.
- This document carries Dublin Core-style title/creator/date and `bibo:Document` metadata; [todo.md](todo.md) is its execution checklist.
