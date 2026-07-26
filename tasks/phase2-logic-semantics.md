# Phase 2 — Logic & Semantics Audit

> Skills composed: pragmatic-semantics, pragmatic-cybernetics, essentialist
> (advisory). All findings carry constraint-force labels:
> **Prohibition** (hard contract violation) / **Guardrail** (should-not) /
> **Guideline** (convention) / **Evidence** (observed usage) /
> **Hypothesis** (unverified).

## T3 — Replica YAML cluster

### pragmatic-semantics

#### `company-researcher.yaml`

| Field / value | Ontological (IS / OUGHT) | Epistemic | Provenance | Confidence | Finding |
|---------------|--------------------------|----------|------------|------------|---------|
| `author: "company-researcher"` | IS = OUGHT | declarative | Artifact header | 1.0 | OK. |
| `embedding.model: "DI/Qwen/Qwen3-Embedding-0.6B"` | IS = OUGHT | declarative | `kask/.env::HKASK_EMBEDDING_MODEL` (verified match) | 1.0 | OK. |
| `embedding.dim: 1024` | IS = OUGHT | declarative | `john-brooks.yaml` `dim: 1024`; runbook `HKASK_EMBEDDING_DIM: "1024"` | 1.0 | OK. |
| `embedding.batch_size: 50` | IS ≠ OUGHT (no consumer constraint on value) | declarative | Unknown — no spec, no Rust constant. `john-brooks.yaml` uses 25. | 0.3 | **Hypothesis** — value is plausible but unjustified. |
| `embedding.centroid_entity_ref` | IS ≠ OUGHT (wrong location) | declarative | Stale schema (pre-refactor) | 0.9 | **Prohibition** — must be top-level. |
| `chunking.min_words: 40`, `max_words: 150`, `sentence_boundary: ".!? "` | IS = OUGHT | declarative | `john-brooks.yaml` identical | 1.0 | OK. |
| `validation.centroid_distance_max: 0.40` | IS = OUGHT | declarative | `john-brooks.yaml` identical; `discover/config.rs:119` uses 0.25 (different default for discovery, not persona) | 0.9 | OK. |
| `validation.per_dimension: true` | IS ≠ OUGHT (unknown field) | declarative | Stale schema | 0.9 | **Guardrail** — silently dropped; indicates drift. |
| `validation.exemplar_count_min/max` (omitted) | IS ≠ OUGHT (missing) | declarative | `ValidationConfig` requires both | 1.0 | **Prohibition** — parse failure. |
| `budget.mode: "absolute"` | IS ≠ OUGHT (unknown) | declarative | Stale schema (`BudgetConfig::Absolute` is untagged) | 0.9 | **Guardrail** — dropped. |
| `budget.max_triples: 25000` | IS = OUGHT (shape) | declarative | Unknown justification; `john-brooks.yaml` uses 0. | 0.3 | **Hypothesis** — value unjustified. |
| `budget.triple_budget_per_100: 100` | IS ≠ OUGHT (unknown) | declarative | Stale schema | 0.9 | **Guardrail** — dropped. |
| `entities.concepts` (bare strings) | IS ≠ OUGHT (type mismatch) | declarative | Stale schema (`Entity { name, appears_in }`) | 0.9 | **Prohibition** — parse failure. |
| `methods[].threshold: {}` | IS ≠ OUGHT (type mismatch) | declarative | Stale schema (`Option<f64>`) | 0.9 | **Prohibition** — parse failure. |
| `dimension_centroids[].dimension` | IS ≠ OUGHT (wrong key) | declarative | Stale schema (`name`) | 0.9 | **Prohibition** — missing `name`/`ref_name`/`weight`. |
| `works: []` | IS = OUGHT | declarative | Artifact comment ("draws from pre-built corpus") | 0.7 | **Hypothesis** — if `works` is empty, `embed_corpus` iterates zero works and produces no passages. The persona may not actually build. |
| `foundational_rules` (omitted) | IS ≠ OUGHT (missing) | declarative | `CorpusConfig` requires | 1.0 | **Prohibition** — parse failure. |

**Conflict resolution (5-tier OT ranking):** The `CorpusConfig` struct is
the **Core ontology tier** (it is the compiled contract). The YAMLs are
**Domain Supplement**. Where they conflict, Core wins. All Prohibition
findings above are Core-tier violations.

#### `john-brooks.yaml`

| Field / value | Ontological | Epistemic | Provenance | Confidence | Finding |
|---------------|-------------|-----------|------------|------------|---------|
| `author: "john-brooks"` | IS = OUGHT | declarative | Artifact header | 1.0 | OK. |
| `author_full` | IS ≠ OUGHT (unknown) | declarative | Cosmetic; no consumer | 0.9 | **Guideline** — dropped, harmless. |
| `embedding.batch_size: 25` | IS = OUGHT | declarative | Unknown; differs from `company-researcher.yaml` (50) | 0.3 | **Hypothesis** — unjustified. |
| `works[0].local_path: "corpus/extracted/researcher"` | IS = OUGHT | declarative | Runbook Phase 0 output | 0.9 | OK. |
| `foundational_rules[0]` | IS = OUGHT | declarative | Artifact | 1.0 | OK. |
| `centroid_entity_ref: "style:john-brooks:centroid"` (top-level) | IS = OUGHT | declarative | Correct placement | 1.0 | OK. |
| `validation.{centroid_distance_max: 0.40, exemplar_count_min: 100, exemplar_count_max: 10000}` | IS = OUGHT | declarative | All required fields present | 1.0 | OK. |
| `budget: { max_triples: 0 }` | IS = OUGHT | declarative | `BudgetConfig::Absolute`; 0 = no triples | 0.9 | OK. |
| `entities.concepts: [{name:...}]` | IS = OUGHT | declarative | Correct `Entity` shape | 1.0 | OK. |
| `dimension_centroids[].{name,ref_name,weight,description}` | IS = OUGHT | declarative | Correct `DimensionCentroid` shape; weights sum to 1.0 | 1.0 | OK. |

### pragmatic-cybernetics

**Loop:** artifact (YAML) → `embed_corpus` (deserializer) → `EmbedResult` →
(no feedback to artifact). **Open loop** — config is read once at start;
no output of `embed_corpus` modifies the YAML.

| Property | Analysis |
|----------|---------|
| Polarity | Negative-feedback absent (open loop). The consumer cannot correct the artifact. |
| Delay | N/A (no correction path). |
| Gain | N/A. |
| Closure | **Open.** No template renders a config that selects the template. |
| Fidelity | **Low for `company-researcher.yaml`.** The deserializer silently drops unknown fields (no `deny_unknown_fields`), so drift is invisible until a required field is missing — then it fails with a generic "missing field" error that does not name the artifact as the cause. |

**Variety check (Ashby):** The regulator (`CorpusConfig` deserializer) must
have requisite variety for the disturbances the artifacts can produce.
Currently it does **not**: unknown fields are silently dropped, so the
deserializer cannot distinguish "intentionally omitted optional field"
from "stale field name due to schema drift." This is a **Guardrail**-tier
finding for the struct, not the YAML — it informs Phase 5.

**VSM mapping:** The artifact participates in **S1** (operations — it
configures the embed operation). It does not participate in S2–S5
(coordination/control/audit/policy). The lack of S3 (audit) is exactly the
variety gap: nothing detects drift until runtime failure.

### essentialist (advisory)

**`company-researcher.yaml`:**
- **G1 Exist:** The artifact **fails the deletion test.** If deleted, the
  persona cannot be built. But the persona may already be unbuilt (the
  file does not parse against current code). The artifact's existence is
  justified *only if* the persona is still intended to be built. **Open
  question** (Phase 3).
- **G2 Surface:** The YAML exposes 8 top-level keys (`author`, `embedding`,
  `chunking`, `validation`, `budget`, `entities`, `methods`,
  `dimension_centroids`, `works`). `CorpusConfig` exposes ~13. The surface
  is **wider than 7** but most are `#[serde(default)]`. **Guideline** —
  not a blocker, but the surface has accreted.
- **G3 Contract:** `validation.per_dimension`, `budget.mode`,
  `budget.triple_budget_per_100`, `embedding.centroid_entity_ref`,
  `dimension_centroids[].dimension` are **stale conditionals/keys** — they
  encode no current behavior. **Guardrail** — single-use cruft.

**`john-brooks.yaml`:**
- **G1 Exist:** Passes — the persona is the active reference.
- **G2 Surface:** Same as above (Guideline).
- **G3 Contract:** `author_full` is the only stale key (cosmetic). Passes.

### Phase 2 findings summary (T3)

| Force | Count | Artifacts |
|-------|-------|-----------|
| Prohibition | 7 | `company-researcher.yaml` (3 missing required + 4 shape mismatch) |
| Guardrail | 5 | `company-researcher.yaml` (stale fields, variety gap) |
| Guideline | 2 | both (surface width, `author_full`) |
| Hypothesis | 3 | `batch_size`, `max_triples: 25000`, `works: []` semantics |
| Evidence | many | conforming fields |

---

## T4 — Pipeline manifest runbook

### pragmatic-semantics

| Value | Ontological | Epistemic | Provenance | Confidence | Finding |
|-------|-------------|-----------|------------|------------|---------|
| `HKASK_CLASSIFIER_MODEL: "DI/Qwen/Qwen3-235B-A22B-Instruct-2507"` | IS = OUGHT | declarative | `kask/.env::HKASK_CLASSIFIER_MODEL` — **exact match** | 1.0 | OK. |
| `HKASK_QA_MODEL: "DI/zai-org/GLM-5.2"` | IS = OUGHT | declarative | `kask/.env::HKASK_QA_MODEL` — **exact match** | 1.0 | OK. |
| `HKASK_EMBEDDING_MODEL: "DI/Qwen/Qwen3-Embedding-0.6B"` | IS = OUGHT | declarative | `kask/.env::HKASK_EMBEDDING_MODEL` — **exact match** | 1.0 | OK. |
| `HKASK_EMBEDDING_DIM: "1024"` | IS = OUGHT | declarative | `john-brooks.yaml` `dim: 1024` | 1.0 | OK. |
| `HKASK_DB_POOL_SIZE: "64"`, `HKASK_HTTP_POOL_MAX_IDLE: "256"` | IS = OUGHT (no consumer constraint checked) | declarative | Unknown — not traced to Rust constant | 0.4 | **Hypothesis** — plausible pool sizes, unverified. |
| `path: "/home/mdz-axolotl/Clones/Library/Researcher"` | IS = OUGHT (operator-local) | declarative | Operator filesystem; runbook is operator-scoped | 0.7 | **Hypothesis** — stale if corpus moved. |
| `db_path: "corpus/memory/john-brooks.db"` | IS = OUGHT | declarative | `john-brooks.yaml` header | 1.0 | OK. |
| `max_tokens: 512` (UNVALIDATED) | IS ≠ OUGHT (self-documented hypothesis) | subjunctive | Runbook comment ("256 over-fragmented, 512 unvalidated") | 0.6 | **Hypothesis** — no Rust default supersedes (to confirm Phase 3). |
| `overlap_tokens: 64` | IS = OUGHT | declarative | Unknown | 0.4 | **Hypothesis** — unverified. |
| `dedup_threshold: 0.89` | IS = OUGHT | declarative | Unknown | 0.3 | **Hypothesis** — provenance Unknown. |
| `base_model: "unsloth/Qwen3.6-27B"` | IS = OUGHT | declarative | `kask/mcp-servers/hkask-mcp-training/src/providers/mod.rs` (6 occurrences); `kask/registry/manifests/training/capabilities-researcher.yaml` (`base_model: unsloth/Qwen3.6-27B`) | 1.0 | OK — approved model. |
| `lora.init_lora_weights: eva` | IS = OUGHT | declarative | `kask/mcp-servers/hkask-mcp-training/src/lora_validation.rs::LoraInit::Eva`; training registry manifest `init: eva` | 1.0 | OK — valid variant. |
| `lora.r: 32`, `alpha: 64`, `dropout: 0` | IS = OUGHT | declarative | Training registry manifest identical | 1.0 | OK. |
| `target_modules: [q_proj, k_proj, v_proj, o_proj, gate_proj, up_proj, down_proj]` | IS = OUGHT | declarative | Training registry manifest identical | 1.0 | OK. |
| `num_epochs: 3`, `learning_rate: 0.0001`, `batch_size: 1`, `gradient_accumulation_steps: 16`, `lr_scheduler: cosine`, `weight_decay: 0.01`, `max_grad_norm: 0.3`, `warmup_steps: 100` | IS = OUGHT | declarative | Training registry manifest `training:` block identical | 1.0 | OK. |
| `sequence_len: 4096` | IS = OUGHT | declarative | Training registry manifest `sequence_len: 4096` | 1.0 | OK. |
| `attn_implementation: sdpa`, `gradient_checkpointing: "true"`, `bf16: true`, `eval_split_ratio: 0.0012` | IS = OUGHT | declarative | Training registry manifest `eval.val_set_size: 0.0012` matches | 1.0 | OK. |
| `optimizer: adamw_8bit` | IS = OUGHT | declarative | Training registry manifest `optim: adamw_8bit` | 1.0 | OK. |
| `tool: corpus_*` (13 tools) | IS = OUGHT | declarative | All 13 match Rust entry points (Phase 1) | 1.0 | OK. |
| `verify: { field, min }` blocks | IS ≠ OUGHT (no consumer) | subjunctive | Runbook-only; no Rust enforcement | 0.8 | **Guardrail** — open loop. |

**Conflict resolution:** The training registry manifest
(`kask/registry/manifests/training/capabilities-researcher.yaml`) is the
**Domain Supplement** authority for training params. The runbook's
`train_lora` step params match it **exactly** — no drift. (Note: the
registry manifest is a skill-registry artifact and out of scope for edits,
but it serves as provenance evidence here.)

### pragmatic-cybernetics

**Loop:** runbook (operator reads) → operator invokes MCP tool → tool
returns result → operator checks `verify:` block → operator proceeds or
stops. **Closed loop, but the closure is human** — no Rust enforces the
`verify:` gate.

| Property | Analysis |
|----------|---------|
| Polarity | Negative (verify stops the pipeline on failure). |
| Delay | Human-mediated (high). |
| Gain | 1 (verify is pass/fail). |
| Closure | **Closed by human, not by Rust.** The `verify:` blocks are advisory. |
| Fidelity | **Low.** The `verify:` `field` names (`total_documents`, `stored_h_mems`, `train_examples`) must match the MCP tool's actual return shape. If a tool's return schema changes, the runbook's verify silently checks a nonexistent field. No Rust validates this. |

**Variety check:** The regulator (operator) must have requisite variety
for the disturbances (tool return shape changes). The operator has no
machine help — the runbook does not auto-validate against tool schemas.
**Guardrail** — the runbook's `verify:` blocks are a fragile contract with
the MCP tools' return types.

**VSM mapping:** The runbook participates in **S1** (operations) and
attempts **S3** (audit via `verify:`) but S3 is unenforced. No S2/S4/S5.

### essentialist (advisory)

- **G1 Exist:** The runbook **passes the deletion test** — without it, an
  operator cannot reproduce the pipeline. It earns its keep as a
  coordination artifact.
- **G2 Surface:** The runbook exposes 9 phases × ~5 params each. The
  surface is wide but justified by the pipeline's scope. **Guideline** —
  not a blocker.
- **G3 Contract:** The `verify:` blocks are the only "abstraction" — they
  encode a real intent (gate the pipeline) but have no machine backing.
  **Guardrail** — single-use, unenforced.

### Phase 2 findings summary (T4)

| Force | Count | Artifact |
|-------|-------|----------|
| Prohibition | 0 | — |
| Guardrail | 3 | `verify:` open loop, `verify:` field-name fragility, pool-size unverified |
| Guideline | 2 | surface width, `verify:` abstraction |
| Hypothesis | 5 | `max_tokens: 512`, `overlap_tokens: 64`, `dedup_threshold: 0.89`, absolute path, pool sizes |
| Evidence | many | conforming model names, training params, tool names |

---

## Checkpoint C2

- [x] T3 findings produced (replica cluster) — 7 Prohibition, 5 Guardrail.
- [x] T4 findings produced (pipeline runbook) — 0 Prohibition, 3 Guardrail, 5 Hypothesis.
- [x] Prohibition findings promoted to Phase 3 interrogation.
- [x] Human review point: the `company-researcher.yaml` Prohibition findings
  are the basis for Phase 4 bug-hunt + diagnose. The runbook's Hypothesis
  findings are Phase 3 provenance questions, not bugs.
