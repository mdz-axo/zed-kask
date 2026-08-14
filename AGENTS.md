<!-- Always-on context: keep minimal. Derivable data belongs in the registry or skills, not here. See .rules "Rules Hygiene". -->

# Agent Operating Guide — hKask

**hKask** (ℏKask) — A Rust framework for running agent skills as PDCA loops via a manifest executor. The `kask` workspace holds the libraries and MCP servers; `hkask-` is the crate prefix. See `Cargo.toml` for the current version.

---

## Skill Authoring Model (read this before creating skills)

In this repo, a skill is **not** a `SKILL.md` file. A skill is a PDCA loop executed by the kask manifest executor, defined by a registry crate (`kask/registry/manifests/<name>.yaml` + `kask/registry/templates/<name>/*.j2`). The `SKILL.md` under `.agents/skills/<name>/` is a **generated companion**, not the source of truth.

- **Creating a skill** → activate `create-skill` (overrides Zed's built-in, which assumes `SKILL.md` is the skill — that model does not apply here).
- **Validating / editing / installing / translating / pruning** → activate `skill-maintenance`.
- **Auditing template logic against stated goals** → activate `skill-logic-audit`.
- **Detecting capability gaps** → activate `skill-discovery`.

Never author `SKILL.md` directly. Build the registry crate first, then derive the companion.

For the current skill catalog (published manifests), see `kask/registry/manifests/`. Installed skill companions live in `.agents/skills/`. `skill-router` matches tasks to EXISTING installed skills; `skill-discovery` acquires NEW skills when `skill-router` emits uncovered capabilities (see `kask/docs/explanation/skills-and-composition.md`).

---

## Divergence & Upstream Seam

`zed-kask` is a minimal-divergence fork of Zed. **`DIVERGENCE.md`** (repo root) is the authoritative map of every upstream edit — the D1–D25 seams. Everything under `kask/` is ours (additive; upstream never touches → near-zero merge conflict). Everything else tracks upstream; the only divergences are the D-seams + the `[workspace.members]` / `[workspace.dependencies]` arrays in the root `Cargo.toml`.

- **Don't "fix" upstream files speculatively.** Push behavior into `kask/` behind a D-seam. If an upstream edit is unavoidable, add a D-seam entry + a pinning test in the same PR.
- **Every `// zed-kask:` comment** disabling upstream behavior needs a test pinning the disabled behavior.
- Before touching `crates/` (upstream), consult `DIVERGENCE.md` for the relevant seam and its pinning tests.
- **Governing invariant (§13.1):** hKask crates NEVER depend on zed-kask crates; zed-kask depends on hKask. The sole bidirectional seam is `kask_bridge` (D8). Enforced by `kask/scripts/check-hkask-no-zed-deps.sh`.
- Upstream-sync runbook: `DIVERGENCE.md` §"Upstream-sync runbook" (`git fetch upstream && git merge upstream/main` → resolve only D-seam conflicts → `./script/clippy` under `--deny warnings`).

---

## MCP Servers

hKask ships **13 MCP servers** launched by zed's `context_server` as child processes over stdio. They are the tool surface over the domain crates. (The `McpRuntime` that governs tool calls — capability-match gate + gas — runs in-process; the servers themselves are child processes.)

- **Runtime registry (authoritative, always current):** `BUILT_IN_MCP_SERVERS` in `kask/crates/kask_bridge/src/mcp_servers.rs`.
- **On-disk servers:** `kask/mcp-servers/hkask-mcp-*` — codegraph, companies, condenser, corpus, curator, kata-kanban, media, portfolio, prediction-markets, research, scenarios, swarm, training.
- **Catalog + per-tool contracts:** `kask/docs/reference/mcp-servers/README.md` (server catalog) + `kask/docs/qa/per-tool-contracts.md` (per-tool input struct, output shape, LLM I/O boundary).
- **Capability-match gate:** `McpRuntime::invoke` (token + gas) + per-agent `mcp_tools` allowlist (D3/D8). Tool responses are `{"content": ...}` envelopes — use `unwrap_tool_envelope`, don't re-implement.
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
- `kali-audit` / `supply-chain-sentinel` — Security posture.
- `lora-training` — PEFT method selection + math-contract gates (pre-flight before training job).
- `skill-router` — Match tasks to installed skills (fit-scored recommendations, gap signals for skill-discovery).
- `skill-discovery` — Detect capability gaps, search catalog, evaluate candidates, guide installation.

### Ensemble / Coaching (Multi-agent interaction)
- `kata-coaching`, `kata-improvement`, `improv` — Toyota Kata dialogues.

For the current skill catalog, see `kask/registry/manifests/`.

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
| Security regression library enforced | Every `status: enforced` checked | `scripts/check-kali-regressions.sh` |
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
- Generated artifacts: remove one-off files; keep `docs/generated/` and skill `manifest.yaml`.

---

## Activation Guide (Quick Reference)

| Situation | Activate First | Then |
|---|---|---|
| Before writing/reviewing code | `coding-guidelines` | `bug-hunt` or `tdd` |
| Hard bug / regression | `diagnose` | `graph-audit` (code mode, if unknown structure) |
| Low confidence / high uncertainty | `metacognition` (assess + calibrate) | `falsifiability` (if hypothesis-conflict) or `improv` (riffing, for divergent exploration) |
| Module design / simplification | `essentialist` (3 gates) | `deep-module` |
| Security audit | `kali-audit` | `supply-chain-sentinel` (manifests) |
| LoRA/QLoRA training config audit | `lora-training` | `tdd` (training-loop code) |
| GPU training pod creation | [`kask/docs/research/archive/gpu-provider-research-2026-07-23.md`](kask/docs/research/archive/gpu-provider-research-2026-07-23.md) | `lora-training` (config audit) |
| Self-improvement / prompt evolution | `metacognition` | `gpa-evolution` (post-convergence) |
| Skill matching for a task | `skill-router` | `task-breakdown` (decompose) then `skill-discovery` (if gaps found) |
| Capability gap detection | `skill-discovery` | `skill-maintenance` (install/validate the new skill) |
| Multi-agent coaching | `kata-coaching` | `improv` (interaction grammar) |

For low-confidence regimes: `metacognition` → `falsifiability` → `improv`. Layered detail lives in the `metacognition` and `pragmatic-semantics` skills.

---

## Key Operational Scripts

- `.github/workflows/ci.yml` — CI pipeline
- `.github/workflows/audit.yml` — Weekly dependency audit
- `scripts/check-string-errors.sh` — `Result<_, String>` guard
- `docs/ci/verify-docs.sh` — Documentation health

> Full reference: `docs/reference/` · Design: `docs/explanation/` · How-to: `docs/how-to/` · Tutorial: `docs/tutorial/`

---

> **Quality reminder (Weinberg):** Value = "value to some person who matters." This guide optimizes for userpod orientation — not exhaustiveness. If you need full registry details, consult `kask/registry/manifests/` directly.
>
> **Feedback:** If an agent failure reveals a missing trap or routing gap, propose an addition under "Suggested AGENTS.md additions" in your PR description. Mirror the `.rules` hygiene pattern: validate the pattern in review before merging.
