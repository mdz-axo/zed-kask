<!-- Manual-read orientation doc. NOT auto-loaded into the system prompt — `.rules` shadows `AGENTS.md` in the rules-file priority order (see `RULES_FILE_NAMES` in `prompt_store/src/prompts.rs`). `.rules` is the auto-loaded file; this is the human/agent reference. See .rules "Rules Hygiene". -->

# Agent Operating Guide — hKask

**hKask** (ℏKask) — A Rust framework for agent skills using upstream Zed's body-injection model plus `lisp_eval` and `render_template` tools for deterministic computation and structured prompt rendering. The `kask` workspace holds the libraries and MCP servers; `hkask-` is the crate prefix. See `Cargo.toml` for the current version.

---

## Skill System

### Skill Authoring

A skill **is** a `SKILL.md` file — the upstream Zed model. The body contains the full methodology. Optional prompt templates in `kask/registry/templates/<skill>/` provide structured scaffolding the model can render via `render_template`.

- **Creating a skill** → activate `create-skill`.
- **Validating / editing / translating / pruning** → activate `skill-maintenance`.
- **Auditing skill or template logic against stated goals** → activate `skill-maintenance` (its template-logic audit covers `.j2` goals and callsites).
- **Detecting capability gaps** → activate `skill-discovery`.

### Skill Locations

- **Project-local skills:** `.agents/skills/<name>/SKILL.md` (in the worktree)
- **Global skills:** `~/.local/share/zed-kask/skills/<name>/SKILL.md` (seeded from the compiled-in payload at startup; core skills are always overwritten, user skills are seed-if-missing)
- **Prompt templates:** `kask/registry/templates/<skill>/*.j2` (dev: live source tree; prod: seeded to `{kask_data_dir}/skills/registry/templates/`)
- `skill-discovery` matches tasks to installed skills (route) and acquires NEW skills when route emits uncovered capabilities (detect-gap → evaluate).

---

## Divergence & Upstream Seam

`zed-kask` is a minimal-divergence fork of Zed. **`DIVERGENCE.md`** (repo root) is the authoritative map of every upstream edit — the D1–D32 seams. Everything under `kask/` is ours (additive; upstream never touches → near-zero merge conflict). Everything else tracks upstream; the only divergences are the D-seams + the `[workspace.members]` / `[workspace.dependencies]` arrays in the root `Cargo.toml`.

- Before touching `crates/` (upstream), consult `DIVERGENCE.md` for the relevant seam and its pinning tests.
- **Governing invariant (§13.1):** hKask crates NEVER depend on zed-kask crates; zed-kask depends on hKask. The sole bidirectional seam is `kask_bridge` (D8). Enforced by `kask/scripts/check-hkask-no-zed-deps.sh`.
- Upstream-sync runbook: `DIVERGENCE.md` §"Upstream-sync runbook" (`git fetch upstream && git merge upstream/main` → resolve only D-seam conflicts → `./script/clippy` under `--deny warnings`).

---

## MCP Servers

hKask ships **12 MCP servers** launched by zed's `context_server` as child processes over stdio. They are the tool surface over the domain crates. (The `McpRuntime` that governs tool calls runs in-process; the servers themselves are child processes.)

- **Runtime registry (authoritative, always current):** `BUILT_IN_MCP_SERVERS` in `kask/crates/kask_bridge/src/mcp_servers.rs`.
- **On-disk servers:** `kask/mcp-servers/hkask-mcp-*` — companies, corpus, curator, kata-kanban, media, portfolio, prediction-markets, research, scenarios, spreadsheet, swarm, training.
- **Catalog + per-tool behavior:** `kask/docs/reference/mcp-servers/README.md` (server catalog with per-server tool-surface pin tests); per-tool behavior is enforced by the tool-behavior contract-test standard (`kask/scripts/check-mcp-tool-tests.sh` + the Testing standard section of the catalog README), not by a per-tool contracts doc.
- **Tool dispatch:** `McpRuntime::invoke` (per-tick call ceiling / runaway-loop breaker) + per-agent `mcp_tools` allowlist (D3/D8).
- **§13.1 at the MCP boundary:** MCP servers reach hKask primitives via `kask_bridge` (D8); they never link zed-kask crates directly.

---

## Essential Skills (By Activation Pattern)

### Author-First (Always activate before writing/reviewing)
- `coding-guidelines` — Simplicity First, Surgical Changes, Goal-Driven Execution.

### Agent-Autonomous (PDCA / defense / improvement cycles)
- `metacognition` — Decompose → Assess → Calibrate → GEPA improve.
- `essentialist` — 3-gate elimination (Exist → Surface → Contract).
- `gpa-evolution` — Genetic-Pareto mutation of text artifacts.
- `bug-hunt` / `diagnose` — Exploration and debugging.
- `refactor-architecture` — End-to-end architecture refactoring (discover → audit → strangle → verify).
- `lora-training` — PEFT method selection + math-contract gates (pre-flight before training job).
- `skill-discovery` — Route tasks to installed skills, detect capability gaps, evaluate candidates before installation.

### Ensemble / Coaching (Multi-agent interaction)
- `kata-coaching`, `kata-improvement`, `improv` — Toyota Kata dialogues.

For the current skill catalog, see `.agents/skills/` (project-local) and `~/.local/share/zed-kask/skills/` (global).

---

## Prohibitions (Magna Carta P1–P4, P12 — Violations Must Be Deleted)

| # | Prohibition | Principle | Enforcement |
|---|---|---|---|
| 1 | No `todo!()`, `unimplemented!()`, `#[deprecated]`, stubs | P5 · P3 | `clippy -D warnings` (partial CI) |
| 2 | No anonymous agency — every action has an authenticated author | P12 · P1 | Code review |
| 3 | No hidden parameters or admin-gated settings | P3 | Code review |
| 4 | No pass-through abstractions (deep-module discipline) | P5 · P7 | Code review |

---

## CI-Enforced Gates

| Gate | Enforcement | Script / Method |
|---|---|---|
| No visual-UI / monitoring infra (grafana/prometheus) | `grep` scan | Review-enforced (was inline in the removed `kask-ci.yml`) |
| No hardcoded secrets | Env vars / keystore only | Review-enforced (was inline in the removed `kask-ci.yml`) |
| No `Result<_, String>` | `thiserror` enums | `kask/scripts/check-string-errors.sh` |
| No unused crate dependencies | `cargo machete` (kask/ scope) | `script/clippy` (local) |
| MCP servers: tool-behavior contract tests | `Parameters(` seam | `kask/scripts/check-mcp-tool-tests.sh` |
| Regulation namespace invariant (`reg.*` → `CANONICAL_NAMESPACES`) | Canonical span check | `kask/scripts/check-reg-canonical.sh` |

Only #1 partially CI-gated; #2–#4 enforced by review.

---

## Build & Test

- Build: `cargo build`
- Test: `cargo test` (CI runs the kask subtree serially: `cargo test --tests --no-fail-fast -p 'hkask-*' -p kask_bridge -- --test-threads=1`)

---

## Tooling Policy

- Rust only. Python is **not** an acceptable dependency (ad-hoc exploration OK, delete before commit).
- Preferred: `bash` under `kask/scripts/`, Rust binaries, `build.rs`.
- Generated artifacts: remove one-off files before commit.

---

## Activation Guide (Quick Reference)

| Situation | Activate First | Then |
|---|---|---|
| Before writing/reviewing code | `coding-guidelines` | `bug-hunt` or `tdd` |
| Hard bug / regression | `diagnose` | `bug-hunt` (exploratory testing) |
| Low confidence / high uncertainty | `metacognition` (assess + calibrate) | `falsifiability` (if hypothesis-conflict) or `improv` (riffing, for divergent exploration) |
| Module design / simplification | `essentialist` (3 gates) | `deep-module` |
| LoRA/QLoRA training config audit | `lora-training` | `tdd` (training-loop code) |
| Fine-tuning run (submit, track, evaluate) | `adapter-lifecycle` | `lora-training` (config audit) |
| Self-improvement / prompt evolution | `metacognition` | `gpa-evolution` (post-convergence) |
| Skill matching for a task | `skill-discovery` (route) | `task-breakdown` (decompose) first; detect-gap if coverage is partial |
| Capability gap detection | `skill-discovery` | `skill-maintenance` (install/validate the new skill) |
| Multi-agent coaching | `kata-coaching` | `improv` (interaction grammar) |
| Deterministic computation needed | `lisp_eval` tool | (call directly — no skill activation needed) |
| Structured prompt scaffolding needed | `render_template` tool | (call directly — no skill activation needed) |

For low-confidence regimes: `metacognition` → `falsifiability` → `improv`. Layered detail lives in the `metacognition` and `pragmatic-semantics` skills.

---

## Key Operational Scripts

- `.github/workflows/kask-invariants.yml` — CI pipeline: invariant checks on every push/PR (§13.1 deps, zed isolation, desktop collision, fmt, dead-code allows, string errors, MCP tool tests, reg namespaces), serial `cargo test` over `hkask-*` + `kask_bridge`, and the full `cargo check -p zed` on main pushes
- `kask/scripts/check-string-errors.sh` — `Result<_, String>` guard

> Docs map: `kask/docs/README.md` · Reference: `kask/docs/reference/` · Architecture: `kask/docs/architecture/` · Diataxis: `kask/docs/diataxis/`

---

> **Quality reminder (Weinberg):** Value = "value to some person who matters." This guide optimizes for user orientation — not exhaustiveness. If you need full skill details, consult `.agents/skills/` directly.
>
> **Feedback:** If an agent failure reveals a missing trap or routing gap, propose an addition under "Suggested AGENTS.md additions" in your PR description. Mirror the `.rules` hygiene pattern: validate the pattern in review before merging.