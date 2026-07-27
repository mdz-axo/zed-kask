# Infrastructure Jinja2 Template & YAML Manifest Audit — Checklist

## Pending Tasks

- [x] **T-KASK-ICON: Replace app icon with burnt orange zed-kask icon**
  - Generated a burnt orange K icon via Z-Image-Turbo, converted to PNG
    (512x512 + 1024x1024), and replaced all 4 app-icon files in
    `crates/zed/resources/` (app-icon.png, app-icon-dev.png, @2x variants).
  - The icon pipeline (build.rs → zed.rs → bundle-linux) picks up the
    PNGs automatically — no code changes needed.
  - Verified: the PNG has burnt orange pixels (RGB 206, 105, 41).
  - The status bar / window title bar / desktop icon currently shows the
    standard zed icon (green/blue). It should show the burnt orange
    zed-kask icon to visually distinguish the fork.
  - Icon pipeline (Linux):
    1. `crates/zed/build.rs:icon_path()` selects `resources/app-icon{channel}.png`
    2. `build.rs:prepare_app_icon_x11()` resizes to `OUT_DIR/app_icon.png`
    3. `crates/zed/src/zed.rs:380` includes `OUT_DIR/app_icon.png` as the
       X11 window icon (used by the status bar / title bar)
    4. `crates/zed/src/zed.rs:1473` uses `resources/app-icon-dev.png` for
       the About window
    5. `script/bundle-linux:170` copies `resources/app-icon.png` to the
       desktop icon directory (`share/icons/hicolor/`)
  - Fix: replace `crates/zed/resources/app-icon.png` and
    `app-icon-dev.png` (and `@2x` variants) with the burnt orange kask
    icon. Generate from `kask/assets/kask-logo.svg` or a dedicated
    app-icon SVG with the burnt orange background.
  - Also check `crates/gpui_linux/src/linux/platform.rs` for Wayland
    window icon setup (may need a separate code path).

- [x] **T-INFERENCE-SYNTAX: Migrate from old hKask prefix syntax to zed model syntax**
  - Old syntax: `DI/model`, `FA/model`, `TG/model`, `OR/model`, `KC/model`
  - Zed syntax: `provider_id/model_name` where provider_id is the JSON key
    from `openai_compatible` in settings.json (e.g., `DeepInfra/model`,
    `OpenRouter/model`, `Together AI/model`)
  - OpenRouter should be the default provider for zed-kask / curator agent
  - Sites to update:
    - `kask/crates/hkask-inference/src/config.rs` — `ProviderId` enum,
      `parse_from_model`, `prefix_model`, `looks_like_prefix`
    - `kask/crates/hkask-inference/src/model_constants.rs` — all default
      model constants
    - `kask/crates/hkask-types/src/fusion.rs` — `kask_default()` defaults
    - `kask/crates/hkask-services-core/src/settings.rs` — embedding default
    - `kask/crates/kask_bridge/src/settings.rs` — doc comments + defaults
    - `kask/.env` — all `HKASK_*_MODEL` vars
    - `kask/mcp-servers/hkask-mcp-codegraph/src/hkask_mcp_codegraph.rs` —
      prefix routing
    - `kask/mcp-servers/hkask-mcp-corpus/` — embedding model defaults
    - `kask/mcp-servers/hkask-mcp-training/src/huggingface.rs` — prefix
      stripping
    - `kask/crates/hkask-services-corpus/src/embed/utils.rs` — prefix
      stripping
    - `kask/crates/hkask-inference/src/chat_protocol.rs` — test fixtures
    - `kask/registry/templates/` — any manifests with model references

- [x] **T-ENV-LOADING: Load kask/.env at startup**
  - `kask/.env` has API keys but nothing loads it. Added `dotenvy::from_path`
    in `main.rs` but needs verification that it runs before settings parsing.

- [x] **T-PROVIDER-DEFAULTS: Enable inference providers by default when API key present**
  - Changed `unwrap_or(false)` to `unwrap_or_else(|| env_var_is_set(...))`
    in `settings.rs`. Needs testing.

- [x] **T-SKILL-BODY-INJECTION: Disable SKILL.md body injection when manifest executor is present**
  - When the manifest executor is wired and a skill has no manifest, the
    skill tool now returns a minimal envelope instead of reading and
    injecting the full SKILL.md body. This stops the token burn from
    body injection.
  - File: `crates/agent/src/tools/skill_tool.rs:303-312`

- [x] **T-SETTINGS-FEEDBACK-LOOP: Fix cursor flashing from settings write loop**
  - `update_settings_file_inner` now skips the write and global update
    when `new_text == old_text`. This breaks the feedback loop where
    `ensure_openai_compatible_entries` writes to settings.json, triggers
    SettingsStore observer, which calls `ensure_openai_compatible_entries`
    again.
  - File: `crates/settings/src/settings_store.rs:587-593`

- [ ] **T-TEMPLATE-MODEL-DEPRECATION: Deprecate model names in templates, use config-driven defaults**
  - Templates still hardcode model names (e.g., `DeepInfra/Qwen/Qwen3-Embedding-0.6B`)
    instead of referencing config defaults (e.g., `{{ embedding_model }}`).
  - This is a larger architectural change — templates should reference
    config keys, not specific model strings.
  - Status: old prefixes updated to zed syntax, but deprecation of hardcoded
    model names in favor of config-driven defaults is not yet done.

## Completed Tasks


## Phase 0 — Plan & Inventory (done)

- [x] **T0: Inventory infrastructure `.j2`/`.yaml` artifacts**
  - 0 `.j2` files outside `kask/registry/` (all 335 are skill-registry).
  - 3 in-scope YAMLs: `kask/corpus/replica/company-researcher.yaml`,
    `kask/corpus/replica/john-brooks.yaml`,
    `kask/corpus/pipeline-capabilities-researcher.yaml`.
  - 7 out-of-scope infra YAMLs (no Rust consumer / generator output / ops config).

## Phase 1 — Functional Role Discovery (done)

- [x] **T1: graph-audit (dual mode) — replica YAML cluster**
  - Confirmed `EmbedService::embed_corpus` → `CorpusConfig` parse path.
  - Mapped every `CorpusConfig` field to its YAML source.
  - Classified edges by constraint force; detected 7 Prohibition + 5 Guardrail.
  - Output: `tasks/phase1-functional-roles.md`.
- [x] **T2: graph-audit (dual mode) — pipeline manifest runbook**
  - Re-verified no Rust consumer.
  - Mapped 13 referenced MCP tools to Rust entry points (all match).
  - Output: `tasks/phase1-functional-roles.md`.

**Checkpoint C1**: ✅ consumer code unchanged; functional-role statements produced.

## Phase 2 — Logic & Semantics Audit (done)

- [x] **T3: pragmatic-semantics + pragmatic-cybernetics + essentialist — replica YAMLs**
  - 7 Prohibition, 5 Guardrail, 2 Guideline, 3 Hypothesis.
  - Output: `tasks/phase2-logic-semantics.md`.
- [x] **T4: pragmatic-semantics + pragmatic-cybernetics + essentialist — pipeline manifest**
  - 0 Prohibition, 3 Guardrail, 2 Guideline, 5 Hypothesis (3 resolved as Evidence).
  - Output: `tasks/phase2-logic-semantics.md`.

**Checkpoint C2**: ✅ Prohibition findings promoted to Phase 3.

## Phase 3 — Gap Interrogation (done)

- [x] **T5: sequential-inquiry + grill-me — replica YAMLs**
  - `company-researcher.yaml`: **Gap** (Rationale/Edge-Cases/Synthesis).
  - `john-brooks.yaml`: **Solid** (all 5 rounds).
  - Output: `tasks/phase3-gap-analysis.md`.
- [x] **T6: sequential-inquiry + grill-me — pipeline manifest**
  - Runbook: **Partial** (Recall/Mechanism/Rationale Solid; Edge-Cases/Synthesis Partial).
  - Confirmed `max_tokens: 512` = Rust default; `dedup_threshold: 0.89` diverges from default 0.85.
  - Output: `tasks/phase3-gap-analysis.md`.

**Checkpoint C3**: ✅ BUG-001 promoted to Phase 4.

## Phase 4 — Bug Hunt & Diagnosis (done)

- [x] **T7: bug-hunt expedition — replica YAMLs + `hkask-services-corpus`**
  - Wrote `tests/replica_persona_parse_test.rs` (2 tests).
  - Confirmed BUG-001 (`company-researcher.yaml` parse failure: missing `exemplar_count_min`).
  - Discovered BUG-002 (`john-brooks.yaml` `budget` silent PerPage mismatch).
  - Output: `tasks/phase4-bughunt-report.md`.
- [x] **T8: diagnose — fix `company-researcher.yaml` contract drift**
  - falsifiability: 3 ranked hypotheses; H1 (missing required) confirmed via `[DIAG-0001]`.
  - Fix applied: 8 changes conforming YAML to `CorpusConfig`.
  - Regression test flipped to positive assertion; instrumentation cleaned.
  - Post-mortem written.
- [x] **T9: diagnose — verify `john-brooks.yaml`**
  - Parses; BUG-002 is a contract defect (deferred to Phase 5).

**Checkpoint C4**: ✅ `cargo test -p hkask-services-corpus` 21 passed; `./script/clippy -p hkask-services-corpus` clean.

## Phase 5 — Architectural Refactor (done — decision only)

- [x] **T10: refactor-architecture decision — replica YAML cluster**
  - Candidate: `BudgetConfig` untagged-enum variant reorder.
  - Decision: **DEFER** with ADR (cross-crate, affects 7 out-of-scope registry YAMLs).
  - Other candidates (`deny_unknown_fields`, surface width, runbook `verify:`): **Reject** (scope/effort).
  - Output: `tasks/phase5-refactor-decision.md`.

**Checkpoint C5**: ✅ ADR recorded; no Phase 5 code changes (correct).

## Phase 6 — Convergence & Report (done)

- [x] **T11: convergence check + final report**
  - All slices converged: Slice A (replica) — fixed + tested; Slice B (runbook) — audited, no fix needed.
  - Per-artifact health scores: company-researcher 0.85, john-brooks 0.80, runbook 0.90.
  - Aggregate report: `tasks/audit-report.md`.
  - 6 recommended follow-ups documented.
