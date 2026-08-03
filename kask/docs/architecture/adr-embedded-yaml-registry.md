---
title: "ADR: Build-time embedded YAML/Jinja2 registry — dev-scoped evolution, user-scoped freeze"
audience: [architects, developers, agents]
last_updated: 2026-08-03
version: "0.31.0"
status: "Active"
domain: "architecture"
mds_categories: [composition, lifecycle, trust]
---

# ADR: Build-time embedded YAML/Jinja2 registry — dev-scoped evolution, user-scoped freeze

**Status:** Active — documents a decision already implemented in `hkask-templates/build.rs` and consumed by `kask_bridge/src/skill_executor.rs`. Recorded retroactively so the dev-vs-user asymmetry is not misread by future explorers.

## Context

The kask-skills system is a four-layer architecture with a single Rust bridge:

| Layer | Source of truth | Consumer |
|-------|----------------|----------|
| SKILL.md companions | `.agents/skills/<name>/SKILL.md` | `agent_skills` (discovery catalog) |
| Per-skill template manifests | `registry/templates/<name>/manifest.yaml` | `Registry` (template index) |
| Process manifests (FlowDef PDCA) | `registry/manifests/<name>.yaml` | `ManifestExecutor` (cascade driver) |
| Jinja2 templates | `registry/templates/<name>/*.j2` | `TemplateRenderer` (prompt rendering) |

The architectural rationale — often stated as *"the flexible non-compiled YAML and Jinja2 layers can rapidly evolve as a natural sandboxing and learning surface around the core Rust code"* — is only half the story. The other half is in `hkask-templates/build.rs`.

## Decision

`build.rs` embeds **all four artifact classes** into the binary at compile time via `include_str!`:

1. `registry/templates/*/manifest.yaml` → `MANIFEST_YAMLS` (per-skill template manifests)
2. `registry/manifests/*.yaml` → `PROCESS_MANIFEST_YAMLS` (FlowDef cascades)
3. `registry/templates/*/*.j2` → `TEMPLATE_FILES` (Jinja2 prompt templates)
4. `registry/templates/*/*.yaml` → `TEMPLATE_YAML_FILES` (FlowDef sub-manifests + RenderAct reference)

At runtime, `BridgeManifestExecutor::manifest_yaml` prefers the embedded copy (`process_manifest_yaml(skill_name)`); the filesystem path is a **dev-only fallback** (per the `build.rs` header: *"The filesystem paths in `main.rs` are dev-only fallbacks"*).

`build.rs` declares `cargo:rerun-if-changed=` on every manifest and template, so editing a `.yaml` or `.j2` and running `cargo build` regenerates the embedded copy automatically.

## Consequences

### The "rapid evolution" property is dev-scoped, not user-scoped

- **For developers**: edit a `.yaml` or `.j2` → `cargo build` → the embedded copy updates. The YAML/Jinja layer genuinely evolves faster than Rust code, and `deny_unknown_fields` on `ManifestFile`/`ManifestHeader` gives compile-time schema enforcement against drift. This is a real and well-designed sandboxing surface.
- **For end users**: the registry is frozen at build time. An end user cannot hot-reload a skill without reinstalling the binary. The filesystem fallback only activates when the source tree is present (dev workflows).

This asymmetry is deliberate: it eliminates the install-time path-resolution problem ("skills execute via the embedded manifests and templates regardless of CWD or install location"). But it means the "evolution surface around the core Rust" is a development-time property, not a runtime property.

### What this means for the architectural rationale

The YAML/Jinja layer is:

- ✅ A **sandboxing surface**: untrusted/evolving content (prompts, PDCA logic) is data, not code. The Rust executor (`ManifestExecutor`) is the security and correctness membrane — it enforces gas/rjoule budgets, OCAP gating, taint propagation, convergence criteria, and `deny_unknown_fields` schema validation. A malformed or malicious manifest cannot crash the executor (it fails to parse) or bypass the OCAP gate (enforced at `McpRuntime::invoke`, not at the YAML layer).
- ✅ A **learning and evolution surface for developers**: the `cargo:rerun-if-changed` wiring means a developer can iterate on a skill's PDCA loop or prompt templates in seconds, not minutes. No Rust recompile needed for content changes — only the `include_str!` re-embedding runs.
- ⚠️ **Not a runtime hot-reload surface**: end users get the frozen embedded copy. If runtime skill evolution is needed (e.g., a user editing a skill without rebuilding), the filesystem fallback path would need to be promoted from dev-only to a first-class runtime path with its own trust/signing model (currently only the marketplace path signs manifests).

### Trust model interaction

The embedding decision interacts with the trust model:

- **Embedded manifests** (built-in): trusted by construction — they were compiled into the binary by the developer. No signature verification.
- **Marketplace manifests** (installed via `kask_extensions_ui`): Ed25519-signed, verified at download (`verify_manifest_signature` in `collab/src/api/kask_skills.rs`). Installed to `data_dir()/agents/skills/` and resolved via the filesystem fallback path.
- **Local manifests** (user-authored, `data_dir()/agents/skills/`): unsigned, resolved via the filesystem fallback path. Run with the same executor privileges as embedded and marketplace manifests.

The executor does not currently distinguish trust provenance (see candidate D in the refactor-architecture ra-candidates analysis). The embedding decision means built-in skills are implicitly trusted (compiled in), while user/marketplace skills rely on the filesystem path — which is correct but undocumented.

## Alternatives considered

- **Runtime filesystem-only** (no embedding): rejected because it creates an install-time path-resolution problem. The binary would need to locate `registry/` relative to CWD or an env var, breaking skills when run from an unexpected directory.
- **Hybrid with runtime precedence** (filesystem first, embedded fallback): rejected because it would allow a local file to silently override a built-in skill without a trust signal. The current design (embedded first, filesystem fallback) ensures built-in skills are stable; the filesystem is opt-in (dev mode or marketplace install).

## Enforcement

This ADR is enforced by:

- `build.rs` `include_str!` embedding (compile-time)
- `BridgeManifestExecutor::manifest_yaml` preferring embedded copy (runtime)
- `manifest_compliance.rs` and `skill_companion_consistency.rs` integration tests (cross-artifact consistency)
- `deny_unknown_fields` on `ManifestFile`/`ManifestHeader` (schema enforcement at parse time)