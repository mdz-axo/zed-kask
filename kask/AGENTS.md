# Agent Operating Guide — hKask

**hKask** (ℏKask) — A Minimal Viable Container for UserPods | `kask` binary | `hkask-` crate prefix | v0.31.0

---

## Capability Groups (Principle-Aligned)

50 skills in `.agents/skills/` (47 PDCA skills + 2 templates + 1 bundle). Below: 7 principle-aligned groups covering the active registry. Full list: `.agents/skills/`.

| Group | Activation | Principle / Defense Link | Key Skills |
|---|---|---|---|
| **Guardrails** | Author-first (before any code/review) | P5 · P7 (simplicity, depth) | `coding-guidelines` |
| **Core Development** | Agent-autonomous (PDCA cycles) | P3 · P5 · P7 | `bug-hunt`, `tdd`, `diagnose`, `deep-module`, `strangler-fig`, `idiomatic-rust` |
| **Reasoning & Analysis** | Agent-autonomous / ensemble | P1 · P3 · P12 (sovereignty, consent, identity) | `pragmatic-semantics`, `sequential-inquiry`, `falsifiability`, `metacognition` |
| **Kata & Coaching** | Ensemble / coaching loop | P4 (clear boundaries) | `kata` (bundle), `kata-improvement`, `kata-coaching`, `improv` |
| **Meta & Maintenance** | Agent-autonomous (self-improvement) | P1 · P12 | `skill-maintenance`, `skill-logic-audit`, `gpa-evolution`, `handoff` |
| **Routing & Curation** | Agent-autonomous (skill matching) | P5 · P12 | `skill-router`, `skill-discovery` |
| **Security & Posture** | Agent-autonomous (runtime) | P5–P8 · P12 | `kali-audit`, `supply-chain-sentinel`, `runtime-posture-monitor`, `attack-taxonomy-mapper` |
| **Training** | Pre-flight (before training job) | P3.1 · P5 · P8 · P12 | `lora-training` |

*Note: `media-workflow`, `logo-builder`, `qa-script-builder` are specialized templates; activate as needed, not by default.*

---

## Essential Skills (By Activation Pattern)

### Author-First (Always activate before writing/reviewing)
- `coding-guidelines` — Simplicity First, Surgical Changes, Goal-Driven Execution.

### Agent-Autonomous (PDCA / defense / improvement cycles)
- `metacognition` — Decompose → Assess → Calibrate → GEPA improve.
- `essentialist` — 3-gate elimination (Exist → Surface → Contract).
- `gpa-evolution` — Genetic-Pareto mutation of text artifacts.
- `bug-hunt` / `diagnose` — Exploration and debugging.
- `kali-audit` / `supply-chain-sentinel` — Security posture.
- `lora-training` — PEFT method selection + math-contract gates (pre-flight before training job).
- `skill-router` — Match tasks to installed skills (fit-scored recommendations).
- `skill-discovery` — Detect capability gaps, search catalog, evaluate candidates, guide installation.

### Ensemble / Coaching (Multi-agent interaction)
- `kata` bundle, `kata-coaching`, `improv` — Toyota Kata dialogues.

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

## Tooling Policy

- Rust only. Python is **not** an acceptable dependency (ad-hoc exploration OK, delete before commit).
- Preferred: `bash` under `scripts/`, Rust binaries, `build.rs`.
- Generated artifacts: remove one-off files; keep `docs/generated/` and skill `manifest.yaml`.

---

## Activation Guide (Quick Reference)

| Situation | Activate First | Then |
|---|---|---|
| Before writing/reviewing code | `coding-guidelines` | `bug-hunt` or `tdd` |
| Hard bug / regression | `diagnose` | `codegraph` (if unknown structure) |
| Low confidence / high uncertainty | `metacognition` (assess + calibrate) | `improv` (riffing) or `zoom-out` (if context-loss) or `falsifiability` (if hypothesis-conflict) |
| Module design / simplification | `essentialist` (3 gates) | `deep-module` |
| Security audit | `kali-audit` | `supply-chain-sentinel` (manifests) |
| LoRA/QLoRA training config audit | `lora-training` | `tdd` (training-loop code) |
| GPU training pod creation | [`docs/research/gpu-provider-research-2026-07-23.md`](docs/research/gpu-provider-research-2026-07-23.md) | `lora-training` (config audit) |
| Self-improvement / prompt evolution | `metacognition` | `gpa-evolution` (post-convergence) |
| Skill matching for a task | `skill-router` | `skill-discovery` (if gaps found) |
| Capability gap detection | `skill-discovery` | `skill-router` (after new skill installed) |
| Multi-agent coaching | `kata-coaching` | `improv` (interaction grammar) |
| Session handoff | `handoff` | — |

---

## Uncertainty Management Stack

When an agent or LLM enters a low-confidence regime (confidence < 0.5), the system routes through a layered stack of certainty-finding strategies:

| Layer | Strategy | Skills |
|-------|----------|--------|
| **Epistemic classification** | Classify certainty level, trace provenance, resolve conflicts | `pragmatic-semantics` |
| **Standard PDCA** | Decompose, assess, calibrate strategy, experiment | `metacognition` (obstacle type `uncertainty`), `sequential-inquiry`, `falsifiability` |
| **Non-standard paths** | Find certainty through perspective-shift, not more data | `metacognition` (Falstaffian rotation, ellipsis detection), `improv` (Riffing for divergent exploration) |
| **Context expansion** | Raise confidence by broadening scope, not adding detail | `zoom-out`, `codegraph` |
| **Runtime enforcement** | Prune low-confidence memory, escalate low-confidence output | `Confidence` type, consolidation `confidence_floor`, Curator escalation queue (Pattern C) |
| **Curation gap-fill** | Detect missing certainty-finding methods, acquire new skills | `skill-router` (`epistemic_state` boost), `skill-discovery` (`epistemic` gap category) |

The `skill-router` accepts an optional `epistemic_state` input (`confidence` + `uncertainty_type`) that boosts trigger-alignment for certainty-finding skills when confidence < 0.5. The `skill-discovery` detect-gap phase classifies `epistemic` gaps distinctly from `knowledge` gaps (epistemic = missing certainty-finding methods; knowledge = missing facts).

---

## Key Operational Docs (≤ 8)

- `.github/workflows/ci.yml` — CI pipeline
- `.github/workflows/audit.yml` — Weekly dependency audit
- `scripts/check-string-errors.sh` — `Result<_, String>` guard
- `docs/ci/verify-docs.sh` — Documentation health
- `crates/hkask-types/src/lib.rs` — Foundation types
- `crates/hkask-regulation/src/types/loops/loop_trait.rs` — `Loop` trait
- `mcp-servers/hkask-mcp-codegraph/src/lib.rs` — CodeGraph MCP server
- `docs/architecture/` — Canonical architecture docs

> Full reference: `docs/reference/` · Design: `docs/explanation/` · How-to: `docs/how-to/` · Tutorial: `docs/tutorial/`

---

> **Quality reminder (Weinberg):** Value = "value to some person who matters." This guide optimizes for userpod orientation — not exhaustiveness. If you need full registry details, consult `.agents/skills/` directly.

