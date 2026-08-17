# Refactor-Architecture Plan — `hkask-templates` (DRAFT v0)

> Method: `refactor-architecture` (explore → candidates → deepen → audit →
> strangle → verify). Exploration grounded by four parallel sub-agent surveys
> covering all 22 source files + the external-consumer surface. This is the
> draft plan pending multi-perspective review (metacognition, idiomatic-rust,
> grill-me, essentialist, pragmatic-semantics, pragmatic-cybernetics).

## Grounding & verification status (metacognition pass)

This section records what the plan has verified against the crate at review
 time vs. what it asserts without verification. It exists so downstream
 reviewers can see the plan's epistemic posture, not just its proposals.

### Verified against source (re-checked this pass)

| Claim in plan | Source | Status |
|----------------|--------|--------|
| `concurrency.rs` is a 13-line `pub use` re-export | `src/concurrency.rs:1-13` (13 lines) | ✅ exact |
| `bundle/cascade.rs` is 23 lines | `src/bundle/cascade.rs:1-23` (23 lines) | ✅ exact |
| `register_skill` returns `Ok(())` on INSERT failure (B1) | `src/registry_sqlite.rs:345-347` (logs, returns `Ok(())`) | ✅ confirmed |
| `register_bundle` returns `Ok(())` on serialize failure (B1) | `src/registry_sqlite.rs:391-394` | ✅ confirmed |
| `count` returns 0 on error (B3) | `src/registry_sqlite.rs:233-237` (doc admits "graceful degradation") | ✅ confirmed |
| `row_to_skill` exists at `registry_sqlite.rs:528` | `src/registry_sqlite.rs:528` | ✅ confirmed |
| `classify_failure_mode` is `#[allow(dead_code)]` (D5) | `src/step_machine.rs:738-742` | ✅ confirmed |
| `Effect::ConsumedGas(amount)` arm exists (D6) | `src/step_machine.rs:624-628` | ⚠️ partial — see below |
| `phase_str` is a one-line delegate (F7) | `src/bundle/manifest.rs:200-207` | ✅ confirmed |
| `render_minijinja` rebuilds renderer per call (F4) | `src/template_renderer.rs:436-443` | ✅ confirmed |
| `kask_bridge` imports ~12 items from crate root | `kask_bridge/src/skill_executor.rs:23` + seed fns | ✅ confirmed (12 distinct) |
| `ManifestFile` is a structural duplicate of `BundleManifest` (C7) | `src/manifest_loader.rs:42-68` | ✅ confirmed |
| `BundleManifestStep` is a wide god-struct (W3) | `src/bundle/manifest.rs:70-80` | ✅ confirmed |

### Inaccuracies found this pass (plan overstates or mis-cites)

| Claim in plan | Actual | Correction |
|----------------|--------|------------|
| `step_actions.rs` is 2,895 lines (W5, CAND-3) | 3,124 lines | Understated by 229 lines; the "stale by 5×" claim is conservative — it's worse. |
| `compute.rs` is 1,270 lines prod (W4, CAND-4) | 3,379 lines total | The 1,270 figure is unverified and likely wrong; the file is 2.6× larger than claimed. The "830-line match" needs re-measurement before CAND-4 sizing. |
| `ManifestFile` is a 14-field duplicate (C7, CAND-5) | 13 fields (`manifest` + 12) | Off by one; minor but the plan cites it twice. |
| `Effect::ConsumedGas` is "silently dropped" (D6) | Comment at `step_machine.rs:625-627` states gas is "charged per iteration, not per step" and handled by `charge_iteration` in the pass loop | The variant is *intentionally* a no-op at the effect site, not a dead/silent-drop bug. Reclassify from "dead effect variant" to "documented no-op effect" — the dead-code question is whether the variant is ever *produced*, not whether it's *dropped*. |
| "22 source files" (Scope) | 25 `.rs` files under `src/` (incl. `bundle/` subdir) | Undercounted; recheck the file inventory before relying on it. |

### Assertions not yet verified (assumptions load-bearing for plan validity)

These are claims the plan makes without a re-checkable citation, or citing
"the survey" without line numbers. Each is rated for plan-validity severity.

| ID | Assertion | Severity | Why it matters |
|----|-----------|----------|----------------|
| A1 | "`Registry` has zero external consumers (test-only)" (CAND-1) | High | CAND-1 deletes `Registry` outright. If even one external crate imports it, the candidate is a breaking change, not a refactor. Needs `grep -r 'use hkask_templates::Registry' kask/` confirmation. |
| A2 | "22 of 32 crate-root re-exports have zero external consumers" (D3, CAND-10) | High | CAND-10's scope depends on this count. The plan lists 19 names, not 22, and does not show the grep that produced the count. |
| A3 | CAND-5's `#[serde(flatten)]` "needs a spike" — spike not done | Medium | The spike is the gating experiment for CAND-5 but the plan defers it without assigning it. Until the spike runs, CAND-5 is unfalsifiable. |
| A4 | CAND-7 is "the highest-risk candidate" (qualitative) | Medium | No blast-radius metric. A quantified proxy (count of `BundleManifestStep` construction sites + `StepNode` field-read sites) would make the risk comparable to other candidates. |
| A5 | "Prior seam-audit (RA-01/09/10) deferred this" (CAND-1 Risks) | High | The deferral reasons are not quoted or linked. CAND-1 cannot proceed until the deferral rationale is re-read and either retired or honored. This is the plan's own Open Question #1, restated as an obstacle. |
| A6 | "`MAX_STEPS` hard gate is applied only in the executor path" (C4, CAND-2) | Medium | The plan cites `step_graph.rs:132` and `executor.rs:206` but does not show the two sub-cascade paths' gate absence. Needs grep of `execute_flowdef`/`execute_parallel` for `MAX_STEPS`. |
| A7 | "`hkask-mcp-media` bypasses crate root, uses `pub mod budget` directly" (P6) | Low | Cited as `hkask-mcp-media/src/budget.rs` but the plan does not show the import line. Low severity because the claim is plausible and the fix is local. |
| A8 | "`kask_extensions_ui` consumes 4 seed fns" (Scope, P7) | Low | DIVERGENCE surface claim; needs the 4 fn names listed. |
| A9 | "`step_actions.rs` action methods range from 50 to 470 lines" (CAND-3) | Low | Plausible but unverified; sizing for the split. |
| A10 | "30+ manifests" must round-trip under `#[serde(flatten)]` (CAND-5 Risks) | Low | The count is unverified; the spike should enumerate them. |

### Obstacles to plan success (typed)

| ID | Type | Obstacle | Blocking which candidate |
|----|------|---------|--------------------------|
| O1 | Unknown blast radius | CAND-7's `BundleManifestStep` construction-site count and `StepNode` reader count are unmeasured | CAND-7 |
| O2 | Unrun experiment | CAND-5's `serde(flatten)` × `deny_unknown_fields` × `serde_yaml_neo` spike is acknowledged but not executed | CAND-5 |
| O3 | Unreconciled prior decision | CAND-1's prior deferral (RA-01/09/10) rationale is not re-checked | CAND-1 |
| O4 | Unverified invariant | C4's "`MAX_STEPS` only in executor path" is asserted, not shown | CAND-2 |
| O5 | Behavior-change ambiguity | CAND-9 fixes feedback loops but callers may depend on silent `Ok(())`; refactor vs. behavior-change PR boundary undecided | CAND-9 |
| O6 | Test-coupling unknown | CAND-10's per-item deletion test (which zero-consumer re-exports are exercised by in-crate contract tests via the crate root?) is unresolved | CAND-10 |
| O7 | Stale sizing | `compute.rs` line count (1,270) and `dispatch_compute` match size (830 lines, 17 arms) are unverified; CAND-4 sizing is unreliable until re-measured | CAND-4 |
| O8 | File inventory drift | "22 source files" is off; downstream counts that depend on it (e.g. coverage fractions) are unreliable | Whole plan |

## Scope

Crate: `kask/crates/hkask-templates/` — 22 source files (⚠️ re-count needed;
actual `.rs` count under `src/` is 25 incl. `bundle/`), ~9,800 lines prod +
~3,400 lines tests (⚠️ `step_actions.rs` is 3,124 lines, not 2,895; `compute.rs`
is 3,379 lines, not 1,270 — the prod/test split needs re-measurement). Public
surface re-exported at `src/hkask_templates.rs:34-57`. Four external consumers:
`kask_bridge` (widest, 12 items — verified), `hkask-mcp-kata-kanban` (2),
`hkask-mcp-media` (submodule-only, bypasses crate root), `kask_extensions_ui`
(4 seed fns).

## Friction inventory (from exploration)

Grouped by signal. File:line citations are from the survey sub-agents and
verified against the crate root.

### A. Shallow modules / pass-through wrappers (deletion-test FAIL)

| ID | Item | Location | Evidence |
|----|------|----------|----------|
| F1 | `concurrency.rs` — 13-line `pub use` re-export of `hkask_types::concurrency` | `src/concurrency.rs:1-13` | Pure pass-through; 3 in-crate import sites could use `hkask_types::concurrency` directly (one test already does). |
| F2 | `bundle/cascade.rs` — 23-line file for a 3-variant `CascadePhase` enum | `src/bundle/cascade.rs:1-23` | One consumer (`bundle/manifest.rs`). Could inline into `manifest.rs`. |
| F3 | `FsSkillReader` — unit struct, no trait, one-line body, one consumer, no injection | `src/ports.rs:216` | Testability theater; `SkillLoader::discover_skills` bypasses it via direct `fs::read_dir`. |
| F4 | `render_minijinja` — one-shot wrapper rebuilding `TemplateRenderer` per call | `src/template_renderer.rs:436` | Re-introduces the per-call cost the cached renderer was built to avoid. |
| F5 | `render_step_template` — 7-line pass-through discarding 2 of 3 return values | `src/step_actions.rs:1557` | Wrapper exists to discard `raw` + `InferenceBlock`. |
| F6 | `apply_input_mapping` — 12-line pass-through over `Value::Object` | `src/step_actions.rs:54` | Could be inlined at 4 call sites; helper exists to avoid 4× `if let` guard. |
| F7 | `phase_str` — one-line delegate to `self.phase.as_str()` | `src/bundle/manifest.rs:204` | Pass-through method. |
| F8 | Three `*_str()` methods on `BundleConflict`/`BundleComplementarity` | `src/bundle/composition.rs:84-109` | One-line delegates with Hoare-triple doc comments longer than the body. |
| F9 | `execute_manifest` (borrowed) — pass-through to `execute_manifest_into` via `self.clone()` | `src/executor.rs:189` | Test ergonomics only; bridge uses `_into` exclusively. |
| F10 | Inherent-vs-trait delegation in `Registry` + `SqliteRegistry` (~120 lines) | `registry.rs:565-623`, `registry_sqlite.rs:350-367` | Pure disambiguation wrappers; the "owned" suffix on `SqliteRegistry` is the same contortion. |

### B. Dead / speculative public surface

| ID | Item | Location | Evidence |
|----|------|----------|----------|
| D1 | `PromptStrategy` — entire module, zero production callers | `src/prompt_strategy.rs`, re-export `hkask_templates.rs:50` | Grep across `kask/` finds only definition + docs. |
| D2 | `render_input_param_spec`, `extract_contract_input_keys` — test-only | `src/inputs.rs`, re-export `hkask_templates.rs:43-44` | Zero production callers; re-exported at crate root. |
| D3 | 22 of 32 crate-root re-exports have zero external consumers | `hkask_templates.rs:34-57` | See external-consumer survey. Includes `Registry`, `SkillLoader`, `BundleRegistryIndex`, `GoldenOutputFixture`, `ManifestLoadError`, `McpReferenceWarning`, `load_manifest_from_file`, `resolve_manifest`, `validate_mcp_references`, `FsSkillReader`, `ManifestResolveError`, `Result`, `SkillFinding`, `TemplateError`, `PromptStrategy`, `process_manifest_yaml`, `template_yaml_file`, `SkillFrontMatter`, `SkillLoadResult`. |
| D4 | `BudgetTracker::from_remaining`, `last_rjoule_cost` — documented "Not yet wired" | `src/budget.rs:130,176` | Speculative surface inside a live module. |
| D5 | `classify_failure_mode` — `#[allow(dead_code)]` "not yet wired" | `src/step_machine.rs:741` | 16-arm match with no caller. |
| D6 | `Effect::ConsumedGas(amount)` — produced by actions, silently dropped | `src/step_machine.rs:627` | `let _ = amount;` — dead effect variant. |
| D7 | `BundleManifest::concurrency` — parsed, round-tripped, documented "not yet enforced" | `src/bundle/manifest.rs:266` | Advertised non-invariant. |
| D8 | `ErrorHandlingConfig::on_gas_exceeded`/`on_validation_failure`/`on_capability_denied` — "accepted but ignored" | `src/bundle/config.rs:341,367,371` | Three dead fields retained for `deny_unknown_fields` back-compat. |
| D9 | `BundleManifestStep::gas_cap` — documented "informational" | `src/bundle/manifest.rs:93` | `total_step_gas` computes a number nothing enforces. |
| D10 | `step_context::materialize` — "until K5" comment stale (K5 landed) | `src/step_context.rs:267` | Still called by bridge; comment describes a future that arrived. |
| D11 | `parse_front_matter` — `pub` with only `Self::` internal caller | `src/skill_loader.rs:389` | Candidate dead surface (needs grep confirmation). |

### C. Duplicated logic

| ID | Duplication | Locations | Note |
|----|-------------|-----------|------|
| C1 | Jinja-comment stripper | `output_schema::strip_jinja_comments:84`, `inputs::strip_jinja_comments_inputs:222` | Byte-equivalent; `inputs.rs` doc admits the duplication. |
| C2 | Frontmatter parser (find `---` → strip Jinja → strip `[inference]` → YAML → `contract.<X>`) | `output_schema::extract_contract_output:50`, `inputs::extract_contract_input_keys:189` | Same 5-step pipeline, two sub-keys. |
| C3 | Operator scan `[<=, >=, ==, !=, <, >]` | `condition::parse_step_comparison:76`, `condition::parse_choice_condition:194` | Same module, two copies, one table. |
| C4 | Sub-cascade orchestration (StepGraph + StepContext + BudgetTracker + ConvergenceTracker + StepMachine + run) | `executor.rs:214-240`, `step_actions.rs::execute_flowdef ~998-1028`, `step_actions.rs::execute_parallel ~1192-1340` | Triplicated; `MAX_STEPS` hard gate applied only in executor path. |
| C5 | `call_inference_stream` / `call_inference_stream_with_messages` | `step_actions.rs:1731,1832` | Near-duplicates; shared streaming+timeout+accumulation body. |
| C6 | `finalize_report` / `inject_running` JSON builders | `convergence.rs:523-575,580-619` | ~80% field overlap; no shared struct/builder. |
| C7 | `ManifestFile` structural duplicate of `BundleManifest` | `manifest_loader.rs:42-142` | 14-field mirror; hand-copied in `load_manifest_from_yaml`. |
| C8 | `ConvergenceConfig` defaults duplicated between `Default` impl and 9 serde default fns | `bundle/config.rs:186-249` | Two sources of truth. |
| C9 | `compute_compound_quality` step_ordinal→`step_N_result` key construction | `convergence.rs:452-516` | Duplicated 3× within one function. |
| C10 | `resolve_quality` 3-step fallback chain duplicated in spirit by `push_cycle_from_context` legacy arm + `check_legacy_met` | `convergence.rs:278-292` | Bug fix routed through one private fn called from 3 sites. |

### D. Wide / god-struct surfaces

| ID | Item | Location | Note |
|----|------|----------|------|
| W1 | `StepNode` — 18 public fields, no encapsulation | `step_graph.rs:163-186` | 16 of 18 are pass-through clones of `BundleManifestStep`. |
| W2 | `Infra` — 10 public fields, no constructor | `step_machine.rs` | Built by field-literal at one site (`executor.rs:219-230`); adding a field touches every action. |
| W3 | `BundleManifestStep` — 18-field god-struct, all action types share it | `bundle/manifest.rs` | `gate` with `template_ref` or `select` with `command` is structurally representable but semantically invalid. |
| W4 | `dispatch_compute` — 830-line match, 17 arms, ad-hoc JSON destructuring per arm | `compute.rs` | God-function; arms are cohesive sub-domains (forecast/kata/swarm/listening/lisp/shell). |
| W5 | `step_actions.rs` — 2,895 lines, `impl StepMachine` split across two files | `step_actions.rs` | Largest module by 2×; "40-80 lines each" claim stale by 5×. |
| W6 | `BudgetTracker` — 14 public methods, `BudgetSnapshot` 8 public fields | `budget.rs` | Executor reaches into tracker across 12 methods. |
| W7 | `ConvergenceTracker` — 14 public methods, two models (Kata + legacy) in one struct | `convergence.rs` | 7 fields per model, no type separation, manifest uses one-or-other never both. |
| W8 | `BundleManifest::validate` — 128 lines, 9 interleaved checks, non-separable | `bundle/manifest.rs:297` | `polarities_in` closure captures `&self`, non-reusable. |

### E. Broken feedback loops (`.rules` traps)

| ID | Item | Location | Note |
|----|------|----------|------|
| B1 | `SqliteRegistry::register_skill`/`register_bundle`/`delete_entry`/`remove_skill` return `Ok(())` on write failure | `registry_sqlite.rs:333,385,206,369` | Logged but not propagated; callers cannot detect write failure. |
| B2 | `SqliteRegistry::row_to_skill` silent `unwrap_or` defaults on parse failure | `registry_sqlite.rs:529` | `Visibility→Private` silent downgrade is security-relevant. |
| B3 | `SqliteRegistry::count` returns 0 on error ("graceful degradation") | `registry_sqlite.rs:235` | `unwrap_or(0)` trap. |
| B4 | `RjouleConfig::cap` defaults to 0 as "disabled" sentinel | `bundle/config.rs:319` | Cannot distinguish "intentionally unlimited" from "forgot to configure." |
| B5 | `input_mapping::resolve_mapping_value` silent fallback to `value.clone()` on render/parse failure | `input_mapping.rs:65,67` | Two silent fallbacks in 10 lines, no warn. |
| B6 | `output_schema::contract_output_to_schema` unknown type → `{"type":"string"}` silent narrow | `output_schema.rs:155` | Typo'd type silently narrows schema. |
| B7 | `skill_loader::infer_domain_from_registry` defaults to `KnowAct` on every failure mode | `skill_loader.rs:298,309,324` | Malformed manifest and missing manifest produce same domain. |
| B8 | `Registry::reload` clears templates but not skills/bundles | `registry.rs:213` | Doc says "refreshes from filesystem"; skills/bundles survive. |
| B9 | `template_renderer::load` doc claims "disk is the single runtime source" but 3 production fallback paths call embedded seeds | `template_renderer.rs:96`, `step_actions.rs:939,1158`, `hkask-mcp-kata-kanban/kata/execution.rs:93` | Doc contradicts code. |
| B10 | `step_graph::MAX_STEPS` advisory warn + executor hard gate — dual-write | `step_graph.rs:132`, `executor.rs:206` | Two enforcement points for one invariant. |

### F. Coupling / dependency-direction issues

| ID | Item | Location | Note |
|----|------|----------|------|
| P1 | `impl StepMachine` split across `step_machine.rs` + `step_actions.rs` | — | Tightest coupling in crate; `step_actions.rs` is an impl extension, not a module. |
| P2 | Bidirectional dep: `step_actions` ↔ `executor` (utilities placed for import convenience) | `step_actions.rs:408,1025,1343`, `step_machine.rs:361` | `extract_feedback_phase`/`normalize_model_output`/`parse_json_response` have no logical home in `executor.rs`. |
| P3 | `ports::ManifestResolveError` reaches upward into `manifest_loader::ManifestLoadError` | `ports.rs:200` | Backwards dependency from foundation to consumer. |
| P4 | `step_machine::dispatch_with_retry` inlines curator MCP call | `step_machine.rs:530-577` | Skill-execution domain leaks into interpreter. |
| P5 | `merge_control_flow` constructs `step_{ordinal}_result` string key — couples machine to context's naming convention | `step_machine.rs:660` | String-key contract lives in call site, not type. |
| P6 | `hkask-mcp-media` bypasses crate root, uses `pub mod budget`/`bundle::config` directly | `hkask-mcp-media/src/budget.rs` | Crate-root surface is not the actual public API. |
| P7 | `kask_extensions_ui` (upstream Zed tree) consumes 4 seed fns — DIVERGENCE surface | `kask_extensions_ui/src/publish.rs` | Changes to seed fns ripple across the Kask↔Zed seam. |

## Candidate refactor packages

Ranked by leverage × locality × testability. Each is a one-domain-per-commit
strangler-fig candidate.

### CAND-1 (Strong) — Collapse the registry trait layer

**Files:** `registry.rs`, `registry_sqlite.rs`, `bundle/mod.rs` (trait def),
`hkask_templates.rs` (re-exports).

**Problem:** Three index traits (`RegistryIndex`, `SkillRegistryIndex`,
`BundleRegistryIndex`) with two implementations (`Registry` in-memory,
`SqliteRegistry` SQLite). `Registry` has zero external consumers (test-only).
The inherent-vs-trait delegation wrappers (~120 lines) exist only to
disambiguate trait-method-name collision. The "owned" suffix on `SqliteRegistry`
is the same contortion.

**Solution:** Delete `Registry` (in-memory) and the three traits. Move the
surviving methods to inherent `SqliteRegistry` methods. Delete the `*_owned`
forwarders. Keep `MANIFEST_YAMLS` seed accessors (live, 2 consumers each).

**Benefits:**
- *Locality:* registry concerns concentrate in one file, one type.
- *Leverage:* `kask_bridge` and `hkask-mcp-kata-kanban` import a smaller
  surface; no more trait-vs-inherent ambiguity.
- *Testability:* `bootstrap_test.rs` asserts against `SqliteRegistry` directly
  (already does for some checks).

**Risks:** `Registry` is used by in-crate tests as a fast in-memory mock.
Migration: those tests either use `SqliteRegistry` (in-memory `:memory:` pool)
or a typed test fake. Prior seam-audit (RA-01/09/10) deferred this; the
deferral reasons should be re-checked before proceeding.

### CAND-2 (Strong) — Extract sub-cascade orchestration

**Files:** `executor.rs`, `step_actions.rs::execute_flowdef`,
`step_actions.rs::execute_parallel`.

**Problem:** Sub-cascade orchestration (StepGraph + StepContext + BudgetTracker
+ ConvergenceTracker + StepMachine + run) is triplicated. The `MAX_STEPS` hard
gate is applied only in the executor path; the two sub-cascade paths get only
the advisory warn.

**Solution:** Extract a `run_sub_cascade(manifest, parent_context, infra) ->
CascadeOutcome` function (deep module). All three call sites invoke it. The
hard gate lives in one place.

**Benefits:**
- *Locality:* orchestration concerns concentrate in one function.
- *Leverage:* `execute_flowdef` and `execute_parallel` shrink by ~40 lines each.
- *Testability:* sub-cascade orchestration becomes testable in isolation.

**Risks:** The three call sites have subtle differences (e.g. `parallel`
constructs per-branch trackers). The extraction must parameterize those
differences, not flatten them.

### CAND-3 (Strong) — Split `step_actions.rs` along action seams

**Files:** `step_actions.rs` (2,895 lines), `step_machine.rs`.

**Problem:** `step_actions.rs` is the largest module by 2×, with `impl
StepMachine` split across two files. The "40-80 lines each" claim is stale by
5×. Action methods range from 50 to 470 lines.

**Solution:** Split into per-action modules behind a `step_actions/` facade:
`choice.rs`, `loop.rs`, `select.rs`, `populate.rs`, `compute.rs`,
`render.rs`, `tool_invoke.rs`, `tool_batch.rs`, `flowdef.rs`, `parallel.rs`,
`gate.rs`, plus `inference.rs` (the `call_inference_stream*` pair) and
`render_helpers.rs` (`render_step_template*`). Each module owns one action's
`impl StepMachine` block. The facade re-exports the `Effect` enum and the
dispatch surface.

**Benefits:**
- *Locality:* each action's logic, helpers, and tests concentrate in one file.
- *Leverage:* `step_machine.rs::dispatch_action` imports from a facade, not a
  2,895-line file.
- *Testability:* each action testable in isolation with a minimal `Infra`.

**Risks:** Rust allows `impl` blocks in multiple files only via `mod`; the
split is mechanical but touches every `use crate::step_actions::*` site. The
`impl StepMachine` split is already a fact — this candidate makes it
navigable rather than accidental.

### CAND-4 (Strong) — Decompose `dispatch_compute` god-function

**Files:** `compute.rs` (1,270 lines prod).

**Problem:** 830-line `match` with 17 arms, ad-hoc JSON destructuring per arm,
three private helpers consumed by exactly one arm each. Arms are cohesive
sub-domains (forecast, kata, swarm, listening, lisp, shell).

**Solution:** Extract each sub-domain into a sibling module behind a
`compute/` facade: `forecast.rs`, `kata.rs`, `swarm.rs`, `listening.rs`,
`lisp.rs`, `shell.rs`. The facade holds the dispatch table (a `const fn` or
a `match` reduced to one line per arm delegating to `arm::run(input)`).

**Benefits:**
- *Locality:* each sub-domain's math/helpers concentrate in one file.
- *Leverage:* proptest regressions can target one arm.
- *Testability:* `shell_exec` gets an injection seam (the only untestable arm).

**Risks:** The arms share JSON-destructuring helpers (`get_f64`/`get_bool`/
`get_u64` closures redefined per arm). Extracting a shared `compute_input`
helper is part of the work.

### CAND-5 (Worth exploring) — Collapse `ManifestFile` into `BundleManifest`

**Files:** `manifest_loader.rs`, `bundle/manifest.rs`.

**Problem:** `ManifestFile` is a 14-field structural duplicate of
`BundleManifest` with different serde attributes. `load_manifest_from_yaml`
hand-copies 18 fields. Adding a field requires editing three sites with no
compile-time enforcement.

**Solution:** Use `#[serde(flatten)]` on `BundleManifest` to absorb the
`manifest:` wrapper, eliminating `ManifestFile`. The `ManifestHeader` peer
fields stay as a separate struct.

**Benefits:**
- *Locality:* manifest shape lives in one type.
- *Leverage:* field additions touch one site.
- *Testability:* no lockstep drift between two structs.

**Risks:** `#[serde(flatten)]` has known interactions with
`deny_unknown_fields` and `serde_yaml_neo`. Needs a spike to confirm the
flattening preserves the existing parse behavior across all 30+ manifests.

### CAND-6 (Worth exploring) — Extract shared frontmatter/Jinja strippers

**Files:** `output_schema.rs`, `inputs.rs`, `template_renderer.rs`.

**Problem:** Duplicated Jinja-comment stripper (C1), duplicated frontmatter
parser (C2), duplicated inference-block stripping (the renderer parses it
just to discard; the caller parses it to read).

**Solution:** Extract a `frontmatter.rs` module owning: `strip_jinja_comments`,
`extract_frontmatter_block` (returns `(yaml, inference_block, body)`),
`parse_contract_field`. `output_schema` and `inputs` become consumers.

**Benefits:**
- *Locality:* frontmatter parsing concentrates in one module.
- *Leverage:* `template_renderer::render` stops parsing the inference block
  twice.
- *Testability:* one parser, one test suite.

**Risks:** Low. The duplication is admitted in doc comments.

### CAND-7 (Worth exploring) — Type-discriminate `BundleManifestStep`

**Files:** `bundle/manifest.rs`.

**Problem:** 18-field god-struct; every action type uses a different subset.
A `gate` with `template_ref` or `select` with `command` is structurally
representable but semantically invalid.

**Solution:** Enum-of-structs: `BundleManifestStep::Select(SelectStep)`,
`::Execute(ExecuteStep)`, `::Compute(ComputeStep)`, etc. Each variant carries
only its valid fields. `StepNode` becomes the same enum (or a `match` in
`step_graph::new`).

**Benefits:**
- *Locality:* invalid states unrepresentable (Hoare P1).
- *Leverage:* `dispatch_action` becomes an exhaustive `match` on the enum.
- *Testability:* each action's valid field set is compile-time enforced.

**Risks:** High blast radius — every step construction site, every `StepNode`
reader, every manifest YAML parse. This is the highest-risk candidate. May
defer pending CAND-3 (which would localize the per-action readers).

### CAND-8 (Worth exploring) — Split `ConvergenceTracker` into Kata + legacy

**Files:** `convergence.rs` (1,157 lines).

**Problem:** Two convergence models coexist in one 14-field struct. Every
public method branches on `kata_enabled()`. A manifest uses one or the other,
never both.

**Solution:** `ConvergenceTracker` becomes a trait or enum with `Kata` and
`Legacy` variants. Each variant owns its 7 fields and its method
implementations. `check_met`/`push_cycle_from_context` dispatch on the variant.

**Benefits:**
- *Locality:* each model's math concentrates in one variant.
- *Leverage:* a manifest using Kata doesn't carry legacy fields.
- *Testability:* each model testable in isolation.

**Risks:** `ConvergenceConfig` carries both models' fields today; splitting
the tracker may require splitting the config too (C8 duplication).

### CAND-9 (Worth exploring) — Fix broken feedback loops in `SqliteRegistry`

**Files:** `registry_sqlite.rs`.

**Problem:** 4 methods return `Ok(())` on write failure (B1). `row_to_skill`
silently defaults `Visibility→Private` (B2, security-relevant). `count`
returns 0 on error (B3).

**Solution:** Propagate errors: `register_skill`/`register_bundle`/`delete_entry`/
`remove_skill` return `Result<(), TemplateError>` and propagate SQL errors.
`row_to_skill` returns `Result<Skill, _>` and warns on unknown enum strings.
`count` propagates the pool error.

**Benefits:**
- *Locality:* error handling concentrates at the SQL boundary.
- *Leverage:* callers can distinguish "succeeded" from "failed silently."
- *Testability:* error-path tests become possible.

**Risks:** Callers that assumed `Ok(())` may need to handle the new `Err`.
This is a behavior change, not a pure refactor. May be gated behind a
separate PR with its own release note.

### CAND-10 (Speculative) — Remove dead public surface

**Files:** `hkask_templates.rs` (re-exports), `prompt_strategy.rs`,
`inputs.rs` (dead fns), `budget.rs` (dead methods), `step_machine.rs`
(`classify_failure_mode`), `step_graph.rs` (`Effect::ConsumedGas`).

**Problem:** 22 of 32 crate-root re-exports have zero external consumers. Dead
modules (`PromptStrategy`), dead functions (`render_input_param_spec`,
`extract_contract_input_keys`), dead methods (`from_remaining`,
`last_rjoule_cost`, `classify_failure_mode`), dead effect variants
(`ConsumedGas`).

**Solution:** Remove from the crate root. Demote to `pub(crate)` or delete.
For items retained for `deny_unknown_fields` backcomat (D8), document as
"accepted but ignored" with a link to the enforcement gate (which is "none").

**Benefits:**
- *Locality:* public surface matches actual consumers.
- *Leverage:* `kask_bridge` import list shrinks.
- *Testability:* fewer dead items to maintain.

**Risks:** Some zero-consumer re-exports are exercised by in-crate contract
tests. Demoting them to `pub(crate)` may break those tests if they import via
the crate root. Each item needs a per-item deletion test (essentialist G1).

### CAND-11 (Speculative) — Inline `concurrency.rs`, `bundle/cascade.rs`, pass-throughs

**Files:** `concurrency.rs`, `bundle/cascade.rs`, `step_actions.rs:1557,54`,
`bundle/manifest.rs:204`, `bundle/composition.rs:84-109`, `executor.rs:189`.

**Problem:** F1, F2, F5, F6, F7, F8, F9 — pass-through wrappers that fail the
deletion test.

**Solution:** Inline each into its sole consumer. Delete the wrapper.

**Benefits:**
- *Locality:* fewer indirections.
- *Leverage:* negligible.
- *Testability:* unchanged.

**Risks:** Low individually; collectively a large diff. Bundle into one
"pass-through sweep" commit.

## Recommended sequencing

1. **CAND-10** (dead surface removal) — clears the ground; reduces noise for
   subsequent candidates. One commit per item cluster.
2. **CAND-11** (pass-through sweep) — mechanical, low-risk, one commit.
3. **CAND-9** (SqliteRegistry feedback loops) — behavior change, separate PR.
4. **CAND-6** (frontmatter/Jinja dedup) — low-risk, unblocks CAND-3.
5. **CAND-2** (sub-cascade extraction) — medium-risk, unblocks CAND-3.
6. **CAND-3** (split step_actions) — large mechanical split, depends on
   CAND-2.
7. **CAND-4** (split dispatch_compute) — independent, parallel with CAND-3.
8. **CAND-1** (collapse registry trait layer) — depends on CAND-10 for the
   `Registry` deletion; re-check prior deferral reasons.
9. **CAND-5** (ManifestFile collapse) — needs a spike; defer pending spike
   result.
10. **CAND-8** (split ConvergenceTracker) — defer pending CAND-3 (localizes
    the per-model readers).
11. **CAND-7** (type-discriminate BundleManifestStep) — highest risk; defer
    pending CAND-3 + CAND-8.

## Verification plan (per candidate)

- **Dependency direction:** `cargo tree` + grep for back-edges.
- **Depth test:** re-apply deletion test to each new module post-extraction.
- **P6/P7/P8:** no stubs, no deprecation attributes, all tests verify stated
  behavioral properties.
- **Clippy:** `./script/clippy` (per `.rules`, not `cargo clippy`).
- **Test suite:** `cargo test -p hkask-templates` + the four consumer crates.
- **Surface adapter thinness:** `kask_bridge` import list before/after.

## Open questions for review

1. **CAND-1 deferral:** The prior seam-audit deferred RA-01/09/10 (registry
   trait deletion). Are the deferral reasons still load-bearing, or has the
   migration path (in-memory `:memory:` `SqliteRegistry` for tests) matured?
2. **CAND-7 risk:** Is the type-discrimination of `BundleManifestStep` worth
   the blast radius, or should it stay a god-struct with runtime validation?
3. **CAND-9 scope:** Should the feedback-loop fixes be a refactor or a
   behavior-change PR? The `.rules` say "propagate errors" but callers may
   depend on the silent `Ok(())`.
4. **CAND-10 vs contract tests:** Which zero-consumer re-exports are exercised
   by in-crate contract tests that import via the crate root? Demoting them
   breaks those tests; deleting them removes the contract.
5. **CAND-3 vs CAND-4 ordering:** Both are large splits. Should they run in
   parallel (disjoint write sets) or sequentially (CAND-3 first to establish
   the split pattern)?
