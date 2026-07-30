---
title: "hkask-templates — Tutorial: Your First Skill Manifest"
audience: [developers new to hKask skills]
last_updated: 2026-07-29
version: "0.2.0"
status: "Active"
domain: "Skills"
mds_categories: [lifecycle]
---

# hkask-templates — Tutorial: Your First Skill Manifest

This tutorial walks through creating a `manifest.yaml` file for a new skill.
You will learn the manifest structure, the step cascade, and how the
`ManifestExecutor` runs it.

## Learning path

```mermaid
flowchart TD
    A[Step 1: Create manifest.yaml] --> B[Step 2: Define steps]
    B --> C[Step 3: Write Jinja2 templates]
    C --> D[Step 4: Test with ManifestExecutor]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-TPL-003
verified_date: 2026-07-29
verified_against: kask/crates/hkask-templates/src/bundle/manifest.rs:91,35; kask/crates/hkask-templates/src/manifest_loader.rs:123; kask/crates/hkask-templates/src/executor.rs:413
status: VERIFIED
-->

## Steps 1-2: Create the manifest and define steps

Create a `manifest.yaml` file in `kask/registry/manifests/`. The
`BundleManifest` struct (`bundle/manifest.rs:91`) is the parsed form. Define a
list of `BundleManifestStep` entries (`bundle/manifest.rs:35`), each with an
`ordinal`, an `action`, a `template_ref` (Jinja2 template path), and a
`phase` (`Pre`/`Core`/`Post`, see `bundle/cascade.rs:8`).

Each step's `action` selects a cascade branch in `ManifestExecutor::execute_manifest`
(`executor.rs:413`): `select` (LLM inference), `populate` (render-only),
`render` (RenderAct, no inference), `flowdef` (nested sub-manifest),
`tool_invoke` (MCP tool call), `compute` (deterministic math primitive),
`choice` (conditional branch), `loop` (PDCA re-entry), `abort`, or `escalate`.

## Steps 3-4: Write templates and test

Write Jinja2 templates in `kask/registry/templates/<skill>/`. Load the
manifest with `load_manifest_from_file` (`manifest_loader.rs:123`) and execute
it with `ManifestExecutor::execute_manifest` (`executor.rs:413`). Step results
are stored in the context map under `step_{ordinal}_result` keys; the final
result is the highest-ordinal `step_N_result` (see
`kask_bridge/src/skill_executor.rs:251` for the ordinal-keyed extractor).

## See also

- [hkask-templates Reference](./reference.md): class diagram of the manifest.
- [hkask-templates How-to](./how-to.md): adding a PDCA step.
- [hkask-templates Explanation](./explanation.md): the D1 invocation sequence.
- [`kask/docs/explanation/skills-and-composition.md`](../../explanation/skills-and-composition.md).

---

[^beck-tdd]: Beck, K. (2003). *Test-Driven Development: By Example.* Addison-Wesley. <https://www.oreilly.com/library/view/test-driven-development/0321146530/>.
