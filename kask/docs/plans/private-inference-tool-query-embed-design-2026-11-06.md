---
title: "Private-inference — the curator tool-side query embed, provenance-routed (design, 2026-11-06)"
audience: [architects, developers, operators]
last_updated: 2026-11-11
version: "1.0.0"
status: "Active"
domain: "Private inference / memory privacy boundary"
mds_categories: [domain, trust, lifecycle]
---

# Private-inference — the curator tool-side query embed, provenance-routed (design, 2026-11-06)

> **Context note, 2026-11-11.** The provenance-routed embed and rerank legs designed here are in force. Where this text says "the private leg" it now means the local Ollama models configured under `kask.private_inference`; the kask-launched llama.cpp server was removed by operator ruling. The live record is `DIVERGENCE.md` row D100.

STATUS: **approved by the operator 2026-11-06; implemented the same day** —
the query side (Phase 1) AND the write side (Phase 2, under the operator's
"proceed with the proposed follow-ups" direction): the passage embeds of
private-marked rows route by the row's provenance, the model-mismatch sweep
skips private-marked candidates with a surfaced count, and the
revision/repair paths preserve the mark. D100's row carries the landed
record and the pin list. **§8.1 (the research rerank leg) landed the same
day too** — see §8's record below.

## 1. The seam (IS — tree-verified 2026-11-06)

The curator MCP child embeds every query via the configured embedder
regardless of the call's provenance:

- `semantic_recall_fragments`
  (`kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:411-441`)
  embeds the query with `curator_embedding_model()` (env
  `HKASK_EMBEDDING_MODEL` ← `kask.models.embedding_model`) through
  `inference_port.embed_with_dimensions` — the zed IPC bridge
  (`HKASK_INFERENCE_SOCKET`) → zed's LanguageModelRegistry → the
  configured (cloud) embedder. It serves `curator_semantic_search`
  (`:446`) and `curator_consult`.
- `curator_federated_search` embeds the same way at its head (`:553-576`)
  and feeds both its curator leg and every sealed corpus source with the
  one query vector.
- The per-call provenance is already readable in the child
  (`current_call_provenance()`,
  `kask/crates/hkask-mcp-server/src/server/call_provenance.rs:35` — the
  M5 read side; every kask server gets it by construction). The
  query-embed sites never consult it.

A private-marked context's tool query therefore crosses to the cloud
embedder whenever the configured model is cloud-provider — the one
remaining privacy seam named in D100.

## 2. The routing rule (the child-side mirror of the item-3 lift)

At the embed sites, read `current_call_provenance()`:

- **Private call → the private embedding model.** The setting
  `kask.private_inference.embedding_model` (default
  `ollama/qwen3-embedding:0.6b`, `DEFAULT_PRIVATE_EMBEDDING_MODEL`) reaches
  the child as a NEW `config_env` entry (`HKASK_PRIVATE_EMBEDDING_MODEL`)
  on the curator server's registry block
  (`kask_bridge/src/mcp_servers.rs`, alongside `HKASK_EMBEDDING_MODEL`),
  emitted by `mcp_env()`. The child embeds the query with THAT model
  string; the IPC server's registry resolution routes it to the local
  ollama descriptor — loopback `/v1/embeddings`, no credentials (the same
  per-call model resolution the backfill sweep already exercises over the
  same port). The model string IS the routing signal — the same
  discipline as the stored-vector model-string gate.
- **Public call → unchanged** (`HKASK_EMBEDDING_MODEL`, the configured
  cloud model).
- **Unset/empty private model (the operator's explicit opt-out) or a
  failed local embed → NO embed for the private call:** the semantic leg
  degrades to exact/keyword-only with a surfaced note — the mirror of
  `private_context_recall_stays_keyword_only_when_no_local_port_is_wired`.
  Never cross.

## 3. The consequence, stated honestly

The child's store is the PUBLIC store — the child never opens the private
DB (the store-split posture; the production private-recall path is the
bridge injector, which already embeds private queries locally). A private
call's locally-embedded query vector lives in the local model's space, so
the KNN model-string gate excludes every cloud-model row: the semantic leg
returns empty with the EXISTING surfaced `excluded_model_mismatch` counts
(`semantic_recall_fragments_for_vector` carries them at `:406`; the
federated status surfaces them per source at `:617-621`). That is the
bridge's already-recorded posture — "the public store's KNN leg is
unreachable from private contexts by construction … while its rows stay
keyword-reachable there" — made true at the child too. Private calls keep
the exact/keyword legs (`call_admits`-filtered) and lose
semantic-over-public; degrade-not-leak, surfaced, never silent. The
federated sealed-corpus legs follow the same rule (the one local-model
vector gates out at each cloud-embedded source — per-source surfaced
status; crossing is the leak being closed). The bridge injector remains
the semantic path for private content, so the capability loss is bounded
and named.

## 4. The write-side seam, named for scope (same repair family)

`memory_insert` from a private call writes a private-marked row (the mark
inherited from evidence — pinned
`memory_insert_inherits_the_evidence_inference_provenance`) into the
child's public store and embeds its PASSAGE via `semantic_embedding` →
the configured (cloud) model — private content on the same crossing.
`publish_semantic_write`/`embed_for_semantic_recall` and the backfill
sweeps share the site. This design proposes that as **Phase 2** — the same
routing rule over private-marked rows' passage embeds — named now so the
seam is not discovered later. The operator decides whether Phase 2 rides
the first implementation or follows it.

## 5. The M5 positive-control re-shape (the operator checkpoint's substance)

- `private_call_contexts_receive_their_private_rows`: the KNN fixture
  must seed its private rows under the LOCAL model string (a private
  call's query embeds local-model; the gate matches local-model rows).
  The re-shaped pin ALSO asserts the surfaced model-gate emptiness over
  cloud-model rows — the production child store's only shape.
- `semantic_search_filters_private_rows_by_call_context`: both sides
  re-shape the same way.
- NEW pins:
  `private_call_query_embeds_via_the_private_model_not_the_configured_embedder`
  (a recording stub port: a private call makes ZERO configured-model
  embeds and one private-model embed), `public_call_query_embeds_unchanged`,
  `private_call_federated_sources_surface_the_model_gate_exclusion`, and
  the degrade pin (unset private model → keyword-only, surfaced, zero
  embed calls).
- The no-field rule holds: no tool SCHEMA gains a provenance field (the
  routing reads `_meta` only, parent-set).

## 6. Gates on implementation

`cargo test -p hkask-mcp-curator` (the tool_behavior suite),
`cargo test -p hkask-mcp-server`, the settings → `mcp_env` → allowlist
injection pin, and `./script/clippy` over every touched kask crate.
D100's row extends in the same change.

## 7. The checkpoint question

Approve this design (it changes the M5 positive-control shape), and say
whether Phase 2 — the write-side passage embeds of private-marked rows —
rides the first implementation or follows. The alternative this design
rejects: routing private queries to the configured cloud model — the
crossing D100 names as the seam.

## 8. The remaining legs of the same class (the 2026-11-06 review's extension, operator-directed)

**Landed record (2026-11-06, the same day the design's phases landed):**
leg 1 (the research server's rerank leg) landed as the surfaced skip — a
private-provenance deep search makes ZERO configured-model rerank calls,
keeps the heuristic RRF order, and surfaces the boundary as the degrade
reason (`mode: heuristic`); no private chat/rerank model setting exists,
which is why the skip (this design's sanctioned alternative) and not the
route. The implementation's own emission census ALSO found a seam this
section did not name: the research server's `evaluate_evidence`
semantic-duplication tier embeds caller artifact content through the
configured cloud embedder on `duplication="semantic"` — the same class,
closed the same day by ROUTING through the private leg's local model (the
`HKASK_PRIVATE_EMBEDDING_MODEL` admission extended to the research server,
unset/empty the surfaced shingle-floor degrade). D100 carries both records
and the four pins. **Leg 2 (the corpus server's assisting-inference legs) is
CLOSED by the operator's 2026-11-06 ruling — shape 2, the perimeter:** the
corpus server joined the default `private_thread_disabled_servers` set
(the settings_content const), so its tool surface is refused on
private-mode threads (the corpus server is not available in private mode
— the ruling's own words); no per-tool routing was built, and the M5
off-axis acceptance keeps covering only the corpus store's recall axis.
Pin: `corpus_server_tools_are_absent_on_private_threads`.

The same repair family has two more server-side inference legs that cross
private thread content to cloud models, both unruled today (the review:
`kask/docs/research/template-inference-private-mode-decision-models-review-2026-11-06.md`
sections 2.1/2.3; appended under the operator's 2026-11-06 "proceed with the
proposed actions" direction):

1. **The research server's rerank leg.** The deep-search rerank stage sends
   private thread content (query plus retrieved snippets) through
   `HKASK_RERANK_MODEL`'s cloud provider regardless of the call's provenance
   — the same shape this design closed at the curator child, with no
   recorded ruling covering it (the corpus surface carries the PI's
   off-axis ruling, M5 plan section 4; the research server has none). The
   M5 read side is wired into every kask server by construction
   (`call_provenance.rs`), so the same rule applies mechanically: a
   private-provenance call routes its rerank leg to a local model or skips
   the stage with a surfaced note (degrade-not-leak), never a cloud
   crossing. **Disposition 2026-11-06 (landed):** the skip shape landed
   first (the provenance-routed research legs, D100); the operator's
   follow-up ruling — private mode uses the ollama qwen reranker — then
   landed the ROUTED shape: the private leg's rerank model setting
   (`kask.private_inference.rerank_model`, default
   `ollama/qwen3-reranker-4b:latest`) routes private-provenance rerank
   through the local ollama judge protocol, with the surfaced skip as the
   explicit-empty opt-out; the repaired `kask.models.rerank_model` default
   and the local serving arm are recorded in the research.md decision
   record and D100.
2. **The corpus server's assisting-inference legs.** `corpus_embed`,
   `corpus_tag_chunks` and the OCR vision leg on private-provenance calls
   embed, tag and OCR through the cloud models. The M5 off-axis ruling
   covers the recall axis; the ruling owed here is whether that acceptance
   covers these tool legs too, or whether the intake-time corpus privacy
   field (M5 plan section 4.1's named future mechanism) is scheduled.

Both ride the decision the operator already took for this design — the
provenance-routed model leg — and need no new mechanism; the alternative
they reject is the same one this design rejects: routing private content
to the configured cloud model.
