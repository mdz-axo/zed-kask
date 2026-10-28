# Goal ownership boundary

Goals separate immutable requirements, worker progress, independent assessment,
and authenticated human decisions. Forecasts are optional and resolve only
against the human outcome. No probability is synthesized for ordinary goals.

## Supported data

**Forward-only policy:** obsolete goal-schema data is discarded, not migrated.
There is no backward compatibility contract. Obsolete stored goal rows and their
recall projections are removed by an explicit one-time reset, not a runtime
migration, archive, or compatibility subsystem. This contract does not record an
executed reset.

The canonical `kanban:goal` row is keyed by the goal ID. Only schema version **3**
is accepted. `goal_list(owner)` returns `Vec<Goal>`; the MCP list returns that
array, including closed goals. Caller ownership is checked before decoding.
Unsupported versions, malformed current records, key/owner mismatches and duplicate
canonical identities fail visibly rather than being silently omitted. Query failures
remain errors. There is no rejected-row map, rejection cache, historical-key
discovery, or machine resolver.

`Goal` retains the declaration/source, criterion IDs and specification digest,
worker verdict history, original native closure, human corrections, assessment
attempts with exact observed input and result/failure, optional frozen forecast,
and score history. `revision` advances when that meaningful state changes.
`indexed_revision` records the last current-state recall publication; there is
no separate dirty flag, snapshot backlog, or per-event delivery receipt.

## Service operations

Ordinary MCP routes are create, judge, get, and list. The service also accepts
trusted native closure/correction. `goal_scan_for_host()` is a native scheduling
read, not an MCP route: it returns `HashMap<WebID, Vec<Goal>>` from one full-store
query with per-owner schema, ownership, key and duplicate-identity checks. Ordinary
`goal_list(owner)` filters the caller's rows before the common canonical decoder;
invalid current records fail the read visibly. Scheduling still
uses the same bounded, fair work loop; per-goal transitions re-read and claim
canonical state normally rather than relying on the scan snapshot.

The service owns two complete background work operations:

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
The host receives the trusted kanban execution owner at composition and uses it
for both preparation and submission ownership checks. Managed kata-kanban's
child configuration excludes `HKASK_WEBID`, so bootstrap and native composition
share `WebID::anonymous()`; a regression pins that allowlist assumption. This
preserves process-scoped ownership, not per-session or authenticated-human ownership.
Neither requested goal contents nor worker-supplied source fields grant ownership.
`goal_tools.rs` separately captures the initiating actor/session for native
interaction provenance and requires a concrete human `ActionChoice`; workers
supply only goal identity and evidence references.
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
Tests cross public MCP create, distinct native-session attribution, actual native
tools and the drainer with file-backed stores. Foreign ownership is denied before
prompting and again at submission, even with forged native-looking declaration
source. Native inputs reject identity/source fields; only an offered explicit
selection records a decision. These are not deployed-editor end-to-end claims;
changed native assembly still requires rebuild, restart and actual human interaction.
