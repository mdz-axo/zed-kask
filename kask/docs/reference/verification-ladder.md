---
title: "The Verification Ladder: Five Rungs and Their zed-kask Homes"
audience: [developers, architects, agents, operators]
last_updated: 2026-10-28
version: "1.1.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [trust, composition, domain]
---

# The Verification Ladder

"Is this correct?" is not one question. It is five, ordered by difficulty, and
**a check answering an easier one will pass while a harder one fails.** Naming
them separately is what stops a presence check being mistaken for a truth
check. The five form a ladder, not a score: no rung is blended into another,
each is a separate contract, and where several readings apply the worst
available one is what a caller branches on.

Design source: fermi's `docs/papers/verification_for_agent_ecologies.md` (fermi
repository, trust wave `62c434f6..1468f18b`).
zed-kask absorbed the rungs as code ports with fermi-citing doc comments; this
document is the one place the whole ladder is presented, so a gap in any rung
is findable.

## The five rungs

| Rung | Question | zed-kask home | Clock | Catches |
|---|---|---|---|---|
| 1. Presence | Does the declared thing exist? | `tool_surface_pin!` (every built-in server pins its exact tool count and generated tool-name set against the live router, so a declared tool that does not exist fails CI instead of degrading to tool-not-found; `kask/scripts/check-mcp-registration.sh` gates on pin presence); the former admission homes (`port_registry.rs`, `schema_validate.rs`) were removed 2026-10-28 with the whole local swarm capability (DIVERGENCE.md retired D33) | CI pin suite, then per dispatch | a card declaring a type that does not exist; a contract nobody wrote |
| 2. Liveness | Does the writer ever run? | `hkask-regulation/src/loops/core.rs:325` (`LivenessTrust`: `Live`/`Stale`/`NeverRun`), `:342` (`Reading`), `:387` (`LoopView`), `:436` (`compute_reading`) | per regulation tick | a loop that reports success while having never run — `WiringClosed`, the dominant failure mode |
| 3. Truth | Does the stored value equal its source of truth? | the `grounding-verify` skill's Step 3 mechanical re-derivation (`.agents/skills/grounding-verify/SKILL.md` — `lisp_eval` re-derives every computed number against its inputs); `core.rs:305` (`OutcomeTrust`: impact verified / untrusted / unverified); source-backed outcome resolution (`forecast_record`, market observations, native human goal closure); Brier checks loss arithmetic, not source truth | per claim at verification; at resolution for forecasts | a derived number that does not re-derive; an outcome label unsupported by its declared source (a probabilistic forecast can legitimately lose on one event) |
| 4. Grounding | Could this value have come from any available tool? | the `grounding-verify` skill's provenance lattice (verification-time, skill-applied: `tool_verified` requires a mechanical match against retained source output, never mere tool-call presence — the swarm server's per-delegation grounding gate that promoted any tool call to `tool_verified` was removed 2026-10-21) | per verification run | a fabricated measurement — a value no tool of the agent's could have supplied |
| 5. Binding | Does the invocation match the declared interface? | **not yet enforced** — the former enforcement sites (the swarm server's `contract_checks` / `validate_typing` / `contract.rs`) were removed 2026-10-28 with the whole local swarm capability (DIVERGENCE.md retired D33); no equivalent binding gate currently exists, so this rung is applied as a review criterion, not a machine check | per request | prose sent to a structured-only port; a document that contradicts its declared type |

## The two ladder properties

**Each rung is invisible to the one below it.** A grounding failure is a valid
value of a present column; a binding failure is a well-grounded document that
never matched the interface. That is why the rungs are separate contracts, not
one validation layer — a single layer reasons at one level of abstraction and
silently declines to ask the others.

**Each rung costs more and runs less often.** Presence is an admission-time
registry read; liveness is a per-tick count; truth is a re-derivation against
retained bytes; grounding is a per-delegation gate; binding is a per-request
check. The spread of clocks is what makes running all five affordable — no
rung is asked to carry a heavier schedule than its cost warrants.

## The provenance lattice (inside the grounding rung)

The `grounding-verify` skill classifies every factual claim on a
strength-ordinal lattice — the grounding rung's internal structure:

| Strength | Provenance value | Meaning |
|---|---|---|
| 2 | `tool_verified` | Value found in source output via mechanical match |
| 2 | `platform_derived` | Value computed by `lisp_eval` from sourced values |
| 1 | `model_inference` | LLM synthesized from source outputs — the extraction ceiling |
| 0 | `unavailable` | No source the pipeline called can supply this claim |
| 0 | `tool_no_match` | Source was consulted and had nothing for this subject |
| 0 | `pending_check` | Check exists but has not run yet — not graded: counted in `pending_count`, excluded from the floor |
| 0 | `rejected` | Checked and found wrong |

Two rules govern it:

- **Floor:** a report is only as strong as its weakest *graded* claim.
  `pending_check` claims are counted, never floored — "not yet checked" is
  not "checked and wrong" (the fermi unknown rule: an ungradeable source
  poisons a result only when it could still move the floor). When pending
  claims exist and the floor is above 0, they could still lower it to 0, and
  the report says so; a floor already at 0 is unchanged by pending.
- **Ceiling:** extraction can never exceed inference. A claim the LLM
  synthesizes from tool outputs is `model_inference` permanently — judgement
  does not inherit retrieval, and no reviewer's agreement raises a tier.

## Falsifiability

Every rung must demonstrate it can fail. fermi enforces this with a
falsification registry (each check broken by hand and watched fail); zed-kask
pins it in tests: the `LoopView`
composition pins (`kask/crates/hkask-regulation/src/loops/core/tests.rs:149`), and the skill's Step 6 floor form, which
errors loudly on a graded claim missing its strength (verified live 2026-10-10:
a mixed set floors over graded claims only; an all-pending set returns null —
unknown, not clean).

## Known gaps (open)

Named honestly so they stay findable — each is a fermi structure not yet
ported:

- **Gate-decision review** (fermi `gate_review.rs`): moot since 2026-10-21 —
  the per-delegation grounding gate whose decisions would be reviewed was
  removed (it promoted mere tool-call presence to `tool_verified`, and
  unrelated or failed tool calls passed it). Rung 4 continues at the
  verification-time skill lattice, where a wrong finding surfaces the way
  every other finding does: as a review-board card with the evidence
  pasted in.
- **Memory provenance tiers — CLOSED 2026-10-10 (R5).** h_mems now carry a
  `provenance` tier (`tool_observed` | `model_inference` — the extraction
  ceiling: a model-synthesized memory never retrieves as tool-observed),
  stamped at every write path (`memory_insert` stamps `model_inference`
  unclaimably; goal-event ingestion stamps `tool_observed`; turn chunks and
  distilled lessons default `model_inference`), surfaced in
  `curator_memory_recall` / `curator_semantic_search`, and backed by a
  write-path contradiction check (`memory_insert` refuses a differing value
  for an existing key, surfaced by id and routed to
  `memory_resolve_contradiction` / `memory_update`). Residual, disclosed:
  the other active servers' stores (kata-kanban, corpus) share the
  schema and default to `model_inference` — fail-safe (never overstates),
  to be stamped in their own changes if their recall surfaces ever
  surface provenance.
- **Dispatch provenance signals — CLOSED 2026-10-10 (R6); REMOVED 2026-10-28.**
  The `DispatchStamp` provenance machinery (stamped by every swarm dispatch
  surface — `direct`, `thread`, `fanout`, `pipeline`, `workflow`, `plan`,
  `eval`, `a2a`) and the C6 routing-vs-agent blame split lived entirely in
  the removed swarm server (`local_runtime.rs`); the capability retirement
  deleted them with it (DIVERGENCE.md retired D33). The record stands as
  the design's audit history; no dispatch provenance signals are
  currently stamped.
