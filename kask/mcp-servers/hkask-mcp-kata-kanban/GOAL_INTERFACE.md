# Canonical goal bridge pact

Approved contract: `goal-closure-and-forecast-verification-plan.md`, P1–P6.
All types below are exported from `hkask_mcp_kata_kanban`; IDs are
`hkask_types::id::GoalID`, owner is `hkask_types::WebID`. Methods return
`Result<..., KanbanError>`. The host owns authentication: these synchronous
methods are in-process only, never registered as MCP tools or generic RPC.

```rust
KanbanService::goal_create(GoalDeclaration, WebID) -> Result<Goal, KanbanError>
KanbanService::goal_get(GoalID, WebID) -> Result<Option<Goal>, KanbanError>
KanbanService::goal_list(&WebID) -> Result<Vec<Goal>, KanbanError>
KanbanService::goal_close_native(GoalID, WebID, NativeGoalClosure) -> Result<Goal, KanbanError>
KanbanService::goal_pending_work(&WebID) -> Result<Vec<Goal>, KanbanError>
KanbanService::goal_claim_assessment(GoalID, WebID, AssessmentLeaseRequest, Vec<GoalArtifactSnapshot>) -> Result<GoalAssessmentInput, KanbanError>
KanbanService::goal_complete_assessment(GoalID, WebID, AssessmentCompletion) -> Result<Goal, KanbanError>
KanbanService::goal_fail_assessment(GoalID, WebID, AssessmentFailure) -> Result<Goal, KanbanError>
KanbanService::goal_observe_machine_artifact_digest(GoalID, WebID, MachineArtifactDigestReceipt) -> Result<Goal, KanbanError>

KanbanService::goal_mark_projection_delivered(GoalID, WebID, GoalProjectionReceipt) -> Result<Goal, KanbanError>
```

Fields (public; `DateTime<Utc>` dates; `String` unless specified):

- `GoalSource { interaction_id, session_id, source }` identifies a source; a worker-supplied source is **not** authentication.
- `GoalDeclaration { goal_text, criteria: Vec<VerificationCriterion>, task_id: Option<TaskId>, source: GoalSource, forecast: Option<GoalForecast>, supersedes: Option<GoalID> }`. `VerificationCriterion::new(String)` constructs a criterion. IDs are stable `criterion-1` through `criterion-4`; `Goal.criterion_ids` and `Goal.spec_digest` are returned. Criteria never change. New requirements use `supersedes`.
- `NativeGoalClosure { event_id, expected_spec_digest, source: GoalSource, occurred_at: DateTime<Utc>, choice: HumanClosureChoice, evidence_manifest: Vec<GoalEvidence>, artifact_access: Option<LocalArtifactAccessApproval>, machine_artifact_access: Option<MachineArtifactDigestApproval> }`. The host constructs this from the actual user interaction; never deserialize a worker tool call into it. `HumanClosureChoice` has `AccomplishedAchieved`, `AccomplishedNotAchieved`, `AccomplishedUnknown`, `StopAchieved`, `StopNotAchieved`, `StopUnknown` (snake_case serialized). Same event and exact payload replay is idempotent; conflicting event/payload is refused.
- `GoalEvidence { id, source, locator, digest, reliability, oracle, limitation }`. Evidence is frozen at closure. Evidence strings remain untrusted content; the assessor input structurally excludes human choice, progress verdicts, probabilities and scores.
- `AssessmentLeaseRequest { attempt_id, assessor_id, model_version, tool_version, lease_until: DateTime<Utc> }`. Expired leases may be replaced, preserving the expired attempt as failed. Replays of the same claim return the same input. No new claim while a live different lease holds.
- `GoalAssessmentInput { goal_id: GoalID, spec_digest, goal_text, criteria: Vec<AssessmentCriterion>, evidence_manifest: Vec<GoalEvidence>, artifacts: Vec<GoalArtifactSnapshot> }`; `AssessmentCriterion { id, description }`. Persisted verbatim with each attempt and its digest.
- `AssessmentCompletion { attempt_id, input_digest, findings: Vec<CriterionFinding>, limitations: Vec<String> }`; `CriterionFinding { criterion_id, status: CriterionEvidenceStatus, evidence_ids: Vec<String>, note }`; status `Supported | Contradicted | Unknown`. Complete exact criterion coverage, valid evidence references; supported/contradicted require nonempty references exclusively to actually fetched, digest-matched artifact IDs in the frozen attempt input (manifest references alone are refused). `Unknown` is never a pass.
- `AssessmentFailure { attempt_id, input_digest, reason }` preserves failure and leaves work retryable. Completion and failure replays must exactly match.
- `GoalForecast { proposition, probability: f64, horizon: DateTime<Utc>, resolution_deadline: DateTime<Utc>, execution_policy, resources, resolver: GoalForecastResolver, baseline_probability: f64, cohort, update_policy: ForecastUpdatePolicy }`; update policy `FixedAtDeclaration`. Exactly `HumanAcceptance` and `MachineArtifactDigest { locator, expected_sha256 }` are admitted. The latter prospectively declares SHA-256 equality of one absolute local UTF-8 regular artifact at the first scheduled in-window observation; the hash is exactly 64 hex digits. `MachineEvidence { source, oracle }` remains a rejected historical wire shape: arbitrary source/oracle strings never constitute a supported capability. Entire declaration is immutable and prospectively validated at goal creation. No implicit forecast.
- `HistoricalForecastObservation { event_id, source, oracle, occurred_at: DateTime<Utc>, outcome: Option<bool>, evidence_ids: Vec<String>, correction_of: Option<String>, reason }` preserves pre-existing observation JSON in `Goal.observations`, read-only. There is no general observation/outcome writer. Only the source-specific typed digest adapter below may supply a new machine receipt. Startup reconciliation marks prior machine forecasts `UnsupportedResolver`, preserving old observations and score versions as history, never producing new scores from them. Supported human forecasts still score only after an assessment attempt (including failure), retain unknown/pending outcomes and append source corrections; baseline loss zero yields undefined skill.
- `GoalProjectionReceipt { event_id, revision: u64, payload_digest, receipt_id }`. Match a stored `Goal.projections` entry before marking delivered. Same receipt replay is idempotent, conflicting receipt fails; canonical goal is never deleted. Projection entries hold `payload: serde_json::Value` complete snapshot (excluding projection backlog to avoid recursive growth), `event_id`, `revision`, `payload_digest`, `delivered_receipt: Option<String>`.

`Goal` is the full canonical aggregate: original text/criteria/source, stable
IDs/digest, progress verdicts, closure, assessment attempts and pending flag,
forecast/observations/score versions, projections, owner and timestamps.
`goal_pending_work` returns goals with pending assessment or undelivered
projections. Read the stored projection payload, not a reconstructed event.
Legacy records are exposed under `legacy: Option<LegacyGoalArchive>` with
raw JSON, explicit provenance limitation and no eligible forecast/score.

Ordinary MCP routes: create, judge, get, list only. Create requires source;
optional `forecast` uses the typed declaration above. Create/get/list return
the complete goal (`id`, not the old abbreviated `goal_id` response).
Bridge/native ingress and scheduled drainer are separate host-owned work.

## Bounded local artifact assessment (host-only)

`LocalArtifactAccessApproval::BoundedUtf8V1` records the actual native closure
approval after the prompt discloses the listed paths/digests, file sharing with
the configured assessor (including retries), retention in attempts/recall,
sharing retained projections with the configured semantic-indexing provider, and
limits: `GOAL_ARTIFACT_MAX_FILES = 16`, `GOAL_ARTIFACT_MAX_FILE_BYTES = 65536`,
`GOAL_ARTIFACT_MAX_TOTAL_BYTES = 262144` per attempt, including any growth probe.
A per-file growth probe may read one additional byte, always inside the total
read budget and never shared as fetched content. The field defaults to absent
for historical closures, which never become
retroactive consent. Native tool input remains goal ID plus untrusted manifest;
workers cannot supply approval, snapshots, actor or outcome fields. Corrections
do not expand the original read approval.

`GoalArtifactSnapshot { evidence_id, observed_at, outcome }` carries exact
host observations. `GoalArtifactReadOutcome` is tagged by `status`:

- `Fetched { source, sha256, byte_count: u64, content }`: `source` is the actual
  absolute local path, `content` is the bounded exact UTF-8 string, SHA-256 is
  over its exact bytes, and it must match the frozen manifest digest.
- `Unavailable { source: Option<String>, reason, observed_sha256: Option<String>,
  bytes_read: u64 }`: an attempted path/reason/count, never usable support.
  A complete digest-mismatched read retains its actual digest but withholds
  the contents; missing, invalid, unsafe and oversized sources remain explicit.

The host accepts absolute local paths or local `file:` URIs, requires canonical
path equality, and on Unix opens every path component descriptor-relatively
with `NOFOLLOW` (directory components also `DIRECTORY`; the final open is
nonblocking before regular-file validation). It rechecks visible identity and
metadata after bounded reads. No directory enumeration/crawling, network fetch,
command execution or symlink targets are permitted; other platforms currently
produce an unavailable receipt rather than an insecure fallback.

The trusted claim atomically validates and retains the supplied snapshots with
the exact specification/manifest input and digest **before** inference. Replaying
an attempt requires the same lease and exact snapshots; conflicting content or
read receipts fail. Snapshot byte counts/digests, native read approval, time,
manifest IDs, unique coverage and byte caps are checked by the service. No MCP
route accepts this argument. Empty historical inputs keep their serialization
and never gain positive evidence retrospectively.

The host records its package version plus `:goal-local-artifacts-v1` in the
assessment lease's `tool_version` to distinguish this reader protocol from
historical reference-only attempts.

A fresh assessor request contains only the frozen input, not human outcomes,
progress verdicts, probabilities or scores. With no fetched contents the host
records deterministic unknown findings and read limitations without a model
call. Otherwise it requests supported/contradicted/unknown findings, validates
exact criterion coverage and fetched citations, and persists provider failures
for scheduled retries. File contents establish only what was read: producer
claims, alleged test logs and oracle names do **not** prove genuine execution.
Assessment snapshots and findings are not machine forecast observations. Only
the explicitly declared/native-approved scheduled digest adapter below derives
a machine outcome. No human label, worker metadata or assessor status is mapped
into that outcome.

## Supported machine artifact digest source

`MachineArtifactDigest { locator, expected_sha256 }` is the ONE supported machine
oracle. Declaration must precede the immutable horizon; locator syntax admits
only absolute UTF-8 local paths/local file URIs, never remote authorities or
network retrieval. Hash format is 64 hex digits, case-insensitive equality;
there are no arbitrary predicates, commands, boolean outcome arguments or
additional source types.

The native closure prompt separately displays the resolver's exact locator,
decoded path, hash, fixed horizon/deadline, read limits and file-byte authority
limits, even with an empty/unrelated assessment manifest. Its actual ActionChoice
constructs `MachineArtifactDigestApproval { spec_digest, forecast: GoalForecast }`
in `NativeGoalClosure.machine_artifact_access`. The service requires equality
with the **complete frozen forecast and specification**. Manifest approval is
not permission for this read; ordinary MCP cannot grant either consent field.
Corrections grant no new source access. Missing historical consent stays absent.

On startup/ticks the production host adapter waits until the horizon, then takes
ONE bounded nofollow regular-file UTF-8 observation during the inclusive window
`[horizon, resolution_deadline]`, independently of assessment latency. It does
not reuse any earlier assessment snapshot. It uses the existing reader's 64 KiB
content bound plus at most one growth-probe byte, with no execution, symlinks,
directory crawling or URL fetch. Resolver-only file contents are discarded, not
sent to the assessor/indexer: only the typed source receipt is retained/indexed.

`MachineArtifactDigestReceipt { event_id, spec_digest, locator, observed_at,
read, reliability, limitation }` contains no outcome boolean. Its `read` is:

- `Observed { canonical_path, actual_sha256, byte_count }`: a complete bounded,
  stable regular-file read successfully decoded as UTF-8. Core derives `true`
  for digest equality and `false` for a genuine successful different digest.
- `Unavailable { reason, observed_sha256: Option<String>, bytes_read }`: failed,
  denied, missing, unsafe, oversized/truncated, non-UTF-8, unapproved or late;
  outcome is unknown, never false. Read failure reasons/counts are retained.

`event_id` is exactly `<goal-id>:machine-artifact-digest:v1`. The trusted
source-specific ingress checks goal owner, native consent, exact frozen source,
canonical path, digest/count bounds, time, event ID, and fixed file-byte-only
reliability/limitations. It is never an MCP/IPC route and cannot accept human or
worker/model boolean labels. The first receipt, including unknown, is immutable:
exact replay is idempotent; changed replay fails; later polls/restarts never
reread a recorded source or substitute another outcome. A closure/poll after the
deadline performs no read and records unknown, unscored. A read finishing late
retains its actual time but cannot score. Scoring still requires a completed or
failed assessment attempt; the receipt itself is independent of that attempt.

The goal service uses a UTC clock boundary shared by declaration, native closure,
leases, scheduled read receipts and resolution. The host's source adapter uses
that same boundary. `with_goal_clock` is a construction-time system-boundary
hook for deterministic tests, not an MCP field or a horizon mutation mechanism.
Tests advance this clock around real file mutations and reopen real stores;
production uses UTC and the existing scheduled drainer cadence.

## Model-aware projection recovery

A passage's presence alone is not semantic delivery. Before acknowledgment the
host obtains a fresh embedding batch for the active requested model, requires
`batch.requested_model` to match that request, and rejects an empty reported
`actual_model`. It persists `actual_model` when reported, otherwise the requested
identity. It never relabels an old vector as a new producer.

Cached publication is reusable only when its exact passage has a vector-index
row under the freshly provider-confirmed producer identity **and its stored
vector equals the freshly generated producer vector for that passage**. This is deliberately
stricter than KNN's requested-or-actual alias gate: an older requested-alias row
may have been produced before an alias retarget. Comparing fresh output also
catches the ambiguous case where a retargeted alias is echoed as actual.
Without a reported actual
identity, the host always publishes the fresh vector rather than trust a cached
alias. Incompatible rows are atomically semantically repaired, preserving full
payload, age/confidence/provenance and removing superseded passage vectors.
Payload, row and active producer/vector/index eligibility are checked before the
canonical receipt, including after the cancellation yield. This may make an
additional embedding request on replay; it does not rerun a completed assessor
or reread its frozen file snapshots. Previously delivered corpora still require
their separate model-migration policy; this gate addresses pending delivery.

## Reconciliation and source corrections

Additional trusted, in-process methods (no MCP routes):

```rust
KanbanService::goal_archive_legacy(WebID) -> Result<usize, KanbanError>
KanbanService::goal_refresh_forecast(GoalID, WebID) -> Result<Goal, KanbanError>
KanbanService::goal_correct_human_outcome(GoalID, WebID, NativeGoalOutcomeCorrection) -> Result<Goal, KanbanError>
```

- Server startup calls `goal_archive_legacy`; a separately constructed bridge
  service calls it too. It atomically re-encodes old schema rows as typed
  archives, preserving raw JSON including unknown historical fields. It also
  reconciles prior arbitrary-label `MachineEvidence` forecasts to `UnsupportedResolver` without
  deleting observations or old scores. The returned count covers both kinds of
  reconciled rows.
- `goal_refresh_forecast` reconciles forecast deadlines without inference.
  `goal_pending_work` also includes pending/unknown/disputed forecasts, not
  only assessment and delivery work. Deadline expiry never invents `false`.
- `NativeGoalOutcomeCorrection { event_id, expected_spec_digest, source:
  GoalSource, occurred_at: DateTime<Utc>, supersedes_event_id, outcome:
  Option<bool>, reason }` must be constructed from an authenticated host
  interaction. It references the latest human outcome event and appends to
  `Goal.human_corrections`. Original six-choice closure never changes.
  Corrections re-score the original event time, not the correction receipt
  time. A correction cannot move an originally premature outcome into the
  declared evaluation window.
- `goal_judge(GoalID, GoalVerdict, WebID) -> Result<Goal, KanbanError>` is the
  worker-progress method behind MCP, not human closure.
- Only `ForecastStatus::Resolved` indicates the last score is current. If a
  later correction is unknown or disputed, previous score versions remain
  historical, not new independent scored cases. Each canonical goal is one
  forecast event; versions must never increase cohort outcome weight.
