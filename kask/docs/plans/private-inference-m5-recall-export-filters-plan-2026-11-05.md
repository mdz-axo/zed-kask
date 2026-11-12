---
title: "Private-inference M5 — the recall/export filters (FR-4's read side): design for PI review"
audience: [architects, developers, operators, agents]
last_updated: "2026-11-05"
version: "0.1.0"
status: "Proposed — designed against the post-D100/FR-4 tree; the ingest and injector legs are landed and cited, this designs the open legs; implementation is a separate engagement — nothing here is built or claimed built"
domain: "Private inference / memory privacy boundary"
mds_categories: [domain, composition, trust, lifecycle]
---

# Private-inference M5 — the recall/export filters (design)

> **Context note, 2026-11-11.** The recall/export filters designed here are in force. The kask-side serving stack named around them (the "D-3 plan", the local llama.cpp leg) was removed by operator ruling; private inference is now served by the local Ollama or llama.cpp install through the editor's built-in providers. The live record is `DIVERGENCE.md` row D100.

**Scope.** FR-4's read side: private-marked content stays inside private
recall and export paths. The D-3 plan's M5 row names four surface groups
(curator memory recall; corpus; media; kata-kanban). This document
inventories what the D100/FR-4 landings already cover, designs the open
legs, and scopes the surfaces the thread-routing mark does not reach in
v1 — with those adjudications as operator checkpoints, never silent.

**Provenance.** Grounded against the current tree (2026-11-05):
`ff745e3233` + the concurrent stream's `77585247c8` (D100 — the thread
privacy mark + FR-3 routing enforcement) and `2f8a5705a3` (the FR-4
ingest stamping + the injector leg). Every landed claim below cites the
live file:line; the design decisions are proposals.

## 1. Direction (kata step 1)

FR-4's falsifiers bind this design (requirements D-2, verbatim):

- **(b)** a public thread or public Curator thread recalling private
  memory fails — the named enforcement path is the Curator
  context-injection seam (`BridgeContextInjector` →
  `recall_context_curator`) **plus the curator recall tools**
  (`curator_federated_search`, `curator_semantic_search`,
  `curator_memory_recall`): a recall in a non-private context returning
  a private-marked row fails, while a private-marked Curator thread can
  recall its private rows;
- **(c)** private memory in a public export path — an export tool
  emitting a private-marked row fails.

Honest limits carried over unchanged: confidentiality against a
host-level attacker is out of scope; a tool-mediated exfiltration path
(an agent posting private content through a public tool) is taint
tracking — **not gated** (P4.2); this design enforces routing and
memory invariants, and says so rather than implying general taint
control.

## 2. Current condition (kata step 2) — the landed vs the open

### Landed and verified (cite-checked at draft time)

1. **The mark type.** `InferenceProvenance::{PublicProvider,
   PrivateInference}` (`kask/crates/hkask-types/src/visibility.rs:99-107`)
   with a fail-closed parse: a present-but-unknown stored mark reads as
   `PrivateInference` — privacy fails closed (V7); the legacy
   `DEFAULT 'public_provider'` backfill is honest history
   (`visibility.rs:125-140` — `kask/crates/hkask-types/src/visibility.rs`).
2. **The ingest stamping** (`2f8a5705a3`): the producing thread's
   routing state is the only source — `TurnRecord.inference_provenance`
   (`kask/crates/hkask-types/src/ports/memory_port.rs:44-56`), every
   chunk h_mem stamped on the write path; `memory_insert` inherits the
   cited evidence row's provenance (the request schema offers no field
   to claim — `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:1145-1146`);
   distillation taints a batch private when any source chunk is private
   (`kask/mcp-servers/hkask-mcp-curator/src/distillation.rs:583-640`).
3. **The injector leg (falsifier (b), context-injection half).** The
   calling thread's D100 mark rides `inject_context(…
   private_context: bool)` from the turn loop
   (`kask/crates/kask_bridge/src/context_injector.rs:316-320`) into
   `recall_context_curator`/`recall_thread_curator`
   (`kask/crates/kask_bridge/src/memory.rs:729-748,757-776`), where
   both recall legs filter `inference_provenance == PrivateInference`
   rows out of non-private contexts **with surfaced exclusion counts**
   — an empty result must not read as "no memory found"
   (`kask/crates/kask_bridge/src/memory.rs:849,946-953,1016-1019,1063,1171-1218`). Pinned by
   `private_context_injection_recalls_its_private_rows` and the public-
   context counterpart (`kask/crates/kask_bridge/src/context_injector.rs:685,756`).

### Open (verified by grep at draft time — zero hits for provenance
handling outside the ingest/injector legs)

1. **The curator recall tools** (`curator_semantic_search` at
   `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:399`,
   `curator_federated_search` at `:472`, `curator_memory_recall` at
   `:639`) take no caller-context signal:
   a public-marked session calling `curator_memory_recall` receives
   private-marked rows today. **Falsifier (b)'s tool leg is unadjudicated.**
2. **The export paths** (falsifier (c)): the recall/search tools'
   outputs are exports in the (c) sense and share the same gap; the
   corpus/kata-kanban servers carry zero `InferenceProvenance`
   references.
3. **The caller-context plumbing** — the load-bearing question: the
   curator MCP child is process-global (one stdio child serves every
   session), so the requesting thread's mark cannot ride the spawn env
   (the `HKASK_WEBID` pattern fits only process-global identity); it
   must be per-request and parent-set.

## 3. The caller-context plumbing (the one design decision that
gates the rest)

The MCP call envelope carries a protocol-level metadata channel:
`CallToolRequestParams.meta` (`_meta`, SEP-1319 — present in the
workspace's rmcp 3.8.0, on the `CallToolRequestParams` struct's
`meta` field — a cargo-registry file, cited structurally here; the
field is `Option<RequestMetaObject>` under `#[serde(rename = "_meta")]`),
set by the CALLER of the tool and
invisible to the tool's arguments. The two options:

- **M5-A (recommended): `_meta`-ridden provenance.** The zed-side
  dispatch (`McpRuntime::invoke`) sets
  `meta.inference_provenance` from the calling thread's D100 mark
  before every curator-server call; `hkask-mcp-server`'s dispatch reads
  it into the per-call `ServerContext`; the curator tools filter on it
  exactly as the injector does (private caller → private rows
  included; public → excluded, with the surfaced-exclusion discipline).
  Security: the stdio channel is the trust boundary (the same trust
  the spawn-time `HKASK_WEBID` rides) — the parent sets the mark, the
  agent never sees a field to claim it (the ingest side's no-field
  rule, applied to the read side). Cost: a small fork-owned protocol
  extension at both ends (both are kask crates — no upstream surface),
  plus one context read in `run_turn`'s dispatch path.
- **M5-B (the minimal alternative): tools are public-only in v1.** The
  three tools unconditionally exclude private rows (surfaced as a
  note); the injection seam remains the only private-recall path — the
  PI's private Curator sessions get their private rows via injection
  but cannot query them by tool. Zero plumbing; a real capability loss;
  the falsifier (b) tool leg passes on the public side only.

**Recommendation: M5-A.** The plumbing is small and entirely
fork-owned, and the PI's stated V1 use (a private-marked Curator
session working with private content) needs tool recall, not just
injection. M5-B is the fallback if the protocol extension is judged
not worth the surface.

## 4. The surfaces the mark does not reach in v1 (adjudications for the operator)

The D-3 M5 row lists four surface groups; the thread-routing mark
honestly reaches one and a half of them today. The rest derive from
content that is **not thread-routed**, and the requirements' honest
limits rule general taint tracking out of scope — so each needs an
explicit ruling, not a silent assumption:

1. **Corpus** (`corpus_query`, `corpus_export_bundle`,
   `corpus_ingest_qa` outputs): corpus passages and QA pairs derive
   from ingested documents and server-side model calls, not from the
   requesting thread's route. A private thread's *report* saved into
   the corpus is a tool-mediated flow (the excluded taint class).
   **Proposed v1 ruling:** corpus content is not on FR-4's
   thread-routing axis — no mark, no filter, recorded as a stated
   limit; a corpus-level privacy field (an intake-time mark on
   documents/bundles) is the future mechanism if you want it.
2. **Media** (`educt_search_transcripts`, `layers_search`,
   `educt_export`): transcripts derive from recordings — no routing
   axis exists at all. **Proposed v1 ruling:** same stated-limit
   record; an intake-time privacy field on recordings is the future
   mechanism.
3. **Kata-kanban goals**: goal records are operator-authored
   lifecycle artifacts; the assessment reads local artifacts, not
   thread routes. **Proposed v1 ruling:** not on the axis; stated
   limit.

If any of these three rulings is wrong for your intent, that surface
needs its own marking design (an intake field + filters) — a separate
engagement, sized per surface.

## 5. The falsifier test plan (per leg)

- **(b) tool leg, public side:** a `curator_memory_recall` call with
  `_meta.inference_provenance = public` never returns a private-marked
  row, and the exclusion is surfaced (never a silent empty).
- **(b) tool leg, private side:** the same call with the private mark
  returns the private rows (the positive control — the gate must not
  over-block).
- **(c):** every tool whose output emits h_mem-derived rows (the three
  recall/search tools; any future export tool) is pinned the same way;
  the surfaced-exclusion note is part of the contract.
- **The plumbing pin:** the agent-facing tool schemas expose NO
  provenance field (the no-field rule, pinned by a schema assertion),
  and a missing `_meta` value fails closed (public — the strictest
  read for recall: no mark claimed, no private rows shown; the child
  treats an absent parent signal as a public context).

## 6. Routing plan (steps, regions, gates)

| # | Step | Region | Gate |
|---|---|---|---|
| 1 | The zed-side `_meta` write: `McpRuntime::invoke` rides the calling thread's D100 mark on curator-server calls | Sem-D | a pin that the mark reaches the wire only parent-side (the request fixture asserts `_meta`; a schema assertion that no tool takes a provenance arg) |
| 2 | `hkask-mcp-server` dispatch: read `_meta` into the per-call `ServerContext`; absent → public (fail-closed for recall) | SD | the decode/parse pin over the closed set (the `visibility.rs` parse, reused — never a second parser) |
| 3 | The three curator tools filter on the per-call provenance with surfaced exclusions (the injector's pattern, one helper — no copy) | Sem-D | the (b) falsifier tests, both sides + the exclusion surfacing |
| 4 | `curator_federated_search`: the same filter over its memory leg (the sealed-source leg is corpus content — the §4 ruling) | Sem-D | the same pins over the federated path |
| 5 | The stated-limits records: FR-4's doc row + the corpus/media/kata sections gain the v1 rulings verbatim | Sem-P | operator checkpoint: approve or amend each ruling |
| 6 | DIVERGENCE.md: the M5 seam registration (the `_meta` provenance extension + the tool filters) | SD | the register's file list + pins in the same change |

## 7. Claim labels

| Claim | Mode | Force | Provenance |
|---|---|---|---|
| The ingest stamping and the injector filter are landed and pinned | IS | Evidence | `2f8a5705a3` + the cited file:line set (§2) |
| The curator tools return private rows to public callers today | IS | Evidence | grep: zero provenance handling at the three tools (draft time) |
| `_meta` exists as a parent-set per-request channel | IS | Evidence | rmcp 3.8.0's `CallToolRequestParams.meta` (SEP-1319) |
| The `_meta` leg never surfaces a private row to a public call | — | **Hypothesis** | not admitted until the (b) pins run |
| Corpus/media/goal content is off the thread-routing axis in v1 | OUGHT→ruling | Proposal | §4 — the operator's checkpoint, never a silent assumption |

## 8. Open items for the operator

1. **M5-A vs M5-B** — the `_meta` plumbing (recommended) vs
   tools-public-only v1.
2. **The three §4 rulings** — approve the stated limits for corpus,
   media, and kata-kanban, or name the surface that needs its own
   marking design.
3. **Implementation gating** — on approval this becomes an engagement
   of steps 1–4 + 6; step 5 is the rulings record.

## 9. Implementation record (2026-11-05, the approved M5-A execution)

The operator approved M5-A and the three §4 stated-limit rulings
("proceed as recommended"); steps 2–6 landed the same day.

- **Step 2 — the child-side plumbing** (`hkask-mcp-server`'s new
  `server/call_provenance.rs`): the per-call provenance task-local, the
  request-claim parser (ONLY the exact closed-set private string claims
  private — absent, null, non-string, or unknown values all read
  PUBLIC; fail-closed for recall, the request-direction mirror of the
  stored-mark parser's own fail-closed), the `with_call_provenance`
  test seam, and the `ProvenanceScopedService` decorator — a
  `Service<RoleServer>` wrapper that scopes the provenance from the
  tools/call request's `_meta` around the inner service, wired ONCE in
  `run_stdio_server` (every kask MCP server gets the M5 read side by
  construction, zero per-server edits). Pins:
  `absent_or_unknown_request_marks_read_public`,
  `the_exact_private_string_claims_private`,
  `the_scope_reaches_handlers_and_absence_reads_public`,
  `the_call_params_shape_parses_from_wire_json`.
- **Steps 3–4 — the curator tools**: the shared `call_admits` predicate
  + per-site surfaced exclusion counts. `exact_recall_leg` filters and
  surfaces `excluded_private_rows` (covers `curator_memory_recall`'s
  legs, `curator_consult`'s fallbacks, and
  `curator_semantic_search`'s degraded leg — the fallback filters too,
  degradation and the privacy boundary compose);
  `semantic_recall_fragments_for_vector` filters at the push loop and
  carries `excluded_private` on `SemanticRecall` (covers
  `curator_semantic_search`'s KNN leg, `curator_consult`, and
  `curator_federated_search`'s curator leg — the federated status's
  `reason` names the exclusions); the ontology leg filters the same
  way. Pins: `public_call_contexts_never_receive_private_rows`
  (falsifier (b), public side — the exclusion surfaced, never silent),
  `private_call_contexts_receive_their_private_rows` (the positive
  control — the gate must not over-block),
  `semantic_search_filters_private_rows_by_call_context` (the KNN
  path, both sides), `no_tool_request_exposes_a_provenance_field`
  (the no-field rule pinned at the SCHEMA surface — all four request
  types' JSON schemas expose no provenance property).
- **Step 5 — the rulings record**: the three §4 rulings stand as
  written (the operator's "proceed as recommended"): corpus, media, and
  kata-kanban content is not on FR-4's thread-routing axis in v1 —
  stated limits, with the intake-time privacy field as the named
  future mechanism.
- **Step 6 — DIVERGENCE.md**: D100's row extends with the M5 read-side
  sentence and the full pin list.
- **Step 1 — the zed-side `_meta` write** (landed 2026-11-05, two
  commits: the kask leg then the zed leg):
  `ToolCallEventStream` captures `calling_provenance` at construction from
  the calling thread's D100 mark (the D68 capture discipline — host-derived
  inside the dispatch update, never from tool input; un-tied test streams
  default `PublicProvider`); `KaskToolSource::invoke` carries it as an
  explicit host-set parameter; the governed runtime's
  `invoke_with_call_provenance` threads it into the request params —
  agent-driven calls stamp `_meta.inference_provenance` under the one-home
  key `hkask_types::META_INFERENCE_PROVENANCE`, while hkask-mcp's plain
  `ToolPort::invoke` path stays un-stamped (non-thread-routed callers keep
  the pre-M5 wire form; the child reads them public, fail-closed).
  Parent-side pins:
  `calling_provenance_is_captured_from_the_threads_d100_mark` (thread.rs),
  `kask_server_tool_passes_the_calling_threads_provenance`
  (context_server_registry.rs — with the input-spoof falsifier, the
  no-field rule's parent-side leg),
  `call_request_params_serialize_the_calling_provenance_meta` (the
  request-fixture wire pin, hkask-mcp),
  `from_private_flag_maps_the_d100_flag_both_ways` +
  `the_meta_key_is_the_exact_wire_spelling` (hkask-types). FR-4 falsifier
  (b)'s tool legs now compose end-to-end at the wire with the child-side
  pins (bc1170dcff unchanged) over the same serialized form — the interim
  strictest-default period closed: a private-marked thread's curator tool
  calls now read its private rows. The `:NNNN@sym` thread.rs/agent.rs
  citations in skills-and-composition.md and lisp-eval-dialect.md were
  re-anchored against the shifted tree in the same change.

**Hypotheses adjudicated:** H-M5.1 (a public call context never
receives a private row — corroborated on both the exact and semantic
legs); the falsifier (c) leg (export paths) is covered by the same
predicate over every fragment-emitting site — the three tools' outputs
plus consult are the h_mem-emitting surface in this server.
