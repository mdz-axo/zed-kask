# Goal ownership boundary

Goals separate immutable requirements, worker progress, independent assessment,
and authenticated human decisions. Forecasts are optional and resolve only
against the human outcome. No probability is synthesized for ordinary goals.

## Supported data

The canonical `kanban:goal` row is keyed by the goal ID. Schema version **3** is
supported. Direct reads reject unsupported versions, malformed records, and
key/owner mismatches without rewriting or deleting stored bytes. Listing returns
one `GoalListing { goals, rejected }`: `goals` contains supported caller-owned
records; `rejected` maps omitted stored row keys to rejection reasons. Owner
filtering happens before decoding or exposing rejections. The MCP list serializes
both fields, including an empty rejection map for ordinary listings. The drainer
logs each rejection with its identity and reason, then processes supported goals
for that owner and every other owner. Database query failures remain errors. There is no legacy
conversion, startup migration, historical-key discovery, or machine resolver.

`Goal` retains the declaration/source, criterion IDs and specification digest,
worker verdict history, original native closure, human corrections, assessment
attempts with exact observed input and result/failure, optional frozen forecast,
and score history. `revision` advances when that meaningful state changes.
`indexed_revision` records the last current-state recall publication; there is
no separate dirty flag, snapshot backlog, or per-event delivery receipt.

## Service operations

Ordinary MCP routes are create, judge, get, and list. The service also accepts
trusted native closure/correction and owns two complete background operations:

- `goal_assess(id, owner, capture, assess)` owns claim acquisition, expiry,
  input persistence, result validation, conditional finalization, and scoring.
  The callbacks only capture evidence and request independent inference.
  A private token and expiry coordinate separate processes. An active claim
  skips capture/inference; an expired attempt becomes a retained failure.
  Late results cannot complete another attempt. Callers do not send claim,
  completion, failure, digest, or replay envelopes.
- `goal_index_current(id, owner, prepare)` refreshes forecast eligibility and
  owns the canonical revision check and index acknowledgment. Preparation
  embeds outside the database lock and returns a synchronous publisher.
  Under the canonical immediate transaction, the service rejects a changed
  revision, runs publication only if still dirty, and records that revision.
  The publisher must not call back into the canonical store. This lock order
  is canonical store then destination memory store.

Closure/correction replay uses exact input equality, not a redundant payload
hash. Human corrections append without changing original closure time or
artifact-access consent. Changed requirements declare a successor goal.

## Native authority and evidence

`NativeGoalHost` is a native-only dependency boundary, not an MCP route.
`goal_tools.rs` captures the actual actor/session and requires a concrete
human `ActionChoice`; workers supply only goal identity and evidence references.
The six closure choices preserve accomplished/stop independently of
achieved/not-achieved/unknown. Corrections are separate human interactions.

Before reads, the prompt discloses exact paths/digests, assessor and semantic
indexer sharing, retained contents, and retries. Native approval permits only
bounded local UTF-8 regular files: 16 entries, 64 KiB contents each, 256 KiB
aggregate reads including growth probes. Descriptor-relative nofollow reads
reject unsafe paths/symlinks. There is no execution, network fetching, or
producer-authenticity claim.

Exact fetched contents or explicit unavailable/mismatch receipts are persisted
before inference. The assessor sees only the frozen specification, manifest,
and observed artifacts—not worker verdicts, human labels, forecast or scores.
No fetched contents produces deterministic Unknown without a model request.
Completion must cover each criterion once, and support/contradiction must cite
actually fetched digest-matched IDs. Strict `AssessmentResult` and
`CriterionFinding` decoding rejects extra fields. Model findings never replace
the human outcome, in either direction of disagreement.

## Optional human-outcome forecasts

A declaration may freeze proposition, probability, horizon, resolution deadline,
execution policy, resources, matched baseline probability and cohort, with
`resolver: {"kind":"human_acceptance"}` and fixed-at-declaration update policy.
Declaration must precede the horizon, and the deadline cannot precede it.
Scoring waits for an assessment attempt result, including visible failure.
Only an explicit known human outcome whose **original closure time** falls
inside the frozen window is eligible. Corrections relabel that original event;
they do not move it into the window. Unknown is unscored; old scores remain
history rather than becoming a current resolution. Baseline loss zero has
undefined skill. Scores never change recall-memory confidence.

## One current recall record

The bridge writes one `curator:goal:<id>` / `current` memory, containing the
current meaningful canonical state and histories, not coordination/delivery
fields. It uses existing `MemoryStore` semantic write/revision publication to
commit the row and vector atomically. Revision replaces the previous memory
and embedding rather than appending snapshots.

Every dirty publication requests a fresh vector from the current provider,
checks requested/actual model identity and vector count, and verifies exact
passage/vector/index visibility. Unconfirmed producer identity is not treated
as permission to reuse a cached alias vector. Publication and acknowledgment
run under the canonical revision lock, preventing an older preparation from
overwriting newer indexed state across processes. A destination commit followed
by failed canonical acknowledgment leaves dirty work: restart either reuses
the exact current row/vector or atomically repairs that single record for the
active producer. Canonical goals are never deleted by successful indexing.

The scheduled host remains gated by `curator.always_on`; disabling it warns and
retains work. Missing inference/indexing capabilities are visible and retryable.
Tests drive the actual native tools, service and drainer with file-backed stores;
these are not claims of a deployed-editor end-to-end run.
