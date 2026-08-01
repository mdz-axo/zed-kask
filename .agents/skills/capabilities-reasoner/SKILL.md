---
name: capabilities-reasoner
visibility: public
description: "Reason about a system's capabilities against a typed registry with floor, ceiling, and maturity-gate limits. Fuses two lineages: the capability approach (Sen/Nussbaum — feasible functionings with thresholds; object-capability security — authority only attenuates; CMMI — prerequisite DAGs) and ML capability evaluation (HELM — multi-metric scenario matrix; EvalTree — hierarchical capability tree; Password-Locked Models — capability is elicited potential not observed behavior; 'Are Emergent Abilities a Mirage?' — capability is metric-dependent). Decomposed into phased templates: Register → Elicit → Evaluate → Reason → Report → Convergence. Composes structured-extraction, falsifiability, pragmatic-cybernetics, graph-audit. Emits reg.capability.* spans. Any userpod may invoke this skill."
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

## Three capability domains

The reasoner distinguishes three domains, each with its own exemplar tradition:

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
- Determine whether a capability restriction is warranted (Akselrod three-part test)
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

**Field**: `step_6_result.convergence_metric`. **Threshold**: 0.25. **Max iterations**: 3.

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
| `capability-register.j2` | `KnowAct` | Build/load typed capability registry with floor, ceiling, maturity prerequisites. Delegates to pragmatic-cybernetics for variety engineering. |
| `capability-elicit.j2` | `KnowAct` | Measure elicited capability with deliberate metric choice. Delegates to structured-extraction for instance extraction. |
| `capability-evaluate.j2` | `KnowAct` | Compare elicited capabilities against floor/ceiling/maturity. Delegates to falsifiability for counterfactual claims and graph-audit for prerequisite DAG. |
| `capability-reason.j2` | `KnowAct` | Determine interventions via attenuation rule + Akselrod warrant test. |
| `capability-report.j2` | `KnowAct` | Compile per-capability verdicts with lessons and signatures. |
| `capability-ontology.yaml` | `RenderAct` | Reference: capability taxonomy (Nussbaum 10 + object-capability + CMMI levels + ML eval definitions + Dunning metacognitive gap). |

## Constraints

- All flow templates are `KnowAct` type with `Public` visibility. Reference documents are `RenderAct`.
- Energy caps: register (6144), elicit (6144), evaluate (6144), reason (4096), report (4096).
- Gas cap: 120,000 per invocation. Maximum 3 iterations.
- The capability definition must be declared before elicitation — different definitions produce different verdicts.
- The attenuation rule is inviolable: authority may only narrow without re-authorization. Widening requires explicit re-authorization with a recorded warrant.
- The metric-stability check is mandatory — a verdict that flips under a different metric is a mirage [^mirage-2023], not a capability finding.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.

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
