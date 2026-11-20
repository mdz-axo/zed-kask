---
title: "Curator MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-11-18
version: "0.42.2"
status: "Active"
domain: "Composition"
mds_categories: [domain, composition, trust, lifecycle]
---

# Curator MCP Server Reference

**Crate:** `kask/mcp-servers/hkask-mcp-curator`
**Tools:** 16 — `curator_ping`, `curator_semantic_search`, `curator_federated_search`, `curator_memory_recall`, `curator_consult`, `curator_algedonic_log`, `reg_query`, `curator_report_skill_use_issue`, `memory_insert`, `memory_update`, `memory_resolve_contradiction`, `curator_memory_prune`, `curator_memory_dedup`, `curator_memory_backfill_embeddings`, `curator_memory_extract`, `curator_citation_staleness_sweep`
**Auto-start:** Yes by default with the full built-in set; `kask.mcp.load_default=false` disables the fleet and `kask.mcp.overrides.curator=false` disables this server (server id `curator`, `kask/crates/kask_bridge/src/mcp_servers/catalog.rs:80`).

The curator server is the **Curator's sovereign memory and regulation-readback
surface**: the durable h_mem store the whole platform learns through, the
algedonic log the review cycle reads, and the `reg_query` readback over the
RegulationRecord archive. It is the Layer C substrate — the register's L10
row (memory recall/ingest cycle) and L2 row (the regulation/curator cycle's
ORIENT surface) classify its loops.

`curator_status`, `curator_directive`, `curator_clear_algedonic_log` and
`record_skill_feedback` are agent-native tools (`crates/agent/src/tools`),
not tools of this server.

## The shared-store design

The server reads from the same `agents/curator/curator.db` the editor writes
turn chunks to — one store, two processes, no sync
(`kask/crates/kask_bridge/src/mcp_servers/catalog.rs:98-102`). `HKASK_WEBID` is mapped to
this server only, so its writes carry the curator's identity
(`kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:2735-2738@run`), and the path
resolution finds the same root the editor uses (`resolve_curator_db_path`,
`kask/crates/hkask-memory/src/curator_store.rs:63`; `open_curator_stores`,
`kask/mcp-servers/hkask-mcp-curator/src/db.rs:219`).

## Request contract (all 16 tools)

- **Undeclared arguments are refused.** Every request type denies unknown
  fields and every input schema carries `additionalProperties: false`. A
  misspelled argument fails the call instead of being dropped.
- **Closed vocabularies are enumerated in the schema**: `ontology_axis`,
  `strategy`, `mode`, `failure_origin`. An optional field is
  never listed under `required`: a `schema_with` enumeration on an `Option`
  field hides the `Option` from the schema generator, so each such field also
  carries `#[serde(default)]` (`kask/mcp-servers/hkask-mcp-curator/src/types.rs`,
  pinned for every server by the `tool_surface_pin!`-emitted
  `no_nullable_property_is_listed_as_required`,
  `kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:126`; the per-server
  pin moved there 2026-11-16 — the 3fac9cfa (b) landing). No
  request path in this editor sends tool schemas under a strict flag that
  would want the opposite shape (`crates/open_ai/src/completion.rs:340` sets
  `strict: Some(false)`; the Anthropic and Google tool definitions have no
  such field).
- **Amounts are checked before they are used.** A lookback window or age too
  large for a calendar offset, a confidence outside 0.0–1.0, a zero limit, and
  a blank query, reason, thread id or report field are `invalid_argument`.
- **A refusal raised while decoding arguments carries a typed kind** (since
  2026-11-16, CU-27): rmcp's pre-handler decode rejection reaches the caller
  with `structuredContent {error, kind: invalid_argument}` — the shared
  `ProvenanceScopedService` decorator maps it (kask/crates/hkask-mcp-server/
  src/server/call_provenance.rs) — while a handler refusal carries
  `{error, kind}` from the tool itself. The kind is on the wire for both.

## Tool groups

Line numbers are for `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs`.

- **Recall and consult** — `curator_semantic_search` (`:671`),
  `curator_federated_search` (`:745`, sealed corpus sources in one
  source-aware retrieval), `curator_memory_recall` (`:962`),
  `curator_consult` (`:1083`), `curator_ping` (`:488`).
  - Each of the three query tools makes one embedding call per request.
  - `curator_memory_recall` takes an `entity` **or** an `ontology_axis`
    with `ontology_value`, never both. It returns at most `limit` h_mems
    (default 100, at most 500), newest first, with `total` and
    `limit_reached`. Entity recall resets the decay clocks of the rows it
    returns, and only those; axis recall resets none.
  - `memory_insert` stores at confidence 0.5; a stored confidence changes
    afterwards only through `memory_update` or
    `memory_resolve_contradiction` (goal and forecast scores do not write
    it).
  - Results carry each h_mem's `provenance` tier (`tool_observed` |
    `model_inference`).
  - A public call context never receives a private-marked row. Every read
    that returns rows applies one filter (`admitted`, `:153`) and reports
    `excluded_private_rows` when it excluded any; rows gated out by the
    embedding model are reported as `model_mismatch_excluded`.
  - **Provenance-routed query and passage embedding (D100):** at every
    embedding site (`curator_semantic_search`, `curator_consult`,
    `curator_federated_search`, and private-marked passage writes such as
    `memory_insert` / `curator_report_skill_use_issue`), the server reads
    `hkask_mcp_server::current_call_provenance()` from the parent-stamped MCP
    `_meta.inference_provenance` envelope. Private-provenance calls embed
    solely through `HKASK_PRIVATE_EMBEDDING_MODEL` (from
    `kask.private_inference.embedding_model`, default
    `ollama/qwen3-embedding:0.6b`), never crossing to the cloud
    `HKASK_EMBEDDING_MODEL`; if the private embedding model is unset or fails,
    recall degrades to exact/keyword legs with a surfaced note rather than
    calling a cloud embedder, and `curator_memory_backfill_embeddings` in
    `model_mismatch` mode skips private-marked rows with a surfaced count.
  - `curator_ping` reports `status: ok` when both stores are open and
    `degraded` with the reason when one is not.
- **Memory writes** — `memory_insert` (`:1402`), `memory_update`
  (`:1546`), `memory_resolve_contradiction` (`:1617`),
  `curator_report_skill_use_issue` (`:1301`).
  - Every write embeds its passage before it commits; the row and its vector
    land in one transaction or neither lands.
  - `memory_insert` requires an existing h_mem id as evidence, stores at
    confidence 0.5 with provenance `model_inference`, inherits the evidence
    row's private mark, refuses a differing value for an existing
    `(entity, attribute)` key, and returns the new `h_mem_id`. The citation
    is stored with the value (an `evidence` array, the distillation path's
    convention), so an inserted memory is traceable to its evidence; it is
    a derived key the contradiction check strips, so a same-value
    re-assertion on a different citation is a re-affirmation, and a
    caller-supplied `evidence` key is refused as reserved. A `note` is kept
    by storing `{"value": ..., "_note": ...}`.
  - `memory_update` combines the given confidence with the stored one by
    log-odds pooling (0.5 leaves it unchanged) and replaces the row; the
    response carries the replacement id.
  - A confidence written at or below `kask.memory.confidence_floor`
    (default 0.3), by `memory_update` or by `update_confidence`, is a
    deferred delete: the editor's consolidation pass removes the row on
    its next tick. Both `new_confidence` parameters say so. The pass runs
    every 300 s and is gated on the `hmems` high-water rowid
    (`kask/crates/hkask-memory/src/consolidation_service.rs:38`): a write
    through this server moves the rowid, so a lowered confidence is seen on
    the next tick; with no write since the last pass the tick is skipped.
    The floor reaches the server as `HKASK_MEMORY_CONFIDENCE_FLOOR`
    (emitted by `mcp_env()` from `kask.memory.confidence_floor` on
    difference from the default, allowlisted in `kask/crates/kask_bridge/src/mcp_servers/catalog.rs:80-169@BUILT_IN_MCP_SERVERS`; the
    documented default 0.3 applies when unset). Since 2026-11-14
    (operator ruling, option (b)) a write that lands at or below it
    succeeds and the response says so: `deferred_delete: true` with a
    `deferred_delete_note` naming the floor (CU-31).
  - `memory_resolve_contradiction`: `forget` deletes the target;
    `update_confidence` sets the target's confidence to `new_confidence`.
    The `reason`, `h_mem_ids` and target id are persisted with a `forget`
    as a `reg.curation` record in the Regulation archive (`reg_query`),
    never in the memory store — the archive answers "what did the server
    do to the store?", never "what should I recall?".
  - `curator_report_skill_use_issue` stores the report at confidence 0.5
    and returns its `h_mem_id`, which `memory_insert` accepts as evidence.
    The report row carries the CALL's inference provenance (CU-19 a,
    2026-11-14): a report filed from a private thread lands private-marked,
    embeds through the private leg's LOCAL model and never recalls into a
    public context; a public call is unchanged.
- **Maintenance** — `curator_memory_prune` (`:1733`, age-gated;
  default scope is turn storage; `prefixes` or `all_layers` widen it),
  `curator_memory_dedup` (`:1838`, scans the newest `limit` rows
  and says `limit_reached` when older rows may remain),
  `curator_memory_backfill_embeddings` (`:1889`),
  `curator_memory_extract` (`:1939`, lists one thread's turns by
  its complete id; read-only),
  `curator_citation_staleness_sweep` (`kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:2479@CuratorServer::curator_citation_staleness_sweep`, therapy category g;
  read-only — flags knowledge-layer h_mems whose code citations no
  longer resolve against the live workspace tree at `repo_root`: missing
  files, out-of-bounds `file:line`, `D<N>` seams retired from
  `DIVERGENCE.md`. `rows_deleted` is `0` always; a private-marked flagged
  row is withheld from a public call and counted in `excluded_private_rows`;
  a missing `DIVERGENCE.md` is reported as `divergence_md_present: false`
  and skips seam checks visibly).
  - Only the backfill tool has a `dry_run`. In `model_mismatch` mode a dry
    run still makes one embedding call (the identity probe). A real
    `model_mismatch` sweep whose probe fails stops before deleting anything.
- **Regulation surfaces** — `reg_query` (`:1214`, oldest first, with
  `limit_reached`), `curator_algedonic_log` (`:1171`, newest
  first, at most 500, with `limit_reached`) returns deviation-carrying
  signals only: idle heartbeats (zero-deviation records) are excluded
  before the limit is applied and counted in `excluded_heartbeats`
  (CU-24; the anchored definition — a record of zero deviations is not
  an algedonic event). `phase` is the stored word
  (`sense`, `compute`, `compare`, `act`). The archive records the server's
  own curation (CU-25): store() writes as `reg.memory` records (the memory
  store's ledger sink — the same wiring the editor's memory port uses) and
  one `reg.curation` record per deliberate forget (carrying the reason and
  ids), prune and dedup (carrying their counts). Nothing is written to the
  memory store about a deletion; the mechanical nightly forgetting pass
  stays a log line (operator ruling 2026-11-14).

## Error classification

| Condition | Kind |
|---|---|
| Unknown id (evidence, target, h_mem) | `not_found` |
| Value outside a closed set or range, blank required text | `invalid_argument` |
| No embedding model configured; a contradicting value on insert | `failed_precondition` |
| The database did not open because the passphrase does not fit, or it did not resolve; a private-marked row named from a public call; an embedding provider with no credential | `permission_denied` (names `HKASK_DB_PASSPHRASE` or the missing credential) |
| The database is under maintenance or not open; an embedding provider that did not answer | `unavailable` |
| Storage or serialization fault | `internal` |

The open failure is recorded once (`map_open_failure` / `store_error`,
`kask/mcp-servers/hkask-mcp-curator/src/db.rs:163-217`) and repeated by every
store-backed tool; embedding failures are classified by `embed_error`
(`:207`).

## Background pass

Started from the server factory, with no tool call
(`kask/mcp-servers/hkask-mcp-curator/src/distillation.rs:165`). Every
`HKASK_MEMORY_DISTILLATION_CADENCE_SECS` (default 600, floor 60; 0 disables
both halves):

1. **Distillation** sends idle threads' turns to the classifier model
   (`HKASK_CLASSIFIER_MODEL`; unset refuses the pass) and stores the returned
   lessons at confidence 0.5 with a per-thread watermark, in one transaction.
   Since 2026-11-14 (CU-19 b) a batch holding private-marked turns generates
   through the private leg's LOCAL classifier model
   (`HKASK_PRIVATE_CLASSIFIER_MODEL`, from `kask.private_inference.classifier_model`)
   only; unset/empty skips the batch with a surfaced `private_batches_skipped`
   count — never the configured cloud classifier — and the thread's watermark
   stays unadvanced, so its turns are never forgettable without proof of
   extraction.
2. **Forgetting** (`kask/mcp-servers/hkask-mcp-curator/src/forgetting.rs:116`)
   deletes the turns a watermark older than `HKASK_MEMORY_FORGETTING_DAYS`
   (default 7; 0 disables) covers, with their embeddings.

Both halves read and delete one thread's rows by exact entity
(`kask/mcp-servers/hkask-mcp-curator/src/thread_turns.rs:26`;
`forgetting.rs:173`, `:186`), never by prefix (operator ruling 2026-11-12).
A turn stored under a key that extends a thread's key is not forgotten
with it, and an ordinary pass takes milliseconds: measured on the installed
binary after the change, 51–125 ms (5 passes) against 1.2–3.1 s before, with
13 of 77 earlier passes at 25–71 s whose cause was not identified.

## Architecture

- **Store**: `hkask_memory::MemoryStore` over the sovereign
  `agents/curator/curator.db` (SQLCipher). Memory decay follows the
  Wozniak-Gorzelanczyk forgetting curve; `HKASK_MEMORY_LIFE_DAYS` overrides
  the 90-day default, and a malformed value warns naming the value rather
  than silently falling back.
- **Credentials**: `HKASK_DB_PASSPHRASE` is declared **required**: when it
  does not resolve, startup fails (`kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:2735@run`; the canonical
  2-tier chain at `kask/crates/hkask-mcp-server/src/server/credentials.rs:76`).
  A passphrase that resolves but does not fit the database does not stop
  startup: the server runs with both stores down, re-attempts the open on
  tool calls (at most once per 5 s), and reports the cause on every refusal.

## Testing

The tool-behavior suite (`tests/tool_behavior.rs`) drives the tools through
their parameter seams; the pins added by the 2026-11-12 tool review decode
their requests from JSON, as a caller's arguments are. Two surface pins read
the live `tools/list`: the forwarded sentence of every description is
complete and carries the tool's load-bearing fact
(`forwarded_descriptions_are_complete_and_carry_the_load_bearing_fact`,
`kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:2951@forwarded_descriptions_are_complete_and_carry_the_load_bearing_fact` — the editor forwards a description only to
its first sentence within 200 bytes), and no nullable property is listed as
required (the `tool_surface_pin!`-emitted pin,
`kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:126`, shared by every
server since the 2026-11-16 3fac9cfa (b) landing). The review's artifact and probe scripts are under
`~/Documents/zk-data/skills/mcp-tool-review/2026-11-12-curator/`. The store-level
contracts (recall round-trips per writer variant, orphaned-embedding
surfacing, decay) are pinned in the `hkask-memory` crate; the register's L10
row carries the citations. `tests/live_memory_recall.rs` holds two ignored
tests that open the operator's live database; they are never part of a
default run.

## Reference models

The memory-system specification
(`kask/docs/architecture/memory-system-specification.md`) is the recorded
model (the anchor ledger's L10 PARTIAL row); the therapy skill governs the
memory-therapy session pattern over this store.
