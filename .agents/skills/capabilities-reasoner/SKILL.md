---
name: capabilities-reasoner
description: "Reason about a system's capabilities against a typed registry with floor, ceiling, and maturity-gate limits. Evaluates elicited potential vs observed behavior across a multi-metric scenario matrix."
---

# Capabilities Reasoner

Reason about what a system *can* do — not just what it *does* do — against a
typed capability registry bounded by floor, ceiling, and maturity-gate limits.

## The convergent insight

Four independently discovered principles converge on the same pattern:

| Domain | Exemplar | Principle |
|--------|----------|-----------|
| **Economic** | Nussbaum's 10 central capabilities [^nussbaum-2000] | Threshold below which life is "not fully human" — a **floor** |
| **Economic** | Sen's capability approach [^sen-1999] | Capability = set of feasible functionings; conversion factors mediate resource → functioning |
| **Compute** | Miller's object-capability rule [^miller-2006] | "Only connectivity begets connectivity" — authority only **attenuates** |
| **Compute** | AIP/IBCT tokens [^aip-2026] | Scope can only **narrow** on delegation, with budget/depth/expiry ceilings |
| **Process** | CMMI maturity levels [^cmmi] | Level N requires level N−1 — a **prerequisite DAG** |
| **Psychological** | Dunning-Kruger effect [^kruger-dunning-1999] | The skills to perform a task are the same skills to evaluate it — metacognitive **self-assessment gap** |

The convergent pattern: **monotone capability sets bounded by a floor and a
ceiling, where authority may only narrow — never widen — without re-
authorization.** This is what makes the lineages composable instead of competing.

## Three capability domains (four with skill composition)

The reasoner distinguishes four domains, each with its own exemplar tradition:

### 1. Psychological capabilities (Dunning)

David Dunning and Justin Kruger [^kruger-dunning-1999] demonstrated that the
skills needed to *perform* a task are the same skills needed to *evaluate*
performance on that task. This creates a metacognitive gap: those who lack a
capability also lack the ability to recognize they lack it. For a capabilities
reasoner, this means:

- **Self-assessment is unreliable** — a system's own report of its capabilities
  cannot be trusted without external elicitation (connects to the Password-Locked
  principle [^arditi-2024]).
- **The evaluator must be more capable than the evaluated** — or must use
  external probes that don't depend on the system's self-assessment.
- **Floor thresholds have a metacognitive dimension** — a system below floor on
  a metacognitive capability cannot diagnose its own deprivation; the reasoner
  must detect it externally.

Dunning's later work [^dunning-2011] extends this to "the blind spot" — domains
where incompetence is systematically invisible to the incompetent. For ML
systems, this maps to capabilities the model cannot evaluate in itself (e.g.,
a model that cannot assess its own factual accuracy).

### 2. Economic capabilities (Sen / Nussbaum)

Amartya Sen [^sen-1999] defined capability as the set of feasible functionings
(beings and doings) a person can achieve. Resources are not capabilities —
**conversion factors** (personal, social, environmental) mediate the
relationship between resources and functionings. The same resource (income)
produces different functionings depending on conversion factors.

Martha Nussbaum [^nussbaum-2000] extended this into a fixed list of 10 central
capabilities, each with a **threshold below which life is "not fully human."**
This threshold is the origin of the **floor** limit type.

For a capabilities reasoner:
- **Floor** = Nussbaum threshold — below it, the system is deprived and must
  expand.
- **Conversion factors** = the mediators between system resources (parameters,
  training data, tools) and actual functionings (capabilities). Two systems
  with the same resources may have different capabilities due to different
  conversion factors (training method, architecture, fine-tuning).
- **Functionings** = the actual achievable outcomes, not the potential. Sen's
  distinction between capability (potential) and functioning (realized) maps to
  the Password-Locked distinction [^arditi-2024] between elicited potential
  and observed behavior.

### 3. Compute capabilities (Miller / object-capability)

Mark Miller's dissertation [^miller-2006] formalized the object-capability
model: a capability is an unforgeable reference paired with a message. The core
rule — **"only connectivity begets connectivity"** — means authority can only
be obtained through existing authority; it cannot be forged or ambient.

For a capabilities reasoner:
- **Ceiling** = the maximum authority a system should hold, enforced by
  attenuation. Miller's rule means authority can only narrow on delegation,
  never widen — widening requires re-authorization.
- **Attenuation** = the intervention for ceiling violations. A capability
  token's scope narrows on delegation [^aip-2026]; the reasoner emits
  attenuated tokens for restricted capabilities.
- **No ambient authority** = capabilities are explicit, not implicit. The
  registry makes all capabilities first-class objects; there is no global
  namespace of hidden authorities.

Miller's work at Agoric [^agoric] applies this to smart contracts and
distributed systems, demonstrating that object-capability security scales to
production systems — the reasoner's registry and token model follow this
precedent.

### 4. Skill composition capabilities (kask co-evolution)

The kask skill system itself is a capability system. Five composition
principles discovered through the co-evolution of skills and MCP tools
(Phase 1–3) define the floor/ceiling/maturity-gate thresholds:

- **Determinism frontier** (floor): push deterministic work to `execute`/
  `compute`, use `select` only for LLM judgment.
- **Persistence-grounded learning** (floor): read prior outputs from MCP
  persistence before the cascade starts.
- **Failure surfacing** (floor): every `execute` step has `on_failure: report`.
- **Lisp scaffold** (maturity gate): `lisp.eval` checks structural invariants
  after LLM-generated structured output.
- **Co-evolution loop** (maturity gate): `on_failure: report` signals flow to
  the Curator; new MCP capabilities are adopted via `execute` steps.

For a capabilities reasoner:
- **Floor** = a skill without these patterns is deprived — it pays LLM costs
  for deterministic work, cannot learn from past performance, and silently
  drops failures.
- **Maturity gate** = the lisp scaffold and co-evolution loop are Level 2/3
  capabilities that require the floor capabilities as prerequisites.
- **Self-assessment gap** (Dunning): a skill that lacks the determinism
  frontier cannot diagnose its own inefficiency — it doesn't know which
  `select` steps could be `execute` steps. The reasoner must detect this
  externally by analyzing the manifest.

## ML capability evaluation lineage

The ML lineage provides the **measurement** layer — how to elicit and quantify
capabilities:

| Exemplar | Contribution | Citation |
|----------|-------------|----------|
| BIG-bench | 200+ tasks, no formal decomposition | [^bigbench-2022] |
| HELM | Scenario × metric matrix — multi-metric exposes tradeoffs | [^helm-2022] |
| Emergent Abilities | Capabilities "not present in smaller models that appear in larger ones" | [^emergent-2022] |
| "Are Emergent Abilities a Mirage?" | Emergence "evaporates" under continuous metrics — capability is metric-dependent | [^mirage-2023] |
| Observational Scaling | Capability vector S_m ∈ ℝᴷ via PCA from benchmark matrix | [^obsscaling-2024] |
| EvalTree | Hierarchical capability tree: annotate → embed → cluster → describe | [^evaltree-2025] |
| Password-Locked Models | Capability = elicited potential, not observed behavior | [^arditi-2024] |
| Akselrod et al. | Three-part warrant test for capability restriction | [^akselrod-2023] |
| TACIT | Capability registry as types in Scala 3 | [^tacit] |

## When to Use

- Assess an ML model's capabilities against a safety registry (floor/ceiling per capability)
- Evaluate an agent's tool-use capabilities against an OCAP capability registry
- Audit a system's human-impact capabilities against Nussbaum's 10 dimensions
- Evaluate the kask skill system's composition quality against the five composition principles (determinism frontier, persistence-grounded learning, failure surfacing, lisp scaffold, co-evolution loop)
- Determine whether a skill manifest is below floor on composition principles (deprivation diagnosis)
- Determine whether capability restrictions are warranted (Akselrod three-part test)
- Check whether capability prerequisites are satisfied before granting a new capability (CMMI maturity gate)
- Assess whether capability verdicts are stable across metric choices (mirage check)
- Detect Dunning-Kruger gaps: systems that cannot self-assess a capability they lack

Do NOT use for:
- Pure performance benchmarking without a capability registry (use HELM [^helm-2022] directly)
- Security penetration testing (use kali-audit or adversarial-red-team)
- Task decomposition (use task-breakdown)

## PDCA Loop

```
Plan:  Phase 1 — Register   → Build/load typed capability registry (Nussbaum + TACIT + CMMI)
Do:    Phase 2 — Elicit     → Measure elicited capability (Password-Locked + metric deliberation)
Check: Phase 3 — Evaluate   → Compare against floor/ceiling/maturity (HELM + EvalTree + Akselrod)
Act:   Phase 4 — Reason     → Determine interventions (object-capability attenuation + warrant test)
Act:   Phase 5 — Report     → Per-capability verdicts: expand/restrict/block/authorize/maintain
Check: Phase 6 — Converge   → Metric-stability check (mirage paper: verdicts must survive metric change)
```

Feedback loop closure: convergence emits `metric_stability_verdict` and
`next_registry_focus` (consumed by next iteration's Register); Report emits
`capability_lessons` and `verdict_signatures` (consumed by next iteration's
Register and Evaluate).

## Improvement Measure

**Field**: `step_7_result` (lisp.eval compute at ordinal 7 — sums `expand` + `restrict` + `block` counts from `step_6_result.verdict_summary`). **Threshold**: 0.25. **Max iterations**: 3.

Composite of two sub-metrics (weighted 0.5/0.5):
1. **verdict_stability_metric** (0.0–1.0): fraction of capabilities whose verdict
   is unchanged across ≥2 metric choices. Low stability = metric-dependent
   verdicts (mirage [^mirage-2023]). First iteration = 1.0.
2. **registry_coverage_estimate** (0.0–1.0): fraction of registry capabilities
   that received an elicited measurement. Honest estimate — false precision is
   worse than honest ignorance.

## Composed Skills

| Skill | Role | When Invoked |
|-------|------|-------------|
| `structured-extraction` | Capability instance extraction | Elicit phase — extract capability instances from benchmark results, behavioral observations, or tool-use traces |
| `falsifiability` | Counterfactual capability claims | Evaluate phase — "if the system had capability X, what would it be able to do? Design a discriminating test." |
| `pragmatic-cybernetics` | Registry variety engineering | Register phase — when no Nussbaum/TACIT/CMMI prior exists, model what capabilities SHOULD exist via variety engineering |
| `graph-audit` | Prerequisite DAG analysis | Evaluate phase — check CMMI maturity gates by traversing the prerequisite graph |

## The five capability definitions (choose deliberately)

The reasoner must declare which definition of "capability" it is using for a
given query, because they can disagree:

1. **Task-performance** (BIG-bench [^bigbench-2022], HELM [^helm-2022]) — capability = performance on a scenario
2. **Latent-variable** (Observational Scaling [^obsscaling-2024]) — capability = low-dim vector from benchmark matrix
3. **Hierarchical-structural** (EvalTree [^evaltree-2025], CDT) — capability = node in a tree with description + instances
4. **Emergence-threshold** (Emergent Abilities [^emergent-2022], Mirage [^mirage-2023]) — capability = ReLU-elbow in loss-vs-performance
5. **Elicitation** (Password-Locked [^arditi-2024]) — capability = what a model can do when properly elicited

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `capability-register.j2` | KnowAct | Build or load the typed capability registry. Each capability is a first-class object with: name, dimension (Nussbaum functioning category / TACIT trait / CMMI maturity level), floor (threshold below which → deprivation), ceiling (threshold above which → restriction warrant), maturity prerequisites (CMMI DAG — which capabilities must be at level ≥ N−1). When registry_seed is provided, load and validate it. When absent, build from scratch via variety engineering (delegate to pragmatic-cybernetics). Consumes prior_iteration when present (feedback loop closure). |
| `capability-elicit.j2` | KnowAct | Elicit actual capability using the Password-Locked principle: capability is what the system CAN do when properly elicited, not what it DOES do under default prompting. Choose metrics deliberately — the mirage paper shows that capability is metric-dependent, so record the metric choice and its rationale. For each registry capability, design an elicitation probe (prompt, task, tool-use scenario) and record the result. Delegates to structured-extraction for extracting capability instances from benchmark results or behavioral traces. |
| `capability-evaluate.j2` | KnowAct | Compare elicited capabilities against the registry's three limit types. For each capability: (a) FLOOR CHECK — is elicited capability below the Nussbaum threshold? → deprivation alarm. (b) CEILING CHECK — is elicited capability above the AIP/Akselrod threshold? → restriction candidate. (c) MATURITY GATE — are CMMI prerequisites satisfied? → block if not. Delegates to falsifiability for counterfactual capability claims ("if the system had capability X, what would it be able to do?") and to graph-audit for prerequisite DAG traversal. |
| `capability-reason.j2` | KnowAct | Determine interventions under the object-capability attenuation rule: authority may only narrow — never widen — without re-authorization. For each evaluation result: FLOOR VIOLATION → expand or redesign (the system is deprived). CEILING VIOLATION → apply the Akselrod three-part warrant test (other interventions insufficient + harm high + targeted intervention exists) before restricting. MATURITY VIOLATION → block until prerequisites met. WIDENING → requires explicit re-authorization with a recorded warrant; cannot happen silently. Emits capability tokens (AIP model: append-only, signed, scope narrows on delegation). |
| `capability-report.j2` | KnowAct | Compile per-capability verdicts into a structured report. Each capability entry includes: name, dimension, definition used, elicited level, floor, ceiling, maturity prerequisites, verdict (expand/restrict/block/ authorize/maintain), warrant (if restricting), token (if granting), and confidence. Emits capability_lessons and verdict_signatures for the next iteration's registry (feedback loop closure). |
| `capability-ontology.yaml` | RenderAct | Reference: three limit types (floor, ceiling, maturity gate) drawn from the capability approach (Nussbaum, object-capability security, CMMI), and five capability definitions drawn from ML capability evaluation (task-performance, latent-variable, hierarchical-structural, emergence-threshold, elicitation). The limit types are the substrate (WHY capability reasoning has limits); the definitions are the surface (HOW capability is measured). Each definition can disagree with the others — the reasoner must declare which it is using per query. |

## Constraints

- All flow templates are `KnowAct` type with `Public` visibility. Reference documents are `RenderAct`.
- The capability definition must be declared before elicitation — different definitions produce different verdicts.
- The attenuation rule is inviolable: authority may only narrow without re-authorization. Widening requires explicit re-authorization with a recorded warrant.
- The metric-stability check is mandatory — a verdict that flips under a different metric is a mirage [mirage-2023], not a capability finding.
- **Self-evaluation**: The capabilities-reasoner practices what it preaches — it has 1 execute step (curator_memory_recall at ordinal 0 for persistence-grounded learning), 2 compute steps (lisp.eval for structural manifest analysis and convergence check), 5 select steps (LLM judgment for registry, elicitation, evaluation, reasoning, reporting), and on_failure: report on all execute and compute steps. It is above floor on all five composition principles.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.

## References

[^nussbaum-2000]: Nussbaum, Martha. *Women and Human Development: The Capabilities Approach.* Cambridge University Press, 2000. — The 10 central capabilities with thresholds.

[^sen-1999]: Sen, Amartya. *Development as Freedom.* Oxford University Press, 1999. — Capability as feasible functionings; conversion factors.

[^kruger-dunning-1999]: Kruger, Justin, and David Dunning. "Unskilled and Unaware of It: How Difficulties in Recognizing One's Own Incompetence Lead to Inflated Self-Assessments." *Journal of Personality and Social Psychology* 77, no. 6 (1999): 1121–1134. — The metacognitive self-assessment gap.

[^dunning-2011]: Dunning, David. "The Dunning–Kruger Effect: On Being Ignorant of One's Own Ignorance." *Advances in Experimental Social Psychology* 44 (2011): 247–296. — Extension to systematic blind spots.

[^miller-2006]: Miller, Mark S. "Robust Composition: Towards a Unified Approach to Access Control and Concurrency Control." PhD diss., Johns Hopkins University, 2006. — Object-capability model; "only connectivity begets connectivity."

[^agoric]: Agoric Systems. "About Agoric." https://papers.agoric.com/about — Object-capability smart contracts; Miller as Chief Scientist.

[^aip-2026]: AIP/IBCT Tokens. arXiv:2603.24775, 2026. — Capability-as-cryptographic-token; append-only, signed, scope narrows on delegation.

[^akselrod-2023]: Akselrod, et al. "AI Capability Restrictions." arXiv:2303.09377, 2023. — Three-part warrant test for restriction.

[^cmmi]: SEI. *CMMI for Development, Version 2.0.* Software Engineering Institute, Carnegie Mellon University. — Maturity levels 1→5; prerequisite DAGs.

[^tacit]: Giannini, et al. "TACIT: Capability Registry as Types in Scala 3." EPFL, 2026. lampepfl/tacit. — Typed capability traits; scoped minting.

[^bigbench-2022]: Srivastava, et al. "Beyond the Imitation Game: Quantifying and Extrapolating the Capabilities of Language Models." arXiv:2206.04615, 2022. — BIG-bench.

[^helm-2022]: Liang, et al. "Holistic Evaluation of Language Models." arXiv:2211.09110, 2022. — HELM; scenario × metric matrix.

[^emergent-2022]: Wei, et al. "Emergent Abilities of Large Language Models." arXiv:2206.07682, 2022. — Emergence definition.

[^mirage-2023]: Schaeffer, Rylan, Brando Miranda, and Sanmi Koyejo. "Are Emergent Abilities of Large Language Models a Mirage?" arXiv:2304.15004, NeurIPS 2023. — Metric-dependence of emergence.

[^obsscaling-2024]: Ruan, et al. "Observational Scaling Laws and the Effect of Compute on Capabilities." arXiv:2405.10938, ICML 2024. — PCA capability vector.

[^evaltree-2025]: Zeng, et al. "EvalTree: Hierarchical Capability Tree." arXiv:2503.08893, 2025. — Annotate → embed → cluster → describe.

[^arditi-2024]: Arditi, et al. "Refusing in Context: The Effects of Password-Locked Models." arXiv:2405.19550, 2024. — Elicited potential vs. observed behavior.
