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
