<!-- Always-on context: keep minimal. Derivable data belongs in skills, not here. See .rules "Rules Hygiene". -->

# Agent Operating Guide — hKask

**hKask** (ℏKask) — A Rust framework for agent skills using upstream Zed's body-injection model plus `lisp_eval` and `render_template` tools for deterministic computation and structured prompt rendering. The `kask` workspace holds the libraries and MCP servers; `hkask-` is the crate prefix. See `Cargo.toml` for the current version.

---

## Skill System

### Execution Model

Skills execute via **upstream Zed body injection**: the `skill` tool reads the `SKILL.md` body from disk and injects it into the conversation as a `<skill_content>` envelope. The model reads the body and follows the instructions. The model IS the executor.

### Skill Tools

Two built-in tools support skill composition beyond what upstream Zed provides:

- **`lisp_eval`** — sandboxed Lisp interpreter. No I/O, no `eval`, no network. Bounded by `max_steps` (default 100000) and `max_depth` (default 64). The model calls it when a SKILL.md instructs deterministic computation: convergence signals, invariant checks, scoring, counting items in structured output.
- **`render_template`** — renders prompt templates from `kask/registry/templates/` with context variables. Path traversal protection via `canonicalize` + `starts_with`. Template base path wired via `agent::set_template_base_path()` (OnceLock) in `main.rs` at startup. The model calls it when a SKILL.md instructs structured prompt scaffolding for a specific step.

### PDCA Loops

PDCA (Plan-Do-Check-Act) loops are **model-coordinated**. The SKILL.md body describes:
- What to do (the methodology)
- Convergence criteria (when to stop iterating)
- Maximum iteration count (when to escalate)

The model executes each full iteration, evaluates the convergence criteria (optionally using `lisp_eval` for deterministic checks), and re-enters the cycle if convergence is not met. The agent loop's token budget and tool permissions are the only limits.

### Skill Authoring

A skill **is** a `SKILL.md` file — the upstream Zed model. The body contains the full methodology. Optional prompt templates in `kask/registry/templates/<skill>/` provide structured scaffolding the model can render via `render_template`.

- **Creating a skill** → activate `create-skill`.
- **Validating / editing / translating / pruning** → activate `skill-maintenance`.
- **Auditing skill logic against stated goals** → activate `skill-logic-audit`.
- **Detecting capability gaps** → activate `skill-discovery`.

### Skill Locations

- **Project-local skills:** `.agents/skills/<name>/SKILL.md` (in the worktree)
- **Global skills:** `~/.local/share/hkask/skills/<name>/SKILL.md` (seeded from the compiled-in payload at startup; core skills are always overwritten, user skills are seed-if-missing)
- **Prompt templates:** `kask/registry/templates/<skill>/*.j2` (dev: live source tree; prod: seeded to `{kask_data_dir}/skills/registry/templates/`)
- `skill-router` matches tasks to EXISTING installed skills; `skill-discovery` acquires NEW skills when `skill-router` emits uncovered capabilities.

---

## Divergence & Upstream Seam

`zed-kask` is a minimal-divergence fork of Zed. **`DIVERGENCE.md`** (repo root) is the authoritative map of every upstream edit — the D1–D32 seams. Everything under `kask/` is ours (additive; upstream never touches → near-zero merge conflict). Everything else tracks upstream; the only divergences are the D-seams + the `[workspace.members]` / `[workspace.dependencies]` arrays in the root `Cargo.toml`.

- **Don't "fix" upstream files speculatively.** Push behavior into `kask/` behind a D-seam. If an upstream edit is unavoidable, add a D-seam entry + a pinning test in the same PR.
- **Every `// zed-kask:` comment** disabling upstream behavior needs a test pinning the disabled behavior.
- Before touching `crates/` (upstream), consult `DIVERGENCE.md` for the relevant seam and its pinning tests.
- **Governing invariant (§13.1):** hKask crates NEVER depend on zed-kask crates; zed-kask depends on hKask. The sole bidirectional seam is `kask_bridge` (D8). Enforced by `kask/scripts/check-hkask-no-zed-deps.sh`.
- Upstream-sync runbook: `DIVERGENCE.md` §"Upstream-sync runbook" (`git fetch upstream && git merge upstream/main` → resolve only D-seam conflicts → `./script/clippy` under `--deny warnings`).

---

## MCP Servers

hKask ships **10 MCP servers** launched by zed's `context_server` as child processes over stdio. They are the tool surface over the domain crates. (The `McpRuntime` that governs tool calls runs in-process; the servers themselves are child processes.)

- **Runtime registry (authoritative, always current):** `BUILT_IN_MCP_SERVERS` in `kask/crates/kask_bridge/src/mcp_servers.rs`.
- **On-disk servers:** `kask/mcp-servers/hkask-mcp-*` — companies, corpus, curator, kata-kanban, portfolio, prediction-markets, research, scenarios, swarm, training.
- **Catalog + per-tool contracts:** `kask/docs/reference/mcp-servers/README.md` (server catalog) + `kask/docs/qa/per-tool-contracts.md` (per-tool input struct, output shape, LLM I/O boundary).
- **Tool dispatch:** `McpRuntime::invoke` (per-tick call ceiling / runaway-loop breaker) + per-agent `mcp_tools` allowlist (D3/D8). Tool responses are `{"content": ...}` envelopes — use `unwrap_tool_envelope`, don't re-implement.
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
- `skill-router` — Match tasks to installed skills (fit-scored recommendations, gap signals for skill-discovery).
- `skill-discovery` — Detect capability gaps, search catalog, evaluate candidates, guide installation.

### Ensemble / Coaching (Multi-agent interaction)
- `kata-coaching`, `kata-improvement`, `improv` — Toyota Kata dialogues.

For the current skill catalog, see `.agents/skills/` (project-local) and `~/.local/share/hkask/skills/` (global).

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
| No visual-UI / monitoring infra (grafana/prometheus) | `grep` scan | Inline `.github/workflows/ci.yml` |
| No hardcoded secrets | Env vars / keystore only | Inline `ci.yml` |
| No `Result<_, String>` | `thiserror` enums | `scripts/check-string-errors.sh` |
| No unused crate dependencies | `nightly -D unused_crate_dependencies` | Nightly job |
| MCP servers: tool-behavior contract tests | `Parameters(` seam | `scripts/check-mcp-tool-tests.sh` |
| Regulation namespace invariant (`reg.*` → `CANONICAL_NAMESPACES`) | Canonical span check | `scripts/check-reg-canonical.sh` |
| Training-config regression library enforced | Every `surface: training` `status: enforced` checked | `scripts/check-lora-training-regressions.sh` |

Only #1 partially CI-gated; #2–#4 enforced by review.

---

## Build & Test

- Lint: `./script/clippy` (not `cargo clippy`)
- Build: `cargo build`
- Test: `cargo test`
- Docs health: `docs/ci/verify-docs.sh`

---

## Tooling Policy

- Rust only. Python is **not** an acceptable dependency (ad-hoc exploration OK, delete before commit).
- Preferred: `bash` under `scripts/`, Rust binaries, `build.rs`.
- Generated artifacts: remove one-off files; keep `docs/generated/`.

---

## Activation Guide (Quick Reference)

| Situation | Activate First | Then |
|---|---|---|
| Before writing/reviewing code | `coding-guidelines` | `bug-hunt` or `tdd` |
| Hard bug / regression | `diagnose` | `bug-hunt` (exploratory testing) |
| Low confidence / high uncertainty | `metacognition` (assess + calibrate) | `falsifiability` (if hypothesis-conflict) or `improv` (riffing, for divergent exploration) |
| Module design / simplification | `essentialist` (3 gates) | `deep-module` |
| LoRA/QLoRA training config audit | `lora-training` | `tdd` (training-loop code) |
| GPU training pod creation | [`kask/docs/research/archive/gpu-provider-research-2026-07-23.md`](kask/docs/research/archive/gpu-provider-research-2026-07-23.md) | `lora-training` (config audit) |
| Self-improvement / prompt evolution | `metacognition` | `gpa-evolution` (post-convergence) |
| Skill matching for a task | `skill-router` | `task-breakdown` (decompose) then `skill-discovery` (if gaps found) |
| Capability gap detection | `skill-discovery` | `skill-maintenance` (install/validate the new skill) |
| Multi-agent coaching | `kata-coaching` | `improv` (interaction grammar) |
| Deterministic computation needed | `lisp_eval` tool | (call directly — no skill activation needed) |
| Structured prompt scaffolding needed | `render_template` tool | (call directly — no skill activation needed) |

For low-confidence regimes: `metacognition` → `falsifiability` → `improv`. Layered detail lives in the `metacognition` and `pragmatic-semantics` skills.

---

## Key Operational Scripts

- `.github/workflows/ci.yml` — CI pipeline
- `.github/workflows/audit.yml` — Weekly dependency audit
- `scripts/check-string-errors.sh` — `Result<_, String>` guard
- `docs/ci/verify-docs.sh` — Documentation health

> Full reference: `docs/reference/` · Design: `docs/explanation/` · How-to: `docs/how-to/` · Tutorial: `docs/tutorial/`

---

> **Quality reminder (Weinberg):** Value = "value to some person who matters." This guide optimizes for userpod orientation — not exhaustiveness. If you need full skill details, consult `.agents/skills/` directly.
>
> **Feedback:** If an agent failure reveals a missing trap or routing gap, propose an addition under "Suggested AGENTS.md additions" in your PR description. Mirror the `.rules` hygiene pattern: validate the pattern in review before merging.