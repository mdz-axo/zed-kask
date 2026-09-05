# Kask core review and improvement plan

- Date: 2026-09-04
- Status: **Implementation in progress — 13/20 tasks verified; D1/D2/D3/D5 approved; D4 weekly-review proposal awaits confirmation**
- Scope: all 18 crates under `kask/crates`, with deeper inspection of critical paths and selected production consumers outside that directory
- Initial baseline: `535e9b2b8b523a933af41248deaf0bdb02bf7b12`
- Validation baseline: `a3496b7164fc1a49e050a245e8ea12d6633d9c88`
- Provenance: source inspection, history/spec recovery, four specialist read-only reviews, coordinator counterchecks, existing unit tests, isolated probes, independent plan challenge
- Document type: review/proposal (`bibo:Document`); process: review → adjudicate → propose → operator decision → implementation → verification

All source paths below are relative to the repository root. Line citations describe the reviewed snapshot. HEAD advanced externally during inspection; `git diff 535e9b2b8b HEAD --stat -- kask/crates` was empty at the coordinator's verification. Existing uncommitted work was not changed by this review. No source changes or fixes were made. This new report does not replace `tasks/plan.md`, `tasks/todo.md`, or any canonical design specification.

## Continuation checkpoint — 2026-09-04

Baseline: `93e9a89799` (the preceding checkpoint was committed externally).
The following decisions supersede the earlier checkpoint's approval gates:

| Decision | Operator ruling / implementation status |
|---|---|
| Memory repair / compatibility | **Approved:** repair the forgetting-migration build break; no backward compatibility requirement. Remove obsolete expiry API/field usage, not restore shims. The existing one-time forward schema conversion remains; old lifecycle behavior is not supported. |
| D1 | **Approved:** parent-held delegation grants; child requests may narrow but cannot enlarge them. Not OS isolation against arbitrary same-UID code. T04 remains to implement. |
| D2 | **Approved and implemented:** reject mismatched embedding providers before sending; no silent destination substitution. The port accepts a provider-bound credential bundle. Unqualified/unknown prefixes also fail before HTTP. |
| D3 | **Approved:** admission-to-completion inference deadline, explicit overload/cancellation errors, and no replay of unknown-effect work. Preserve AIMD. T13/T14 remain to implement; T08's MCP startup deadline is separate. |
| D5 | **Approved:** visible temporary passphrase maintenance/recovery, settled writers and reopened consumers; all databases succeed before keychain publication. T11 remains to implement and is still a live-rotation safety gap. |
| D4 | **Proposal, not ratified:** weekly advice review, not weekly sensing. Reconcile recovery promptly from fresh observations; never interpret missing data as healthy. Start a seven-day assessment window after confirmed human action, distinguish improvement/recovery/no improvement/insufficient evidence, and do not infer causal efficacy from an accepted observation. User asked whether weekly was appropriate; no seven-day constant or timer has been added. |

### Newly verified

- **Memory repair:** `MemoryStore::dedup_by_normalized_value` uses `query_all`
  and `delete_by_id`, reports `deleted_count`, and tests the physical row count.
  Curator forgetting telemetry uses `turns_deleted`. Stale expiry descriptions
  were removed from the affected README, request types, and memory/storage docs.
- **T02/T20:** previously blocked MCP-server tests now pass, including mapped
  addresses and relative-path containment. The real corpus conversion fixture
  writes new basename and nested relative outputs successfully.
- **T05:** provider descriptor and key stay together from startup to dispatch.
  Removed the unbound constructor and now-unused permissive bridge prefix helper.
  Recording-transport tests cover every registered provider, mismatch/unknown/
  Unicode/empty-model rejection with zero HTTP, and same-provider case-insensitive
  overrides. IPC preserves the `InvalidRequest` classification. No network calls.
- **T07:** desired generation/cancellation gates publication against stop and
  replacement. A startup drop guard owns cleanup until publication, including
  repeated failed discovery; keeper tokens remain tracked while supervisors live.
  The command also enables kill-on-drop as a runtime-teardown fallback.
- **T08:** reconnect's potentially non-Send startup future is constructed and
  polled on the runtime's worker pool, not by blocking the caller. Handshake and
  discovery each use `HKASK_MCP_STARTUP_TIMEOUT_SECS` (default 60s, the existing
  health interval; zero means an immediate deadline). Real-process tests verify
  foreground progress, named phase timeout, replacement, and cleanup. rmcp's
  three-second graceful-close period is included in the cleanup test; the
  fixture stays paused for thirty seconds so self-exit cannot mask a leak.
- **T12:** ordinary tools use Auto; an empty tool list carries no tool choice;
  the sole `emit_result` structured-output protocol remains required. A GPUI
  fake-provider regression failed before the fix and now returns final text.
  A swarm executor fixture performs exactly one tool round and then answers,
  requiring two inference calls rather than exhausting the round cap.

### Verification for this continuation

All commands used offline/locked dependencies and bounded tool runtimes.

- `cargo test --offline --locked -p hkask-memory -p hkask-mcp-server --lib`
  — **30 memory + 19 MCP-server tests passed**; final repeat used temporary
  `HKASK_DATA_DIR` / `HKASK_ARTIFACTS_DIR` roots.
- `cargo test --offline --locked -p hkask-mcp-corpus --lib convert_writes_new_relative_outputs`
  — **1 passed**, with temporary data/artifact roots and subprocess CWD.
- `cargo test --offline --locked -p hkask-mcp-curator --lib forgetting`
  — **4 passed**, with temporary roots.
- `cargo test --offline --locked -p hkask-mcp --features test-fixture -- --test-threads=1`
  — **17 unit + 13 integration tests passed**. The two initial discovery
  regressions failed before the lifecycle change; the deadline cleanup fixture
  was corrected to respect rmcp's documented grace period, not weakened to
  accept an unowned process.
- `cargo test --offline --locked -p kask_bridge --lib inference_`
  — **32 passed**, including recording-HTTP and actual IPC dispatch tests.
- `cargo test --offline --locked -p hkask-mcp-swarm --lib tool_enabled_delegate_returns_answer_before_round_cap`
  — **1 passed**, with temporary roots.
- `cargo check --offline --locked -p kask_bridge --tests` and
  `cargo check --offline --locked -p zed` — **passed**. This verifies the
  changed D8 composition-root call site; it is not a full application test run.
- `./script/clippy --offline --locked -p kask_bridge -p hkask-inference -p hkask-memory -p hkask-mcp -p hkask-mcp-server -p hkask-mcp-curator -p hkask-mcp-swarm`
  — **passed**, including Kask cargo-machete, typos, and buf checks.

### Remaining work and ownership

**T04, T11, T13, T14** are approved, unimplemented work owned by the coding
agent for subsequent checkpoints. Parent authority, live passphrase maintenance,
and inference queue/cancellation/deadline guarantees must not be advertised as
fixed. T11 still requires cross-consumer lifecycle design and failure injection;
MCP stop currently initiates asynchronous shutdown, not a complete maintenance
quiescence barrier.

**T15–T17** remain pending the D4 semantic decision (operator), then implementation
and trace-based tests (coding agent). A weekly cadence must not turn the current
one-minute sensor observations into week-old "current" samples.

An external action again staged intermediate edits during this continuation.
The agent did not stage or commit; validate/reconcile the full index before
committing. Latest timeout cleanup tests and documentation may be unstaged.

**Learning:** process cleanup has its own measured lifetime (including rmcp's
grace period). Deadline tests must distinguish that lifetime from both request
completion and the fixture's independent exit timer.

## Implementation checkpoint — 2026-09-04 (preceding snapshot)

The operator authorized proceeding with implementation in this session. That
supersedes the proposal-only scheduling status, not the explicit D1–D5 experience
gates. The findings and original review evidence below remain historical; they
have not been rewritten to make the implementation its own specification.

### Delivered and verified

| Tasks | Outcome | Evidence / scope |
|---|---|---|
| T01 | Credential unit tests cannot use the ambient keyring. | Already implemented in `c4c5c2b123`; 10 keystore tests passed, including disposable backend and cross-keyring isolation. No production credential mutation. |
| T03 | Hostile Lisp input is checked in subprocesses; ordinary recursion remains functional. | Already implemented in `c4c5c2b123`; 17 tests passed. This checkpoint aligned the README with the implemented limits and scoped a Clippy exception to the bounded synchronous subprocess test only. |
| T06 | Ledger writes retain one connection/RAII transaction, including reference and balance checks. | Added concurrent funding/idempotency/debit, nonnegative-balance, and forced-posting rollback tests. The concurrent test failed before the fix with a UNIQUE reference error; all 3 tests now pass. No ledger integration-test directory exists in the current tree. |
| T09 | Populated RSS tables, contentless FTS search, triggers, and AUTOINCREMENT state survive rotation. | Actual research-server DDL is read into a temporary SQLCipher fixture. Regression failed before the fix on `main.feeds`; now passes, including post-reopen insert/delete search updates. |
| T10 | Old and new vectors remain searchable after reopening, with preserved rowids and h_mem JOINs. | Regression initially returned no old neighbor; now passes. Malformed vector/dimension/NaN and foreign-key fixtures fail before replacement and retain old-key access. |
| T18 | Startup uses configured outcome thresholds and rejects invalid probability bounds. | Constructor test covers parsed YAML 0.95/0.90 plus defaults; validation rejects non-finite/out-of-range and reversed/equal thresholds. Both regressions failed before correction; all 58 regulation tests pass. |
| T19 | Commented shared settings preserve model overrides without changing environment precedence. | Uses the host's existing `serde_json_lenient` dependency. Tests cover comments/trailing commas, defaults, environment precedence, malformed syntax/types and warning emission. JSONC regression failed before correction; all 5 services-core tests pass. |

Checked task boxes below mean implemented and verified at this checkpoint, not
operator-confirmed acceptance of the experience. T01/T03 credit belongs to the
preceding commit; it is not newly implemented functionality from this session.

### Implemented but validation blocked

- **T02:** mapped-address normalization already exists in `c4c5c2b123`; full
  MCP-server test verification remains blocked below.
- **T20:** relative inputs are anchored to the already-resolved server CWD.
  Added subprocess tests for basename/nested/absolute equivalence, existing-file
  behavior, traversal and symlink denial, plus a real `corpus_convert` fixture.
  These tests have **not run**, because their dependency graph fails first.

**Build blocker (owner: operator decision, then coding agent):** the unchanged
`hkask-memory/src/memory_store.rs` still calls removed `query_all_live` and
`close_by_id`, and constructs removed `DedupOutcome::expired_count`, after the
prior forgetting migration. Baseline and targeted MCP/corpus builds fail there.
Repairing that separate migration requires scope approval; it was not silently
folded into these review fixes. Until then T02/T20 are not verified and the
MCP/corpus applications are not certified buildable.

### Design record and remaining ownership

- **Provenance:** this report's F08/F07/F06/F19/F20/F21 and task acceptance
  criteria; authorization to proceed in the current session.
- **Pattern:** use existing owners, not a new service framework. Ledger follows
  storage's connection-scoped transaction idiom. Rotation uses SQLCipher's
  schema export, reopens the attachment to load virtual declarations, then
  rebuilds KNN and validates foreign keys/integrity before replacement.
- **Preserved invariants:** user settings/default precedence; fail visibly on
  invalid data; no live-key/database tests; all-DB-before-keychain publication
  remains required; no autonomous regulation or AIMD changes; no upstream edits.
- **Refused shortcuts:** ledger-local mutexes that miss other pool users;
  alphabetical-table reorder as a substitute for schema preservation; claiming
  copied embedding metadata proves search; stripping comments manually;
  relaxing containment to admit relative paths.
- **Boundary:** the affected Kask ledger/storage/regulation/services-core and
  MCP-server files, corpus-handler regression, crate manifests/lockfile, adjacent
  README corrections, and this report. No settings orchestration, IPC authority,
  inference lifetime, or upstream Zed code was changed.
- **T07/T08/T12 (owner: coding agent, next checkpoint):** not started. Runtime
  lifecycle/reconnect and ordinary-tool completion still require implementation
  and their acceptance tests; not being abandoned or marked complete.
- **T04/T05/T11/T13–T17 (owner: operator for D1–D5, then coding agent):** blocked
  on the experience choices in the original decision table. No horizon, overload
  policy, trust model, or live-key recovery policy has been inferred as ratified.
- **T11 safety warning remains:** passing isolated storage rotation tests does
  not make rotation safe while live database consumers remain open.

### Validation actually run for this checkpoint

- `cargo test --offline --locked -p hkask-keystore -p hkask-lisp -p hkask-ledger -p hkask-storage -p hkask-regulation -p hkask-services-core`
  — **129 tests passed** (10 + 17 + 3 + 36 + 58 + 5); three pre-existing storage
  doctest examples ignored. No ignored keyring tests were invoked.
- `./script/clippy --offline --locked -p hkask-keystore -p hkask-lisp -p hkask-ledger -p hkask-storage -p hkask-regulation -p hkask-services-core`
  — **passed**, including Kask-scoped cargo-machete, typos, and buf checks.
  Initial lint runs rejected synchronous subprocess calls in test code;
  narrowly justified test-only exceptions fixed those, without relaxing
  production foreground restrictions.
- `cargo test --offline -p hkask-mcp-server -p hkask-mcp-corpus --lib relative`
  — **blocked** by the existing memory migration errors described above.
- `git diff HEAD --check` — **passed**. Only coordinator self-review was
  available: the attempted independent read-only review failed before execution
  because the delegation endpoint exceeded its context limit.

**Working-tree handoff:** an external action staged much of the in-flight patch
while this session was running; the agent did not stage or commit anything.
Later rotation fixes/tests and documentation are unstaged. Verification applies
to the complete working tree, not the partial index. Reconcile the index with
this checkpoint before committing; do not commit its earlier intermediate state.

**Learning:** storage preservation must be checked through the operations users
need after reopening—RSS search/triggers and h_mem recall—not metadata counts.

### Original final working-tree caveat (review-time snapshot)

After the reported test/probe runs, a final status check found concurrent modifications to `crates/agent/src/tools/lisp_eval_tool.rs`, `kask/crates/hkask-lisp/src/hkask_lisp.rs`, `kask/crates/hkask-storage/src/core/connection.rs`, `kask/crates/hkask-storage/src/core/sql/schema.sql`, `kask/crates/hkask-storage/src/hmem.rs`, and `kask/crates/hkask-types/src/hkask_types.rs`. These were not made by this review and were left untouched. The 393 passing tests do not certify that later working tree. Revalidate affected paths before implementation; line numbers may have shifted.

The inspected concurrent Lisp diff raises default evaluation depth from 64 to 1024 but does not bound recursive parsing. It therefore does not remedy F03, whose probe supplied an explicit depth budget of 1. The remaining concurrent changes were not re-reviewed as part of this report. This document is evidence against the reviewed snapshots, not a claim that a changing worktree has been frozen or fully revalidated.

## Executive verdict

**Improvements are needed before relying on the affected safety, persistence, and recovery guarantees.** This is a current-state audit, not a PR approval decision: no diff baseline was supplied, so no artificial PR diff was invented.

The strongest pattern is **contracts that do not compose across boundaries**:

- a request's tool list is checked, but the request supplies that list;
- an evaluation budget exists, but recursive parsing precedes it;
- database rows survive rotation tests, but derived search state and live handle ownership are not preserved by those tests;
- a semaphore bounds active inference, but not queued detached tasks;
- stream establishment has a timeout, but queueing and stream drain do not share it;
- an unchanged observation is acceptable, but that acceptance is also reported as effectiveness.

**21 findings are retained.** Three defects were reproduced by the coordinator in isolated probes: mapped-IP URL admission, relative-output-path rejection, and Lisp process abort. A specialist reproduced the RSS copy-order mechanism in in-memory SQLite, not the full SQLCipher rotation path. The other findings remain static observations/inferences with concrete production call paths and proposed falsification tests.

**393 existing library tests passed across 16 crates.** This is useful baseline evidence, not evidence that the new findings are false. `kask_bridge` and `hkask-keystore` tests were not run. Two selected library targets contained zero unit tests; that does not mean their integration tests do not exist.

### Immediate operator cautions

1. **Do not run `cargo test -p hkask-keystore -- --ignored` against your normal keyring.** One test deletes and replaces the production database passphrase, then deletes it without restoration (F09).
2. **Do not exercise passphrase rotation on the only copy of valuable databases to validate this review.** Use isolated copies and a disposable keyring; the rotation findings include preservation and handle-lifecycle risks (F05–F07).
3. Treat the present IPC allowlist as request consistency checking, not demonstrated confinement of a compromised child (F01).
4. Do not send a local-provider embedding override through a cloud-bound embedding bridge until routing is verified (F04).

These cautions are recommendations, not actions taken on the user's installation.

## Method and evidence discipline

### Requested skill application

| Skill | How it informed this review |
|---|---|
| Code review | Recover contracts; inspect callers; separate detection from adjudication; cite source; propose named remedies and falsifiers. |
| Metacognition | Track static versus reproduced claims; challenge cross-component assumptions; measure test/probe coverage instead of claiming exhaustive understanding. |
| Diagnose | Anchor each symptom to an executable path; prefer a minimal deterministic probe; leave untested race/rotation hypotheses explicitly unconfirmed. No instrumentation or fix phase was entered. |
| Bug hunt | Explore timing, data, interface, integration, and configuration threats rather than only style patterns. |
| Hypothesis framer | For each issue: current behavior versus contract, alternative explanation, discriminating test, observable outcome. |
| Grill-me | Ask whether the behavior is intentional, caller-constrained, provider-dependent, unreachable, or already superseded by a PM decision. |
| Pragmatic semantics | Keep IS, OUGHT, proposal, and hypothesis separate. Do not promote a comment or a test name into proof. |
| Pragmatic cybernetics | Trace observation → interpretation → recommendation → human action → later observation; inspect freshness, closure, and outcome fidelity. |
| Essentialist | Prefer fewer independently managed states at existing owners; reject blanket module splitting, visibility churn, and deletion of intentionally designed capabilities. |

Supplementary task-breakdown, deep-module, and coding-guidelines skills informed the plan. An independent reviewer challenged plan sizing, recovery contracts, dependencies, and hidden user-experience choices.

### Epistemic conventions

- **Observed:** source or test/probe output directly inspected.
- **Static inference:** a reachable consequence inferred from code; not a reproduced production incident.
- **Reproduced boundary defect:** isolated execution demonstrated the stated boundary failure, not every downstream consequence.
- **OUGHT:** recovered requirement or explicitly identified proposed behavior.
- Confidence values below are **subjective review estimates**, not historical calibration or production incidence. The calibration tool reported no stored forecasts; no Brier accuracy claim is made.
- Each finding's alternative/falsifier expresses H0: the existing behavior is intentional or protected elsewhere. H1 is its stated defect. Proposed acceptance tests must fail on the defect and distinguish that alternative.
- Priorities: **P1** = address before depending on the affected guarantee; **P2** = scheduled correctness/feedback repair. Priority is separate from reproducibility. “Blocker” is reserved for a cited safety prohibition; conditional/assessment findings remain should-fix even when high priority.

### Contracts deliberately preserved

- `kask/docs/architecture/core/magna-carta.md:54–87` separates live enforcement from OUGHT-only sovereignty constructs. Their absence is not reported as a new regression.
- The unseeded call meter is deliberately fail-open and is not an authorization gate.
- The September 3 ratified AIMD policy supersedes stepped-ramp concurrency. This plan does not restore the old policy.
- Regulation is advisory with a human actuator; missing autonomous throttling is not a defect to fix by adding autonomy.
- Code model defaults were deliberately restored by `55a366a30c`. Valid overrides must work; the defaults themselves are not a defect.
- Preserve bridge dependency direction and upstream D-seams. Implementation should remain in `kask/` where possible; unavoidable upstream changes need their seam/test treatment.

## Findings

### F01 — IPC permission membership is checked against a caller-supplied list

**P1 · Blocker against the documented independent-authority claim · integration/security · confidence 0.99 · static**

**Evidence:** `kask/crates/kask_bridge/src/inference_ipc_server.rs:802–804`:

```rust
match &params.tool_allowlist {
    Some(allowlist) if !allowlist.is_empty() => {
        if !allowlist.iter().any(|a| a == &qualified) {
```

**IS:** The child sends both the requested `server/tool` and its permission list. `hkask-inference/src/inference_ipc_client.rs:680–695` serializes `allowed` directly. The server checks membership, then invokes using the fixed `kask-panel` accounting identity (`inference_ipc_server.rs:827–829`). Peer checking establishes Unix UID, not an independent per-child tool grant. The downstream runtime meters calls rather than supplying that missing authority.

**Reachability/impact:** Built-in servers receive the IPC socket; a child capable of crafting IPC can name a registered tool and include it in its own list. This is **not** evidence of cross-UID access or a model bypassing the trusted swarm executor's card check.

**OUGHT:** Magna Carta `:63–69` describes authority boundaries whose checked caller cannot choose the list. History `1b3dc00bb6` introduced the request list; `05911f0c1c` removed the vacuous token check.

**Alternative/falsifier:** The swarm executor does constrain model calls using an agent card. That protects that trusted execution path, not the child-process boundary claimed here. An authenticated, parent-held grant constraining this dispatch would falsify the broader finding. If all same-UID children are intentionally fully trusted, the operator should ratify a narrower claim instead.

**Remedy:** Parent-owned delegation context; a request can narrow, never enlarge, its grant. Do not reintroduce self-issued credentials. Do not advertise this as OS isolation from a malicious same-UID process.

**Acceptance:** `ipc_child_cannot_expand_parent_grant`; `ipc_inference_only_child_cannot_invoke_tools`. Exercise real dispatch with a recording tool port; assert forbidden calls never reach it.

### F02 — IPv4-mapped IPv6 bypasses strict private-address checks

**P1 · Blocker, explicit URL safety prohibition · interface/security · confidence 0.99 · reproduced admission defect**

**Evidence:** `kask/crates/hkask-mcp-server/src/security.rs:213–219`:

```rust
let is_ula = (segments[0] & 0xfe00) == 0xfc00;
let is_link_local = (segments[0] & 0xffc0) == 0xfe80;
is_ula || is_link_local
```

**IS:** Mapped IPv4 addresses take the IPv6 branch, escape these checks and native IPv6 loopback detection, then return early as already-validated literals (`:166–167`).

**Reproduction:** Actual public `validate_tool_url_with_dns` returned `Ok(())` for mapped `127.0.0.1`, `10.0.0.1`, and `169.254.169.254`. Native `127.0.0.1` and `::1` were rejected. No DNS or HTTP requests were made.

**Reachability/impact:** Research RSS discovery calls strict validation, then fetches the URL (`hkask-mcp-research/src/hkask_mcp_research.rs:943`, `research/feed.rs:68–75`). Actual private service access depends on connector and network reachability; the admission bypass itself was demonstrated.

**OUGHT:** Strict validation promises to reject loopback/private destinations (`security.rs:104–110`).

**Alternative/falsifier:** A downstream connector may block the eventual connection, but that does not make this validator reject the address. This is not the documented DNS-rebinding limitation; no DNS race is required.

**Remedy:** Canonical IP classification: normalize IPv4-mapped IPv6 before both loopback and private checks. Preserve intentional permissive mode.

**Acceptance:** Reject mapped loopback/RFC1918/link-local at the strict entry point; permit mapped public IPv4; assert denied input causes zero transport calls.

### F03 — Lisp parsing can abort the process before its budget is enforced

**P1 · Blocker, sandbox safety prohibition · coding/resource safety · confidence 0.99 · reproduced isolated process abort**

**Evidence:** `kask/crates/hkask-lisp/src/hkask_lisp.rs:1399,1410` and `:360`:

```rust
let parsed = parse(form)?;
// ...
let mut budget = EvalBudget::new(max_steps, max_depth);
```

```rust
let (form, next) = parse_form(remaining)?;
```

**IS:** Recursive parsing happens before the evaluation budget exists. JSON conversion and result traversal also sit outside that evaluator budget; those additional resource boundaries require their own tests rather than being assumed safe.

**Reproduction:** A disposable Rust harness compiled the actual interpreter source. A quoted list nested 128 levels returned successfully with `max_depth=1`. At 20,000 nesting levels the subprocess exited via SIGABRT (`-6`), reporting stack overflow. Core dumps were disabled; no editor process was probed.

**Reachability/impact:** `crates/agent/src/tools/lisp_eval_tool.rs:124–137` passes supplied form/budgets directly to this function inside a foreground task. The form has no parse-nesting cap at that entry point. Stack overflow in this in-process path can terminate the editor, not return a normal tool error.

**OUGHT:** The crate README promises bounded execution preventing stack overflow. Evaluation-only depth checks do not establish that guarantee.

**Alternative/falsifier:** `max_depth` may intentionally mean evaluation depth, not quoted data depth. That semantic distinction is valid, but then parsing needs a separate hard structural safety limit. A wrapper enforcing that limit before recursive parsing would falsify the exposed production path; none was found.

**Remedy:** Bound the entire interpreter boundary, not just recursive evaluation. Establish input/nesting limits before parsing, constrain conversion/output work, and account for large single builtins and recursive drop paths. Retain the small no-I/O interpreter; do not add a general execution framework.

**Acceptance:** Subprocess test `deep_source_returns_limit_error_without_abort`, including quotes and malformed deep input; separate environment/output/builtin resource tests. A `Result::Err` is acceptable; process abort is not. Keep legitimate recursive predicates working within the documented limits.

### F04 — Embedding overrides can transmit text to a different provider

**P1 · High should-fix, data-destination guardrail · integration · confidence 0.99 · static**

**Evidence:** `kask/crates/kask_bridge/src/inference_embedding.rs:71–78,95`:

```rust
let api_url = api_url.clone();
let api_key = api_key.clone();
// ...
let model_id = crate::inference_providers::strip_provider_prefix(&req.model);
```

**IS:** Endpoint and credentials are bound at construction; each request's provider prefix is stripped without changing or validating that binding. The request is sent to the captured endpoint.

**Reachability/impact:** Startup constructs one shared embedding port (`crates/zed/src/main.rs:1682–1719`). `corpus_embed` accepts and forwards a model override (`hkask-mcp-corpus/src/tools/semantic.rs:290,346`); IPC forwards it to the port. A local-provider-prefixed request through a cloud-bound port can therefore send its text to the cloud before any model-not-found response.

**OUGHT:** Respect a provider-qualified request or reject it before transmitting its contents elsewhere.

**Alternative/falsifier:** Same-provider overrides can work correctly. A documented single-provider interface with a pre-send mismatch rejection would be safe; prefix removal without that check is not. The direct fallback resolves from the requested model, so availability of the bridge changes routing behavior.

**Remedy:** Bind dispatch to a validated provider/model pair. Reject mismatches before HTTP as an immediate guard; implement per-request routing if cross-provider overrides are required. Operator decision D2 covers the intermediate experience.

**Acceptance:** `embedding_provider_mismatch_sends_no_http`; `ipc_embedding_override_uses_requested_provider`, using recording clients and no paid calls.

### F05 — Settings rotation does not quiesce existing database consumers

**P1 · High should-fix, data-preservation guardrail · timing/integration · confidence 0.96 · static**

**Evidence:** `kask/crates/hkask-storage/src/rotation.rs:217–220`:

```rust
 drop(source_pool);
 drop(new_pool);
 drop(source_db);
 drop(new_db);
```

**IS:** These drops close rotation's handles, not all existing consumers. Settings initiates rotation while consumers can retain pools, then nudges MCP servers afterward (`crates/settings_ui/src/pages/kask_page/security.rs:150–180`). The copy has a destination transaction, not a coordinated source snapshot; file replacement and WAL/SHM removal follow. The in-process curator retains its original pool/passphrase and only heals an absent store (`kask_bridge/src/memory/curator_stores.rs:52–57,103–133`).

**Impact:** Concurrent writes can miss the replacement; surviving handles can reference old storage; curator reopening is not established. This was not tested against live databases.

**OUGHT:** Rotation documents that every other database user must close first (`rotation.rs:38–41`). Restarting afterward cannot satisfy a before-copy precondition. Shared rotation must preserve all databases before publishing a new key.

**Alternative/falsifier:** A quiet installation may appear successful. A lifecycle gate that drains writers, closes every pool, and reopens all consumers with the resulting key would falsify the finding; none appears in the traced settings path.

**Remedy:** Coordinated rotation ownership: stop admission, settle in-flight work, close pools, rotate/verify, publish the key only after all DBs succeed, reopen all consumers, resume. Specify recovery if reopening fails; do not claim multi-file crash atomicity merely because one rename is atomic.

**Acceptance:** `settings_rotation_preserves_inflight_writes_and_reopens_curator`; failure injection across each database and reopening stage; old/new key authority and operator-visible recovery state are explicit. Isolated SQLCipher fixtures only.

### F06 — Rotation omits the KNN index without implementing its promised rebuild

**P1 · High should-fix, preservation guardrail · data · confidence 0.99 · static**

**Evidence:** `kask/crates/hkask-storage/src/rotation.rs:330–334`:

```sql
WHERE type = 'table'
AND name NOT LIKE 'sqlite_%'
AND name NOT LIKE 'vec_%'
AND name NOT LIKE 'vec0%'
ORDER BY name
```

**IS:** `vec_embeddings` is excluded; the destination receives an empty virtual table. The normal embedding store only inserts each newly written vector (`hkask-storage/src/embeddings.rs:197–209`), while search starts from `vec_embeddings` (`:296–301`). No rebuild of older vectors was found.

**Impact:** Relational metadata can survive while old memories silently disappear from KNN recall. Corpus's independent in-memory hydration path does not repair sqlite-vec recall.

**OUGHT:** `rotation.rs:42–48` explicitly promises reindexing on next use/write. Existing rotation tests count copied metadata but do not verify this function.

**Alternative/falsifier:** A production reindex on reopen or a rotation/new-write/old-vector search round trip would falsify the claim. Finding the metadata rows alone does not.

**Remedy:** Rebuild the destination KNN index from canonical embedding rows before rotation reports successful preservation, maintaining row identity and dimension invariants.

**Acceptance:** `rotate_passphrase_preserves_knn_recall_after_reopen_and_new_write`; compare pre/post nearest neighbors and exercise the h_mem JOIN, not just vector-table counts.

### F07 — Alphabetical create-and-copy fails on populated RSS foreign keys

**P1 · High should-fix, rotation functionality guardrail · data/integration · confidence 0.99 · SQL mechanism reproduced by specialist**

**Evidence:** `kask/crates/hkask-storage/src/rotation.rs:334,374,401–405`:

```rust
for table_name in &table_names {
    // ...
    new_conn
        .execute_batch(&create_sql)
```

**IS:** Tables are created and filled one at a time in alphabetical order, with foreign keys enabled. RSS `entries.feed_id` references `feeds`, but `entries` is copied before `feeds`; the destination's generic schema does not precreate the RSS schema.

**Reachability/impact:** Shared rotation includes `research_rss` (`kask_bridge/src/identity.rs:312–315`). An ordinary populated RSS database can prevent completion and invoke rollback of already-rotated databases.

**Reproduction limit:** The specialist used the actual RSS DDL and copy ordering with two in-memory SQLite 3.46.1 databases; copying `entries` failed with `no such table: main.feeds`. This is not an execution of Rust/SQLCipher rotation.

**OUGHT:** A valid feed plus entry must survive the advertised all-database passphrase change.

**Alternative/falsifier:** Empty RSS data may not trigger the insert failure. Successful real rotation of a populated feed/entry/search fixture would falsify the finding.

**Remedy:** Schema-aware copying: create ordinary schemas before loading data, manage/defer and validate foreign keys, and handle FTS virtual/shadow tables explicitly. Merely changing table sort order is not a complete remedy.

**Acceptance:** `rotate_passphrase_preserves_rss_feed_entries_and_search`, plus foreign-key validation and failure rollback against the real schema in temporary encrypted DBs.

### F08 — Ledger transactions do not retain their database connection

**P1 · High should-fix, atomicity guardrail · timing/data · confidence 0.97 · static**

**Evidence:** `kask/crates/hkask-ledger/src/hkask_ledger.rs:120–124`:

```rust
self.driver.execute_batch("BEGIN IMMEDIATE")?;
let result: Result<(), LedgerError> = (|| {
    self.driver.execute(
```

**IS:** BEGIN, queries, inserts, COMMIT, and ROLLBACK independently acquire/release pooled connections (`hkask-storage/src/database/sqlite.rs:241,257`). No transaction-scoped reservation connects those operations. `debit_if_funds` repeats the pattern (`hkask_ledger.rs:211–265`).

**Reachability/impact:** Local swarm funding/spend uses this ledger with a four-connection production pool (`hkask-mcp-swarm/src/local_runtime.rs:191–199`). Interleaving can separate statements from the connection holding the transaction, causing lock errors or crossing transaction ownership. The nonnegative debit and idempotency contracts are not guaranteed by the current structure.

**OUGHT:** `commit` promises persistent transaction/posting updates; `debit_if_funds` explicitly promises one transaction around balance check and debit.

**Alternative/falsifier:** Sequential reuse of a LIFO pool may appear correct, and one fan-out spend path serializes calls. Neither proves affinity across all other consumers. An operation-wide connection reservation or complete shared serialization would falsify this finding.

**Remedy:** One connection and RAII transaction for each complete ledger operation, including idempotency/balance checks. Use the existing storage transaction idiom rather than layering a second ledger service over it.

**Acceptance:** `ledger_commit_isolated_from_concurrent_pool_users`; concurrent debit-versus-funding and forced rollback tests with a file-backed multi-connection pool and barriers. Assert no partial postings, negative balance, or rollback of another operation. Existing integration tests must also run; zero library unit tests is not evidence they are absent.

### F09 — An ignored keystore test destroys the production passphrase entry

**P1 · Blocker for safe execution of the advertised ignored test suite · data/test safety · confidence 0.99 · static; intentionally not executed**

**Evidence:** `kask/crates/hkask-keystore/src/keychain.rs:483–490,503–505`:

```rust
let _ = Keychain.delete_by_key(KEY_DB_PASSPHRASE);
// ...
kc.store_by_key(KEY_DB_PASSPHRASE, TEST_VALUE)
```

**IS:** `resolve_finds_entry_written_by_store_by_key` deletes the real entry, writes `TEST_VALUE`, then deletes it again. It never preserves/restores the original, although the section advertises sentinel-key isolation (`:432–434`).

**Impact:** Explicitly running the ignored suite can discard the only stored key for existing encrypted databases. Default unit-test execution does not run this test. Reprovisioning the default later does not rotate old data.

**OUGHT:** Test validation must not destroy operator credentials. This is a maintenance hazard, not a claim that normal app startup executes the test.

**Alternative/falsifier:** A disposable Secret Service/keyring makes it safe, but the test does not require one. Ordinary save/restore is not robust against panic or process termination.

**Remedy:** Require isolated keyring state or an injected backend for resolver/provisioning tests. Sentinel keys alone cannot safely test a resolver hardcoded to the production key without namespace/backend isolation.

**Acceptance:** `resolver_round_trip_leaves_existing_operator_passphrase_unchanged`; explicitly invoked ignored tests must refuse unsafe ambient setup or demonstrably use a disposable backend, including failure paths.

### F10 — A pending start can resurrect a deliberately unloaded MCP server

**P1 · High should-fix, user-control/lifecycle guardrail · timing · confidence 0.98 · static interleaving**

**Evidence:** `kask/crates/hkask-mcp/src/runtime.rs:826,845–848`:

```rust
connections.insert(server_id.to_string(), Connection { peer, generation });
// ...
if let Some(previous) = tokens.insert(server_id.to_string(), cancel.clone()) {
    previous.cancel();
}
```

**IS:** A start can wait on handshake/discovery while `stop_server` removes its existing runtime state. The old start can then publish a new connection, fresh cancellation token, tools, and supervisor. Publication checks for another live connection, not whether this start is still desired. Its token is independently created (`:734`).

**Reachability/impact:** Settings unload races supervisor/on-demand restart. The server and tools can return after the user unloads them, contradicting the no-resurrection requirement in `crates/zed/src/main.rs:3393–3411` and history `45e9ac92b7`.

**Alternative/falsifier:** Existing generation checking prevents a stale keeper from deleting a replacement; it does not invalidate a start on unload. A stop-during-discovery test proving refusal of late publication would falsify this claim.

**Remedy:** Generation-checked desired lifecycle at the existing runtime owner. Stop/unload invalidates pending starts; stale completions close their services rather than publish.

**Acceptance:** `unload_during_discovery_stays_unloaded`, exercised for supervisor and invocation-driven restart; old configuration cannot overwrite a replacement's environment; no residual child or tool registration.

### F11 — Discovery failure can orphan a child before runtime ownership is recorded

**P1 · High should-fix, cleanup guideline · timing/lifecycle · confidence 0.99 · static**

**Evidence:** `kask/crates/hkask-mcp/src/runtime.rs:749–751,789–794`:

```rust
tokio::spawn(async move {
    let reaped = tokio::select! {
        quit = running.waiting() => {
```

**IS:** The detached keeper owns the service before tool discovery. A `tools/list` error returns before the token is installed in the runtime map. The keeper retains a token clone; dropping the local token does not cancel it.

**Reachability/impact:** A configured server can initialize, reject discovery, and keep stdio open. Startup reports failure, but `stop_server`/`shutdown_all` cannot find that child's token. Repeated failed starts can accumulate children.

**Alternative/falsifier:** rmcp's drop cleanup exists, but the detached keeper still owns the running service. A discovery-error fixture that stays alive unless explicitly cleaned up distinguishes this from normal transport death.

**Remedy:** Transactional startup ownership: retain cleanup responsibility until discovery and lifecycle publication succeed; transfer to the keeper only at commit. Address alongside F10 at the same owner, not via a second supervisor.

**Acceptance:** `discovery_failure_reaps_child`; cancellation mid-discovery and repeated failures leave no additional processes; concurrent-start losers also clean up.

### F12 — Runtime reconnect synchronously blocks a foreground caller

**P1 · High should-fix, responsiveness guardrail · timing · confidence 0.98 · static**

**Evidence:** `kask/crates/hkask-mcp/src/runtime.rs:1234`:

```rust
handle.block_on(self.start_server_with_env(server_id, &spec.command, spec.env))
```

**IS:** On a non-Tokio executor, reconnect parks the caller during startup. The agent's tool path runs in foreground `cx.spawn` (`crates/agent/src/tools/context_server_registry.rs:532–542`) and reaches this runtime via `ZedKaskToolSource` (`crates/zed/src/main.rs:3331–3337`). A slow or indefinitely stalled handshake/discovery can freeze that foreground.

**OUGHT:** Supplying a Tokio reactor must not synchronously occupy the editor foreground. Retry count is not a deadline for an attempt that never returns.

**Alternative/falsifier:** Startup may not require the blocked GPUI executor, avoiding a cyclic deadlock; that does not avoid UI freezing. The off-runtime integration test uses a plain thread and establishes eventual success, not foreground responsiveness. A foreground-progress test with paused startup would discriminate.

**Remedy:** Runtime-owned async reconnect with bounded/cancelable startup and discovery. History `f4bedcb32b` deliberately removed caller-specific Tokio wrappers; preserve that simplification rather than restoring wrappers at every caller.

**Acceptance:** `foreground_progresses_during_reconnect`; startup deadline reports server and phase; preserve `Unavailable` (not delivered) versus `Interrupted` (outcome unknown), with no automatic replay of delivered mutations.

### F13 — Ordinary tool-enabled agents are forced to call a tool instead of finish

**P1 · High should-fix, completion guardrail · integration · confidence 0.98 · static/provider-dependent**

**Evidence:** `kask/crates/kask_bridge/src/inference_chat.rs:745–749`:

```rust
tool_choice: if tools.is_some() {
    Some(LanguageModelToolChoice::Any)
} else {
    None
},
```

**IS:** Offering any tools forces a call. OpenRouter maps `Any` to `Required`. The swarm executor offers its tools on every round and terminates normally only when `tool_calls.is_empty()` (`hkask-mcp-swarm/src/agent_executor.rs:321–324,354–356`). After four rounds it can return the initially empty final text.

**Impact:** On a provider honoring required tool choice, a tool-enabled delegation can repeat effects/exhaust rounds without a final answer.

**OUGHT:** Ordinary agent tools are optional; explicitly structured output may require its result tool. History `f4f94a9ffe` justifies `emit_result`, not every open-ended tool loop.

**Alternative/falsifier:** A provider ignoring required choice can mask the issue. A terminal-result tool or a final tool-free inference round could avoid it, but neither exists in the traced path.

**Remedy:** Explicitly distinguish structured-output enforcement from ordinary tool availability; do not infer “required result” from `Some(tools)`.

**Acceptance:** `ordinary_tools_allow_final_answer`; `structured_emit_result_remains_required`; `tool_enabled_delegate_returns_answer_before_round_cap`, using a fake model honoring tool choice.

### F14 — Cancellation does not own queued/in-flight inference work

**P1 · High should-fix, resource-control guideline · timing · confidence 0.98 · static**

**Evidence:** `kask/crates/kask_bridge/src/inference_chat.rs:273–275,314–328`:

```rust
let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<InferenceRequest>();
```

```rust
Self::handle_non_streaming(req, &model, timeout, cx, &recent_timeouts).await;
// ...
}).detach();
```

**IS:** Unbounded channels feed detached tasks; each task waits for a semaphore. Non-streaming work notices the lost reply only after collecting the result (`:505–507`). The IPC handler awaits dispatch before reading/writing again, so socket closure is not a concurrent cancellation source.

**Impact:** Cancelled requests can remain queued and later begin provider work. Active-call limits do not bound queued task count or payload memory. Actual cost/memory impact was not benchmarked.

**Alternative/falsifier:** The semaphore genuinely bounds active calls; this finding does not dispute it. A closed-reply/disconnect branch during permit acquisition and provider polling would falsify cancellation continuation. Streaming send failures provide partial cleanup, not cancellation while awaiting the next event.

**Remedy:** Bounded admission and request-owned cancellation propagated through queueing and generation. Cancellation can stop local work but cannot guarantee reversal of a request already accepted by a provider.

**Acceptance:** `cancelled_queued_request_never_starts_model`; `ipc_disconnect_releases_inference_permit`; bounded admission under a paused fake provider. Decide visible overload behavior before implementation.

### F15 — Client and server timeouts cover different portions of a request

**P1 · High should-fix, timeout contract guardrail · timing/integration · confidence 0.99 · static**

**Evidence:** `kask/crates/kask_bridge/src/inference_chat.rs:490–494`:

```rust
Ok(mut stream) => {
    let mut acc = StreamAccumulator::new(model.name().0.to_string());
    while let Some(event) = stream.next().await {
```

**IS:** The server's timer ends at stream establishment (`:464–487`); queue wait and drain lie outside it. The client times the whole response wait at server timeout plus grace (`hkask-inference/src/inference_ipc_client.rs:147–162`).

**Impact:** A healthy long-queued/draining request can hit the client timeout; an established stalled stream can retain its server permit beyond that deadline.

**OUGHT/spec conflict:** The setting still describes establishment plus drain (`kask_bridge/src/settings.rs:115–120`), while later IPC text calls it establishment-only. History `cbcf0d6c33` introduced the wall-clock bound; `019df3ca78` aligned the client with a narrower server timer. Record the chosen semantics rather than laundering either wording into a new spec.

**Alternative/falsifier:** Provider-specific deadlines may mitigate individual calls, not establish a bridge-wide lifetime bound. A full request deadline encompassing all phases would falsify this finding.

**Remedy:** One explicit end-to-end deadline, with transport grace derived from the same scope; or operator-ratified separate total/idle/establishment policies. Preserve GPUI-native timers and AIMD.

**Acceptance:** `established_stalled_stream_times_out`; `queued_request_deadline_includes_wait`; `ipc_client_receives_server_timeout_before_closing`. Reuse F14 cancellation ownership.

### F16 — Escalation recovery misses ordinary recovery and accepts partial improvement

**P2 · High-impact should-fix, recovery guideline · integration/feedback · confidence 0.98 · static**

**Evidence:** `kask/crates/hkask-regulation/src/cybernetics_loop/cycle.rs:1077–1095`:

```rust
if decision == ActionDecision::Accept && improved {
    if let Some(ref sink) = self.alert_escalation_sink {
```

**IS:** Each tick verifies freshly computed actions, not pending conditions from prior ticks. A fleet unhealthy throughout tick A and healthy before tick B yields no unhealthy fleet action on B, so this branch never resolves the pending escalation. Conversely, improving from 2/10 to 3/10 within a tick can resolve it while the original set-point remains unmet.

**Reachability:** Scheduled tick → bridge escalation sink → durable queue resolution (`kask_bridge/src/memory/alert_escalation.rs:162–178`).

**OUGHT:** Auto-resolve when the triggering condition clears, per `31c8de80c7` and condition-key refinement `7445d30346`, not merely when a value changes in a favorable direction.

**Alternative/falsifier:** Manual review is available but does not satisfy automatic recovery. Pending-condition reconciliation against fresh healthy observations would falsify the missed-recovery case. Existing message-reconstruction tests bypass the tick lifecycle.

**Remedy:** Reconcile pending condition identities with later observations using original threshold/direction. Unavailable is not healthy.

**Acceptance:** Unhealthy→healthy across separate ticks resolves exactly once; partial improvement remains pending; unavailable sensing never resolves.

### F17 — “Acceptable” observations masquerade as effectiveness and non-stagnation

**P2 · Should-fix, interpretation guideline/assessment · feedback · confidence 0.94 · static**

**Evidence:** `kask/crates/hkask-regulation/src/cybernetics_loop/cycle.rs:931–938`:

```rust
let accepted = decision == ActionDecision::Accept;
// ...
let plateau = self.stagnation_detector.record_and_check(
    metric.as_str(), action_type_str, accepted,
);
```

**IS:** An unchanged metric yields zero worsening and is accepted. Acceptance resets stagnation, contributes to regulation effectiveness (`loops/core.rs:239–253`), and can support trusted outcomes (`metacognition.rs:312–319`). Yet the loop routes recommendations for human action, and repeated recommendations may be deduplicated.

**Impact:** A persistent unresolved condition can repeatedly be counted as accepted/effective without observed correction. User-facing health exposes these derived metrics through `kask_bridge/src/metacognition_bridge.rs:47–71`.

**OUGHT/proposal:** Keep “acceptable/no worse” distinct from “observed progress” and from causal effectiveness. The exact progress horizon and UI terminology require operator agreement (D4).

**Alternative/falsifier:** Noise tolerance and acceptance ratios are intentional, and an existing test pins that ratio. The finding is not that `Accept` must reject tolerated noise; it is its reuse as proof of efficacy and as a stagnation reset. Independent progress evidence for those consumers would falsify it.

**Remedy:** Separate acceptance, progress, and attribution at the current outcome model. Do not add autonomous action to manufacture an actuator.

**Acceptance:** Constant degraded observations cannot continually erase stagnation or imply corrective improvement; tolerated noise remains acceptable; unapplied advice does not become causal success.

### F18 — Quiet domains retain stale “current-window” measurements

**P2 · Should-fix, observation-freshness guideline · timing · confidence 0.99 · static**

**Evidence:** `kask/crates/hkask-regulation/src/runtime.rs:160–166,188–190`:

```rust
pub(crate) fn increment(&mut self, key: &str) {
    self.check_window();
    *self.counts.entry(key.to_string()).or_insert(0) += 1;
}
```

**IS:** Window expiration runs on writes. Read paths for variety/deficit and outcomes do not check elapsed time (`:175–185,284–303`). No new write means no reset.

**Reachability/impact:** Independent sensor ticks continue querying these ledgers. A formerly active domain can remain deficient or contribute an old success ratio indefinitely after it becomes idle.

**OUGHT:** The recorded one-minute active-domain contract distinguishes idle from deficient (`7445d30346`, `d846f2ced5`). Current observations must age out without requiring new activity.

**Alternative/falsifier:** Lazy expiry works when reads follow writes, not with independent periodic reads. An external periodic reset covering these sensors would falsify the claim; no production reset caller was found.

**Remedy:** Time-aware snapshots: expired variety contributes no current deficit; expired outcome data reports no current sample rather than stale failure or fabricated success.

**Acceptance:** `active_domain_becomes_idle_without_new_write` and `expired_outcomes_are_not_current_samples`, through the ledger/sensor boundary with a controlled clock.

### F19 — Configured outcome thresholds are not applied at construction

**P2 · Should-fix, configuration guideline · configuration · confidence 0.99 · static**

**Evidence:** `kask/crates/hkask-regulation/src/runtime.rs:504–511`:

```rust
RegState::with_history_caps(
    threshold,
    set_points.max_regulation_history,
    set_points.max_skill_span_history,
    set_points.max_alerts,
)
```

**IS:** Parsed warning/critical outcome thresholds do not enter this constructor; `AlgedonicManager` retains defaults. The separate setter at `:524–527` has no production caller in the inspected workspace.

**Reachability:** Startup loads the YAML and constructs the ledger/loop (`crates/zed/src/main.rs:769–795`). The exposed settings are accepted but ineffective.

**OUGHT:** Accepted operator thresholds must affect classification; this is not an objection to a helper merely being unused.

**Alternative/falsifier:** A later production setter call or a constructor that applies the values would falsify the claim. A setter-only test would not test startup wiring.

**Remedy:** Apply thresholds at construction from the same `SetPoints`; eliminate the optional second wiring step.

**Acceptance:** Nondefault warning=0.95/critical=0.90 yields warning after nine successes and one failure; default thresholds retain their intended behavior. Validate configuration bounds.

### F20 — Strict JSON parsing drops overrides from valid commented Zed settings

**P2 · Should-fix, settings compatibility guideline · configuration · confidence 0.98 · static**

**Evidence:** `kask/crates/hkask-services-core/src/standalone_settings.rs:148–157`:

```rust
let user: serde_json::Value = match serde_json::from_str(json) {
    Ok(value) => value,
    Err(error) => {
```

**IS:** The standalone reader uses strict JSON on Zed's shared settings file. Comments accepted by the host parser (`crates/settings_content/src/settings_content.rs:429–434`) instead trigger a warning and full fallback here.

**Reachability/impact:** Corpus embedding/OCR/composition fallback readers use this layer. Environment overrides mask it on some normal launches; when disk fallback is used, a valid commented file loses model overrides.

**OUGHT:** `41243a7173` established shared Zed settings consumption; `55a366a30c` preserves overrides over code defaults. Accepted file syntax is part of that contract.

**Alternative/falsifier:** This is not a claim that every Zed-launched server uses the wrong model. A compatible parser or an invariant that this reader never sees host syntax would falsify the affected fallback case; the latter contradicts the stated shared-file role.

**Remedy:** A compatible comment-capable parser without importing GPUI/host settings machinery into the foundation crate; preserve environment precedence and code defaults.

**Acceptance:** Commented settings preserve embedding/OCR/model overrides; missing fields retain defaults; malformed input surfaces degradation. Do not rewrite the user's settings file.

### F21 — New relative output paths fail unless explicitly prefixed with `./`

**P2 · Should-fix, path usability guideline · interface · confidence 0.99 · reproduced**

**Evidence:** `kask/crates/hkask-mcp-server/src/server/validation.rs:218–223`:

```rust
ancestor = parent;
if ancestor.exists() {
    break;
}
// ...
None => return Err(e),
```

**IS:** Lenient canonicalization of a nonexistent relative output reaches an empty path instead of anchoring it to CWD. Empty-path existence fails, so traversal returns `NotFound`.

**Reproduction:** Actual `contain_for_write("review-output.txt")` and `contain_for_write("review-new-dir/output.txt")` failed in a temporary working directory. The equivalent `./...` forms succeeded. No output files were created by the probe.

**Reachability:** Corpus conversion passes the raw requested output to this helper before creating it (`hkask-mcp-corpus/src/tools/document.rs:62–65,630`).

**OUGHT:** Nonexistent write targets inside allowed roots are supported. Current path documentation uses server CWD as the relative base; this plan does not silently switch to a different project root.

**Alternative/falsifier:** Existing files take the successful canonicalization path; that does not cover new outputs. Callers prepending CWD could mask it, but the identified caller does not.

**Remedy:** Anchor relative inputs to the already-resolved CWD before ancestor traversal; retain containment and symlink protections.

**Acceptance:** New basename, `./basename`, and nested relative outputs resolve equivalently and work through a real handler; traversal/symlink escapes remain denied; existing-file behavior is unchanged.

## Cybernetic synthesis

This is an evidence-scoped mapping, not a numerical viability score for all of hKask.

| Role | Principal inspected mechanisms | Finding |
|---|---|---|
| S1: operations | Tool runtime, inference, persistence | Operations exist; lifecycle/transaction boundaries fail to retain ownership in F05, F08, F10–F15. |
| S2: coordination | Runtime lifecycle state; inference admission and semaphore | Active-call coordination exists, but pending work and late completion can evade desired-state control. |
| S3: monitoring/resources | RegulationLedger, metering, sensors | Metering is deliberately not authorization. Freshness/configuration gaps distort observations (F18–F19). |
| S4: adaptation/assessment | CyberneticsLoop and metacognitive outcome interpretation | Advice is present, but acceptance is conflated with effect and recovery is not reconciled over time (F16–F17). |
| S5: policy/operator | Magna Carta, settings, escalation queue, human decisions | Human authority remains the intended actuator. F01 is a live-enforcement claim needing reconciliation; no OUGHT sovereignty subsystem is assumed implemented. |

Loop properties:

- **Polarity:** principal metric direction is encoded; F16 shows that favorable direction is not equivalent to reaching the target.
- **Delay:** observation cadence is not an effect horizon. F15 omits phases from deadline accounting; F17 observes recommendations too early to infer human-caused improvement.
- **Gain:** action/deviation and acceptance ratios do not establish causal regulatory gain or stability. No measured gain claim is made.
- **Closure:** degraded at pending-condition recovery (F16) and request/process cancellation ownership (F10–F15).
- **Fidelity:** degraded by stale observations, unused thresholds, and accepted→effective reinterpretation (F17–F19).
- **Algedonic channel:** emission, queueing, bridge resolution, and email paths exist. Delivery and end-to-end operator response were not tested; this review does not pronounce the entire channel absent or the system unviable.

Variety here is qualitative rather than a fabricated scalar deficit. The plan restores distinctions the implementation currently collapses: permission versus declaration; cancelled versus pending; stopped versus stale-start completion; recovered versus improving; acceptable versus effective; no current sample versus old failure; establishment timeout versus whole-request deadline.

## Essentialist conclusions

Apply existence → surface → contract as advisory design checks, not permission for a dead-code sweep.

1. **Keep the deep owners.** Ledger, runtime, rotation, interpreter, and inference bridge encode real behavior. Deleting them would push transactions, process ownership, provider semantics, or budgets into callers.
2. **Reduce independently coordinated pieces.** A single connection-scoped transaction replaces several loosely related DB calls. Startup-owned cleanup plus one publication decision replaces token/connection/metadata ownership distributed across phases. One request lifetime coordinates cancellation and deadlines.
3. **Do not invent a replacement platform.** No new supervisor hierarchy, policy framework, monitoring stack, generic lifecycle bus, or autonomous regulator is justified by these findings.
4. **Do not optimize a public-item score at the expense of behavior.** Vocabulary/type crates are intentionally declarative. Public-count heuristics and one-implementor traits are leads, not proven defects. No mechanical “seven public items” refactor is proposed.
5. **Preserve intentionally designed but unwired capabilities pending PM choice.** Existing OUGHT gaps and removed policies stay explicitly distinct from implementation defects.
6. **Remove stale claims only alongside recovered intent.** Fix comments that promise automatic reindexing, nonblocking reconnect, or independent authority only after deciding/enforcing the real contract; do not rewrite specs merely to match broken code.

## Improvement plan

### Operator decisions before implementation

| ID | Experience choice | Recommendation |
|---|---|---|
| D1 | Are child processes restricted by parent-established delegation, or fully trusted within a same-UID trust domain? | Retain restricted delegation where intended; establish parent-held grants. Explicitly disclaim OS isolation from arbitrary same-UID code. Ratify scope before F01 implementation. |
| D2 | Cross-provider embedding override: route it, or reject mismatches on a provider-bound port? | Never send to a different provider silently. Agree on a clear pre-send rejection as an interim release if full routing is deferred; then support intended routing. |
| D3 | What does inference timeout mean; what should a full queue do? | Prefer a whole-request deadline starting at admission, explicit overload/cancellation errors, no replay of unknown-effect work. Ratify any different total/idle/establishment model. Preserve AIMD. |
| D4 | What does “effective regulation” mean for an advisory system; when is progress expected? | Separate acceptable observation from progress and causal attribution. Set the human-response observation horizon before implementing the regulation cluster. |
| D5 | How does passphrase change behave while work is running or reopening fails? | Visible temporary maintenance state; no silent lost work; defined key authority/recovery after every phase. All databases must rotate successfully before keychain publication. |

The relative-output base is already described as server CWD; T20 restores that behavior. If the operator wants active-project-relative paths instead, that is a separate requirement, not an incidental fix.

### Work packages and dependencies

Originally, unchecked boxes meant **proposed**, not scheduled or authorized. The implementation checkpoint above now records authorization and progress; unchecked tasks remain incomplete or blocked. S/M are planning estimates, not promises. T03, T07, and T11 require a design/sizing checkpoint; if the scope exceeds one safe implementation session, split into working vertical increments before coding. Do not preserve a count of 20 at the expense of a sound split.

Each package inherits the corresponding finding's acceptance/falsifier above. Verification must test outcomes, not merely satisfy a quota of test names.

| Task | Vertical outcome and likely code owners | Dependencies / gate | Acceptance and verification | Estimate |
|---|---|---|---|---|
| [x] T01 Isolate credential tests | Running explicit resolver tests cannot overwrite operator keys. `hkask-keystore/src/keychain.rs` and its isolated test support. F09. | None | Disposable-backend round trip; fail/panic path; refuse unsafe ambient ignored-test execution. No real-key tests. | S–M |
| [x] T02 Normalize URL destination classification | Strict tools reject mapped private addresses before transport. `hkask-mcp-server/src/security.rs`; caller fixture. F02. | None | Native/mapped deny matrix, public/permissive controls, zero-fetch assertion. | S |
| [x] T03 Bound the complete Lisp boundary | Supplied forms return a result/error without aborting the host. `hkask-lisp/src/hkask_lisp.rs`; subprocess test support; existing agent seam only if required. F03. | Safety design checkpoint | Matrix for parser/quotes/env/output/builtin/drop work; adversarial subprocesses; ordinary skill predicates remain valid. Do not equate evaluator depth with data depth. | M; split if needed |
| [ ] T04 Enforce parent-held delegation | Child requests cannot enlarge their assigned tool set. `kask_bridge/src/inference_ipc_server.rs`, IPC client/types, grant-producing caller. F01. | D1 | Real dispatch refusal with forged request list; inference-only caller refused; allowed calls still succeed. Protocol change may touch more than five files—justify as one authority boundary. | M |
| [x] T05 Preserve embedding destination intent | Provider-prefixed inputs are routed correctly or rejected before send. `kask_bridge/src/inference_embedding.rs`, provider resolution, relevant IPC fixture. F04. | D2 | Recording transport observes endpoint/model/key consistency; mismatch performs no HTTP; same-provider overrides retained. | S–M |
| [x] T06 Own ledger transactions | Commit/debit remains atomic amid pooled concurrent work. `hkask-ledger/src/hkask_ledger.rs`, existing storage transaction interface, ledger integration tests. F08. | None | Barrier-driven multi-connection debit/commit/rollback/idempotency tests; no partial transactions or cross-operation rollback. | M |
| [x] T07 Publish only desired server lifecycles | Unload remains unloaded; failed startup retains cleanup responsibility. `hkask-mcp/src/runtime.rs`, fixture and reconnect integration tests. F10–F11. | Ownership design checkpoint | Stop/config-replace during discovery; discovery failure; cancellation; repeat runs leave no child/token/tool residue. Keep one runtime owner. | M; split only at safe states |
| [x] T08 Reconnect without blocking the caller | Editor foreground continues during slow restart. `hkask-mcp/src/runtime.rs`, existing spawn hook and integration fixture. F12. | T07 | Foreground-progress probe during paused handshake; cancelable phase deadline; actionable typed failure; no replay of uncertain calls. | M |
| [x] T09 Copy populated RSS schemas safely | A valid feed/entry/search fixture survives rotation. `hkask-storage/src/rotation.rs`, isolated schema fixture matching research DB. F07. | None | Real SQLCipher rotation with FK validation and FTS round trip; failure preserves source/key; test uses actual schema rather than minimal fake tables. | M |
| [x] T10 Preserve post-rotation semantic recall | Old and new memories remain searchable. `hkask-storage/src/rotation.rs`, `embeddings.rs`, recall tests. F06. | T09 where copy logic is shared | Reopen and nearest-neighbor/h_mem JOIN round trip before/after rotation/new write; malformed vectors cause explicit failure, not silent omission. | M |
| [ ] T11 Coordinate passphrase maintenance | Rotation settles writers, preserves all DBs, reopens all consumers, resumes visibly. `kask_bridge/src/identity.rs`, curator store ownership, rotation, settings orchestration. F05. | D5, T07, T09, T10; T08 if reopen uses runtime reconnect | Failure matrix for quiesce/copy/verify/rollback/key publication/reopen; every consumer covered; keychain unchanged on failed rotation; explicit recoverable state after reopen failure. Cross-crate integration requires sizing before edits. | M or re-slice |
| [x] T12 Permit ordinary agent completion | Tool-enabled delegates can finish with an answer while structured results remain required. `kask_bridge/src/inference_chat.rs`, inference request intent/caller as needed, swarm fixture. F13. | None | Fake provider honoring Required/Auto; one useful tool round then final text; `emit_result` remains enforced; no repeated-effect loop. | S–M |
| [ ] T13 Own inference cancellation | Queued cancelled requests do not start provider work; admission is bounded. `kask_bridge/src/inference_chat.rs`, IPC request/disconnect path. F14. | D3 | Pause provider, saturate capacity, cancel queue entries, assert zero dispatch for cancelled requests and capacity release; explicit overload response. | M |
| [ ] T14 Align request deadlines | Queue/establishment/drain share the chosen lifetime bound. Bridge chat timers, settings contract, `hkask-inference/src/inference_ipc_client.rs`. F15. | D3, T13 | Paused queue and stalled established stream; server error precedes transport deadline; zero/disabled behavior explicitly tested; no tokio timer on GPUI. | M |
| [ ] T15 Expire sensor observations | Quiet domains become idle without another operation. `hkask-regulation/src/runtime.rs`, sensor tests. F18. | D4 semantic decision only | Controlled-clock active→idle transition; no-current-data distinct from failed/successful sample; aggregate reading excludes expired contributions. | S–M |
| [ ] T16 Reconcile pending conditions | Recovery clears the right escalation on a later tick. `hkask-regulation/src/cybernetics_loop/cycle.rs`, sensor/bridge sink fixture. F16. | T15; D4 | Complete recovery resolves once; partial improvement stays pending; missing observation stays unresolved; tests run through `tick()`. | M |
| [ ] T17 Report progress honestly | Accepted noise does not become efficacy or erase persistent stagnation. Regulation outcome model/consumers, metacognition bridge. F17. | D4, T16 | Constant degraded trace, tolerated noise, delayed human response, genuinely recovered condition; status names distinguish observation from attribution. | M |
| [x] T18 Apply configured outcome thresholds | Startup sensitivity matches supplied SetPoints. `hkask-regulation/src/runtime.rs`, algedonic construction/tests. F19. | None; verify YAML path independent of T19 | Constructor-driven nondefault/default classifications and invalid-bound tests; avoid setter-only proof. | S |
| [x] T19 Read host-compatible settings | Valid shared-file comments do not discard model overrides. `hkask-services-core/src/standalone_settings.rs`, existing compatible parser dependency if available. F20. | None | JSONC override fixtures, precedence/default controls, malformed-file surfaced degradation; no settings writer or GPUI dependency. | S |
| [x] T20 Resolve new relative outputs consistently | `out.txt` works like `./out.txt` without weaker containment. `hkask-mcp-server/src/server/validation.rs`, real corpus-handler fixture. F21. | None | Temp-CWD basename/nested creation, existing-file behavior, traversal/symlink negative controls. | S |

### Checkpoints and sequencing

Suggested delivery groups: **T01–T03 → checkpoint; T04–T06 → checkpoint; T07–T08 → checkpoint; T09–T11 → checkpoint; T12–T14 → checkpoint; T15–T17 → checkpoint; T18–T20 → checkpoint.** Independent small fixes can be prioritized earlier; dependencies, not row numbers, govern execution.

D1–D5 decisions occur before their affected implementations, not when a late row is reached. In particular, D4 must be settled before the regulation cluster, even though T17 ships its principal metric changes. T18 consumes YAML SetPoints, while T19 consumes host JSONC model settings; no dependency between them was established, but verify the actual startup path again before implementation.

Each checkpoint requires:

1. A targeted regression that fails on the reviewed defect and passes after the change, plus existing relevant tests.
2. Appropriate integration/build validation and `./script/clippy` for the changed scope, with actual results recorded. No bare `cargo clippy` substitute.
3. A working intermediate tree; no partial API refactor, hidden disabled feature, or deferred cleanup presented as done.
4. Applicable operator decisions recorded in the actual spec with date/supersession where necessary.
5. No user-data/credential mutation by test fixtures, no residual children, no forgotten temporary instrumentation.
6. Review affected comments against both recovered intent and behavior; preserve D-seams and error semantics.
7. Human confirmation of the affected experience before treating the package as accepted ground truth.

Only one agent process may edit the tree at a time. Read-only review and test interpretation can run in parallel; shared-state changes, migrations, and implementation of trait/IPC contracts must be coordinated sequentially.

### Recovery and release risks

| Risk | Required mitigation |
|---|---|
| Rotation tests accidentally use real paths/keys | Explicit temporary database roots and disposable keyring; reject ambient production setup. |
| Key rotation succeeds but consumer reopen fails | Defined authoritative key, visible maintenance failure, recoverable reopen/restart path; never overwrite key state by guess. |
| Grant change breaks legitimate delegation | Parent/child compatibility test and explicit grant provisioning; do not silently allow all during transition. |
| New limits reject useful inference/Lisp work | Pin legitimate workloads, define user-visible bounds/overload semantics, explain failures; preserve requested capabilities where safe. |
| Cancellation followed by retry duplicates a mutation or charge | Preserve not-delivered versus outcome-unknown distinction; never automatically replay uncertain operations. |
| Reporting changes mask unresolved conditions | Use stable replay traces with known recovery/no-recovery outcomes; do not score against an LLM's preferred wording. |
| Broad refactor creates rebase or validation friction | Fix at existing owners; re-slice if necessary; no speculative upstream cleanup. |

## Validation actually performed

All Cargo commands were offline and locked, with a 120-second tool timeout. None timed out. No ignored keychain tests, paid provider requests, or live user-database operations were run.

```sh
cargo test --offline --locked -p hkask-lisp -p hkask-forecast -p hkask-types -p hkask-bridge-ontology -p hkask-condenser --lib
cargo test --offline --locked -p hkask-regulation -p hkask-services-core -p hkask-mcp-server --lib
cargo test --offline --locked -p hkask-storage -p hkask-memory -p hkask-event-store -p hkask-ledger --lib
cargo test --offline --locked -p hkask-inference -p hkask-mcp -p hkask-tool-port -p hkask-email --lib -- --test-threads=1
```

| Crate | Passing library tests | Inspection focus / limit |
|---|---:|---|
| hkask-bridge-ontology | 21 | Axis selection, vocabulary contracts and fixture-backed unit checks; not a full ontology/source audit. |
| hkask-condenser | 59 | Engine/algorithm/profile behavior; no semantic-loss benchmark or runtime compression-quality claim. |
| hkask-email | 8 | Configuration/error path and alert sink; no actual email delivery or inbound loop verification. |
| hkask-event-store | 18 | Append identity, retention, corrupt payload handling; no crash-injection campaign. |
| hkask-forecast | 53 | Core math and caller/guard checks; not a full proof of every scenario topology. |
| hkask-inference | 35 | IPC/client/provider interfaces; no paid provider verification. |
| hkask-keystore | Not run | Resolution/provisioning/rotation call paths and dangerous ignored tests inspected; no live keyring access. |
| hkask-ledger | 0 | Commit/debit/pool call chains inspected; library target only, integration tests not executed. |
| hkask-lisp | 13 | Parser/evaluator boundary and isolated abort probe; no exhaustive fuzzing/allocation campaign. |
| hkask-mcp | 17 | Runtime lifecycle/reconnect/dispatch and fixture source; feature-gated reconnect suite not run. |
| hkask-mcp-server | 16 | URL/path/error/bootstrap helpers; public URL/path probes reproduced. |
| hkask-memory | 30 | CRUD/prune/dedup/recall, production writer/reader paths; no production-corpus integrity audit. |
| hkask-regulation | 56 | Sensors, policy, verification, outcome interpretation, settings and history; no live closed-loop experiment. |
| hkask-services-core | 3 | Shared settings, dependency role and callers; no real user-settings mutation. |
| hkask-storage | 32 | Rotation, pools, embeddings, schemas and relevant CRUD; no real DB rotation. |
| hkask-tool-port | 0 | Port/dependency boundary and runtime consumers; zero tests here is not blanket test absence. |
| hkask-types | 32 | Shared contracts, JSON extraction, IDs/path helpers and selected boundary types; not every type invariant. |
| kask_bridge | Not run | Deep static trace of inference/IPC, credentials, rotation, lifecycle and regulation adapters; no editor build or GPUI runtime tests. |
| **Total executed** | **393** | **16 library targets; no test failures.** |

Additional isolated probes:

- **Lisp:** rustc harness imports the actual interpreter file. `(quote <nested-list>)`, evaluation budget 100 steps/depth 1: depth 128 succeeds; depth 20,000 SIGABRT/stack overflow. Ten-second subprocess bounds; core dumps disabled; temporary files automatically removed.
- **URL/path:** temporary Cargo harness depending on the actual `hkask-mcp-server` path. It polls literal-only URL validation once (no DNS/network) and calls path containment in an empty temporary CWD. Mapped private IPs are admitted; equivalent relative paths differ by `./`. Harness/source/lockfile removed afterward; build artifacts remain only in the normal target cache. The temporary harness used offline resolution, not the workspace's locked command; unit-suite commands above used the workspace lock.
- **RSS:** specialist-reported in-memory SQLite mechanism test using the pinned RSS DDL/copy ordering. Not a full encrypted rotation test.

No `./script/clippy`, full editor build, mutation testing, GPUI benchmark, OS confinement test, feature-gated reconnect suite, or exhaustive integration suite was run. These are explicit residual verification gaps, not passed checks.

## Rejected and bounded leads

- Missing sovereignty structs/per-call capability tokens: documented OUGHT or deliberately removed; not newly discovered implementation regressions.
- Fail-open unseeded metering: intentional; do not turn a resource meter into authority.
- Stepped-ramp restoration: superseded by ratified AIMD.
- Adding direct autonomous regulation: contradicts the advisor/human-actuator design.
- Blanket GPUI/Tokio criticism: several inspected paths correctly use GPUI timers or Tokio/channel boundaries. F12 is a specific blocking caller path, not a universal deadlock claim.
- Event append ID race: `INSERT … RETURNING` retains statement identity; corrupt event payloads return errors rather than silently becoming null.
- Old HMem update connection-affinity defect: current path already retains a connection/transaction; do not report historical behavior as current.
- “Ledger postings are not zero-summed”: each posting represents source→destination transfer; summing positive amounts is not the accounting net invariant.
- `get_tool_info` lock-order concern and unused database-opening fallback helpers: no production consumers established, so excluded from live-defect findings.
- DNS rebinding: already documented as residual risk. F02 is a separate literal-IP bypass, not a relabeling of it.
- Forecast tree helper concerns: `combine_tree_probabilities` has concerning general-tree/validation claims, but this pass did not establish a live external caller. No production defect assigned from that helper alone. Independent-parent limitations need a topology-focused next audit, not a speculative math rewrite here.
- Condenser feedback comments reference removed machinery; no new learning subsystem is proposed just to make the comments true. Semantic-preservation quality was not measured.
- Broad public visibility tightening, single-implementor-trait deletion, arbitrary file-size limits: no demonstrated functional benefit; exclude from the improvement backlog.

## Refinement history and review closure

**Pass 1 — detection:** four read-only tracks recovered contract/history and collected source-grounded candidates. Coordinator inspected the remaining foundational/domain crates.

**Pass 2 — counterchallenge and probes:** tested whether authority was independent, whether transaction/lifecycle ownership crossed calls, whether temporal recovery was actually observed, and whether helper tests represented production usage. Rejected historical/dead-surface/style leads. Three coordinator probes converted static hypotheses into reproduced boundary defects; existing test suites remained green.

**Plan challenge:** an independent reviewer accepted the direction only as a proposal and identified compressed sizing, missing dependency edges, and hidden experience choices. This version incorporates explicit D1–D5 decisions, T11's schema/index/lifecycle dependencies, T14's cancellation dependency, D4 before the regulation cluster, unsafe-test isolation, and reopen-failure/key-authority acceptance. Complex packages remain subject to re-slicing; no artificial perfect plan-quality score is assigned.

**What was learned:** tests that validate individual helpers and immediate states can miss ownership and time. A green row-count test does not establish recall preservation; a passing required-tool test does not establish an agent can finish; a favorable delta does not establish recovery; a semaphore does not establish bounded admission; a Tokio handle does not establish nonblocking execution.

**Next review focus:** after operator decisions, convert F02/F03/F09 into safe regression guards first, then validate persistence/lifecycle repairs through composed failure paths. Run the reconnect fixture and GPUI fake-model tests before claiming recovery/cancellation fixes. Revisit the full rotation graph with SQLCipher fixtures and failure injection before accepting preservation. Subsequent reviews should consume these findings and actual repair outcomes rather than restart from generic lint searches.

This bounded review has completed its report/plan deliverable. It does **not** claim exhaustive bug-hunt saturation, complete system safety, or implementation completion. Operator acceptance and implementation verification remain open.
