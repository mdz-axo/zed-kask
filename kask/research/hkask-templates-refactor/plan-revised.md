# Refactor-Architecture Plan — `hkask-templates` (REVISED v1)

> Method: `refactor-architecture` (explore → candidates → deepen → audit →
> strangle → verify). Revised after multi-perspective review (metacognition,
> idiomatic-rust, grill-me, essentialist, pragmatic-semantics,
> pragmatic-cybernetics). See `multi-perspective-review.md` for the per-skill
> findings and consensus that drove this revision.

## Grounding & verification status

All sizing claims in this revision were re-measured against the crate via
grep/wc on 2026-08-17. The verification greps are recorded in
`multi-perspective-review.md`.

### Verified against source (re-measured this revision)

| Claim | Source | Value |
|-------|--------|-------|
| `compute.rs` line count | `wc -l` | 3,379 (was 1,270 in v0 — stale) |
| `step_actions.rs` line count | `wc -l` | 3,146 (was 2,895 in v0 — stale) |
| `step_graph.rs` line count | `wc -l` | 322 |
| `bundle/manifest.rs` line count | `wc -l` | 598 |
| `registry_sqlite.rs` line count | `wc -l` | 825 |
| `dispatch_compute` string arms | `grep -cE '^\s+"[a-z_.]+"\s*=>'` | 19 (was 17 in v0; grill-me's "8" was wrong) |
| `dispatch_compute` sub-domains | grep enumeration | 6: forecast (8 arms), kata (4), swarm (3), listening (2), lisp (1), shell (1) |
| `Effect::ConsumedGas` producers | `grep -rEn "Effect::ConsumedGas\("` | **0** — dead variant (never produced) |
| `Registry` external consumers | `grep -rEn "hkask_templates::Registry\b"` | **0 external** — 1 in-crate test (`bootstrap_test.rs:12`) |
| `RegistryIndex` / `SkillRegistryIndex` ownership | `grep -rn "trait ..."` | Both in `kask/crates/hkask-types/src/ports/registry.rs:286,309` — **cross-crate** |
| `BundleRegistryIndex` ownership | grep | In `kask/crates/hkask-templates/src/bundle/mod.rs` — local |
| `BundleManifest` `deny_unknown_fields` | `read_file bundle/manifest.rs:212` | **Absent** — only `#[non_exhaustive]`. CAND-5 spike must test `ManifestFile` (the wrapper), not `BundleManifest`. |

### Prior seam-audit reconciliation

The prior seam-audit (`research/seam-audit/refactor-architecture-review.md`)
marked:
- **RA-01** (`SkillRegistryIndex`): `deferred: no` — **NOT deferred.** v0
  mis-attributed this. Struck from the deferred grouping.
- **RA-09** (`Registry`): `deferred: yes` — reason: "may be intentional
  research/test scaffolding; the user decides delete vs cfg-gate vs
  wire-a-real-consumer." This is a meta-reason (defer to operator), not a
  technical blocker. **Operator approval of this plan resolves the deferral.**
- **RA-10** (`BundleRegistryIndex`): `deferred: yes` — same reason; depends
  on RA-03 (per `mcda-remediation.md:29`).

The prior audit's strangler-fig migration plan (L67-76) is the execution
template for CAND-1a.

## Scope

Crate: `kask/crates/hkask-templates/` — 25 `.rs` files under `src/` (incl.
`bundle/`), ~13,200 lines total (prod + tests; the prod/test split needs
re-measurement per file). Public surface re-exported at
`src/hkask_templates.rs:34-57` (36 individual symbols across 11 `pub use`
statements — v0's "32" was unreproducible). Four external consumers:
`kask_bridge` (widest, 12 items — verified), `hkask-mcp-kata-kanban` (2),
`hkask-mcp-media` (submodule-only, bypasses crate root), `kask_extensions_ui`
(4 seed fns — DIVERGENCE surface).

## Friction inventory (from exploration, re-grounded)

Grouped by signal. File:line citations are from the survey sub-agents and
re-verified where load-bearing.

### A. Shallow modules / pass-through wrappers (deletion-test FAIL)

| ID | Item | Location | Evidence |
|----|------|----------|----------|
| F1 | `concurrency.rs` — 13-line `pub use` re-export of `hkask_types::concurrency` | `src/concurrency.rs:1-13` | Pure pass-through; 3 in-crate import sites could use `hkask_types::concurrency` directly (one test already does). |
| F2 | `bundle/cascade.rs` — 23-line file for a 3-variant `CascadePhase` enum | `src/bundle/cascade.rs:1-23` | One consumer (`bundle/manifest.rs`). Could inline into `manifest.rs`. |
| F3 | `FsSkillReader` — unit struct, no trait, one-line body, one consumer, no injection | `src/ports.rs:216` | Testability theater; `SkillLoader::discover_skills` bypasses it via direct `fs::read_dir`. |
| F4 | `render_minijinja` — one-shot wrapper rebuilding `TemplateRenderer` per call | `src/template_renderer.rs:436` | Re-introduces the per-call cost the cached renderer was built to avoid. **Fix: delete, don't inline — callers should hold a `TemplateRenderer`.** |
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
| D3 | ~22 of 36 crate-root re-exports have zero external consumers | `hkask_templates.rs:34-57` | **Count needs re-verification** — v0 said "22 of 32" (unreproducible); actual symbol count is 36. Reconciliation grep required before CAND-10 executes. |
| D4 | `BudgetTracker::from_remaining`, `last_rjoule_cost` — documented "Not yet wired" | `src/budget.rs:130,176` | Speculative surface inside a live module. |
| D5 | `classify_failure_mode` — `#[allow(dead_code)]` "not yet wired" | `src/step_machine.rs:741` | 16-arm match with no caller. |
| D6 | `Effect::ConsumedGas(amount)` — **never produced** (grep-verified) | `src/step_actions.rs:49` (def), `src/step_machine.rs:623` (consumer) | Dead effect variant. v0's "silently dropped" was imprecise; metacognition's "documented no-op" was wrong. **Idiomatic-rust + grep: 0 producers.** |
| D7 | `BundleManifest::concurrency` — parsed, round-tripped, documented "not yet enforced" | `src/bundle/manifest.rs:266` | Advertised non-invariant. |
| D8 | `ErrorHandlingConfig::on_gas_exceeded`/`on_validation_failure`/`on_capability_denied` — "accepted but ignored" | `src/bundle/config.rs:341,367,371` | Three dead fields retained for `deny_unknown_fields` backcomat. |
| D9 | `BundleManifestStep::gas_cap` — documented "informational" | `src/bundle/manifest.rs:93` | `total_step_gas` computes a number nothing enforces. |
| D10 | `step_context::materialize` — "until K5" comment stale (K5 landed) | `src/step_context.rs:267` | Still called by bridge; comment describes a future that arrived. |
| D11 | `parse_front_matter` — `pub` with only `Self::` internal caller | `src/skill_loader.rs:389` | Candidate dead surface (needs grep confirmation). |

### C. Duplicated logic

| ID | Duplication | Locations | Note |
|----|-------------|-----------|------|
| C1 | Jinja-comment stripper | `output_schema::strip_jinja_comments:84`, `inputs::strip_jinja_comments_inputs:222` | Byte-equivalent; `inputs.rs` doc admits the duplication. |
| C2 | Frontmatter parser (find `---` → strip Jinja → strip `[inference]` → YAML → `contract.<X>`) | `output_schema::extract_contract_output:50`, `inputs::extract_contract_input_keys:189` | Same 5-step pipeline, two sub-keys. |
| C3 | Operator scan `[<=, >=, ==, !=, <, >]` | `condition::parse_step_comparison:76`, `condition::parse_choice_condition:194` | Same module, two copies, one table. |
| C4 | Sub-cascade orchestration (StepGraph + StepContext + BudgetTracker + ConvergenceTracker + StepMachine + run) | `executor.rs:214-240`, `step_actions.rs::execute_flowdef ~998-1028`, `step_actions.rs::execute_parallel ~1192-1340` | Triplicated; `MAX_STEPS` hard gate applied only in executor path. **5 concrete differences** (budget constructor, context merge, depth, manifest_id, post-run merge) — see CAND-2. |
| C5 | `call_inference_stream` / `call_inference_stream_with_messages` | `step_actions.rs:1731,1832` | Near-duplicates; shared streaming+timeout+accumulation body. |
| C6 | `finalize_report` / `inject_running` JSON builders | `convergence.rs:523-575,580-619` | ~80% field overlap; no shared struct/builder. |
| C7 | `ManifestFile` structural duplicate of `BundleManifest` | `manifest_loader.rs:42-142` | 13-field mirror; hand-copied in `load_manifest_from_yaml`. |
| C8 | `ConvergenceConfig` defaults duplicated between `Default` impl and 9 serde default fns | `bundle/config.rs:186-249` | Two sources of truth. |
| C9 | `compute_compound_quality` step_ordinal→`step_N_result` key construction | `convergence.rs:452-516` | Duplicated 3× within one function. |
| C10 | `resolve_quality` 3-step fallback chain duplicated in spirit by `push_cycle_from_context` legacy arm + `check_legacy_met` | `convergence.rs:278-292` | Bug fix routed through one private fn called from 3 sites. |

### D. Wide / god-struct surfaces

| ID | Item | Location | Note |
|----|------|----------|------|
| W1 | `StepNode` — 22 public fields (re-counted), no encapsulation | `step_graph.rs:163-186` | ~18 of 22 are pass-through clones of `BundleManifestStep`. |
| W2 | `Infra` — 10 public fields, no constructor | `step_machine.rs` | Built by field-literal at one site (`executor.rs:219-230`); adding a field touches every action. |
| W3 | `BundleManifestStep` — 20-field god-struct (re-counted), all action types share it | `bundle/manifest.rs` | `gate` with `template_ref` or `select` with `command` is structurally representable but semantically invalid. **Verified: `validate()` does NOT reject per-action field misuse** (essentialist finding). |
| W4 | `dispatch_compute` — 832-line match, 19 string arms, ad-hoc JSON destructuring per arm | `compute.rs` | God-function; 6 cohesive sub-domains (forecast/kata/swarm/listening/lisp/shell). **Closures are defined ONCE at the top (`compute.rs:229-249`), not "redefined per arm"** (grill-me correction); some arms bypass them with inline destructuring. |
| W5 | `step_actions.rs` — 3,146 lines (re-counted), `impl StepMachine` split across two files | `step_actions.rs` | Largest module by 2×. |
| W6 | `BudgetTracker` — 14 public methods, `BudgetSnapshot` 8 public fields | `budget.rs` | Executor reaches into tracker across 12 methods. |
| W7 | `ConvergenceTracker` — 14 public methods, two models (Kata + legacy) in one struct | `convergence.rs` | 7 fields per model, no type separation, manifest uses one-or-other never both. **Branching is a 1-line `if kata_enabled()` guard** (essentialist finding). |
| W8 | `BundleManifest::validate` — 128 lines, 9 interleaved checks, non-separable | `bundle/manifest.rs:297` | `polarities_in` closure captures `&self`, non-reusable. |

### E. Broken feedback loops (`.rules` traps)

**Status note (post-execution):** B1, B5, B6, B9 are now fixed in commits
`f1d95635c7`, `9e90e0381b`, `9a24b4bd4b`. B2 and B7 were already partially
addressed (warns present, still default). B3 was a false alarm (no production
callers). B4 is fixed (warn at `BudgetTracker::new`). B8 is addressed by
CAND-1a (deletes `Registry`). B10 is addressed by CAND-2.

| ID | Item | Location | Note | Status |
|----|------|----------|------|--------|
| B1 | `SqliteRegistry::register_skill`/`register_bundle`/`delete_entry`/`remove_skill` return `Ok(())` on write failure | `registry_sqlite.rs:333,385,206,369` | **Survey was wrong** — errors already propagated via `?` + `.map_err(...)`. Commit `f1d95635c7` confirmed and tightened the propagation. | **Fixed** |
| B2 | `SqliteRegistry::row_to_skill` silent `unwrap_or` defaults on parse failure | `registry_sqlite.rs:529` | `Visibility→Private` silent downgrade is security-relevant. **Already warns** (L560-580); still defaults to `Private`. The cybernetics recommendation to add `Visibility::Unknown` variant is still valid but lower priority. | **Partially fixed** |
| B3 | `SqliteRegistry::count` returns 0 on error ("graceful degradation") | `registry_sqlite.rs:235` | **Survey was wrong** — `count` has zero production callers (grep-verified; only in-crate tests call it). The "regulation-loop sense input" claim was fabricated. No fix needed. | **False alarm** |
| B4 | `RjouleConfig::cap` defaults to 0 as "disabled" sentinel | `bundle/config.rs:319` | Cannot distinguish "intentionally unlimited" from "forgot to configure." **Fixed:** `BudgetTracker::new` now warns when `rjoule.cap == 0` (commit `9a24b4bd4b`). The `Option<NonZeroU64>` / enum change is deferred — the warn + `manifest_compliance.rs` validation gate together surface the failure mode. | **Fixed (warn)** |
| B5 | `input_mapping::resolve_mapping_value` silent fallback to `value.clone()` on render/parse failure | `input_mapping.rs:65,67` | Two silent fallbacks in 10 lines, no warn. **Fixed:** all three fallback paths now `tracing::warn!` naming the failed render/parse and the template expression (commit `9e90e0381b`). | **Fixed** |
| B6 | `output_schema::contract_output_to_schema` unknown type → `{"type":"string"}` silent narrow | `output_schema.rs:155` | Typo'd type silently narrows schema. **Fixed:** unknown types now `tracing::warn!` naming the field and declared type before narrowing (commit `9e90e0381b`). | **Fixed** |
| B7 | `skill_loader::infer_domain_from_registry` defaults to `KnowAct` on every failure mode | `skill_loader.rs:298,309,324` | **Survey was partially wrong** — the method already warns on unreadable/unparsable manifests (L303, L317). It still defaults to `KnowAct`, but the warns are present. The `Result<Domain, _>` change is deferred — the warns surface the drift. | **Partially fixed** |
| B8 | `Registry::reload` clears templates but not skills/bundles | `registry.rs:213` | Doc says "refreshes from filesystem"; skills/bundles survive. **Addressed by CAND-1a** (deletes `Registry` entirely). | **Addressed by CAND-1a** |
| B9 | `template_renderer::load` doc contradicts code (3 embedded-seed fallbacks) | `template_renderer.rs:96`, `step_actions.rs:957-972,1176-1191`, `hkask-mcp-kata-kanban/kata/execution.rs:93-103` | Doc contradicts code. **Fixed:** doc now documents the fallback behavior and names the 3 production call sites (commit `9a24b4bd4b`). | **Fixed** |
| B10 | `step_graph::MAX_STEPS` advisory warn + executor hard gate — dual-write | `step_graph.rs:132`, `executor.rs:206` | Two enforcement points for one invariant. Sub-cascade paths get only the warn. **Addressed by CAND-2.** | **Addressed by CAND-2** |

### F. Coupling / dependency-direction issues

| ID | Item | Location | Note |
|----|------|----------|------|
| P1 | `impl StepMachine` split across `step_machine.rs` + `step_actions.rs` | — | Tightest coupling in crate; `step_actions.rs` is an impl extension, not a module. |
| P2 | Bidirectional dep: `step_actions` ↔ `executor` (utilities placed for import convenience) | `step_actions.rs:408,1025,1343`, `step_machine.rs:361` | `extract_feedback_phase`/`normalize_model_output`/`parse_json_response` have no logical home in `executor.rs`. |
| P3 | `ports::ManifestResolveError` reaches upward into `manifest_loader::ManifestLoadError` | `ports.rs:200` | Backwards dependency from foundation to consumer. |
| P4 | `step_machine::dispatch_with_retry` inlines curator MCP call | `step_machine.rs:530-577` | Skill-execution domain leaks into interpreter. |
| P5 | `merge_control_flow` constructs `step_{ordinal}_result` string key — couples machine to context's naming convention | `step_machine.rs:660` | String-key contract lives in call site, not type. **Pairs with CAND-3-minimal** (cybernetics finding). |
| P6 | `hkask-mcp-media` bypasses crate root, uses `pub mod budget`/`bundle::config` directly | `hkask-mcp-media/src/budget.rs` | Crate-root surface is not the actual public API. |
| P7 | `kask_extensions_ui` (upstream Zed tree) consumes 4 seed fns — DIVERGENCE surface | `kask_extensions_ui/src/publish.rs` | Changes to seed fns ripple across the Kask↔Zed seam. |

## Candidate refactor packages (revised)

Re-ranked by leverage × locality × testability, informed by the six reviews.
Essentialist verdicts (G1/G2/G3) and idiomatic-rust Hoare assessments are
noted where load-bearing.

### CAND-1a (Deferred) — Collapse the local registry trait layer

**Status: Deferred pending operator decision.** The audit confirmed `Registry`
has zero external consumers (grep-verified: only `bootstrap_test.rs:12`).
However, `bootstrap_test.rs` is a real build-time safety net — it verifies that
`build.rs` correctly discovers and embeds manifests via `Registry::bootstrap()`.
`SqliteRegistry` doesn't have a `bootstrap()` method (it's a runtime registry,
not a compile-time one). Deleting `Registry` requires either:
- (a) adding `SqliteRegistry::bootstrap()` that seeds from `MANIFEST_YAMLS`, or
- (b) rewriting `bootstrap_test.rs` to seed a `SqliteRegistry` from
  `MANIFEST_YAMLS` inline and assert against it.

Both are non-trivial. The prior seam-audit's strangler-fig migration plan
(L67-76) said "Migrate `bootstrap_test.rs` to assert against `SqliteRegistry`
directly" but didn't specify which option. **Operator decision needed.**

**Files:** `registry.rs`, `registry_sqlite.rs`, `bundle/mod.rs` (trait def),
`hkask_templates.rs` (re-exports), `tests/bootstrap_test.rs`.

**Problem:** `Registry` (in-memory) has zero external consumers (grep-verified:
only `bootstrap_test.rs:12`). `BundleRegistryIndex` trait has 2 impls but one
(`Registry`) is test-only → effectively single-use in production. The
inherent-vs-trait delegation wrappers (~120 lines) exist only to disambiguate
trait-method-name collision. The "owned" suffix on `SqliteRegistry` is the same
contortion.

**Solution:** Delete `Registry` (in-memory) and `BundleRegistryIndex` trait.
Move the surviving methods to inherent `SqliteRegistry` methods. Delete the
`*_owned` forwarders. Keep `MANIFEST_YAMLS` seed accessors (live, 2 consumers
each). **Keep the three index traits** (`RegistryIndex`, `SkillRegistryIndex`
from `hkask-types`, `BundleRegistryIndex` locally) — wait, `BundleRegistryIndex`
is local and is being deleted. The `hkask-types` traits stay (they're not in
this crate's scope; see CAND-1b).

**Essentialist verdict:** G1 PASS (behavior lost: polymorphic dispatch, but
unused in production → cosmetic). G2 PASS (net surface reduction). G3 PASS
(`*_owned` forwarders are pass-through wrappers).

**Idiomatic-rust note:** Hoare P6 (composition over inheritance) favors many
small capability traits. The `.rules` dead-code rule applies to `Registry` (the
impl), not to the trait *definition*. The traits are a composition seam for test
doubles — but `Registry` is the only test double, and it's being deleted. If
future test doubles are needed, the traits can be re-added. **Reframe: delete
`Registry` + `BundleRegistryIndex` + `*_owned`; the `hkask-types` traits stay
on `SqliteRegistry` as inherent-style trait impls.**

**Benefits:**
- *Locality:* registry concerns concentrate in one file, one type.
- *Leverage:* `kask_bridge` and `hkask-mcp-kata-kanban` import a smaller
  surface; no more trait-vs-inherent ambiguity.
- *Testability:* `bootstrap_test.rs` asserts against `SqliteRegistry` directly
  (in-memory `:memory:` pool with `max_size(1)` to avoid the multi-connection
  in-memory database bug — idiomatic-rust finding).

**Risks:** `bootstrap_test.rs` uses `Registry::bootstrap()`. Migration: verify
`SqliteRegistry::bootstrap()` exists (or add it). Use `:memory:` SQLite with
`Pool::max_size(1)`. **Prior deferral (RA-09/RA-10) resolved by operator
approval of this plan** — the deferral reason ("user decides") is a
meta-reason, not a technical blocker. Execution template: prior audit's
strangler-fig migration plan (L67-76).

**Success criteria:** `Registry` type deleted; `BundleRegistryIndex` trait
deleted; `*_owned` forwarders deleted; `bootstrap_test.rs` passes against
`SqliteRegistry`; `cargo check` clean; `./script/clippy` clean.

### CAND-1b (Deferred) — Cross-crate trait deletion in `hkask-types`

**Files:** `kask/crates/hkask-types/src/ports/registry.rs`,
`kask/crates/hkask-templates/src/registry_sqlite.rs`,
`kask/crates/hkask-templates/src/registry.rs` (if it still exists).

**Problem:** `RegistryIndex` and `SkillRegistryIndex` are defined in
`hkask-types`, not `hkask-templates`. v0's CAND-1 claimed all three traits
were in scope; grill-me and pragmatic-semantics corrected this.

**Solution:** After CAND-1a lands and `Registry` is gone, evaluate whether
`SqliteRegistry` is the only impl of `RegistryIndex`/`SkillRegistryIndex`. If
yes, delete the traits from `hkask-types` and move the methods to inherent
`SqliteRegistry` methods. **This is a cross-crate change** — separate PR,
separate release note.

**Deferred because:** cross-crate, lower leverage (the traits are in
`hkask-types`, which has other consumers), and the essentialist G3 verdict
(traits with 1 impl → single-use → inline) only applies after CAND-1a removes
the second impl.

### CAND-2 (Strong) — Extract `SubCascade` builder

**Files:** `executor.rs`, `step_actions.rs::execute_flowdef`,
`step_actions.rs::execute_parallel`.

**Problem:** Sub-cascade orchestration is triplicated (C4). The `MAX_STEPS`
hard gate is applied only in the executor path (grep-verified); the two
sub-cascade paths get only the advisory warn (B10). The three call sites have
**5 concrete differences** (grill-me enumeration):
1. Budget constructor: `BudgetTracker::new` (executor, flowdef) vs
   `BudgetTracker::from_remaining_shared` (parallel).
2. Context merge: clone-and-merge-back (flowdef) vs clone-template (parallel).
3. Depth tracking: flowdef increments `self.depth + 1`; parallel doesn't.
4. Manifest ID suffix: `::flowdef` vs `::parallel`.
5. Post-run merge: flowdef merges back via `merge_back_sub_cascade`; parallel
   collects by `branch_id`.

v0's single-signature `run_sub_cascade(manifest, parent_context, infra)`
cannot accommodate these without parameter explosion.

**Solution (idiomatic-rust design):** Extract a `SubCascade` builder struct
with two constructors:

```rust
pub struct SubCascade {
    machine: StepMachine,
    parent_step_id: StepId,
    merge_context: bool,
}

impl SubCascade {
    /// Construct a flowdef sub-cascade (single child, context-merging).
    pub fn for_flowdef(manifest, parent_context, parent_budget, parent_manifest_id, depth) -> Result<Self, TemplateError> {
        // MAX_STEPS hard gate — single enforcement point (fixes B10)
        Self::check_cap(&manifest)?;
        let sub_budget = BudgetTracker::capped_by(parent_budget, &manifest.gas, &manifest.rjoule);
        // ... construct machine with format!("{}::flowdef", parent_manifest_id), depth+1
        Ok(Self { machine, parent_step_id, merge_context: true })
    }

    /// Construct a parallel branch sub-cascade (shared atomic gas, no context merge).
    pub fn for_parallel_branch(manifest, branch_context, shared_gas, branch_rjoule_cap, parent_manifest_id, depth) -> Result<Self, TemplateError> {
        Self::check_cap(&manifest)?;
        let sub_budget = BudgetTracker::with_shared_gas(shared_gas, branch_rjoule_cap);
        // ... construct machine with format!("{}::parallel", parent_manifest_id)
        Ok(Self { machine, parent_step_id, merge_context: false })
    }

    /// Run. Box::pin to guard recursion depth (P8-adjacent — v0 omitted this).
    pub async fn run(self, infra: Infra) -> Result<CascadeOutcome> {
        Box::pin(self.machine.run(infra)).await
    }

    fn check_cap(manifest: &BundleManifest) -> Result<(), TemplateError> {
        if manifest.steps.len() > MAX_STEPS {
            return Err(TemplateError::Manifest(format!(
                "Sub-cascade '{}' has {} steps — exceeds cap {}", manifest.id, manifest.steps.len(), MAX_STEPS,
            )));
        }
        Ok(())
    }
}
```

**Ownership DAG:** `SubCascade` owns the `StepMachine` (single owner).
`execute_flowdef` owns the `SubCascade`. `execute_parallel` owns
`Vec<SubCascade>` (one per branch). The shared `Arc<AtomicU64>` gas is
shared-mutable across branches — the only shared-mutable value, and it's
atomic (correct).

**P8-adjacent:** `Box::pin` is preserved (recursion-depth guard). v0 omitted
this; idiomatic-rust flagged it.

**Cancellation semantics (idiomatic-rust edge case):** if the parent cascade
is cancelled mid-`execute_parallel`, branches are dropped (correct), but the
shared `Arc<AtomicU64>` gas has already been debited by cancelled branches.
The budget is over-charged. **Address:** either (a) accept the over-charge
(branches did real work before cancellation), or (b) refund on cancel (track
per-branch debits and reverse). Recommend (a) with a `tracing::warn!` naming
the over-charge.

**Essentialist verdict:** G1 PASS (triplication verified; inlining re-triplicates). G2 PASS conditional (facade re-exports ONLY `SubCascade` + `CascadeOutcome`; if >7 items, reduce). G3 PASS (adds orchestration behavior, not a wrapper).

**Idiomatic-rust Hoare assessment:** P1 High (MAX_STEPS violation unrepresentable), P5 Medium+ (gate explicit in one place), P8 addressed (Box::pin). Critique score 0.20 (revised from v0's 0.45).

**Success criteria:** `MAX_STEPS` hard gate fires in all 3 paths (test asserts
this); `execute_flowdef` and `execute_parallel` shrink by ~40 lines each;
`SubCascade` struct ≤ 200 lines; `Box::pin` preserved; cancellation over-charge
logged.

### CAND-3-minimal (Deferred) — Extract `call_inference_stream*` only

**Status: Deferred.** The audit found `call_inference_stream` is dead in
production (`#[allow(dead_code)]`) but pinned by 2 `// zed-kask: D25` tests
(`call_inference_stream_threads_finish_reason_length`,
`execute_select_empty_output_guard_pins_stream_shape`). The "dedup" is really
"delete the dead one + keep the live one (`call_inference_stream_with_messages`)."

Deleting `call_inference_stream` would remove the D25 pinning tests' target.
The tests pin finish_reason propagation, which `_with_messages` also has — the
tests could be rewritten to use `_with_messages`, but that's a test rewrite,
not a simple deletion. Given low leverage (the dedup is cosmetic — the two
functions share a body but one is dead) and the `.rules` "keep changes minimal,"
this is deferred pending operator decision on whether to rewrite the D25 tests.

**Files:** `step_actions.rs` → new `inference.rs`.

**Problem:** v0's CAND-3 proposed splitting `step_actions.rs` (3,146 lines)
into 12 per-action modules. **Essentialist G1 FAIL** (pass-through extraction
— inlining the facade loses no behavior; navigability is not behavior).
**Essentialist G2 FAIL** (>7 public items without justification). Grill-me
found v0 missed 5 of 9 free functions.

**Solution (essentialist reduction):** Extract ONLY the `call_inference_stream`
/ `call_inference_stream_with_messages` pair (C5 duplication) into
`inference.rs`. Leave the rest of `step_actions.rs` as-is. The per-action split
is deferred as an optional follow-up, gated on CAND-7 proceeding (if CAND-7
lands, the per-action split localizes the enum-variant readers, making it
essentialist-justified).

**Essentialist verdict:** G1 PASS (dedup of C5 — inlining re-duplicates the
shared streaming+timeout+accumulation body). G2 PASS (1 module, 2 functions).
G3 PASS (consolidates duplicated logic).

**Success criteria:** `call_inference_stream*` pair deduplicated into
`inference.rs` (≤ 200 lines); `step_actions.rs` shrinks by ~200 lines; no
behavior change; `cargo test` clean.

### CAND-4 (Partially complete, audit-grounded) — Decompose `dispatch_compute` along the swarm seam + add `ComputeRef` enum + delete dead forecast arms

**Status: Step 2 (`ComputeRef` enum) is COMPLETE.** Steps 1 (dead arm
 deletion), 3 (swarm extraction), 4 (`ComputeInput` helper), 5
 (`CommandRunner` trait) are deferred.

**What landed (Step 2 — `ComputeRef` enum):**
- Added `ComputeRef` enum with `parse(s: &str) -> Result<Self>` and `as_str()`.
- `dispatch_compute` now dispatches on `ComputeRef` (exhaustive match, no
  `_ =>` catch-all arm).
- The supported-list error message is auto-generated from `SUPPORTED_LIST`
  (single source of truth — no hand-maintained list to drift).
- The old catch-all error message was stale (omitted `combine_tree_probabilities`);
  the new one includes it.
- Re-exported `ComputeRef` via `test_utils`.
- 3 new tests: `compute_ref_parse_round_trips_all_variants`,
  `compute_ref_parse_rejects_unknown_with_supported_list`,
  `dispatch_unknown_ref_errors` (replaces the old assertion-only version).

**What's deferred:**
- **Step 1 (dead arm deletion):** `brier_score`, `brier_score_multi`,
  `brier_interpretation` have 0 manifest callers but are tested and listed in
  3 allow-lists. Deletion is a judgment call — the arms call real
  `hkask_forecast` functions (not dead helpers). Deferred pending operator
  decision.
- **Step 3 (swarm extraction):** 3 arms (368 lines) + 3 helpers into
  `swarm_compute.rs`. Mechanical but non-trivial. The `ComputeRef` enum
  already delivers the P1 gain; the swarm extraction would reduce `compute.rs`
  by ~12%. Deferred.
- **Step 4 (`ComputeInput` helper):** extract the shared `get_f64`/`get_bool`/
  `get_u64` closures + the 6 bypass arms' ad-hoc `input.get("...")` chains.
  Low leverage — the closures are already defined once at the top of
  `dispatch_typed`. Deferred.
- **Step 5 (`CommandRunner` trait):** add an injection seam for `shell.exec`.
  The only untestable arm. Deferred — the trait adds a parameter to
  `dispatch_compute`'s signature, which is a larger change.

**Files:** `compute.rs` (`ComputeRef` enum + `dispatch_typed` + tests),
`hkask_templates.rs` (`ComputeRef` re-exported via `test_utils`).

**Success criteria:** ✅ `ComputeRef` enum added; ✅ `dispatch_compute`
dispatches on the enum (exhaustive, no `_ =>` arm); ✅ supported-list error
auto-generated; ✅ 3 new tests; ✅ `cargo test` clean (205 lib tests); ✅
`./script/clippy` clean.

**Risks:** The `ComputeRef` enum changes the dispatch from string-match to
enum-match. The `compute_ref` field in manifests is still a string (YAML);
`ComputeRef::parse` runs at dispatch time. Typo'd `compute_ref` now fails at
`ComputeRef::parse` with an auto-generated error (was: catch-all with a stale
hardcoded list). Behavior change (better error, different text). Low risk —
all existing tests pass.

### CAND-5 (Worth exploring) — Collapse `ManifestFile` into `BundleManifest`

**Files:** `manifest_loader.rs`, `bundle/manifest.rs`.

**Problem:** `ManifestFile` is a 13-field structural duplicate of
`BundleManifest` (C7). `load_manifest_from_yaml` hand-copies fields. Adding a
field requires editing three sites with no compile-time enforcement.

**Solution:** Use `#[serde(flatten)]` on `BundleManifest` to absorb the
`manifest:` wrapper, eliminating `ManifestFile`. The `ManifestHeader` peer
fields stay as a separate struct.

**Essentialist verdict:** G1 PASS (14-field mirror; inlining re-creates it).
G2 PASS (net removal). G3 PASS (config struct passed through untouched).

**Pragmatic-semantics finding:** `BundleManifest` (L212) does **not** carry
`deny_unknown_fields` (only `#[non_exhaustive]`). The `#[serde(flatten)]` ×
`deny_unknown_fields` collision applies to the wrapper struct
(`ManifestFile`), not `BundleManifest`. The spike must test the right struct.

**Idiomatic-rust note:** `#[serde(flatten)]` + `deny_unknown_fields` is a
known serde footgun (serde-rs/serde#1544). The spike must run before CAND-5
proceeds. **Fallback if spike fails:** make `load_manifest_from_yaml` use
`BundleManifest` directly with a custom `Deserialize` impl, eliminating the
mirror without `flatten`.

**Success criteria:** `ManifestFile` deleted; all manifests round-trip (spike
enumerates them); `deny_unknown_fields` behavior preserved on the wrapper;
`cargo test` clean.

### CAND-6 (Worth exploring) — Extract shared frontmatter/Jinja strippers

**Files:** `output_schema.rs`, `inputs.rs`, `template_renderer.rs` → new
`frontmatter.rs`.

**Problem:** Duplicated Jinja-comment stripper (C1), duplicated frontmatter
parser (C2), duplicated inference-block stripping (renderer parses to discard;
caller parses to read).

**Solution:** Extract a `frontmatter.rs` module owning: `strip_jinja_comments`,
`extract_frontmatter_block` (returns a `Frontmatter` struct, not a tuple —
idiomatic-rust P5), `parse_contract_field`. `output_schema` and `inputs` become
consumers. The renderer stops parsing the inference block twice.

**Essentialist verdict:** G1 PASS (dedup of C1/C2). G2 PASS (3 functions). G3
PASS (consolidates duplicated logic).

**Success criteria:** `strip_jinja_comments` has one definition; frontmatter
parser has one definition; `template_renderer::render` parses the inference
block once; `cargo test` clean.

### CAND-7 (Deferred, spike-gated) — Type-discriminate `BundleManifestStep`

**Files:** `bundle/manifest.rs`, `step_graph.rs`, `step_actions.rs`,
`step_machine.rs`.

**Problem:** 20-field god-struct (W3); every action type uses a different
subset. A `gate` with `template_ref` or `select` with `command` is structurally
representable but semantically invalid. **Verified: `validate()` does NOT
reject per-action field misuse** (essentialist finding — CAND-7 passes G1).

**Solution (idiomatic-rust design):** Enum-of-structs with `StepCommon`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum BundleManifestStep {
    Select(SelectStep),
    Execute(ExecuteStep),
    // ... 12 variants
}

pub struct StepCommon {
    pub ordinal: StepOrdinal,  // newtype (P1)
    pub description: String,
    pub phase: CascadePhase,
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub on_failure: Option<OnFailureConfig>,
}

pub struct SelectStep {
    #[serde(flatten)]
    pub common: StepCommon,
    pub template_ref: String,  // required, not Option
    // ... only Select-valid fields
}
```

`StepNode` becomes `Arc<StepNodeEnum>` (one Arc per node, not 18 — idiomatic-rust
memory win).

**Idiomatic-rust Hoare assessment:** P1 High (invalid states
unrepresentable), P3 High+ (exhaustive match — adding a variant is a compile
error), P5 High+ (valid field sets explicit). Critique score 0.30.

**Pragmatic-semantics finding:** `#[serde(tag = "action")]` (internally
tagged) captures content as `Value` — loses `deny_unknown_fields`. Adjacent
tagging (`#[serde(tag = "action", content = "config")]`) requires YAML shape
change. Untagged requires custom dispatch. **All three have tradeoffs the spike
must enumerate.**

**Essentialist verdict:** G1 PASS (verified `validate()` does NOT reject
per-action field misuse). G2 BORDERLINE (1 enum type, but variant structs have
public fields — each needs justification if >7). G3 PASS (genuine behavior).

**Pre-sequencing spike (required):**
1. Serde tagging strategy: test all 3 options against the manifest corpus.
2. Blast-radius measurement: 6 construction sites (4 in tests, 1 in
   `step_graph` tests, 1 struct def — idiomatic-rust finding) + 18 distinct
   field reads across `step_actions.rs` + `step_machine.rs` (113 total reads).
3. Manifest grep: does `gate` + `template_ref` co-occurrence or `select` +
   `command` co-occurrence actually occur in `registry/manifests/`? If zero
   instances, downgrade CAND-7 from Evidence to Hypothesis and defer
   indefinitely.

**Success criteria:** `BundleManifestStep` is an enum; `StepNode` is
`Arc<StepNodeEnum>`; `dispatch_action` is an exhaustive match (no `_ =>` arm);
all manifests round-trip; `cargo test` clean.

### CAND-8 (Deferred) — Extract Kata math into private functions

**Files:** `convergence.rs`.

**Problem:** v0's CAND-8 proposed splitting `ConvergenceTracker` into Kata +
Legacy variants. **Essentialist G1 BORDERLINE FAIL** (branching is a 1-line
`if kata_enabled()` guard, not real complexity). **Essentialist G3 BORDERLINE**
(tagged struct). Idiomatic-rust found the "trait or enum" hedging is a P6
violation waiting to happen.

**Solution (essentialist reduction):** Extract the Kata-specific math
(`check_kata_met`, `push_cycle_from_context` Kata arm, `check_cauchy_converged`,
`check_calibration_converged`) into private functions. Do NOT split the struct
or introduce a trait/enum. The branching is a 1-line guard, not a maintenance
burden.

**Deferred because:** the split is aesthetic (essentialist). The P1 gain
(inactive model's fields are invalid state) is real but low-severity — a
manifest uses one or the other, never both, so the "invalid state" is never
reached.

**Success criteria:** Kata math extracted into private functions; no struct
split; `cargo test` clean.

### CAND-9 (Strong, Prohibition) — Fix broken feedback loops in `SqliteRegistry`

**Status: B1, B5, B6, B9 are FIXED.** Commits `f1d95635c7` (B1 — error
propagation in write paths), `9e90e0381b` (B5, B6 — warns on silent fallbacks),
and `9a24b4bd4b` (B9 — doc fixed, B4 — warn at BudgetTracker::new) landed.

**Remaining work:**
- **B2** (partial): `row_to_skill` already warns on unknown visibility/zone
  strings but still defaults to `Private`. The cybernetics recommendation to
  add a `Visibility::Unknown` variant (so the operator can distinguish
  "intentionally Private" from "corrupted") is still valid but lower priority
  — the warn surfaces the drift. **Deferred.**
- **B3** (false alarm): `count` has zero production callers. No fix needed.
- **B4** (partial): `BudgetTracker::new` now warns when `rjoule.cap == 0`.
  The `Option<NonZeroU64>` / `enum Cap` change is deferred — the warn +
  `manifest_compliance.rs` validation gate together surface the failure mode.
  **Deferred.**
- **B7** (partial): `infer_domain_from_registry` already warns on
  unreadable/unparsable manifests. The `Result<Domain, _>` change is deferred
  — the warns surface the drift. **Deferred.**
- **B8**: addressed by CAND-1a (deletes `Registry`).
- **B10**: addressed by CAND-2.

**Caller audit (E7) result:** The only external caller of `SqliteRegistry`
methods is `hkask-mcp-kata-kanban/kata/execution.rs:76` calling `get_entry`.
No external caller calls `register_skill`, `register_bundle`, `delete_entry`,
`remove_skill`, `remove_bundle`, or `count`. The B1 fix therefore had zero
external blast radius — only in-crate tests needed updating.

**This candidate is largely complete.** The remaining items (B2, B4, B7) are
deferred to a future iteration pending operator decision on whether the warns
are sufficient or the type-level changes (`Visibility::Unknown`,
`Option<NonZeroU64>`, `Result<Domain, _>`) are warranted.

### CAND-9b (Deferred) — Eliminate config sentinels

**Files:** `bundle/config.rs`, `budget.rs`.

**Problem:** B4 — `RjouleConfig::cap` defaults to 0 as "disabled" sentinel.
Cannot distinguish "intentionally unlimited" from "forgot to configure."

**Interim fix landed (commit `9a24b4bd4b`):** `BudgetTracker::new` now
`tracing::warn!`s when `rjoule.cap == 0`, naming the failure mode and the
remediation. Combined with `manifest_compliance.rs`'s validation gate ("uses
inference but rjoule.cap == 0"), the operator now has two signals.

**Deferred type-level fix:** `RjouleConfig::cap` → `Option<NonZeroU64>` or
`enum Cap { Disabled, Limited(u64), Unlimited }`. This eliminates the sentinel
entirely but changes the manifest YAML schema (30+ manifests would need
`rjoule: { cap: 0 }` → `rjoule: { cap: disabled }` or omission). **Operator
decision needed:** is the warn sufficient, or is the schema change warranted?

**Success criteria (if pursued):** `RjouleConfig::cap` is `Option` or enum; no
sentinel; all manifests updated; `cargo test` clean.

### CAND-9c (Fixed) — Eliminate silent fallbacks

**Files:** `input_mapping.rs`, `output_schema.rs`.

**Problem:** B5 — `resolve_mapping_value` silent fallback to `value.clone()`
on render/parse failure. B6 — `contract_output_to_schema` unknown type →
`{"type":"string"}` silent narrow.

**Fixed (commit `9e90e0381b`):**
- B5: all three fallback paths in `resolve_mapping_value` now
  `tracing::warn!` naming the template expression, the rendered output (where
  available), and the error. The fallback behavior is preserved (returning the
  literal template string keeps the cascade running) but is no longer silent.
- B6: unknown type strings in `contract_output_to_schema` now
  `tracing::warn!` naming the field and the declared type before narrowing to
  `"string"`.

**Deferred:** the `MappingResult` enum (distinguishing
`Resolved`/`Literal`/`Failed`) and the `unverified_unsupported_schema` return
value. The warns surface the drift; the type-level changes would require
caller updates at 4 sites (`resolve_mapping_value`) and 1 site
(`contract_output_to_schema`). **Operator decision needed.**

**Success criteria:** ✅ zero silent fallbacks in `resolve_mapping_value`;
✅ zero silent schema narrows in `contract_output_to_schema`; ✅ `cargo test`
clean; ✅ `./script/clippy` clean.

### CAND-9d (Deferred) — Fix domain inference variety collapse

**Files:** `skill_loader.rs`.

**Problem:** B7 — `infer_domain_from_registry` defaults to `KnowAct` on every
failure mode (malformed manifest, missing manifest, registry error).

**Already partially addressed:** the method warns on unreadable (L303) and
unparsable (L317) manifests, tagged `reg.skill.lifecycle` with
`operation = "manifest_unreadable"` / `"manifest_unparseable"`. The
NotFound case (L296-299) legitimately returns `KnowAct` without a warn — a
Zed-only skill has no registry layer.

**Deferred type-level fix:** return `Result<TemplateType, DomainInferError>`
with distinct variants. The warns already distinguish the three failure modes
in the log; the type-level change would let the caller branch. **Operator
decision needed:** is the warn sufficient?

### CAND-9e (Fixed) — Fix cross-crate doc-drift

**Files:** `template_renderer.rs`.

**Problem:** B9 — `template_renderer::load` doc claimed "disk is the single
runtime source — there is no compiled-in fallback" but 3 production fallback
paths call embedded seeds.

**Fixed (commit `9a24b4bd4b`):** the doc now says disk is the **primary**
runtime source and documents the embedded-seed fallback, naming all three
production call sites (`step_actions.rs::execute_flowdef`,
`step_actions.rs::execute_parallel`,
`hkask-mcp-kata-kanban/kata/execution.rs::render_template`). It also explains
why the fallback exists (bootstrapping a fresh install before the seeding path
runs) and the operational consequence (a disk edit may be shadowed by the
embedded seed if the disk file is missing).

**Deferred:** the DIVERGENCE.md entry + cross-crate test pinning the fallback
behavior. The doc fix removes the contradiction; the test would pin it against
regression. **Recommend adding the test in a follow-up.**

**Success criteria:** ✅ doc matches code; ⏸ DIVERGENCE.md entry (deferred);
⏸ cross-crate test (deferred).

### CAND-10 (Speculative) — Remove dead public surface

**Files:** `hkask_templates.rs` (re-exports), `prompt_strategy.rs`,
`inputs.rs` (dead fns), `budget.rs` (dead methods), `step_machine.rs`
(`classify_failure_mode`), `step_actions.rs` (`Effect::ConsumedGas`).

**Problem:** ~22 of 36 crate-root re-exports have zero external consumers (D3
— **count needs re-verification**). Dead modules (`PromptStrategy`), dead
functions (`render_input_param_spec`, `extract_contract_input_keys`), dead
methods (`from_remaining`, `last_rjoule_cost`, `classify_failure_mode`), dead
effect variant (`Effect::ConsumedGas` — grep-verified 0 producers).

**Pre-sequencing experiment (required):**
1. Re-run the external-consumer grep with per-symbol output.
2. Reconcile against the prior seam-audit's "live re-exports that must stay"
   list (`ManifestLoadError`, `ports::*`, `PromptStrategy` — pragmatic-semantics
   conflict C-A). **Do not delete these until the reconciliation grep
   completes.**
3. For each zero-consumer re-export, run `cargo test --workspace` with the item
   removed in isolation. Green = safe to delete; red = contract test covers
   it (restore or rewrite at the new import path).

**Solution:** Remove from the crate root. Demote to `pub(crate)` or delete. For
items retained for `deny_unknown_fields` backcomat (D8), document as "accepted
but ignored" with a link to the enforcement gate (which is "none" — `.rules`
"advertised invariants must point to the enforcement line").

**Cybernetics caveat (NL-3):** the test-failure → restore loop is a negative
feedback loop that corrects the deletion — but only if the test exists and is
run. If the contract test was itself dead, the loop is open. The per-item
isolation test (step 3 above) closes this loop.

**Success criteria:** `kask_bridge` import list ≤ 8 items; `hkask-mcp-kata-kanban`
≤ 2; zero dead effect variants; zero dead modules; `cargo test --workspace`
clean.

### CAND-11 (Partially complete) — Inline pass-through wrappers

**Status: F1, F2, F4 are FIXED.** F1 (`concurrency.rs`) deleted, 3 import
sites updated to use `hkask_types::concurrency` directly. F2
(`bundle/cascade.rs`) deleted, `CascadePhase` inlined into `bundle/manifest.rs`,
re-exported via `bundle/mod.rs`. F4 (`render_minijinja`) deleted, 3 tests
rewritten to use `TemplateRenderer::new(...).render(...)` directly.

**Remaining (lower priority):**
- F5 (`render_step_template`) — 7-line pass-through discarding 2 of 3 return
  values. Inline at 2 call sites.
- F6 (`apply_input_mapping`) — 12-line pass-through over `Value::Object`.
  Inline at 4 call sites.
- F7 (`phase_str`) — one-line delegate to `self.phase.as_str()`. Now inlined
  into `manifest.rs` (since `CascadePhase` is there). **Done as part of F2.**
- F8 (three `*_str()` methods on `BundleConflict`/`BundleComplementarity`) —
  one-line delegates. Inline at call sites.
- F9 (`execute_manifest` borrowed) — pass-through to `execute_manifest_into`
  via `self.clone()`. Test ergonomics only; bridge uses `_into` exclusively.
  **Keep** — the test ergonomics value is real.

**Files:** `concurrency.rs` (deleted), `bundle/cascade.rs` (deleted),
`template_renderer.rs` (F4 deleted, tests rewritten), `bundle/manifest.rs`
(CascadePhase inlined), `bundle/mod.rs` (re-export updated),
`executor.rs` + `step_machine.rs` (imports updated),
`tests/concurrency_properties.rs` (import updated),
`output_schema.rs` + `step_graph.rs` (test imports updated).

**Success criteria:** ✅ F1 deleted; ✅ F2 deleted + CascadePhase inlined;
✅ F4 deleted + tests rewritten; ✅ `cargo test` clean; ✅ `./script/clippy` clean.

### CAND-12 (Worth exploring, pairs with CAND-3-minimal) — Typed `StepResultKey`

**Files:** `step_machine.rs`, `step_context.rs`, `step_actions.rs`.

**Problem:** P5 — `merge_control_flow` constructs `step_{ordinal}_result`
string key; couples machine to context's naming convention. The string-key
contract is the S2 coordination channel (cybernetics finding). CAND-3-minimal
splits readers without lifting the contract, which **amplifies the coupling**
(cybernetics NL-2).

**Solution (cybernetics recommendation):** lift `step_{ordinal}_result` into a
`StepResultKey` type (or `StepContext::result_for(ordinal)` accessor). The
string-key convention becomes a typed contract.

**Pairs with CAND-3-minimal:** if CAND-3-minimal extracts `call_inference_stream*`,
the typed key should land in the same PR (or immediately after) to avoid the
S2 degradation window.

**Success criteria:** `step_{ordinal}_result` string construction replaced by
`StepResultKey` type; `merge_control_flow` uses the typed accessor; `cargo test`
clean.

### CAND-13 (Worth exploring) — Dependency-direction audit

**Files:** `ports.rs`, `step_machine.rs`.

**Problem:** P3 — `ports::ManifestResolveError` reaches upward into
`manifest_loader::ManifestLoadError` (backwards dependency from foundation to
consumer). P4 — `step_machine::dispatch_with_retry` inlines curator MCP call
(skill-execution domain leaks into interpreter).

**Solution (cybernetics recommendation):** move the error type down (into
`manifest_loader`) or the MCP call up (into a dedicated `curator` module).

**Success criteria:** `ports.rs` has no upward dependencies;
`step_machine.rs` has no curator MCP calls; `cargo test` clean.

### CAND-14 (Worth exploring) — Reconcile crate-root surface with actual API

**Files:** `hkask_templates.rs`, `hkask-mcp-media/src/budget.rs`.

**Problem:** P6 — `hkask-mcp-media` bypasses the crate root, uses
`pub mod budget`/`bundle::config` directly. The crate-root surface is not the
actual public API.

**Solution (cybernetics recommendation):** either (a) promote the submodule
path to the public surface (re-export `BudgetTracker`, `BundleGasConfig`,
`RjouleConfig` at the crate root), or (b) move the consumer onto the crate
root.

**Success criteria:** `hkask-mcp-media` uses only crate-root re-exports; or
the submodule path is explicitly re-exported; `cargo test` clean.

### CAND-15 (Worth exploring) — DIVERGENCE seam test for seed fns

**Files:** `kask_extensions_ui/src/publish.rs`, `DIVERGENCE.md`.

**Problem:** P7 — `kask_extensions_ui` (upstream Zed tree) consumes 4 seed
fns. Changes to seed fns ripple across the Kask↔Zed seam. No test pins this.

**Solution (cybernetics recommendation):** add a DIVERGENCE.md entry + seam
test pinning the 4 seed fns, per `.rules` "every `// zed-kask:` comment needs
a test."

**Success criteria:** DIVERGENCE.md entry added; seam test pins the 4 seed fns;
`cargo test` clean.

## Pre-sequencing experiments (required before any candidate executes)

| ID | Experiment | Owner | Falsifiable outcome | Blocks |
|----|------------|-------|---------------------|--------|
| E1 | CAND-5 `serde(flatten)` × `deny_unknown_fields` spike (test `ManifestFile`, not `BundleManifest`) | — | All manifests round-trip → CAND-5 proceeds; failure → fallback to custom `Deserialize` | CAND-5 |
| E2 | CAND-7 serde tagging strategy spike (3 options × manifest corpus) | — | One option preserves `deny_unknown_fields` → CAND-7 proceeds; all fail → CAND-7 deferred indefinitely | CAND-7 |
| E3 | CAND-7 blast-radius measurement (construction sites + reader sites) | — | ≤ 10 construction sites → CAND-7 tractable; > 10 → defer | CAND-7 |
| E4 | CAND-7 manifest grep (`gate` + `template_ref` co-occurrence) | — | ≥ 1 instance → CAND-7 is Evidence; 0 instances → Hypothesis, defer | CAND-7 |
| E5 | CAND-10 external-consumer reconciliation grep (per-symbol) | — | Reconciled list → CAND-10 proceeds; unresolved conflict with prior audit → scope CAND-10 to uncontested items | CAND-10 |
| E6 | CAND-10 per-item isolation test (`cargo test --workspace` with each item removed) | — | Green = safe; red = contract test covers it | CAND-10 |
| E7 | CAND-9 caller audit (grep all call sites of 4 write methods + `row_to_skill` + `count`) | — | Per-caller handling decision table | CAND-9 |

## Recommended sequencing (re-ordered by leverage)

Re-sequenced from v0 per grill-me (CAND-10 first delivers least leverage),
pragmatic-cybernetics (CAND-10 removes spec-drift signal surface before wiring
enforcement), and pragmatic-semantics (CAND-9 is Prohibition-grade).

Critical path: **CAND-9 → CAND-9b–9e → CAND-2 → CAND-12 → CAND-3-minimal →
CAND-1a → CAND-5 (E1-gated) → CAND-6 → CAND-10 (E5/E6-gated) → CAND-11 →
CAND-4 (audit-grounded) → CAND-7 (E2/E3/E4-gated, last).**

Off-critical-path (parallelizable): CAND-13, CAND-14, CAND-15, CAND-1b
(deferred), CAND-8 (deferred). CAND-4 can run in parallel with
CAND-3-minimal (disjoint write sets: `compute.rs` vs `step_actions.rs`).

1. **CAND-9** (Prohibition fixes in `SqliteRegistry`) — `.rules`-mandated;
   highest leverage (fixes B1/B2/B3). Separate PR with release note. Caller
   audit (E7) required.
2. **CAND-9b–9e** (remaining broken loops) — Prohibition fixes for B4-B7, B9.
   Parallel with CAND-9 (disjoint write sets: `bundle/config.rs`,
   `input_mapping.rs`, `output_schema.rs`, `skill_loader.rs`,
   `template_renderer.rs`).
3. **CAND-2** (`SubCascade` builder) — correctness fix (MAX_STEPS gate in all
   3 paths); unblocks CAND-3-minimal.
4. **CAND-12** (typed `StepResultKey`) — pairs with CAND-3-minimal; lifts the
   S2 string-key contract before the split.
5. **CAND-3-minimal** (extract `call_inference_stream*`) — dedup C5; small,
   low-risk.
6. **CAND-1a** (collapse local registry trait layer) — deletes `Registry` +
   `BundleRegistryIndex` + `*_owned`; prior deferral resolved by operator
   approval.
7. **CAND-5** (collapse `ManifestFile`) — gated on E1 spike.
8. **CAND-6** (frontmatter/Jinja dedup) — low-risk, unblocks nothing but
   reduces C1/C2 duplication.
9. **CAND-10** (dead surface removal) — gated on E5/E6; **after** CAND-9b–9e
   (don't delete "not yet enforced" doc comments before wiring enforcement —
   cybernetics S4 finding).
10. **CAND-11** (pass-through sweep) — mechanical, low-risk.
11. **CAND-4** (audit-grounded `dispatch_compute` decomposition) — see
    `dispatch-compute-audit.md`. Deletes 3 dead forecast arms, adds `ComputeRef`
    enum, extracts swarm sub-domain (368 lines + 3 helpers) into `swarm_compute.rs`,
    adds `ComputeInput` helper + `CommandRunner` trait. Independent of CAND-2/3/12;
    can run in parallel with CAND-3-minimal.
12. **CAND-7** (type-discriminate `BundleManifestStep`) — gated on E2/E3/E4;
    highest risk; last.

**Parallelizable (off critical path):**
- CAND-13 (dependency-direction audit) — independent.
- CAND-14 (crate-root reconciliation) — independent.
- CAND-15 (DIVERGENCE seam test) — independent.
- CAND-1b (cross-crate trait deletion) — deferred; after CAND-1a.
- CAND-8 (extract Kata math) — deferred; low-leverage.

## Verification plan (per-candidate, with measurable success criteria)

Closes the metacognition Outcome gap (v0 had no measurable success criteria).

| Candidate | Success criteria (binary, measurable) | Verification command |
|-----------|---------------------------------------|---------------------|
| CAND-1a | `Registry` type deleted; `BundleRegistryIndex` trait deleted; `*_owned` forwarders deleted; `bootstrap_test.rs` passes against `SqliteRegistry` | `grep -rEn "struct Registry\b\|trait BundleRegistryIndex" kask/crates/hkask-templates/src` → 0 hits; `cargo test -p hkask-templates` |
| CAND-2 | `MAX_STEPS` hard gate fires in all 3 paths (test asserts this); `SubCascade` struct ≤ 200 lines; `Box::pin` preserved | New test: `sub_cascade_max_steps_gate_fires_in_all_paths`; `wc -l` on `SubCascade` |
| CAND-3-minimal | `call_inference_stream*` pair deduplicated into `inference.rs` (≤ 200 lines); `step_actions.rs` shrinks by ~200 lines | `wc -l inference.rs step_actions.rs` |
| CAND-4 | 3 dead forecast arms deleted; `ComputeRef` enum added; `swarm_compute.rs` created (3 arms + 3 helpers); `ComputeInput` helper extracted; `CommandRunner` trait added; `compute.rs` shrinks from 3,379 to ~2,200 | `grep -cE "compute_ref: brier_score" kask/registry/manifests` → 0; `grep -c "_ =>" compute.rs::dispatch_compute` → 0; `wc -l compute.rs` ≤ 2,200; `cargo test` |
| CAND-5 | `ManifestFile` deleted; all manifests round-trip; `deny_unknown_fields` preserved on wrapper | `grep -c "struct ManifestFile" manifest_loader.rs` → 0; spike report |
| CAND-6 | `strip_jinja_comments` has 1 definition; frontmatter parser has 1 definition; renderer parses inference block once | `grep -rc "fn strip_jinja_comments" .` → 1 |
| CAND-7 | `BundleManifestStep` is an enum; `StepNode` is `Arc<StepNodeEnum>`; `dispatch_action` is exhaustive match (no `_ =>`); all manifests round-trip | `grep -c "_ =>" step_machine.rs::dispatch_action` → 0; spike report |
| CAND-9 | Zero `Ok(())`-on-write-failure methods; `count` returns `Result`; `row_to_skill` warns on unknown enum strings; all callers audited | `grep -cE "Ok\(\(\)\)" registry_sqlite.rs` → 0 (on write paths); caller audit table |
| CAND-9b | `RjouleConfig::cap` is `Option` or enum; no sentinel | `grep -c "cap: u64" bundle/config.rs` → 0 |
| CAND-9c | Zero silent fallbacks in `resolve_mapping_value`; zero silent schema narrows | `grep -c "unwrap_or\|\.clone()\(\)$" input_mapping.rs` → 0 on fallback paths |
| CAND-9d | `infer_domain_from_registry` returns `Result`; distinct error variants | `grep -c "fn infer_domain_from_registry" skill_loader.rs` → 1; signature check |
| CAND-9e | Doc matches code; DIVERGENCE.md entry; cross-crate test | `grep -c "disk is the single runtime source" template_renderer.rs` → 0 or updated |
| CAND-10 | `kask_bridge` import list ≤ 8 items; `hkask-mcp-kata-kanban` ≤ 2; zero dead effect variants; zero dead modules | `grep -c "use hkask_templates::" kask_bridge/src/skill_executor.rs` → ≤ 8 |
| CAND-11 | 7 wrappers inlined/deleted | `grep -c "fn render_minijinja\|fn phase_str\|fn render_step_template\b" ` → 0 |
| CAND-12 | `step_{ordinal}_result` string construction replaced by `StepResultKey` | `grep -c "step_.*_result" step_machine.rs` → 0 (or all via `StepResultKey`) |
| CAND-13 | `ports.rs` has no upward dependencies; `step_machine.rs` has no curator MCP calls | `grep -c "manifest_loader::" ports.rs` → 0 |
| CAND-14 | `hkask-mcp-media` uses only crate-root re-exports | `grep -c "hkask_templates::budget\|hkask_templates::bundle::config" hkask-mcp-media/src/` → 0 |
| CAND-15 | DIVERGENCE.md entry; seam test pins 4 seed fns | `grep -c "seed" DIVERGENCE.md` ≥ 1 |

**Cross-cutting:**
- **Dependency direction:** `cargo tree` + grep for back-edges.
- **Depth test:** re-apply deletion test to each new module post-extraction.
- **P6/P7/P8:** no stubs, no deprecation attributes, all tests verify stated
  behavioral properties.
- **Clippy:** `./script/clippy` (per `.rules`, not `cargo clippy`).
- **Test suite:** `cargo test -p hkask-templates` + the four consumer crates.
- **Surface adapter thinness:** `kask_bridge` import list before/after.

## Open questions resolved by the review

1. **CAND-1 deferral (v0 Open Q1):** Resolved. RA-01 was NOT deferred (prior
   audit marked `deferred: no`). RA-09/RA-10 were deferred with reason "user
   decides" — operator approval of this plan resolves the deferral. CAND-1a
   proceeds; CAND-1b (cross-crate) deferred.
2. **CAND-7 risk (v0 Open Q2):** Quantified by idiomatic-rust. 6 construction
   sites, 18 distinct field reads (113 total). Gated on E2/E3/E4 spikes.
3. **CAND-9 scope (v0 Open Q3):** Resolved by pragmatic-semantics OT ranking.
   `.rules` Prohibition overrides "callers may depend on silence" Hypothesis.
   CAND-9 is a fix, not a gateable behavior change. Ship behind release note.
4. **CAND-10 vs contract tests (v0 Open Q4):** Resolved by E5/E6 per-item
   isolation tests. Green = safe; red = contract test covers it.
5. **CAND-3 vs CAND-4 ordering (v0 Open Q5):** Resolved by essentialist. Both
   cut to minimal versions; CAND-3-minimal and CAND-4-minimal are independent
   (disjoint write sets: `step_actions.rs` vs `compute.rs`); can run in
   parallel after CAND-2 and CAND-12.
