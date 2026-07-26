# Phase 1 — Functional Role Discovery (graph-audit, dual mode)

> Code mode executed by direct code tracing (the `hkask-mcp-codegraph` MCP server
> is not available in this session; file reads + grep substitute). Semantic
> mode executed by constraint-force classification per pragmatic-semantics.

## T1 — Replica YAML cluster

### Artifacts
- `kask/corpus/replica/company-researcher.yaml`
- `kask/corpus/replica/john-brooks.yaml`

### Code mode — consumer graph

**Entry point (MCP tool):**
`kask/mcp-servers/hkask-mcp-corpus/src/tools/persona/mod.rs::corpus_build_persona`
(lines ~272–330). Takes `Parameters<BuildRequest>` where `BuildRequest.config_path`
is a YAML file path. Validates the path exists, then delegates to:

**Service:**
`kask/crates/hkask-services-corpus/src/embed/service.rs::EmbedService::embed_corpus`
(line 34). Reads the file, deserializes:

```rust
let config: CorpusConfig = serde_yaml_neo::from_str(&config_str).map_err(...)?;
```

**Contract of record:**
`kask/crates/hkask-services-corpus/src/embed/types.rs::CorpusConfig` (line ~88).
Fields consumed downstream:
- `config.author` → `author_prefix = "style:{author}:"` (service.rs:67)
- `config.centroid_entity_ref` → `centroid_ref` (service.rs:71) — **required, no `#[serde(default)]`**. Used at service.rs:703, 705, 725, 845, 857, 894.
- `config.embedding.dim` → `SemanticMemory::open(db_path, passphrase, dim)` (service.rs:108) — required.
- `config.embedding.model` → `centroid_store.store(ref, vec, model)` (service.rs:845).
- `config.works` → iterated (service.rs:130 `for work in config.works.iter()`). Each `Work` requires `title`, `slug`, `url`, plus `local_path`/`format`/`document_type`/`dimensions`/`section_types`/`mds_categories` with defaults.
- `config.foundational_rules` → iterated as passages.
- `config.chunking` (`min_words`, `max_words`, `sentence_boundary`) — all required, no defaults.
- `config.validation` → `ValidationConfig { centroid_distance_max, exemplar_count_min, exemplar_count_max }` — all required, no defaults. **`per_dimension` is not a field.**
- `config.budget` → `BudgetConfig` enum (Flat/PerPage/Absolute), `#[serde(default)]`.
- `config.entities` → `EntityConfig` with `characters`/`places`/`events`/`concepts`/`co_authors`/`venues`/`topics`/`paradigms`, each `Vec<Entity>` where `Entity { name, appears_in }`. **Bare strings fail.**
- `config.methods` → `Vec<DeclaredMethod>` where `DeclaredMethod { name, description, signal: MethodThresholds, threshold: Option<f64> }`. **`threshold: {}` fails — `{}` is not a valid `Option<f64>`.**
- `config.dimension_centroids` → `Vec<DimensionCentroid>` where `DimensionCentroid { name, ref_name, weight, description }`. **`dimension` (not `name`) and missing `ref_name`/`weight` fail.** Used at service.rs:758, 828, 876 (accesses `.name`, `.ref_name`, `.weight`).

**Critical serde fact:** `CorpusConfig` and all its nested structs have **NO `#[serde(deny_unknown_fields)]`**. Standard serde behavior is to **silently ignore unknown fields**. Therefore:
- Unknown fields (`embedding.centroid_entity_ref`, `validation.per_dimension`, `budget.mode`, `budget.triple_budget_per_100`, `dimension_centroids[].dimension`) are **silently dropped** — they do NOT cause a parse error.
- **Missing required fields without `#[serde(default)]`** DO cause a parse error. `centroid_entity_ref` is required (no default) — `company-researcher.yaml` nests it under `embedding`, so it is missing at top level → **parse fails with "missing field `centroid_entity_ref`"**.
- `validation` requires `exemplar_count_min` and `exemplar_count_max` (no defaults) — `company-researcher.yaml` omits both → **parse fails with "missing field `exemplar_count_min`"** (and `exemplar_count_max`).
- `chunking` requires `min_words`, `max_words`, `sentence_boundary` — both YAMLs provide these. OK.
- `embedding` requires `model`, `dim`, `batch_size` — both YAMLs provide these. OK.
- `works` is `Vec<Work>` with no default — required. `company-researcher.yaml` has `works: []` (empty, satisfies the requirement). `john-brooks.yaml` has one work. OK.
- `foundational_rules` is `Vec<FoundationalRule>` with no default — required. `company-researcher.yaml` **omits `foundational_rules`** → **parse fails with "missing field `foundational_rules`"**.

**Conclusion (code mode):** `company-researcher.yaml` will fail to deserialize
on at least three independent missing-required-field errors:
1. `centroid_entity_ref` (nested under `embedding`, dropped as unknown there, missing at top level)
2. `validation.exemplar_count_min` and `validation.exemplar_count_max` (omitted)
3. `foundational_rules` (omitted)

Plus silent shape drift on `dimension_centroids` (uses `dimension` not `name`,
missing `ref_name`/`weight`), `entities.concepts` (bare strings vs `Entity` map),
`methods[].threshold` (`{}` vs `Option<f64>`), and `budget` (unknown `mode`/`triple_budget_per_100` dropped).

`john-brooks.yaml` provides all required fields and uses correct shapes — it
should parse. It is the de-facto reference instance.

### Semantic mode — edge classification

Edges (artifact field → consumer field), classified by constraint force
(Prohibition = must-not-violate / hard contract; Guardrail = should-not-violate;
Guideline = convention; Evidence = observed usage; Hypothesis = unverified):

**`company-researcher.yaml`:**

| Edge | Force | Finding |
|------|-------|---------|
| `author` → `CorpusConfig.author` | Evidence | OK, present. |
| `embedding.model` → `EmbeddingConfig.model` | Evidence | OK. |
| `embedding.dim` → `EmbeddingConfig.dim` | Evidence | OK. |
| `embedding.batch_size` → `EmbeddingConfig.batch_size` | Evidence | OK. |
| `embedding.centroid_entity_ref` → (nothing) | **Prohibition** | Unknown field under `embedding` — silently dropped. Top-level `centroid_entity_ref` missing → parse failure. |
| `chunking.*` → `ChunkingConfig.*` | Evidence | OK. |
| `validation.centroid_distance_max` → `ValidationConfig.centroid_distance_max` | Evidence | OK. |
| `validation.per_dimension` → (nothing) | **Guardrail** | Unknown field, silently dropped. Indicates stale schema (pre-refactor). |
| `validation.exemplar_count_min/max` → (missing) | **Prohibition** | Required fields omitted → parse failure. |
| `budget.mode` → (nothing) | **Guardrail** | Unknown field, dropped. `BudgetConfig::Absolute` has no `mode` tag. |
| `budget.max_triples` → `BudgetConfig::Absolute.max_triples` | Evidence | OK shape, but `mode`/`triple_budget_per_100` are dropped noise. |
| `budget.triple_budget_per_100` → (nothing) | **Guardrail** | Unknown field, dropped. |
| `entities.concepts` (bare strings) → `EntityConfig.concepts: Vec<Entity>` | **Prohibition** | Type mismatch — bare string cannot deserialize into `Entity { name, appears_in }`. Parse failure. |
| `methods[].threshold: {}` → `DeclaredMethod.threshold: Option<f64>` | **Prohibition** | `{}` is not a valid `Option<f64>`. Parse failure. |
| `methods[].signal: {}` → `DeclaredMethod.signal: MethodThresholds` | Evidence | `MethodThresholds` is `Default`, so `{}` is OK. |
| `dimension_centroids[].dimension` → (nothing) | **Prohibition** | Unknown field, dropped. Required `name` missing → if `dimension_centroids` were non-empty, parse failure. But the entries also lack `ref_name` and `weight` (required, no defaults). |
| `dimension_centroids[].description` → `DimensionCentroid.description` | Evidence | OK (has `#[serde(default)]`). |
| `works: []` → `CorpusConfig.works` | Evidence | OK (empty satisfies required Vec). |
| `foundational_rules` (omitted) → `CorpusConfig.foundational_rules` | **Prohibition** | Required, no default → parse failure. |

**`john-brooks.yaml`:**

| Edge | Force | Finding |
|------|-------|---------|
| `author` → `CorpusConfig.author` | Evidence | OK. |
| `author_full` → (nothing) | Guideline | Unknown field, dropped. Cosmetic; no consumer. |
| `embedding.*` → `EmbeddingConfig.*` | Evidence | OK. |
| `works[0]` → `Work` | Evidence | OK (`local_path`, `format` provided; `url: ""` OK). |
| `foundational_rules[0]` → `FoundationalRule` | Evidence | OK. |
| `chunking.*` → `ChunkingConfig.*` | Evidence | OK. |
| `centroid_entity_ref` (top-level) → `CorpusConfig.centroid_entity_ref` | Evidence | OK — correct placement. |
| `validation.*` → `ValidationConfig.*` | Evidence | OK — all three required fields present. |
| `budget: { max_triples: 0 }` → `BudgetConfig::Absolute` | Evidence | OK. |
| `entities.concepts: [{name:...}]` → `EntityConfig.concepts: Vec<Entity>` | Evidence | OK — correct `Entity` shape. |
| `methods: []` → `CorpusConfig.methods` | Evidence | OK. |
| `dimension_centroids[].{name,ref_name,weight,description}` → `DimensionCentroid` | Evidence | OK — all required fields present. |

### Structural pathologies detected

- **Gap (Prohibition):** `company-researcher.yaml` omits 3 required top-level
  fields (`centroid_entity_ref`, `validation.exemplar_count_min/max`,
  `foundational_rules`). The consumer cannot deserialize it.
- **Shape mismatch (Prohibition):** `entities.concepts` bare strings,
  `methods[].threshold: {}`, `dimension_centroids[].dimension` — all type
  mismatches against the current `CorpusConfig`.
- **Orphan fields (Guardrail):** `validation.per_dimension`, `budget.mode`,
  `budget.triple_budget_per_100`, `author_full`, `embedding.centroid_entity_ref`
  — fields the consumer no longer (or never did) recognizes. Indicate schema
  drift between YAML authoring and `CorpusConfig` evolution.
- **Redundancy (Guideline):** The two YAMLs use **different shapes** for the
  same logical fields (`entities.concepts`, `dimension_centroids`, `budget`).
  At least one is wrong; the consumer says `company-researcher.yaml` is wrong.
- **No cycle:** The artifact → consumer → artifact loop is open (config is
  read once at start; no template renders a config that selects the template).

### Functional-role statement (T1)

> **`kask/corpus/replica/company-researcher.yaml`** and **`kask/corpus/replica/john-brooks.yaml`**
> are persona configuration files consumed by
> `hkask_services_corpus::EmbedService::embed_corpus` (via the
> `corpus_build_persona` MCP tool) as the `CorpusConfig` deserialization input.
> They must conform to the `CorpusConfig` struct in
> `kask/crates/hkask-services-corpus/src/embed/types.rs`: provide `author`,
> `embedding.{model,dim,batch_size}`, `works`, `foundational_rules`,
> `chunking.{min_words,max_words,sentence_boundary}`, top-level
> `centroid_entity_ref`, `validation.{centroid_distance_max,exemplar_count_min,exemplar_count_max}`,
> and optionally `budget`, `entities`, `methods`, `dimension_centroids` (each
> with the documented shapes). `john-brooks.yaml` conforms;
> `company-researcher.yaml` does not — it omits 3 required fields and uses
> stale shapes for 4 optional fields. The contract is one-shot (read at
> embed start); there is no feedback loop from the consumer back to the
> artifact.

---

## T2 — Pipeline manifest runbook

### Artifact
- `kask/corpus/pipeline-capabilities-researcher.yaml` (368 lines)

### Code mode — consumer graph

**No Rust consumer.** Verified by:
- `grep -rln "capabilities-researcher-pipeline|corpus/pipeline|PipelineManifest|type: pipeline" --include="*.rs"` → only unrelated "scenario pipeline" matches in `hkask-mcp-scenarios` and `kask_panel` (scenario forecasting, not corpus).
- The file's own header says "All pipeline steps run through MCP corpus tools" via CLI (`kask mcp invoke --server corpus --tool corpus_* --input '{...}'`). It is a **human-run runbook**, not a Rust-loaded manifest.

**Referenced MCP tools (all exist as Rust entry points):**

| Runbook `tool:` | Rust entry point |
|-----------------|------------------|
| `corpus_convert` | `kask/mcp-servers/hkask-mcp-corpus/src/tools/document.rs::corpus_convert` |
| `corpus_chunk` | (in `hkask-mcp-corpus`) |
| `corpus_purge_qa` | (in `hkask-mcp-corpus`) |
| `corpus_tag_chunks` | (in `hkask-mcp-corpus`) |
| `corpus_embed` | (in `hkask-mcp-corpus`) |
| `corpus_extract_triples` | (in `hkask-mcp-corpus`) |
| `corpus_consolidate_chunks` | (in `hkask-mcp-corpus`) |
| `corpus_dedup_chunks` | (in `hkask-mcp-corpus`) |
| `corpus_build_prompts` | (in `hkask-mcp-corpus`) |
| `corpus_generate_qa_batch` | (in `hkask-mcp-corpus`) |
| `corpus_ingest_qa` | (in `hkask-mcp-corpus`) |
| `training_assemble_dataset` | `kask/mcp-servers/hkask-mcp-training/src/tools/dataset.rs` |
| `training_submit` | `kask/mcp-servers/hkask-mcp-training/` |

The runbook is therefore a **coordination artifact over real Rust tools** —
its correctness matters (it tells an operator which tools to call in which
order with which params), but there is no Rust data contract to drift
against. Defects are runbook-logic defects (wrong tool name, wrong param,
stale model name, stale path), not contract violations.

### Semantic mode — edge classification

Edges (runbook value → referent), classified by constraint force:

| Value | Referent | Force | Finding |
|-------|----------|-------|---------|
| `HKASK_CLASSIFIER_MODEL: "DI/Qwen/Qwen3-235B-A22B-Instruct-2507"` | `kask/.env` | **Hypothesis** | Must verify the var exists in `.env` and the model name matches. |
| `HKASK_QA_MODEL: "DI/zai-org/GLM-5.2"` | `kask/.env` | **Hypothesis** | Same. |
| `HKASK_EMBEDDING_MODEL: "DI/Qwen/Qwen3-Embedding-0.6B"` | `kask/.env` | **Hypothesis** | Same. |
| `HKASK_EMBEDDING_DIM: "1024"` | `EmbeddingConfig.dim` / `john-brooks.yaml` | Evidence | Matches `john-brooks.yaml` `dim: 1024`. |
| `path: "/home/mdz-axolotl/Clones/Library/Researcher"` | operator filesystem | **Hypothesis** | Absolute path — operator-local. Must confirm corpus still lives there. |
| `db_path: "corpus/memory/john-brooks.db"` | `john-brooks.yaml` usage | Evidence | Consistent with `john-brooks.yaml` header. |
| `max_tokens: 512` (marked UNVALIDATED) | `corpus_chunk` tool param | **Hypothesis** | Runbook self-documents as a tunable hypothesis. Must confirm no Rust default supersedes. |
| `dedup_threshold: 0.89` | `corpus_dedup_chunks` param | **Hypothesis** | Provenance unknown. |
| `base_model: "unsloth/Qwen3.6-27B"` | `training_submit` param | **Hypothesis** | Must confirm model is still approved/available. |
| `lora.init_lora_weights: eva` | `training_submit` `TrainingParams` | **Hypothesis** | Must confirm `eva` is a valid enum variant in `TrainingParams`. |
| `tool: corpus_*` (13 tools) | `hkask-mcp-corpus` Rust tools | Evidence | All 13 tool names match Rust entry points (verified above). |
| `verify: { field, min }` blocks | (no consumer) | **Guardrail** | The `verify:` keys are runbook-level assertions, not enforced by Rust. Open-loop: no Rust code reads or checks them. |

### Structural pathologies detected

- **Open loop (Guardrail):** The runbook's `verify:` blocks are not enforced
  by any Rust consumer. If a tool returns fewer than `min` items, nothing in
  Rust stops the pipeline — the operator must notice. This is a runbook
  discipline gap, not a Rust bug.
- **Hardcoded absolute path (Hypothesis):** `/home/mdz-axolotl/Clones/Library/Researcher`
  is operator-local. Acceptable in a runbook, but stale if the corpus moved.
- **Unvalidated tunables (Hypothesis):** `max_tokens: 512`, `dedup_threshold: 0.89`
  are explicitly self-documented as hypotheses. No Rust default has been
  established to supersede them (to be confirmed in Phase 3).
- **No orphan/redundancy/cycle:** The runbook does not render config that
  selects the runbook.

### Functional-role statement (T2)

> **`kask/corpus/pipeline-capabilities-researcher.yaml`** is a **human-run
> pipeline runbook** (not a Rust-loaded manifest). It coordinates 13 MCP
> tools (12 in `hkask-mcp-corpus`, 1 in `hkask-mcp-training`) across 9 phases
> to build a `john-brooks` persona corpus and train an EVA-initialized LoRA
> adapter. It has **no Rust data contract** — no Rust code parses it. Its
> correctness is defined by (a) the tool names matching real Rust entry
> points (verified — all 13 match), (b) the `params:` matching each tool's
> input schema (to be audited in Phase 2–3), and (c) the hardcoded model
> names and paths matching `.env` and the operator environment (to be
> verified in Phase 3). Its `verify:` blocks are runbook-level assertions
> with no Rust enforcement (open loop).

---

## Checkpoint C1

- [x] T1 functional-role statement produced (replica cluster).
- [x] T2 functional-role statement produced (pipeline runbook).
- [ ] `cargo check -p hkask-services-corpus` — deferred to Phase 4 (T7 probe) to avoid redundant builds; the consumer code is unchanged in Phase 1.
- [x] Human review point: the Prohibition-tier findings on `company-researcher.yaml` (3 missing required fields + 4 shape mismatches) are the basis for Phase 4.
