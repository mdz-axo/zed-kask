# Continuation prompt — finish the Kask core review

Use this prompt in a fresh coding-agent session in `/home/mdz-axolotl/Clones/zed-kask`.

## Mission and authoritative inputs

Finish the work in `tasks/kask-core-review-2026-09-04.md`. Read the original findings,
acceptance criteria, dependencies, and checkpoint sections before changing code.
The operator has repeatedly asked to proceed; do not restart the review or
reimplement completed tasks. Finish implementation, verification, stale-comment
cleanup, and task-list closure. Do not mark implementation as verified without
running its acceptance tests.

At handoff, the latest commit is:

- `f868676f81` — `Harden delegation and regulation feedback`
- Previous: `475d8c73a1` — `Harden MCP lifecycle and inference dispatch`
- Previous: `93e9a89799` — `Fix core storage and configuration invariants`

The tree was clean before this continuation document and its task-list pointer
were written. The operator/external tooling committed and sometimes staged work
while the agent was active. The agent did not stage or commit. Recheck `git status`
and `git log`; compare against **HEAD**, not just the index, to see all changes.
Do not overwrite external work or create commits/branches unless requested.

The main review document still reports **13/20 verified tasks**. The six further
tasks below have substantial implementation in `f868676f81`, but final validation
and cleanup are unfinished. **T11 is not implemented.** Do not promote the count
to 19 or 20 based on the commit message.

## Operator decisions — preserve these

1. Repair the previous memory-forgetting migration without backward-compatibility
   shims. Remove obsolete expiry APIs, fields, and code paths. Physical deletion
   replaces the expired-memory state. The existing one-time forward schema
   conversion was retained to convert stored data, not to support old APIs.
2. **D1 / T04:** parent-held delegation grants; child requests can narrow but
   cannot enlarge their grant. This is not OS isolation from arbitrary same-UID
   processes.
3. **D2 / T05:** reject mismatched embedding providers before HTTP. Already
   implemented/tested in the preceding checkpoint; do not restore permissive
   prefix stripping or silent provider substitution.
4. **D3 / T13–T14:** admission-to-completion inference deadline; explicit overload
   and cancellation behavior; no automatic replay of unknown-effect work. Preserve
   the ratified AIMD policy, not the superseded stepped-ramp policy.
5. **D4 / T15–T17:** prompt sensing/recovery, **not weekly sensing**. Assess advice
   seven days after confirmed human action. Acceptance/no-worsening, observed
   progress, and causal attribution are different. Missing/stale evidence stays
   unknown. After this recommendation was presented, the operator instructed the
   agent to proceed with all seven remaining tasks; the session stated that it
   was taking that instruction as confirmation of these D4 semantics. Record
   that provenance rather than presenting seven days as an engineering invention.
6. **D5 / T11:** visible maintenance/recovery, settle writers, close consumers,
   preserve all DBs before publishing the new key, reopen and resume. The detailed
   implementation experience remains a design checkpoint; see T11 below.

## Immediate next steps — observed validation problems

### 1. Fix the stale regulation test, not the observation contract

Latest run:

```sh
cargo test --offline --locked -p hkask-regulation --lib
```

**59 passed, 1 failed**:

`cybernetics_loop::cycle::tests::sense_reports_algedonic_log_population`

At handoff, `kask/crates/hkask-regulation/src/cybernetics_loop/cycle.rs:1828`
asserted `clean log must not emit AlgedonicEvents`. Its first three assertions
still expect absent population signals. The implementation intentionally now
returns healthy zero-valued observations so recovery can be distinguished from
missing sensing. Inspect the whole test: assert actual zero values and no
`Deviation`, retain the degraded-state assertions, and correct its comments.
Do **not** restore healthy-signal suppression merely to satisfy this old test.

### 2. Complete the app build check

```sh
cargo check --offline --locked -p zed
```

The latest attempt **timed out at 120 seconds**, after package/build lock waits,
at `1958/1959: zed-kask(bin)`. No compiler error was captured. It is **not a pass**.
The operator's latest request was to compose this continuation prompt, so no
longer retry was run. In the resumed session, rerun with a suitable bounded
allowance and report the actual outcome. Avoid simultaneous debug Cargo builds
that spend the allowance waiting for one another.

### 3. Run final lint after finishing code

`./script/clippy` passed for the earlier checkpoint, **not for all changes in
`f868676f81`**. Run it for the affected crates and fix diagnostics introduced here.
Use the wrapper, not bare `cargo clippy`. Do not widen its Kask-only machete scope.

## State of the six implemented-but-unclosed tasks

### T04 — parent-held delegated-tool authority

Implemented:

- `kask/crates/kask_bridge/src/delegation_grants.rs`: parent registry; opaque UUID
  tokens; exact qualified tool membership; stable token for unchanged grants;
  replacement/revocation invalidates the old token; missing/malformed grants deny.
- `kask.mcp.delegated_tools`: map from **child server ID** to exact `server/tool`
  names. Added in bridge settings and `crates/settings_content/src/settings_content.rs`.
  Defaults are empty/deny; no wildcard or caller-defined grant-all fallback.
- `build_mcp_server_env` injects only the child's `HKASK_TOOL_GRANT` token.
- IPC params carry `tool_grant`; the IPC client reads the injected token.
- Server dispatch intersects the existing request allowlist with the parent
  registry **before** invoking the tool port; denial is `Auth`.
- `crates/zed/src/main.rs` revokes the grant before settings-driven unload.

Example configuration shape (illustrative, do not write the operator's settings):

```json
{"kask":{"mcp":{"delegated_tools":{"swarm":["research/rss_search"]}}}}
```

Qualified target names must match the actual runtime registration. Existing
agent-card allowlists alone no longer authorize IPC calls. This behavior must
be documented clearly; do not silently seed broad grants to restore old behavior.

**Fresh handoff test passes:**

```sh
cargo test --offline --locked -p kask_bridge --lib ipc_child_cannot_expand_parent_grant
cargo test --offline --locked -p kask_bridge --lib grants_are_stable_until_changed_and_revoked
```

Also passed earlier in the same work: the seven `dispatch_tool_invoke` tests.
Finish configuration/launch wiring coverage, settings-default/schema checks,
unload/reload/revocation review, and D8/D9 seam documentation. Audit token logging
and do not overstate the per-server grant as PID-bound or same-UID OS isolation.

### T13/T14 — bounded admission, cancellation, total deadline

`kask/crates/kask_bridge/src/inference_chat.rs` now contains:

- `RequestLifetime`: owned admission permit plus optional GPUI timer.
- Total accepted work bounded at **2 × configured concurrency**; active execution
  remains bounded by the existing concurrency semaphore. Admission is acquired
  before enqueue; unbounded channel types no longer imply unbounded queued tasks.
- `Overloaded` error when no admission permit is available; no provider dispatch.
- One timer created at admission covers queue wait, model resolution, stream
  establishment, and complete drain. `Duration::ZERO` deliberately disables the
  timer, not admission bounds or cancellation.
- Both receiver branches select caller-channel closure against deadline/work.
- `InFlightGuard` decrements the active counter on cancellation/timeout as well
  as normal completion.
- Shared `collect_completion` replaces duplicated handlers and establishment-only
  timers. Shared `stream_request` replaces three submission copies.
- `InferenceError::{Overloaded, Timeout}` and their IPC conversions were added.

`inference_ipc_server::handle_connection` now monitors the read side during
pending dispatch: EOF drops local dispatch; pipelined requests are refused.
The current client opens a fresh socket per request and does not half-close its
write side. Cancellation cannot reverse already accepted provider work or tool
side effects; retain unknown-outcome/no-replay semantics.

**Passed earlier in this work (7 inference-chat tests):**

- `cancelled_queued_request_never_starts_model`
- `queued_and_established_requests_share_deadline`
- `streaming_cancellation_releases_permits_with_disabled_deadline`
- the existing concurrency, ordinary-tool completion, structured-output tests.

Still finish:

- A real Unix-socket disconnect test proving an in-progress dispatch future is
  dropped; combine it with the already passing permit-release tests.
- Server-timeout-before-client-grace coverage, error-code round trips, and
  streaming deadline coverage as needed by the original acceptance criteria.
- Review timer behavior when admission precedes receiver polling, and error
  classification at all consumers of the new variants.
- Update establishment-only and removed-handler comments in chat, IPC client,
  IPC constants, socket/settings/env documentation. Some are definitely stale.
- Preserve AIMD and avoid automatic retries of uncertain mutations.

GPUI test clock: `cx.executor().advance_clock(duration)`, then
`cx.run_until_parked()`. `TestAppContext` does not have `advance_clock()` or
`background_executor()` methods. Production timers use
`AsyncApp::background_executor().timer`, never Tokio timers on GPUI foreground.

### T15 — current observation windows

`hkask-regulation/src/runtime.rs`:

- Expired variety reads return zero current variety/deficit without another write.
- `OutcomeTracker::success_rate` returns `Option<f64>`; expired/empty windows
  return `None`, not stale results or a fabricated 100% success rate.
- Current operation count expires on read; aggregation ignores absent samples.
- `observations_expire_without_another_write` passes.

Review all read consumers and keep historical EMA semantics separate from the
current-window signal. Do not conflate an idle domain with a broken sensor.

### T16 — durable condition reconciliation

- Sensors now expose `observe()` for healthy as well as degraded readings;
  `None` means unavailable. Test-only `sense()` filters to deviations.
- `Deviation::from_signal` excludes healthy-side readings for the relevant
  floor/ceiling metrics and excludes non-finite values.
- `CyberneticsLoop::tick` snapshots observations and calls sink reconciliation.
- Alert JSON includes `recovery_signal` with original threshold/value/time,
  selected by the action's `metric_name`, including NoData-backed actions.
- `Signal::recovered_by` requires the same metric, a sufficiently new finite
  value, and crossing the **original** threshold toward health.
- The old `Accept && improved` auto-resolve branch was removed.
- `BridgeAlertEscalationSink` reads durable contexts, updates observations with
  compare-and-swap, and resolves only recovered conditions.
- Healthy in-memory log population and resolved model availability now emit
  readings; the old absence-expecting population test is the known failure above.

**Fresh handoff pass:**

```sh
cargo test --offline --locked -p kask_bridge --lib later_tick_reconciles_durable_conditions
```

This uses a real in-memory escalation queue through `tick()`: 2/10 unhealthy →
3/10 partial improvement remains pending → unavailable remains pending → rebuilt
loop + 10/10 healthy resolves; another tick does not change resolution time.

Finish review of all affected sensor/metric mappings, stale comments, and
missing/invalid recovery metadata. Unknown conditions must not auto-resolve.

### T17 — honest progress and weekly advice review

Implemented across regulation, storage, bridge, and curator:

- Stagnation resets on observed improvement, not mere acceptance.
- `ImpactReport` uses metric direction; unknown direction is not called improved.
- `LoopMetrics.effectiveness_score` renamed to `observed_progress_score`, measuring
  improved/verified rather than accepted/verified.
- `RegulationHealth.acceptance_rate()` replaces the misleading effectiveness
  method and returns `None` with no samples. Related snapshot/history fields use
  acceptance terminology. Metacognition no longer derives `Trusted` causal
  outcomes from acceptance; advisory causal trust remains unverified.
- `Signal::advice_review` returns `awaiting_action`, `observation_window`,
  `recovered`, `improved`, `no_improvement`, or `insufficient_evidence`.
  Assessment is due seven days after confirmed application and requires fresh,
  finite, matching baseline/current observations. The baseline/current freshness
  checks currently use 60 seconds; consolidate with the existing window constant
  rather than leaving duplicated unexplained numbers.
- Storage `EscalationQueue::{list_advice_observations, update_advice_context}`
  keeps applied advice observable after resolution and uses CAS so ticks do not
  overwrite concurrent operator acknowledgement.
- New MCP tools in `hkask-mcp-curator`:
  - `curator_advice_mark_applied`: requires `operator_confirmed` and an action
    note; records application/baseline/due date, without claiming effectiveness.
    Repeat acknowledgement does not reset the observation window.
  - `curator_advice_reviews`: exposes reviews, including resolved alerts.
- Bridge reconciliation writes latest observations and review state, with
  `causal_attribution: "unverified"`.
- Pure `weekly_advice_review_distinguishes_progress_from_acceptance` test exists
  and passed in the latest 60-test regulation run (the different population
  test failed).

Finish before claiming T17 complete:

- Test the **actual curator tools** against a real isolated escalation queue:
  missing confirmation/note refusal, absent/unmeasurable escalation, idempotent
  acknowledgement, review visibility after resolution.
- Test the seven-day transition through persisted contexts, not only the pure
  helper; test absent/stale baseline/current evidence and continued observation
  after early recovery.
- Test CAS conflicts and verify duplicate/superseding alert persistence does not
  erase application acknowledgement or the original trigger.
- Pin constant-degraded observations versus accepted noise in stagnation and
  progress metrics; do not convert observed improvement into causal success.
- Audit consumers, schemas, source comments, and docs for removed
  `effectiveness_score`, `regulation_effectiveness`, `cumulative_effectiveness`,
  `effectiveness()` names and old acceptance→trusted descriptions.

## T11 — still unimplemented; do not rotate live user DBs

Inspection confirmed the original ownership problem:

- `RealMemoryPort` retains `Arc<CuratorStore>` and consolidation handles.
- `CuratorStore` retains an immutable old passphrase and hands out
  `Arc<MemoryStore>` clones; its self-heal only repairs an absent store.
- Regulation archive and escalation queue adapters independently retain pools.
- MCP stop starts asynchronous cleanup; it is not a complete consumer drain.
- `identity::rotate_all_kask_db_passphrases` enumerates fixed paths and does not
  establish quiescence. Its comments also exclude caller-supplied corpus paths.
- `settings_ui/.../security.rs::spawn_db_passphrase_rotation` still runs rotation
  before stopping consumers, writes the key afterwards, and nudges restart.
  Its keychain-write failure handling and success UI marking need scrutiny.

Last recommendation to the operator: **maintenance restart** — settle work,
close the editor and its children, rotate/verify offline, publish the key, and
restart — instead of an in-process managed-store refactor across every consumer.
The operator then said “please proceed -- please compose a continuation prompt”.
Do not claim a maintenance-restart workflow has been implemented. General D5
approval is clear; recover/confirm the exact restart experience if needed before
adding a shutdown/relaunch path. Do not repeatedly re-ask already approved D1–D4.

Alternative: implement a genuinely coordinated in-process lifecycle with
revocable/managed store ownership. Merely dropping rotation-owned pools or
stopping MCP children does not satisfy the requirement.

Required design checkpoint and acceptance matrix:

1. Enumerate every shared-key DB and every opener/consumer; explicitly address
   external/caller-supplied corpus paths rather than silently omitting them.
2. Stop admission; settle/retain in-flight work; close all relevant handles.
3. Rotate and verify isolated copies, preserving RSS and KNN behavior already
   implemented in T09/T10.
4. Publish the key only after all DBs succeed. Define authority and recovery for
   rollback failure, keychain-write failure, and consumer-reopen failure.
5. Reopen/resume visibly, or remain in an explicit recoverable maintenance state.
6. Test every failure stage using temporary encrypted DBs and disposable
   credentials. Never claim multi-file crash atomicity from one atomic rename.

## Execution discipline and final closure

- Before editing, load `program-manager`, read an architecture doc, name its
  constraining invariant, and inspect current status/history. Respect local rules.
- Only one agent process edits the tree at a time. No user-keyring tests, live
  passphrase rotation, paid provider calls, speculative upstream cleanup, or
  restoring deleted compatibility APIs.
- The previous `spawn_agent` review attempt exceeded its endpoint context limit;
  no independent-review evidence exists from that attempt.
- Use file tools for scoped edits. `edit_file` rejects ambiguous text and can
  format on partial failure; reread the exact region once before retrying.
- Run bounded commands. If a command times out, state that; do not call it passed.
- Run current regression suites, relevant integration tests, `cargo check -p zed`,
  and scoped `./script/clippy`. Use temporary `HKASK_DATA_DIR` and
  `HKASK_ARTIFACTS_DIR` for tests that create markers/artifacts.
- Update `DIVERGENCE.md` for D8/D9/main wiring; update actual architecture/spec
  documents with decision provenance. Remove stale claims only against the
  operator's contract, not by laundering current code into a new spec.
- Update task boxes and checkpoint evidence in the original review only after
  the task's acceptance criteria have been met. Leave T11 explicitly incomplete
  until its full lifecycle and recovery contract are implemented and tested.
- Final response: functional outcomes, actual validation, exact remaining risks,
  and no implied commits or background work. The work is not complete merely
  because it was committed.
