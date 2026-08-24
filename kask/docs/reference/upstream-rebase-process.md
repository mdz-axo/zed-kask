---
title: "Upstream Rebase Management Process — zed-kask"
audience: [architects, integrators, release engineers]
last_updated: 2026-08-24
version: "1.1.0"
status: "Active"
domain: "Lifecycle"
mds_categories: [lifecycle, composition]
---

# Upstream Rebase Management Process — zed-kask

**Purpose:** a repeatable, auditable process for bringing zed-kask up to upstream
Zed's `main` while preserving the fork's functional kask-wiring changes *without*
carrying forward accumulated cruft (use-before-def bugs, duplicate clones,
stale refactor artifacts). Produced 2026-08-06 after the first full-cycle rebase
(merge of `b5796233bd`) revealed that a naive git merge inherits the fork's
evolutionary debt; a mapped re-application onto clean upstream is the disciplined
alternative.

**Scope:** applies to any D-seam file where the fork has accumulated significant
divergence (currently `crates/zed/src/main.rs` is the worst case: 4115 lines vs
upstream's 2022, with only 4 `// zed-kask:` markers for 111 kask call sites).
Files that auto-merge cleanly with all markers preserved (`markdown.rs`,
`git_graph.rs`, `agent_panel.rs`, etc.) do not need this process.

---

## 1. The three strategies and when to use each

| Strategy | When to use | Risk | Effort |
|---|---|---|---|
| **Git merge** (preserve fork's evolved file) | Fork's file is well-marked (every deviation has `// zed-kask:` + test) and compiles cleanly | Low — single conflict-resolution pass | Low |
| **Mapped re-application** (this process) | Fork's file is under-marked, has accumulated cruft, or has compile bugs | Medium — must map every functional unit | High |
| **Destroy and rebuild** | Never | Catastrophic — loses semantic intent | High |

**Decision rule:** if the fork's file has > 2× the upstream line count, or
< 50% of kask call sites carry `// zed-kask:` markers, use mapped re-application.
Otherwise use git merge.

For `crates/zed/src/main.rs` on 2026-08-06: 4115 vs 2022 lines (2.03×), 4/111
markers (3.6%) → **mapped re-application**.

---

## 2. The mapped re-application process (7 steps)

### Step 1 — Establish the functional inventory (code-graph extraction)

Extract every kask-wiring functional unit from the fork's file. A *functional
unit* is a contiguous block that implements one kask capability (e.g., "wire the
cybernetics loop", "register kask MCP servers"). Use `grep` and
`git diff upstream/main HEAD -- <file>` to extract section headers and kask symbols.

Output: a numbered list of functional units (F1, F2, …) with line ranges and
one-sentence purpose. See §4 for the `main.rs` inventory (28 units).

### Step 2 — Classify each unit by constraint force (semantic-mode audit)

Classify every functional unit by pragmatic-semantics constraint force:

- **Prohibition** — must be re-applied or the system breaks (e.g., a load-bearing
  hook: without it, skill execution runs blind). These are the load-bearing wirings.
- **Guardrail** — should be re-applied; omitting degrades behavior but doesn't
  break (e.g., algedonic threshold: without it, the setting is dead config).
- **Guideline** — nice-to-have; omitting is a regression but not a failure
  (e.g., kask extensions panel wiring).
- **Evidence** — diagnostic/observability (e.g., `log::info!("CyberneticsLoop
  tick cycle started")`).
- **Hypothesis** — speculative/future-facing (e.g., a hook wired for a
  not-yet-landed consumer).

Output: a table mapping each functional unit to its constraint force + the
enforcement point (the line that realizes it) + the pinning test (or "not yet
pinned" — which must be fixed before the re-application is complete).

### Step 3 — Build the dependency graph (ordering constraints)

For each functional unit, identify what it *defines* (variables, hooks, entities)
and what it *uses* (references defined by earlier units). This produces a DAG.
The re-application order must be a topological sort of this DAG — this is where
the fork's use-before-def bugs come from: the fork's incremental evolution
violated the DAG by inserting a use before its definition.

Output: a dependency table (unit → defines → uses → must-come-after).

### Step 4 — Map insertion points in clean upstream

For each functional unit, identify the insertion point in clean upstream's
file — the landmark line after which the unit should be inserted (e.g., "after
`settings::init(cx)`", "after `copilot_chat::init(...)`"). This requires
reading upstream's structure and understanding what each unit depends on from
upstream (e.g., `cx`, `app_state`, `client`, `fs`).

Output: a table mapping each unit to its upstream insertion landmark.

### Step 5 — Re-apply (manual editing)

Take clean upstream's file and insert each functional unit at its mapped
insertion point, in topological order. For each insertion:
- Add a `// zed-kask: D<N>` marker pointing to the DIVERGENCE.md row.
- Ensure the unit's `let` bindings are placed before any use.
- Ensure no duplicate definitions (the fork's duplicate `cybernetics_loop_for_tick`
  bug came from inserting the same binding twice across two edits).

### Step 6 — Pin every deviation with a test

Per the `.rules` trap "Tests must pin deliberate zed-kask deviations from
upstream": every `// zed-kask:` marker must have a corresponding test asserting
the wired behavior. For `main.rs` wirings (which are process-global hooks, not
unit-testable functions), the pinning test is typically:
- A test asserting the hook is `Some` after init (e.g.,
  `assert!(agent::memory_port().is_some())`).
- A test asserting the wired behavior fires (e.g., the cybernetics loop tick
  populates the regulation ledger).
- A compile-time pin: the `set_*` hook signature requires a type from a kask
  crate, so removing the wiring breaks compilation.

Output: a test file (or additions to an existing test module) with one test per
Prohibition/Guardrail unit.

### Step 7 — Update DIVERGENCE.md

Update the D-seam row in `DIVERGENCE.md` to reflect the re-applied file:
- List the file in the D-row's file list.
- Document every functional unit's constraint force and pinning test.
- If the re-application introduced a new D-seam (e.g., a new hook), add a D-row.

### Step 8 — Run post-rebase cleanup

The upstream-rebase skill's step 7 (cleanup) automatically handles this.
 It re-deletes upstream Zed files that git restored from upstream's tree
 (icon files, .desktop templates, flatpak/snap packaging, release workflows)
 and runs the isolation test to verify the collision surface is closed.
 Skipping this step leaves upstream icon files on disk, recreating the
 collision surface that caused zed-kask to hijack Zed's desktop identity
 (commit 853542beab).

---

## 3. Verification gate (before committing the re-application)

1. `cargo check -p <crate>` — the file compiles.
2. `cargo test -p <crate> -- <pinning tests>` — all pinning tests pass.
3. `bash kask/scripts/check-hkask-no-zed-deps.sh` — §13.1 invariant holds.
4. `grep -c "// zed-kask:" <file>` — marker count matches the functional unit
   count (every unit is marked).
5. `git diff upstream/main -- <file>` — the diff is *only* kask additions (no
   upstream code modified outside the D-seam).

---

## 4. Functional inventory for `crates/zed/src/main.rs` (2026-08-06)

28 functional units extracted from the fork's `main.rs` (4115 lines) vs
upstream's (2022 lines). Classified by constraint force; ordering constraints
noted.

### 4.1 Inventory table

| ID | Lines | Purpose | Force | Defines | Uses | Must-come-after |
|---|---|---|---|---|---|---|
| F1 | — | **REMOVED** — `.env` loading via dotenvy has been removed. API keys are configured via the settings UI (keychain) or shell env vars. The keychain is the single source of truth. | — | — | — | — |
| F2 | 617 | `gpui_tokio` init from kask tokio runtime handle | Prohibition | `kask_runtime_handle` | — | after `app` built, before any `Tokio::spawn` |
| F3 | 677 | Alert channel: CyberneticsLoop → MetacognitionLoop | Prohibition | `alert_tx`, `alert_rx`, `regulation_ledger`, `event_sink` | `kask_runtime_handle` | F2 |
| F4 | 700 | Algedonic threshold → `variety_max_deficit` wiring | Guardrail | `set_points`, `cybernetics_loop_inner` | `kask_settings_for_mcp` (F9), `regulation_ledger` (F3) | F3, F9 |
| F5 | 725 | `swarm-panel` call-cap persona (call cap seed) | Prohibition | (call cap registration) | `cybernetics_loop_inner` (F4) | F4 |
| F6 | 750 | CyberneticsLoop + MetacognitionLoop tick cycles (`Tokio::spawn`) | Prohibition | `cybernetics_loop`, `cybernetics_loop_for_tick`, `cybernetics_loop_for_panel`, `mcp_runtime`, `metacognition_loop` | F2, F3, F4, F5 | F4, F5 |
| F7 | 804 | Metacognition provider hook (`set_metacognition_provider`) | Prohibition | (hook set) | `metacognition_loop` (F6) | F6 |
| F8 | 835 | Global `Fs` registration (`<dyn Fs>::global`) | Prohibition | (global set) | `fs` (upstream) | after `fs` defined |
| F9 | 845 | `kask_settings_for_mcp` + MCP server launch list | Prohibition | `kask_settings_for_mcp` | `KaskSettings::get_global` | after `settings::init` |
| F10 | 851 | `curator.always_on` gating of tick cycles | Guardrail | (tick spawn conditional) | `kask_settings_for_mcp` (F9), `cybernetics_loop_for_tick` (F6), `metacognition_loop_for_tick` (F6) | F6, F9 |
| F11 | — | **REMOVED** — `ensure_openai_compatible_entries` has been removed. Providers are registered via zed's native Settings → AI → LLM Providers. | — | — | — | — |
| F12 | — | **REMOVED** — `openai_compatible` re-sync observer has been removed with `ensure_openai_compatible_entries`. | — | — | — | — |
| F13 | 1004 | `sync_kask_mcp_servers` (ContextServerStore registration) | Prohibition | (descriptors registered) | `kask_settings_for_mcp` (F9), `resolve_mcp_binary` (F22), `kask_server_env` (F23) | F9, F22, F23 |
| F14 | 1182 | Embedding credentials resolution (deferred task) | Prohibition | (credentials bound) | `kask_settings` (deferred), `gpui_tokio` (F2) | F2, in deferred task |
| F15 | 1337 | MCP re-sync (curator server, deferred) | Guardrail | (re-sync call) | F13 | F13, in deferred task |
| F16 | 1396 | `LazyToolRouter` hook (`set_tool_router`) | Prohibition | (hook set) | `kask_settings` (deferred) | in deferred task |
| F17 | 1559 | kask extensions panel wiring | Guideline | (panel registered) | upstream panel infra | after panel infra init |
| F18 | 1582 | Collab binary path resolution (dev) | Guideline | (path resolved) | — | early, before collab init |
| F19 | 1919 | MCP re-sync (inference socket, deferred) | Prohibition | (re-sync call) | F13, `INFERENCE_SOCKET_PATH` | F13, in deferred task |
| F20 | 2211 | Deferred task (model-dependent hooks: `set_memory_port`, `set_thread_condenser`, `set_tool_invoker`, `set_context_injector`, `set_curator_context_injector`) | Prohibition | (all model-dependent hooks) | `LanguageModelRegistry::default_model`, F2 | after user resolves, F2 |
| F21 | 2496 | `sync_kask_mcp_servers` fn definition | Prohibition | (fn) | F22, F23, F24 | — (fn definition, hoisted) |
| F22 | 2501 | `resolve_mcp_binary` fn definition | Prohibition | (fn) | `HKASK_MCP_*_BIN` env | — (fn definition, hoisted) |
| F23 | 2563 | `kask_server_env` (env var resolution for MCP servers) | Prohibition | (fn) | `KaskSettings`, credentials | — (fn definition, hoisted) |
| F24 | 2618 | `sync_kask_mcp_servers` impl (descriptor registration) | Prohibition | (fn body) | F22, F23, `KaskMcpDescriptor` | F22, F23 |
| F25 | 2701 | `sync_kask_mcp_runtime_servers` (governed McpRuntime restart) | Prohibition | (fn) | `McpRuntime`, `kask_server_env` (F23) | F23 |
| F26 | 2706 | `tool_invoker` hook (`set_tool_invoker`) | Prohibition | (hook set) | `swarm_panel::ToolInvoker` | in deferred task (F20) |
| F27 | 2808 | `tool_invoke` IPC (inference IPC server) | Prohibition | (IPC methods) | `ToolPort` | F20 |
| F28 | 2810 | Skill executor resolution (resolves at call time) | Prohibition | (resolver fn) | (F20) | F20 |

### 4.2 Dependency DAG (topological order for re-application)

```
F1 (.env)           → before settings::init
F2 (gpui_tokio)     → after app built
F18 (collab path)   → early (before collab init)
F22 (resolve_mcp_binary) → fn (hoisted)
F23 (kask_server_env)     → fn (hoisted)
F21 (sync_kask_mcp_servers fn) → fn (hoisted, depends on F22/F23)
F24 (sync_kask_mcp_servers impl) → fn (hoisted, depends on F22/F23)
F25 (sync_kask_mcp_runtime_servers) → fn (hoisted, depends on F23)
F3 (alert channel)  → after F2
F9 (kask_settings_for_mcp) → after settings::init
F4 (algedonic)      → after F3, F9
F5 (swarm-panel cap) → after F4
F6 (cybernetics loop + mcp_runtime) → after F2, F3, F4, F5
F7 (metacognition provider) → after F6
F8 (global Fs)      → after fs defined
F10 (curator.always_on gating) → after F6, F9
F11 (ensure_openai_compatible) → after F9, before language_models::init
F12 (openai_compatible re-sync) → after F11
F13 (sync_kask_mcp_servers call) → after F9, F22, F23
F17 (kask extensions panel) → after panel infra
F20 (deferred task) → after user resolves, F2
  F14 (embedding creds) → in F20
  F15 (MCP re-sync curator) → in F20, after F13
  F16 (LazyToolRouter) → in F20
  F19 (MCP re-sync inference socket) → in F20, after F13
  F26 (tool_invoker) → in F20
  F27 (tool_invoke IPC) → in F20
  F28 (skill executor resolver) → in F20
```

### 4.3 The fork's 2 bugs, explained by the DAG

1. **`kask_settings_for_mcp` use-before-def (F4 uses F9, but F4 was placed at
   L700 and F9 at L849).** The DAG says F4 must come after F9. The fork's
   incremental evolution inserted F4 (algedonic wiring) before F9 (settings
   load) because F4 was added in a later commit (`6e7bf4fa0e`) without moving
   F9 up. **Re-application fixes this by placing F9 before F4 per the DAG.**

2. **`cybernetics_loop_for_tick` duplicate (F6 defines it at L742, then a
   duplicate `let` at L769).** The fork added the tick-cycle spawn block (F10)
   in `6e7bf4fa0e` and re-cloned the binding instead of reusing the F6 clone.
   **Re-application fixes this by defining once in F6 and reusing in F10.**

### 4.4 Upstream insertion landmarks

| Unit | Insert after (upstream landmark) | Upstream line |
|---|---|---|
| F1 | after `app` built, before `settings::init` | ~L343 (`build_application()`) |
| F2 | after `app` built, before any tokio use | ~L343 |
| F18 | early, before collab init | ~L400 |
| F22–F25 | end of file (fn definitions, hoisted) | EOF |
| F3 | after F2, before `settings::init` | ~L490 |
| F9 | after `settings::init` | ~L498 |
| F4 | after F3, F9 | after F9 |
| F5 | after F4 | after F4 |
| F6 | after F2, F3, F4, F5 | after F5 |
| F7 | after F6 | after F6 |
| F8 | after `fs` defined | ~L435 |
| F10 | after F6, F9 | after F9 |
| F11 | after F9, before `language_models::init` | ~L698 (before `language_models::init`) |
| F12 | after F11 | after F11 |
| F13 | after F9, F22, F23 | after F12 |
| F17 | after panel infra | ~L700+ |
| F20 | in the deferred task (after user resolves) | upstream's deferred task |

---

## 5. Semantic-mode audit findings (constraint-force classification)

### 5.1 Prohibition-force units (must re-apply — 20 units)

These are load-bearing: removing any breaks compilation or core kask behavior.
- F1 (.env), F2 (gpui_tokio), F3 (alert channel), F6 (cybernetics loop + mcp_runtime),
  F7 (metacognition provider), F8 (global Fs), F9 (kask_settings_for_mcp),
  F11 (ensure_openai_compatible), F13 (sync_kask_mcp_servers), F14 (embedding creds),
  F16 (LazyToolRouter), F19 (MCP re-sync inference socket), F20 (deferred task),
  F21–F25 (fn definitions), F26 (tool_invoker), F27 (tool_invoke IPC), F28 (skill executor resolver).

### 5.2 Guardrail-force units (should re-apply — 4 units)

Removing degrades behavior but doesn't break:
- F4 (algedonic threshold — setting becomes dead config),
- F5 (swarm-panel cap — governed dispatch unbounded without it, but only for swarm),
- F10 (curator.always_on gating — tick cycles always run),
- F12 (openai_compatible re-sync — provider toggles need restart),
- F15 (MCP re-sync curator — curator server stale on settings change).

### 5.3 Guideline-force units (nice-to-have — 2 units)

- F17 (kask extensions panel — marketplace UI),
- F18 (collab binary path — dev-only convenience).

### 5.4 Evidence-force units (observability — distributed across units)

`log::info!` / `log::warn!` calls embedded in F2, F6, F10, F20. Re-apply with
their parent unit.

### 5.5 Pinning test gaps (the under-marking problem)

The fork's `main.rs` has 111 kask call sites but only 4 `// zed-kask:` markers.
Of the 28 functional units, **0 have a dedicated pinning test** — the wirings
are process-global hooks that the fork never tested in isolation. The
re-application must add pinning tests for at least the 20 Prohibition units.
Candidate test approach: a `#[test]` in `crates/zed/src/main.rs`'s test module
that calls a `#[cfg(test)] fn kask_wiring_smoke_check()` asserting every
`set_*` hook is `Some` after a test init.

---

## 6. Re-application execution plan for `main.rs`

This is the concrete task list for re-applying `main.rs` onto clean upstream.
Each task is a vertical slice with acceptance criteria.

### Task R1: Branch + clean upstream base
- Branch from `sync/upstream-2026-08-06` as `sync/mainrs-reapply-2026-08-06`.
- `git checkout upstream/main -- crates/zed/src/main.rs` to get clean upstream.
- Acceptance: `git diff upstream/main -- crates/zed/src/main.rs` is empty.

### Task R2: Insert fn definitions (F22, F23, F21, F24, F25) at EOF
- These are hoisted fn definitions; insert at end of file.
- Add `// zed-kask: D3/D8` markers.
- Acceptance: `cargo check -p zed` compiles (fns are unused but defined).

### Task R3: Insert early wirings (F1, F2, F18) before `settings::init`
- F1 (.env) after `build_application()`, F2 (gpui_tokio) after F1, F18 (collab path) early.
- Acceptance: `cargo check -p zed` compiles.

### Task R4: Insert F3 (alert channel) + F9 (kask_settings_for_mcp) after F2/settings::init
- F3 after F2, F9 after `settings::init`. **F9 before F4** (DAG order — fixes bug 1).
- Acceptance: `cargo check -p zed` compiles; `kask_settings_for_mcp` in scope for F4.

### Task R5: Insert F4, F5, F6, F7 (cybernetics loop stack) in DAG order
- F4 (algedonic) after F3+F9, F5 (cap) after F4, F6 (loop+mcp_runtime) after F2+F3+F4+F5, F7 after F6.
- **F6 defines `cybernetics_loop_for_tick` once; F10 reuses it** (fixes bug 2).
- Acceptance: `cargo check -p zed` compiles.

### Task R6: Insert F8, F10, F11, F12, F13, F17
- F8 after `fs`, F10 after F6+F9, F11 before `language_models::init`, F12 after F11, F13 after F9+F22+F23, F17 after panel infra.
- Acceptance: `cargo check -p zed` compiles.

### Task R7: Insert F20 (deferred task) with F14, F15, F16, F19, F26, F27, F28
- F20 is the deferred task block; insert all sub-units in DAG order inside it.
- Acceptance: `cargo check -p zed` compiles; `cargo test -p zed -- kask_wiring_smoke_check` passes.

### Task R8: Add pinning tests for all Prohibition units
- Add a `kask_wiring_smoke_check` test asserting every `set_*` hook is wired.
- Add per-unit tests where feasible (e.g., cybernetics loop tick populates ledger).
- Acceptance: `cargo test -p zed -- kask` passes; every `// zed-kask:` marker has a test.

### Task R9: Update DIVERGENCE.md D3/D8 rows
- Document the re-applied `main.rs` with the full functional unit list + pinning tests.
- Acceptance: DIVERGENCE.md D-row file lists match the post-re-application tree.

### Task R10: Verify + commit
- `cargo check -p zed`, `cargo test -p zed`, `bash kask/scripts/check-hkask-no-zed-deps.sh`.
- `grep -c "// zed-kask:" crates/zed/src/main.rs` ≥ 28 (one per functional unit).
- Commit on `sync/mainrs-reapply-2026-08-06`.

---

## 7. Generalization: when to invoke this process

Invoke this process (and the future `upstream-rebase` skill) when:
1. A git merge of upstream leaves a D-seam file with conflicts that touch
   kask-wiring regions.
2. A D-seam file's `// zed-kask:` marker density is < 50% of its kask call sites.
3. A D-seam file has known compile bugs from incremental fork evolution.
4. After any upstream merge, as an audit step: re-run the functional inventory
   and confirm marker density + pinning test coverage.

The process is **not** needed for:
- Files under `kask/` (upstream never touches them).
- D-seam files that auto-merge cleanly with all markers preserved.
- Additive-only crates (`hkask-*`, `swarm_panel`, `kanban_panel`, `hkask-*-widget`).

---

## 8. Proposed skill: `upstream-rebase`

A skill that encodes this process for reuse. Sketch:

- **Name:** `upstream-rebase`
- **Purpose:** Manage upstream Zed rebases for zed-kask: decide strategy per
  D-seam file (merge vs. mapped re-application), execute the chosen strategy,
  pin every deviation with a test, update DIVERGENCE.md.
- **Composes:** `essentialist` (deletion test for cruft detection),
  `coding-guidelines` (surgical re-application), `task-breakdown` (slice the
  re-application into vertical tasks).
- **Phases:** Assess (per-file strategy decision) → Map (functional inventory
  + DAG) → Re-apply (topological insertion) → Pin (tests) → Verify → Document.
- **Emits:** `reg.upstream_rebase.*` spans.

This skill would be authored via the `create-skill` skill (which produces the
skill structure: `SKILL.md` + `.j2` templates).
The process document above becomes the `SKILL.md` companion content.
