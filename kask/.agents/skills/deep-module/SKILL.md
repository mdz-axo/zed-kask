---
name: deep-module
visibility: public
description: "Module design discipline based on John Ousterhout's 'A Philosophy of Software Design.' Apply the deletion test to evaluate whether a module deserves to exist: delete the callers — if complexity reappears, extract. Delete the module — if complexity vanishes, don't create it. Enforces depth (high benefit/cost ratio), interface minimalism (≤7 public functions), and dependency direction."
---


# Deep Module

Module design discipline based on John Ousterhout's *A Philosophy of Software Design*. Apply the deletion test to evaluate whether a module deserves to exist: delete the callers — if complexity reappears, extract. Delete the module — if complexity vanishes, don't create it. Enforces depth (high benefit/cost ratio), interface minimalism (≤7 public functions), and dependency direction.

## When to Use

- You are evaluating whether an existing module is deep (small interface, much behavior) or shallow (large interface, thin behavior)
- You need to decide whether a module deserves to exist — should it be kept, extracted, deepened, merged, or deleted
- You are designing a new module interface and want to maximize depth from the start
- You need to check whether module-depth design recommendations have converged across iteration cycles
- You suspect a module is a pass-through, data bag, or abstraction-for-one
- You want to enforce the ≤7 public function target and unified error/config design

## Instructions

### 1. Assess Module Depth

1. Count every public item in the module mechanically: public functions (`pub` / `pub(crate)`), public types (struct, enum, type alias), public traits, public constants, and public sub-modules. Do not count impl blocks, tests, or re-exports.
2. Estimate the behavior encapsulated: count non-comment, non-blank implementation lines (including private helpers and impl blocks), list invariants enforced, and list complexity managed on behalf of callers.
3. Compute the depth score: `Behavior Lines / (Public Functions + Public Types + Public Traits + Public Constants)`.
4. Classify the module: Deep (100+), Adequate (50–99), Shallow (20–49), or Very Shallow (0–19).
5. Identify red flags: more public functions than private (pass-through suspicion), more public types than functions (data bag), all public functions delegating to a single dependency (pass-through), zero invariants enforced (no encapsulation), or single consumer (inline candidate).
6. Produce recommendations even if the depth score is acceptable — flag all red flags.

### 2. Execute the Deletion Test

1. **Direction 1 — Caller's perspective**: For each caller, imagine inlining the module's logic at the call site. Determine what complexity would reappear: state management, error handling, invariant enforcement, coordination logic. Assess whether the inline replacement is trivial (a few lines) or substantial.
2. **Direction 2 — Module's perspective**: For each public function, imagine replacing it with a direct call to the module's dependency. Determine whether any behavior would be lost, any invariants broken, or any complexity management eliminated.
3. Apply the decision matrix: complexity reappears + behavior lost → **EXTRACT**; complexity reappears + complexity vanishes → **DEEPEN**; trivial replacement + behavior lost → **MERGE**; trivial replacement + complexity vanishes → **DELETE**.
4. Produce a definitive recommendation with a rationale citing concrete complexity examples, not vague claims. If the module has a single consumer, flag it as an inline candidate.

### 3. Design the Deep Module Interface

1. Define the core operation — the one thing this module does. If you cannot describe it in one sentence, the module is too broad.
2. Add public functions only when the operation cannot be accomplished by combining the core operation with something else, serves a different caller need, and would cause significant complexity if callers implemented it themselves. Target ≤7 public functions total; if exceeded, split the module.
3. Design minimal public types: prefer enums over structs with many optional fields; expose only what callers need.
4. Hide information: keep algorithms, data structures, caching, internal state, business rules, validation logic, and the identity of dependencies private.
5. Design one unified error enum per module: map dependency errors to module-level variants (never leak dependency error types), add context to each variant.
6. Design one config struct: passed at construction time, validated on construction (fail early), with defaults for optional values.
7. Project the depth score: `Estimated Behavior Lines / (Public Functions + Public Types)`. Target ≥100 (Deep), minimum ≥50 (Adequate).

### 4. Check Convergence

1. Evaluate whether the deletion test passes (removing the module would cause complexity to reappear).
2. Check interface depth: public surface ≤7 items with justified exceptions.
3. Verify dependency direction: dependencies are acyclic and point toward stability.
4. Assess caller benefit: callers genuinely benefit from the abstraction (not pass-through).
5. Check that depth-improvement recommendations are specific and actionable.
6. Compute the convergence metric in [0,1]: start at 1.0, subtract for each satisfied check. 0.0 = converged (passes deletion test, ≤7 items, minimal interface); 0.15 = minor depth opportunities remain; 0.50 = significant shallow modules identified but not acted on; 1.00 = no depth analysis performed.

## Registry Templates

| Template | Type | Purpose |
|----------|------|--------|
| `deep-module-assess.j2` | `KnowAct` | Assess module depth: enumerate public interface items, evaluate behavior complexity, compute depth score, classify as Deep/Adequate/Shallow/Very Shallow. Identify interface cost drivers and behavior gaps. |
| `deep-module-delete.j2` | `KnowAct` | Execute the deletion test in both directions: caller's perspective (complexity reappears?) and module's perspective (complexity vanishes?). Produce a definitive keep/extract/don't create recommendation. |
| `deep-module-design.j2` | `KnowAct` | Design a deep module interface from deletion test results. Minimize public surface, hide information, design for caller mental model, unify config and errors. Produce a module specification with ≤7 public functions. |
| `deep-module-convergence-check.j2` | `KnowAct` | Compute normalized convergence metric for module-depth cycles. Outputs `convergence_metric` in [0,1], where 0 means deletion/surface/depth checks indicate an acceptable deep-module shape. |

## Fusion Mode

This skill supports **fusion mode** via the `fusion:` block in its flow manifest.
When enabled, all analysis steps route through a multi-model panel — either with
LLM judge synthesis or the **algo / no-judge** path (`judge: algo`) for deterministic
JSON merge without an LLM judge call. This skill uses **synthesis mode** — Compose
module assessments.

The convergence check step has `fusion: false` to ensure deterministic rubric
evaluation uses single-model inference.

## Constraints

- All templates are `KnowAct` type with `Public` visibility.
- Energy caps: assess, delete, and design templates have `energy_cap: 6144`; convergence-check has `energy_cap: 2048`.
- Count public items mechanically — do not guess. Estimate behavior conservatively, erring toward undercounting.
- Apply both directions of the deletion test — never skip either.
- No more than 7 public functions per module. If the design exceeds 7, split the module.
- One error type per module — map, do not leak, dependency errors. One config struct, validated at construction.
- Hide everything that callers do not strictly require.
- Jinja2 sandboxed execution: no arbitrary Python code, no file system access, no network calls, no environment variable access when safety mode is enabled.
- Handle missing variables gracefully (leave as-is or use default if specified).
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.