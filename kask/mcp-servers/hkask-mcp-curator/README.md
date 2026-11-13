# hkask-mcp-curator — Curator MCP Server

MCP server exposing Regulation history, memory search and per-store liveness (`curator_ping`). Live regulation health (`loop_reading`, alert-log cap and acceptance rate) comes from the **one** built-in `curator_status` AgentTool over the host's metacognition provider, not from a second MCP implementation or an inferred event-history snapshot.

**Version:** v0.40.2 | **Crate:** `hkask-mcp-curator`

## Tools (15)

| Tool | Description |
| --- | --- |
| `curator_ping` | Report per-store liveness: `ok`, or `degraded` with the reason. |
| `curator_semantic_search` | Search Curator memory by semantic similarity. |
| `curator_federated_search` | Search Curator memory plus configured sealed corpus sources without merging stores; returns source/record provenance and per-source status. |
| `curator_memory_recall` | Recall memory by exact entity, or along an ontology axis (one of the two). |
| `curator_consult` | Consult Curator memory with a question. |
| `curator_algedonic_log` | Read the newest algedonic events in a time window (at most 500; `limit_reached`). |
| `reg_query` | Query chronological Regulation records across all namespaces, optionally filtering an exact/dot-descendant namespace prefix before limiting (`limit_reached`). |
| `curator_report_skill_use_issue` | Record a failed or unexpected skill/tool execution with granular `failure_type` and controlled `failure_origin` ownership. |
| `memory_insert` | Insert an evidence-cited semantic memory; returns its id. |
| `memory_update` | Bayesian-combine a new confidence into a memory, optionally replacing its value. |
| `memory_resolve_contradiction` | Resolve contradictory memories by forgetting one or setting its confidence. |
| `curator_memory_prune` | Delete h_mems older than a given age; turn storage by default, wider only by explicit scope. |
| `curator_memory_dedup` | Deterministically deduplicate normalized string memories among the newest `limit` rows. |
| `curator_memory_backfill_embeddings` | Backfill semantic embeddings: missing passages or model-mismatch re-embed — the write side of the search model gate. |
| `curator_memory_extract` | List one thread's stored turns, by its complete id, as candidates for `memory_insert`. Read-only. |

## Request contract (2026-11-12 tool review, CU-01..CU-32)

- Every tool refuses an argument it does not declare, and every input schema
  says so (`additionalProperties: false`). Closed vocabularies
  (`ontology_axis`, `strategy`, `mode`, `recall_shape`, `failure_origin`) are
  enumerated in the schema; an optional field is never listed as `required`.
- Amounts are checked before use: an oversized `hours`, `window_seconds`,
  `max_age_days` or `spare_recalled_within_days`, a confidence outside
  0.0–1.0, a zero limit, or a blank query, reason, thread id or report field
  is `invalid_argument` (the pre-review binary panicked on four of these).
- `curator_memory_recall` takes `entity` **or** `ontology_axis` +
  `ontology_value`; it returns at most `limit` rows per scope (default 100,
  max 500) with `total` and `limit_reached`; only the rows it returns have
  their decay clock reset.
- `memory_insert` and `curator_report_skill_use_issue` return the new
  `h_mem_id`. A `new_confidence` at or below the editor's
  `kask.memory.confidence_floor` (default 0.3), through `memory_update`'s
  pooling or `update_confidence`, is a deferred delete: the editor's
  consolidation pass removes the row on its next tick. Both parameters say so.
- `phase` in `reg_query` and `curator_algedonic_log` is the stored word
  (`sense`, `compute`, `compare`, `act`).
- A refusal raised while decoding arguments carries no error kind (shared
  runtime behaviour); a handler refusal carries `{error, kind}`.

Besides the tools, the server runs a background pass with no tool call: every
`HKASK_MEMORY_DISTILLATION_CADENCE_SECS` (default 600; 0 disables it) it
distils idle threads into lessons through a model call, then deletes the
turns of threads whose distillation is older than
`HKASK_MEMORY_FORGETTING_DAYS` (default 7; 0 disables the deletion). Both
halves read and delete one thread's rows by exact entity (`thread_turns.rs`),
never by prefix: a thread id that extends another's never returns or
forgets the other thread's turns, and a pass costs milliseconds instead of a
full scan per aged watermark (operator ruling 2026-11-12).

`curator_report_skill_use_issue` stores one controlled ownership value:
`skill_contract`, `agent_execution`, `tool_implementation`,
`provider_transport`, `environment_or_baseline`, `operator_interruption`,
`expected_absence`, or explicit `unknown`. The field is required; granular symptoms remain in
`failure_type`; ownership is never inferred from that free-form field.

`reg_query` is the general governance-observability read path. With no
namespace it includes records from every Regulation namespace and cycle phase;
with a namespace such as `reg.skill`, it includes that exact path and
dot-delimited descendants. The time and namespace predicates are applied in
SQL before the requested limit. `curator_algedonic_log` remains the separate
act-phase, algedonic-category view; it returns at most 500 events newest-first
and declares that ordering in its response.


## Configuration

The server opens its sovereign `curator.db` (SQLCipher) using the `HKASK_CURATOR_DB` path and `HKASK_DB_PASSPHRASE` from the keychain. If the DB cannot be opened at startup, the server self-heals: every tool call re-attempts the open (rate-limited to once per 5s) until it succeeds. Until then each store-backed tool returns a typed error naming the cause (a passphrase that does not fit is `permission_denied` naming `HKASK_DB_PASSPHRASE`), and `curator_ping` reports `degraded`.

Federated retrieval is configured by the presence of
`$HKASK_DATA_DIR/agents/curator/federated-sources.json`; the explicit search
has no separate enable toggle. Sources are registered manually: create or edit
this JSON file with `schema_version: 1` and a `sources` array. For each source,
provide its unique `id`, `display_name`, `database_path`, `run_identity_path`,
`representations_manifest_path`, and `index_name` from the sealed corpus run.
The settings picker reads these IDs; it does not register a database or seal a
corpus. The current schema is version 1. Each source names an ID,
display name, database path, sealed `run-identity.json`, representation
manifest, and index name. The source run identity must be current schema 3,
and its representation manifest current schema 2. The server re-computes the
producer's sorted-JSON SHA-256 run ID and verifies the representation manifest
and database digests, exact embedding identities, dimensions, passage prefix,
current database schema, and passage count. Legacy manifests, schemas, model
aliases, and database shapes are rejected rather than migrated or adapted.

Configured external stores open through SQLite `mode=ro&immutable=1` only
after confirming the `-wal` sidecar is absent or empty; a nonempty WAL makes the
source incompatible rather than serving a potentially stale main-file image.
The federated path creates no maintenance lock, inventory entry, WAL/SHM
sidecar, schema, migration, recall touch, co-occurrence link, or corpus write.
Corpus results project only `embeddings.passage_text`, never h_mems such as
`method_signals`. `curator_federated_search` rank-interleaves already-ranked
source batches and exposes `ready`, `unconfigured`, `invalid`, `incompatible`,
or `unavailable` status per source. The server rechecks configured source file
identity (path, size, modification time, and Unix inode) on each search;
changes trigger re-admission and full hash verification. A source that changes
during retrieval contributes no hits. This freshness check assumes normal
filesystem metadata changes; it is not a tamper-proof guarantee against a
writer able to restore metadata or race the check. The explicit tool search
remains available independently of the opt-in Curator chat injection setting.
The Memory page's dropdown selects one registered sealed source for Curator
chat injection (or "Curator memory only" to disable it); the existing
Auto-Inject Memories switch is the master gate. The injector labels external
passages as evidence and never promotes them to Curator memory.

**TODO (operator policy decision):** define the global inclusion rules for
which databases, replicas, and corpus chunks may be registered for federation
and why. Specify eligibility, consent/access boundaries, source provenance,
quality and freshness requirements before broadening registration beyond the
current sealed-corpus contract.

## Dependencies

- `hkask-mcp-server` — MCP runtime and dispatch
- `hkask-storage` / `hkask-memory` — sovereign `curator.db` stores
- `hkask-inference` — the embedding and lesson-extraction port (the editor's IPC bridge)
