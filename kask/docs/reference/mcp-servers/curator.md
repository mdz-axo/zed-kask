---
title: "Curator MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-11-12
version: "0.42.1"
status: "Active"
domain: "Composition"
mds_categories: [domain, composition, trust, lifecycle]
---

# Curator MCP Server Reference

**Crate:** `kask/mcp-servers/hkask-mcp-curator`
**Tools:** 15 — `curator_ping`, `curator_semantic_search`, `curator_federated_search`, `curator_memory_recall`, `curator_consult`, `curator_algedonic_log`, `reg_query`, `curator_report_skill_use_issue`, `memory_insert`, `memory_update`, `memory_resolve_contradiction`, `curator_memory_prune`, `curator_memory_dedup`, `curator_memory_backfill_embeddings`, `curator_memory_extract`
**Auto-start:** Yes by default with the full built-in set; `kask.mcp.load_default=false` disables the fleet and `kask.mcp.overrides.curator=false` disables this server (server id `curator`, `kask/crates/kask_bridge/src/mcp_servers.rs:105`).

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
(`kask/crates/kask_bridge/src/mcp_servers.rs:129`). `HKASK_WEBID` is mapped to
this server only, so its writes carry the curator's identity
(`kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:1909`), and the path
resolution finds the same root the editor uses (`open_curator_stores`,
`kask/mcp-servers/hkask-mcp-curator/src/db.rs:381`).

## Request contract (all 15 tools)

- **Undeclared arguments are refused.** Every request type denies unknown
  fields and every input schema carries `additionalProperties: false`. A
  misspelled argument fails the call instead of being dropped.
- **Closed vocabularies are enumerated in the schema**: `ontology_axis`,
  `strategy`, `mode`, `recall_shape`, `failure_origin`.
- **Amounts are checked before they are used.** A lookback window or age too
  large for a calendar offset, a confidence outside 0.0–1.0, a zero limit, and
  a blank query, reason, thread id or report field are `invalid_argument`.
- **A refusal raised while decoding arguments carries no error kind**: the
  wire result is `isError: true` with a text message and no
  `structuredContent`. That is the shared server runtime's behavior, not
  this crate's; a handler refusal carries `{error, kind}`.

## Tool groups

Line numbers are for `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs`.

- **Recall and consult** — `curator_semantic_search` (`:580`),
  `curator_federated_search` (`:654`, sealed corpus sources in one
  source-aware retrieval), `curator_memory_recall` (`:838`),
  `curator_consult` (`:960`), `curator_ping` (`:397`).
  - Each of the three query tools makes one embedding call per request.
  - `curator_memory_recall` takes an `entity` **or** an `ontology_axis`
    with `ontology_value`, never both. Entity recall resets the returned
    rows' decay clocks; axis recall does not.
  - Results carry each h_mem's `provenance` tier (`tool_observed` |
    `model_inference`).
  - A public call context never receives a private-marked row. Every read
    that returns rows applies one filter (`admitted`, `:146`) and reports
    `excluded_private_rows` when it excluded any; rows gated out by the
    embedding model are reported as `model_mismatch_excluded`.
  - `perspective_scoped` returns rows whose perspective is the curator's
    WebID. No production writer sets a perspective (a grep of `kask/` and
    `crates/` on 2026-11-12 finds the builder called only in tests), so this
    scope is empty for rows written by current code.
  - `curator_ping` reports `status: ok` when both stores are open and
    `degraded` with the reason when one is not.
- **Memory writes** — `memory_insert` (`:1291`), `memory_update`
  (`:1411`), `memory_resolve_contradiction` (`:1467`),
  `curator_report_skill_use_issue` (`:1197`).
  - Every write embeds its passage before it commits; the row and its vector
    land in one transaction or neither lands.
  - `memory_insert` requires an existing h_mem id as evidence, stores at
    confidence 0.5 with provenance `model_inference`, inherits the evidence
    row's private mark, refuses a differing value for an existing
    `(entity, attribute)` key, and returns the new `h_mem_id`. A `note` on
    a value that is not an object is kept by storing `{"value": ..., "_note": ...}`.
  - `memory_update` combines the given confidence with the stored one by
    log-odds pooling (0.5 leaves it unchanged) and replaces the row; the
    response carries the replacement id.
  - `memory_resolve_contradiction`: `forget` deletes the target;
    `update_confidence` sets the target's confidence to `new_confidence`.
    The `reason` and `h_mem_ids` are returned in the response and are not
    stored.
  - `curator_report_skill_use_issue` stores the report at confidence 0.5
    and returns its `h_mem_id`, which `memory_insert` accepts as evidence.
- **Maintenance** — `curator_memory_prune` (`:1557`, age-gated;
  default scope is turn storage; `prefixes` or `all_layers` widen it),
  `curator_memory_dedup` (`:1648`, scans the newest `limit` rows
  and says `limit_reached` when older rows may remain),
  `curator_memory_backfill_embeddings` (`:1686`),
  `curator_memory_extract` (`:1736`, lists one thread's turns by
  its complete id; read-only).
  - Only the backfill tool has a `dry_run`. In `model_mismatch` mode a dry
    run still makes one embedding call (the identity probe). A real
    `model_mismatch` sweep whose probe fails stops before deleting anything.
- **Regulation surfaces** — `reg_query` (`:1110`, oldest first, with
  `limit_reached`), `curator_algedonic_log` (`:1072`, newest
  first, at most 500, with `limit_reached`). `phase` is the stored word
  (`sense`, `compute`, `compare`, `act`).

## Error classification

| Condition | Kind |
|---|---|
| Unknown id (evidence, target, h_mem) | `not_found` |
| Value outside a closed set or range, blank required text | `invalid_argument` |
| No embedding model configured; a contradicting value on insert | `failed_precondition` |
| The database did not open because the passphrase does not fit, or it did not resolve; a private-marked row named from a public call; an embedding provider with no credential | `permission_denied` (names `HKASK_DB_PASSPHRASE` or the missing credential) |
| The database is under maintenance or not open; an embedding provider that did not answer | `unavailable` |
| Storage or serialization fault | `internal` |

The open failure is recorded once (`open_failure`,
`kask/mcp-servers/hkask-mcp-curator/src/db.rs:361`) and repeated by every
store-backed tool; embedding failures are classified by `embed_error`
(`:200`).

## Background pass

Started from the server factory, with no tool call
(`kask/mcp-servers/hkask-mcp-curator/src/distillation.rs:165`). Every
`HKASK_MEMORY_DISTILLATION_CADENCE_SECS` (default 600, floor 60; 0 disables
both halves):

1. **Distillation** sends idle threads' turns to the classifier model
   (`HKASK_CLASSIFIER_MODEL`; unset refuses the pass) and stores the returned
   lessons at confidence 0.5 with a per-thread watermark, in one transaction.
2. **Forgetting** (`kask/mcp-servers/hkask-mcp-curator/src/forgetting.rs:116`)
   deletes the turns a watermark older than `HKASK_MEMORY_FORGETTING_DAYS`
   (default 7; 0 disables) covers, with their embeddings.

Both halves read one thread's rows by exact entity
(`kask/mcp-servers/hkask-mcp-curator/src/thread_turns.rs:26`).

## Architecture

- **Store**: `hkask_memory::MemoryStore` over the sovereign
  `agents/curator/curator.db` (SQLCipher). Memory decay follows the
  Wozniak-Gorzelanczyk forgetting curve; `HKASK_MEMORY_LIFE_DAYS` overrides
  the 90-day default, and a malformed value warns naming the value rather
  than silently falling back.
- **Credentials**: `HKASK_DB_PASSPHRASE` is declared **required**: when it
  does not resolve, startup fails (`hkask_mcp_curator.rs:1917`; the canonical
  2-tier chain at `kask/crates/hkask-mcp-server/src/server/credentials.rs:76`).
  A passphrase that resolves but does not fit the database does not stop
  startup: the server runs with both stores down, re-attempts the open on
  tool calls (at most once per 5 s), and reports the cause on every refusal.

## Testing

The tool-behavior suite (`tests/tool_behavior.rs`) drives the tools through
their parameter seams; the pins added by the 2026-11-12 tool review decode
their requests from JSON, as a caller's arguments are. The store-level
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
