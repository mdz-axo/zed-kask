# Architecture Audit 2026-08-26 — Kask Crates, Skills, Templates, MCP Servers

> Merged findings register and implementation plan from two read-only
> `refactor-architecture` audits (ra-explore → ra-candidates → ra-audit).
> Nothing was modified during the audit; this document is the deliverable.
>
> Reference model: `kask/docs/architecture/zed-host-architecture-plan.md`
> (§13 ports-and-adapters, §13.1 dependency invariant), `DIVERGENCE.md`
> (D1–D33), `core/MDS.md`, `core/PRINCIPLES.md`, repo-root `.rules`.
>
> Tests applied to every finding:
> - **Load-bearing** — serves the D-seam contract or a documented reference
>   pattern, or it doesn't and should be flagged.
> - **Canonical** — one canonical implementation per mechanism; duplicates
>   or near-duplicates are findings.
> - **Deletion** — if removing it changes nothing observable, flag it.
> - **Impedance** — boundary mismatch between surfaces (skill↔tool,
>   template↔context, server↔allowlist).
>
> Severity: 🔴 High · 🟠 Medium · 🟡 Low.

> **Amendment (2026-08-26, post-review):** operator review established that
> the dominant root cause behind Surface B/C findings is **incomplete
> multi-leg changes** — a deletion or rename landed on one side while the
> consumer-side update never did. Git-history triage (§2.1) confirmed this
> for every 🔴 skill finding and surfaced a further cluster of vestiges
> (G1–G8). §5.0 adds a triage protocol; Phase 0/3 steps are re-scoped
> accordingly. Finding B1 was **retracted** after verification.

---

## 1. Verdict on the reference model

The core pattern is sound and consistently applied: ports in
`hkask-types`/`hkask-tool-port`, `kask_bridge` as the sole bidirectional
seam, composition root in `main.rs`, `check-hkask-no-zed-deps.sh` passes,
and the MCP-server framework (`hkask-mcp-server`) gives all 10 servers
one canonical shape (`resolve_db_passphrase`, `execute_tool_semantic`,
SSRF validation, span emission). Most friction is **drift between the
documented reference model and the tree**, plus a handful of genuinely
dead or unseeded surfaces — not architectural rot.

---


## 2. Merged findings register

Findings are deduplicated across the two source audits. The `Origin`
column records which audit first surfaced each item; convergent findings
are marked `both`. IDs are stable for plan cross-reference.

### 2.1 Root-cause triage: the incomplete-change inventory (git-verified)

Every "phantom reference" finding was traced through git history to its
originating change. In each case the *removal leg* completed and the
*consumer-update leg* did not. This is the pattern to watch for globally:

| Origin commit | Removed | Consumer leg missed | Live symptom |
|---|---|---|---|
| `9e9c41ef3c` (Aug 20) "Remove unused skills, harness, and verification crate" | skills `harness-optimize`, `proptest`, `eqm-improvement`, `eqm`, `kali-audit`; crates `hkask-test-harness`, `hkask-verification`; registry dir `templates/media/` (incl. all logo templates) | `bug-hunt` still cites `harness-optimize` + deleted `test-harness-trace-schema.md`; `tdd` still dispatches to `harness-optimize`/`proptest`, imports `hkask-test-harness` oracle taxonomy, references missing `./scripts/test --trace`; `superforecasting` step 21 calls `eqm-improvement`; `lora-training` divides authority with `kali-audit`; `logo-builder` references templates whose dir was deleted in the same window | B7, B8, B9-class refs; G1 |
| `6761c23961` (Aug 20) "Replace manifest executor with agent-driven skills" | `kask_bridge/src/skill_executor.rs` incl. `BridgeManifestExecutor::validate_golden_outputs` | `gemba-walk` action enum + `recommend-actions.j2` still offer `validate_golden_outputs` as an approvable action — now a no-op approval loop | B2 (root cause found: executor existed, was deleted, skill not updated) |
| `26215d845e` (Aug 20) "…remove hkask-mcp-codegraph, condenser, and media crates" | media MCP server (~8k lines), codegraph server | `system_prompt.hbs:46-47` still instructs the agent to copy `display_hint`/```media blocks from media-tool results; D18 gate in `markdown.rs:2723` still routes ```media fences; IPC request struct retains 10 write-only `media_*` fields read by nobody; `falsifiability` mentions "codegraph ontological anchoring" | G2–G5 |
| `7d0253ab0d` (Aug 20) | `kask/docs/architecture/test-harness-trace-schema.md` | `bug-hunt/SKILL.md:81` link now dead | B5 (one instance root-caused) |

**Lesson encoded into the plan:** when a removal PR lands, the same PR must
resolve every inbound reference (skill↔skill, skill↔template,
skill↔doc, prompt↔tool, protocol-field↔reader). The new cross-reference
gate (step 0.8) makes the unresolved half fail CI instead of surviving as
zombie prose.

### Media/logo-builder disposition (operator decision recorded)

The media MCP server was **deferred deliberately** (complexity of
multi-media-type handling; strategic focus on text/number/logic domains:
financial, planning, markets, research, document processing). It is
recoverable: `git show 26215d845e^:kask/mcp-servers/hkask-mcp-media/...`
yields ~8k lines including `generate_image`/`describe_image` over the
(still-live) vision path, and `git show 9e9c41ef3c^:kask/registry/templates/media/logo-{discovery-map,formal-prompt}.j2`
yields the two logo templates the skill expects. Disposition:

- **logo-builder**: keep the skill; recompose it against current tools.
  The generation pipeline needs an image backend that no longer exists,
  so until media is revived the skill's template refs are restored
  (recoverable verbatim) but the skill gains a front-matter note naming
  its dependency (`media` server, deferred) — honest about being
  dormant rather than silently broken.
- **media server revival**: out of scope for this audit's plan; recorded
  here so the recovery command and rationale survive. When revived, the
  `media_*` IPC fields (G4) become live again — do not delete them until
  that decision is final.

---


### Surface A — kask crates (`kask/crates/`, 18 crates)

| ID | Sev | Test | Origin | Finding | Evidence |
|---|---|---|---|---|---|
| A1 | 🟠 | Canonical | both | `hkask-event-store` is a workspace member (root `Cargo.toml:285`) with 3 dependents (`hkask-mcp-training/Cargo.toml:22`, `hkask-mcp-swarm/Cargo.toml:21`, `kask_bridge/Cargo.toml:19`) but is absent from the hKask members enumeration in DIVERGENCE.md. The upstream-sync runbook treats that list as complete. | `DIVERGENCE.md:85` vs root `Cargo.toml:285` |
| A2 | 🟠 | Deletion / Load-bearing | other | `DatabaseDriver` trait has one production impl (`SqliteDriver`) and no mock/test driver, yet `hkask-event-store/Cargo.toml:5` advertises it as a swappable abstraction. Per `.rules`, trait-with-one-impl without a test seam is speculative generality. | `kask/crates/hkask-storage/src/database/driver.rs`; `sqlite.rs:223` |
| A3 | 🟡 | Load-bearing | both | `Mutex.lock().unwrap()` ×3 on the inference timeout-rate-limit path; a poisoned mutex cascades into the stream error path. Additional unwrap clusters: `companies/tools/analytics.rs` (~12, mostly post-guard), `swarm/local_registry.rs` (~9 × `lock().unwrap()`). | `kask_bridge/src/inference_chat.rs:416,488,961`; `hkask-mcp-companies/src/tools/analytics.rs`; `hkask-mcp-swarm/src/local_registry.rs` |
| A4 | 🟡 | Load-bearing | mine | ~12 production `let _ =` on fallible ops in `kask_bridge` — mostly best-effort reply sends (defensible), but stale-socket `remove_file` before bind silently changes bind behavior. `.rules` requires `.log_err()`. (Post-review correction: two originally-cited lines, `inference_ipc_server.rs:1850,1858`, are inside `#[cfg(test)]` — dropped.) | `inference_ipc_server.rs:356,358,411,430,437,459,462,467`; `inference_chat.rs:554,559,566`; `inference_embedding.rs:146,188` |
| A5 | 🟡 | Load-bearing | mine | `WorktreeSpawner` trait has one impl (`AgentPanelWorktreeSpawner`) and no test seam — the IPC worktree-spawn path is untestable without GPUI. Justified as §13.1 inversion, but unseamed. | `kask_bridge/src/inference_ipc_server.rs:73`; `crates/zed/src/main.rs:3254` |
| A6 | 🟡 | Canonical | mine | Stale crate name in WAL-invariant doc ("crates that depend on `hkask-database`" — crate is `hkask-storage`). | `hkask-storage/src/database/sqlite.rs:29` |
| A7 | 🔴 | Load-bearing | other | `reg.sensor.memory` is emitted by `RealMemoryPort::h_mem_count` and `low_confidence_count` but is not registered in `CANONICAL_NAMESPACES` (`hkask-types/src/event.rs:75`). The reg-canonical and reg-creep gates currently fail on this namespace. The sensor is real (it feeds the regulation loop); the correct fix is registration, not retargeting. | `kask_bridge/src/memory.rs:997,1012`; `hkask-types/src/event.rs:75` |
| A8 | 🔴 | Impedance | other | `hkask_services_core::ServiceConfig` (+ `from_env`/`from_secrets`/`in_memory`/`open_driver`, ~262 lines) and `load_settings<T>`/`settings_path()` are dead: zero production callers outside their own crate (only doc references in `zed-host-architecture-plan.md:297` and `kask-settings.md:344`, plus an incidental collab-test comment). MDS claims "shared by 6 consumers"; reality: only `HkaskSettings::load` (corpus) and `ServiceError` (corpus) are consumed. | grep across `kask/`, `crates/` |
| A9 | 🟠 | Canonical | other | Stale `DomainKind::{Wallet,Pod}` variants — wallet deleted, pod concept deprecated (D28 renamed `pod.db`→`curator.db`). Corpus errors still classify failures as `DomainKind::Wallet`. | `hkask-services-core/src/error.rs`; `corpus/runtime/classify_impl.rs:110` |
| A10 | 🟡 | Deletion | other | `hkask-bridge-ontology::{sumo, sdmx}` near-dead: referenced only internally by `axis.rs`'s namespace enum round-trip; no server consumes either module directly. | grep |
| A11 | 🟡 | Deletion | other | Swarm `a2a.rs`/`a2a_http.rs`/`a2a_tools.rs` (~835 lines) unverified aliveness after D5 removed "a2a secret threading." One tool (`swarm_a2a_broadcast`) is referenced in an `mcp_tools` comment. Needs a keep-or-delete ruling. | `hkask-mcp-swarm/src/a2a*.rs` |
| A12 | 🟡 | Deletion | other | `KaskScenariosSettings {}` empty struct — placeholder kept only for schema symmetry; harmless but document-or-delete. | `kask_bridge/src/settings.rs:369` |
| A13 | 🟠 | Canonical | other | Dual config systems: `HkaskSettings` (file-based, keystore-tier) coexists with `KaskSettings` (zed settings.json). Only real consumer: corpus classifier-model fallback. Two sources of truth for model defaults violates the "defaults live in `Default` impls / `model_constants`" rule. | `hkask-services-core`, `kask_bridge/src/settings.rs` |
| A14 | 🟠 | Canonical | other | Credential-key naming split: `eodhd`, `fmp`, `runpod`… vs `hkask_abw_api_key`, `hkask_db_passphrase`, `hkask_smtp_password`. RunPod appears in both `INFERENCE_PROVIDERS` and `DATA_SERVICES` with the same key, patched over by a string special-case (`if provider.credential_key == "runpod" { continue }`) in `credential_urls_for_mcp` — should be a descriptor field. | `kask_bridge/src/inference_providers.rs`, `mcp_env.rs` |
| A15 | 🟡 | Canonical | other | Two DB-error taxonomies: `hkask_types::DbError` (generic, moved to break a cycle) vs `hkask_storage::DatabaseError` (rich: `PassphraseMismatch`, `KeyDerivation`…). Justified origin, confusing names; corpus even has a third mapping layer (`map_database_error`). | `hkask-types`, `hkask-storage`, `corpus/src/helpers.rs:53-55` |
| A16 | 🟡 | Canonical | other | Crate-name confusion: `hkask-mcp` (governed runtime) vs `hkask-mcp-server` (framework). Already caused a wrong doc example (`use hkask_mcp::server::…` inside `hkask-mcp-server/src/server.rs:7`). | cited |

### Surface B — Skills (`.agents/skills/`, 64 skills)

| ID | Sev | Test | Origin | Finding | Evidence |
|---|---|---|---|---|---|
| B1 | ~~retracted~~ | — | mine | **RETRACTED post-review.** `market_context` is a template *input name*, not a tool; the skill correctly says "call `market_match` directly" (SKILL.md:48) and all referenced tools (`market_match`, `scenario_calibration`, `rss_search`) exist. Step 0.3 removed. | verification: no "call market_context" phrasing anywhere |
| B2 | 🔴 | Impedance / Load-bearing | mine | **Root-caused (§2.1):** `validate_golden_outputs` had a real executor (`BridgeManifestExecutor::validate_golden_outputs`, `skill_executor.rs:213`) deleted by `6761c23961`; gemba-walk's action enum was never updated. Approving it is a no-op — a broken feedback loop. Disposition per §5.0 triage: **remove from the action enum** (the manifest-executor architecture it served is gone deliberately; agent-driven skills replaced it). | `.agents/skills/gemba-walk/SKILL.md:64,78`; `recommend-actions.j2:23,60,81` |
| B3 | 🔴 | Impedance | both | **Root-caused (§2.1):** logo-builder's templates lived under `templates/media/` (per its own manifest: "no own .j2 … uses media server tools") and were deleted with the media server. Operator decision recorded in §2.1: **recover the two templates verbatim from git** (`9e9c41ef3c^`) and mark the skill dormant pending media revival. | `.agents/skills/logo-builder/SKILL.md:67-82`; recovery: `git show 9e9c41ef3c^:kask/registry/templates/media/logo-discovery-map.j2` |
| B7 | 🔴 | Impedance | review | `bug-hunt` writes traces "visible to the `harness-optimize` skill" and cites the deleted `test-harness-trace-schema.md`; `tdd` dispatches to phantom skills `harness-optimize` + `proptest`, imports the deleted `hkask-test-harness` oracle taxonomy, references missing `./scripts/test --trace`, and emits `reg.contract.violated` — not in `CANONICAL_NAMESPACES`. The advertised bug-hunt→harness-optimize→CI mutation loop terminates in vapor. Root cause: `9e9c41ef3c` + `7d0253ab0d`. | `bug-hunt/SKILL.md:81`; `tdd/SKILL.md:49,55,58,137,153,170,178,189`; `hkask-types/src/event.rs` CANONICAL_NAMESPACES |
| B8 | 🟠 | Impedance | review | `superforecasting` step 21 invokes the `eqm-improvement` skill — deleted by `9e9c41ef3c`, consumer step not updated. | `superforecasting/SKILL.md:151` |
| B9 | 🟡 | Impedance | review | `constraint-forces-recast` + `gradient-seeded-recombination` defer evidence assembly to `web-deep-research` — neither a skill nor an MCP tool. Triage needed: uninstalled dependency vs stale prose. | both SKILL.mds |
| B4 | 🟠 | Deletion | mine | `listening` self-declares `apply-template-rag.j2` as legacy ("registered but NOT referenced by skill execution") — survives only for hypothetical standalone use. | `.agents/skills/listening/SKILL.md:44` |
| B5 | 🟠 | Impedance (doc) | mine | Doc-path drift: `bug-hunt/SKILL.md:81` → `architecture/test-harness-trace-schema.md` does not exist anywhere (dead link); `skill-discovery/SKILL.md:26` misses the `core/` segment; `algedonic-review/SKILL.md:96` + `gemba-walk/SKILL.md:98` resolve to repo-root `docs/`, inconsistent with sibling refs. | cited per item |
| B6 | 🟡 | Deletion | mine | `swarm-intelligence/SKILL.md:58` references a `swarm_panel` UI surface that doesn't exist as a tool or callable surface — stale prose. | cited |

### Surface C — Template registry (`kask/registry/templates/`, 309 `.j2`)

| ID | Sev | Test | Origin | Finding | Evidence |
|---|---|---|---|---|---|
| C1 | 🔴 | Impedance | mine | `strip_frontmatter` only strips when content *starts with* `---`, but 294/317 templates start with `{# … #}` comments or `[inference]` blocks — so YAML contract blocks and `[inference] temperature = …` params render verbatim into prompts for 93% of the corpus. The tool doc comment claims they're stripped. | `crates/agent/src/tools/render_template_tool.rs:189-199` (doc at :20-21); e.g. `kanban-task-management/triage.j2:1` |
| C2 | 🔴 | Impedance | both | Runtime-consumed non-`.j2` registry assets are never embedded/seeded. (a) Training chat templates `qwen3.jinja`/`gemma4.jinja` consumed by `hkask-mcp-training/src/providers/harness.rs:185-209` — fresh installs launch fine-tunes with no `chat_template` (warn fires, job proceeds degraded). (b) Corpus classify YAMLs in `kask/registry/classify/*.yaml` (7 files) consumed via `classify_impl.rs:99` — `registry_dir` is derived positionally from `config_path` (`parent().parent().parent()/registry`), so non-default classifiers fail with `ServiceError::Domain{Wallet, ServiceUnavailable}` in a fresh install. `agent_skills/build.rs:158` collects `.j2` only. | (a) `hkask-mcp-training/src/providers/harness.rs:185-209`; `crates/agent_skills/build.rs:158`. (b) `kask/registry/classify/` (7 YAMLs); `classify_impl.rs:94-99`; `embed/service.rs:113-124` |
| C3 | 🟠 | Canonical | mine | 15 templates carry a second `[inference]` block *after* any strippable frontmatter — renders mid-body even if C1 is fixed. | `listening/apply-template.j2`, `goal-analysis/judge*.j2`, `sankey-flow/sankey-adapt.j2`, `metacognition/ellipsis-analysis.j2`, `create-skill/create-skill-scaffold.j2`, `hypothesis-framer/hypothesis-operationalize.j2`, +5 more |
| C4 | 🟠 | Impedance | mine | Context-shape impedances: `mcda/weight-and-score.j2` requires `decision_question`/`weighting_method`/`criteria[]` — zero mentions in `mcda/SKILL.md`; `listening/apply-template.j2` requires `company_symbol` — undocumented; `goal-analysis/judge.j2` requires `essentialist_report`/`outcome_summary`/`completion_criteria` — undocumented; `lora-training/report.j2` requires `existing_regressions` — undocumented. | template files vs owning SKILL.mds |
| C5 | 🟠 | Canonical | mine | Rule-vs-reality: `skill-maintenance` T4 forbids `[inference]` frontmatter in `.j2` templates (`SKILL.md:62`), yet 302/317 contain it — the catalog fails its own validation gate wholesale, making the gate meaningless. | cited |
| C6 | 🟡 | Deletion / Canonical | mine | Unreferenced templates: `lisp-scaffold-reasoning/{propose-hypotheses,refine-hypotheses}.j2` (only `report.j2` reachable); all four `create-skill/create-skill-{describe,research,scaffold,validate}.j2`; `skill-maintenance/{prose,reverse}.j2`; `docproc/tag-chunks.j2` (single-chunk variant superseded by `tag-chunks-batch`, similarity 0.60). | grep reachability |
| C7 | 🟡 | Deletion | mine | Misfiled non-template assets inside the render base path: `listening/tests/fixtures/*.txt` ×4 (incl. a full 10-K transcript), two READMEs. Harmless to rendering but bloats the directory-of-record. | `kask/registry/templates/listening/tests/fixtures/` |

### Surface D — MCP servers (`kask/mcp-servers/`, 10 crates)

| ID | Sev | Test | Origin | Finding | Evidence |
|---|---|---|---|---|---|
| D1 | 🔴 | Impedance | mine | Env-allowlist impedance, kata-kanban: server reads `HKASK_ABW_MAX_CREDITS`, `HKASK_SWARM_LEDGER_PATH`, `HKASK_LOCAL_AGENTS_DIR` on the spawn path, but its `config_env` allowlist delivers only `HKASK_DATA_DIR`+`HKASK_KANBAN_DB` — governed launches silently drop all three. | reads: `hkask_mcp_kata_kanban.rs:1177,1690,1707`; allowlist: `kask_bridge/src/mcp_servers.rs:198-206` |
| D2 | 🔴 | Impedance | mine | Same pattern, swarm: reads `HKASK_SWARM_EVENTS_PATH`, `HKASK_SWARM_BODY_RETENTION_HOURS`, `HKASK_SWARM_ROLLOUT_RETENTION_DAYS`; none allowlisted — operator overrides silently dropped. | `hkask_mcp_swarm.rs:282`; `local_tools.rs:2194,2212`; allowlist `mcp_servers.rs:280-319` |
| D3 | 🔴 | Impedance | mine | Same pattern, corpus: `HKASK_EMBED_CONCURRENCY` read (`semantic.rs:710`) but absent from corpus `config_env` (`mcp_servers.rs:112-144`), which lists every other corpus knob. | cited |
| D4 | 🟠 | Canonical | both | Training re-implements passphrase resolution inline (`std::env::var("HKASK_DB_PASSPHRASE").unwrap_or_default()`), bypassing the canonical 2-tier chain (`ctx.credentials` → env → keystore keychain). Its own `run()` uses the canonical helper — divergent within one crate. | `hkask-mcp-training/src/tools/dataset.rs:99` vs `hkask-mcp-server/src/server/credentials.rs:65-102` |
| D5 | 🟠 | Canonical | mine | Numeric env parses silently fall back instead of using the framework's `parse_env_warn` (which research and kata-kanban already use correctly): 4 sites in swarm `config.rs` + 1 in corpus `semantic.rs`. Violates the "warn naming the malformed value" rule. | `hkask-mcp-swarm/src/config.rs:206,214,216,277`; `semantic.rs:711-713` |
| D6 | 🟠 | Canonical | mine | Training credentials allowlist misclassifies non-secret `NEBIUS_PROJECT_ID`/`NEBIUS_SUBNET_ID` as secrets, while identical-in-kind `RUNPOD_TEMPLATE_ID` was deliberately moved out with an explanatory comment. Inconsistent within one entry. | `mcp_servers.rs:327-328` vs `:349-357` |
| D7 | 🟠 | Canonical | mine | DB-open failures blanket-classified `internal` where per-variant mappers exist (wrong passphrase should be `permission_denied`). Corpus has the correct mapper but two call sites bypass it. | `corpus/src/tools/storage.rs:182`; `training/src/tools/dataset.rs:107`; correct mapper: `corpus/src/helpers.rs:53-55` |
| D8 | 🟡 | Canonical / Deletion | mine | Corpus stacks three passphrase wrappers: `default_corpus_passphrase` ← `database_passphrase` ← `default_purge_passphrase` (pure alias — fails deletion test outright). | `semantic.rs:982`; `persona.rs:39`; `storage.rs:452` |
| D9 | 🟡 | Canonical | mine | DB open/heal boilerplate diverges per server (curator self-healing reopen; portfolio/research/swarm/corpus each re-implement open-with-passphrase with different degradation behavior — some silent in-memory fallback). A shared helper would make the no-silent-fallback rule enforceable centrally. | `hkask_mcp_curator.rs:208-251`; `local_knowledge.rs:49`; et al. |

### Surface E — Unpinned advertised invariants (test gaps)

The repo's own rule: "advertised invariants must point to the enforcement
line." Several gates are advertised but have zero pinning tests.

| ID | Sev | Test | Origin | Finding | Evidence |
|---|---|---|---|---|---|
| E1 | 🟠 | Load-bearing | other | D28 standardized-artifact-storage layout unpinned: `agent_paths.rs` has zero tests; `tests/agent_paths.rs` never restored (DIVERGENCE.md D28 admits this). | `kask/crates/hkask-types/src/agent_paths.rs` |
| E2 | 🟠 | Load-bearing | other | Swarm typing layer unpinned: `schema_validate.rs` (7-keyword validator, `UnsupportedSchema` semantics) and `port_registry.rs` (the admission gate) have zero tests. | `hkask-mcp-swarm/src/{schema_validate,port_registry}.rs` |
| E3 | 🟠 | Load-bearing | other | Pure math unpinned: `hkask-forecast` (Brier, Bayesian update, tree marginalization — 930 lines, 0 tests) and `hkask-lisp` sandbox (0 in-crate tests; only indirect coverage via `lisp_eval_tool`). | `kask/crates/hkask-forecast/`, `kask/crates/hkask-lisp/` |
| E4 | 🟡 | Load-bearing | other | Tool-test ratchet stalled at 5/10: companies, corpus, prediction-markets, swarm, training lack `Parameters(`-seam contract tests (allowlist cap 9). | per-server `tests/` |

### Surface F — Documentation drift (the reference model itself is stale)

| ID | Sev | Test | Origin | Finding | Evidence |
|---|---|---|---|---|---|
| F1 | 🟠 | Canonical | both | D3 row is wrong in both directions: claims idempotency/reconnect tests were "deleted and not restored," but `tests/idempotent_creates.rs`, `tests/reconnect_integration.rs`, and 16 inline `runtime.rs` tests exist (restored 2026-08-24). Meanwhile D28's "not restored" claim about `agent_paths` tests is still true. | `DIVERGENCE.md` D3 vs `kask/mcp-servers/hkask-mcp-kata-kanban/tests/idempotent_creates.rs` |
| F2 | 🟠 | Canonical | both | Crate-count drift: plan §2.2 says 16 crates; MDS says 19; actual = 18 (17 `hkask-*` + `kask_bridge`). `hkask-event-store` is absent from the DIVERGENCE members list and the plan's crate table (REFRESH_TRIAGE already flags the MDS gap). | `zed-host-architecture-plan.md` §2.2; `DIVERGENCE.md:85` |
| F3 | 🟡 | Canonical | other | D18 row lists 4 widgets; code ships 5 (`hkask-swarm-widget` wired through `hkask-viz-core`). Members note also names a nonexistent `hkask-media-widget`. | `DIVERGENCE.md` D18 |
| F4 | 🟡 | Canonical | other | Plan §13.2 port table still shows the pre-D5 "keyring over CredentialsProvider" row — superseded, garbled. | `zed-host-architecture-plan.md` §13.2 |
| F5 | 🟡 | Canonical | other | `hkask-mcp-server/src/server.rs:7` doc example imports the wrong crate name (`use hkask_mcp::server::…`). | cited |
| F6 | 🟡 | Canonical | other | DIVERGENCE.md D-rows have accreted into multi-thousand-word essays (D3 alone dwarfs the rest) — drifting from "sync conflict map" toward "design history database." | `DIVERGENCE.md` |

### Surface G — Incomplete-deletion vestiges (post-review sweep, git-verified)

Found by sweeping for the §2.1 pattern globally. Each is a leftover the
originating deletion commit should have removed in the same PR.

| ID | Sev | Test | Finding | Evidence |
|---|---|---|---|---|
| G1 | 🟠 | Load-bearing | `system_prompt.hbs:46-47` instructs the agent to copy `display_hint` / ```media fenced blocks from media-tool results into replies — hkask-mcp-media was deleted (`26215d845e`); no live tool emits these fields. Dead instruction weight steering agent behavior toward a surface that cannot fire. Also: D18's fence-gate in `markdown.rs:2723` still routes ```media (harmless fall-through, but the gate's own comment claims it enumerates the registry). | `crates/agent/src/templates/system_prompt.hbs:26,46-47`; `crates/markdown/src/markdown.rs:2723` |
| G2 | 🟠 | Deletion | IPC request struct carries ~10 write-only `media_*` fields (`media_op`, `media_prompt`, `media_image_url`, `media_audio_url`, `media_text`, `media_voice`, `media_size`, `media_count`, `media_strength`, …) — declared, serialized, **read by nobody** after the media server deletion. Protocol dead weight; also blocks honest documentation of the IPC surface. **Hold** until the media-revival decision is final (§2.1) — revival makes them live again. | `kask/crates/hkask-types/src/inference_ipc.rs:168-185` |
| G3 | 🟡 | Deletion | `falsifiability/SKILL.md:59` references "codegraph ontological anchoring" — codegraph server deleted in the same window. Stale prose. | cited |
| G4 | 🟠 | Impedance | The D18 fence gate (`markdown.rs:2723`) lists `media/graph/kanban/portfolio/scenarios` but NOT `swarm_delegate_results` — yet `hkask-viz-core` composes 5 widgets including swarm (VIZ_TAG `swarm_delegate_results`). A ```` ```swarm_delegate_results ```` block never reaches its widget. Compounding: **no skill, template, or panel code currently emits that fence** — the widget is doubly unreachable (gate + no producer), and no pin asserts gate-list == registry-composition despite the comment claiming it must. | `crates/markdown/src/markdown.rs:2716-2729`; `hkask-viz-core/src/hkask_viz_core.rs:165`; grep: zero emitters |
| G5 | 🟡 | Canonical | `principle-constraints/SKILL.md:75` honestly documents deriving constraints from the *deleted* `hkask-verification/src/grounding.rs` — fine as history, but the phrasing "historical reference" is the only reason it isn't a B-class phantom. No action; recorded as the correct way to reference deleted machinery. | cited |

### Verified clean (no findings, recorded for completeness)

- All three Curator-prompt tools (`curator_status`, `curator_directive`,
  `curator_clear_algedonic_log`) are real built-in agent tools
  (`crates/agent/src/tools/curator_tools.rs:100,418,645`), pinned by
  `agent.rs:7811` — not missing MCP tools. (One audit's sub-agent
  initially flagged these as phantom; refuted on verification.)
- No dead crates: all 18 have ≥1 dependent manifest.
- Zero production `unwrap()` outside tests in `kask/crates/` proper
  (the unwrap clusters in A3 are in `kask_bridge` and MCP servers).
- No passphrase-resolution bypasses outside D4 (canonical chain intact
  at `hkask-mcp-server/src/server/credentials.rs:65-102`).
- No model-literal drift; constants flow through
  `hkask_inference::model_constants::DEFAULT_*`.
- All allowlists are `Some(...)`, never `None`.
- Zero `serde_json::Value` tool inputs (`AnyJsonValue` rule holds).
- Every named template ref across skills (other than B3) resolves in
  the registry; ~50 spot-checked MCP tool names exist.
- No canonical duplicates among near-neighbor skills (deep-vs-flash,
  coaching-vs-improvement, scenario-builder-vs-superforecasting,
  router-vs-discovery).
- Gradient-hunter vs gradient-seeded-recombination templates are NOT
  duplicates (line similarity 0.31–0.41 — independently authored).

---

## 3. Implementation plan

Phased, minimal-diff, mapped to finding IDs. ⬆ = touches upstream files
(`crates/**`) → needs a D-seam entry + pinning test in the same PR per
project rules. Each phase = one commit/PR domain, per strangler
discipline. Run the full gate (`check-hkask-no-zed-deps.sh`, reg gates,
`./script/clippy`, `cargo nextest run -p 'hkask-*' -p kask_bridge`,
selftests) at each phase boundary.

### Phase 0 — stop the bleeding (trivially reviewable, one PR each)

| Step | Fixes | Action | Upstream touch |
|---|---|---|---|
| 0.1 | A7 | Register `reg.sensor.memory` in `CANONICAL_NAMESPACES` (`kask/crates/hkask-types/src/event.rs:75`). Mirror in `scripts/check-reg-canonical.sh::is_canonical`. Both reg gates go green. | No |
| 0.2 | D1–D3 | Add the seven missing env vars to `config_env` allowlists in `kask_bridge/src/mcp_servers.rs` (kanban ×3, swarm ×3, corpus ×1). Align with actual reads; no behavior change when unset. | No |
| 0.3 | B2 | Remove `validate_golden_outputs` from gemba-walk's action enum + `recommend-actions.j2` (root-caused: its executor was deliberately deleted with the manifest-executor architecture in `6761c23961`; removal is the completion of that change, not a new deletion). | No |
| 0.4 | B3 | Recover `logo-discovery-map.j2` + `logo-formal-prompt.j2` verbatim from `git show 9e9c41ef3c^:kask/registry/templates/media/…` into a restored `templates/media/` dir; add a dormancy note to logo-builder naming its deferred media-server dependency (§2.1 disposition). | No |
| 0.5 | B7 | Complete the `9e9c41ef3c`/`7d0253ab0d` deletions: strip `harness-optimize`/`proptest` dispatch + `hkask-test-harness` oracle taxonomy from `tdd`, fix bug-hunt's trace-schema reference (restore the schema doc or inline the schema), decide `reg.contract.violated` (register or retarget), and either restore `./scripts/test --trace` or correct the references. | No |
| 0.6 | C2(b) | Seed `kask/registry/classify/*.yaml` alongside templates (extend the build.rs scan to a second asset class + test); derive `registry_dir` from `HKASK_TEMPLATE_ROOT` instead of `config_path` triple-parent arithmetic. | No |
| 0.7 | A1, A6, B5, B6, B4 | Doc/truth repair: add `hkask-event-store` to `DIVERGENCE.md:85`; correct stale `hkask-database` name in `sqlite.rs:29`; fix dead/drifted skill doc links (bug-hunt, skill-discovery, algedonic-review, gemba-walk); delete stale `swarm_panel` prose in `swarm-intelligence/SKILL.md:58`; mark `apply-template-rag.j2` deprecated or delete with its SKILL.md mention. | No |
| 0.8 | B8, B9, cross-cutting | **New gate: `check-skill-crossrefs.sh`** — mechanically resolve every `` `skill-name` `` backtick ref against `.agents/skills/`, every template ref against `kask/registry/templates/`, and every MCP tool name against server registries; fail CI on unresolved refs (allowlist for deliberate dormancy, e.g. logo-builder). This is the structural fix for the entire incomplete-change class: the unresolved half of a two-leg edit now fails CI instead of surviving as zombie prose. Use it to triage B8/B9 before fixing them by hand. | No |
| 0.9 | G1 | Remove the media display-hint instructions from `system_prompt.hbs:46-47` and drop ```media from the D18 fence gate + system-prompt widget list (media deferred; revival restores them — one commit when that decision lands). ⬆ upstream files → D-seam note + pin update (`test_system_prompt_advertises_every_supported_diagram_type`). | ⬆ |
| 0.10 | G4 | Either add `swarm_delegate_results` to the D18 fence gate and give swarm-steering an emitter contract, or delete the unreachable swarm widget until a producer exists; either way add the missing pin asserting gate-list == viz-core registry composition. ⬆ → D-seam note. | ⬆ |

### Phase 1 — pins before refactors (so later phases can't silently break behavior)

| Step | Fixes | Action | Upstream touch |
|---|---|---|---|
| 1.1 | E1 | Restore `agent_paths` layout tests (D28). | No |
| 1.2 | E2 | Add `schema_validate` + `port_registry` test modules (property tests for the validator + port registry). | No |
| 1.3 | E3 | Add `hkask-forecast` numeric tests (Brier, Bayesian update, tree marginalization) and `hkask-lisp` sandbox-budget tests. | No |
| 1.4 | E4 | One `Parameters(`-seam contract test per ratcheted server (companies, corpus, prediction-markets, swarm, training); empty the allowlist. | No |
| 1.5 | cross-cutting | Extend the D2 overlay-pin obligation: add a prompt-token test asserting `CURATOR_STATIC_CONTEXT`'s three named agent tools match `CuratorStatusTool::NAME` etc. Cheap hardening against future renames. | ⬆ (test lives in `crates/agent/`) |

### Phase 2 — template contract integrity ⬆ (upstream file `render_template_tool.rs`)

| Step | Fixes | Action | Upstream touch |
|---|---|---|---|
| 2.1 | C1 | Fix `strip_frontmatter` to handle the actual on-disk convention (strip leading `{# … #}` banner and/or recognize `[inference]` blocks regardless of position). Add failing tests first: render `kanban-task-management/triage`, assert output contains neither `[inference]` nor `visibility:`. Requires a D-seam note (file is D1-adjacent upstream surface). | ⬆ |
| 2.2 | C2(a) | Widen `crates/agent_skills/build.rs:158` collection filter to include `.jinja` (or explicitly enumerate runtime-consumed non-`.j2` assets); extend `test_seed_templates_writes_all_shipped_templates_to_disk` to assert the training harness's named files ship. | ⬆ |
| 2.3 | C3, C5 | Decide the `[inference]` convention once: either T4 is rescoped to mean "inside frontmatter only", or a lint script enforces single-block placement; then fix the 15 double-block templates. Resolves the rule-vs-reality contradiction rather than papering it. | No |
| 2.4 | C4 | For each of the four undocumented-context templates, add the input-contract table to the owning SKILL.md (mechanical documentation). | No |

### Phase 3 — deletions

| Step | Fixes | Action | Upstream touch |
|---|---|---|---|
| 3.1 | A8, A9, A13 | `hkask-services-core` reduction: delete `ServiceConfig`, `load_settings`, `settings_path`; prune `DomainKind::{Wallet,Pod}`; fold the crate down to what's actually consumed (`HkaskSettings::load`, `ServiceError`) or dissolve it into its 3 consumers (MDS already anticipates dissolution). Grep Cargo.toml deps afterwards per the dead-code rule. | No |
| 3.2 | A10 | Rule on `hkask-bridge-ontology::{sumo, sdmx}`: keep-with-test or delete (axis.rs is their only reference). | No |
| 3.3 | A11 | Rule on swarm `a2a*` (keep-with-test or delete). | No |
| 3.4 | A12 | Document or delete `KaskScenariosSettings {}`. | No |
| 3.5 | C6, C7 | Delete unreferenced templates (7 files) after a final grep-based reachability check **and** the 0.8 cross-ref gate passes; move `listening/tests/fixtures/*` and READMEs out of `registry/templates/`. | No |
| 3.6 | G2 | Delete the write-only `media_*` IPC fields — **only after** the media-revival decision is final (§2.1); if revival is approved instead, this step converts to "wire them or document them as reserved." ⬆ (protocol struct is kask-side but consumers span the seam). | No |
| 3.7 | G3 | Fix `falsifiability/SKILL.md:59` codegraph mention (keep the ontological-anchor concept, drop the deleted server's name). | No |

### Phase 4 — structural canonicalization

| Step | Fixes | Action | Upstream touch |
|---|---|---|---|
| 4.1 | D4 | Replace `dataset.rs:99` inline env read with `hkask_mcp_server::server::resolve_db_passphrase(&ctx.credentials)`; thread credentials through if not already available. | No |
| 4.2 | D5 | Replace five hand-rolled numeric parses with `parse_env_warn` (swarm `config.rs` ×4, corpus `semantic.rs` ×1). | No |
| 4.3 | D7 | Route the two DB-open sites through existing per-variant error mappers (`corpus/helpers.rs:53-55`; add equivalent for training). | No |
| 4.4 | D8 | Delete `default_purge_passphrase` alias; collapse to one wrapper next to the canonical helper. | No |
| 4.5 | D9 | Extract shared `open_sqlcipher_store(path, passphrase) -> StoreHandle` into `hkask-mcp-server` (or `hkask-services-core`); migrate curator/portfolio/research/swarm/corpus open paths; make the no-silent-in-memory-fallback rule enforceable centrally. One domain per commit. | No |
| 4.6 | A14 | Credential descriptor unification: one registry, one shape; add `inject_for_mcp: bool` (kills the `"runpod"` string special-case); decide the `hkask_` prefix question once (recommend: keep as-is but encode the rule in the descriptor doc + a naming-convention test). | No |
| 4.7 | A2 | Resolve `DatabaseDriver` ambiguity: add a test/mock driver impl (justifying the trait) or document it as an in-memory-SQLite test seam in the trait docs. | No |
| 4.8 | A5 | Add a mock `WorktreeSpawner` test seam so the IPC worktree-spawn dispatch path is testable without GPUI. | No |
| 4.9 | A3, A4 | Swap `lock().unwrap()` → `unwrap_or_else(|p| p.into_inner())` ×3; add `.log_err()` to the non-defensible `let _ =` sites (stale-socket removal, cleanup paths). Leave expected-dead-receiver sends as-is with a comment. | No |

### Phase 5 — documentation reconciliation (final, describes end state)

| Step | Fixes | Action | Upstream touch |
|---|---|---|---|
| 5.1 | F1, F6 | Rewrite DIVERGENCE.md D3 as a summary + pointer to inline tests (the tests are now the pins; the prose shouldn't duplicate them). Apply the same compression discipline to other over-long D-rows. | No |
| 5.2 | F2, F3 | Reconcile DIVERGENCE.md (D18 widget list, members list incl. `hkask-event-store`), plan §2.2/§13.2, MDS Composition Root; work off `REFRESH_TRIAGE.md`. | No |
| 5.3 | F4 | Update plan §13.2 port table to remove the pre-D5 "keyring over CredentialsProvider" row. | No |
| 5.4 | F5 | Fix `hkask-mcp-server/src/server.rs:7` doc example to import the correct crate name. | No |
| 5.5 | A16 | Decide `hkask-mcp` ↔ `hkask-mcp-server` naming disambiguation: rename runtime → `hkask-mcp-runtime` (or framework → `hkask-mcp-framework`) + fix the wrong doc example. Mechanical but touches many manifests; do during a quiet window. | No |

---

## 4. Sequencing rationale

- **Phase 0** is pure wins with near-zero risk and protects the sync
  runbook, operator overrides, and CI gates today. A7 alone unblocks the
  reg-canonical gate; D1–D3 restore operator-visible env overrides; 0.3–0.5
  complete the consumer legs of three verified deletions. **0.8 is the
  highest-leverage single step in the plan**: the cross-reference gate
  converts the entire incomplete-change failure class (§2.1) from
  "discovered by a later audit" to "fails CI at the boundary."
- **Phase 1** establishes the test pins that Phases 2–4 will rely on. The
  repo advertises these as gates; they're unpinned. Highest leverage per
  line written: property tests for the validator + port registry,
  golden-value tests for forecast math, one contract test per
  allowlisted server to empty the ratchet.
- **Phase 2** is the highest-leverage behavioral fix (93% of rendered
  prompts currently leak scaffolding per C1) but touches upstream files,
  so it carries the D-seam overhead — do it as one focused PR with
  tests. Folds naturally with the seeding-completeness work from Phase 0.
- **Phase 3** is internal hygiene: deletions that the deletion test
  flags. Each is independently reviewable.
- **Phase 4** is structural canonicalization inside MCP servers and
  kask_bridge. 4.5 is the only multi-crate migration and should follow
  the strangler-fig discipline (one domain per commit, dependency
  direction preserved).
- **Phase 5** is documentation as the final step — it describes the end
  state, not the path. Doing it earlier would mean rewriting it after
  every phase.

## 5. Open questions for the operator

### 5.0 Standing policy: triage before fix (the incomplete-change protocol)

Every phantom-reference finding gets the same three-way triage **before**
an edit direction is chosen, because "just remove it" and "just build it"
are both guesses when the real question is *which leg of a prior change
didn't finish*:

1. **Was the referenced thing deleted?** (`git log --diff-filter=D`) → if
   yes, the removal leg completed; the fix is to complete the consumer leg
   (update/remove the referencing side). The deletion was presumably
   deliberate — do not resurrect it without an operator decision.
2. **Was it renamed/moved?** (`git log --follow`, grep history) → if yes,
   the rename leg completed; fix is a mechanical reference update.
3. **Was it planned but never built?** (manifest/enum/doc references with
   no commit ever touching the target) → the *initiating* change is the
   incomplete one; decide whether to finish it (build the artifact) or
   roll the initiation back (remove the reference + any scaffolding).
   This is the only case where "implement the missing piece" is the
   default — and only when the surrounding design still makes sense.

The audit's root-cause column in §2.1 applies this protocol; findings
landing in case 3 are exactly where rushed work left scaffolding for
something never built. The 0.8 cross-reference gate exists so that future
incomplete legs fail CI at the boundary instead of being discovered by a
later audit.

### Remaining operator decisions

1. ~~B2 / B3~~ — resolved by §2.1 triage: B2 removes (case 1), B3
   recovers templates + marks dormant (operator decision recorded §2.1).
2. **A11 / 3.3**: keep swarm `a2a*` with tests, or delete? Needs a
   keep-or-delete ruling — apply §5.0 triage first (were its consumers
   deleted, or was it never wired?).
3. **A16 / 5.5**: rename `hkask-mcp` ↔ `hkask-mcp-server` now, or
   tolerate the doc-example confusion? Mechanical but touches many
   manifests.
4. **C5 / 2.3**: is the `[inference]` frontmatter rule (skill-maintenance
   T4) meant to apply inside frontmatter only, or to forbid the block
   entirely? The catalog predates the rule and fails it wholesale as
   written. **Resolve this before writing 2.1's parser tests** — the rule
   determines what the stripper should accept.
5. **Media revival (§2.1)**: defer confirmed for now; G2's `media_*` IPC
   fields and B3's recovered templates are held in dormancy. Revisit when
   media-type handling returns to scope.

---

*Audit conducted 2026-08-26 by two read-only `refactor-architecture`
passes; amended the same day after operator review (B1 retracted, §2.1
root-cause triage added, Surface G + steps 0.8–0.10 added, §5.0 triage
protocol adopted). No source files were modified; this document is the
deliverable. Verification was by inspection, grep, and git history.*