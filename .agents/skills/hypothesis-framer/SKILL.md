---
name: hypothesis-framer
description: "Research question framing and hypothesis formulation using FINER criteria and PICO process. Evaluates topics through Feasibility, Interest, Novelty, Ethics, and Relevance gates, structures questions, derives testable hypotheses."
---

# Hypothesis Framer

Research question framing and hypothesis formulation using FINER criteria and PICO process. Evaluates broad research topics through Feasibility, Interest, Novelty, Ethics, and Relevance (FINER) gates, structures questions via Population-Intervention-Comparison-Outcome (PICO) framework, derives testable hypotheses with null hypothesis formulation, operationalizes into aims and objectives, and verifies alignment. Iterative PDCA refinement until the question-hypothesis-aims chain is coherent and testable.

## When to Use

- When you have a broad research topic that needs evaluation against FINER criteria (Feasibility, Interest, Novelty, Ethics, Relevance) before committing to a study design
- When a research question needs structuring via the PICO framework (Population, Intervention, Comparison, Outcome) to ensure precision and testability
- When you need to derive a testable hypothesis with null hypothesis formulation from a PICO-structured question
- When hypothesis operationalization into research aims and objectives is required, with alignment verification and feasibility recheck
- When iterative PDCA refinement is needed to converge the question-hypothesis-aims chain into a coherent, testable, decision-ready framing
- When convergence assessment across FINER compliance, PICO completeness, hypothesis coherence, and aims alignment is needed to determine if the research framing is ready

## Instructions

1. **Evaluate the broad research topic against FINER criteria.** For each of the five dimensions — Feasible (subjects, expertise, resources, institutional support), Interesting (audience, applicability, engagement), Novel (knowledge gap, methodology, confirmation), Ethical (regulatory compliance, risk, informed consent, animal welfare), and Relevant (clinical impact, knowledge contribution, generalizability, timeliness) — assign a score from 0–10 with specific, justified rationale.
2. **Identify the most concerning FINER dimension** (lowest score) and formulate a refined research question that addresses the weaknesses. Provide actionable refinement suggestions for each dimension scoring below 7. The refined question must be a question (not a declarative statement), hypothesis-driven (not data-driven), and open inquiry rather than closeable with yes/no.
3. **Apply the PICO framework** to structure the refined research question. Define the Population (condition, demographics, setting, inclusion/exclusion criteria, justification), Intervention (type, description, dose/intensity/frequency, duration, delivery), Comparison (type, description, justification — acknowledge if no comparator exists), and Outcome (primary and secondary outcomes, measurement methods, timing, clinical significance).
4. **Synthesize the PICO elements into a single structured question** using the appropriate template (intervention, diagnostic, prognostic, or etiology format). Assess PICO completeness for each element as complete, partial, or missing.
5. **Determine the hypothesis type** — difference, association, superiority, non-inferiority, equivalence, diagnostic accuracy, or prognostic — based on the PICO structure and study design.
6. **Formulate the research hypothesis (H₁)** as a declarative statement predicting the expected outcome. Reference PICO elements explicitly, use directional language when possible, and ensure falsifiability. Format: "In [population], [intervention] will [direction] [outcome] compared to [comparison]."
7. **Formulate the null hypothesis (H₀)** postulating no difference or no relationship. Format: "In [population], there is no difference in [outcome] between [intervention] and [comparison]."
8. **Define the primary aim** as a broad, overarching purpose directly linked to the research question, referencing PICO elements. Format: "The primary aim of this study is to [verb] [what] in [population]."
9. **Define 2–4 primary objectives** as specific, measurable steps that accomplish the primary aim, linked to the primary outcome measure. Define secondary aims with clear rationale if applicable — avoid "fishing expeditions."
10. **Assess testability** now that objectives are specified: verify measurable outcome with validated method, specified population, defined comparison, suggested statistical test, clinically meaningful effect size, non-inferiority/equivalence margin (δ) if applicable, and sample size feasibility.
11. **Verify five-link alignment**: question→hypothesis, hypothesis→primary aim, primary aim→objectives, objectives→PICO outcome, and hypothesis→null hypothesis. Flag any misalignments honestly and propose corrections.
12. **Recheck feasibility** in light of operational aims and objectives: sample size, methods, timeline, and resources. Identify any new concerns that emerged during aims/objectives development.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `finer-evaluate.j2` | Apply FINER criteria to a broad research topic. Evaluate feasibility (subjects, expertise, resources), interest (audience relevance), novelty (knowledge contribution), ethics (compliance), and relevance (clinical impact). Produces per-dimension scores with rationale and refinement suggestions. |
| `pico-structure.j2` | Apply PICO framework to structure the research question. Identifies Population characteristics, Intervention/exposure, Comparison/control, and Outcome measures. Produces a structured question and completeness assessment. |
| `hypothesis-operationalize.j2` | Single cognitive act: derive a testable hypothesis from the PICO-structured question, formulate the null hypothesis, classify type, operationalize into research aims and objectives, assess testability against the specified objectives, verify five-link alignment, and recheck feasibility. Merges what were previously separate hypothesis and aims steps — because you cannot assess testability without knowing the objectives. |

To render a template, call the `render_template` tool with the template ref (e.g., `essentialist/essentialist-flow`) and a context object with the required variables.

## Constraints

- All templates are prompt templates with `Public` visibility
- The research question must be a question, not a declarative statement; the research hypothesis must be a declarative statement, not a question
- The null hypothesis must postulate no difference or no relationship
- Non-inferiority and equivalence hypotheses require a defined δ margin — without it, the hypothesis is not testable
- Aims must be broader than objectives; objectives must be measurable — "to improve understanding" is not measurable
- Secondary aims must have clear rationale — avoid "nice to know" add-ons
- The alignment check must be honest — flag misalignments, do not paper over them
- If PICO comparison is "none," acknowledge the descriptive/pre-post design limitation rather than fabricating a comparator
- Do not execute arbitrary Python code in Jinja2 expressions (sandboxed execution)
- When safety mode is enabled: no file system access, no network calls, no environment variable access, strict Jinja2 sandbox enforcement
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.