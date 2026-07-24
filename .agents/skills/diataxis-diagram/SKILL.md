---
name: diataxis-diagram
visibility: public
description: "Generate Mermaid diagrams from code using Diataxis methodology. Convergent PDCA loop: classify target and diagram type, extract entities from source, generate Mermaid syntax, evaluate against Diataxis quality criteria, iterate until convergence. Three core types (ERD, flowchart, state) and two extended (sequence, class). All diagrams render natively in Zed."
---


# Diataxis Diagram

Generate Mermaid diagrams from code using Diataxis methodology. The skill runs a convergent PDCA loop: classify the target and diagram type, extract entities from source, generate Mermaid syntax, evaluate against Diataxis quality criteria, and iterate until convergence. Supports three core diagram types (ERD, flowchart, state) and two extended types (sequence, class). All diagrams render natively in Zed.

## When to Use

- You need to generate a Mermaid diagram from source code, SQL schemas, or documentation
- You need to visualize database schemas (ERD), control flow (flowchart), state machines (state), service interactions (sequence), or type hierarchies (class)
- Documentation should follow Diataxis methodology with quadrant-appropriate voice (reference, explanation, how-to, tutorial)
- Diagrams must render natively in Zed's markdown preview
- You want iterative quality convergence — diagrams are scored and refined until they meet a quality threshold (≤ 0.15 weighted total across six criteria)

## Instructions

1. **Classify the target.** Determine which Mermaid diagram type is appropriate — ERD for SQL schemas and CREATE TABLE statements, flowchart for control flow and decision trees, state for enums with lifecycle variants and status transitions, sequence for message passing and request/response chains, class for traits, structs, and impl blocks. Classify which Diataxis quadrant the diagram will serve (reference by default, unless user intent suggests explanation, how-to, or tutorial). Identify which source files to read. Produce a classification verdict with a one-sentence rationale.

2. **Extract entities and relationships.** Guide extraction from source files using type-specific rules. For ERDs: extract table names, columns with SQL types and modifiers, foreign keys, and cardinality (one-to-one via UNIQUE FK, one-to-many via plain FK, many-to-many via junction tables). For flowcharts: trace control flow through if/match expressions, function calls, and return paths. For state diagrams: extract enum variants as states, match arms as transitions, guard conditions as labels. For sequence diagrams: extract services and agents as participants, function calls as messages, conditionals as alt/else blocks. For class diagrams: extract structs as classes, traits as interfaces, impl blocks as relationships. If `extracted_entities` is pre-populated, validate completeness and skip extraction.

3. **Generate Mermaid syntax.** Convert extracted entities and relationships into valid Mermaid source. Apply type-specific conventions: Crow's Foot cardinality (`||--||`, `||--o{`, `}o--o{`) for ERDs; node shapes (`[rectangle]`, `{rhombus}`, `([rounded])`) for flowcharts; `[*]` start/end markers and transition labels for state diagrams; `participant`, `->>`, `-->>`, and block constructs (`alt`, `loop`, `opt`, `par`) for sequences; `<<interface>>` and `<<enumeration>>` markers with `<|--`, `o--`, `..>` relationships for class diagrams. Respect Zed rendering constraints — no `%%{init}%%`, no `classDef`, no inline color styles; prefer `TD` over `LR`. Output only Mermaid source without markdown fences. Apply refinement directives from previous evaluation if present.

4. **Evaluate against Diataxis criteria.** Score the generated diagram on six weighted dimensions: entity completeness (0.30), relationship accuracy (0.25), label readability (0.15), type appropriateness (0.15), Diataxis voice (0.10), cross-linking (0.05). Score each criterion from 0 (perfect) to 1 (severely deficient). Be honest — inflated scores produce worse diagrams, not better ones. Produce specific, actionable refinement directives for any criterion scored above 0.00 — each directive must name the criterion, state what is wrong, and describe the expected fix. Do not emit directives for criteria scored at 0.00.

5. **Check convergence.** Compute the normalized convergence metric from the evaluation's weighted total. Threshold is 0.15 — a metric of ≤ 0.15 means CONVERGED. Range 0.16–0.25 is NEAR (one more iteration should resolve). Range 0.26–0.50 is DRIFTING (refinement directives should target specific weaknesses). Above 0.50 is DIVERGED (consider re-classifying diagram type). Maximum 3 iterations.

6. **Write the final diagram.** Wrap the Mermaid source in a markdown file with a title and a plain-English description paragraph keyed to the target Diataxis quadrant's voice: austere and factual for reference, discursive and contextual for explanation, direct and actionable for how-to, encouraging and concrete for tutorial. Include cross-links to at least one related document using relative links from the `docs/diagrams/` directory. Output to `docs/diagrams/{diagram_type}-{target_slug}.md` where the target slug is lowercased with hyphens, ≤ 40 characters.

## Registry Templates

| Template | Type | Purpose |
|----------|------|--------|
| `diataxis-diagram-classify.j2` | `KnowAct` | Classify the target for diagram generation. Determine which Mermaid diagram type is appropriate (ERD, flowchart, state, sequence, or class), which Diataxis quadrant the diagram will serve, and which source files to read. Produces a classification verdict with rationale. |
| `diataxis-diagram-extract.j2` | `KnowAct` | EXTRACT phase — agent-coordinated source reading. Provides extraction guidance for the target diagram type: what to look for in SQL schemas (tables, columns, PK/FK), Rust code (enums, traits, control flow), or documentation. The actual file I/O is agent-coordinated between template calls. If extracted_entities is pre-populated, skip extraction and proceed. |
| `diataxis-diagram-generate.j2` | `KnowAct` | Render extracted entities into valid Mermaid syntax. Apply type-specific conventions: Crow's Foot cardinality for ERD, node shapes for flowcharts, transition labels for state diagrams, block constructs for sequences, interface markers for class diagrams. Accepts refinement directives from the evaluate step for iterative improvement. |
| `diataxis-diagram-evaluate.j2` | `KnowAct` | Score a generated diagram against Diataxis quality criteria. Six weighted dimensions: entity completeness (0.30), relationship accuracy (0.25), label readability (0.15), type appropriateness (0.15), Diataxis voice (0.10), cross-linking (0.05). Produces a scored evaluation with specific refinement directives when quality gaps are found. |
| `diataxis-diagram-convergence.j2` | `KnowAct` | Compute normalized convergence metric from the evaluation scores. Weighted sum across six Diataxis criteria, normalized to [0,1] where 0 = diagram perfectly matches source and Diataxis standards. Threshold: 0.15 (same as idiomatic-rust — "maximally correct"). |
| `diataxis-diagram-write.j2` | `KnowAct` | Finalize the diagram into a markdown file. Wraps the Mermaid source in a code block, adds a plain-English description paragraph keyed to the target Diataxis quadrant's voice, includes cross-links to related documentation, and outputs to docs/diagrams/{type}-{target}.md. |

## Constraints

- All templates are `visibility: Public` — no restricted spans generated
- Energy caps: classify=4096, extract=6144, generate=8192, evaluate=6144, convergence=2048, write=4096
- Zed rendering constraints: no `%%{init}%%`, no `classDef`, no inline color styles; prefer `TD` over `LR` for narrow sidebar rendering
- Labels must be ≤ 40 characters; state names ≤ 30 characters
- Entity IDs must be alphanumeric with underscores — no spaces, dashes, or special characters
- Output Mermaid source must parse without errors in Mermaid.js
- Generate step outputs only Mermaid source — no markdown fences (write step handles wrapping)
- Maximum 3 iterations before forced convergence exit
- Convergence threshold: 0.15 weighted total across six Diataxis criteria
- All diagrams must include at least one cross-link to related documentation
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins
