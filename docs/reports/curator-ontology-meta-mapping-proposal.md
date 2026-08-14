# Curator ontology meta-mapping — proposal

> Status: **Design document for review.** Nothing here is implemented. This
> proposal sketches the curator's meta-level capabilities for the
> ontology-tagging fleet — observability, drift detection, and evaluation —
> and grounds each in the cybernetics review's findings.

## Context

Ten MCP servers now carry an `ontology_anchor(tool: &str) -> Option<&'static str>`
fn mapping every registered tool to a concept URI from
`hkask-bridge-ontology`:

| Server                         | Ontology             | Anchor pattern                        |
| ------------------------------ | -------------------- | ------------------------------------- |
| `hkask-mcp-prediction-markets` | SDMX + Dublin Core   | inline match                          |
| `hkask-mcp-companies`          | FIBO                 | delegates to `fibo::tool_to_ontology` |
| `hkask-mcp-scenarios`          | PKO + Dublin Core    | inline match                          |
| `hkask-mcp-portfolio`          | FIBO                 | inline match (reference impl)         |
| `hkask-mcp-swarm`              | PKO                  | typed constant `pko::PROCEDURE`       |
| `hkask-mcp-kata-kanban`        | PKO                  | `kanban_type_to_pko` mapping          |
| `hkask-mcp-codegraph`          | SUMO + Dublin Core   | inline match                          |
| `hkask-mcp-condenser`          | PKO + Dublin Core    | inline match                          |
| `hkask-mcp-training`           | ML-Schema            | inline match                          |
| `hkask-mcp-media`              | OMC (partial: 17/40) | `omc::tool_to_omc` (scaffolded)       |

Every server calls `execute_tool_semantic` (in
`kask/crates/hkask-mcp-server/src/server/tool_span.rs`), which tags the
`reg.tool` span with the concept and emits a `tracing::warn!` when the
anchor is `None`. The bridge crate's `explain_tool_for` (in
`kask/crates/hkask-bridge-ontology/src/hkask_bridge_ontology.rs`) dispatches
a concept URI to an explain-tool name; `OntologyNamespace` (in
`kask/crates/hkask-bridge-ontology/src/axis.rs`) enumerates the seven
domain supplements.

The within-crate coverage is enforced: every standardized server has an
`ontology_anchor_covers_all_registered_tools` test that iterates the
router and asserts `Some` for every tool, plus an
`ontology_anchor_distinguishes_*` stub-collapse regression test. The
bridge crate has `explain_tool_for_covers_all_ontology_namespaces`
asserting dispatch coverage for all seven `OntologyNamespace` variants.

**What's missing is the fleet-level view.** The within-crate tests are
local; nothing aggregates across crates. The `tracing::warn!` fires but
nothing consumes it. The dispatch routes fire but nobody tracks whether
they land. The loop is open.

## The problem — three gaps

### 1. No consumption (blocked algedonic channel)

The `tracing::warn!` we added in `execute_tool_semantic`
(`kask/crates/hkask-mcp-server/src/server/tool_span.rs:213-219`) is the
algedonic channel: a registered tool with a `None` anchor is visible at
runtime. But the warning is emitted to the tracing stream and nothing
aggregates it. An operator cannot answer:

- Which ontology concepts are being used across the fleet right now?
- Which tools are unanchored (the `None` branch firing)?
- Which `explain_tool_for` dispatch routes are firing vs falling through
  to the `"research_search"` fallback?

The `reg.tool` span (emitted by `ToolSpanGuard` in the same file, line 147) carries the `ontology` field as a structured tracing field. The
curator's `reg_query` tool
(`kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:694`)
already reads `reg.*` spans from `RegulationArchive::replay_weighted`
and filters by namespace prefix. But it projects only `namespace`,
`path`, `phase`, `weight`, `observation` — it does not surface the
`ontology` field, and it has no aggregation (per-concept counts,
per-server coverage, fallback rate).

This is the cybernetics review's **blocked algedonic channel**: the
signal is emitted but not consumed. The S1→S5 feedback path exists at
the emission layer and stops there.

### 2. No maintenance (absent S4)

When a server adds a new tool, the `ontology_anchor_covers_all_registered_tools`
test catches the missing anchor _within that crate_. But there is no
cross-crate view. The curator cannot detect:

- A tool name that appears in multiple servers with different anchors
  (e.g., if `codegraph_query` and a hypothetical `research_query` both
  anchored on `sumo:ENTITY`, the dispatch would be ambiguous).
- An `OntologyNamespace` variant added to `axis.rs` without a
  corresponding `explain_tool_for` arm in `hkask_bridge_ontology.rs`.
  The bridge crate's `explain_tool_for_covers_all_ontology_namespaces`
  test catches this _if_ the test is updated; it does not catch a
  variant added without the test case.
- A server whose `ontology_anchor` returns concepts that
  `explain_tool_for` routes to a _different_ server's tools (cross-server
  dispatch). E.g., if `hkask-mcp-codegraph` anchored on `fibo:Corporation`,
  `explain_tool_for` would route the "Explain" affordance to
  `research_search`, not to a codegraph tool — a silent misroute.

This is the cybernetics review's **absent S4**: the system has no
sensor for cross-crate drift. Each crate is locally consistent; the
fleet is not checked.

### 3. No evaluation (open loop)

Does the ontology tagging actually improve widget dispatch quality?
When a widget's "Explain" affordance fires and `explain_tool_for`
routes to (say) `kanban_task_list` for a `pko:*` concept, does the
dispatched tool actually help the user? Nobody knows. The loop is:

```
widget "Explain" → explain_tool_for(concept) → tool name → tool invocation
```

The tool invocation emits its own `reg.tool` span (via
`execute_tool_semantic`), so the outcome is observable. But nothing
joins the explain-dispatch to the explain-outcome. The loop is open:

- No record that a `reg.tool` span was triggered _by_ an explain
  dispatch (vs a direct user invocation).
- No success/failure rate per concept.
- No feedback to the `ontology_anchor` fns: if a concept consistently
  routes to a tool that fails or returns unhelpful output, the anchor
  is wrong, but nothing flags it.

This is the cybernetics review's **open loop**: the action (dispatch)
is not closed back to the sense (did it help?).

## Proposal — three curator tools

The curator server (`kask/mcp-servers/hkask-mcp-curator/src/`) is the
right home for these capabilities. It already holds the Regulation
archive (`RegulationArchive`), the escalation queue, and the
`reg_query` tool. It is the fleet-level observability surface. The
curator does not need its own `ontology_anchor` fn in the same sense
as the others — its tools are _about_ the fleet, not tagged by it.

### Tool 1: `curator_ontology_audit` — consumption

Reads `reg.tool` spans from the Regulation trace (via
`RegulationArchive::replay_weighted`, the same store `reg_query` uses)
and reports fleet-level ontology usage.

**Inputs:** time window (default 24h), optional server filter.

**Outputs:**

- **Concept usage**: per-concept invocation count, grouped by
  ontology namespace (`fibo:*`, `pko:*`, `sumo:*`, `mls:*`, `sdmx:*`,
  `omc:*`, `dcterms:*`). Answers "which ontologies are actually in
  use?"
- **Unanchored tools**: tools whose `reg.tool` span has an empty
  `ontology` field — the `None` branch of `execute_tool_semantic`
  firing. This is the aggregated algedonic signal. Each entry cites
  the tool name and the server (from the span's `subsystem` field,
  `ToolSubsystem`).
- **Dispatch route firing**: for each `reg.tool` span whose tool name
  matches an `explain_tool_for` target, record the concept that routed
  to it. Aggregate to "which concepts dispatched to which tools, how
  often." Concepts that always fall through to the `"research_search"`
  fallback are flagged.
- **Per-server anchor coverage**: for each server, the ratio of
  spans with a specific anchor (a non-fallback concept) to total
  spans. A server whose `ontology_anchor` always hits its `_ =>`
  fallback arm has 0% specific coverage — the anchors are theater.

**Mechanism:** the `reg.tool` span's `ontology` field is already
emitted by `emit_tool_span` (`tool_span.rs:147`) as a structured
tracing field. The curator's `RegulationArchive` stores the raw span
event; the audit tool reads it back and aggregates. No new emission
is needed — this is pure consumption of an existing signal.

**Cybernetics mapping:** closes the **blocked algedonic channel**
(gap 1). The `tracing::warn!` becomes an aggregate the operator can
query, not a log line that scrolls past.

### Tool 2: `curator_ontology_drift` — maintenance

Detects cross-crate drift that the within-crate tests cannot see.

**Inputs:** none (reads the static fleet state).

**Outputs:**

- **Tool-name collisions**: a tool name that appears in multiple
  servers' routers with different anchors. (E.g., if two servers both
  registered a `query` tool but anchored on different concepts.) This
  requires the curator to enumerate each server's
  `combined_router().list_all()` and call its `ontology_anchor` — the
  servers are in-process via `hkask-mcp-*` library crates, so this is
  a compile-time fleet inventory, not a runtime probe.
- **Namespace without dispatch**: an `OntologyNamespace` variant in
  `axis.rs` without a corresponding arm in `explain_tool_for`. The
  bridge crate's test catches this only if the test is updated; this
  tool catches it by enumerating `OntologyNamespace::all()` (or
  equivalent) and calling `explain_tool_for` with a representative
  concept per namespace, asserting a non-fallback result for
  namespaces that have a domain-specific explain tool.
- **Cross-server dispatch**: a server whose `ontology_anchor` returns
  concepts that `explain_tool_for` routes to a _different_ server's
  tools. E.g., `hkask-mcp-codegraph` anchoring on `fibo:Corporation`
  would route "Explain" to `research_search` (a research-server tool),
  not to a codegraph tool. This is a silent misroute: the widget
  dispatches away from the server that produced the artifact.

**Mechanism:** this is a static analysis tool. It does not read the
trace; it reads the code. The curator imports each `hkask-mcp-*`
library crate, enumerates the router, and calls `ontology_anchor` for
each tool. It imports `hkask-bridge-ontology` and calls
`explain_tool_for` for each concept. The drift is a property of the
compiled fleet, not the running fleet.

**Cybernetics mapping:** closes the **absent S4** (gap 2). The
curator becomes the sensor for cross-crate consistency. The
within-crate tests remain; this tool adds the fleet-level view they
cannot have.

### Tool 3: `curator_ontology_evaluation` — evaluation loop

Closes the feedback from explain-dispatch to explain-outcome.

**Inputs:** time window (default 7d), optional concept filter.

**Outputs:**

- **Per-concept explain success rate**: for each `reg.tool` span
  whose tool name is an `explain_tool_for` target, join to the
  originating widget's explain affordance and to the outcome of the
  dispatched tool. The outcome is the `reg.tool` span's `outcome`
  field (`ok` / `error` / `dropped`). Aggregate: "concept X routed
  to tool Y N times, M of which succeeded."
- **Misroute candidates**: concepts whose dispatched tool has a
  high error rate or a high "dropped" rate (the tool was invoked but
  produced no output — a sign it was the wrong tool for the job).
  These are candidates for re-anchoring: the `ontology_anchor` fn
  that produced the concept is routing to a tool that doesn't help.
- **Fallback rate**: the fraction of explain dispatches that fell
  through to the `"research_search"` fallback. A high fallback rate
  means the ontology tagging is not actually driving dispatch —
  the concepts are missing from `explain_tool_for`'s explicit arms.

**Mechanism:** this requires joining two `reg.tool` spans: the
widget's explain-affordance span (the _cause_) and the dispatched
tool's invocation span (the _effect_). The join key is not currently
emitted — the explain affordance and the tool invocation are not
linked in the trace. **This is the one piece of new emission the
proposal requires:** the widget's explain affordance should emit a
`reg.tool.explain` span carrying the concept and the dispatched tool
name, so the curator can join cause to effect. This is a small
addition to the widget layer, not to the MCP server layer.

**Cybernetics mapping:** closes the **open loop** (gap 3). The
action (dispatch) is joined to the outcome (did the tool help?),
and the aggregate feeds back to the `ontology_anchor` fns via the
misroute candidates. An anchor that consistently misroutes becomes
a curator escalation (the curator already has the escalation queue).

## What this proposal does NOT do

- **No new `ontology_anchor` fn on the curator.** The curator's tools
  are about the fleet, not tagged by it. Adding an anchor for
  `curator_ontology_audit` would be tagging the auditor with the
  audited — a category error.
- **No re-implementation of `explain_tool_for`.** The dispatch lives
  in `hkask-bridge-ontology`; the curator consumes its decisions, it
  does not duplicate them.
- **No runtime mutation of anchors.** The `ontology_anchor` fns are
  `&'static str` returns — they are compile-time decisions. The
  evaluation loop feeds back to the human (or the curator's
  escalation queue), who edits the fn. The loop is closed at human
  cadence, not at runtime cadence. This is deliberate: anchors are
  type-level decisions, not configuration.
- **No new ontology namespaces.** The seven existing
  `OntologyNamespace` variants are sufficient. The drift tool
  catches a new variant added without a dispatch arm; it does not
  propose new variants.

## File and mechanism references

- `kask/crates/hkask-mcp-server/src/server/tool_span.rs:147` —
  `emit_tool_span`, the `reg.tool` span emission carrying the
  `ontology` field.
- `kask/crates/hkask-mcp-server/src/server/tool_span.rs:203-223` —
  `execute_tool_semantic`, the `tracing::warn!` on `None` ontology
  (the algedonic channel this proposal consumes).
- `kask/crates/hkask-bridge-ontology/src/hkask_bridge_ontology.rs:101-119` —
  `explain_tool_for`, the concept → explain-tool dispatch.
- `kask/crates/hkask-bridge-ontology/src/axis.rs:47-65` —
  `OntologyNamespace`, the seven domain supplements.
- `kask/crates/hkask-bridge-ontology/src/axis.rs:220-374` —
  `select_ontology_anchor`, the domain → axis selection (not directly
  used by this proposal, but the upstream of the anchor fns).
- `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:694` —
  `reg_query`, the existing Regulation-query tool this proposal
  extends.
- `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:49-59` —
  `CuratorStores`, the store handles (`RegulationArchive`,
  `EscalationQueue`, `MemoryStore`) the new tools would reuse.

## Cybernetics review alignment

The cybernetics review (see `docs/reports/` cybernetics review, and
the `pragmatic-cybernetics` skill) identified three failures in the
ontology-tagging fleet:

| Failure                                  | Gap in this proposal                                 | Curator tool that closes it                |
| ---------------------------------------- | ---------------------------------------------------- | ------------------------------------------ |
| Blocked algedonic channel                | The `tracing::warn!` fires but nothing aggregates it | `curator_ontology_audit` (consumption)     |
| Absent S4 (no cross-crate sensor)        | Within-crate tests are local; no fleet view          | `curator_ontology_drift` (maintenance)     |
| Open loop (action not joined to outcome) | Explain dispatch is not joined to explain outcome    | `curator_ontology_evaluation` (evaluation) |

Each tool is a sensor in the cybernetic sense: it observes a property
of the system the operator could not otherwise see. The audit tool
observes runtime usage; the drift tool observes static structure; the
evaluation tool observes the action→outcome join. Together they
constitute the S4 (fleet-level sensing) the review found absent.

The actuator remains the human (or the curator's escalation queue):
the tools report drift and misroutes, the human edits the
`ontology_anchor` fns. This is the correct cadence — anchors are
type-level decisions, and type-level decisions should not be
mutated at runtime by the system they observe.

## Open questions for review

1. **Join key for evaluation.** The explain-dispatch → explain-outcome
   join requires a new `reg.tool.explain` span from the widget layer.
   Is the widget layer the right emission point, or should the
   dispatch itself (in `explain_tool_for`'s caller) emit the span?
   The latter keeps the join key in the MCP layer; the former keeps
   it in the UI layer. The proposal leans toward the widget layer
   because the widget knows it is an "Explain" affordance; the MCP
   layer sees only a tool invocation.

2. **Static vs runtime drift.** `curator_ontology_drift` is a static
   analysis (it reads the compiled fleet). Should it also run at CI
   time, as a fleet-level test? The within-crate tests are already
   CI-enforced; a fleet-level drift test would be the natural
   complement. This proposal does not specify the CI integration —
   that is a follow-up decision.

3. **Evaluation cadence.** The evaluation loop feeds back at human
   cadence (the curator escalates, the human edits). Should there be
   a faster loop — e.g., the curator auto-disabling an anchor that
   misroutes >N% of the time? The proposal says no: anchors are
   type-level, and auto-disabling a type-level decision at runtime is
   a category error. But this is a design decision worth confirming.

4. **Scope of `curator_ontology_drift`.** The cross-server dispatch
   check requires the curator to import every `hkask-mcp-*` library
   crate. This is a build-time dependency from the curator on every
   server. Is this acceptable, or should the drift check be a
   separate binary/test that the curator invokes? The proposal
   assumes the curator imports them; the alternative is a
   `cargo test`-style fleet inventory.
