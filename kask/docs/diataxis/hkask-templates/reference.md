---
title: "hkask-templates — Reference"
audience: [developers, architects, agents]
last_updated: 2026-08-05
version: "0.2.2"
status: "Active"
domain: "Skills"
mds_categories: [domain, composition]
---

# hkask-templates — Reference

`hkask-templates` implements the skill manifest registry and the
`ManifestExecutor` that runs skill PDCA cascades. It loads `manifest.yaml`
files from the registry, resolves template dependencies, and executes Jinja2
templates against the inference port. Template types are `Prompt` (WordAct),
`Process` (FlowDef), and `Cognition` (KnowAct) — see `hkask_templates.rs:1`.

## Source citations

| Symbol                        | Location                                                   |
| ----------------------------- | ---------------------------------------------------------- |
| `ManifestExecutor` struct     | `kask/crates/hkask-templates/src/executor.rs:128`          |
| `ManifestExecutor::new`       | `kask/crates/hkask-templates/src/executor.rs:170`          |
| `execute_manifest`            | `kask/crates/hkask-templates/src/executor.rs:467`          |
| `extract_final_step_result`   | `kask/crates/hkask-templates/src/executor.rs:1853`         |
| `propagate_taint_for_binding` | `kask/crates/hkask-templates/src/executor.rs:282`          |
| `check_untrusted_input`       | `kask/crates/hkask-templates/src/executor.rs:233`          |
| `spotlight_tool_output`       | `kask/crates/hkask-templates/src/executor.rs:1904`         |
| `BundleManifest` struct       | `kask/crates/hkask-templates/src/bundle/manifest.rs:91`    |
| `BundleManifestStep`          | `kask/crates/hkask-templates/src/bundle/manifest.rs:35`    |
| `BundleSkill`                 | `kask/crates/hkask-templates/src/bundle/manifest.rs:24`    |
| `BundleConflict`              | `kask/crates/hkask-templates/src/bundle/composition.rs:72` |
| `BundleComplementarity`       | `kask/crates/hkask-templates/src/bundle/composition.rs:97` |
| `CascadePhase` enum           | `kask/crates/hkask-templates/src/bundle/cascade.rs:8`      |
| `ConvergenceConfig`           | `kask/crates/hkask-templates/src/bundle/config.rs:52`      |
| `ConvergenceTracker`          | `kask/crates/hkask-templates/src/convergence.rs:73`        |
| `ValidationResult`            | `kask/crates/hkask-templates/src/bundle/manifest.rs:314`   |
| `BundleRegistryIndex` trait   | `kask/crates/hkask-templates/src/bundle/mod.rs:22`         |
| `SkillLoader`                 | `kask/crates/hkask-templates/src/skill_loader.rs:64`       |
| `SkillFrontMatter`            | `kask/crates/hkask-templates/src/skill_loader.rs:28`       |
| `SkillLoadResult`             | `kask/crates/hkask-templates/src/skill_loader.rs:57`       |
| `SqliteRegistry`              | `kask/crates/hkask-templates/src/registry_sqlite.rs:65`    |
| `load_manifest_from_file`     | `kask/crates/hkask-templates/src/manifest_loader.rs:123`   |
| `load_manifest_from_yaml`     | `kask/crates/hkask-templates/src/manifest_loader.rs:138`   |
| `resolve_manifest`            | `kask/crates/hkask-templates/src/manifest_loader.rs:197`   |
| `ManifestLoadError` enum      | `kask/crates/hkask-templates/src/manifest_loader.rs:319`   |
| `PromptStrategy` enum         | `kask/crates/hkask-templates/src/prompt_strategy.rs:14`    |
| `BudgetTracker`               | `kask/crates/hkask-templates/src/budget.rs:71`             |

## Manifest schema

The `BundleManifest` (`bundle/manifest.rs:91`) is the parsed representation of
a `manifest.yaml` file. It contains a list of `BundleSkill` entries, a list of
`BundleManifestStep` entries, and declared `BundleConflict` /
`BundleComplementarity` relations. Each step references a Jinja2 template via
`template_ref`, declares its `action`, `phase`, `input_mapping`,
`output_schema`, optional `compute_ref` (deterministic math primitive),
and `condition`.

```mermaid
classDiagram
    class BundleManifest {
        +id: String
        +name: String
        +skills: Vec~BundleSkill~
        +steps: Vec~BundleManifestStep~
        +conflicts: Vec~BundleConflict~
    }
    class BundleManifestStep {
        +ordinal: u32
        +action: String
        +template_ref: Option~String~
        +phase: CascadePhase
        +compute_ref: Option~String~
        +condition: Option~String~
    }
    class CascadePhase {
        <<enumeration>>
        Pre
        Core
        Post
    }
    class ManifestExecutor {
        +new(inference, tools, default_params)
        +with_runtime_policy(policy)
        +with_terminal_check(check)
        +execute_manifest(manifest, ctx)
    }
    class ConvergenceTracker {
        +new(config)
        +max_iterations() u32
        +threshold() f64
    }
    class SkillLoader {
        +new(project_root)
    }
    class SqliteRegistry {
        +new(path)
    }
    class BundleRegistryIndex {
        <<interface>>
        +register_bundle(b)
        +get_bundle(id)
        +list_bundles()
    }

    BundleManifest --> BundleManifestStep : contains
    BundleManifestStep --> CascadePhase : phase
    ManifestExecutor --> BundleManifest : executes
    ManifestExecutor --> ConvergenceTracker : tracks
    SkillLoader ..> BundleManifest : loads
    SqliteRegistry ..> BundleRegistryIndex : implements
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-TPL-001
verified_date: 2026-08-05
verified_against: kask/crates/hkask-templates/src/executor.rs:128,170,467; kask/crates/hkask-templates/src/bundle/manifest.rs:91,35,24,314; kask/crates/hkask-templates/src/bundle/cascade.rs:8; kask/crates/hkask-templates/src/bundle/config.rs:52; kask/crates/hkask-templates/src/convergence.rs:73; kask/crates/hkask-templates/src/bundle/composition.rs:72,97; kask/crates/hkask-templates/src/skill_loader.rs:64; kask/crates/hkask-templates/src/registry_sqlite.rs:65; kask/crates/hkask-templates/src/bundle/mod.rs:22
status: VERIFIED
-->

## Manifest loading

Three functions in `manifest_loader.rs` handle manifest loading:
`load_manifest_from_file` (`manifest_loader.rs:123`) reads from a file path,
`load_manifest_from_yaml` (`manifest_loader.rs:138`) parses a YAML string, and
`resolve_manifest` (`manifest_loader.rs:197`) resolves a manifest reference
against a `BundleRegistryIndex`. The `ManifestLoadError` enum
(`manifest_loader.rs:319`) covers IO and YAML parse failures.

## Skill loading

The `SkillLoader` (`skill_loader.rs:64`) loads skill definitions from the
registry. It parses `SkillFrontMatter` (`skill_loader.rs:28`) and returns a
`SkillLoadResult` (`skill_loader.rs:57`).

## Registry

The `SqliteRegistry` (`registry_sqlite.rs:65`) implements the
`BundleRegistryIndex` trait (`bundle/mod.rs:22`) and persists skill and
template metadata in SQLite. An in-memory `Registry` adapter also exists (see
`hkask_templates.rs:39`).

## ManifestExecutor — constructor and defense wiring

`ManifestExecutor::new(inference, tools, default_params)`
(`executor.rs:170`) takes exactly three arguments: the `InferencePort`, the
`ToolPort`, and default `LLMParameters`. (The former `secret` parameter was
removed.) The struct (`executor.rs:128`) additionally carries three
defense-layer fields, all defaulted in `new` and settable via builders:

| Field                                                                      | Layer                                                                                   | Set via                                                      | Default |
| -------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- | ------------------------------------------------------------ | ------- |
| `spotlighter: Spotlighter` (`executor.rs:143`)                             | Spotlighting of untrusted tool outputs (arXiv:2403.14720)                               | always `SpotlightMode::Delimit` in `new` (`executor.rs:183`) | Delimit |
| `runtime_policy: Option<Arc<DefaultPolicy>>` (`executor.rs:147`)           | Pre-execution policy check (Layer 6)                                                    | `with_runtime_policy` (`executor.rs:217`)                    | None    |
| `taint_labels: Arc<Mutex<HashMap<String, ToolTaint>>>` (`executor.rs:151`) | FIDES taint tracking for context entries (Layer 5, arXiv:2505.23643)                    | internal                                                     | empty   |
| `terminal_check: Option<Arc<dyn Fn() -> bool>>` (`executor.rs:160`)        | Profile enforcement for the built-in `terminal` tool (F6 proposer/evaluator separation) | `with_terminal_check` (`executor.rs:198`)                    | None    |

At every tool invocation, `invoke_tool` runs the runtime policy first
(`executor.rs:402`): `PolicyVerdict::Block` and `RequireHuman` abort the step,
`Log` emits a `reg.guard.runtime_policy` span, `Allow` proceeds. The result
is spotlight-transformed (`spotlight_tool_output`, `executor.rs:1904`) and
returned together with the tool's `ToolTaint` label, which is stored in
`taint_labels` under `step_{ordinal}_result`.

### FIDES taint propagation

Two complementary functions close the Source→Sink information-flow gate (see
[`guard-taint-pipeline.md`](../../architecture/guard-taint-pipeline.md)):

- `propagate_taint_for_binding(original_value, new_key)` (`executor.rs:282`)
  — called at every `input_mapping` binding **before** `context.insert`, so
  the new key inherits the strongest taint of every context key it references
  (Source > Endorser > Pure). Enforced by regression RR-0026/RR-0027 (cargo
  tests in `executor.rs`); the call sites are `run_cascade`
  (`executor.rs:789`), `execute_select` (`executor.rs:1245`),
  `execute_populate` (`executor.rs:1376`), and `execute_render`
  (`executor.rs:1423`). Passing the _original_ mapping value (with `$ref` /
  `{{ }}` markers), not the resolved value — the function inspects
  pre-resolution markers to find referenced keys.
- `check_untrusted_input(value)` (`executor.rs:233`) — the gate side:
  recursively scans a bound tool-input JSON for `{"$ref": "step_N_result..."}`
  patterns **and** inline-Jinja `{{ step_N_result }}` strings (the same
  reference grammar propagation recognizes — the gate must scan the same
  grammar or inline Jinja would bypass the Source→Sink block), returning true
  when any referenced entry is labeled `Source`.

## Cascade actions

`ManifestExecutor::execute_manifest` (`executor.rs:467`) dispatches on
`step.action`:

| Action               | Handler                                    | Purpose                                            |
| -------------------- | ------------------------------------------ | -------------------------------------------------- |
| `select`             | `execute_select` (`executor.rs:1231`)      | LLM inference, parse JSON, merge into context      |
| `populate`           | `execute_populate` (`executor.rs:1358`)    | Render-only, store under `step_N_populated`        |
| `render`             | `execute_render` (`executor.rs:1412`)      | RenderAct — no inference, for reference docs       |
| `flowdef`            | `execute_flowdef` (`executor.rs:1472`)     | Nested sub-manifest cascade (composability)        |
| `tool_invoke`        | `execute_tool_invoke` (`executor.rs:1629`) | MCP tool call via `step.mcp`                       |
| `compute`            | `execute_compute` (`executor.rs:1704`)     | Deterministic math primitive (`hkask_forecast::*`) |
| `choice`             | inline (`executor.rs:671`)                 | Conditional branch via `_next_ordinal`             |
| `loop`               | inline (`executor.rs:695`)                 | PDCA re-entry from target ordinal                  |
| `abort` / `escalate` | inline                                     | Terminate cascade                                  |

Step results are stored under `step_{ordinal}_result` keys. Final-result
extraction must be ordinal-keyed — the canonical extractors are
`extract_final_step_result` (`executor.rs:1937`) and the same-named function
in `kask_bridge/src/skill_executor.rs`; `HashMap::values().last()` is
non-deterministic (`RandomState`).

### Wall-clock defense

The cascade has three layers of wall-clock defense against the "performative
reasoning" failure mode documented in Wang (2026, arXiv:2603.02615v1) — a
model stuck in a serial sub-call verification loop that burns wall-clock
while consuming few tokens (observed: 741.5s for 11,715 tokens):

1. **Per-step timeout** (`step.timeout_seconds`, `manifest.rs:55`) — hard,
   enforced via `tokio::time::timeout` in `execute_select` (`executor.rs:1353`).
   Catches individual stuck inference calls. Default is 0 (disabled); skill
   authors set it per step.
2. **Iteration cap** (`convergence.max_iterations`, `config.rs:194`, default 10)
   — bounds the number of PDCA loop passes. Each pass can spawn sub-calls,
   so the cascade is bounded by `max_iterations × steps_per_pass ×
per_step_timeout`.
3. **Matryoshka depth guard** (`SYSTEM_MAX_RECURSION = 7`, `token_types.rs:18`)
   — bounds flowdef sub-cascade nesting depth, enforced at `run_cascade`
   entry (`executor.rs:496`).

**Known gap:** there is no _cascade-level aggregate_ wall-clock cap that
bounds total elapsed time across all nesting levels. The per-step timeout
bounds individual calls; the iteration cap bounds loop passes; the matryoshka
guard bounds depth. A deeply-nested flowdef cascade with `max_iterations: 10`
and 30s per-step timeouts can run up to 7 × 10 × 30 = 2100s in the worst case.
The `reg.skill.budget.*` spans and `ConvergenceStatus` provide operator
visibility, but the default ceiling is high. A cascade-level
`wall_clock_cap_seconds` field is a known defense-in-depth candidate, not
yet implemented.

### Output normalization

`normalize_model_output` (`executor.rs:1976`) strips `<thinking>...</thinking>`
reasoning wrappers that reasoning models (Kimi K2, DeepSeek-R1) emit before
the final answer. Without stripping, these tags pollute downstream step
inputs and break JSON parsing — the failure mode documented in Wang (2026,
Appendix A.4), where the RLM framework's parsers missed answers entirely
until a `strip_think_tags` helper was added. Applied at the
`extract_final_step_result` entry point. Non-string values pass through
unchanged; clean strings borrow (`Cow::Borrowed`), dirty strings own
(`Cow::Owned`).

## See also

- [hkask-templates Explanation](./explanation.md): sequence diagram of the
  ManifestExecutor invocation path.
- [hkask-types Reference](../hkask-types/reference.md): the
  `SkillRegistryIndex` and `RegistryIndex` traits this crate implements.
- [`guard-taint-pipeline.md`](../../architecture/guard-taint-pipeline.md): the
  FIDES taint pipeline this executor participates in.
- [`kask/docs/explanation/skills-and-composition.md`](../../explanation/skills-and-composition.md):
  cross-cutting skill anatomy and composition.

---

[^beck-tdd]: Beck, K. (2003). _Test-Driven Development: By Example._ Addison-Wesley. <https://www.oreilly.com/library/view/test-driven-development/0321146530/>. The red-green-refactor cycle that the manifest PDCA steps parallel.

[^minijinja]: mitsuhiko. (2024). _minijinja — a Jinja2 template engine for Rust._ <https://docs.rs/minijinja/>. The Rust Jinja2 implementation used for template rendering.
