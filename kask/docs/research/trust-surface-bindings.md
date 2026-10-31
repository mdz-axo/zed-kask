---
title: "Trust Surface Bindings — Design Document"
audience: [architects, agents]
last_updated: 2026-10-31
version: "0.1.0"
status: "Design proposal — no code changes in this phase; implementation is the operator-approved pass"
domain: "Trust"
mds_categories: [domain, trust, composition, lifecycle]
---

# Trust Surface Bindings — Design Document (Part III)

Requirement served: the trust model (`trust-economics-and-model.md`, Part II)
must meet the agent at every decision surface — a system-prompt sentence alone
is a promise the rest of the system does not keep. This document designs each
binding with its exact code seam, sensor, attenuator/amplifier, and D/P split.
**No code changes in this phase.** System-prompt and Regulation edits are
operator-gated: proposed here, never landed unilaterally.

## The substrate: the expectation-ledger module

Everything below reads one module. Deep-module design (Ousterhout via
`deep-module`): one core operation — record what the user expected and what
happened; everything else derives.

```rust
// Proposed interface (7 public functions — at the cap; implementation hidden)
pub fn record_instruction(&self, text: &str, source: InteractionId,
                          weight_hint: Weight) -> Result<LedgerEventId, LedgerError>;
pub fn record_outcome(&self, id: LedgerEventId, outcome: Outcome,
                      oracle_note: Option<OracleNote>) -> Result<(), LedgerError>;
pub fn trust_state(&self, window: Window) -> Result<TrustState, LedgerError>;
pub fn price(&self, violations: &[LedgerEventId], stake: Stake) -> Result<Cost, LedgerError>;
pub fn window_stats(&self, window: Window) -> Result<WindowStats, LedgerError>;
pub fn delegation_radius(&self) -> Result<Radius, LedgerError>;
pub fn steer(&self, goal: &GoalRef, actions: &[ActionRef], window: Window)
    -> Result<Vec<SteerProjection>, LedgerError>;   // operator ruling 2026-10-31
// one error enum, one config struct (ratified parameters) — never leaked
```

`steer` is the operator's steering-algorithm ruling made concrete: per
candidate action it projects `{path: open|cliff, P_goal, ΔT, note}` — the
goal gradient with the deception/omission cliff visible (`P_goal = 0.00,
path: cliff`). Proven properties are Lean-pinned in
`kask/lean/trust_model_invariants.lean` (the cliff set and the dominance
theorems). Every projection with a concrete `P_goal` is persisted as a
forecastable row; resolution (goal-judge and verification outcomes) and
Brier scoring use the platform's existing machinery.

Hidden behind the interface: the T fold and recovery-debt state machine (Part
II), the parameter set (operator-ratified g, l, ρ, κ, T_0, W), and storage.
`LedgerError::StoreUnavailable` must exist and must NOT serialize as an empty
ledger — the standing trap (no `unwrap_or(0)` on sense inputs) is enforced at
the module boundary so no consumer can accidentally commit it.

Storage decision for the implementation pass: events as episodic h_mems in the
curator store (entity `trust:ledger:<id>`) makes binding 5 near-free — the
`memory_insert` evidence gate already requires citing an existing h_mem, and
the ledger event IS one. The constraint to honor: one store, three readers
(agent, regulation loop, curator server), no silent fallbacks — the
`regulation_store()` accessor pattern
(`kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:862`) shows the
windowed-query shape already in production.

## Binding 1 — System prompt (operator-gated)

- **Seam:** `crates/agent/src/thread.rs:6289` `fn render_system_prompt` — the
  digest-keyed cached path ("When the digest changes, the new render is cached
  and a telemetry event is emitted so cache-bust events are observable"), called
  from `build_request_messages_until` (`crates/agent/src/thread.rs:6268`). The
  clause renders through this path only — no bypass of the byte-stability
  cache discipline.
- **Clause content (draft):** the pricing statement — total cost = task cost +
  trust cost; explicit instructions are priced at κ·w·S; when honoring is
  expensive, renegotiate openly, never decide it away silently; the ledger
  records kept and violated expectations; verification must disclose
  sidestepping.
- **Sensor:** none at render time — the clause is the policy statement; the
  sensor is the ledger downstream.
- **Amplifier:** primed at every turn, byte-stable per digest; a clause change
  is an observable cache-bust event.
- **D/P:** D — the rendered clause text and the deterministic render; P — its
  application in decisions (gated by the ledger's D-recorded facts).

## Binding 2 — Regulation loop sense input (operator-gated)

- **Seams:** `kask/crates/hkask-regulation/src/cybernetics_loop/cycle.rs:334`
  `pub(super) async fn sense()` ("Produces signals for: per-agent energy ratio,
  variety deficit, queue depth" — the extension point);
  `kask/crates/hkask-regulation/src/loops/signals.rs:14` `pub enum SignalMetric`
  (add `TrustDeviation`); `kask/crates/hkask-regulation/src/sensor_provider.rs:27`
  `pub(crate) trait Sensor` — `async fn observe(&self) -> Option<Signal>` with
  the contract **"None is not evidence of recovery."**
- **Binding:** a `TrustSensor` implementing `Sensor`, observing the ledger:
  weighted violation count, unverified-explicit count, and ΔT direction over
  the sense window. A trust violation reads as a deviation, never silence.
- **Sensor:** ledger window stats (D-computed counts).
- **Attenuator/amplifier:** the loop's deviation thresholds and algedonic
  escalation (`RuntimeAlert::new`, `kask/crates/hkask-regulation/src/algedonic.rs`:
  Warning past half the threshold, Critical at it) — trust deviations feed the
  existing escalation ladder.
- **The outage discipline (load-bearing):** the `Sensor` trait's own
  contract carries it — ledger store unavailable → `observe()` returns `None`
  → the loop must read sensor-unavailable, and nothing in the chain may
  `unwrap_or(0)` it into "no deviation." The module's `StoreUnavailable`
  error is the mechanism; a regression test pins that an outage surfaces as
  unavailability, not zero.
- **D/P:** D — the fold, the counts, the None-propagation; P — the threshold
  values (deviation bands for trust, like every other metric's bands).

## Binding 3 — Goal loop

- **Seams:** `kask/crates/kask_bridge/src/goals.rs:23` `pub struct GoalHost`;
  `kask/crates/kask_bridge/src/goals.rs:325` `impl NativeGoalHost for
  GoalHost::prepare`; the goal tools `kanban_goal_create` / `kanban_goal_judge`;
  the canonical contract
  `kask/mcp-servers/hkask-mcp-kata-kanban/GOAL_INTERFACE.md`.
- **Binding:** trust-building as a first-class goal criterion, judged through
  `kanban_goal_judge` like any criterion. Criterion text (template): "the user
  can extend delegation to the agent without re-checking the work." Judge
  evidence for the criterion: ledger stats over the goal's window (violations
  by weight, ΔT trajectory, unverified-explicit count) plus the delegation-
  radius reading.
- **Sensor:** (a) the ledger's D-computed stats; (b) re-check proxies — user
  revert/re-do/clarification actions against completed deliverables.
- **Amplifier:** goal progress visibility; the judge verdict is recorded
  against the criterion with evidence and limitations, per the goal-loop
  contract.
- **D/P:** D — event counting, ΔT computation; P — interpreting re-checks as
  trust signals and the criterion judgment itself. No code change required to
  adopt the criterion template (criteria are free-form at goal creation); the
  binding is discipline plus the template, and the ledger as evidence source.
- **Shared-tree note (2026-10-31):** while this design was being written,
  commit `6e8d054016` landed a `T_trust >= 0` structural constraint attached to
  every canonical goal (`hkask-mcp-kata-kanban/src/kanban/types/goal.rs`,
  `service_impl/goals.rs`). The two are complementary — the landed constraint
  is a hard floor on goals; this binding is the delegation-radius criterion
  with ledger-sourced judge evidence — and the implementation pass must
  reconcile them: the criterion template should reference the same T_trust
  term the constraint enforces, not invent a second one.

## Binding 4 — Algedonic review

- **Seams:** the reserved-name review board convention
  (`kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/types/board.rs:22-26` —
  `done_requires_verification` stamped by the reserved "Algedonic review" name);
  card creation through `kanban_task_create`; windowed event reads through the
  `curator_algedonic_log` pattern
  (`kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:857` —
  `regulation_store().query_recent_algedonic`).
- **Binding:** one trust-account card per review cycle: ΔT over the review
  window; the top violations by weight (verbatim instruction, what happened,
  repair state); the unverified-explicit count. The operator sees the trust
  account at review, not just task outcomes.
- **Sensor:** ledger deltas over the window (D).
- **Amplifier:** the operator's review disposition feeds back as ledger events
  — an operator ruling is the highest-weight expectation source there is:
  acceptance of verified work confirms keeps; a rejection on trust grounds is
  a violation-class event; `record_skill_feedback` outcomes land in the
  ledger with the review card as evidence.
- **D/P:** D — the delta math and card creation; P — the operator's judgment
  of the account (the review remains the human's, per the algedonic-review
  discipline; this binding only puts the account in front of the human).

## Binding 5 — Curator memory

- **Seams:** `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:1071`
  `pub async fn memory_insert`, with the evidence gate at `:1077-1081`
  (`fetch_existing_h_mem` — "The tool rejects inserts without a citation");
  `kask/mcp-servers/hkask-mcp-curator/src/types.rs:283-287`
  (`evidence_h_mem_id` — "the tool rejects inserts without a citation").
- **Binding:** trust lessons stored via `memory_insert` citing the ledger
  event as the evidence h_mem. With events stored as h_mems (the substrate
  decision), the citation gate closes mechanically: a trust lesson with no
  ledger event behind it cannot be inserted. Contradictory trust claims
  ("this constraint can be skipped") hit the existing contradiction surface;
  recurring lessons route to `therapy` reification (skills/templates/rules).
- **Sensor:** violation events trigger candidate extraction from the thread
  history (which contains the ledger rows).
- **Amplifier:** future recall of trust history feeds judgment across
  sessions — the rolling window is the memory's trust section; the
  `curator:goal:<id>` recall pattern shows how the memory store already
  publishes scoped rows.
- **D/P:** D — the insert's evidence gate (enforced today; the binding
  strengthens what counts as admissible evidence); P — lesson formulation
  and the decision to reify.

## Binding 6 — Kanban verification disclosure

- **Seam:** `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs:827-841`
  `pub async fn kanban_task_verify` — "Record verification evidence for a task
  in Review. The evidence text is the pass signal; acceptance criteria are
  guidance, not a gate," with the empty-evidence rejection at `:829-831`; the
  service-side gate `task_verify`
  (`kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/service_impl/service.rs:840`).
- **Binding:** verification evidence must state whether any explicit
  instruction was sidestepped to complete the task, and at what recorded trust
  cost. Two forms: "adherence: no explicit instruction sidestepped" or
  "sidestepped: <list>, ledger refs, ΔT = −x". Implementation shape: a typed
  `adherence` block on `TaskVerifyRequest` (structured, so presence is
  mechanically checkable — D) rather than a prose convention; a verify call
  without the block fails validation at the same place the empty-evidence
  check lives today.
- **Sensor:** the adherence declaration itself.
- **Amplifier:** the structural Done gate — on gated boards the only path to
  Done is through `kanban_task_verify`, so the disclosure rides the gate that
  already exists.
- **D/P:** D — the declaration's presence and format (schema-enforced);
  P — the declaration's truth, cross-checkable against the ledger wherever
  the instructions were captured (and a mismatch between a claimed
  "no sidestep" and a ledger violation row is a high-severity finding — the
  Kim et al. result prices false denial as the worst response).
- **Grounding for the disclosure-first design:** Kim–Ferrin–Cooper–Dirks 2004
  (verified in Part II) — exposed denial damages trust more than apology;
  honest disclosure is the least-damaging path once a violation occurred. The
  disclosure requirement is the system making the honest path the cheap path.

## Binding 7 — Steering projection surface (operator-instructed 2026-10-31)

- **Seams:** the decision-context assembly in the agent loop (the same path
  that renders tool results into the next turn's context) for the
  automatic injection; the ledger module's `steer` for the projection
  itself; the persistence rides the existing forecast-row pattern
  (`hkask-forecast`'s Brier machinery for scoring, resolved at
  `kanban_goal_judge` criterion results and `kanban_task_verify`
  outcomes).
- **Binding:** (a) automatic — when the ledger holds open weight-3
  expectations for the active task, `steer`'s projection table renders
  into the decision context before the agent acts on a candidate that
  touches those expectations; (b) on-demand — the projection is callable
  at any other decision point. This is the "provide it algorithmically in
  a way that makes sense to the LLMs" surface: one compact line per action
  — `path: open/cliff, P_goal, ΔT, note` — with the cliff reading as the
  path terminating (`P_goal: 0.00`), never as a mild cost.
- **Sensor:** the ledger's D-recorded expectations and outcomes feeding
  the projection; the persisted projections themselves (their own sensor
  — Brier-scored at resolution).
- **Attenuator/amplifier:** the injection trigger (weight-3 expectations
  open) attenuates token cost to exactly where the cliff risk lives; the
  calibration read (Brier, slope) on the algedonic trust-account card
  amplifies systematic mis-projection into operator visibility.
- **D/P:** D — the projection arithmetic (Lean-pinned), the persistence,
  the Brier scoring (existing machinery); P — the completion-probability
  estimate `P_complete` that feeds the projection (model-estimated, then
  calibration-scored — the estimate is P, its score is D).

## Rollout order (strangler-fig)

1. **Ledger module + intake capture** — the substrate; the new tree every
   later binding delegates to. No consumer yet; the fig starts standing alone.
2. **Binding 6 — verification disclosure** — cheap, high-signal; it forces the
   disclosure habit at the one structural gate that already exists.
3. **Binding 5 — memory evidence binding** — events-as-h_mems makes the
   citation gate pay immediately; trust history starts persisting.
4. **Binding 2 — regulation sense input** (operator-gated) — trust deviations
   start reading as deviations.
5. **Binding 4 — algedonic review card** — the account becomes visible to the
   operator; the sensor and the store exist by this point.
6. **Binding 3 — goal criterion template** — judged evidence now has a ledger
   behind it.
7. **Binding 7 — steering projection surface** — the automatic cliff
   projection lands once the ledger feeds it and the calibration row can
   score it; this is the operator-instructed guidance surface.
8. **Binding 1 — system-prompt clause** (operator-gated) — last, deliberately:
   the clause states what the other surfaces must already make true. A
   clause with no ledger behind it is a promise the system cannot keep; the
   strangler order lands the promise only when the mechanism can honor it.

## Operator gates in this design

- Binding 1 (system prompt) and Binding 2 (regulation sense input): explicitly
  operator-gated — proposed, not landed.
- Binding 6 changes a tool contract (`TaskVerifyRequest`) — a public surface
  change requiring operator approval before implementation.
- The parameter set the module hides (g, l, ρ, κ, T_0, W) is the operator's to
  ratify (Part II decision point 2); the module hides it precisely so ratifying
  does not touch consumers.

## Wiring

- `trust-economics-and-model.md` — the model these bindings carry
  (E, T, C_trust, provenance, check receipts).
- `trust-principle-proposal.md` — the values derivation that names why.
- `trust-program-synthesis.md` — the merged report and the validation design
  that will measure whether these bindings change anything.