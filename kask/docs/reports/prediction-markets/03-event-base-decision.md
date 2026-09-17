# T12 Decision Record: Event-Base Persistence — Flat Store vs Graph Database

**Date:** 2026-08-05
**Status:** DECIDED (flat store, revisit triggers documented)
**Method:** essentialist deletion test, per `tasks/plan.md` T12 acceptance criteria.

---

## The question

Does the prediction-markets data service (and eventually companies/scenarios/corpus) need a graph "event base" — events as nodes, typed edges (market→resolves→outcome, market→matches→scenario_event, event→parent_series→event, market→references→company) — or is the flat store sufficient?

## The deletion test (G1)

**If we delete the graph database from the design, does complexity reappear in consumers?**

Searched for concrete consumer relationship queries that a flat store cannot serve:

1. **Calibration loop (T10):** bucket-keyed lookups (`brier(bucket)`, `sample_size(bucket)`). A `HashMap<String, Vec<Observation>>` + JSONL journal serves this. No traversal. **Flat suffices.**
2. **market_match (T4c):** scores a query against a flat candidate list. No relationship query. **Flat suffices.**
3. **scenario_from_markets (T8):** consumes a single record by value. **Flat suffices.**
4. **CMP construction (T14):** needs "all markets in a base-event family, bucketed by time-to-resolution" — a single-level filter on `series`, not a graph traversal. **Flat suffices.**
5. **Hypothetical future queries** ("which scenario events share a market anchor?", "which companies does this market reference?"): **zero of these exist in code today.** The grep for relationship queries across the server returns nothing.

**Verdict: no demonstrated consumer relationship query.** The deletion test fails the graph — adopting it now would be speculative generality (the `.rules` trait-with-one-impl trap at infrastructure scale: a graph DB with no traversal queries is a database with 1,700 lines of unused surface area).

## Decision

**Keep the flat store** (in-memory `HashMap` + JSONL journal, already shipped in T5/T10). Defer the graph decision.

## Revisit triggers (adopt a graph when ANY becomes true)

1. **Two or more concrete consumer queries** require multi-hop traversal that a flat store answers only by re-deriving joins in Rust at the call site (the T12 acceptance criterion).
2. **Cross-server entity linking** lands: a real need to traverse market↔company↔corpus-doc↔scenario-event (e.g., "which corpus documents reference companies with live markets moving against them"). The corpus server already has triple machinery (`services/triples.rs`); if that becomes the integration seam, evaluate whether it subsumes the graph need before adding a second store.
3. **Match-history analytics:** if matcher-quality feedback (T10's loop-attribution concern — distinguishing market miscalibration from wrong-event matches) needs relationship queries over match history.

## Pre-registered backend ranking (from the 2026-08-04 research, re-verified)

If a revisit trigger fires, the evaluation order is:

1. **Grafeo** — Apache-2.0, pure-Rust embedded, GQL/Cypher/SPARQL + vector/BM25, tokio async-storage. Risk: 0.5.x maturity, vendor benchmarks.
2. **CozoDB** — MPL-2.0, Datalog (best query model for recursive event relationships), SQLite backend, time-travel (useful for belief-revision auditing). Risk: pre-1.0 storage-format instability.
3. **SurrealDB** — mature, but BSL 1.1 license needs a legal call for an editor-shipped component, and the footprint is heavy.
4. IndraDB (no query language) / Kuzu (archived Oct 2025) — disqualified.

**CRDT position (explicit):** none of the candidates offer CRDT/multi-writer replication. If multi-device sync ever becomes a requirement, layer automerge/yrs above the store as the sync substrate with the graph as a local materialized view. Do not select a backend on CRDT claims none of them make.

**Port pattern:** if adopted, storage goes behind a thin trait in a new `kask/crates/hkask-eventgraph` crate (ADR-042 port-promotion: the trait is created when the second consumer materializes, not before) so the backend is swappable while the schema stabilizes.

## What this decision protects

- The MCP server stays dependency-light (no 40MB embedded DB) until evidence demands it.
- The JSONL journal format is forward-compatible: rows can be replayed into a graph store later without migration loss.
- T14/T15 (CMP, residual risk) proceed on the flat store without waiting on a database decision.
