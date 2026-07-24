---
title: "Fusion Mode — Multi-Model Deliberation"
audience: [operators, developers, users]
last_updated: 2026-07-16
version: "0.31.0"
status: "Active"
domain: "Inference"
mds_categories: [domain, composition, trust]
---

# Fusion Mode — Multi-Model Deliberation

Configure and operate hKask's provider-agnostic fusion engine: a panel of models answers in parallel, then a **judge** processes the responses. The judge is either an **LLM** operating in one of five deliberation modes (synthesis, best-of-n, critique, deliberation, pi), or an **algorithm** — the `algo` / no-judge path that merges panel responses deterministically with zero LLM calls. Fusion is **opt-in and disabled by default** — it activates only when you explicitly configure a judge and panel.

This guide covers global env-var configuration, per-skill manifest overrides, the five LLM deliberation modes, the algo / no-judge method family, skill anchoring, bypass semantics, and operational checks. All facts are verified against the implementation in `crates/hkask-inference/src/fusion_orchestrator.rs`, `crates/hkask-types/src/fusion.rs`, `crates/hkask-inference/src/config.rs`, and `crates/hkask-templates/src/executor.rs`.

---

## What Fusion Is

Fusion is a **hKask-side orchestration engine**, not a provider feature. hKask itself:

1. Sends the user's prompt to every panel model **in parallel** (each routing through its own provider via a 2-letter prefix).
2. Collects all panel responses.
3. Dispatches to a **judge** — either an LLM operating in one of five deliberation modes, or an algorithm that merges responses without an LLM call.

The hKask orchestrator is provider-agnostic and works across DeepInfra, fal.ai, Together, OpenRouter, KiloCode, Ollama, and Cline.

### Two Judge Families

The `judge` field in `FusionConfig` accepts one of two families:

| Family | `judge` value | How it works | LLM judge call? | Cost |
|--------|---------------|-------------|-----------------|------|
| **LLM deliberation** | A model name (e.g., `deepseek-v4-pro`, `DI/qwen/qwen3`) | Panel answers in parallel → judge model synthesizes/picks/critiques/deliberates/plans per the configured mode | Yes | (panel size + judge calls × rounds) × token cost |
| **Algo / no-judge** | `algo` | Panel answers in parallel → deterministic algorithm merges responses — no LLM judge call | No | Panel tokens only — zero judge overhead |

The algo / no-judge family is extensible: the current implementation provides two methods — **recursive JSON merge** (`merge_json_values`) and **majority vote** (`vote_json_values`) — selected via the `algo_method` field. The architecture anticipates additional methods (see [Algo / No-Judge Methods](#algo--no-judge-methods) below).

**Why fusion exists:** Multi-model deliberation improves answer quality on hard reasoning tasks by combining diverse model perspectives under a methodologically-anchored judge. It is a quality/cost tradeoff — LLM-judge fusion multiplies token cost by roughly (panel size + judge calls × rounds). The algo / no-judge path is the exception: zero judge token cost, zero judge latency, since it skips the LLM judge call entirely. Panel model cost is the only expense.

---

## Enable Fusion Globally

Fusion is off by default. Activate it by setting two required environment variables:

| Env var | Required | Purpose |
|---------|----------|---------|
| `HKASK_FUSION_JUDGE_MODEL` | yes | Judge model — either an LLM model name or the literal `algo`. Supports provider prefix. |
| `HKASK_FUSION_PANEL_MODELS` | yes | Comma-separated panel models, 1–8. Each supports provider prefix. |
| `HKASK_FUSION_MODE` | no | Deliberation mode for LLM judges. One of `synthesis`, `best-of-n`, `critique`, `deliberation`, `pi`. Default: `synthesis`. Ignored when `judge: algo`. |
| `HKASK_FUSION_SKILLS` | no | Comma-separated skill anchors for the judge. Default: none. Ignored when `judge: algo`. |
| `HKASK_FUSION_MAX_ROUNDS` | no | Max rounds for `deliberation` mode. Default: `5`. Ignored when `judge: algo`. |
| `HKASK_FUSION_ALGO_METHOD` | no | Algo merge strategy when `judge: algo`. One of `merge`, `vote`. Default: `merge`. Ignored when the judge is an LLM. |
| `HKASK_FUSION_DISABLED=1` | no | Force-disable fusion (overrides all other vars). |

Minimal activation with an LLM judge (uses kask defaults for judge/panel when unset, but explicit is recommended):

```bash
export HKASK_FUSION_JUDGE_MODEL=deepseek-v4-pro
export HKASK_FUSION_PANEL_MODELS=Kimi2.7,Qwen3.7 Max,GLM5.2,Minimax3
```

With an explicit deliberation mode and skill anchor:

```bash
export HKASK_FUSION_JUDGE_MODEL=DI/deepseek-v4-pro
export HKASK_FUSION_PANEL_MODELS=OR/auto,KC/anthropic/claude-sonnet-4.5,DI/qwen/qwen3
export HKASK_FUSION_MODE=critique
export HKASK_FUSION_SKILLS=pragmatic-semantics
```

With the algo / no-judge path — `mode`, `skills`, and `max_rounds` are ignored:

```bash
export HKASK_FUSION_JUDGE_MODEL=algo
export HKASK_FUSION_PANEL_MODELS=KC/qwen/qwen3-235b-a22b-2507,DI/google/gemma-4-31b-it-turbo
```

**Default model set** (when env vars are unset but fusion is enabled via `FusionConfig::kask_default()`):

| Role | Model |
|------|-------|
| Judge | `deepseek-v4-pro` |
| Panel | `Kimi2.7`, `Qwen3.7 Max`, `GLM5.2`, `Minimax3` |

---

## Provider Prefix Routing

Every model name — judge or panelist — may carry a 2-letter provider prefix. Unprefixed names use the configured default provider. The `algo` judge value is not a model name and does not accept a prefix.

| Prefix | Provider |
|--------|----------|
| `DI/` | DeepInfra |
| `FA/` | fal.ai |
| `TG/` | Together AI |
| `OR/` | OpenRouter |
| `KC/` | KiloCode |
| `CL/` | Cline |

Mixed-provider panels are supported and encouraged for epistemic diversity. Example: a DeepInfra judge with an OpenRouter auto-routed panelist, a KiloCode Claude, and a DeepInfra Qwen:

```bash
HKASK_FUSION_JUDGE_MODEL=DI/deepseek-v4-pro
HKASK_FUSION_PANEL_MODELS=OR/auto,KC/anthropic/claude-sonnet-4.5,DI/qwen/qwen3
```

If a panel model fails to resolve or generate, the orchestrator logs a warning at `reg.inference` and drops it from the round. If **all** panel models fail, the orchestrator returns `InferenceError::Generation("All panel models failed")`.

---

## Choose a Judge Strategy

The `judge` field determines which family processes panel responses. When the judge is an LLM model name, the `mode` field selects one of five deliberation modes. When the judge is `algo`, the mode is ignored and an algorithmic merge runs instead.

> **When `judge: "algo"`, the `mode`, `skills`, and `max_rounds` fields are all ignored** — the algorithmic merge has its own logic and does not accept methodology anchoring or round caps.

### LLM Deliberation Modes

Set via `HKASK_FUSION_MODE` or the `mode:` field in a manifest's `fusion:` block. Only applies when `judge` is an LLM model name.

#### `synthesis` (default) — 1 round

The judge composes a unified response incorporating the strongest elements from each panelist and explicitly resolves contradictions. Use this as the general-purpose default when you want a merged answer rather than a picked one.

#### `best-of-n` — 1 round

The judge evaluates all panel responses and outputs **only the chosen response verbatim** — no commentary, no synthesis, no justification. Use this when you want one panelist's answer, not a merge. With **two or more panelists**, the judge votes twice — once with candidates in dispatch order, once reversed — and the two picks are compared by matching the verbatim output back to its source response. Agreement yields high confidence; disagreement flags position bias and is logged at `reg.fusion` (Zheng et al. 2024, arXiv:2406.07791). A single panelist skips the swap (one judge call). The two judge calls run concurrently (`futures_util::join!`), so swap-revote costs 2× tokens but only ~1× latency over a single call.

##### Measuring whether swap-revote is justified

Swap-revote doubles the judge token cost of `best-of-n`. Before relying on it, measure whether your judge actually has position bias severe enough to warrant the second call:

1. Assemble a **fixed panel-output fixture** — a canned set of panel responses to a known prompt (so the only variable is display order, not panel output variance).
2. Run `best-of-n` on that fixture in **2 display orderings** (dispatch order and reversed). The harness in `fusion_orchestrator::tests::best_of_n_bias_harness_*` demonstrates the mechanism with mock judges.
3. **If the pick changes** between orderings → position bias is present, swap-revote is justified — keep it.
4. **If the pick is stable** across orderings → the judge is order-invariant for this workload; simplify to **order-randomization** (1 judge call, permuted display order) to halve the cost.

The decision is per-judge and per-workload — re-measure when you change the judge model or the panel composition. The swap-revote `agree`/`disagree` verdicts logged at `reg.fusion` provide ongoing observational data: a sustained `agree` rate near 100% is evidence the second call is not earning its cost.

#### `critique` — 2 rounds

1. **Round 1:** Judge produces a draft synthesis from panel responses.
2. **Round 2:** Panel critiques the draft (weaknesses, gaps, contradictions); judge revises into a final synthesis.

Use this for tasks where a draft-then-refine loop improves quality — design reviews, reasoning chains, documentation. Matches the diagnosis loop and the essentialist 3-gate pattern.

#### `deliberation` — ≤ N rounds (default 5)

Multi-round with a **structured judge stabilization verdict**. Each round, the judge emits `{"converged": true, "synthesis": "…"}` or `{"converged": false, "follow_up": "…"}` (parsed structurally via the JSON-tolerant parser), deciding convergence from the responses it reads — not from a prose prefix. Each round:

- The judge reads the current panel responses and self-reports whether the discussion has stabilized.
- If `converged: true`: the judge's `synthesis` field is the final answer.
- If `converged: false`: the judge's `follow_up` field is sent to the panel, and the loop continues.

If `max_rounds` is reached without convergence, the judge is forced to synthesize from the last round. Configure the cap with `HKASK_FUSION_MAX_ROUNDS` (default `5`). This replaces the former `FOLLOW_UP:` string-prefix self-report: the structured verdict is parsed robustly rather than matched as a prose prefix. Use this for hard problems where iterative refinement surfaces information a single pass misses.

#### `pi` (Plan-Implement) — 2 phases

1. **Phase 1 (Plan):** Panel proposes high-level strategies (architecture, key decisions, tradeoffs — no implementation details). Judge synthesizes a unified strategy plan.
2. **Phase 2 (Implement):** The strategy plan is sent back to the panel, which proposes concrete implementation steps (files, functions, tests, sequencing). Judge synthesizes a unified implementation plan.

Use this for engineering tasks where strategy and execution should be separated — refactors, feature design, architectural work. Matches the `refactor-service-layer` and `improve-codebase-architecture` skills.

### Algo / No-Judge Methods

When `judge: "algo"`, the orchestrator dispatches the panel in parallel — exactly as in the LLM-judge modes — then merges responses algorithmically instead of calling an LLM judge. Zero judge token cost, zero judge latency. See [Algo / No-Judge Methods](#algo--no-judge-methods) below for the merge algorithm, when to use it, and the extensibility story for future algorithmic methods.

---

## Algo / No-Judge Methods

`judge: algo` selects the **algo / no-judge** family — a set of deterministic, algorithmic merge strategies that process panel responses without an LLM judge call. Two methods are implemented (recursive JSON `merge` and majority `vote`), selected via the `algo_method` field (see [Algo / No-Judge Methods](#algo--no-judge-methods) below).

### The Merge Algorithm (current)

The current and only algo / no-judge method is `merge_json_values` — a recursive JSON union that preserves both viewpoints rather than picking one or synthesizing a third:

- **Objects** merge by key (recursively).
- **Arrays** concatenate with case-insensitive deduplication.
- **Diverging strings** are annotated as `[A:... B:...]` so both values survive.
- **Null + value** → the non-null value wins.
- **Equal scalars** → one copy retained.

The orchestrator parses each panelist's response as JSON (tolerating markdown fences and surrounding prose via `parse_json_lenient`), then reduces them through `merge_json_values`. Panel token usage is aggregated and reported in the `InferenceResult`.

**When to use.** Epistemic-integrity tasks where both models' outputs should be preserved rather than judged — e.g. classification steps where agreement and disagreement are both signal, memory formation where diverging extractions carry provenance value. The merge algorithm is designed for **2-model** panels; annotation quality degrades with 3+ panelists (the `[A:... B:...]` annotation assumes a binary pair).

Minimal config:

```yaml
fusion:
  judge: algo
  panel:
    - KC/qwen/qwen3-235b-a22b-2507
    - DI/google/gemma-4-31b-it-turbo
```

Or via env vars:

```bash
export HKASK_FUSION_JUDGE_MODEL=algo
export HKASK_FUSION_PANEL_MODELS=KC/qwen/qwen3-235b-a22b-2507,DI/google/gemma-4-31b-it-turbo
```

**Limitations of the merge algorithm.**

- Designed for a **2-model** merge — annotation quality degrades with 3+ panelists.
- Requires **JSON** panel responses; non-JSON output falls back to `null`.
- **No skill anchoring** — the merge is methodology-agnostic. Use an LLM-judge mode (`synthesis`, `critique`, etc.) when you need methodology-anchored evaluation.
- **No iterative refinement** — the merge is single-pass with no convergence check or follow-up rounds.

### Extensibility — Future Algo Methods

The `algo` judge value is a **family of algorithmic merge strategies**, selected by the `algo_method` field. Two methods are implemented:

| Method | `algo_method` | Merge strategy | Use case |
|--------|---------------|---------------|---------|
| **Recursive JSON merge** (default) | `merge` | Union with case-insensitive dedup, diverging strings annotated `[A:... B:...]` | Epistemic-integrity classification, memory formation — preserve both viewpoints. Designed for 2-model panels |
| **Majority vote** | `vote` | Object fields take the majority value across panelists (recursively); array items kept only if a majority of panelists include them; scalars take the majority | Classification with confidence scoring — agreement frequency as signal. Scales beyond 2 panelists where `merge`'s pairwise annotation degrades |

Potential future algo / no-judge methods (not yet implemented):

| Candidate method | Merge strategy | Use case |
|-------------------|---------------|---------|
| **Set intersection** | Keep only fields where panelists agree (case-insensitive) | High-confidence extraction — discard diverging extractions |
| **First-wins** | Take the first panelist's response, drop the rest | Deterministic priority ordering — one model is authoritative |
| **Schema-validated merge** | Merge per a declared JSON schema (type-checked, required-field enforced) | Structured extraction with validation — reject malformed panel responses |

Adding a new algo method requires:

1. Implementing the merge function in `crates/hkask-inference/src/fusion_orchestrator.rs`.
2. Adding an `AlgoMethod` variant in `crates/hkask-types/src/fusion.rs` (with the `enum_snake_str!` arm) and a matching arm in `orchestrate`'s algo dispatch.
3. Documenting the method's merge semantics, limitations, and recommended panel size.

The `algo` judge value routes through the `algo_method` sub-selector (`merge` by default, `vote` for majority tally). Both methods are deterministic, zero-cost (no LLM judge call), and aggregate panel token usage. Set `algo_method: vote` in the `fusion:` block (or `HKASK_FUSION_ALGO_METHOD=vote`) to use majority vote.

---

## Anchor the Judge with Skills

The LLM judge can be anchored on hKask's pragmatic methodologies via `HKASK_FUSION_SKILLS` (comma-separated). Each anchor injects a compact methodology prompt into the judge's system context, steering its reasoning without forcing a rigid template.

> **Algo / no-judge ignores skill anchors.** When `judge: "algo"`, the `skills` field has no effect — the algorithmic merge is methodology-agnostic. Use an LLM-judge mode when you need methodology-anchored evaluation.

| Skill anchor | Methodology injected |
|--------------|---------------------|
| `pragmatic-semantics` | IS vs OUGHT, certainty levels, provenance, constraint hierarchy |
| `pragmatic-cybernetics` | Feedback loops, variety engineering, homeostasis |
| `pragmatic-laziness` | Path of least action, delete before adding |
| `coding-guidelines` | Karpathy's 4 principles: think first, simplicity, surgical changes, goal-driven |
| `deep-module` | Ousterhout deletion test, interface minimalism (≤7 public items) |
| `essentialist` | 3-gate challenge loop: Exist → Surface → Contract |
| `superforecasting` | Fermi decomposition, Bayesian updating, dragonfly-eye synthesis, calibrated probabilities |
| `mcda` | Weighted scoring, compensation masking, sensitivity analysis |
| `tdd` | Red-Green-Refactor, contract-first, vertical tracer-bullet |
| `bug-hunt` | Weinberg quality, Beizer taxonomy, Bach/Bolton heuristics, exploratory charters |
| `diagnose` | Cybernetic debugging: reproduce → hypothesize → instrument → fix → regression-test |
| `falsifiability` | Popper/Platt/Chamberlin: rule out untestable, eliminate falsified, corroborate survivors |
| `grill-me` | Socratic interrogation: Recall → Mechanism → Rationale → Edge Cases → Synthesis |
| `idiomatic-rust` | Hoare: validating newtypes, ownership DAG, error domains, many small traits |
| `improve-codebase-architecture` | Ousterhout: deletion test, deep modules, surface shallow modules |
| `metacognition` | Goal decomposition, progress self-assessment, perspective rotation, GEPA improvement |
| `refactor-service-layer` | Strangler fig migration, deep-module discipline, vertical tracer-bullet TDD |
| `self-critique-revision` | Draft → critique against criteria → revise. Iterative cycle, no first-draft acceptance. Subsumes the former `review` skill (use `quality_criteria: [contradictions, unsupported_claims, logical_gaps, calibration]` for the review-only path). |

Example: a judge anchored on cybernetics and essentialism:

```bash
HKASK_FUSION_SKILLS=pragmatic-cybernetics,essentialist
```

The full methodology text for each skill is defined in `skill_prompt()` in `crates/hkask-inference/src/fusion_orchestrator.rs`. To add a new anchor:
1. Add the variant to the `FusionSkill` enum in `crates/hkask-types/src/fusion.rs` (with `#[serde(rename = "...")]`).
2. Add the variant to the `enum_snake_str!` macro invocation (generates `as_str()` and `FromStr`).
3. Add a matching arm in `skill_prompt()` in `fusion_orchestrator.rs` with the compact methodology prompt.

> **Note:** `hypothesis-framer` appears in the skill catalog but is not yet a `FusionSkill` variant. Adding it requires extending the enum, the `enum_snake_str!` macro invocation, and the `skill_prompt` match.

---

## Configure Per-Skill Fusion (Manifests)

A skill's flow manifest can declare its own `FusionConfig` via a `fusion:` block, overriding the global env-var config for all steps in that skill's pipeline. This lets each skill pick its own judge, panel, mode, skill anchors, and round cap without touching other skills.

```yaml
# registry/manifests/superforecasting.yaml
fusion:
  judge: deepseek-v4-pro
  panel:
    - Kimi2.7
    - Qwen3.7 Max
    - GLM5.2
    - Minimax3
  mode: synthesis
  skills:
    - superforecasting
  max_rounds: 5
```

The same structure works for the algo / no-judge path — `mode`, `skills`, and `max_rounds` are accepted but ignored:

```yaml
# registry/manifests/memory_remember.yaml
fusion:
  judge: algo
  panel:
    - KC/qwen/qwen3-235b-a22b-2507
    - DI/google/gemma-4-31b-it-turbo
```

The full `FusionConfig` shape (5 fields, YAML-clean):

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `judge` | string | required | LLM model name (supports provider prefix) or the literal `algo` for the no-judge path. |
| `panel` | string[] | required | 1–8 panel models; each supports provider prefix. |
| `mode` | `synthesis` \| `best-of-n` \| `critique` \| `deliberation` \| `pi` | `synthesis` | LLM judge deliberation mode. Ignored when `judge: algo`. |
| `skills` | string[] | `[]` | Skill anchors to inject into the LLM judge. Ignored when `judge: algo`. |
| `max_rounds` | u32 | `5` | Cap for `deliberation` mode. Ignored when `judge: algo`. |
| `algo_method` | `merge` \| `vote` | `merge` | Algo merge strategy when `judge: algo`. Ignored when the judge is an LLM. |

### Resolution Priority

When a step runs, fusion config is resolved in this order (highest priority first):

1. `step.fusion: Some(false)` → **bypass fusion** (single-model inference). Used for deterministic rubric evaluation and convergence checks.
2. `step.fusion: Some(true)` or `None` → inherit the manifest config.
3. `manifest.fusion: Some(config)` → per-manifest config (carried via `LLMParameters.fusion_config`).
4. `manifest.fusion: None` → global config (`HKASK_FUSION_*` env vars).
5. `params.bypass_fusion: true` → **bypass everything** (chat path, condenser, daemon narratives, summarization).

### Per-Step Bypass

Set `fusion: false` on individual steps to keep them single-model while the rest of the manifest uses fusion. This is the standard pattern for convergence checks and quality gates, which must be deterministic:

```yaml
steps:
  - id: converge
    kind: select
    fusion: false        # deterministic single-model rubric
    template: converge.j2
```

The implementation lives in `execute_select()` in `crates/hkask-templates/src/executor.rs`.

---

## Where Fusion Applies (and Doesn't)

Fusion routing is decided per inference call by `bypass_fusion` on `LLMParameters`:

| Call path | Routes through fusion? | Why |
|-----------|------------------------|-----|
| Skill `select` steps (fusion active) | ✅ yes | Skills benefit from multi-model deliberation |
| Skill tool invocations | ✅ yes | Same as above |
| `kask chat` interactive chat | ❌ no | `bypass_fusion = true` — user's chosen model is used directly |
| `kask api` chat stream | ❌ no | `bypass_fusion = true` |
| Condenser / summarization | ❌ no | Always bypass — cost/latency sensitive |
| Daemon narratives | ❌ no | Always bypass |
| Algo / no-judge steps (`fusion: true`, `judge: algo`) | ✅ yes | The `algo` judge is a fusion path — it dispatches the panel in parallel and merges JSON via `merge_json_values` |

Chat intentionally bypasses fusion so the user's explicitly-chosen model answers directly, while skills (which run autonomously) route through the fusion panel for higher quality. The algo / no-judge path follows the same routing rules as LLM-judge fusion — it is not a separate bypass mechanism.

---

## Operate Fusion

### REPL commands

Inside the REPL (`kask repl` or the TUI):

| Command | Effect |
|---------|--------|
| `/fusion` | Print fusion status (active/inactive, judge, panel, mode) |
| `/fusion on` | Activate fusion for the session |
| `/fusion off` | Deactivate fusion for the session |

### Startup banner

When fusion is configured and the judge model is set, `kask` prints a banner on startup:

```
  ⚡ Fusion mode active — model: deepseek-v4-pro
     4 panel models judged by deepseek-v4-pro (mode: synthesis)
```

When the algo / no-judge path is active:

```
  ⚡ Fusion mode active — model: algo
     2 panel models merged by algo (no LLM judge)
```

This is emitted by `check_fusion_startup()` in `crates/hkask-cli/src/main.rs` — a P9 proactive cost-safety check so you never accidentally run fusion with an unintended model.

### Doctor check

`kask doctor` verifies the fusion judge is reachable:

```
Fusion Model
────────────
  ✅ Fusion judge reachable — deepseek-v4-pro
```

When `judge: algo`, the doctor check skips the reachability test (there is no model to reach) and reports:

```
Fusion Model
────────────
  ⚠️ Fusion judge set to "algo" — no model to verify (algorithmic merge)
```

If the judge is unreachable, doctor reports `❌ Fusion judge NOT reachable` or `⚠️ Could not verify fusion model: <error>`. Fix connectivity or credentials before relying on fusion.

### Diagnostics

Fusion emits tracing spans at `target: "reg.inference"`:

- `Fusion orchestration starting` — mode, judge, panel count, skill count
- `Algo judge merge complete` — algo / no-judge path, panel count
- `Critique round 1 complete` / `Critique round 2` — critique mode round boundaries
- `Judge requested follow-up` / `Deliberation converged` — deliberation mode round decisions
- `P-I Phase 1 complete — strategy synthesized` — plan-implement phase boundary
- `Panel model resolution failed` / `Panel model generation failed` — per-panelist failures (warnings, not fatal unless all fail)

Filter with `RUST_LOG=reg.inference=info` to watch fusion in action.

---

## Disambiguation: hKask Fusion vs. OpenRouter Fusion Plugin

hKask's fusion orchestrator is a **client-side** multi-model deliberation engine — it dispatches panel calls to any `InferencePort` backend, collects responses, and runs the judge. This is distinct from OpenRouter's server-side Fusion plugin: hKask's orchestrator is provider-agnostic and runs entirely on the hKask side, so it works across all supported providers (DeepInfra, fal.ai, Together, OpenRouter, KiloCode, Ollama, Cline), not just OpenRouter.

---

## Reference: Crates and Types

| Artifact | Location |
|----------|----------|
| `FusionConfig`, `FusionMode`, `FusionSkill` types | `crates/hkask-types/src/fusion.rs` |
| Env-var parser (`parse_fusion_config`) | `crates/hkask-inference/src/config.rs` |
| Orchestrator entry (`orchestrate`), 5 LLM mode implementations, `ALGO_JUDGE` constant, `algo_merge()`, `merge_json_values()` | `crates/hkask-inference/src/fusion_orchestrator.rs` |
| Router fusion override (`effective_model`, `orchestrate_fusion`) | `crates/hkask-inference/src/inference_router/` |
| Per-manifest `fusion:` block (`BundleManifest.fusion`, `BundleManifestStep.fusion`) | `crates/hkask-templates/src/bundle/manifest.rs` |
| Per-step resolution logic | `crates/hkask-templates/src/executor.rs` (`execute_select`) |
| `LLMParameters.fusion_config` carrier | `crates/hkask-types/src/template.rs` |
| `NonEmptyVec` (panel invariant) | `crates/hkask-types/src/fusion.rs` |

Types live in `hkask-types` (not `hkask-inference`) so manifests and `LLMParameters` can carry fusion config without a dependency on the inference crate.

The `ALGO_JUDGE` constant (`"algo"`) and the `algo_merge()` / `merge_json_values()` functions are the implementation surface for the algo / no-judge family. The `orchestrate()` entry point checks `fusion.judge.to_lowercase() == ALGO_JUDGE` and routes to `algo_merge()` before the `FusionMode` match — the algo path bypasses the mode dispatch entirely.

---

## See Also

- [Architecture: Fusion — Multi-Model Deliberation](../architecture/core/hKask-architecture-master.md#fusion--multi-model-deliberation) — canonical architecture reference
- [Cognition and Replica: Fusion System Design Recommendations](../explanation/cognition-and-replica.md) — design rationale (why no partial inheritance, why no `fusion_mode` shorthand, why algo judge subsumes dual-model)
- [Install and Configure](install-and-configure.md) — env-var setup including `HKASK_FUSION_DISABLED`
- [Skills and Composition](skills-and-composition.md) — manifest authoring and the `fusion:` block in context