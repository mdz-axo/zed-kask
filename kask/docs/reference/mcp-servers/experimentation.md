---
title: "Experimentation MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-10-19
version: "0.44.0"
status: "Active"
domain: "Composition"
mds_categories: [domain, composition, lifecycle, trust]
---

# Experimentation MCP Server Reference

**Crate:** `kask/mcp-servers/hkask-mcp-experimentation`
**Tools:** 8 — `experiment_propose`, `variant_register`, `experiment_seal`, `fitness_record`, `final_evaluation_record`, `selection_record`, `lineage_read`, `population_query`
**Auto-start:** Yes by default with the full built-in set; `kask.mcp.load_default=false` disables the fleet and `kask.mcp.overrides.experimentation=false` disables this server (server id `experimentation`, `kask/crates/kask_bridge/src/mcp_servers.rs`).

The experimentation server is the **experiment registry** for the sharded
experimentation program (the repair plan §P8, extended by the
agentic-scientist-v3 integration, 2026-10-15): the durable record of declared
experiments, their variant lineages with roles, sealed eval contracts, fitness
records, final evaluations, and selection verdicts. It is a registry, not an
executor — fitness entries are recorded report references; the caller asserts
they name real harness runs (the registry stores the declaration, it does not
authenticate the reports).

## The protocol

1. **Declare** — `experiment_propose`: hypothesis, artifact layer (skill |
   agent_card | prompt_template | tool_schema | regulation_scalar |
   scientific_hypothesis — the general lab notebook, operator
   ruling D2 2026-10-15), genotype refs, eval set, fitness function, noise
   band, **a typed rubric bundle** (P1: non-empty; at least one `metric`
   rubric and one `implementation` or `grounded_report` rubric; stable
   snake_case ids; verification in the swarm evaluator vocabulary —
   contains / not_contains / regex / report_ref), a pre-registered
   prediction with confidence in [0,1], and an energy budget carrying
   `max_runs`. Optional `design_spec` carries a typed, versioned scientific
   DesignSpec — a JSON object with a non-empty `spec_version` string
   (`types.rs` `validate_design_spec`) — for a `scientific_hypothesis`
   declaration: it is stored, exposed through lineage/population reads, and
   joins the seal fingerprint (a post-seal design edit yields a different
   fingerprint rather than silently reusing a stale design); omit it for a
   legacy artifact experiment. Optional `linked_goal_id` joins the
   experiment to a verification goal; its optional forecast is a separate declaration. Optional
   `experiment_key` converges retried calls.
2. **Vary** — `variant_register`: genotype configurations with an optional
   **role** (P4: baseline | challenger | ablation — exactly one baseline per
   experiment; an ablation names its parent variant) and optional parent
   lineage. The first variant moves the experiment to running.
3. **Seal** — `experiment_seal` (P2): freezes the declaration's (eval_set,
   rubrics, fitness_fn, noise_band, design_spec) under a SHA-256 fingerprint
   and records the headroom attestation — the baseline variant, its recorded
   fitness evidence, and **exactly one of** a measured pass rate
   (`baseline_pass_rate`, must be < 1.0; saturation is a typed no-headroom
   refusal that voids via `selection_record`, §5 Q6) **or** a tagged
   `scientific_headroom` criterion `{design_family, criterion, evidence}`
   for a design family with no meaningful binary baseline pass rate
   (single-arm calibration, observational estimation, equivalence; non-empty
   `design_family`/`criterion`, `types.rs`
   `validate_scientific_headroom`). Supplying both or neither is a typed
   refusal. Pre-seal, only the baseline records fitness; post-seal, every
   fitness record cites the seal. An identical retried seal converges; a
   different attempt is a typed refusal.
4. **Test** — `fitness_record`: fitness — caller-asserted report references plus
   run_count (summed against `budget.max_runs`, typed `budget_exhausted`
   refusal past the ceiling) and per-objective scores. Immutable once
   written.
5. **Final evaluation** — `final_evaluation_record` (P3): the leading
   variant re-scored on the selection set. Run refs must be disjoint from
   every prior fitness record and final evaluation for the experiment (the
   GEPA feedback/selection split made mechanical); run_count joins the
   budget ceiling atomically.
6. **Select** — `selection_record`: **exactly one of** a legacy `verdict` —
   selected (naming the winning variant) or rejected (with at least one
   reason) — **or** a typed `scientific_outcome` for a scientific design:
   `rejected | corroborated | evidence_of_practical_equivalence |
   inconclusive | measurement_void | design_void`. A scientific outcome is
   stored as itself, never renamed selected/rejected. Only the two genuinely
   binary outcomes (`rejected`, `corroborated`) enter Brier calibration; the
   bounded and void outcomes are excluded on the typed field. Supplying both
   or neither is a typed refusal. Both outcome classes persist as fossils;
   the first selection resolves the experiment. `algedonic_reference` names
   the review record that chaired the selection.

Readback: `lineage_read` returns the ancestry chain (variant kind) or the
full experiment record (experiment kind — variants, fitness, selections, the
seal, and final evaluations). `population_query` is the registry's population
view for the algedonic agenda and the curator's ORIENT.

## The expectation signal

The `Prediction` type is a pre-registered claim with the experiment's own
confidence — "the tiny controller (§P8.1)". The claim is scored against the
experiment's eligible binary measured outcome in its own registry cohort. A linked kanban goal records verification progress, never automatic human closure or duplicate independent forecast weight. The seal adds a
second expectation carriage: the eval contract is fingerprinted before the
challengers run, so the measurement that resolves the claim is provably the
measurement that was declared.

## Architecture

- **Store**: `ExperimentationStore` over the `hkask_storage` database driver
  trait — SQLCipher in production, the in-memory driver in tests. One
  database per server (`HKASK_EXPERIMENTATION_DB` overrides the per-agent
  default). Tables: experiments (with rubrics, max_runs, linked_goal_id),
  variants (with role), fitness_records (with run_count, seal_id), seals
  (UNIQUE experiment_id — one seal per experiment), final_evaluations,
  selection_records. The full schema is declared at creation — there is no migration
  surface (the pre-contract accommodations were removed 2026-10-15 under the
  operator's no-backward-compatibility ruling; the pre-contract registry database
  at the per-agent default — holding the E-cycle rows — was deleted 2026-10-16
  under the same ruling, and a fresh full-schema registry is created on the next
  server open).
- **Seal fingerprint**: SHA-256 (workspace `sha2`) over the canonical JSON of
  (eval_set, fitness_fn, noise_band, rubrics, design_spec) — identical
  declarations seal to identical fingerprints; the headroom arm
  (`baseline_pass_rate` / `scientific_headroom`) round-trips through TEXT
  (Rust's shortest-round-trip f64 Display).
- **Replay convergence**: `experiment_propose` and `variant_register` take
  optional caller keys (UNIQUE + INSERT OR IGNORE); `experiment_seal`
  converges structurally — UNIQUE(experiment_id) plus an args-equality check
  (identical retries return the existing seal; different attempts are typed
  `AlreadySealed` refusals).
- **Errors**: `ExperimentationError` classified per-variant for MCP dispatch
  via `map_experimentation_error` — never a blanket internal. Seal-state
  errors (not sealed / already sealed / citation missing / no headroom) are
  failed_precondition; wrong ids and non-disjoint refs are invalid_argument.
- **Credentials**: `HKASK_DB_PASSPHRASE` declared required — the canonical
  2-tier chain; startup failures map to Infrastructure.

## Testing

The tool count is pinned by `tool_surface_is_exactly_8_registered_tools`
(build-generated TOOL_NAMES against the live router); the tool-behavior
suite drives all eight tools through their `Parameters` seams (rubric
validation, role enforcement, the sealed lifecycle, disjointness, budget
boundaries, replay convergence, the credential pin); the store suite drives
the full registry lifecycle on the in-memory driver, including the seal
fingerprint determinism and the concurrent-selection race.

## Reference models

The repair plan §P8 and the experimentation reference model
(`kask/docs/research/experimentation-reference-model.md` — design authority);
the agentic-scientist-v3 integration (landed 2026-10-15 — the D1–D4 operator
rulings are recorded in the derived ontology registry,
`kask/crates/hkask-bridge-ontology/src/derived.rs`; the proposal record
lives in git history). The loop-register's L24 row
classifies the registry cycle (primary Layer B, secondary C; INV2 held by
design).
