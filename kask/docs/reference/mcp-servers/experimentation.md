---
title: "Experimentation MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-09-30
version: "0.41.0"
status: "Active"
domain: "Composition"
mds_categories: [domain, composition, lifecycle, trust]
---

# Experimentation MCP Server Reference

**Crate:** `kask/mcp-servers/hkask-mcp-experimentation`
**Tools:** 6 — `experiment_propose`, `variant_register`, `fitness_record`, `selection_record`, `lineage_read`, `population_query`
**Auto-start:** Yes by default with the full built-in set; `kask.mcp.load_default=false` disables the fleet and `kask.mcp.overrides.experimentation=false` disables this server (server id `experimentation`, `kask/crates/kask_bridge/src/mcp_servers.rs:550`).

The experimentation server is the **experiment registry** for the sharded
experimentation program (the repair plan §P8): the durable record of declared
experiments, their variant lineages, grounded fitness, and selection
verdicts. It is a registry, not an executor — fitness entries are
recorded report references to real harness runs, never simulated
(`server.rs:239`, `fitness_record`: "runs are recorded report references
ONLY — never simulated").

## The protocol (§P8's four steps)

1. **Declare** — `experiment_propose` (`server.rs:160`): hypothesis,
   artifact layer (skill | agent_card | prompt_template | tool_schema |
   regulation_scalar | lora_adapter), genotype refs, eval set, fitness
   function, noise band, a **pre-registered prediction with confidence
   in [0,1]**, and energy budget. Optional `experiment_key` converges
   retried calls onto the existing record.
2. **Vary** — `variant_register` (`server.rs:211`): genotype
   configurations (arbitrary JSON) with optional parent lineage; the
   first variant moves the experiment to running; a resolved experiment
   accepts no further variants.
3. **Test** — `fitness_record` (`server.rs:239`): grounded fitness —
   report references plus per-objective scores computed from those
   runs; immutable once written.
4. **Select** — `selection_record` (`server.rs:266`): the verdict —
   selected (naming the winning variant) or rejected (with at least
   one reason). Both outcomes persist as fossils; the first selection
   resolves the experiment; a further selection is a
   `failed_precondition` conflict. `algedonic_reference` names the
   review record that chaired the selection.

Readback: `lineage_read` (`server.rs:301`) returns the ancestry chain
with fitness and selection events — rejected variants and their
reasons are retained "so a rejected mutation is never blindly retried".
`population_query` (`server.rs:367`) is the registry's population view
for the algedonic agenda and the curator's ORIENT (optional layer,
status, created-since, limit; newest first).

## The expectation signal

The `Prediction` type (`types.rs:28-38`) is a pre-registered claim with
the experiment's own confidence — "the tiny controller (§P8.1). The
claim is scored against the experiment's measured outcome by the linked
kanban goal" — the goal loop's Brier scoring (loop-register L9) closes
the expectation loop. This makes the experimentation registry the fleet's
second INV2-exemplary surface after the regulation loop: the
expectation is stored at declaration time and scored against the
observation.

## Architecture

- **Store**: `ExperimentationStore` (`store.rs:93`) over the
  `hkask_storage::database::driver::DatabaseDriver` trait — SQLCipher
  in production (`hkask_storage::open_or_repair`), the in-memory driver
  in tests (`store.rs:572`). One database per server (the per-agent
  default under the hKask data dir, overridable via
  `HKASK_EXPERIMENTATION_DB`); "the registry is the record of record from
  day one — there is no legacy-import path (§P8.7-Q5)"
  (`store.rs:1-6`).
- **Replay convergence**: `experiment_propose` and `variant_register`
  mint server-side identity, so an interrupted-then-retried call could
  duplicate a registry row; optional caller-supplied keys converge
  those replays onto the existing record (a `UNIQUE` index plus
  `INSERT OR IGNORE` — the kata-kanban replay-store shape,
  `store.rs:8-12`).
- **Errors**: `ExperimentationError` (`types.rs:94`) classified per-variant
  for MCP dispatch via `map_experimentation_error` (`server.rs:33`) — never a
  blanket internal.
- **Credentials**: `HKASK_DB_PASSPHRASE` declared **required** — the
  server refuses to start without it (the canonical 2-tier chain,
  `server.rs:446-456`; startup failures map to Infrastructure,
  visible and named, never a silent empty-key open). Pinned by
  `experimentation_declares_its_db_passphrase_as_required`
  (`tests/tool_behavior.rs`).

## Testing

The tool count is pinned by `tool_names_match_live_router` (the
build-generated `TOOL_NAMES` set against the live `experimentation_router`
surface); the tool-behavior suite drives all six tools through their
`Parameters` seams, including the replay-convergence contract and the
credential declaration pin (`tests/tool_behavior.rs`); the store suite
drives the full registry lifecycle on the in-memory driver
(`store.rs:572`).

## Reference models

The repair plan §P8
(`kask/docs/plans/hkask-core-mcp-repair-improvement-plan.md`) records
the experimentation reference model and program with its locked operator
decisions (2026-09-30). The loop-register's L24 row classifies the
registry cycle (primary Layer B, secondary C; INV2 held by design).
