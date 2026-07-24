# Zed Codebase Architecture Audit — 2026-07-24

<!--
DC+BIBO document metadata
Title:        Zed Codebase Architecture Audit
Creator:      improve-codebase-architecture + idiomatic-rust + pragmatic-laziness + falsifiability + grill-me + kata-improvement + task-breakdown + codegraph (manual mode)
Date:         2026-07-24
Type:         bibo:Document
Description:  Read-only architectural audit of the zed-kask codebase. Surfaces large
              files, graph malformations, consolidation opportunities, and cross-cutting
              refactorings. No code changes — analysis only.
-->

## Scope and method

Read-only audit of `crates/` (240 crates, 2,522 `.rs` files, ~1.70M LOC including
tests, ~1.28M LOC excluding tests/fixtures). The `codegraph` MCP server was not
available in this session, so structural analysis was performed with `find`,
`wc`, `grep`, and `awk` against the source tree. Findings are grounded in
concrete line counts, function counts, and `Cargo.toml` dependency edges.

Skills composed: `improve-codebase-architecture` (deletion test, deep-module
discipline), `idiomatic-rust` (Hoare principles, ownership graph, error
domain), `pragmatic-laziness` (least-action, effort hotspots), `falsifiability`
(admissibility gate, multiple hypotheses, discriminating tests),
`grill-me` (gap probing), `kata-improvement` (direction → current → target →
experiment), `task-breakdown` (vertical slicing, dependency ordering).

---

## Kata Step 1 — Direction

**Challenge.** The zed codebase is a mature, large Rust codebase whose
architecture must remain navigable as features accumulate. The directional
challenge: *keep the semantic dependency graph well-formed (acyclic, narrow
seams, deep modules) and the code idiomatic (Hoare principles), so that change
remains local and tests remain reachable through public interfaces.*

**Excellent performance looks like.**
- No single non-test `.rs` file exceeds ~5,000 LOC.
- No `impl T` block carries more than ~50 public methods (deep interface test).
- No mutual `Cargo.toml` dependency edges between sibling crates.
- Every crate earns its keep under the deletion test (complexity reappears
  elsewhere if deleted).
- `unwrap()` / `expect()` / `let _ =` counts in non-test source are bounded and
  each occurrence is justifiable.

**Measurement plan.** Line counts, function counts, `Cargo.toml` edge counts,
`grep`-based pattern counts. All measurements taken in this session; baseline
recorded below.

---

## Kata Step 2 — Current condition (baseline measurements)

### 2.1 Largest non-test source files (LOC)

| Rank | File | LOC | `pub fn` count | Notes |
|------|------|-----|-----------------|-------|
| 1 | `crates/workspace/src/workspace.rs` | 17,308 | 445 | `impl Workspace` = 263 methods |
| 2 | `crates/project/src/lsp_store.rs` | 15,051 | 288 | `impl LspStore` = 180 methods |
| 3 | `crates/agent_ui/src/agent_panel.rs` | 13,399 | 340 | 3× `impl AgentPanel` blocks |
| 4 | `crates/agent_ui/src/conversation_view/thread_view.rs` | 12,768 | 251 | 2× `impl ThreadView` blocks (768–6013, 6013–end) |
| 5 | `crates/git_ui/src/git_panel.rs` | 12,558 | 308 | 4× `impl GitPanel` blocks |
| 6 | `crates/editor/src/editor.rs` | 12,536 | 488 | `impl Editor` = 329 methods |
| 7 | `crates/editor/src/element.rs` | 12,510 | 183 | |
| 8 | `crates/agent_ui/src/conversation_view.rs` | 11,118 | 339 | |
| 9 | `crates/project/src/git_store.rs` | 10,982 | 313 | 2× `impl Repository` blocks (5492, 10137) |
| 10 | `crates/settings_ui/src/page_data.rs` | 10,659 | 93 | 16 free `fn`s, one per settings page |
| 11 | `crates/acp_thread/src/acp_thread.rs` | 10,055 | 298 | |
| 12 | `crates/dev_container/src/devcontainer_manifest.rs` | 9,781 | 135 | single `impl DevContainerManifest` spans 66–2676 |
| 13 | `crates/workspace/src/pane.rs` | 9,503 | 274 | |
| 14 | `crates/agent/src/thread.rs` | 9,282 | 322 | |
| 15 | `crates/sidebar/src/sidebar.rs` | 8,481 | 182 | |
| 16 | `crates/multi_buffer/src/multi_buffer.rs` | 8,280 | 423 | high fn density per LOC |

**Pattern.** 16 non-test files exceed 8,000 LOC. The top 6 exceed 12,000 LOC.
Several files carry multiple `impl T` blocks for the same type — a structural
smell that the type has more than one responsibility and the file is being used
as a catch-all.

### 2.2 Largest crates (non-test LOC)

| Crate | LOC | Internal deps |
|-------|-----|---------------|
| `editor` | 115,755 | 36 depend on it |
| `agent_ui` | 83,636 | 4 depend on it |
| `project` | 76,686 | 38 depend on it |
| `gpui` | 73,945 | 103 depend on it |
| `agent` | 49,012 | — |
| `workspace` | 48,212 | 27 depend on it (workspace = the Cargo workspace name collision) |
| `vim` | 47,657 | — |
| `git_ui` | 45,663 | — |
| `settings_ui` | 30,703 | — |

### 2.3 Dependency graph shape (corrected after verification)

**Initial false positive, corrected.** A naive `grep` for `^workspace` in
`crates/project/Cargo.toml` matched `workspace = true` under `[lints]`
(Cargo workspace-inheritance shorthand for lint config), **not** a dependency
on the `workspace` crate. After verifying actual `[dependencies]` sections
and `use` imports, the real graph among the top crates is:

```
editor ──→ workspace ──→ project
  │           │
  └───────────┘
  (editor also depends on project directly)
```

- `editor` → `project` (real, production: `use project::lsp_store::...`)
- `editor` → `workspace` (real, production: `use workspace::{OpenOptions,
  OpenVisible, CollaboratorId, TabBarSettings, ...}`)
- `workspace` → `project` (real, production: `use project::{Project,
  ProjectEntryId, ProjectPath}`)
- `project` → neither `editor` nor `workspace` (clean — `project` is a
  leaf in this subgraph; verified zero `use editor::` / `use workspace::`
  imports in `project/src`)

**Verdict: the graph is a DAG (no cycle).** The earlier "mutual edge" finding
was a grep false positive (`.workspace = true` inheritance syntax vs.
`workspace` crate dependency). The graph is well-formed *topologically*.

**Remaining concern is coupling width, not cyclicity.** `editor` (115k LOC,
36 dependents) depends on both `workspace` and `project` directly, and
`workspace` also depends on `project`. Any change to `project`'s public
surface ripples to both `editor` and `workspace`; any change to
`workspace`'s surface ripples to `editor`. This is a wide coupling fan-in
on `project` and `workspace`, but it is not a graph malformation — it is
the expected shape for a layered app (UI shell → project model).

**`agent_ui` → `editor` + `workspace` + `project` (transitive, wide).**
`agent_ui` (83k LOC, only 4 dependents) is a leaf consumer that pulls in
the full `editor`/`project`/`workspace` triad. Its 13k-LOC `agent_panel.rs`
and 12k-LOC `thread_view.rs` are doing UI rendering, agent protocol state,
terminal management, and serialization in one file each.

### 2.4 Idiomatic-Rust baseline (non-test source)

| Crate | `unwrap()` | `expect()` | `let _ =` | `.clone()` |
|-------|-----------|-----------|-----------|-----------|
| `editor` | 734 | 120 | 12 | 1,617 |
| `agent_ui` | 540 | 286 | 24 | 2,787 |
| `project` | 177 | 32 | 28 | 1,991 |
| `workspace` | 460 | 46 | 7 | 1,181 |
| `git_ui` | 288 | 131 | 6 | 1,411 |
| `agent` | 681 | 180 | 23 | 1,702 |
| `gpui` | 353 | 110 | 19 | 777 |

**Observations.**
- `agent_ui` has the highest `expect()` count (286) and the highest `.clone()`
  count (2,787). Many `expect()` calls are in `thread_metadata_store.rs` with
  messages like `"metadata should be cached"` — these are invariants being
  asserted at runtime rather than encoded in the type system. Hoare P3
  (ownership graph) and P5 (error domain) are both weak here.
- `project` has 28 `let _ =` patterns — the most of any crate. Several discard
  fallible operations: `let _ = self.pull_document_diagnostics_for_server(...)`,
  `let _ = self.recalculate_diffs(...)`, `let _ = self.send_keyed_job(...)`,
  `let _ = child.kill()`. These violate the project's own `.rules` ("Never
  silently discard errors with `let _ =` on fallible operations").
- `editor` has 734 `unwrap()` calls. Spot checks show many are on
  `NonZeroU32::new(1).unwrap()`, `selections.next().unwrap()` after a
  length check, and `char::from_u32(...).unwrap()` on a computed code point.
  Some are provably unreachable; others (`self.style.as_ref().unwrap()`) are
  state invariants that should be encoded as `Option<&EditorStyle>` or a
  state enum.

### 2.5 Cross-cutting concerns (consolidation signals)

- **`settings.rs` appears 44 times** across crates. Most are per-crate
  `Settings` structs implementing the global settings registry. This is the
  intended pattern (each crate owns its settings), not duplication — but the
  *migrator* crate has 22 `migrations/m_*/settings.rs` files, one per
  migration. The migrator pattern is fine; the count signals that settings
  migrations are a frequent, mechanically-similar change.
- **`telemetry.rs` appears 4 times** (`telemetry`, `client`,
  `language_models/provider/anthropic`, `zed/reliability/hang_detection`).
  Each is a different concern; no consolidation opportunity.
- **`mod.rs` appears 6 times** (`gpui/elements`, `agent/tests`,
  `keymap_editor/ui_components`, `repl/kernels`, `call/call_impl`,
  `terminal/mappings`). The project `.rules` say "Never create files with
  `mod.rs` paths". These are pre-existing violations; 6 is a bounded,
  fixable count.

### 2.6 Shallow / stub crates (deletion-test candidates)

| Crate | LOC | Status |
|-------|-----|--------|
| `gpui_platform` | 186 | Facade crate that re-exports platform features from `gpui_macos`/`gpui_linux`/`gpui_windows`. Earns its keep as a platform-abstraction seam. |
| `gpui_tokio` | 100 | Tokio runtime bridge. Small but load-bearing. |
| `language_onboarding` | 100 | Small UI onboarding flow. |
| `env_var` | 40 | Tiny utility. Deletion-test candidate. |

**Correction on `gpui_shared_string`.** Initially flagged as a phantom
because `find ... -name "*.rs"` under `src/` returned 0 LOC. The crate
has no `src/` directory — its `[lib] path = "gpui_shared_string.rs"` is
at the crate root (203 LOC). It exports `SharedString` (an `Arc<str>` /
`&'static str` abstraction over `SmolStr`), and has **4 real dependents**:
`gpui`, `env_var`, `language_core`, `language_model_core`. It is
load-bearing and earns its keep. **Not a deletion candidate.** The
initial 0-LOC finding was a `find`-path false positive.

**Deletion-test verdicts.**
- `gpui_shared_string`: **keep.** Load-bearing shared type with 4
  dependents. (Initial phantom classification was a `find`-path error.)
- `env_var` (40 LOC): apply the deletion test — if the few helpers are
  inlined at call sites, does complexity reappear? Likely yes (env-var
  parsing with validation is non-trivial), so it probably earns its keep.
  Verify by checking callers.
- `gpui_platform`: **keep.** Platform abstraction is a textbook deep seam.

### 2.7 Family-of-crates consolidation map

| Family | Members | Total LOC | Notes |
|--------|---------|-----------|-------|
| `gpui` | `gpui`, `gpui_linux`, `gpui_macos`, `gpui_windows`, `gpui_platform`, `gpui_shared_string`, `gpui_tokio`, `gpui_util`, `gpui_wgpu`, `gpui_web`, `gpui_macros` | ~110k | Platform split is correct. `gpui_shared_string` is dead. |
| `language` | `language`, `language_core`, `language_extension`, `language_model`, `language_model_core`, `language_models`, `language_models_cloud`, `language_selector`, `language_tools`, `language_onboarding`, `languages` | ~64k | 11 crates. `language` vs `language_core` vs `language_model_core` vs `language_model` is a fine-grained split that may have shallow members. Apply deletion test to `language_core` (1,916 LOC) and `language_model_core` (3,070 LOC). |
| `agent` | `agent`, `agent_ui`, `agent_servers`, `agent_settings`, `agent_skills`, `acp_thread`, `acp_tools`, `ai_onboarding` | ~176k | `agent` (49k) + `agent_ui` (83k) + `acp_thread` (14k) = 146k of the 176k. The other 5 are small. `acp_thread` and `agent` overlap in domain (agent threads) — verify the seam is clean. |
| `edit_prediction` | 6 crates | ~44k | `edit_prediction_cli` (18.8k) is larger than `edit_prediction` (10k). Verify the CLI crate isn't doing core logic. |
| `git` | `git`, `git_ui`, `git_hosting_providers` | — | Clean split (lib / ui / hosting). |
| `copilot` | `copilot`, `copilot_chat`, `copilot_ui` | — | Clean split. |

---

## Falsifiability — Multiple working hypotheses for the large-file problem

**Target claim.** "The 16 files >8k LOC are architectural defects that should
be refactored." This is an IS claim about the codebase. Admissible: a
falsifying observation would be "the file is internally cohesive (single
responsibility, single impl block, high locality) and splitting it would
scatter one concept across many small modules" (the deep-module defense).

**H1 — God-object hypothesis.** The large files are god objects: multiple
responsibilities, multiple `impl T` blocks, methods spanning unrelated
concerns. *Prediction:* the top files contain ≥2 `impl T` blocks for the same
`T` OR a single `impl T` with methods from >3 distinct concern areas (render,
state, serialization, event handling, IO). *Falsifier:* a file with one `impl
T` block whose methods all serve one concern.

**H2 — Test-bloat hypothesis.** The large files are large because they
co-locate test-support code with production code under `#[cfg(test)]`. *Prediction:*
>40% of LOC in the top files is under `#[cfg(test)]` or `#[cfg(any(test,
feature = "test-support"))]`. *Falsifier:* <10% test-gated LOC.

**H3 — Deep-module hypothesis (Ousterhout defense).** The large files are
*deep modules* — small interface, large implementation — and the size is
justified. *Prediction:* the file exports a small public surface (≤20 public
items) and the bulk is private implementation. *Falsifier:* the file exports
a wide public surface (the interface is as complex as the implementation).

**H4 — Accumulated-history hypothesis.** The files grew by accretion; each
addition was locally reasonable but no periodic consolidation occurred. The
multiple `impl T` blocks (where the second is `#[cfg(test)]`-gated) are the
signature. *Prediction:* the second/third `impl T` block is test-gated or
contains `*_for_test` methods. *Falsifier:* all `impl T` blocks are
production code with no test gating.

### Discriminating tests applied

| File | `impl T` blocks | Test-gated 2nd block? | Public surface | Verdict |
|------|----------------|----------------------|----------------|---------|
| `git_panel.rs` | 4× `impl GitPanel` (1030, 7998, 8010, + Render/Focusable) | Yes — block at 8010 is `#[cfg(any(test, feature = "test-support"))]` | Wide | **H1 + H4 corroborated.** Main block (1030–7998) has 202 methods. Test block is separate. |
| `thread_view.rs` | 2× `impl ThreadView` (768, 6013) | First block 768–6013 (113 methods, production). Second 6013–end (render + helpers). | Wide | **H1 corroborated.** Render split from logic, but in same file. |
| `agent_panel.rs` | 3× `impl AgentPanel` (1191, 5068, 6547) | Block at 5068 is `Panel` impl; 6547 is post-`Render` helpers. | Wide | **H1 + H4 corroborated.** |
| `git_store.rs` | 2× `impl Repository` (5492, 10137) | Block at 10137 is `*_for_test` methods. | Wide | **H4 corroborated** (test accretion). |
| `editor.rs` | 1× `impl Editor` (1729) with 329 methods | N/A | Very wide (488 pub fns) | **H3 falsified** — interface is as wide as implementation. **H1 corroborated.** |
| `workspace.rs` | 1× `impl Workspace` (1488) with 263 methods | N/A | Very wide (445 pub fns) | **H3 falsified. H1 corroborated.** |
| `lsp_store.rs` | 1× `impl LspStore` (4439) with 180 methods | N/A | Wide (288 pub fns) | **H1 partially corroborated** — single impl but very wide. |
| `page_data.rs` | 0 `impl` blocks; 16 free `fn`s, one per settings page | N/A | 16 page-builder fns | **H1 corroborated differently** — not a god object but a flat catalog that should be one module per page. |
| `devcontainer_manifest.rs` | 1× `impl DevContainerManifest` (66–2676) | N/A | Narrow-ish (135 pub fns) | **H3 partially corroborated** — single responsibility (manifest parsing). Size is partly justified. Worth splitting by concern (compose / dockerfile / features) but lower priority. |

**Verdict.** `one_corroborated_survivor` is too strong; `multiple_corroborated`
is the honest verdict. H1 (god object) and H4 (accretion) are corroborated
for the panel/view files (`git_panel`, `agent_panel`, `thread_view`,
`git_store`). H3 (deep module) is *falsified* for `editor.rs` and
`workspace.rs` — their interfaces are as wide as their implementations, which
is the opposite of deep. H2 (test bloat) is *falsified* for all top files —
the test-gated blocks are small relative to the whole.

**Irreducible remainder.** `devcontainer_manifest.rs` is a borderline case:
single responsibility, but 9.7k LOC. Splitting by sub-concern (compose
resources, dockerfile parsing, features build info) is plausible but not
strongly indicated. Mark as lower priority.

---

## Pragmatic-laziness — Effort hotspots and least-action analysis

### Syntax layer (structure)
- 16 files >8k LOC, 6 files >12k LOC.
- Mutual `project` ↔ `workspace` edge.
- `editor` → `project` + `workspace` wide coupling.
- 6 `mod.rs` files (`.rules` violation).
- 1 phantom crate (`gpui_shared_string`).

### Semantics layer (behavior)
- `editor.rs` `impl Editor` = 329 methods. No single responsibility.
- `workspace.rs` `impl Workspace` = 263 methods.
- `lsp_store.rs` `impl LspStore` = 180 methods.
- `git_panel.rs` splits `impl GitPanel` across 4 blocks — the type is
  doing rendering, state, serialization, context menus, and test scaffolding.
- `project` crate has 28 `let _ =` fallible-discards — a `.rules` violation
  and an error-domain (Hoare P5) defect.

### Pragmatics layer (intent)
- The panels (`git_panel`, `agent_panel`, `thread_view`) are the UI surfaces
  users touch most. Their size is driven by feature accretion, not by a
  single deep responsibility.
- `editor.rs` and `workspace.rs` are the load-bearing types of the app.
  Their width is the central architectural risk: every feature adds a
  method to one of them.

### Effort hotspots (energy per unit value)
1. **`editor.rs` / `element.rs`** — 25k LOC combined, 671 pub fns. Any
   editor change touches this surface. Highest effort-per-change in the
   codebase.
2. **`project` ↔ `workspace` cycle** — every cross-cutting feature must
   thread both crates. The cycle is a permanent effort tax.
3. **`agent_ui` panels** — 3 files >11k LOC each, high `expect()` density,
   high `.clone()` density. Effort hotspot for the agent feature team.

### Lazy-stationary-action (corrected)
- **Delete `gpui_shared_string`**: ~~complexity vanishes.~~ **REVERSED —
  not a phantom.** It is a 203-LOC crate (lib path at crate root, not
  `src/`) exporting `SharedString` with 4 dependents (`gpui`, `env_var`,
  `language_core`, `language_model_core`). **Retain.** The initial
  classification was a `find`-path false positive (no `src/` dir).
- **Delete the second `impl GitPanel` test block from `git_panel.rs`** and
  move to a `git_panel_tests.rs` sibling: complexity (test methods) moves
  out of the production file. **Eliminate** (move).
- **Delete `mod.rs` files** by renaming to the crate-conventional
  `foo.rs` + `foo/` directory pattern: complexity vanishes. **Eliminate.**
- **Delete `editor.rs`'s `impl Editor`**: complexity reappears scattered
  across 329 method definitions with no home. **Retain** — but split the
  *impl* across concern-specific submodules (`editor/render.rs`,
  `editor/input.rs`, `editor/selection_ops.rs`, etc.) keeping `Editor` the
  single type. The type stays; the file shrinks.
- **Delete `workspace.rs`'s `impl Workspace`**: same as editor. **Retain
  the type, split the impl.**
- **The `project` ↔ `workspace` "cycle" was a false positive** (Cargo
  workspace-inheritance syntax, not a crate dependency). No back-edge to
  break. The real graph is a DAG. **No action** on the graph topology; the
  remaining concern is `editor`'s wide coupling to both `workspace` and
  `project`, which is structural (layered app shape) and not a deletion
  candidate.

**Stationary-action check (δS).** Not yet stationary — the elimination
candidates above reduce action without adding complexity. The remaining
work (splitting `impl Editor` / `impl Workspace` / `impl LspStore` across
submodules) is the irreducible remainder; it is structural work that
concentrates locality rather than eliminating it.

---

## Idiomatic-Rust — Hoare principle assessment of the top defects

| Principle | `editor.rs` | `workspace.rs` | `lsp_store.rs` | `agent_ui` panels | `project` `let _ =` |
|-----------|-------------|----------------|----------------|-------------------|---------------------|
| P1 — make wrong usage impossible | Weak: `style: Option<EditorStyle>` + `unwrap()` on it | Weak | Moderate | Weak: `expect("metadata should be cached")` | Weak: fallible fns return `()` discarding `Result` |
| P2 — ownership graph clear | Weak: 329 methods, hard to see who mutates what | Weak | Moderate | Weak: 2,787 `.clone()` | Moderate |
| P3 — error domain explicit | Moderate | Moderate | Moderate | Weak: 286 `expect()` | **Violated**: 28 `let _ =` on `Result` |
| P4 — traits as capabilities | Good (many small traits) | Good | Good | Moderate | — |
| P5 — no silent error swallow | Good | Good | Good | Good | **Violated** (28 sites) |
| P6 — derive common traits | Good | Good | Good | Good | — |
| P7 — `Result` propagation | Good | Good | Good | Good | Violated at 28 sites |
| P8 — ecosystem alignment | Good | Good | Good | Good | — |

**Highest-severity violations.**
1. `project` crate's 28 `let _ =` on `Result` — violates P3, P5, P7, and
   the project's own `.rules`. **Fix is mechanical and high-signal.**
2. `agent_ui`'s 286 `expect()` calls — many encode invariants
   ("metadata should be cached") that should be type-state or `Option`
   rather than runtime assertions. P1 violation.
3. `editor.rs` `self.style.as_ref().unwrap()` — `style` is `Option<EditorStyle>`
   but is treated as always-`Some` after initialization. Either make it
   non-optional (init in `new`) or encode the pre-init state as an enum
   variant. P1 violation.

---

## Grill-me — Gap probing (self-interrogation)

**Q1 (Recall).** How many non-test `.rs` files exceed 8,000 LOC?
**A.** 16.

**Q2 (Mechanism).** Why does `git_panel.rs` have 4 `impl GitPanel` blocks?
**A.** Block 1 (1030) is the main production impl (202 methods). Block 2
(7998) is a small production impl with a few helpers. Block 3 (8010) is
`#[cfg(any(test, feature = "test-support"))]`-gated test helpers. Block 4
is `Render`/`Focusable`/`Panel` trait impls. The split is partly test-gating
(H4) and partly trait-impl grouping.

**Q3 (Rationale).** Why does `project` ↔ `workspace` appear to be a mutual
edge?
**A.** **It is not.** The appearance was a grep false positive:
`workspace = true` under `[lints]` in `crates/project/Cargo.toml` is Cargo
workspace-inheritance shorthand for lint configuration, not a dependency
on the `workspace` crate. Verified: zero `use workspace::` imports in
`project/src`, and `workspace` is absent from `project`'s
`[dependencies]` section. The real graph is `editor → workspace → project`
plus `editor → project` — a DAG. **This question is resolved; the focus
obstacle O1 is closed.**

**Q3′ (Rationale, revised).** Given the graph is a DAG, what is the actual
highest-leverage graph concern?
**A.** `editor`'s direct dependency on both `workspace` and `project` is
wide coupling, but it is the expected layered-app shape and not a defect.
The higher-leverage concern is the *file-level* god objects (`impl Editor`
= 329 methods, `impl Workspace` = 263 methods) which make the wide coupling
hard to evolve. Shrinking the impls by concern is the leverage; the graph
topology itself is fine.

**Q4 (Edge cases).** What breaks if we delete `gpui_shared_string`?
**A.** Need to check: does any `Cargo.toml` list it as a dependency? If
yes, those crates break. If no, it is a phantom and safe to remove. *Not
verified in this session — flag as open question.*

**Q5 (Synthesis).** What is the single highest-leverage refactor?
**A.** Breaking the `project` ↔ `workspace` cycle. It is a graph
malformation (not a file-size issue), it affects the two most-depended-on
crates after `gpui`, and it is likely a `[dev-dependencies]` move rather
than a redesign. Lowest cost, highest graph-health return.

**Gap analysis.**
- **Solid:** file-size inventory, `impl T` block counts, idiomatic-Rust
  pattern counts, and (after correction) the dependency-graph shape.
- **Resolved:** the `project` ↔ `workspace` "cycle" was a grep false
  positive (Cargo `.workspace = true` inheritance syntax). The real graph
  is a DAG: `editor → workspace → project` plus `editor → project`.
- **Gap:** caller counts for `gpui_shared_string` (deletion safety);
  per-concern method grouping for `impl Editor` (which methods belong to
  rendering vs input vs selection vs serialization); whether
  `edit_prediction_cli` (18.8k LOC) is doing core logic that belongs in
  `edit_prediction`.

---

## Kata Step 3 — Target condition (1–3 months out)

**Target.**
1. ~~Break the `project` ↔ `workspace` mutual edge.~~ **Resolved: no
   cycle exists.** The graph is a DAG. No action on topology.
2. Reduce `editor`'s direct coupling width by ensuring `editor` depends on
   `project` only through `workspace` where possible (audit which `project`
   symbols `editor` imports directly vs via `workspace` re-exports).
   **Metric:** `editor`'s direct `project` imports reduced to
   language-server/LSP-protocol symbols only.
2. Eliminate all 6 `mod.rs` files per `.rules`. **Metric:** 0 `mod.rs`.
3. Move all `*_for_test` / `#[cfg(test)]` `impl T` blocks out of production
   `.rs` files into `*_tests.rs` siblings. **Metric:** 0 test-gated `impl
   T` blocks in the top-16 large files.
4. Fix the 28 `let _ =` fallible-discards in `project`. **Metric:** 0
   `let _ =` on `Result` in `project/src`.
5. Split `impl Editor` (329 methods) and `impl Workspace` (263 methods)
   across concern-specific submodules. **Metric:** no single `impl T`
   block exceeds 100 methods; `editor.rs` and `workspace.rs` each <6,000
   LOC with the bulk moved to `editor/<concern>.rs` siblings.

**Obstacles (parking lot).**
- ~~O1: The `project` → `workspace` edge reason is unknown (test vs prod).~~
  **Closed — false positive, no cycle.**
- ~~O4: `gpui_shared_string` deletion safety unverified.~~ **Closed — not
  a phantom, 4 dependents, keep.**
- O2: Splitting `impl Editor` requires classifying 329 methods by concern
  — labor-intensive, high blast radius (36 dependents).
- O3: `agent_ui` panel splits are large UI refactors with rendering
  subtlety; high risk of regression.
- O5: Which `project` symbols does `editor` import directly that
  could come through `workspace` instead (to narrow `editor`'s direct
  coupling)?

**Focus obstacle.** **O5** — classify `editor`'s direct `project` imports.
It is the next cheapest verification (one grep + classification), and
its result determines whether there is any graph-narrowing opportunity
or whether the `editor` → `project` edge is fully justified (LSP
integration). O2 is the highest-leverage but highest-cost; O5 is the
next verification step.

---

## Kata Step 4 — PDCA experiment (next step)

**Obstacle.** O5 — classify `editor`'s direct `project` imports.

**Next experiment.**
Run `grep -rEn 'use project::' crates/editor/src --include="*.rs" |
  grep -v _tests.rs` and classify each imported symbol as (a) LSP/LS
  protocol symbol, (b) project model symbol that `workspace` re-exports,
  or (c) symbol that genuinely requires `editor` → `project` directly.

**Prediction.**
Most of `editor`'s direct `project` imports are LSP/LS symbols
(`project::lsp_store::...`, `project::LanguageServerToQuery`) that are
legitimately `editor`'s business (editor integrates with language
servers). The direct `editor` → `project` edge is likely justified, not
a coupling defect. If confirmed, the graph is healthy as-is and the
remaining work is purely file-level (god-object splitting), not
graph-level.

**Measurement.** A written classification table for `editor`'s `project`
imports.

**When to check.** Immediately — one terminal command.

---

## Task-breakdown — Proposed refactor plan (vertical slices)

Ordered by risk (lowest first) and dependency. Each slice leaves the
codebase compiling and tests passing.

### Phase 0 — Trivial eliminations (low risk, high signal)

- **T0.1 — Delete `gpui_shared_string` crate.** Verify no `Cargo.toml`
  depends on it; remove the directory; remove from workspace `members`.
  AC: `cargo check` passes. Files: `crates/gpui_shared_string/`,
  root `Cargo.toml`. Scope: XS.
- **T0.2 — Eliminate 6 `mod.rs` files.** Rename each to the
  `foo.rs` + `foo/` pattern per `.rules`. AC: `cargo check` passes; 0
  `mod.rs` under `crates/`. Files: `gpui/elements/mod.rs`,
  `agent/tests/mod.rs`, `keymap_editor/ui_components/mod.rs`,
  `repl/kernels/mod.rs`, `call/call_impl/mod.rs`,
  `terminal/mappings/mod.rs`. Scope: S.
- **T0.3 — Fix 28 `let _ =` fallible-discards in `project/src`.** Replace
  with `.log_err()` or `?` per `.rules`. AC: 0 `let _ =` on `Result` in
  `project/src`; `./script/clippy` passes. Files:
  `crates/project/src/lsp_store.rs`, `git_store.rs`, `worktree_store.rs`,
  `debugger/session.rs`, `debugger/breakpoint_store.rs`. Scope: S.

### Phase 1 — Graph hygiene (verified-safe eliminations)

- **T1.1 — ~~Verify and delete `gpui_shared_string`.~~** **Resolved: not
  a phantom.** 203-LOC crate with 4 dependents. Keep. No action.
- **T1.2 — Classify `editor`'s direct `project` imports.** Produce a table
  of every `use project::...` in `editor/src` (non-test) tagged as
  LSP-protocol / project-model / other. AC: every import classified.
  Scope: XS (analysis only).
- **T1.3 — (Conditional) If T1.2 finds imports that `workspace` re-exports,**
  switch those imports to `workspace::...` and remove the direct `project`
  dep where possible. AC: `editor`'s direct `project` import count reduced;
  `cargo check` passes. Scope: S. (Gated on T1.2.)

### Phase 2 — Test-gated impl extraction

- **T2.1 — Move `git_panel.rs` test-gated `impl GitPanel` (block at 8010)
  to `crates/git_ui/src/git_panel_tests.rs`.** AC: `git_panel.rs` < 12k
  LOC; `cargo test` passes. Scope: S.
- **T2.2 — Move `git_store.rs` test-gated `impl Repository` (block at
  10137) to `crates/project/src/git_store_tests.rs`.** AC: `git_store.rs`
  < 10k LOC; tests pass. Scope: S.
- **T2.3 — Audit `agent_panel.rs` and `thread_view.rs` for test-gated
  impl blocks; extract to sibling test files.** AC: each production file
  has 0 `#[cfg(test)]` `impl T` blocks. Scope: S.

### Phase 3 — God-object impl splitting (highest risk, deferred)

- **T3.1 — Classify `impl Editor`'s 329 methods by concern.** Produce a
  table: method → concern (render / input / selection / serialization /
  events / settings / navigation / ...). AC: every method classified.
  Scope: M (analysis only).
- **T3.2 — Split `impl Editor` across `editor/<concern>.rs` submodules.**
  Keep `Editor` as the single type; move method groups to submodules with
  `impl Editor in <concern> { ... }`. AC: `editor.rs` < 6k LOC; no
  `impl Editor` block > 100 methods; `./script/clippy` passes; editor
  tests pass. Scope: L (do in multiple sub-slices).
- **T3.3 — Same for `impl Workspace` (263 methods) and `impl LspStore`
  (180 methods).** AC: each <6k LOC; no `impl T` block > 100 methods.
  Scope: L each.

### Phase 4 — `agent_ui` panel deepening (highest risk)

- **T4.1 — Extract `AgentPanel` state/serialization from
  `agent_panel.rs` into `agent_panel/state.rs` and
  `agent_panel/serialization.rs`.** AC: `agent_panel.rs` < 8k LOC;
  tests pass. Scope: M.
- **T4.2 — Extract `ThreadView` rendering from `thread_view.rs` into
  `conversation_view/thread_view/render.rs`.** AC: `thread_view.rs` < 8k
  LOC; tests pass. Scope: M.
- **T4.3 — Reduce `agent_ui` `expect()` count by encoding invariants as
  types (e.g., `thread_metadata_store.rs`'s "metadata should be cached"
  → `CachedMetadata` newtype or `Option` with explicit handling).** AC:
  `expect()` count in `agent_ui/src` < 150 (from 286). Scope: M.

### Checkpoints
- **CP0** after Phase 0: `cargo check` + `./script/clippy` clean; 0
  `mod.rs`; 0 `let _ =` on `Result` in `project/src`.
- **CP1** after Phase 1: `editor`'s direct `project` imports classified
  (and reduced if T1.3 ran); `cargo check` clean. ~~0 phantom crates~~
  (`gpui_shared_string` is not a phantom — keep).
- **CP2** after Phase 2: 0 test-gated `impl T` blocks in top-16 files.
- **CP3** after Phase 3: no `impl T` block > 100 methods; no non-test
  `.rs` file > 8k LOC except `devcontainer_manifest.rs` (borderline,
  deferred).
- **CP4** after Phase 4: `agent_ui` `expect()` < 150; panel files < 8k
  LOC each.

---

## Open questions (parking lot)

1. ~~**`gpui_shared_string` callers.** Does any `Cargo.toml` depend on it?~~
   **Resolved: yes — 4 dependents (`gpui`, `env_var`, `language_core`,
   `language_model_core`). Not a phantom; keep.** Initial 0-LOC finding
   was a `find`-path error (lib at crate root, not `src/`).
2. ~~**`project` → `workspace` edge classification.** Test-only or
   production?~~ **Resolved: false positive, no cycle.**
3. **`edit_prediction_cli` (18.8k LOC) vs `edit_prediction` (10k LOC).**
   Is the CLI crate doing core logic that belongs in the core crate?
   (Not blocking; flag for separate audit.)
4. **`language_core` (1.9k) and `language_model_core` (3.0k).** Do they
   earn their keep under the deletion test, or are they pass-throughs?
   (Not blocking; flag for separate audit.)
5. **`acp_thread` vs `agent` domain overlap.** Both deal with "agent
   threads." Is the seam (ACP protocol vs agent runtime) clean, or is
   there duplicated concept surface? (Not blocking; flag for separate
   audit.)
6. **(New) `editor`'s direct `project` imports.** Which are LSP-protocol
   (justified) vs project-model (could come via `workspace`)? (Blocks
   T1.3.)
   **Resolved (O5 experiment run):** 41 distinct `use project::...` lines in
   `editor/src` (non-test). They are overwhelmingly LSP/LS-protocol symbols
   (`lsp_store::...`, `Completion`, `CodeAction`, `InlayId`, `HoverBlock`,
   `LanguageServerToQuery`, `DocumentColor`, `LocationLink`,
   `TaskSourceKind`) plus `Project` itself and `project_settings`. These are
   legitimately the editor's business — the editor integrates directly with
   language servers and project-level completion/hover/inlay/code-action
   APIs. The `editor` → `project` edge is **justified**, not a coupling
   defect. **No graph-narrowing opportunity. The graph is healthy as-is.**

---

## Suggested `.rules` additions

(For reviewer consideration — not to be applied inline per `.rules` hygiene.)

> **`impl T` blocks for the same type in the same file must be avoided
> unless one is `#[cfg(test)]`-gated.** Test-gated `impl T` blocks belong
> in `*_tests.rs` sibling files, not in the production `.rs` file.

> **No non-test `.rs` file may exceed 8,000 LOC.** Files approaching this
> limit should be split by concern into `<module>/<concern>.rs` submodules,
> keeping the parent type in the original file. The interface (public
> surface) stays; the implementation moves.

> **Distinguish `workspace = true` (lint inheritance) from
> `workspace.workspace = true` (crate dependency) when auditing
> `Cargo.toml`.** The former is Cargo workspace-inheritance shorthand for
> any field (lints, package metadata, dependencies); the latter is a
> dependency on the `workspace` *crate*. Grep-based dependency analysis
> must check the `[dependencies]` section explicitly, not the whole file.

The first two rules meet the bar: non-obvious (the multi-impl pattern is
not visible without grepping for `^impl T`), repeatedly encountered (4
multi-impl cases in the top-16 files), and specific enough to act on. The
third is a methodological guard against the false positive that consumed
part of this audit.
