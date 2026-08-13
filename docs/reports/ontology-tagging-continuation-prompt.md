# Continuation prompt: MCP ontology-tagging standardization

## Context

We standardized ontology tagging across the hKask MCP server fleet. The work is partially complete. This prompt picks up where we left off.

### What's done (committed + uncommitted working tree)

**6 servers fully standardized** (all tests pass, clippy clean):
- `hkask-mcp-prediction-markets` (32 tools) — SDMX for economic-data, Dublin Core for market tools
- `hkask-mcp-companies` (44 tools) — FIBO per-tool, delegates to `fibo::tool_to_ontology`
- `hkask-mcp-scenarios` (21 tools) — PKO for process tools, Dublin Core for computed outputs
- `hkask-mcp-portfolio` (14 tools) — FIBO per-tool (reference implementation)
- `hkask-mcp-swarm` (53 tools) — `pko::PROCEDURE` for all (was bare `"pko"` string, now typed constant)
- `hkask-mcp-kata-kanban` (23 tools) — already had `kanban_type_to_pko` mapping

**Bridge crate** (`hkask-bridge-ontology`):
- New `sdmx` module (7 SDMX concept constants)
- `OntologyNamespace::Sdmx` variant added to `axis.rs`
- `explain_tool_for` dispatch covers `sdmx:` prefix
- Dead `"dublin-core"` string-literal branch removed

**Framework** (`hkask-mcp-server`):
- `execute_tool_semantic` now emits `tracing::warn!` when ontology is `None` (algedonic channel)

**Tests added to every standardized server:**
- `tool_surface_is_exactly_N_registered_tools` (count pin)
- `ontology_anchor_covers_all_registered_tools` (coverage test — iterates router, asserts `Some` for every tool)
- `ontology_anchor_distinguishes_*` (stub-collapse regression — asserts distinct tool families get distinct concepts)
- `explain_tool_for_covers_all_ontology_namespaces` (in bridge crate — dispatch coverage for all 7 `OntologyNamespace` variants)

### What's in progress (codegraph migration started, not finished)

The `hkask-mcp-codegraph` migration was started but not completed:
- Import line changed: `execute_tool` → `execute_tool_semantic` (done)
- `ontology_anchor` fn: NOT added yet
- Call sites: NOT swapped yet (9 sites in `hkask_mcp_codegraph.rs`)
- Coverage test: NOT added yet
- Cargo.toml: `hkask-bridge-ontology` dependency NOT added yet

### The standardization pattern (apply this to each remaining server)

For each server:

1. **Add `hkask-bridge-ontology.workspace = true` to `Cargo.toml`** `[dependencies]`.

2. **Add an `ontology_anchor` fn** on the server's `impl` block (where `combined_router` lives):
```rust
fn ontology_anchor(tool: &str) -> Option<&'static str> {
    use hkask_bridge_ontology::{dc_bibo, pko, /* etc */ };
    match tool {
        "tool_name" => Some(pko::PROCEDURE),
        // ... one arm per tool ...
        _ => Some(dc_bibo::DATASET),  // fallback — never None for registered tools
    }
}
```

3. **Swap all call sites** from `execute_tool(self, "name", async {` to `execute_tool_semantic(self, "name", Self::ontology_anchor("name"), async {`. For multi-file servers, each `tools/*.rs` file's import must change from `execute_tool` to `execute_tool_semantic`.

4. **Add two tests** to the `tool_surface_tests` module:
```rust
#[test]
fn ontology_anchor_covers_all_registered_tools() {
    let router = ServerName::combined_router();  // or tool_router() for single-router servers
    for tool in router.list_all() {
        assert!(
            ServerName::ontology_anchor(&tool.name).is_some(),
            "ontology_anchor returned None for registered tool '{}'",
            tool.name
        );
    }
}

#[test]
fn ontology_anchor_distinguishes_tool_families() {
    // Assert at least 2 distinct tools get distinct concepts
    let a = ServerName::ontology_anchor("tool_a");
    let b = ServerName::ontology_anchor("tool_b");
    assert_ne!(a, b, "tool_a and tool_b must anchor on distinct concepts");
    // Assert specific concepts for key tools
}
```

5. **Run `cargo test -p <crate> --lib` and `./script/clippy -p <crate>`** to verify.

## Task 1: Finish the 3 actionable server migrations

### 1a. `hkask-mcp-codegraph` (9 tools, single file)

**Path:** `kask/mcp-servers/hkask-mcp-codegraph/src/hkask_mcp_codegraph.rs`
**Cargo.toml:** `kask/mcp-servers/hkask-mcp-codegraph/Cargo.toml` — needs `hkask-bridge-ontology.workspace = true` added
**Import:** Already changed to `execute_tool_semantic` (line 13)
**Status:** Import done, everything else NOT done.

Tools and recommended SUMO anchors (codegraph is a code-structure tool — SUMO is the upper ontology for entities/relations/processes):

| Tool | Anchor |
|---|---|
| `codegraph_query` | `sumo::ENTITY` |
| `codegraph_traverse` | `sumo::RELATION` |
| `codegraph_impact` | `sumo::RELATION` |
| `codegraph_analysis` | `sumo::PROCESS` |
| `codegraph_context` | `sumo::TEXT` (assembled context is a text representation) |
| `codegraph_structure` | `sumo::ENTITY` |
| `codegraph_stats` | `dc_bibo::DATASET` |
| `codegraph_reindex` | `sumo::PROCESS` |
| `codegraph_index_embeddings` | `sumo::REPRESENTATION` |

The `ontology_anchor` fn goes on the `impl CodeGraphServer` block (around line 213, before the `#[tool_router]` attribute). The 9 call sites are all `execute_tool(self, "name", async {` — swap each to `execute_tool_semantic(self, "name", Self::ontology_anchor("name"), async {`.

### 1b. `hkask-mcp-condenser` (4 tools, single file)

**Path:** `kask/mcp-servers/hkask-mcp-condenser/src/hkask_mcp_condenser.rs`
**Cargo.toml:** `kask/mcp-servers/hkask-mcp-condenser/Cargo.toml` — needs `hkask-bridge-ontology.workspace = true` added
**Import:** `use hkask_mcp_server::server::{McpToolError, execute_tool};` — change to `execute_tool_semantic`

Tools and recommended PKO anchors (condensation is a knowledge-production process):

| Tool | Anchor |
|---|---|
| `condenser_ping` | `dc_bibo::DATASET` (liveness/status) |
| `condenser_persist` | `pko::PROCEDURE_EXECUTION` |
| `condenser_thread_summary` | `pko::PROCEDURE` |
| `condenser_score_saliency` | `pko::PROCEDURE` |

### 1c. `hkask-mcp-training` (8 tools, multi-file)

**Path:** `kask/mcp-servers/hkask-mcp-training/src/`
**Cargo.toml:** `kask/mcp-servers/hkask-mcp-training/Cargo.toml` — needs `hkask-bridge-ontology.workspace = true` added
**Structure:** Tools split across `tools/{cancel,dataset,evaluate,status,submit,validate}.rs`. The `ontology_anchor` fn goes on `impl TrainingServer` in `hkask_mcp_training.rs` (where `combined_router` is at line 297). Each `tools/*.rs` file imports `execute_tool` — each must change to `execute_tool_semantic`.

Tools and recommended ML-Schema anchors:

| Tool | File | Anchor |
|---|---|---|
| `training_cancel` | `tools/cancel.rs` | `mlschema::RUN` |
| `training_ingest_qa` | `tools/dataset.rs` | `mlschema::DATA` |
| `training_assemble_dataset` | `tools/dataset.rs` | `mlschema::DATA` |
| `training_ingest_dataset` | `tools/dataset.rs` | `mlschema::DATA` |
| `training_evaluate` | `tools/evaluate.rs` | `mlschema::MODEL` |
| `training_status` | `tools/status.rs` | `mlschema::RUN` |
| `training_submit` | `tools/submit.rs` | `mlschema::RUN` |
| `training_validate_config` | `tools/validate.rs` | `mlschema::MODEL` |

ML-Schema constants in `hkask-bridge-ontology/src/mlschema.rs`: `MODEL = "mls:Model"`, `RUN = "mls:Run"`, `DATA = "mls:Data"`.

## Task 2: Curator meta-mapping proposal

Write a proposal to `docs/reports/curator-ontology-meta-mapping-proposal.md`.

The curator server (`kask/mcp-servers/hkask-mcp-curator/src/`) doesn't need its own `ontology_anchor` fn in the same sense as the others. Instead, it needs **meta-level capabilities** for the ontology-tagging fleet:

### The problem

We now have 10+ MCP servers each with an `ontology_anchor` fn mapping tool names to concept URIs. The `explain_tool_for` dispatch in `hkask-bridge-ontology` routes concepts to explain-tool names. But there's no fleet-level observability or evaluation:

1. **No consumption**: The curator can't see which ontology concepts are being used across the fleet, which tools are unanchored (the `tracing::warn!` we added fires but nothing aggregates it), or which dispatch routes are firing.

2. **No maintenance**: When a server adds a new tool, the `ontology_anchor_covers_all_registered_tools` test catches missing anchors within that crate. But there's no cross-crate view: the curator can't detect that server A's anchor for a concept conflicts with server B's, or that a new `OntologyNamespace` variant was added without an `explain_tool_for` arm.

3. **No evaluation**: Does the ontology tagging actually improve widget dispatch quality? Are the `explain_tool_for` routes landing on the right tools? Nobody knows — the loop is open.

### What to propose

The proposal should sketch:

1. **A curator tool** (`curator_ontology_audit`) that reads `reg.tool` spans from the Regulation trace and reports:
   - Which ontology concepts are being used across the fleet
   - Which tools are unanchored (ontology field empty on the span)
   - Which `explain_tool_for` dispatch routes are firing vs falling through to the fallback
   - Per-server anchor coverage (how many tools have specific vs fallback anchors)

2. **A curator tool** (`curator_ontology_drift`) that detects cross-crate drift:
   - A tool name that appears in multiple servers with different anchors
   - An `OntologyNamespace` variant without an `explain_tool_for` dispatch decision
   - A server whose `ontology_anchor` returns concepts that `explain_tool_for` routes to a different server's tools (cross-server dispatch)

3. **An evaluation loop** that closes the feedback:
   - When a widget's "Explain" affordance fires, does the dispatched tool actually help?
   - Track explain-tool invocation success vs failure
   - Feed back to `ontology_anchor` fns: if a concept consistently routes to a tool that doesn't help, the anchor is wrong

The proposal should reference the cybernetics review's findings (blocked algedonic channel, absent S4, open loop) and explain how the curator tools address each. It should cite the specific files and mechanisms:
- `kask/crates/hkask-mcp-server/src/server/tool_span.rs` — the `reg.tool` span emission
- `kask/crates/hkask-bridge-ontology/src/hkask_bridge_ontology.rs` — `explain_tool_for`
- `kask/crates/hkask-bridge-ontology/src/axis.rs` — `OntologyNamespace`, `select_ontology_anchor`
- The `tracing::warn!` we added in `execute_tool_semantic` for `None` ontology

The proposal should NOT implement anything — it's a design document for review.

## Task 3: Media, corpus, and research scaffolding prompt

These 3 servers are premature for full ontology-tagging migration because the ontology work isn't ready or the server isn't built out. Instead of migrating, compose a scaffolding plan for each.

### 3a. `hkask-mcp-media` (40 tools)

**Current state:** Already depends on `hkask-bridge-ontology`. Has `omc::tool_to_omc(tool: &str) -> Option<OmcConcept>` in `kask/crates/hkask-bridge-ontology/src/omc.rs` covering 17/40 tools with typed OMC constants. The remaining 23 tools return `None`. All 40 call sites use bare `execute_tool`.

**Why premature:** The media generation ontology (OMC — MovieLabs Ontology for Media Creation) exists in the bridge crate, but the media workflows aren't built out. The remaining 23 tools don't have clear OMC mappings because the workflows they'd map to don't exist yet. Forcing anchors now would be speculative.

**Scaffolding plan:** The OMC module (`omc.rs`) already has the right constants (`SCENE`, `ASSET`, `CREATIVE_WORK`, `VERSION`, `MEDIA_SOURCE`, `SEQUENCE`). The `tool_to_omc` fn is the right shape. The plan should:
- Extend `tool_to_omc` to cover all 40 tools as the workflows are built out
- Wire `tool_to_omc` as the server's `ontology_anchor` (same delegation pattern as companies uses `fibo::tool_to_ontology`)
- Add the coverage test once all 40 tools are mapped
- The fallback for unmapped tools should be `omc::CREATIVE_WORK` (all media tools produce creative works) rather than `dc_bibo::DATASET`

**Key reference:** `kask/crates/hkask-bridge-ontology/src/omc.rs` — the OMC concept vocabulary and `tool_to_omc` mapping (17 tools mapped, 23 returning `None`).

### 3b. `hkask-mcp-corpus` (27 tools)

**Current state:** Already depends on `hkask-bridge-ontology`. Uses typed constants in non-dispatch code (`services/consolidation.rs` uses `dc_bibo::DOCUMENT`, `tools/semantic/triples.rs` uses `eso`, `fibo`, `golem`). All 27 call sites use bare `execute_tool`.

**Why premature:** Ontology tagging lives in YAML in the corpus processing pipeline, not in Rust. The pipeline is still being debugged before being reified in Rust. The ontology concepts (document types, extraction stages, persona/narrative categories) are defined in the YAML config and consumed by the pipeline at runtime — they don't have Rust-side constants yet because the pipeline itself isn't reified.

**Scaffolding plan:** The corpus server spans multiple ontologies:
- Document processing → `dc_bibo::TEXT` / `pko::STEP` (document conversion and processing are text-producing steps)
- Knowledge extraction → `eso::HAS_EVIDENCE` / `eso::HAS_HYPOTHESIS` (triple extraction is epistemic)
- Persona/narrative → `golem::CREATIVE_WORK` / `golem::CHARACTER` (persona building is narrative)
- Storage/query → `dc_bibo::DATASET` (stored chunks are datasets)

The plan should:
- Map the YAML pipeline stages to bridge-crate constants as the pipeline is reified in Rust
- Add an `ontology_anchor` fn once the pipeline stages are stable
- The corpus server's `tools/semantic/triples.rs` already imports `eso`, `fibo`, `golem` — these are the right ontologies for the extraction tools

**Key reference:** `kask/mcp-servers/hkask-mcp-corpus/src/tools/semantic/triples.rs` lines 5-7 — already imports `eso`, `fibo`, `golem` from the bridge crate.

### 3c. `hkask-mcp-research` (21 tools)

**Current state:** Does NOT depend on `hkask-bridge-ontology`. All 21 call sites use bare `execute_tool`. No ontology work done.

**Why premature:** Research should use PKO (scientific process) and ESO (epistemic science ontology) for tagging search/extraction/synthesis tools. But the mapping from research workflow stages to PKO/ESO concepts hasn't been designed. The `pko::research_stage_to_pko` fn exists in `kask/crates/hkask-bridge-ontology/src/pko.rs` (lines 128-139) but maps *stage names* (hypothesis, search, extract, evaluate, synthesize, curate, cite), not *tool names*.

**Scaffolding plan:** The research server's tools fall into two groups:
- Web search/browse tools (`web_search`, `web_find_similar`, `web_extract`, `web_browse`) → `eso::HAS_EVIDENCE` (search discovers evidence)
- RSS/feed tools (`rss_subscribe`, `rss_fetch`, `rss_get_entries`, etc.) → `dc_bibo::DATASET` (feed management is dataset operations)
- Synthesis tools (`rss_synthesize`, `rss_fetch_synthetic`) → `pko::PROCEDURE` (synthesis is a process)

The plan should:
- Add `hkask-bridge-ontology` dependency
- Map research tool names to PKO/ESO concepts using the existing `pko::research_stage_to_pko` as a reference for stage-to-concept mapping
- The research server's tools are the "action" surface of the scientific process — PKO's `ACTION` and `PROCEDURE` concepts are the natural anchors

**Key reference:** `kask/crates/hkask-bridge-ontology/src/pko.rs` lines 128-139 — `research_stage_to_pko` maps research workflow stages to PKO concepts. This is the starting point for mapping research *tool names* to PKO concepts.

## Verification

After completing Tasks 1a-1c, run:
```sh
cargo test -p hkask-mcp-codegraph -p hkask-mcp-condenser -p hkask-mcp-training --lib
./script/clippy -p hkask-mcp-codegraph -p hkask-mcp-condenser -p hkask-mcp-training
```

All tests must pass, clippy must be clean, `cargo-machete` must find no unused deps.

After completing Task 2, the proposal should be at `docs/reports/curator-ontology-meta-mapping-proposal.md`.

After completing Task 3, the scaffolding plans should be sections in the same proposal or a separate document at `docs/reports/ontology-scaffolding-media-corpus-research.md`.

## Key files to reference

- `kask/crates/hkask-bridge-ontology/src/hkask_bridge_ontology.rs` — `explain_tool_for`, module list
- `kask/crates/hkask-bridge-ontology/src/axis.rs` — `OntologyNamespace`, `select_ontology_anchor`
- `kask/crates/hkask-bridge-ontology/src/pko.rs` — PKO constants, `research_stage_to_pko`
- `kask/crates/hkask-bridge-ontology/src/sumo.rs` — SUMO constants
- `kask/crates/hkask-bridge-ontology/src/mlschema.rs` — ML-Schema constants
- `kask/crates/hkask-bridge-ontology/src/omc.rs` — OMC constants, `tool_to_omc`
- `kask/crates/hkask-mcp-server/src/server/tool_span.rs` — `execute_tool_semantic`, `tracing::warn!` on `None`
- `kask/mcp-servers/hkask-mcp-prediction-markets/src/hkask_mcp_prediction_markets.rs` — reference implementation of the pattern (ontology_anchor fn, coverage test, stub-collapse regression test)
- `kask/mcp-servers/hkask-mcp-companies/src/hkask_mcp_companies.rs` — reference for delegation pattern (ontology_anchor delegates to fibo::tool_to_ontology)
- `docs/reports/mcp-ontology-tagging-proposal.md` — the original standardization proposal (implemented)
