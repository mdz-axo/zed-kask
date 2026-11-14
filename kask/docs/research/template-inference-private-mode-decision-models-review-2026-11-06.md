---
title: Template inference settings, private-mode template operations, and decision-model composition — review and design assessment
audience: the operator (PI)
last_updated: 2026-11-06
version: 1.0
status: "Active"
domain: kask skills registry / private inference / decision models
mds_categories: research record
---

# Template inference settings, private-mode operations, and decision-model composition (2026-11-06)

**Record status:** review and design record — every recommendation here is a proposal filed for the algedonic review; nothing is self-approved and nothing is implemented.

A gated review over three workstreams: (A) the template registry's settings and
hygiene census, run through skill-maintenance; (B) whether zed-kask should add
a default private template and how skill templates operate on private threads;
(C) decision points and decision models — the census unconditional, the style
evaluation behind the JEV gate. Method: Fagan-style inspection (detection
separated from correction; proposals filed, never applied), S14's
deterministic-output discipline as the decision-point lens, and the FR-3/FR-4
falsifier tests as the private-boundary oracle.

## 0. Machine-check ledger (all run this session, all green)

Registry sweeps (advisory gates, run voluntarily):

- `bash kask/scripts/check-template-contracts.sh` → `OK: checked 216
  templates; 0 flagged; 6 skipped (no contract)` — the `[inference]`
  contract/body agreement gate.
- `bash kask/scripts/check-skill-crossrefs.sh` → `OK: all skill↔template/
  deleted-skill cross-references resolve`.
- `bash kask/scripts/audit/skill-corpus-prescreen.sh` → 57 computation-
  prescribing SKILL.mds D/P-labelled; 57 oracle-naming; 58 initial+target
  condition statements; 216 templates, 2 flagged / 2 accepted baseline
  (`mcp-tool-review/findings-artifact.j2` LOW OVERLAP 19%,
  `metacognition/falstaffian-rotation.j2` 22%).

Pin tests (`cargo test -p <crate> -- <filters>`, sequential runs, exit 0):

- `agent` (15 green, filter `private`): `private_thread_refuses_a_public_
  model_at_the_storage_gate`, `private_thread_turns_on_the_configured_
  private_leg`, `private_thread_turn_entry_fails_typed_with_no_model`,
  `private_thread_refuses_a_public_summarization_model`,
  `private_mark_round_trips_through_db`, `private_routing_errors_name_the_
  boundary`, `private_thread_model_list_disables_non_local_models`,
  `local_provider_set_extends_the_private_leg`,
  `private_thread_select_model_refuses_before_any_write`,
  `media_server_tools_are_absent_on_private_threads`,
  `kask_server_tool_refuses_private_provenance_on_disabled_servers`.
- `agent` (13 green, the D100 pins without "private" in their names):
  `curator_thread_marks_and_routes_identically`,
  `a_marked_thread_never_loads_holding_a_public_model`,
  `inherit_from_carries_the_privacy_mark`,
  `marking_mid_turn_re_scopes_the_running_turns_tool_map`.
- `agent` (34 green, `render_template` scope — includes
  `all_registry_templates_conform`, `corpus_templates_strip_without_leaking_
  metadata`, `corpus_templates_render_without_error`,
  `corpus_includes_resolve_and_strip`).
- `agent_ui`: `private_inference_toggle_label_renders_the_routing_decision`
  (1 green, filter `private`); `cycling_favorites_skips_disabled_models`
  (green, 16-test scope).
- `kask_bridge` (31 green, filters `private|ollama` + `d100`):
  `ingest_turn_stamps_private_routed_turn_chunks_private`,
  `bridge_ingest_turn_maps_the_d100_flag_to_the_mark`,
  `private_turn_chunks_embed_via_the_local_port_not_the_configured_embedder`,
  `private_context_recall_embeds_the_query_via_the_local_port`,
  `private_turn_chunks_stay_keyword_only_when_no_local_port_is_wired`,
  `private_context_recall_stays_keyword_only_when_no_local_port_is_wired`,
  `private_embedding_model_defaults_overrides_and_opts_out`,
  `private_consolidation_twin_prunes_low_confidence_across_the_split`,
  `private_forgetting_leg_prunes_aged_turn_storage_only`,
  `public_context_injection_never_returns_private_marked_rows`,
  `private_context_injection_recalls_its_private_rows`,
  `local_ollama_credentials_bind_the_serving_descriptor_with_no_key`,
  `the_default_private_model_names_the_local_provider`,
  `build_rejects_empty_and_cloud_prefixed_models_but_binds_the_local_one`,
  `the_ollama_descriptor_serves_embeddings_from_loopback`.
- `settings_content`: `resolved_local_providers_defaults_to_ollama_and_merges`
  (1 green).
- `hkask-mcp-server` (10 green): the M5 read-side pins —
  `the_exact_private_string_claims_private`,
  `absent_or_unknown_request_marks_read_public`,
  `the_scope_reaches_handlers_and_absence_reads_public`,
  `the_call_params_shape_parses_from_wire_json`.
- `hkask-mcp-curator` (15 green): `private_call_contexts_receive_their_
  private_rows`, `public_call_contexts_never_receive_private_rows`,
  `semantic_search_filters_private_rows_by_call_context`,
  `memory_insert_inherits_the_evidence_inference_provenance`,
  `no_tool_request_exposes_a_provenance_field`,
  `a_lesson_from_private_marked_turns_lands_private`.

Tree state at review: HEAD `5a074ef411`; staged index empty; one untracked
directory `kask/registry/templates/direct-model-edit/` (finding A1).

## 1. Workstream A — the template census

### 1.1 Corpus shape (measured)

- 216 `.j2` templates under `kask/registry/templates/`, across 55 skill-named
  directories; 213 carry the `[inference]`-keyed contract header terminated by
  a lone `---` (`render_template` reads `contract.input`; the header is the
  only metadata carrier — `crates/agent/src/tools/render_template_tool.rs:342-367`).
- 3 headerless `.j2` files: `coding-guidelines/anti-patterns.j2` (an
  include-only shared fragment, consumed via `{% include %}` by
  `guidelines-assess.j2` and `turn-shape.j2` — never rendered standalone) and
  `docproc/ocr-extract.j2` + `docproc/tag-passages-batch.j2`.
- The contract audit's "6 skipped (no contract)" = those 3 plus
  `docproc/consolidate-chunks.j2`, `docproc/rag-answer.j2`,
  `docproc/extract-hmems.j2` — the docproc directory is a **second template
  population**: the corpus MCP server loads and renders these through its own
  cached minijinja environment (`kask/mcp-servers/hkask-mcp-corpus/src/
  template.rs:14-52`, env `HKASK_TEMPLATE_ROOT`), not through the agent-side
  `render_template` contract machinery. `docproc/adjudicate-passages.j2` is
  dual-consumed (server-side and agent-side at build-corpus-pipeline Stage 8,
  `.agents/skills/build-corpus-pipeline/SKILL.md:549`) — hence its header.
- 2 empty template directories carry no files: `media/` and `program-manager/`
  (local-only leftovers; git does not track empty directories).
- 58 installed SKILL.mds; 6 skills have no registry template directory
  (algedonic-review, build-corpus-pipeline — which renders the docproc-owned
  `adjudicate-passages.j2`, gpui-bench, onto-anchor, system-maintenance,
  transcript-reel). The `experimentation/` directory is owned by
  skill-maintenance's optimize phase (it renders
  `experimentation/experiment-protocol.j2`).
- Every template carries a `{# goal: ... #}` first-line annotation (prescreen
  goal-presence green over all 216); purposes below are sourced from those
  annotations and the header contracts.

### 1.2 Inference settings: none — the landed norm stands

`render_template` renders locally and returns text as guidance for the calling
agent's own reasoning (`render_registry_template`, `render_template_tool.rs:131-157`).
There is no per-template model and no per-template inference parameter block.
The former body `[inference]` parameter stanza (temperature / work_effort /
verbosity / thinking_budget) had no consumer and was removed corpus-wide on
2026-09-28; `all_registry_templates_conform` (green this session over all 216,
including the four untracked direct-model-edit templates) rejects its
reintroduction (`render_template_tool.rs:1170-1186`, pinned doc at `:342-367`).
A registry-wide sweep confirms: zero `temperature =`-style stanza lines, zero
`work_effort|verbosity|thinking_budget` tokens. The only temperature text in
the registry is prose inside `direct-model-edit/baseline.j2:23,55` — an
instruction to **diff the vendor model card's recommended sampling config
against the run config** before any weight edit, not a template inference
setting; it is not a stanza line and passes the conform pin. This extends the
task's prior census: the single in-registry temperature mention is verified and
classified as vendor-card diff guidance.

Any proposal to add per-template inference settings must therefore address why
the stanza was deliberately removed (no consumer; settings live in the
model-resolution chain) and what changed since. Nothing in this review found
such a change: model selection remains a settings-layer concern with code
defaults in `Default` impls (`KaskModelsSettings::default()`,
`kask/crates/kask_bridge/src/settings.rs:524-545`), `mcp_env()` injection, and
`DEFAULT_*_MODEL` constants — exactly as the `.rules` convention requires,
verified live for embedding/media/rerank (`settings.rs:366,427-437,542`).

### 1.3 Purpose census (one line per template, from each template's goal annotation)

adhd-mode: `turn-shape` — per-turn-type output shape for ADHD-shaped
composition (obligations injected, never restated). `caveman-compress` —
compression pass dropping filler while preserving substance/sacred text.
`pre-send-gate` — extraction checklist feeding the deterministic lisp gate.
bug-hunt: `bug-hunt-charter` — frame the exploratory test charter.
`bug-hunt-probe` — execute one heuristic probe. `bug-hunt-oracle` — define the
oracle adjudicating probes. `bug-hunt-report` — compile the threat-coverage
report.
code-review: `code-review-scope` — scope the change from git evidence, machine
checks first, no judging. `code-review-perspectives` — multi-axis detection
with file:line evidence, no verdicts. `code-review-adjudicate` — tiered
evidence-backed verdicts, severity from constraint force (CR-1).
`code-review-report` — verdict + remedies + coverage honesty + loop closure.
`code-review-implement` — user-authorized Act phase applying fixes at/above
the tier.
coding-guidelines: `anti-patterns` — shared fragment: the seven Karpathy
anti-patterns (include-only). `guidelines-assess` — pre-implementation
assessment producing constrained directives and guardrails.
`guidelines-verify` — post-implementation verification against the four
principles.
create-skill: `scaffold` — scaffold a full skill (SKILL.md + .j2 leaves) from
a description. `translate` — translate a foreign skill into the kask artifact
contract.
deep-module: `deep-module-assess` — score module depth and classify.
`deep-module-delete` — run the deletion test in both directions.
`deep-module-design` — design a deep interface from deletion-test results.
diagnose: `diagnose-loop` — build the feedback loop, confirm repro before
hypothesizing. `diagnose-spec-anchor` — anchor the bug via the DC/PKO dual
ontology. `diagnose-instrument` — one-variable probes per hypothesis.
`diagnose-fix` — apply fix with regression test, verify, post-mortem.
diataxis-diagram: `diataxis-diagram-classify` — classify the target diagram
type. `diataxis-diagram-extract` — extraction guidance for schema/code/doc
sources. `diataxis-diagram-generate` — generate the Mermaid diagram.
`diataxis-diagram-evaluate` — evaluate the diagram against type rules.
`diataxis-diagram-write` — write the accompanying documentation.
`present-diagram` — deterministic markdown preview naming a verified path.
direct-model-edit (untracked — finding A1): `baseline` — audit baseline
behavior and sampling config against the vendor model card. `localize` —
localize the failure to a causal weight-space direction/layer/columns.
`edit-plan` — plan the minimal weight edit under an explicit budget. `verify`
— verify the edit with honest gain attribution and a sibling control.
docproc (server-side population; see 1.1): `adjudicate-passages` — adjudicate
prepared QA passages (dual-consumed). `consolidate-chunks` — consolidate
related chunks via LLM synthesis. `extract-hmems` — extract h_mem assertions.
`ocr-extract` — the OCR vision-extraction prompt. `rag-answer` — answer over
retrieved passages. `tag-passages-batch` — batch passage classification.
doc-update: `triage` — Phase 0 condensation triage (CORE/FOLD/DELETE + fold
plan).
eqm: `eqm-aggregate` — aggregate scores; apply the asymmetric red/green
screen (EQM-2 verifies). `eqm-validate` — interpret Brier/correlation against
realized outcomes. `eqm-imp-predict` — kata Step 4: predict the intervention.
`eqm-imp-experiment` — kata Step 5: rewrite the rationale against failing
markers.
essentialist: `essentialist-flow` — the 3-gate eliminative loop
(Exist/Surface/Contract), both modes.
eureka-cycle: `eureka-triage-brief` — route an open question
(research/experiment/forecasting). `eureka-generate` — generate recall-cited
candidates. `eureka-study-report` — compose the study report.
experimental-design: `normalize-hypothesis` — normalize the hypothesis to the
typed contract. `synthesize-designs` — candidate designs with evidence and
limits. `select-design` — compare and select the discriminating design.
`interpret-findings` — interpret sealed findings against the design.
experimentation (rendered by skill-maintenance-optimize):
`experiment-protocol` — render the five-step experiment protocol for one
artifact layer.
falsifiability: `falsifiability-admit` — the admissibility gate.
`falsifiability-hypothesize` — multiple falsifiable hypotheses.
`falsifiability-counterfactual` — minimal counterfactuals.
`falsifiability-discriminate` — discriminating tests.
`falsifiability-eliminate` — eliminate failed hypotheses.
gradient-hunter: `expectation-inquiry` — gate an expectation mismatch against
the active goal. `gradient-prior` — build the expected-field prior.
`gradient-map` — measure the actual field at prior granularity.
`gradient-detect` — classify gradients by the eight ontological shapes.
`gradient-hypothesize` — reason hypotheses via the seven-class taxonomy.
`gradient-report` — the prioritized gradient report.
grill-me: `grill-me-round` — one escalating interrogation round.
`grill-me-assess` — assess answers, produce the gap analysis.
grounding-verify: `extract-claims` — extract load-bearing claims with
materiality scoping. `assign-provenance` — provenance on the strength
lattice. `scan-narrative` — vouch/trace the text against sources.
`compile-error-log` — findings with severity.
hypothesis-framer: `finer-evaluate` — FINER evaluation into a valid question.
`pico-structure` — PICO structuring. `hypothesis-operationalize` — the
testable hypothesis and aims.
hypothesis-testing: `registry-adapter` — classify DesignSpec→experimentation
preconditions. `registry-declaration` — map artifacts into declaration args.
`conclusions` — HT-1 conclusion updates on contradiction/flip only. `report`
— the study report from sealed artifacts.
idiomatic-rust: `idiomatic-rust-inquiry` — assess a design problem through
Hoare's lens with diagnostics as ground truth. `idiomatic-rust-design` —
type-driven solutions informed by LSP actions. `idiomatic-rust-challenge` —
adversarial review grounded in compiler feedback.
improv: `improv-select` — select the best-fit improv mode. `improv-plussing`
— Plussing (Catmull). `improv-yes-and` — accept and extend. `improv-yes-but`
— accept and constrain. `improv-freestyling` — rapid short-response cycling.
`improv-riffing` — solo divergent exploration from a seed.
kanban-task-management: `decompose-tasks` — INGEST-compliant vertical-slice
decomposition. `review-tasks` — review decomposed tasks. `configure-spawn` —
delegation level/skills/memory scope. `coordinate-agents` — read comment
threads, prepare replies. `track-deliverables` — assess deliverable
completeness. `verify-completion` — criterion-by-criterion evidence before
verify. `escalate` — operator-ready escalations.
kask-seam-audit: `seam-map` — map the divergence surface, verify convention
priors. `audit-security` — security track over 10 priority surfaces.
`audit-architecture` — dead-surface + deepening track. `audit-ui` — GPUI
measured-layout + interaction audit. `layout-sense` — measure widths; KSA-1
computes the gates. `layout-review` — adversarial layout probes.
`layout-act` — canonical GPUI remedies per failing gate. `mcda` — MCDA
scoring; KSA-2 computes totals/ranking. `adjudicate` — adjudicate findings
via semantics/cybernetics/essentialist. `remediate` — apply only top-ranked
essentialist-surviving remediations. `final-report` — the engagement report.
kata-improvement: `improvement-step1-4` — Understand Direction / Grasp
Current / Establish Target / Iterate. `coaching-q1-5` — the five coaching
questions. `beginner-pdca-cycle` — the PDCA drill. `beginner-observation-
drill` — the observation drill.
lean-prover: `lean-prover-anchor` — anchor the obligation (Prop/Type,
structure, method). `lean-prover-construct` — construct the compiling proof.
`lean-prover-refute` — adversarial counterexample search.
mcda: `identify-criteria` — identify and classify benefit/cost criteria.
`weight-and-score` — weights and raw scores (normalization via lisp).
`sensitivity-analysis` — interpret lisp-computed ranking and perturbations.
mcp-tool-review: `findings-artifact` — serialize review records as JSON
without model reconstruction.
metacognition: `meta-establish-target` — declare the target metacognitive
state. `meta-grasp-current` — measure the actual state. `meta-predict` —
predict the intervention with optional confidence. `meta-experiment` — apply
the rotation and re-measure. `ellipsis-analysis` — Bloom's five-step gap
detection. `falstaffian-rotation` — the five frame-rotation shapes.
`inquiry-engine` — sequential inquiry with deep-dive delegation.
pragmatic-cybernetics: `cybernetics-analyze-loop` — 5-property loop analysis.
`cybernetics-variety-check` — Ashby variety balance. `cybernetics-vsm-map` —
VSM S1-S5 viability mapping.
pragmatic-semantics: `semantics-classify-statement` — IS/OUGHT + certainty +
constraint-force classification. `semantics-route-step` — D/P regime routing
per P8.4. `semantics-provenance-trace` — trace a claim to its authoritative
source. `semantics-conflict-resolve` — resolve conflicts by force/provenance.
product-manager: `intake-brief` — render the intake brief (the work order).
prompt-enhance: `enhance-classify` — classify the prompt against the 7-type
taxonomy, select the effort tier. `enhance-rewrite` — typed inline audit +
rewrite in one call. `enhance-verify` — independent interrogation against
self-confirmation. `enhance-output-render` — deliver per output_format with
critic findings preserved.
provider-audit: `inbound` — provider alignment with P1-P4/V1-V8. `outbound` —
our code's alignment with provider capabilities. `findings` — the audit
findings record.
refactor-architecture: `ra-explore` — explore friction, rank deepening
candidates. `ra-deepen` — the grill loop for the selected candidate.
`ra-audit` — duplication audit with the deletion test. `ra-strangle` —
strangler-fig migration plan. `ra-verify` — post-migration verification
report.
region-routing: `route` — the syntax-semantic x deterministic-probabilistic
routing plan with named gates and checkpoints.
scenario-planning: `type-framing` — MAIA per-type framing defaults.
`driving-forces` — STEEP forces by importance/uncertainty. `axes-and-narratives`
— divergent 2x2 quadrant narratives. `scenario-quality-gate` — independent
divergence/consistency/coverage scoring. `implications-indicators` —
implications, robust/contingent strategies, early warnings.
security-audit: `map-surface` — PLAN: assets/entry points/priority surfaces.
`probe` — DO: deterministic sweeps with known-positive validation.
`adjudicate` — CHECK: evidence-backed verdicts, counts in lisp. `report` —
ACT: coverage-honest report + ratchet proposals.
self-improvement: `si-agenda-generator` — design/prioritize the experiment
agenda. `si-select-pathway` — select scaffold component/signal + plan.
`si-exec-scaffold-prompt|memory|tool|full` — the four scaffolding executors.
`gpa-reflect` — diagnose failures, surface rules (NL gradient).
`gpa-propose-mutations` — generate variants via mutation+crossover.
`si-evaluate-improvement` — evaluate on held-out tasks.
`si-propose-or-discard` — file or discard from the measured Check.
skill-bundler: `bundler-merge` — merge N skill outputs into one grounded
report.
skill-discovery: `skill-discovery-route` — ranked skill matches + uncovered
capabilities. `skill-discovery-detect-gap` — gap detection and classification.
`skill-discovery-evaluate` — candidate evaluation for format/quality/safety.
skill-maintenance: `skill-maintenance-validate` — S1-S14/T1-T5 validation
pass. `skill-maintenance-audit` — staleness audit with health scoring.
`skill-maintenance-optimize` — fixed-task candidate comparison → proposal
card. `logic-critique-template` — adversarial template critique.
`logic-critique-critique` — drop unsupported concerns. `logic-compare-
candidates` — compare candidates on fixed cases. `logic-compose-proposal` —
compose the diff for the proposal card.
structured-extraction: `identify-entities` — entity identification against
the schema. `extract-relations` — OpenIE binary relations (Banko 2007).
`map-to-schema` — map to the target schema with coverage reporting.
superforecasting: `stage_0_triage` — difficulty/Goldilocks triage.
`stage_1_fermi_decompose` — Fermi decomposition. `stage_2_outside_view` —
sourced frequencies/priors only. `stage_3_probability_estimate` — inside-view
probability adjustment. `stage_4_evidence_update` — Bayesian updates with
likelihood ratios. `stage_5_synthesis` — dragonfly-eye MCDA-style aggregation
(composite via lisp). `stage_6_calibration` — calibration justification.
`stage_7_record` — the record synthesis. `forecast-quality-gate` — independent
16-criterion gate (SF-3 computes totals/threshold).
task-breakdown: `task-breakdown-plan` — PLAN: read-only current-condition
grasp. `task-breakdown-decompose` — DO: decompose into component target
conditions. `task-breakdown-quality-gate` — CHECK: seven weighted criteria
(TB-2 computes). `task-breakdown-write-plan` — ACT: plan.md + todo.md with
PKO anchors.
tdd: `tdd-seams` — propose seams/layers before writing tests. `tdd-cycle` —
one red→green cycle at one confirmed seam.
therapy: `scan` — scan memory for contradictions/fragmentation/miscalibration.
`classify` — classify the found defects. `report` — the therapy session
report.
upstream-rebase: `map` — map the merge surface. `decide` — reconcile seams
per survival records + deletion test. `execute` — per-file merge/re-apply
decisions. `document` — update DIVERGENCE.md + final report. `reflect` —
propose amendments tied to held-out cases.
verification-compression: `inventory` — the expectation-to-oracle verification
graph. `elimination` — synthesize one reduction candidate. `experiment` —
fixed-oracle before/after with a non-compensable verdict.
wardley-mapper: `inventory-components` — exhaustive component inventory.
`classify-evolution` — evolution-axis classification + quadrant chart.
`synthesize-recommendations` — strategic movement + recommendations.
`present-map` — deterministic markdown preview.
writing-style: `conversational-turn` — compose/rewrite a conversational turn
in a curated style. `style-recommend` — style recommendation with validation
instrument status.

Purpose-evidence verdict: no template's purpose is non-evident from its own
text — all 216 carry goal annotations, and the prescreen's goal-presence,
length and overlap checks pass with only the two accepted-baseline low-overlap
entries (documented above).

### 1.4 Model-resolution traces (templates → inference calls)

Templates instruct agent reasoning; the SKILL.md bodies name the tools, and
the tools resolve models through the settings chain
(`KaskModelsSettings::default()` → settings.json → `mcp_env()` env injection
→ the consuming server's fail-visible resolver). The trace, per
inference-carrying consumer:

- All 216 templates' reasoning runs on the calling thread's model — on a
  private-marked thread that is the private leg by construction (FR-3,
  D100 storage/turn/request gates, all green this session).
- `build-corpus-pipeline` (renders docproc templates; instructs corpus tools):
  classification/tagging → `HKASK_CLASSIFIER_MODEL` (refuses when unset,
  `model_constants.rs:93-102`; default `OpenRouter/z-ai/glm-5.2` — a
  non-thinking model by documented ruling, `kask_bridge/src/settings.rs:529-535`);
  embedding → `HKASK_EMBEDDING_MODEL` → `DEFAULT_EMBEDDING_MODEL`;
  QA generation → `HKASK_QA_GENERATION_MODEL` — deliberately unconfigured by
  default and refuses (`model_constants.rs:54-59`); OCR → `HKASK_OCR_MODEL`
  (default `ollama/glm-ocr:latest`, `kask_bridge/src/settings.rs:538`).
- `transcript-reel` (`SKILL.md:49,156,192`): STT/vision/passes →
  `HKASK_MEDIA_*_MODEL` (settings-layer defaults via `DEFAULT_MEDIA_*`,
  `settings.rs:427-437`; no in-server fallback, `settings_content.rs:1880-1884`);
  `corpus_embed` → the embedding chain above.
- `doc-update` (`SKILL.md:106-107`): `corpus_embed` + `corpus_tag_chunks` →
  the embedding and classifier chains above.
- `eqm` and `superforecasting` (eqm SKILL.md many sites; superforecasting
  :302): `market_score_rationale` → the prediction-markets server →
  `eqm::score_rationale` → `inference_port.generate(&prompt, &params, None)`
  (`kask/mcp-servers/hkask-mcp-prediction-markets/src/eqm.rs:304`) — model
  override `None` resolves to the configured kask default
  (`HKASK_DEFAULT_MODEL`, `kask/crates/hkask-inference/src/config.rs:135`;
  `kask never falls back to a hidden code constant` — typed NotConfigured
  when empty, `hkask_inference.rs:134-147`). Note A7 below.
- Server-side docproc inference (the second population): the corpus server's
  OCR vision leg uses `HKASK_OCR_MODEL`; fails before vision inference if
  `ocr-extract.j2` cannot be loaded/rendered (`kask/docs/reference/mcp-
  servers/corpus.md:62`).
- Research deep-search rerank → `HKASK_RERANK_MODEL` → `DEFAULT_RERANK_MODEL`
  (typed error when unset — `model_constants.rs:123-131`).
- Curator memory: private contexts embed through the local port bound to
  `DEFAULT_PRIVATE_EMBEDDING_MODEL` (`kask/crates/hkask-inference/src/
  model_constants.rs:290-301`); the classifier half degrades keyword-only,
  surfaced (D100 store split, all pins green this session).

### 1.5 Census findings

- **A1 (high — durability/provenance): the direct-model-edit skill install is
  outside version control.** `.gitignore:13` (`.agents`) ignores its SKILL.md
  (`git check-ignore -v` confirms), and its four registry templates are an
  untracked directory (`?? kask/registry/templates/direct-model-edit/`,
  mtimes 2026-11-06 08:51-09:06 — another stream's in-flight work). The skill
  is live surface (in the agent catalog; the conform pin walks its templates
  from disk). The 58 legacy SKILL.mds were tracked before the ignore rule —
  any skill installed since is silently uncommitted. Filed as proposal card
  `aff20ded-ae95-4a84-bb11-bf741ca2fd89`.
- **A2 (medium — stale doc, active misinformation): the OCR default doc
  comment names a different model than the chain serves.**
  `crates/settings_content/src/settings_content.rs:1923-1925` says the OCR
  fallback is `RunPod/kask-ocr`; the actual settings-layer default is
  `ollama/glm-ocr:latest` (`kask/crates/kask_bridge/src/settings.rs:538`).
- **A3 (medium — stale doc, phantom env var): the model-name discipline's own
  convention names an env var nothing reads.** `model_constants.rs:23`
  documents `HKASK_MODEL_DEFAULT` as "fallback when provider-specific not
  set"; the live name is `HKASK_DEFAULT_MODEL` (read
  `kask/crates/hkask-inference/src/config.rs:135`, written
  `kask/crates/kask_bridge/src/mcp_env.rs:313`). A repo-wide sweep finds no
  read or write of the documented name. A2+A3 filed as proposal card
  `67824bf2-91e1-44a6-b421-14bf57712ae1`.
- **A4 (note — two-population registry):** 213 agent-facing templates under
  the `render_template` contract; 5 server-internal docproc templates under
  the corpus server's own minijinja env; `anti-patterns.j2` include-only;
  `adjudicate-passages.j2` dual-consumed. Documented only in
  `corpus.md:62` and the corpus server source — no defect, but the split is
  not visible from the registry itself.
- **A5 (note — cosmetic):** provider-prefix casing varies across
  `DEFAULT_*_MODEL` literals (`OpenRouter/...` vs `openrouter/...` vs
  `deepinfra/...`); resolution is case-insensitive
  (`kask/crates/hkask-inference/src/provider.rs:269`, `eq_ignore_ascii_case`),
  so no functional impact; provider descriptors are lowercase ids
  (`kask/registry/providers/openrouter.yaml` `id: openrouter`).
- **A6 (note — local leftovers):** empty `kask/registry/templates/media/` and
  `program-manager/` directories (not tracked by git; delete locally if
  stale).
- **A7 (note — an unruled chain inconsistency, feeds 3.3):** eqm's
  `market_score_rationale` scoring call rides the default chat model
  (thinking-capable, `thinking_allowed: true` in `eqm.rs:296-304`) where
  sibling bounded-scoring workloads (classifier, QA generation) use dedicated
  refuse-when-unset chains. Not a silent fallback (the default is visible in
  settings and the call fails typed when unconfigured) — but the same shape
  of workload is resolved through a different chain than its siblings. The
  natural home for it is the decision-model question (3.3).

### 1.6 Hygiene verdict

The template corpus is hygienic: contracts agree with bodies (0 flagged),
cross-references resolve, goals are present and evident, no performative
inference stanzas survive, and the model-resolution chain matches the
`.rules` discipline at every verified hop. The defects the census found are
durability (A1) and stale documentation (A2/A3) — both filed as proposals;
nothing requires template-text changes. The decision-point dimension (3.2)
is the one structural gap, and it is a review-capability gap, not a template
defect.

## 2. Workstream B — private templates and private-mode template operations

### 2.1 What "a private template" would have to mean — and why the gap is not there

Templates render locally (`render_registry_template` performs no inference and
strips its own metadata header before the caller sees text). The reasoning a
template instructs runs on the thread's model — so on a private-marked thread,
template-guided work already runs on the private leg by construction: the
thread's model IS the dispatch decision (FR-3, D100), and a marked thread can
neither hold nor dispatch a public model (storage gates, mark-time clear,
restore sanitize, turn-entry typed refusal, request gate, picker dispatch
legs — all listed green in 0's pin ledger). Templates are
provenance-agnostic by design and cannot even know the thread is private; a
per-thread template population would duplicate every skill's guidance with a
private variant that replaces nothing (P5.5) and adds the first
provenance-sensitive surface to a registry whose one contract machine is
provenance-blind.

The real question is the one the task names: templates that instruct calls to
dedicated public models on private threads. Verified per consumer, with the
mechanism named:

- **Media server**: disabled by default on marked threads —
  `DEFAULT_PRIVATE_THREAD_DISABLED_SERVERS = ["media", "corpus"]`
  (`crates/settings_content/src/settings_content.rs:2018`), the PI-owned set
  (`private_thread_disabled_servers`), enforced at the offer point per request
  iteration plus a dispatch-time belt for kask-sourced tools
  (`kask_server_tool_refuses_private_provenance_on_disabled_servers`,
  `media_server_tools_are_absent_on_private_threads` — green). The set is
  surfaced at the marked toggle's tooltip (commit `d98663cc18`, review F5).
- **Corpus server**: disabled by default on marked threads since the
  operator's 2026-11-06 ruling (the tool-query-embed design §8.2, shape 2 —
  the same default set, the same enforcement; its assisting-inference legs
  `corpus_embed`/`corpus_tag_chunks`/OCR cannot run locally).
  `corpus_server_tools_are_absent_on_private_threads` — green.
- **Curator memory, read side**: the per-call provenance `_meta` plumbing
  (M5-A) is landed — `call_admits` filters every fragment-emitting site with
  surfaced excluded counts (`kask/mcp-servers/hkask-mcp-curator/src/
  hkask_mcp_curator.rs:102,212,380,517`; pins green, 1.4). The **named open
  seam** is the curator child's tool-side query embed —
  `curator_semantic_search`/`consult`/`curator_federated_search` embed every
  query through the configured cloud embedder regardless of call provenance —
  with a filed design awaiting the operator checkpoint
  (`kask/docs/plans/private-inference-tool-query-embed-design-2026-11-06.md`,
  commit `8deae548ea`; its Phase 2 names the write-side passage embeds).
- **Classifier, curator memory leg**: private contexts degrade to
  keyword-only/structural (no cloud classifier), surfaced — the deliberate
  degrade-not-leak posture (D100 store split; the embedding half lifted via
  `DEFAULT_PRIVATE_EMBEDDING_MODEL`, pins green).
- **Corpus server**: off the thread-routing axis by the PI's recorded ruling
  (M5 plan §4.1 — corpus content derives from ingested documents, not
  thread routes; stated limit, PI-approved). Consequence: corpus tools are
  NOT in the default disabled set, and a private thread running
  corpus_embed/tag/QA routes that content to the cloud models — accepted by
  the off-axis ruling; the PI can extend `private_thread_disabled_servers`,
  and the M5 plan names an intake-time corpus privacy field as the future
  mechanism.
- **Research server (the seam this review adds): no recorded ruling exists.**
  The research server is not in the disabled set and carries no off-axis
  adjudication; its deep-search rerank leg sends private thread content
  (query + retrieved snippets) to `HKASK_RERANK_MODEL`'s cloud provider, and
  its search legs send query text to external search providers. Unlike the
  corpus case there is no stated-limit record covering it. The M5 read side
  is wired into every kask server by construction
  (`call_provenance.rs`, green), so the same provenance-routing rule as the
  filed query-embed design (private call → local model or surfaced
  keyword-only degrade) is mechanically available for the rerank leg.

### 2.2 Reference models (provenance recorded)

- **Taint-tracking / IFC for LLM agents — in-corpus anchor:** FIDES,
  "Securing AI Agents with Information-Flow Control" (arXiv:2505.23643,
  present in zk-ref-open as `fides-2505.23643.pdf`; id resolved through the
  research server into run `a1409673cd88f3f9`). Its design — a security policy
  parameterizing a taint-tracking planner, labeled message components, and a
  quarantined tool-less LLM with schema-constrained output — is the published
  form of the repo's own posture: the PI owns the policy (the disabled-server
  set, the local-leg providers), the mark rides trusted parent-set channels,
  and assisting inference is either local, schema-bounded, or refused.
- **Same field, current state (research run `a1409673cd88f3f9`, web legs):**
  NeuroTaint (arXiv:2604.23374 — taint rehydration across memory boundaries
  and session restarts; explicitly the successor evaluation of FIDES), APPA
  (arXiv:2607.24625 — recoverable IFC for real-world agents), and an IFC
  privacy-leakage design (arXiv:2609.14003) whose shape matches this repo's
  landed boundary almost clause for clause: private-store records marked at
  ingest, a tool-level interceptor on every read, destination classification
  independent of adversarial requests.
- **Hybrid / local-first inference architectures — in-repo syntheses:** the
  private-inference research syntheses F3.2 (hybrid local/cloud as the
  field's direction, privacy a named driver) and F3.3 (routing as a harness
  feature; single-model agents ending) — `kask/docs/research/
  private-inference-research-syntheses-2026-10-30.md:260-294`, run-ledger
  citations at `:324-348`.
- **Privacy-preserving request rewriting — in-corpus anchor:** the
  private-user-query-to-external-LLM request-rewriting result (zk-ref-open,
  `gepa-2507.19457.pdf:528`). Relevant as the rejected alternative: this
  repo's designs route the private leg locally instead of rewriting requests
  toward a public provider — the stronger boundary (the query-embed design
  1.4 names "routing private queries to the configured cloud model" as the
  crossing it rejects).
- **In-repo curator lessons (precedent citations):** tool-call provenance must
  come from the calling thread's trusted mark at the dispatch boundary, not
  caller-supplied arguments; where model-invoked tools lack a trusted privacy
  signal, keep the private store unreachable from those tools and serve
  private recall through context injection; keep the promise of a research
  direction separate from the quality of a particular implementation.

### 2.3 Design assessment

**Verdict: already resolved — no default private template; keep the boundary
oracle green.** The landed architecture already answers the question the
hypothetical "private template" gestures at: the thread mark, not the
template, carries provenance; every inference-carrying leg is covered by a
named mechanism (disable, filter, degrade, or stated-limit ruling), and the
one mechanism gap with no recorded ruling is the research server's
assisting-inference legs (2.1, last bullet). Adding a private-template
population would create provenance-sensitive registry surface with no
consumer, would duplicate guidance without replacing any path (P5.5), and
would sit outside the one contract machine (`validate_contract_inputs`) the
registry has.

Recommendation carried by this report (report-level, no new card — it extends
an already-filed design's checkpoint question): when the operator takes the
**tool-query-embed checkpoint** (`private-inference-tool-query-embed-design-
2026-11-06.md` 1.4), extend the same checkpoint question to cover (a) the
research server's rerank leg — the same provenance-routing rule over
`HKASK_RERANK_MODEL` (private call → a local rerank model, or surfaced
skip-with-note degrade), and (b) an explicit ruling for the corpus server's
assisting-inference legs on private-provenance calls (either record the
off-axis acceptance as covering them, or schedule the intake-mark design the
M5 plan names). All three are the same design family over the same landed
read side; one checkpoint closes the class. Every recommendation above keeps
the FR-3/FR-4 falsifier tests green: none touches a covered seam, and each
narrows the set of legs private content can reach (falsifier (a): private
content in a public provider's request — strictly reduced by every named
action).

## 3. Workstream C — decision models and decision templates

### 3.1 The JEV gate: resolved (with an operator confirmation still owed)

The gate required resolving what "JEV" refers to before any style or
model-class design work. Searched: zk-ref-open via `curator_federated_search`
(two queries, zero relevant passages — no JEV entry exists in the reference
corpus); the repository, docs, skills and template registry (zero `JEV`
matches repo-wide). Resolved through the research server (run
`87ef2b94c10dd92d`, ledger-recorded):

**Jev is TypeSafe AI's decision model (released 2026-09-15), the first
"System One" model.** A System One model takes unstructured state plus one
or more typed questions defined in code and returns typed decisions — a
selected choice, a probability distribution, a score, a confidence value, or
a yes-probability — with no text output of any kind; probabilities are
sampled in parallel rather than autoregressively. Sources in the run ledger:
`docs.typesafe.ai/introduction` ("Jev evaluates typed questions against a
state and returns structured results directly. No text generation, no
parsing."); `typesafe.ai/blog/introducing-system-one-models-and-jev`;
`openrouter.ai/blog/insights/what-is-jev` (typed answers with calibrated
probabilities); the Thoughtworks and Red Hat Developer analyses; and
"JEV-as-a-Judge: Accept When Confident, Escalate When Unsure"
(arXiv:2609.26550, id resolved into the same run) — JEV as a judge exposing
a typed decision and label probabilities whose maximum is its confidence.

The gate's rule ("never guess a style; an unresolvable anchor is an operator
question") is satisfied by the resolution, not overridden by it: the style
evaluation below is grounded in the resolved reference, and **adoption of
anything JEV-class remains gated on the operator confirming that this
resolution is the intended reference** — that confirmation question is carried
on proposal card `45e4f8fd-508d-47d2-8f15-4ffb26a7cdbc`.

### 3.2 The decision-point census (unconditional; every candidate verified in-tree)

The corpus already externalizes the deterministic halves of its bounded
judgments to named lisp gates — S14's discipline, landed. The census lists the
decision points that remain (semantic judgments over fixed answer spaces) and
names each one's existing oracle:

| Decision point | file:line | Bounded judgment | Model class today | Verification oracle |
|---|---|---|---|---|
| Forecast quality gate | `superforecasting/forecast-quality-gate.j2:40,88` | 16 criteria scored 0-1; all-four ≥ 0.60 rule | thread model (semantic) + `SF-3` lisp computes weighted totals, bounds, the threshold (`:88` — "Only criterion judgments are yours") | SF-3 lisp gate (exists) |
| Dragonfly-eye synthesis | `superforecasting/stage_5_synthesis.j2:69,95` | criterion scores; a holistic probability for cross-validation | thread model; composite/weights/average computed by lisp (`:69`) | stage_5 lisp form (exists) |
| EQM aggregate screens | `eqm/eqm-aggregate.j2:61-70,175` + eqm SKILL.md `:119-122` | red-flag screen (≥2 Hurts at score 2) / green-flag endorsement (composite ≥ 4, 0 Hurts) | thread model composes; `EQM-2` lisp verifies — "the thresholds are deterministic, not LLM judgment" (SKILL.md `:119`) | EQM-2 lisp gate (exists) |
| EQM marker scoring | `eqm.rs` (`market_score_rationale`) | 12 markers scored 0-2, JSON mode | the default chat model via the inference IPC (`eqm.rs:304`, model override `None`; finding A7) | shape/bounds parse + composite arithmetic in code (`eqm.rs:314-330`) |
| MCDA weighting | `mcda/weight-and-score.j2` (goal `:1`) | criterion weights, alternative scores (direct or swing) | thread model; "normalization, composites and ranking are computed by lisp_eval" (goal) | mcda lisp forms (exist) |
| MCDA sensitivity | `mcda/sensitivity-analysis.j2:1,10` | interpret ranking, compensation masking, `danger_threshold` | thread model (interpretation only) | lisp-computed perturbation results (input to the template) |
| Scenario quality gate | `scenario-planning/scenario-quality-gate.j2:1` | divergence/consistency/coverage calibrated scores | thread model | SKILL-side bounds validation (verify at adoption) |
| Task-breakdown quality gate | `task-breakdown/task-breakdown-quality-gate.j2:1` | seven weighted criteria | thread model; "weighted-total and pass-gate computation to TB-2 lisp_eval" (goal) | TB-2 lisp gate (exists) |
| KSA MCDA + layout gates | `kask-seam-audit/mcda.j2:1`, `layout-sense.j2:1` | remediation scoring; 5 hard layout gates | thread model senses/measures; KSA-1/KSA-2 "never by the model" (goals) | KSA-1/KSA-2 lisp gates (exist) |
| Security adjudication | `security-audit/adjudicate.j2:1` | severity via constraint force | thread model; "leaving count arithmetic to lisp_eval" (goal) | adjudication contract + lisp counts (exist) |
| Code-review severity | `code-review/code-review-adjudicate.j2:1` | severity + waivable derived from constraint force | thread model | CR-1 gate (verify at adoption) |

Census verdict: the seed candidates named by the task (mcda, eqm,
superforecasting) are confirmed as the densest decision-point sites, and the
corpus-wide pattern is consistent — bounded semantic judgments typed by the
model over fixed answer spaces, deterministic arithmetic in named lisp gates.
No census candidate is a defect under S14 (the deterministic halves are
externalized); the gap is that **no standing check requires a skill to name
its decision points, answer spaces, and oracles** — they are discoverable only
by episodic audits like this one.

### 3.3 Style and model-class evaluation (against the resolved JEV reference)

Should a dedicated decision-model setting exist, and would the censused points
be better composed as decision templates passed to it?

**Fit.** The Jev class maps cleanly onto the census's semantic remainder:
every candidate is a typed question over a state with a predefined answer
space plus a confidence — exactly Jev's "noul"/choice/score modes. The tree's
closest existing decision-model call is `market_score_rationale` (A7): a
bounded 12-marker 0-2 JSON-mode scoring call already shaped like a Jev
request, currently riding the default chat model where its sibling scoring
workloads use dedicated refuse-when-unset chains. A decision-model setting
would be that call's natural resolution home.

**Adoption shape, if the operator confirms and adopts** (each element follows
a named in-repo precedent; nothing here is implemented):

- A `DEFAULT_DECISION_MODEL` constant in `model_constants.rs` plus a
  `kask.models.decision_model` settings field — the six-file chain
  (`kask/crates/kask_bridge/src/settings.rs:458-467` documents it: content
  twin, From<Content>, `emit_models_env`, the target server's `config_env`
  allowlist pinned by `*_allowlist_matches_actual_reads`, the consumer, the
  settings UI page). A private-leg variant follows the
  `DEFAULT_PRIVATE_EMBEDDING_MODEL` precedent verbatim: a dedicated
  local constant (`kask/crates/hkask-inference/src/model_constants.rs:290-301`),
  a settings field under `kask.private_inference`, fail-visible resolution —
  an unset decision model is a typed error naming the setting, never a silent
  fallback to the chat default (the classifier precedent,
  `model_constants.rs:93-102`); on private threads, an unset or failed local
  decision leg degrades to the pre-decision-model prose path, surfaced.
- Decision templates would be registry templates whose `[inference]` contract
  declares the answer space in `contract.output` — which today **has no
  runtime reader** (documented in `check-template-contracts.sh`; the
  caller↔template handoff is documented, not validated). A real decision-model
  call site would be the first consumer of declared output contracts — new
  surface that must be named as such.
- S14 complementarity: a Jev-class decision is semantic (not a function of
  inputs a deterministic engine holds), so it is not an S14 violation; the
  S14 discipline still pins the deterministic parts (counts, thresholds over
  the returned scores) to lisp gates. The two compose: the decision model
  judges, the gate adjudicates arithmetic and bounds.
- Honest limits, stated: (1) no JEV-class model is wired in this tree today,
  and the public Jev service's provider posture (availability, pricing,
  provenance of "calibrated" claims) is unverified here — the Vizuara
  analysis in the run ledger names the second-order risk ("a million small
  thresholds that nobody reviewed individually"), which is exactly the
  threshold-audit problem the lisp gates exist to keep visible; (2) no
  measured baseline comparison exists for any candidate (P8: adoption of an
  artifact change rides a recorded experiment — the skill-maintenance-optimize
  path declares it via `experiment_propose` with the composite genotype);
  (3) the calibration claims of any decision model must be Brier-scored
  against realized outcomes before its confidences are trusted — the
  superforecasting/eqm calibration machinery is the existing instrument.

**Recommendation (JEV-confirmed-conditional):** do not wire a decision model
now. The style evaluation concludes that the class fits, the settings shape
follows three named precedents, and the first candidate is named (A7) — but
adoption is a decision-model procurement plus a measured experiment, both
operator decisions. File both under the standing-dimension card
(`45e4f8fd-508d-47d2-8f15-4ffb26a7cdbc`), whose acceptance would make the
census continuous and whose rejection keeps the corpus exactly as censused.

## 4. The recomposition decision

**Recommendation: option (b) — recompose the skill-maintenance skill itself to
carry decision-point census and decision-model composition as a standing
review dimension; do not one-off recompose individual skills now.** The
per-skill recomposition then happens — if and only if the operator adopts a
decision model — as measured proposals through skill-maintenance-optimize's
existing loop, one candidate at a time.

Evidence basis (from 1 and 3):

1. **The deterministic halves are already externalized.** Every censused
   decision point already hands its arithmetic to a named lisp gate (SF-3,
   EQM-2, TB-2, KSA-1/KSA-2, CR-1, the mcda forms — 3.2). A one-off
   recomposition of skills to "use decision templates" would re-arrange the
   semantic halves with no measured benefit and leave the same enforcement
   hole: nothing checks that NEW skills name their decision points.
2. **Less added surface.** Option (b) extends an existing check set
   (S1-S14/T1-T5) with one dimension — every bounded structured judgment
   names its answer space and its verification oracle (a pinned test, a
   `lisp_eval` form, or an operator checkpoint); where a decision model is
   configured, decision templates route through it with fail-visible
   resolution. Option (a) instead adds per-skill template edits across many
   skills with no standing enforcement and no reuse of the existing
   vocabulary — surface that must later be re-audited by hand.
3. **Greater reuse of existing patterns.** The proposed check reuses: S14's
   classification vocabulary (template-computable / lisp_eval-computable /
   genuinely semantic); the `[inference]` contract header (the answer space
   rides `contract.output`, which then gains its first runtime reader only at
   adoption); the model-resolution chain and the fail-visible resolver
   precedents (classifier, QA generation, `DEFAULT_PRIVATE_EMBEDDING_MODEL`);
   and the skill-maintenance-optimize registry path (`experiment_propose`,
   layer `skill`, the composite SKILL.md+templates genotype) for any measured
   adoption.
4. **P5.5 — what the chosen path replaces:** the episodic decision-point
   census (this review) becomes continuous enforcement, making the separate
   one-off recomposition program (option a) unnecessary; prose-embedded
   decision instructions at the censused points are superseded by named
   answer-space-plus-oracle declarations as each skill next passes review.
   No code or template is deleted by the recommendation itself — it is an
   addition to a review capability, which is precisely why it is filed as a
   proposal card rather than applied.
5. **In the blocked-JEV path this recommendation would stand on census
   evidence alone**; with the gate resolved (3.1), the same recommendation
   additionally carries the style evaluation's conditional: the check is
   worth adopting regardless of model-class adoption, and the model-class
   question (confirmation + procurement + measured experiment) is separable.
   The recommendation is therefore robust to either JEV outcome: the census
   dimension survives even if the operator rejects the Jev resolution or the
   decision-model class.

Sequence: (1) operator disposition of card `45e4f8fd-508d-47d2-8f15-4ffb26a7cdbc`
(the standing dimension + the JEV confirmation); (2) if adopted, the
skill-maintenance check-set change lands as its own measured proposal with
the fixed-case comparison (a known-good skill with named decision points, a
known-bad fixture with an unnamed bounded judgment); (3) if and only if a
decision model is adopted, per-skill recomposition proceeds candidate by
candidate under the optimize loop, starting with the named first candidate
(A7, eqm's scoring chain).

## 5. Filed proposals (the operator's algedonic review)

All three cards are on the **Algedonic review** board
(`22979ee8-8eea-4322-ad5d-d97b2476b5a4`), filed 2026-11-06, status backlog:

1. **`aff20ded-ae95-4a84-bb11-bf741ca2fd89`** — target: the direct-model-edit
   skill install (SKILL.md + 4 registry templates). The skill is live but
   outside version control; the systemic `.agents` gitignore question rides
   the card. (Census finding A1.)
2. **`45e4f8fd-508d-47d2-8f15-4ffb26a7cdbc`** — target: the skill-maintenance
   SKILL.md check set. Adds decision-point census + decision-model
   composition as a standing review dimension; carries the JEV resolution
   (TypeSafe's Jev / System One, run `87ef2b94c10dd92d`), the census evidence
   (3.2), the conditional adoption shape (3.3), and the operator's
   confirmation question. (1.5 A7, 3, 4.)
3. **`67824bf2-91e1-44a6-b421-14bf57712ae1`** — target: two stale doc
   comments (OCR default name in `settings_content.rs:1923-1925`; phantom
   `HKASK_MODEL_DEFAULT` in `model_constants.rs:23`). Doc-only fix.
   (Census findings A2/A3.)

Report-level (no card — carried by this record for the operator's next
checkpoint): extend the tool-query-embed checkpoint question to the research
server's rerank leg and the corpus server's assisting-inference legs (2.3);
the empty `media/`/`program-manager/` template directories (A6); the
provider-prefix casing note (A5); the two accepted-baseline low-overlap
prescreen entries.

## 6. Limits of this review, stated

- The pin ledger (1.4) verifies the boundary at current HEAD on this machine;
  it does not certify future commits — each named pin must re-run against
  changes that touch its seam.
- The JEV resolution is research-server evidence with primary sources in the
  run ledger; the operator's intended reference is assumed-but-unconfirmed
  until the card's first criterion is answered.
- The census's model-resolution traces cover the five skills that instruct
  inference-carrying tools plus the docproc server population; a template
  whose SKILL.md gains a new tool call after this review is outside the
  census — the standing-dimension card is the mechanism that closes exactly
  this class.
- No design proposed here is implemented; no verification claim beyond the
  ledger in 1.4/1.5 is made. Workstream C's style evaluation is conditional
  on the operator confirming the JEV reference; if the operator corrects the
  reference, 3.3's evaluation is void and the card re-opens with the correct
  anchor.

## Addendum — later the same day (2026-11-06, post-adoption)

The operator's in-session rulings supersede three of this report's state
descriptions, recorded here rather than silently edited into the body (the
dated-amendment convention): JEV is CONFIRMED as the intended reference
("let's use JEV"); the decision-model settings field is ADOPTED per the
six-file settings-chain pattern (commit `b92124aebb`); and §3.3's "no
decision model is wired today" is void — `kask.models.decision_model` now
exists (default `OpenRouter/typesafe/jev-1.14` per `DEFAULT_DECISION_MODEL`,
served over OpenRouter's Decisions API), and the eqm `market_score_rationale`
scoring routes through it: the eqm citations in §1.4/§1.5 (finding A7) and
§3.2 describe the pre-adoption state (the chat-JSON prompt path at
`eqm.rs:296-330` in the review-morning numbering — since replaced by twelve
typed Score questions over the `InferenceMethod::Decision` wire). Card
`45e4f8fd`'s criteria are all dispositioned (verified done, with the
ruling recorded verbatim). Two parallel-stream commits the same day also
supersede §2.1's seam table: the curator tool-side query embed landed
(`54688d615b`, Phase 2 following in `89c5fbfe26`) and the classifier
degrade lifted (`79e8964d96`, private tagging through the local
classifier); the remaining unruled same-class legs stay recorded in the
tool-query-embed design doc §8. This addendum also re-anchored the
citation lines this report's own commits had shifted in the reference docs
(the drift-gate failure class fixed in the same pass).
