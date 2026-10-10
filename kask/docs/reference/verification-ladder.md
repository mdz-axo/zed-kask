---
title: "The Verification Ladder: Five Rungs and Their zed-kask Homes"
audience: [developers, architects, agents, operators]
last_updated: 2026-10-10
version: "1.0.0"
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
repository, trust wave `62c434f6..1468f18b` — the 56-commit wave the swarm
grounding gate documents as its port source,
`kask/mcp-servers/hkask-mcp-swarm/src/grounding.rs:1-5`).
zed-kask absorbed the rungs as code ports with fermi-citing doc comments; this
document is the one place the whole ladder is presented, so a gap in any rung
is findable.

## The five rungs

| Rung | Question | zed-kask home | Clock | Catches |
|---|---|---|---|---|
| 1. Presence | Does the declared thing exist? | `port_registry.rs` (an `accepts`/`produces` label that resolves to no registered type is rejected at admission); `schema_validate.rs:1-24` (no schema or an unsupported keyword is `unverified_*`, never `valid`); the grounding gate's no-contract arm (`grounding.rs:443-450` — `unchecked`, not `clean`) | admission, then per invocation | a card declaring a type that does not exist; a contract nobody wrote |
| 2. Liveness | Does the writer ever run? | `hkask-regulation/src/loops/core.rs:325` (`LivenessTrust`: `Live`/`Stale`/`NeverRun`), `:342` (`Reading`), `:387` (`LoopView`), `:436` (`compute_reading`) | per regulation tick | a loop that reports success while having never run — `WiringClosed`, the dominant failure mode |
| 3. Truth | Does the stored value equal its source of truth? | the `grounding-verify` skill's Step 3 mechanical re-derivation (`.agents/skills/grounding-verify/SKILL.md` — `lisp_eval` re-derives every computed number against its inputs); `core.rs:305` (`OutcomeTrust`: impact verified / untrusted / unverified); resolved-outcome Brier scoring (`forecast_record`, `market_calibration`, `kanban_goal_score`) | per claim at verification; at resolution for forecasts | a derived number that does not re-derive; a prediction that disagrees with the resolved outcome |
| 4. Grounding | Could this value have come from any available tool? | `grounding.rs:105` (`grade` — a `sourced` field with no tool call in the run is stripped; the run is amended, not refused) + the `grounding-verify` skill's provenance lattice | per delegation; per verification run | a fabricated measurement — a value no tool of the agent's could have supplied |
| 5. Binding | Does the invocation match the declared interface? | `local_runtime.rs:586` (`contract_checks` — `input_contract_check` / `output_contract_check` / `bind_matched` on every delegation result); `local_registry.rs:46` (`validate_typing` at admission); `contract.rs` (the composition contract, fermi's `agent_contract.rs` port) | per request | prose sent to a structured-only port; a document that contradicts its declared type |

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

## The reliance envelope

The grounding gate's per-delegation verdicts surface to callers as one token,
worst-first (`grounding.rs:55`): `unusable` → `malformed` → `amended` →
`incomplete` → `unchecked` → `clean`. The token is *derived* from the
grounding, completeness and schema verdicts, never recomputed — one producer
per verdict, so the envelope cannot drift from the checks beneath it.
`unchecked` (no contract applied) is deliberately not `clean`: counting
silence as consent would report a gate that never engaged as a control that
never needed to fire.

## Falsifiability

Every rung must demonstrate it can fail. fermi enforces this with a
falsification registry (each check broken by hand and watched fail); zed-kask
pins it in tests: the grounding gate's worst-first precedence and
closed-vocabulary pins (`grounding.rs:657-673`, `:743-749`), the `LoopView`
composition pins (`core.rs:693-712`), and the skill's Step 6 floor form, which
errors loudly on a graded claim missing its strength (verified live 2026-10-10:
a mixed set floors over graded claims only; an all-pending set returns null —
unknown, not clean).

## Known gaps (open)

Named honestly so they stay findable — each is a fermi structure not yet
ported:

- **Gate counters and readings** (fermi `gate_trust`/`gate_api`): the
  grounding gate counts nothing per agent or contract. A card whose grounding
  map mis-declares every field strips 100% of that agent's output and the gate
  reads as working; nothing distinguishes a contract bug from a misbehaving
  agent at the aggregate level.
- **Gate-decision review** (fermi `gate_review.rs`): no recorded after-the-fact
  human review of gate decisions. A wrong strip or flag leaves no reviewable
  ledger entry; the algedonic review covers skill output, not gate decisions.
- **Memory provenance tiers** (fermi extraction ceiling applied to curator
  memory): h_mems carry evidence citation and confidence but no provenance
  tier — a model-synthesized memory and a tool-observed one retrieve with
  equal standing, and no write-path check routes contradictions.
- **Dispatch provenance signals** (fermi `stamp_invocation`): dispatch records
  carry no route-reason stamps, so swarm C6 blame cannot separate "the wrong
  question was sent" from "the agent is bad at the job".
