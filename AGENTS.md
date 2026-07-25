<!-- Always-on context: keep minimal. Derivable data belongs in the registry or skills, not here. See .rules "Rules Hygiene". -->

# Agent Operating Guide — hKask

**hKask** (ℏKask) — A Rust framework for running agent skills as PDCA loops via a manifest executor. The `kask` workspace holds the libraries and MCP servers; `hkask-` is the crate prefix. See `Cargo.toml` for the current version.

---

## Skill Authoring Model (read this before creating skills)

In this repo, a skill is **not** a `SKILL.md` file. A skill is a PDCA loop executed by the kask manifest executor, defined by a registry crate (`kask/registry/manifests/<name>.yaml` + `kask/registry/templates/<name>/*.j2`). The `SKILL.md` under `.agents/skills/<name>/` is a **generated companion**, not the source of truth.

- **Creating a skill** → activate `create-skill` (overrides Zed's built-in, which assumes `SKILL.md` is the skill — that model does not apply here).
- **Validating / editing / installing / translating / pruning** → activate `skill-maintenance`.
- **Detecting capability gaps** → activate `skill-discovery`.

Never author `SKILL.md` directly. Build the registry crate first, then derive the companion.

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
- `skill-router` — Match tasks to installed skills (fit-scored recommendations).
- `skill-discovery` — Detect capability gaps, search catalog, evaluate candidates, guide installation.

### Ensemble / Coaching (Multi-agent interaction)
- `kata` bundle, `kata-coaching`, `improv` — Toyota Kata dialogues.

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
| GPU training pod creation | [`docs/research/gpu-provider-research-2026-07-23.md`](docs/research/gpu-provider-research-2026-07-23.md) | `lora-training` (config audit) |
| Self-improvement / prompt evolution | `metacognition` | `gpa-evolution` (post-convergence) |
| Skill matching for a task | `skill-router` | `skill-discovery` (if gaps found) |
| Capability gap detection | `skill-discovery` | `skill-router` (after new skill installed) |
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
