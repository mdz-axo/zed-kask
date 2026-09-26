---
name: diataxis-diagram
description: "Generate Mermaid diagrams from code using Diataxis methodology. Supports core types (ERD, flowchart, state, sequence, class) plus extended types (architecture, sankey, gantt, mindmap, timeline, and more). Renders natively in Zed."
---


# Diataxis Diagram

Generate Mermaid diagrams from code, using Diátaxis to choose the documentation quadrant and voice. Its six-weight diagram rubric is local advisory feedback, not a formula published by Diátaxis. Classify and extract from observed sources, generate Mermaid, check what can actually be checked, and refine only measured defects. Supports five core diagram types (ERD, flowchart, state, sequence, class) and fourteen extended types (architecture, block, radar, treemap, sankey, kanban, gantt, pie, gitgraph, mindmap, timeline, quadrant, xychart, journey). All diagrams render natively in Zed.

## Initial and target condition

- **Initial condition:** the target, the source files read, and the extracted entity and relationship lists (step 2).
- **Target condition:** diagram nodes/relationships trace to the extracted sources with no invented entities, syntax parses where a parser exists (otherwise `parse unverified`), and the requester can inspect its quadrant-appropriate voice. A file exists only after a successful write. A `DIAGRAM_ALIGNMENT` block is required only for an explicitly requested `kask/docs/diagrams/` update.

## Step types

| Step | Type | Oracle / critique |
|------|------|-------------------|
| 1 Classify, 2 Extract, 3 Generate | P | step 5 checks; the requester |
| 4 Evaluate | P | local rubric for refinement, not a stop rule or Diátaxis formula |
| 5 Check | P for entity/relationship review; D for `mmdc` exit when available | comments/labels cannot prove an entity node exists |
| 6 Write, 7 Present | D only for actual file-write receipt and deterministic render | `render_template` alone creates no file |

## When to Use

- You need to generate a Mermaid diagram from source code, SQL schemas, or documentation
- You need to visualize database schemas (ERD), control flow (flowchart), state machines (state), service interactions (sequence), type hierarchies (class), system topology (architecture), module boundaries (block), capability profiles (radar), hierarchical quantities (treemap), weighted flows (sankey), task boards (kanban), timelines (gantt, timeline), distributions (pie), commit history (gitgraph), concept maps (mindmap), strategic matrices (quadrant), performance data (xychart), or user journeys (journey)
- Documentation should follow Diataxis methodology with quadrant-appropriate voice (reference, explanation, how-to, tutorial)
- Diagrams must render natively in Zed's markdown preview
- You want iterative refinement — diagrams are checked for completeness and parse errors and refined for up to 3 iterations

## When NOT to Use

- Freeform whiteboarding — the output must be valid Mermaid that renders in Zed's preview.
- Sankey flows from natural-language quantities — use `sankey-flow` (it owns the domain conservation rules and never fabricates weights).
- Diagrams with no source to derive from — this skill generates from code, schemas, or docs; a diagram with no source is an illustration request.
- Verifying or re-aligning the existing `kask/docs` diagram set — `doc-update` owns that (its Phase 4 calls this skill when a doc needs a new diagram).

## Instructions

1. **Classify the target.** Determine which Mermaid diagram type is appropriate — ERD for SQL schemas and CREATE TABLE statements, flowchart for control flow and decision trees, state for enums with lifecycle variants and status transitions, sequence for message passing and request/response chains, class for traits, structs, and impl blocks, architecture for system topology and service boundaries, block for module boundaries and component composition, radar for multi-dimensional capability assessment, treemap for hierarchical quantity data, sankey for weighted flow between stages, kanban for task boards and work items, gantt for scheduled work and milestones, pie for proportional breakdowns, gitgraph for commit history, mindmap for hierarchical concept maps, timeline for chronological events, quadrant for 2×2 strategic matrices, xychart for quantitative x-y data, journey for user experience mapping. Classify which Diataxis quadrant the diagram will serve (reference by default, unless user intent suggests explanation, how-to, or tutorial). Identify which source files to read. Produce a classification verdict with a one-sentence rationale.

2. **Extract entities and relationships.** Guide extraction from source files using type-specific rules. For ERDs: extract table names, columns with SQL types and modifiers, foreign keys, and cardinality (one-to-one via UNIQUE FK, one-to-many via plain FK, many-to-many via junction tables). For flowcharts: trace control flow through if/match expressions, function calls, and return paths. For state diagrams: extract enum variants as states, match arms as transitions, guard conditions as labels. For sequence diagrams: extract services and agents as participants, function calls as messages, conditionals as alt/else blocks. For class diagrams: extract structs as classes, traits as interfaces, impl blocks as relationships. For architecture: extract services, groups, junctions, and port-based edges. For block: extract modules as blocks with nested structure and column layout. For radar: extract evaluation dimensions as axes and scored entities as curves. For treemap: extract hierarchical sections and leaves with numeric values. For sankey: extract nodes and weighted edges as CSV rows. For kanban: extract status columns and task cards with optional metadata. For gantt: extract tasks with start dates, durations, and dependencies. For pie: extract categories with proportional values. For gitgraph: extract branches, commits, merges, and tags. For mindmap: extract hierarchical concepts with indentation-based levels. For timeline: extract chronological events grouped by period. For quadrant: extract two-axis positioning with quadrant labels and entity points. For xychart: extract x-y numeric series. For journey: extract stages with satisfaction scores and participants. If `extracted_entities` is pre-populated, validate completeness and skip extraction.

3. **Generate Mermaid syntax.** Convert extracted entities and relationships into valid Mermaid source. Apply type-specific conventions: Crow's Foot cardinality (`||--||`, `||--o{`, `}o--o{`) for ERDs; node shapes (`[rectangle]`, `{rhombus}`, `([rounded])`) for flowcharts; `[*]` start/end markers and transition labels for state diagrams; `participant`, `->>`, `-->>`, and block constructs (`alt`, `loop`, `opt`, `par`) for sequences; `<<interface>>` and `<<enumeration>>` markers with `<|--`, `o--`, `..>` relationships for class diagrams; `service`/`junction`/`group` notation with port-based edges for architecture; `columns` and nested `block` syntax for block diagrams; `axis`/`curve` notation for radar; indentation-based hierarchy with `"label": value` for treemap; `sankey-beta` CSV edges with front-matter config for sankey; `kanban` column/task notation with `@{}` metadata for kanban; `gantt` sections with date/duration syntax for gantt; `pie` slices for pie; `gitGraph` branch/commit/merge for gitgraph; `mindmap` root and indentation for mindmap; `timeline` sections and events for timeline; `quadrantChart` axes and points for quadrant; `xychart-beta` axes and series for xychart; `journey` sections and scored tasks for journey. Respect Zed rendering constraints — no `%%{init}%%`, no `classDef`, no inline color styles; prefer `TD` over `LR`. Use the `-beta` suffix where required (`architecture-beta`, `radar-beta`, `treemap-beta`, `sankey-beta`, `xychart-beta`; `block-beta` and `block` are both accepted). Output only Mermaid source without markdown fences. Apply refinement directives from previous evaluation if present.

4. **Evaluate against a local diagram rubric (P, not Diátaxis's quality formula).** Diátaxis distinguishes measurable functional quality from judged deep quality and prescribes no numeric weights. Use the following project-specific six dimensions only to prioritize revisions: entity completeness (0.30), relationship accuracy (0.25), label readability (0.15), type appropriateness (0.15), Diataxis voice (0.10), cross-linking (0.05). Score each criterion from 0 (perfect) to 1 (severely deficient). Compute the weighted total in `lisp_eval` (`(+ (* c1 0.30) (* c2 0.25) (* c3 0.15) (* c4 0.15) (* c5 0.10) (* c6 0.05))`), never by hand. The total is the model's estimate of its own diagram, used only to choose what to refine; it is not a stop condition. Produce specific, actionable refinement directives for any criterion scored above 0.00 — each directive must name the criterion, state what is wrong, and describe the expected fix. Do not emit directives for criteria scored at 0.00. Pass `related_docs` as the checked paths actually found; an empty list is not a request to invent a link.

5. **Check against sources, not substrings.** Compare the extracted entity/relationship list to *actual Mermaid declarations or edges* for this diagram type, citing the source file:line and generated line for each disputed item. An ID found only in a comment or label is not a node. This mapping is a reviewed judgment unless a suitable type-specific parser ran; report `completeness unverified` rather than treating a `string-contains` hit as proof. If `command -v mmdc` succeeds, parse the generated source with a bounded CLI call; otherwise set `parse unverified` and request a rendered preview. A model-produced invented-node list is a lead, not an oracle. Re-enter generation on one named, observed defect; at most 3 iterations, then present remaining failed/unverified checks without claiming target attainment.

6. **Prepare, then actually write.** Render `diataxis-diagram-write` with a caller-supplied `output_path`; when none was supplied, choose a unique run path under `~/Documents/zk-data/skills/diataxis-diagram/{date}-{run}/diagram.md`. The template proposes markdown; it does not write a file. After review, use `terminal` for producer-data paths or the project file tools for an explicitly requested in-project path, and check the write receipt before reporting `file_path`. When `doc-update` explicitly supplies a `kask/docs/diagrams/` destination, update the relevant consolidated doc under its ownership and include a source-backed `DIAGRAM_ALIGNMENT` block; don't insert that metadata into ordinary skill previews. Cross-link only documents whose actual paths were checked. A missing related document is a noted gap, not a license to invent a link.

7. **Surface the diagram.** Pass `file_content`, proposed `file_path`, and `written` from the *actual write receipt* to `present-diagram.j2`, which deterministically surfaces the fenced ```mermaid block. If no file was written, show an inline preview and explicitly say no path was published; rendering a path is not a write receipt.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `diataxis-diagram-classify.j2` | Classify the target for diagram generation. Determine which Mermaid diagram type is appropriate (ERD, flowchart, state, sequence, class, architecture, block, radar, treemap, sankey, kanban, gantt, pie, gitgraph, mindmap, timeline, quadrant, xychart, or journey), which Diataxis quadrant the diagram will serve, and which source files to read. Produces a classification verdict with rationale. |
| `diataxis-diagram-extract.j2` | EXTRACT phase — agent-coordinated source reading. Provides extraction guidance for the target diagram type: what to look for in SQL schemas (tables, columns, PK/FK), Rust code (enums, traits, control flow), or documentation. The actual file I/O is agent-coordinated between template calls. If extracted_entities is pre-populated, skip extraction and proceed. |
| `diataxis-diagram-generate.j2` | Render extracted entities into valid Mermaid syntax. Apply type-specific conventions: Crow's Foot cardinality for ERD, node shapes for flowcharts, transition labels for state diagrams, block constructs for sequences, interface markers for class diagrams, service/junction notation for architecture, column/row layout for block diagrams, axis/curve notation for radar, hierarchical nesting for treemap, weighted CSV edges for sankey, column/task notation for kanban, and standard syntax for gantt, pie, gitgraph, mindmap, timeline, quadrant, xychart, and journey. Accepts refinement directives from the evaluate step for iterative improvement. |
| `diataxis-diagram-evaluate.j2` | Apply a local advisory diagram rubric, not a Diátaxis-published formula. Six weighted dimensions: entity completeness (0.30), relationship accuracy (0.25), label readability (0.15), type appropriateness (0.15), Diataxis voice (0.10), cross-linking (0.05). Produces a scored evaluation with specific refinement directives when quality gaps are found. |
| `diataxis-diagram-write.j2` | Propose markdown with quadrant-appropriate description and sourced optional cross-links at a caller-supplied path; the invoking agent writes it separately and retains the receipt. |
| `present-diagram.j2` | Rendering template — surfaces the finalized diagram markdown (containing the fenced ```mermaid block) as the process's final output string. Without this step, the write step's JSON object {file_path, file_content, description_paragraph} is the final step output and the model must discover and extract the file_content field. This render step flattens it to a raw markdown string so the model sees the mermaid block directly. Deterministic (no LLM call). |

To render a template, call the `render_template` tool with the template ref (e.g., `diataxis-diagram/diataxis-diagram-classify`) and a context object with the required variables.

## Constraints

- All templates are `visibility: Public` — no restricted spans generated
- Zed rendering constraints: no `%%{init}%%`, no `classDef`, no inline color styles; prefer `TD` over `LR` for narrow sidebar rendering
- Labels must be ≤ 40 characters; state names ≤ 30 characters
- Entity IDs must be alphanumeric with underscores — no spaces, dashes, or special characters
- Output must have a parser receipt or be labeled `parse unverified`; do not claim native rendering without observation
- Generate step outputs only Mermaid source — no markdown fences (write step handles wrapping)
- Maximum 3 iterations; after the third, deliver with the failed checks listed
- The six-criterion weighted total is a labelled estimate, computed in `lisp_eval`; it chooses refinements and never gates
- Only verified existing related documents are cross-linked; no link is fabricated to satisfy a score
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
- **Visual artifact surfacing** — the `present-diagram.j2` render step (rendering template) must be the process's final output step. It surfaces the fenced ```mermaid block as a raw markdown string so acp_thread's mermaid renderer picks it up. Removing it causes the diagram to stay buried in the write step's JSON `{file_path, file_content}` object.
