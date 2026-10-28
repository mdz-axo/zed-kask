---
title: "Scenarios MCP Server Reference"
audience: [developers, architects]
last_updated: 2026-10-28
version: "0.41.0"
status: "Active"
domain: "Composition"
mds_categories: [composition, lifecycle]
---

# Scenarios MCP Server Reference

**Crate:** `kask/mcp-servers/hkask-mcp-scenarios`
**Tools:** 22 — `scenario_frame`, `scenario_frame_document`, `scenario_brainstorm`, `scenario_build`, `scenario_quantify`, `scenario_propagate`, `scenario_recompute_posteriors`, `scenario_calibrate`, `scenario_update`, `scenario_synthesize`, `scenario_cross_validate`, `scenario_declare`, `scenario_score`, `scenario_calibration`, `scenario_assess`, `scenario_triage`, `scenario_status`, `scenario_from_markets_set`, `forecast_persist`, `forecast_record`, `forecast_list`, `scenario_risk_measure`. The direct market-record bridge is `scenario_from_markets_set`; a single record is passed as a set of one. The four forecast tools were ported from the retired companies server (2026-10-18).
**Auto-start:** Yes by default with the full built-in set; operators can disable the fleet or this server through `kask.mcp` (`kask/crates/kask_bridge/src/settings.rs:140-165`; `kask/crates/kask_bridge/src/mcp_servers.rs:254-268,700-710`).

Tool count is pinned against the live `combined_router()` by `tool_surface_is_exactly_22_registered_tools` (the `mod tests` pin in `kask/mcp-servers/hkask-mcp-scenarios/src/hkask_mcp_scenarios.rs:1726`).

## Pipeline Architecture (DIAG-RF-005)

This diagram shows the recommended flow between the 22 MCP tools, grouped by methodology phase. Arrows describe how outputs can feed later work, not an enforced tool-call order; dashed arrows show optional or independent paths. Tools process caller-supplied inputs without session-order advisories. Framing guidance remains available through `scenario_frame`, but supplied answers can go directly to `scenario_frame_document`; actual input and persisted-state requirements still apply.[^tetlock-scenarios-ref][^schwartz-scenarios-ref]

```mermaid
flowchart TD
    subgraph Framing["Framing and construction"]
        frame["scenario_frame"] --> frame_doc["scenario_frame_document"]
        frame_doc --> brainstorm["scenario_brainstorm"] --> build["scenario_build"]
    end

    subgraph Bridges["Prediction-market bridges"]
        markets["scenario_from_markets_set"]
    end

    subgraph Computation["Quantification and updating"]
        quantify["scenario_quantify"]
        propagate["scenario_propagate"]
        posteriors["scenario_recompute_posteriors"]
        calibrate["scenario_calibrate"]
        update["scenario_update"]
        cross_validate["scenario_cross_validate"]
    end

    subgraph Synthesis["Synthesis, tracking, and assessment"]
        synthesize["scenario_synthesize"]
        score["scenario_score"]
        calibration["scenario_calibration"]
        triage["scenario_triage"]
        assess["scenario_assess"]
    end

    subgraph State["Server state"]
        status["scenario_status"]
    end

    build --> quantify
    markets --> quantify
    quantify --> propagate
    quantify -.-> posteriors
    quantify --> calibrate
    calibrate --> update
    calibrate --> cross_validate
    calibrate --> synthesize
    quantify --> declare["scenario_declare"] --> score --> calibration
    synthesize --> assess
    triage -.-> build
    status -.-> calibration
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-RF-005
verified_date: 2026-10-06
verified_against: kask/mcp-servers/hkask-mcp-scenarios/src/hkask_mcp_scenarios.rs (the 22 #[tool] handlers; tool_surface_is_exactly_22_registered_tools pin); kask/mcp-servers/hkask-mcp-scenarios/src/superforecast.rs; kask/mcp-servers/hkask-mcp-scenarios/src/types.rs
status: VERIFIED
-->

## The three Schwartz surfaces

Two distinct "Schwartz scenario" semantics live across the stack; they
share the reference model (Schwartz, *The Art of the Long View*, 1991)
and nothing else — each has its own contract:

1. **Divergent-futures narratives** — the `scenario-planning` skill's
   Phase 2b templates (`scenario-planning/axes-and-narratives` and
   siblings): Schwartz's steps 5–8 — two critical-uncertainty axes, four
   quadrant stories, implications, early-warning indicators. LLM
   judgment in skill templates; the scenarios server quantifies the
   event-tree backbone underneath them.
2. **Probability-weighted four-scenario distribution** —
   Tetlock-calibrated probabilities distributed across four Schwartz
   scenarios to produce a probability-weighted value, via the skill layer's
   own templates over caller-supplied inputs.

The scenarios server itself implements the MAIA event-based template
(event sets as yes/no questions over fixed horizons; four scenario
types, three horizons) and the Tetlock quantification; the Schwartz
narrative phases are the skill layer's.

## Design model

Twelve primitives, derived from the four reference models in the
2026-10 redesign review, define the server's shape (the review's
from-scratch verdict kept the event, tree, synthesis, journal,
calibration, impact-mapping, market-bridge, and certainty-tier
primitives substantially as built and added the project record; the
full derivation is in git history — see the Redesign record):

| Primitive | Contract | Reference-model source | Lives in |
|---|---|---|---|
| FramingDocument | the project charter: focal question, decision at stake, horizon, scope, stakeholders, success criteria, constraints, assumptions | Chermack Phase 1; Schwartz step 1 | `scenario_frame` → `scenario_frame_document`, persisted under the project record |
| BinomialEvent | yes/no question + deadline + probability + basis + Fermi sub-questions + base rate | MAIA event composition; Tetlock question discipline | `ScenarioEvent` |
| DependencyEdge | parent ids + bitmap-ordered conditional table, length 2^\|parents\| | MAIA path dependence; Bayesian-network CPT | `depends_on` |
| EventTree | marginals, topological order, joint = P(all events occur) | MAIA template; Tetlock inside view | `scenario_quantify` and the market bridges |
| Perspective + Synthesis | explicitly equal-weight aggregation with disagreement score | Tetlock dragonfly-eye | `scenario_synthesize` |
| ForecastRecord + journal | append-only journal + snapshot; prospectively frozen tree marginals and timestamp cutoffs (schema v5) | Tetlock record/score | `ForecastStore` — `scenario_declare` freezes records; `scenario_score` records matching outcomes |
| CalibrationObservation | eligible-history bins, signed probability error with sample/bin scope and isotonic diagnostic; no confidence or calibration certificate | Tetlock calibration | `scenario_calibration` |
| ScenarioProject | the spine: framing document, trees, assessment history, journal linkage | Chermack's unit of assessment | `ProjectStore` (see Project persistence) |
| Narrative / Implications / Indicators | quadrant stories, strategies, early-warning indicators | Schwartz steps 5–8 | the skill layer (`scenario-planning` templates) — a ruled contract, not a server primitive |
| ImpactMapping | per-node yes/no DCF deltas; 2^N paths weighted by probability | MAIA "events drive the financial forecast" | caller-authored impact mappings consumed by the skill layer (the former `scenario_impact_valuation` tool retired with the companies server; the shared `hkask_forecast` risk core survives) |
| MarketBridge | market records → root events with provenance and gates | prediction-market base rates as priors | `scenario_from_markets_set` |
| CertaintyTier | proximate ≥67% / probable 33–66% / possible <33% | MAIA three-level tier | `hkask_forecast` |

## Tool reference

### Framing (2)

| Tool | Description | Key params |
|------|-------------|------------|
| `scenario_frame` | Start a conversational framing session: a 7-turn protocol with behavioral-psychology openings and improv mode guidance. Run FIRST, before `scenario_brainstorm`. | `subject` |
| `scenario_frame_document` | Structure completed framing answers into a typed `FramingDocument` and persist it under the scenario project record (id defaults to the subject; a re-run updates in place). Feeds `scenario_brainstorm`. | `subject`, answers JSON, optional `project_id` |

### Ideation (1)

| Tool | Description | Key params |
|------|-------------|------------|
| `scenario_brainstorm` | Generate a 4-round structured brainstorming protocol (DIVERGE → GROUND → LINK → PRUNE) with persona rotation, temperature guidance, and quality gates. | `frame` (FramingDocument) |

### Structuring (1)

| Tool | Description | Key params |
|------|-------------|------------|
| `scenario_build` | Build a scenario event-tree scaffold from web research: returns an extraction template (event schema, dependency format, certainty tiers, Tetlock's 10 commandments) the LLM fills against `research_text`. | `frame`, `research_text` |

### Market bridges (1)

| Tool | Description | Key params |
|------|-------------|------------|
| `scenario_from_markets_set` | Compose a set of prediction-market records into a validated `EventTree` with caller-authored dependency edges; per-record gates, duplicate-question flags, cycle and CPT-size rejection; returns resolved tree (marginals, joint probability) plus warnings. | `market_records`, `match_confidences`, `dependency_specs` |

### Computation (5)

| Tool | Description | Key params |
|------|-------------|------------|
| `scenario_quantify` | Quantify an event tree: topological sort, marginal probabilities via conditional propagation, joint probability, per-event variance contribution, sensitivity ranking; detects cycles and missing parents. | `events` JSON |
| `scenario_propagate` | Update one event's prior and propagate through the tree: descendant marginals and joint probability recomputed; returns the updated tree plus a per-node before/after propagation journal (tâtonnement record). CPTs untouched. | `events`, `event_id`, `new_prior` |
| `scenario_recompute_posteriors` | Condition binary polytrees using edge-excluding factor messages in the shared `hkask_forecast::posterior` engine; the graph widget uses the same implementation. Boolean observations and likelihood ratios are applied once. Fractional observed probabilities are fixed marginal constraints fitted within 1e-12; incompatible, numerically unresolved or unsatisfied evidence returns an error, never an estimate. Multiply-connected DAGs retain the forward-only path with an explicit limitation note. | `events`, `evidence` (one of `observed_probability` / `occurred` / `likelihood_ratio` per entry) |
| `scenario_calibrate` | Fermi decomposition → outside view → inside view, without automatic historical-bias adjustment; returns calibrated probability, bounds, and certainty tier. | Fermi sub-questions, base rate |
| `scenario_update` | Bayesian update: P(H\|E) = P(E\|H) × P(H) / P(E); returns posterior and update magnitude. | `prior`, `likelihood`, `evidence_base_rate` |

### Aggregation (2)

| Tool | Description | Key params |
|------|-------------|------------|
| `scenario_synthesize` | Dragonfly-eye synthesis (Tetlock Stage 5): explicit equal-weight aggregation of ≥2 perspectives; unmatched historical loss is not a defensible weight; disagreement score and strongest dissent. | `perspectives` |
| `scenario_cross_validate` | Cross-validate an LLM estimate against a server-computed estimate per sub-question; flags for review when overall divergence exceeds threshold (default 0.15). | two estimate sets |

### Tracking (2)

| Tool | Description | Key params |
|------|-------------|------------|
| `scenario_declare` | Freeze full events/marginals, exact event-ID → RFC 3339 UTC cutoff map and caller-reported resolver. Keys exactly match events; cutoff UTC dates match descriptive deadlines; declared_at is strictly before cutoff. Whole manifest journals atomically; identical same-ID retries are idempotent even after cutoff, changed declarations conflict. Withheld-market scaffolds are ineligible. | `forecast_id`, `events`, `resolution_at`, `resolver` |
| `scenario_score` | Resolve only a timestamped prospective declaration at/after the frozen cutoff, with exact events/resolver; score stored marginals. Duplicate/conflicting outcomes fail; unmatched outcomes are listed and ignored. Old date-only/undeclared history is unscored. Raw loss has no quality bands. | `forecast_id`, `events`, `resolver`, `outcomes` |
| `scenario_calibration` | Calibration curve from stored forecasts (10 probability bins, actual hit rate vs mean forecast), optionally filtered by subject. | `subject` |

### Assessment (2)

| Tool | Description | Key params |
|------|-------------|------------|
| `scenario_assess` | Chermack Phase-5 project assessment anchored on the project record: Preparation from the framing document, event/dependency counts from the stored tree, calibration scoped to the project's subject; caller metrics override the derived values. Unknown project id → not found. | `project_id` + optional overrides |
| `scenario_triage` | Triage a forecasting question (Tetlock Commandment 1): clarity, data availability, resolution criteria → well_specified / goldilocks / needs_refinement. | `question` |

### Independent (1)

| Tool | Description | Key params |
|------|-------------|------------|
| `scenario_status` | Current server state: pipeline overview, calibration curve, cached event tree. | — |

## Key paths

- **Standard pipeline:** `scenario_frame` → `scenario_frame_document` → `scenario_brainstorm` → `scenario_build` → `scenario_quantify` → `scenario_calibrate` → `scenario_synthesize` → `scenario_declare` → `scenario_score` → `scenario_assess`[^tetlock-key-paths]
- **Research entry:** `scenario_build` with research text (skip brainstorming if events are extracted from web text)
- **Valuation bridge (skill layer):** `scenario_quantify` → the caller authors per-node impact mappings and branch returns → the `scenario-planning` skill composes them with the scenario tree; the shared `hkask_forecast` risk core (`scenario_risk_measure`, `fuse_volatility`) is exposed by the scenarios server's `scenario_risk_measure` tool over caller-supplied branch returns. (The former companies-server `scenario_impact_valuation` DCF bridge retired 2026-10-18.)
- **Markets bridge:** `scenario_from_markets_set` (a single market is a set-of-1) → `scenario_quantify`; market records come from `hkask-mcp-prediction-markets` (`market_lookup` / `market_match`)
- **Update loop:** `scenario_propagate` re-propagates a tree after a prior revision; `scenario_update` applies a one-off Bayesian revision; `scenario_recompute_posteriors` recomputes under evidence in both directions (the shared `hkask_forecast::posterior` engine — the graph widget delegates to the same implementation)
- **Independent:** `scenario_triage`, `scenario_status` callable at any point

## Forecast persistence

**Equity admission:** `forecast_persist` freezes the return target, tolerance and
caller-reported horizon resolver. Missing `forecast_probability` remains absent
and unscored, never a default .7. `forecast_record` resolves a stored declaration,
not a return target reconstructed after observing the result. Report raw loss and
eligibility/source limits, not score labels.

`scenario_declare` freezes full events/marginals and named resolver, plus required
`resolution_at: { event_id: RFC3339_UTC_timestamp }`. Keys exactly match event IDs;
each cutoff's UTC date matches its descriptive `deadline`. Admission requires
`declared_at < cutoff`; scoring requires `now >= cutoff`. A resolver name records
a caller-reported source rule, not authenticated evidence.

`ForecastStore` (`src/superforecast/store.rs`) writes the complete declaration
manifest as ONE synced journal batch before publishing any member. Identical
same-ID retries preserve original timestamps/probabilities even after cutoff;
changed event membership/proposition/probability/cutoff/resolver conflicts. Snapshot
failure leaves the complete journal recoverable by identical retry, never a partly
admitted manifest. `scenario_score` only resolves exact frozen events/resolver;
it never creates a forecast at resolution or scores submitted probabilities.
Schema v5 carries precise timestamps. Older date-only/undeclared records stay
provenance-limited unscored history; no retrospective admission. Replays do not
add independent event weight; changed forecasts need new IDs.
Raw binary Brier loss is not calibration, and no automatic historical-bias
correction is applied. Murphy (1973), DOI
`10.1175/1520-0450(1973)012<0595:ANVPOT>2.0.CO;2`, separates reliability,
resolution and uncertainty. Compare references on matched cases/weights/horizons;
report admission, resolution, exclusions and dependence, not universal bands.
Durability ordering — verified by regression (`kask/mcp-servers/hkask-mcp-scenarios/tests/tool_behavior.rs`):

- A failed snapshot publication leaves the journal intact, so reopening
  recovers every acknowledged record exactly once
  (`snapshot_failure_preserves_journal_for_recovery`).
- A journal write failure surfaces as a tool error before the record is
  admitted to memory or published (`journal_failure_is_surfaced_before_memory_changes`).
- A snapshot published with its journal not yet cleared — the crash window
  between publication and truncation — recovers exactly once with the
  journal's last write winning, never the stale snapshot or a duplicate
  (`snapshot_with_uncleared_journal_recovers_exactly_once`).
- Compaction writes a synced same-directory `tempfile` and publishes it by
  atomic rename before the journal is truncated (and directory-synced on
  Unix); `scenario_score` surfaces persistence errors and notes that earlier
  records may already be journaled rather than claiming request rollback.

Not claimed: the store assumes a single writing process (no multi-process
locking), and only `fsync` ordering is verified — no power-loss/crash-consistency
guarantee is tested. The property that a failed replacement leaves the previous
snapshot readable is enforced by construction (temp file + atomic rename; a
failed write cannot touch the published file), but it has no deterministic
failure-injection fixture: the only publication failure that is
privilege-independent — a directory at the destination — cannot coexist with
a prior snapshot file at that path. It is design-reviewed, not test-pinned.

## Project persistence

Scenario projects (the framing document, the last quantified tree, the
assessment history) persist in `ProjectStore`
(`kask/mcp-servers/hkask-mcp-scenarios/src/superforecast/project.rs`):
a single `projects.json` snapshot under the server's data dir
(`{kask_data_dir}/mcp/scenarios/projects.json`), published by atomic
rename per write — the same ordering as the forecast snapshot, with no
journal (projects are low-volume by design).

- `scenario_frame_document` creates/updates the project (id defaults
  to the subject; a re-run updates in place) and persists the framing
  document — Chermack Phase 1 evidence.
- The tree-caching tools (`scenario_quantify`, `scenario_propagate`)
  persist the tree into the project record — the durable tree cache.
  `ProjectStore::last_tree()` is the documented default a restarted server
  reads when its in-memory cache is empty, so the tree survives a server
  restart (pinned by `quantified_tree_survives_a_server_restart`).
- `scenario_assess` is anchored on the project record: an unknown
  project id is `not_found` naming it; event/dependency counts derive
  from the stored tree, Preparation from the framing document, and the
  calibration curve is scoped to the project's subject.
  Caller-supplied metrics override the derived values.

## Testing

`tests/tool_behavior.rs` drives the tool surface through the public
`Parameters<T>` seam; the tool count is pinned against the live router by
`tool_surface_is_exactly_19_registered_tools`. The property layer
(`src/property_tests.rs`, testing-protocol layer 2) verifies the event-tree
marginalization math against two independent oracles: brute-force
marginalization of the enumerated global joint distribution (exact when
parent-independence holds — the generator enforces disjoint-ancestry parent
sets) and a test-local re-implementation of the documented noisy-OR rule for
multi-group trees. The parent-independence approximation itself (marginals
computed over parent *marginals*, per `types.rs` — exact only under
disjoint ancestries) is pinned as intentional by
`correlated_parents_keep_the_documented_independence_approximation`.

## Formal specification

The event-tree contract is machine-checked in core Lean (4.34.0, no
Mathlib) at
[`kask/lean/event_tree_marginalization.lean`](../../../lean/event_tree_marginalization.lean):
eight theorems over an abstract `OrdField` — CPT completeness and
soundness (a marginalizing table has exactly 2^|parents| entries, and
every 2^|parents|-entry table marginalizes), the bitmap order for one
and two parents, probability bounds (parents and entries in [0,1] →
marginal in [0,1]), mass conservation (the all-ones table marginalizes
to 1 whatever the parents), and noisy-OR exactness and bounds.
`lean_check` exit 0, no `sorryAx`; `noisyOr_single` depends on no
axioms. Stated assumptions: the `OrdField` class fields (the theorems
are parametric), parent independence (the same factorization the Rust
makes — its approximation character under shared ancestry is pinned
by `correlated_parents_keep_the_documented_independence_approximation`),
exact arithmetic (the f64 clamp is not modeled), and `none` = the
server's validation rejection. The property layer pins the f64
implementation against two independent oracles; the Lean pin proves
the contract for every ordered field — contract proven, implementation
property-tested.

## Redesign record (2026-10-06/07)

The server was reviewed from scratch against its four reference models
(MAIA "Time Horizons", Schwartz 1991, Tetlock & Gardner 2015, Chermack
2011) and rebuilt per a 16-proposal set. The review artifacts (plan,
findings, proposals, improvement plan) were consolidated into this
reference on 2026-10-07 and live in git history
(`8692a23520`..`e677993804`).

**Decisions with standing force:**

- **`scenario_full` removed** (operator decision, 2026-10-06): the
  one-call Tetlock batch had zero skill callers and its inline
  assessment stage was a pass-through of caller metrics; the staged
  pipeline is the only path (surface pin 20 → 19).
- **Portfolio event-exposure report deferred** (operator decision,
  2026-10-06): no consumer demands it; the recorded scenario join on
  equity forecasts (`scenario_project_id`, `scenario_tree`,
  `fused_volatility`) makes it an M effort when demand materializes.
- **Narratives and indicators live in the skill layer** (ruled): the
  server stores the project record, trees, and assessments; Schwartz's
  narrative phases are the `scenario-planning` skill's templates.

**Accepted limitations (ruled, watched):**

- Empty bins have null observed and expected rates. `signed_probability_error`
  is descriptive p−outcome, with sample_count/qualifying_bins/bin_min_count
  metadata (full wire fields use the `signed_probability_error_` prefix); no
  qualifying bins means null/insufficient. Signed cancellation or complementing
  an event cannot establish confidence or calibration.
- `scenario_declare` journals prospective records; `scenario_score` snapshots matching resolution updates — a
  minor durability-ordering cost, accepted.
- `hkask_forecast::marginalize` zero-fills missing table entries while
  the server's validation rejects wrong lengths first — the lenient
  path is shielded; a future caller that skips validation inherits
  silent zero-fill (documented seam).
- `compute_marginal_probabilities` defaults a missing parent to 0.0
  with a warn — unreachable through the public path (the topological
  sort rejects unknown parents first); watched.
- Ontology anchoring is coarse (protocol tools → `pko:PROCEDURE`,
  everything else → `dc_bibo:DATASET`) — functional.

**Commit ledger:** `8692a23520` (review + Slices 1–3) → `64b2f4f6d5`
(Slices 4–6: shared posterior engine, marginal scoring, scenario
provenance) → `c03cf4aa89` (PR-10 workbook presentation) →
`b252d88b3d` (post-restart follow-ups) → `1a8c500c12` (PR-13 removal,
pin 20 → 19) → `e677993804` (closeout receipts). Live receipts: the
19-tool surface verified post-restart (`list_mcp_tools`); 39 scenarios
tests green; scoped clippy + machete clean. The execution goal
(`c84339e0`) closed through the batch-record path — its durable record
is this section plus the commits (the curator-memory batch record
`kanban:goal-retention`/`batch_prune_2026-10-07` cites this location).

## Cross-links

- [Prediction Markets MCP Server Reference](prediction-markets.md) — market records consumed by `scenario_from_markets_set`
- [The Forecasting Stack: Three-Layer Architecture](README.md#the-forecasting-stack-three-layer-architecture) — three-layer model (skill, math, servers)
- [MCP Server Registry](README.md) — built-in server index
- [Diagram Index](../../DIAGRAMS_INDEX.md) — DIAG-RF-005 registration

## Footnotes

[^tetlock-scenarios-ref]: Tetlock, P. E., & Gardner, D. (2015). *Superforecasting: The Art and Science of Prediction*. Crown Publishers.
    Cited for the calibration pipeline (Fermi decomposition, Bayesian update, Brier scoring, dragonfly-eye synthesis) the diagram traces.

[^schwartz-scenarios-ref]: Schwartz, P. (1991). *The Art of the Long View*. Doubleday.
    Cited for the scenario-framing and brainstorming phases the pipeline starts with.

[^tetlock-key-paths]: Tetlock, P. E., & Gardner, D. (2015). *Superforecasting: The Art and Science of Prediction*. Crown Publishers.
    Cited for the record → score → assess sequence the standard pipeline follows.
