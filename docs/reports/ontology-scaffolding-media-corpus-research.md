# Ontology scaffolding — media, corpus, research

> Status: **Migrations complete.** All three servers now have full
> ontology-tagging migration: `ontology_anchor` fns, `execute_tool_semantic`
> span tagging, coverage tests, and stub-collapse regression tests. This
> document is retained as the historical plan record. See the server source
> for the current state:
>
> - `kask/mcp-servers/hkask-mcp-research/src/hkask_mcp_research.rs` — 23 tools, 7 concepts
> - `kask/mcp-servers/hkask-mcp-corpus/src/hkask_mcp_corpus.rs` — 27 tools, 7 concepts
> - `kask/mcp-servers/hkask-mcp-media/src/omc.rs` — 41 tools, 8 concepts

## Why these three are deferred

The standardization pattern (see `docs/reports/mcp-ontology-tagging-proposal.md`
and the continuation prompt) requires that a server's `ontology_anchor`
fn map every registered tool to a real concept URI from
`hkask-bridge-ontology`. Forcing anchors before the concept mappings are
designed produces speculative tags — anchors that look right but are
theater. The within-crate `ontology_anchor_covers_all_registered_tools`
test would pass (every tool returns `Some`), but the
`ontology_anchor_distinguishes_*` stub-collapse regression would fail
to catch a real collapse because the anchors were never grounded in
workflow semantics.

Each of these three servers has a different blocker, described below.

---

## 3a. `hkask-mcp-media` (40 tools)

### Current state

- **Already depends on `hkask-bridge-ontology`.**
- Has `omc::tool_to_omc(tool: &str) -> Option<OmcConcept>` in
  `kask/crates/hkask-bridge-ontology/src/omc.rs` covering **17 of 40**
  tools with typed OMC constants (`SCENE`, `ASSET`, `CREATIVE_WORK`,
  `VERSION`, `MEDIA_SOURCE`, `SEQUENCE`, `SHOT`, `PARTICIPANT`, `TASK`).
- The remaining **23 tools return `None`**.
- All 40 call sites use bare `execute_tool` (not `execute_tool_semantic`).

### Why premature

The OMC module exists and has the right constants. The `tool_to_omc` fn
is the right shape (same delegation pattern `hkask-mcp-companies` uses
with `fibo::tool_to_ontology`). But the media workflows are not built
out. The remaining 23 tools do not have clear OMC mappings because the
workflows they would map to do not exist yet. Forcing anchors now would
be speculative: the anchor would be a guess at what the workflow will
produce, not a description of what it does produce.

### Scaffolding plan

1. **Extend `omc::tool_to_omc` to cover all 40 tools as the media
   workflows are built out.** Each new tool gets an arm in
   `tool_to_omc` returning a typed OMC constant. The arm is added in
   the same PR that adds the tool — the within-crate coverage test
   (added in step 3) enforces this.

2. **Wire `tool_to_omc` as the media server's `ontology_anchor`.**
   Same delegation pattern as companies:

   ```rust
   fn ontology_anchor(tool: &str) -> Option<&'static str> {
       hkask_bridge_ontology::omc::tool_to_omc(tool)
   }
   ```

   This lives on `impl MediaServer` in
   `kask/mcp-servers/hkask-mcp-media/src/hkask_mcp_media.rs` (where
   `combined_router` is). The fn returns `Option<OmcConcept>` (which is
   `&'static str`), matching the standard `ontology_anchor` signature.

3. **Add the coverage test once all 40 tools are mapped.** The test
   iterates `MediaServer::combined_router().list_all()` and asserts
   `MediaServer::ontology_anchor(&tool.name).is_some()` for every tool.
   Until all 40 are mapped, this test would fail — so it is added last,
   not first. The stub-collapse regression test
   (`ontology_anchor_distinguishes_*`) can be added earlier, asserting
   that at least two distinct OMC concepts are in use (e.g., `SCENE` vs
   `CREATIVE_WORK`).

4. **Swap all 40 call sites** from `execute_tool(self, "name", async {`
   to `execute_tool_semantic(self, "name", Self::ontology_anchor("name"), async {`.
   Each `tools/*.rs` file's import changes from `execute_tool` to
   `execute_tool_semantic`.

5. **The fallback for unmapped tools should be `omc::CREATIVE_WORK`**
   (all media tools produce creative works), not `dc_bibo::DATASET`.
   This means the `tool_to_omc` fn's `_ =>` arm returns
   `Some(CREATIVE_WORK)` rather than `None` once the server is fully
   migrated. During the partial-migration phase, unmapped tools return
   `None` (so the `tracing::warn!` fires and the curator's audit tool
   can see them) — the fallback is only added when the coverage test
   would pass.

### Key reference

- `kask/crates/hkask-bridge-ontology/src/omc.rs` — the OMC concept
  vocabulary (`CREATIVE_WORK`, `SCENE`, `SHOT`, `SEQUENCE`,
  `PARTICIPANT`, `MEDIA_SOURCE`, `ASSET`, `TASK`, `VERSION`) and the
  `tool_to_omc` mapping (17 tools mapped, 23 returning `None`).
- `kask/mcp-servers/hkask-mcp-companies/src/hkask_mcp_companies.rs:246-250` —
  the delegation pattern to copy (`ontology_anchor` delegates to
  `fibo::tool_to_ontology`).

### Trigger for migration

The media workflows are built out enough that every tool has a clear
OMC concept. Concretely: the 23 unmapped tools each have a workflow
that produces an artifact mappable to an OMC constant. This is a
product decision, not a technical one — the OMC vocabulary is
sufficient; the workflows are not.

---

## 3b. `hkask-mcp-corpus` (27 tools)

### Current state

- **Already depends on `hkask-bridge-ontology`.**
- Uses typed constants in non-dispatch code:
  - `services/consolidation.rs` uses `dc_bibo::DOCUMENT`.
  - `tools/semantic/triples.rs` imports `eso`, `fibo`, `golem` (lines 5-7)
    for RDF predicate → 5W1H dimension mapping.
- All 27 call sites use bare `execute_tool`.

### Why premature

Ontology tagging lives in **YAML** in the corpus processing pipeline,
not in Rust. The pipeline is still being debugged before being reified
in Rust. The ontology concepts (document types, extraction stages,
persona/narrative categories) are defined in the YAML config and
consumed by the pipeline at runtime — they do not have Rust-side
constants yet because the pipeline itself is not reified. Adding
Rust-side constants now would duplicate the YAML's vocabulary and
create a second source of truth that drifts.

### Scaffolding plan

The corpus server spans four ontology domains, one per tool family:

| Tool family                               | Ontology          | Concept                                     | Why                                                                  |
| ----------------------------------------- | ----------------- | ------------------------------------------- | -------------------------------------------------------------------- |
| Document processing (convert, chunk, OCR) | Dublin Core + PKO | `dc_bibo::TEXT` / `pko::STEP`               | Document conversion produces text; processing stages are PKO steps   |
| Knowledge extraction (triples, entities)  | ESO               | `eso::HAS_EVIDENCE` / `eso::HAS_HYPOTHESIS` | Triple extraction is epistemic — it produces evidence and hypotheses |
| Persona / narrative                       | GOLEM             | `golem::CREATIVE_WORK` / `golem::CHARACTER` | Persona building is narrative construction                           |
| Storage / query                           | Dublin Core       | `dc_bibo::DATASET`                          | Stored chunks are datasets                                           |

The plan:

1. **Map the YAML pipeline stages to bridge-crate constants as the
   pipeline is reified in Rust.** Each stage that moves from YAML to
   Rust gets a constant in `hkask-bridge-ontology` (or reuses an
   existing one — `dc_bibo::TEXT`, `pko::STEP`, `eso::HAS_EVIDENCE`,
   etc. are already defined). The mapping is a property of the
   reified stage, not of the YAML config.

2. **Add an `ontology_anchor` fn once the pipeline stages are stable.**
   The fn lives on `impl CorpusServer` in
   `kask/mcp-servers/hkask-mcp-corpus/src/hkask_mcp_corpus.rs` (where
   `combined_router` is). Each tool gets an arm returning the constant
   for its pipeline stage's domain.

3. **The corpus server's `tools/semantic/triples.rs` already imports
   `eso`, `fibo`, `golem`** (lines 5-7) — these are the right
   ontologies for the extraction tools. The `ontology_anchor` fn for
   the extraction tools should return `eso::HAS_EVIDENCE` (or
   `eso::HAS_HYPOTHESIS` for hypothesis-extraction tools), reusing
   the constants the module already imports.

4. **Add the coverage and stub-collapse tests** once the `ontology_anchor`
   fn is added. The stub-collapse test should assert that document
   processing tools (Dublin Core / PKO) and extraction tools (ESO) get
   distinct concepts — the two families are ontologically distinct and
   a stub regression would collapse them.

5. **Swap all 27 call sites** from `execute_tool` to
   `execute_tool_semantic` with `Self::ontology_anchor("name")`.

### Key reference

- `kask/mcp-servers/hkask-mcp-corpus/src/tools/semantic/triples.rs:5-7` —
  already imports `eso`, `fibo`, `golem` from the bridge crate. These
  are the right ontologies for the extraction tools.
- `kask/crates/hkask-bridge-ontology/src/pko.rs:114-125` —
  `corpus_stage_to_pko_step`, an existing stage → PKO concept mapping
  that can inform the `ontology_anchor` arms for document-processing
  tools.

### Trigger for migration

The corpus processing pipeline is reified in Rust (stages move from
YAML config to Rust types), and each stage has a stable ontology
concept. Until then, the YAML is the source of truth and a Rust-side
`ontology_anchor` would be a duplicate.

---

## 3c. `hkask-mcp-research` (21 tools)

### Current state

- **Does NOT depend on `hkask-bridge-ontology`.**
- All 21 call sites use bare `execute_tool`.
- No ontology work done.

### Why premature

Research should use **PKO** (scientific process) and **ESO** (epistemic
science ontology) for tagging search/extraction/synthesis tools. But the
mapping from research workflow stages to PKO/ESO concepts has not been
designed. The `pko::research_stage_to_pko` fn exists in
`kask/crates/hkask-bridge-ontology/src/pko.rs` (lines 128-139) but maps
_stage names_ (`hypothesis`, `search`, `extract`, `evaluate`,
`synthesize`, `curate`, `cite`), not _tool names_. The research server's
tools are the action surface of the scientific process, but the mapping
from a tool name (`web_search`, `rss_synthesize`) to a stage name
(`search`, `synthesize`) is not defined.

### Scaffolding plan

The research server's tools fall into three groups:

| Tool group            | Tools                                                         | Ontology    | Concept             | Why                                   |
| --------------------- | ------------------------------------------------------------- | ----------- | ------------------- | ------------------------------------- |
| Web search / browse   | `web_search`, `web_find_similar`, `web_extract`, `web_browse` | ESO         | `eso::HAS_EVIDENCE` | Search discovers evidence             |
| RSS / feed management | `rss_subscribe`, `rss_fetch`, `rss_get_entries`, etc.         | Dublin Core | `dc_bibo::DATASET`  | Feed management is dataset operations |
| Synthesis             | `rss_synthesize`, `rss_fetch_synthetic`                       | PKO         | `pko::PROCEDURE`    | Synthesis is a process                |

The plan:

1. **Add `hkask-bridge-ontology.workspace = true` to the research
   server's `Cargo.toml`.**

2. **Map research tool names to PKO/ESO concepts.** Use the existing
   `pko::research_stage_to_pko` (lines 128-139) as a reference for the
   stage → concept mapping. The tool → stage mapping is the new work:

   - `web_search` / `web_find_similar` / `web_extract` / `web_browse` →
     stage `search` → `pko::ACTION` (or `eso::HAS_EVIDENCE` for the
     epistemic axis).
   - `rss_synthesize` / `rss_fetch_synthetic` → stage `synthesize` →
     `pko::PROCEDURE_EXECUTION` (per `research_stage_to_pko`).
   - RSS feed tools → `dc_bibo::DATASET` (feed entries are datasets).

   The research server's tools are the "action" surface of the
   scientific process — PKO's `ACTION` and `PROCEDURE` concepts are
   the natural anchors. ESO's `HAS_EVIDENCE` / `HAS_HYPOTHESIS` are
   the natural anchors for the epistemic-axis tools (search,
   extraction).

3. **Add an `ontology_anchor` fn** on `impl ResearchServer` (wherever
   `combined_router` lives). The fn returns `Option<&'static str>`,
   one arm per tool, with a `dc_bibo::DATASET` fallback for tools that
   don't fit a specific stage.

4. **Add the coverage and stub-collapse tests.** The stub-collapse test
   should assert that web-search tools (ESO) and synthesis tools (PKO)
   get distinct concepts — the two families are ontologically distinct.

5. **Swap all 21 call sites** from `execute_tool` to
   `execute_tool_semantic` with `Self::ontology_anchor("name")`.

### Key reference

- `kask/crates/hkask-bridge-ontology/src/pko.rs:128-139` —
  `research_stage_to_pko`, the stage → PKO concept mapping. This is
  the starting point for mapping research _tool names_ to PKO concepts:
  first map tool → stage, then stage → concept (via this fn).
- `kask/crates/hkask-bridge-ontology/src/eso.rs` — ESO constants
  (`HAS_EVIDENCE`, `HAS_HYPOTHESIS`) for the epistemic-axis tools.

### Trigger for migration

The tool → stage mapping is designed. This is a design decision, not a
code change: someone needs to look at each of the 21 research tools and
decide which research stage it belongs to. Once that mapping exists
(even as a doc table), the `ontology_anchor` fn is mechanical to write.
Unlike media (workflows not built) and corpus (pipeline not reified),
research has no code-level blocker — only a design-level one.

---

## Summary

| Server               | Blocker                                                   | Trigger to unblock                                                 |
| -------------------- | --------------------------------------------------------- | ------------------------------------------------------------------ |
| `hkask-mcp-media`    | Workflows not built out (23/40 tools have no OMC mapping) | Media workflows are built so every tool has a clear OMC concept    |
| `hkask-mcp-corpus`   | Ontology lives in YAML, pipeline not reified in Rust      | Corpus pipeline stages move from YAML to Rust with stable concepts |
| `hkask-mcp-research` | Tool → stage mapping not designed                         | Design decision: map each of 21 tools to a research stage          |

None of these require new `OntologyNamespace` variants or new bridge-crate
modules. The ontologies (OMC, ESO, PKO, Dublin Core, GOLEM) are
sufficient; the gap is between the ontologies and the tools, not within
the ontologies themselves.
