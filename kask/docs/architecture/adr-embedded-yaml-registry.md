---
title: "ADR: Build-time embedded YAML/Jinja2 registry — amended to disk-first seed model"
audience: [architects, developers, agents]
last_updated: 2026-08-20
version: "0.37.0"
status: "Superseded"
domain: "architecture"
mds_categories: [composition, lifecycle, trust]
---

# ADR: Build-time embedded YAML/Jinja2 registry — amended to disk-first seed model

**Status:** Superseded — **AMENDED 2026-08-05** (see [§ Amended decision](#amended-decision-2026-08-05) below), then **SUPERSEDED 2026-08-20** by the upstream-Zed body-injection model (see [§ Supersession (2026-08-20)](#supersession-2026-08-20) below). The original Decision section is preserved verbatim for context; its central mechanism (embedded-preferred, filesystem-as-dev-fallback) was inverted by commit `134d19659c` ("Seed shipped skills to disk at startup", 2026-08-04, part of the 2026-08-02-era registry seeding work), then the entire `hkask-templates` crate and `registry/manifests/` FlowDef layer were deleted by commit `5f4cf5f10d`. **The current reality is: skill execution is upstream-Zed body injection via `SkillTool::run` → `render_skill_envelope` (`crates/agent/src/tools/skill_tool.rs:266`); there is no `ManifestExecutor`, no StepMachine, no PDCA cascade machinery. The `render_template` tool still reads Jinja2 templates from `kask/registry/templates/` (62 template crates remain), but there are no FlowDef manifests.** Reading the original sections without the amendment and supersession inverts the architecture.

## Supersession (2026-08-20)

The build-time embedding model described in this ADR — and the disk-first seed amendment — are **superseded** by the upstream-Zed body-injection model. Commit `5f4cf5f10d` deleted the `hkask-templates` crate (which contained `build.rs`, `BridgeManifestExecutor`, `ManifestExecutor`, `TemplateRenderer`, the `Registry` template index, and the FlowDef cascade machinery) and the entire `kask/registry/manifests/` directory (all FlowDef process manifests).

**What replaced it:**

- **Skill execution** is now upstream-Zed body injection. `SkillTool::run` (`crates/agent/src/tools/skill_tool.rs:266`) reads the `SKILL.md` body from disk via `agent_skills::read_skill_body` and injects it into the model context via `render_skill_envelope`. The model reads the body and follows the instructions. There is no `ManifestExecutor`, no `StepMachine`, no PDCA cascade machinery, no `ConvergenceTracker`, no `BundleManifest`, no `ExitKind`. PDCA loops are **model-coordinated**, not machine-enforced: the SKILL.md body describes convergence criteria and the model self-iterates using the `lisp_eval` tool (`hkask_lisp::eval_sandboxed_with_budget`) for deterministic checks and the `render_template` tool for structured prompt scaffolding.
- **Jinja2 templates** survive. The `render_template` tool (registered in `register_session`, not `add_default_tools`) renders Jinja2 templates from `kask/registry/templates/` using `minijinja`. **62 template crates remain** under that path. The tool strips YAML frontmatter (`---`-delimited headers) — the frontmatter's `contract:` and `[inference]` blocks are NOT processed; LLM parameters (temperature, thinking_budget) in the frontmatter have no effect. Path traversal protection via `canonicalize` + `starts_with` rejects a `template_ref` containing `..` that resolves outside the base path. The template base path is wired via `agent::set_template_base_path()` (OnceLock) in `main.rs` at startup (dev: `kask/registry/templates/`, prod: `{kask_data_dir}/skills/registry/templates/`); if unset, the tool returns an error rather than rendering from the wrong path.
- **`lisp_eval`** survives as a registered built-in tool (`add_default_tools`) — a sandboxed Lisp interpreter with no I/O, no `eval`, no network, bounded by `max_steps` (default 100000) and `max_depth` (default 64).

**What this means for the four-layer table below:** the SKILL.md companions layer and the Jinja2 templates layer survive (the former via `agent_skills` discovery + `SkillTool::run` body injection; the latter via the `render_template` tool). The per-skill template manifests layer and the process manifests (FlowDef PDCA) layer are **deleted** — their rows below describe a model that no longer exists and are preserved only for historical context.

## Context

The kask-skills system is a four-layer architecture with a single Rust bridge:

| Layer | Source of truth | Consumer |
|-------|----------------|----------|
| SKILL.md companions | `.agents/skills/<name>/SKILL.md` | `agent_skills` (discovery catalog) + `SkillTool::run` (body injection) |
| Per-skill template manifests | `registry/templates/<name>/manifest.yaml` | ~~`Registry` (template index)~~ — **deleted with `hkask-templates` (commit `5f4cf5f10d`)** |
| Process manifests (FlowDef PDCA) | `registry/manifests/<name>.yaml` | ~~`ManifestExecutor` (cascade driver)~~ — **deleted with `hkask-templates` (commit `5f4cf5f10d`); `registry/manifests/` no longer exists** |
| Jinja2 templates | `registry/templates/<name>/*.j2` | `render_template` tool (prompt rendering, 62 template crates remain) |

The architectural rationale — often stated as *"the flexible non-compiled YAML and Jinja2 layers can rapidly evolve as a natural sandboxing and learning surface around the core Rust code"* — is only half the story. The other half was in `hkask-templates/build.rs`.[^fowler-poeaa] **(The `hkask-templates` crate and its `build.rs` were deleted by commit `5f4cf5f10d`; the embedding mechanism they describe is preserved below for historical context only.)**

## Decision (original, 2026-08-04 — SUPERSEDED)

> `build.rs` embeds **all four artifact classes** into the binary at compile time via `include_str!`:
>
> 1. `registry/templates/*/manifest.yaml` → `MANIFEST_YAMLS` (per-skill template manifests)
> 2. `registry/manifests/*.yaml` → `PROCESS_MANIFEST_YAMLS` (FlowDef cascades)
> 3. `registry/templates/*/*.j2` → `TEMPLATE_FILES` (Jinja2 prompt templates)
> 4. `registry/templates/*/*.yaml` → `TEMPLATE_YAML_FILES` (FlowDef sub-manifests + RenderAct reference)
>
> At runtime, `BridgeManifestExecutor::manifest_yaml` prefers the embedded copy (`process_manifest_yaml(skill_name)`); the filesystem path is a **dev-only fallback**.
>
> `build.rs` declares `cargo:rerun-if-changed=` on every manifest and template, so editing a `.yaml` or `.j2` and running `cargo build` regenerates the embedded copy automatically.[^rust-include-str]

The embedding described in this block is real — `build.rs` still compiles all four artifact classes into the binary. What changed is the **runtime precedence**: the embedded copy is no longer consulted at runtime.

## Amended decision (2026-08-05)

Disk is the single runtime source of truth. The embedded payload exists solely so a self-contained binary can populate the registry on a fresh install with no source tree.

- `BridgeManifestExecutor::manifest_yaml` (`kask/crates/kask_bridge/src/skill_executor.rs:148`) reads the filesystem path **first and only**. There is no compiled-in runtime fallback: if the file is absent or unreadable, it returns `None` (with a `tracing::warn!` on read error, `skill_executor.rs:154`).
- `seed_registry_to_disk` (`skill_executor.rs:184`) materializes the embedded seed to `{kask_data_dir}/skills/registry/` at startup: process manifests, per-skill template manifests, `.j2` templates, and `.yaml` template files. It is **seed-if-missing** — `fs.is_file` short-circuits every write (`skill_executor.rs:188`, `:208`, `:219`, `:232`), so user edits are never overwritten. A user who deletes a shipped file sees it re-seeded on the next startup. `agent_skills::seed_shipped_skills` does the same for shipped `SKILL.md` files.
- DIVERGENCE.md D1 pins the invariant: *"There is no compiled-in runtime fallback — reads exclusively from disk."* In dev (CWD = repo root) the bridge points at the live `kask/registry/` source tree so edits take effect without recompilation; in production it points at the seeded `{kask_data_dir}/skills/registry/`.

Why the inversion: under embedded-preferred, a YAML edit was silently shadowed by the compiled copy until a rebuild — the "edit takes effect" promise was false, and operator-supplied manifests could not override built-ins. Disk-first makes the YAML/Jinja layer a genuine runtime evolution surface (for both developers and end users) while keeping the install-time path-resolution guarantee via the one-time seed.

## Consequences (corrected)

### The "rapid evolution" property is now user-scoped as well as dev-scoped

- **For developers**: edit a `.yaml` or `.j2` → takes effect immediately (dev points at the live source tree). `deny_unknown_fields` on `ManifestFile`/`ManifestHeader` still gives parse-time schema enforcement against drift.
- **For end users**: the seeded registry under `{kask_data_dir}/skills/registry/` is editable and hot-effective. No reinstall or rebuild is needed to change a skill. User edits are sovereign (seed-if-missing never overwrites); only deletions are repaired on the next startup.

### What this means for the architectural rationale

The YAML/Jinja layer is:

- ✅ A **sandboxing surface**: untrusted/evolving content (prompts, PDCA logic) is data, not code. The Rust executor (`ManifestExecutor`) is the correctness membrane — it enforces gas/rjoule budgets, convergence criteria, and `deny_unknown_fields` schema validation. (A prior version of this ADR also credited a FIDES `Source`→`Sink` taint check in `invoke_tool`; that check was deleted 2026-08-12 because both its inputs were constants, so it could not deny. Defense Layer 5 is absent by decision — RR-0053.) A malformed or malicious manifest cannot crash the executor (it fails to parse), and it cannot widen a skill's tool reach: which tools it may dispatch is set by the allowlists outside the YAML layer (the inference IPC `tool_allowlist`, the swarm card `mcp_tools` list, the per-server env allowlists). Note that `McpRuntime::invoke` is **not** part of that enforcement — it meters and dispatches; the per-call capability gate a prior version of this ADR credited was removed 2026-08-12 as vacuous (RR-0056).
- ✅ A **learning and evolution surface for everyone**: no Rust recompile is needed for content changes — neither for developers nor for end users.
- ⚠️ **A trust surface now**: because on-disk YAML is live, a user (or anything that can write the data dir) can change skill behavior at runtime. The executor does not yet distinguish trust provenance at the execution boundary (see below); marketplace signing covers only the marketplace install path.

### Trust model interaction

- **Embedded seeds**: trusted by construction at build time, but they only run once seeded to disk — after seeding, the on-disk copy is what executes, and it is mutable.
- **Marketplace manifests** (installed via `kask_extensions_ui`): Ed25519-signed, verified at download (`verify_manifest_signature` in `collab/src/api/kask_skills.rs`). Installed to `{kask_data_dir}/skills/`.
- **Local manifests** (user-authored, `{kask_data_dir}/skills/`): unsigned, run with the same executor privileges as seeded and marketplace manifests.

The executor emits a provenance signal: `BridgeManifestExecutor::execute_skill` logs `reg.skill.provenance` so an operator reading logs can distinguish seeded/registry skills from filesystem-sourced ones, and `tracing::warn!`s when high-risk actions (`flowdef` sub-cascades, `compute` primitives) execute from filesystem-provenance manifests. Blocking these actions on provenance is a future-wiring target. The `is_skill()` category check is enforced at `execute_skill` (the execution boundary) and at `resolve_manifest` (the `flowdef` sub-cascade binding path). The `on_capability_denied` error-handling policy is wired into the executor: `escalate` → return error with span, `abort` → break cascade with convergence span, default → propagate raw error.

## Alternatives considered

- **Runtime filesystem-only (no embedding)**: rejected — creates the install-time path-resolution problem for self-contained binaries with no source tree. The seed model gets the disk-first semantics without this failure mode.
- **Embedded-first, filesystem fallback (the original decision)**: rejected by the amendment — a stale compiled copy silently shadows disk edits, and the "hot-reload" promise was false for end users.[^fowler-strangler]

## Enforcement

This ADR is enforced by:

- `BridgeManifestExecutor::manifest_yaml` reading disk exclusively (`skill_executor.rs:148`)
- `seed_registry_to_disk` seed-if-missing semantics (`skill_executor.rs:184`) + `agent_skills::seed_shipped_skills`
- DIVERGENCE.md D1 pinning ("no compiled-in runtime fallback")
- `manifest_compliance.rs` and `skill_companion_consistency.rs` integration tests (cross-artifact consistency)
- `deny_unknown_fields` on `ManifestFile`/`ManifestHeader` (schema enforcement at parse time)[^fowler-refactoring]

---

## References

[^fowler-poeaa]: Fowler, M. (2002). *Patterns of enterprise application architecture*. Addison-Wesley. https://martinfowler.com/books/eaa.html
    Cited for the Registry pattern — the four-layer architecture uses a registry as the source of truth for template manifests and process manifests.

[^rust-include-str]: The Rust Standard Library. (n.d.). *include_str! macro*. The Rust Project. https://doc.rust-lang.org/std/macro.include_str.html
    Cited for the build-time embedding mechanism (`include_str!`) that carries the seed payload in the binary.

[^saltzer-protection]: Saltzer, J. H., & Schroeder, M. D. (1975). The protection of information in computer systems. *Proceedings of the IEEE*, 63(9), 1278–1308. https://doi.org/10.1109/PROC.1975.9939
    Cited for the trust-model principles underlying the seeded (trusted by construction at build time) vs. marketplace (signed) vs. local (unsigned) provenance distinction.

[^fowler-strangler]: Fowler, M. (2004). *StranglerFigApplication*. https://martinfowler.com/bliki/StranglerFigApplication.html
    Cited for the incremental-replacement pattern informing the embedded-first → disk-first-seed migration.

[^fowler-refactoring]: Fowler, M. (2018). *Refactoring: Improving the design of existing code* (2nd ed.). Addison-Wesley. https://martinfowler.com/books/refactoring.html
    Cited for the schema-enforcement and integration-test discipline that pins the registry mechanism against drift.
