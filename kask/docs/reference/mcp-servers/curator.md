---
title: "Curator MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-10-01
version: "0.42.0"
status: "Active"
domain: "Composition"
mds_categories: [domain, composition, trust, lifecycle]
---

# Curator MCP Server Reference

**Crate:** `kask/mcp-servers/hkask-mcp-curator`
**Tools:** 15 — `curator_ping`, `curator_semantic_search`, `curator_federated_search`, `curator_memory_recall`, `curator_consult`, `curator_algedonic_log`, `reg_query`, `curator_report_skill_use_issue`, `memory_insert`, `memory_update`, `memory_resolve_contradiction`, `curator_memory_prune`, `curator_memory_dedup`, `curator_memory_backfill_embeddings`, `curator_memory_extract`
**Auto-start:** Yes by default with the full built-in set; `kask.mcp.load_default=false` disables the fleet and `kask.mcp.overrides.curator=false` disables this server (server id `curator`, `kask/crates/kask_bridge/src/mcp_servers.rs:170`).

The curator server is the **Curator's sovereign memory and regulation-readback
surface**: the durable h_mem store the whole platform learns through, the
algedonic log the review cycle reads, and the `reg_query` readback over the
RegulationRecord archive. It is the Layer C substrate — the register's L10
row (memory recall/ingest cycle) and L2 row (the regulation/curator cycle's
ORIENT surface) classify its loops.

## The shared-store design

The server reads from the same `agents/curator/curator.db` the agent writes
curator copies to — one store, two processes, no sync
(`mcp_servers.rs:193-194`). `HKASK_WEBID` is mapped to this server only, so
its writes carry the curator's identity (`mcp_servers.rs:197-198`), and the
path resolution finds the same root the agent uses
(`open_curator_stores`, `hkask_mcp_curator.rs:229`).

## Tool groups

- **Recall and consult** — `curator_semantic_search` (`:579`),
  `curator_federated_search` (`:690`, sealed corpus sources in one
  source-aware retrieval), `curator_memory_recall` (`:864`),
  `curator_consult` (`:999`), `curator_ping` (`:433`). The recall path
  covers the entity_ref JOIN — an embedding stored under an entity with no
  h_mem is an orphan the KNN path must surface, never silently drop.
- **Memory writes** — `memory_insert` (`:1418`, evidence-cited: rejects
  inserts without a supporting h_mem id), `memory_update` (`:1539`,
  Bayesian log-odds confidence combination), `memory_resolve_contradiction`
  (`:1616`, forget or update-confidence with a cited reason),
  `curator_memory_extract` (`:1891`, turn-history candidates),
  `curator_memory_backfill_embeddings` (`:1828`).
- **Hygiene** — `curator_memory_prune` (`:1710`, age-gated with the
  spare-recalled-within escape), `curator_memory_dedup` (`:1789`,
  normalized-string value dedup).
- **Regulation surfaces** — `reg_query` (`:1234`, the RegulationRecord
  archive readback), `curator_algedonic_log` (`:1199`, the review cycle's
  algedonic entries), `curator_report_skill_use_issue` (`:1312`, the
  skill-use incident path).

## Architecture

- **Store**: `hkask_memory::MemoryStore` over the sovereign
  `agents/curator/curator.db` (SQLCipher). Memory decay follows the
  Wozniak-Gorzelanczyk forgetting curve; `HKASK_MEMORY_LIFE_DAYS` overrides
  the 180-day default, and a malformed value warns naming the value rather
  than silently falling back.
- **Credentials**: `HKASK_DB_PASSPHRASE` declared **required** — without it
  `open_curator_stores` cannot decrypt the DB and startup fails visibly
  (`hkask_mcp_curator.rs:2391`; the canonical 2-tier chain at `:216`).
  No email-transport vars are injected here — the curator server has no
  email role (`mcp_servers.rs:184`).
- **Distillation**: `DistillationConfig::from_env()` — the consolidation
  pass's configuration surface.

## Testing

The tool-behavior suite (`tests/tool_behavior.rs`) drives the tools through
their parameter seams. The store-level contracts (recall round-trips per
writer variant, orphaned-embedding surfacing, decay) are pinned in the
`hkask-memory` crate; the register's L10 row carries the citations. The
capability-under-test lesson is load-bearing here: tests construct the
server with the embedding store and inference port present — a
capability-stripped constructor can only pin degradation, never function.

## Reference models

The memory-system specification
(`kask/docs/architecture/memory-system-specification.md`) is the recorded
model (the anchor ledger's L10 PARTIAL row); the therapy skill governs the
memory-therapy session pattern over this store.