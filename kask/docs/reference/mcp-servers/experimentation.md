---
title: "Experimentation MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-11-12
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
   prompt_template | tool_schema | regulation_scalar |
   scientific_hypothesis — the general lab notebook, operator
   ruling D2 2026-10-15; five layers, `types.rs` `LAYERS`), genotype refs, eval set, fitness function, noise
   band, **a typed rubric bundle** (P1: non-empty; at least one `metric`
   rubric and one `implementation` or `grounded_report` rubric; stable
   snake_case ids; verification in the deterministic evaluator vocabulary —
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
   `experiment_key` converges retried calls. An optional id or key is
   omitted or meaningful: a blank string is refused. The declaration is
   immutable — no tool edits it.
2. **Vary** — `variant_register`: genotype configurations with an optional
   **role** (P4: baseline | challenger | ablation — exactly one baseline per
   experiment; an ablation names its parent variant) and optional parent
   lineage. The first variant moves the experiment to running. A retry under
   the same `variant_key` returns the variant the first call registered, also
   when that variant is the baseline or the experiment has resolved since.
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
   different attempt is a typed refusal. The fingerprint covers the five
   fields exactly as declared, object key order included: the same eval set
   with its keys in another order seals to a different fingerprint.
4. **Test** — `fitness_record`: fitness — caller-asserted report references plus
   run_count (summed against `budget.max_runs`, typed `budget_exhausted`
   refusal past the ceiling) and per-objective scores. Immutable once
   written. A run ref a final evaluation already used is refused.
5. **Final evaluation** — `final_evaluation_record` (P3): the leading
   variant re-scored on the selection set. Run refs must be disjoint from
   every prior fitness record and final evaluation for the experiment (the
   GEPA feedback/selection split made mechanical) and must not repeat
   within the call; a later fitness record may not reuse one. The server
   compares the ref strings only — whether two refs name disjoint data is
   the caller's assertion. run_count joins the budget ceiling in the same
   write transaction as the insert.
6. **Select** — `selection_record`: **exactly one of** a legacy `verdict` —
   selected (naming the winning variant) or rejected (with at least one
   reason) — **or** a typed `scientific_outcome` for a scientific design:
   `rejected | corroborated | evidence_of_practical_equivalence |
   inconclusive | measurement_void | design_void`. A scientific outcome is
   stored as itself, never renamed selected/rejected, and an outcome word in
   the `verdict` field is refused. Supplying both or neither is a typed
   refusal. A named `selected_variant_id` must be a variant of the
   experiment under every resolution. Both outcome classes persist as
   fossils; the first selection resolves the experiment.
   `algedonic_reference` names the review record that chaired the selection.
   The server stores the resolution and computes no score from it.

Readback: `lineage_read` takes an experiment id or a variant id and returns
the experiment, its seal, fitness records, final evaluations and selections;
with an experiment id also all variants, with a variant id that variant's
ancestry chain. `population_query` lists full declarations, newest first
(50 by default, at most 200) — the registry's population view for the
algedonic agenda and the curator's ORIENT.

Every tool refuses an unknown parameter, and the closed vocabularies (layer,
role, verdict, scientific outcome, rubric kind, status) are enumerated in
the input schemas from the constants the handlers check.

## The expectation signal

The `Prediction` type is a pre-registered claim with the experiment's own
confidence — "the tiny controller (§P8.1)". The registry stores the claim,
the confidence and the resolution; it does not score them. The skills that
read the registry compute the per-prediction Brier themselves, through
`lisp_eval` (`eureka-cycle` EUR-2, `self-improvement` SI-3). A linked kanban goal records verification progress, never automatic human closure or duplicate independent forecast weight. The seal adds a
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
  selection_records. The full schema is declared at creation. One guarded
  additive migration runs at open (`store.rs`
  `migrate_scientific_design_columns`, 2026-10-19): it adds
  `experiments.design_spec` and `selection_records.scientific_outcome` and
  rebuilds `seals` in one transaction so `baseline_pass_rate` may be NULL,
  keeping every existing row. The earlier pre-contract accommodations were
  removed 2026-10-15 under the operator's no-backward-compatibility ruling.
- **Write transactions**: each of `variant_register`, `fitness_record`,
  `final_evaluation_record` and `selection_record` re-reads what its insert
  depends on (the experiment is open, one baseline, the seal state, run-ref
  overlap, the run budget) inside the same immediate write transaction as
  the insert, so two concurrent calls cannot both pass a check the other
  invalidates.
- **Seal fingerprint**: SHA-256 (workspace `sha2`) over the JSON text of
  (eval_set, fitness_fn, noise_band, rubrics, design_spec) in that order,
  each object in its declared key order (the workspace builds `serde_json`
  with `preserve_order`; the text is not key-sorted) — identical
  declarations seal to identical fingerprints, and the computation is pinned
  to fixed digests by `seal_fingerprint_computation_is_pinned`; the headroom arm
  (`baseline_pass_rate` / `scientific_headroom`) round-trips through TEXT
  (Rust's shortest-round-trip f64 Display).
- **Replay convergence**: `experiment_propose` and `variant_register` take
  optional caller keys (UNIQUE + INSERT OR IGNORE); `experiment_seal`
  converges structurally — UNIQUE(experiment_id) plus an args-equality check
  (identical retries return the existing seal; different attempts are typed
  `AlreadySealed` refusals).
- **Errors**: `ExperimentationError` classified per-variant for MCP dispatch
  via `map_experimentation_error` — never a blanket internal. An unknown
  experiment, variant or lineage reference is not_found. State refusals (a
  resolved experiment, a second baseline, budget exhausted, not sealed /
  already sealed / citation missing / no headroom / baseline not measured)
  are failed_precondition. Malformed input — a value outside a closed set,
  both or neither headroom form, a blank optional, overlapping run refs, a
  key reused for a different payload — is invalid_argument. A request that
  fails to deserialize (a wrong type, a missing or unknown parameter) is
  refused by the rmcp layer before the handler runs and carries no kind on
  the wire.
- **Health snapshot**: `ExperimentationStore::health_snapshot` returns, per
  experiment, its status, creation time, run ceiling and recorded runs
  (fitness records plus final evaluations — the sum the budget refusal
  uses). Its one reader is the editor's cybernetics loop
  (`kask_bridge` `experimentation_health_bridge.rs`, seam D87), through
  `stuck_running`.
- **Credentials**: `HKASK_DB_PASSPHRASE` declared required — the canonical
  2-tier chain; startup failures map to Infrastructure.

## Testing

The tool count is pinned by `tool_surface_is_exactly_8_registered_tools`
(build-generated TOOL_NAMES against the live router); the tool-behavior
suite drives all eight tools through their `Parameters` seams (rubric
validation, role enforcement, the sealed lifecycle, disjointness, budget
boundaries, replay convergence, the credential pin), and its 2026-11-12 wire
pins build each request from JSON the way a tool call arrives (unknown
parameters, blank optionals, keyed retries, held-out refs in both
directions, `created_since` across UTC offsets, the pinned fingerprint
digests, error kinds); the lib suite pins the live schemas (closed inputs,
enumerated vocabularies, described parameters, descriptions that lead with a
complete sentence); the store suite drives the full registry lifecycle on
the in-memory driver and three races on a file-backed pool (concurrent
selections, baseline registrations and final evaluations on one run ref).

## Reference models

The repair plan §P8 and the experimentation reference model
(`kask/docs/research/experimentation-reference-model.md` — design authority);
the agentic-scientist-v3 integration (landed 2026-10-15 — the D1–D4 operator
rulings are recorded in the derived ontology registry,
`kask/crates/hkask-bridge-ontology/src/derived.rs`; the proposal record
lives in git history). The loop-register's L24 row
classifies the registry cycle (primary Layer B, secondary C; INV2 held by
design).
