---
name: therapy
description: Memory therapy session — scans a memory database (curator, replica/corpus, or swarm) for contradictions, fragmentation, and miscalibrated confidence; resolves them; then reifies useful lessons as skills, templates, or rules and purges/condenses the source memories. The user approves all modifications. Therapy is the process of re-organizing memory to be useful and reifying learned experience into proactive guidance.
---

# Therapy

Memory therapy session for resolving contradictions, reifying lessons, and shedding cognitive load. The user initiates the session, the skill scans for issues, proposes resolutions, reifies useful patterns into skills/templates/rules, and the user approves all modifications.

Therapy has two concrete goals:

1. **Memory hygiene** — manage the consistency and usefulness of memory. Keep it from being a danger (stale code status, pre-solution configurations, contradictions that mislead agents) and make it a constructive, useful database. This includes condensing and purging low-value information so it doesn't obstruct learning. Hygiene is necessary infrastructure, but it is NOT learning — it's forgetting.

2. **Reification** — extract meaning from memory and reify useful lessons as skills, templates, or rules. This is the step that closes the learning loop. The goal of cognition isn't to recall the past — it's for past experiences to be applied to being more effective in the future. Therapy is where experience gets extracted into meaning and reified into proactive guidance.

**The learning loop** (therapy is the extraction-and-reification step that closes it):

```
experience → memory → therapy (extract meaning + reify) → proactive guidance (skill/template/rule) → new experience
```

**Memory hygiene** (runs alongside, not part of the learning loop):

```
scan for contradictions/stale/fragmented → resolve, purge, condense → clean memory database
```

Forgetting (purging/condensing) is NOT learning. It is shedding low-value information so it doesn't obstruct learning. The goldfish principle applies here: once a lesson is reified into proactive guidance, the episodic memory that produced the lesson can be forgotten — but the forgetting is a hygiene side-effect, not the learning step. The learning step is the reification.

## When to Use

- When the user wants to run a therapy session on the curator's memory database — **must be run from a Curator agent panel session**, not from the zed agent. The curator must remember the act of therapy (the forgetting, the reification, the lessons learned) so the cybernetic loop closes. Therapy run from the zed agent would modify curator memory without the curator's awareness — that defeats the cybernetic design.
- When the user wants to run a therapy session on a replica/corpus chunk memory database — can be run from a curator panel session or a corpus-scoped kask panel tab.
- When the user wants to run a therapy session on a swarm's shared memory database — should be run from a curator panel session or a swarm-scoped kask panel tab.
- When recall is misleading agents by returning old code status or pre-solution configurations.
- When the user notices contradictions in what agents recall or assert.
- When confidence values appear miscalibrated.
- When the user wants to extract lessons from accumulated memory and reify them as skills, templates, or rules.
- When memory is bloated with episodic detail that has been superseded by learned habits (skills/rules).

## When NOT to Use

- **From the zed agent (non-curator mode).** Therapy on curator memory must run from a curator panel session so the curator remembers the therapy. The zed agent has no memory and the curator won't observe the session.
- For routine memory consolidation (that runs automatically on the timer).
- For adding new memories (use `memory_insert` directly).
- For single-memory updates (use `memory_update` directly).
- For creating a skill from scratch with no memory basis (use `create-skill` directly).

## Grounding

### Computational models of memory (Cox & Shiffrin, 2026, OECS)

- **Memory traces can be altered once retrieved** — "long-term traces are not necessarily immutable; long-term traces can be altered, augmented, and changed once retrieved." Therapy is the deliberate process of doing this.
- **Distorted traces** (Loftus, 2005) — "long-term traces may represent events in a distorted manner." Therapy identifies and corrects distortions.
- **Trace coevolution** (Nelson & Shiffrin, 2013) — "traces can accumulate information across events, enabling event memory and knowledge traces to coevolve." Therapy re-organizes traces so they accumulate correctly, then reifies the accumulated knowledge into skills/rules.
- **Probe-activation retrieval** — traces activate in proportion to similarity to the probe. Contradictory traces with similar features both activate, producing noise. Therapy reduces noise by resolving contradictions.
- **REM model** (Shiffrin & Steyvers, 1997) — storage parameters `u` (transfer probability), `c` (correct storage probability), `g` (distinctiveness). Low `c` produces error-prone traces. Therapy identifies low-`c` traces and corrects them.

### Cognitive dissonance (Festinger; Lidwell, *Universal Principles of Design*)

- **Three resolution strategies**: reduce importance (lower confidence), add consonant (insert reconciling memory), remove dissonant (expire/delete). Therapy classifies contradictions by strategy and proposes resolutions.

### Dunning's self-knowledge framework

- **Double curse** (Kruger & Dunning, 1999) — the agent cannot self-evaluate which memories are correct. Therapy requires the human user's judgment, not the agent's self-assessment.
- **Hypocognition** (Dunning, 2018) — the system may lack a representation for the contradiction. Therapy names contradictions explicitly.
- **Naive realism** (Ehrlinger & Dunning, 2003) — agents treat recalled context as objective reality. Therapy makes contradictions visible so the user can see them.

### The goldfish principle (Vardy, 2020; Ted Lasso)

- **"Be a goldfish"** — the goldfish has a 10-second memory and is the happiest animal on earth. The principle: don't let the past own the present. Applied to memory therapy: forgetting is a feature, not a bug. Once a lesson is reified into a skill, template, or rule, the episodic memory that produced the lesson can be forgotten (purged or condensed). You don't need to recall the past experience if the lesson is already proactively embedded in your guidance system. This is cognitive load shedding — the process of converting recall-dependent guidance into proactive contextualized guidance, then shedding the recall dependency.

### Reification and the learning loop

- **Reification** is the process of converting abstract experience (episodic memory) into concrete, reusable guidance (skills, templates, rules). It is the memory-to-learning bridge: the past becomes useful not by being recalled, but by being embedded in the system that guides future action.
- **Therapy is the step that closes the learning loop**: experience → memory → therapy (extract meaning + reify) → proactive guidance → new experience. Without therapy, memory accumulates but never becomes learning — the past is stored but not applied.
- **Forgetting is NOT learning.** Purging or condensing source memories after reification is memory hygiene, not part of the learning loop. It is shedding low-value information so it doesn't obstruct future learning. The goldfish principle: once the lesson is reified, forget the episodic detail — but the forgetting is a side-effect of successful reification, not the learning step itself.

## Instructions

### Phase 1 — Target selection

1. Ask the user which memory database to run therapy on:
   - **Curator memory** (`curator.db`) — the curator's own memory of conversations and observations.
   - **Replica/corpus memory** — a corpus chunk database (e.g., `john-brooks.db`).
   - **Swarm memory** — a swarm's shared memory database.

2. Record the target. For curator memory, the MCP tools are `curator_memory_recall`, `curator_semantic_search`, `memory_insert`, `memory_update`, `memory_resolve_contradiction`. For corpus memory, the tools are `corpus_query` and corpus-specific tools. For swarm memory, the tools are `swarm_recall_local` and swarm-specific tools.

### Phase 2 — Scan

1. Call `render_template` to render the scan template:
   - template: `therapy/scan.j2`
   - variables: { "target": "<target name>", "target_type": "<curator|corpus|swarm>" }

2. Following the template's guidance, scan the memory database for four categories of issues:

   **a. Contradictions** — h_mems with the same entity+attribute but divergent values or confidence. Use `curator_memory_recall` (for curator) or `corpus_query` (for corpus) or `swarm_recall_local` (for swarm) to query by entity, then compare values. For curator memory, also use `curator_semantic_search` to find related entities that may contradict.

   **b. Fragmentation** — h_mems that are isolated (low connectedness, no co-occurrence links) but semantically similar to well-connected h_mems. These are dilution candidates (Tetlock's dilution effect). Query by embedding similarity and check for isolated vs. connected clusters.

   **c. Miscalibrated confidence** — h_mems with confidence that doesn't match their evidence. Look for:
   - High-confidence h_mems with no evidence citation (no `evidence_h_mem_id`).
   - Low-confidence h_mems that have been recalled many times (high `recalled_at` frequency) without contradiction.
   - Confidence values that diverge significantly from related h_mems on the same topic.

   **d. Reification candidates** — clusters of episodic memories that share a common pattern or lesson but have not yet been reified into a skill, template, or rule. Look for:
   - Repeated patterns across multiple entities (e.g., "this skill fails 40% of the time when X" appears across multiple skill-use-issue reports).
   - Accumulated experience on a topic that could inform a rule or template (e.g., "when doing X, always check Y first" — if this lesson appears in multiple memories, it's a reification candidate).
   - Episodic detail that has been superseded by a learned habit (if a skill or rule already captures the lesson, the episodic memories are purge candidates).

3. Collect all findings as structured data. Each finding includes:
   - `h_mem_id`: the ID of the problematic h_mem (or the cluster ID for reification candidates).
   - `entity`: the entity of the h_mem.
   - `attribute`: the attribute.
   - `issue_type`: "contradiction" | "fragmentation" | "miscalibrated_confidence" | "reification_candidate".
   - `description`: what the issue is.
   - `contradicting_h_mem_ids`: for contradictions, the IDs of the contradicting h_mems.
   - `source_h_mem_ids`: for reification candidates, the IDs of the memories that form the pattern.
   - `proposed_resolution`: the strategy + specific action.
   - `evidence`: the values/confidence/connectedness data that supports the finding.

4. Call `lisp_eval` to check the scan is non-trivial:
   - form: "(length (assoc \"findings\" scan_result))"
   - env: { "scan_result": <your scan output> }
   - If the result is 0, report "No issues found in {target}. Memory is clean." and exit.

### Phase 3 — Classify and propose

1. Call `render_template` to render the classification template:
   - template: `therapy/classify.j2`
   - variables: { "findings": <scan findings>, "target": "<target name>" }

2. Following the template's guidance, classify each finding:

   **For contradictions** — Festinger's three dissonance resolution strategies:

   **Reduce importance** (lower confidence):
   - Use when a contradiction is genuine (both memories are plausible) and the system can't determine which is correct.
   - Action: `memory_update` to lower confidence on both, so neither dominates recall.
   - Grounding: Festinger — "reduce the importance of dissonant cognitions."

   **Add consonant** (insert reconciling memory):
   - Use when a contradiction can be resolved by a new insight that reconciles both memories.
   - Action: `memory_insert` with a new h_mem that synthesizes the contradiction, citing both contradicting h_mems as evidence.
   - Grounding: Festinger — "add consonant cognitions." Also Nelson & Shiffrin (2013) — "traces can accumulate information across events."

   **Remove dissonant** (expire or delete):
   - Use when one memory is clearly wrong (e.g., old code status, pre-solution configuration).
   - Action: `memory_resolve_contradiction` with strategy "expire" (soft-delete) or "delete" (hard-delete).
   - Grounding: Festinger — "remove or change dissonant cognitions." Also Loftus (2005) — distorted traces should be corrected. Also the goldfish principle — don't let the past own the present.

   **For fragmentation** — propose:
   - **Merge**: if an isolated h_mem is a duplicate of a connected one, expire the isolated one.
   - **Link**: if an isolated h_mem is related but not linked, propose inserting a connecting h_mem.
   - **Keep**: if the isolation is intentional (a unique perspective), keep it but note it.

   **For miscalibrated confidence** — propose:
   - **Raise confidence**: if a h_mem has been recalled many times without contradiction, `memory_update` to raise confidence.
   - **Lower confidence**: if a h_mem has no evidence citation, `memory_update` to lower confidence.
   - **Reset to floor**: if confidence is clearly wrong, `memory_update` to 0.5 (the floor).

   **For reification candidates** — propose:
   - **Create skill**: if the pattern is a repeatable process that could guide future action, propose creating a skill (SKILL.md + templates) via the `create-skill` skill. The skill captures the lesson as proactive contextualized guidance.
   - **Create template**: if the pattern is a prompt structure or output format that could guide future generation, propose creating a .j2 template.
   - **Create rule**: if the pattern is a simple constraint or guideline (e.g., "always check X before Y"), propose adding it to the project `.rules` file or the agent's system prompt.
   - **Purge source memories**: after reification, the source episodic memories are no longer needed — propose `memory_resolve_contradiction` with strategy "delete" to purge them (cognitive load shedding).
   - **Condense source memories**: if the source memories have ongoing reference value, propose replacing them with a single high-confidence summary h_mem via `memory_insert` + `memory_resolve_contradiction` (expire the originals, keep the summary).

3. Produce a structured proposal list. Each proposal includes:
   - `finding_id`: the ID of the finding being addressed.
   - `strategy`: the resolution strategy.
   - `action`: the specific tool call(s) to execute.
   - `h_mem_ids`: the h_mems involved.
   - `reification_target`: for reification proposals, the skill/template/rule to create.
   - `reason`: why this resolution is proposed (citing the finding and the grounding).
   - `requires_approval`: true (all modifications require user approval).

### Phase 4 — User review and approval

1. Present the proposals to the user in a readable format:
   - Group by issue type (contradictions, fragmentation, miscalibrated confidence, reification candidates).
   - For each proposal, show: the finding, the proposed action, and the reason.
   - For reification proposals, show the proposed skill/template/rule content and ask the user to review it.
   - Ask the user to approve, modify, or reject each proposal.

2. Record the user's decisions. Only approved proposals are executed.

3. Call `lisp_eval` to count approved proposals:
   - form: "(length (filter (lambda (p) (eq (assoc \"approved\" p) t)) proposals))"
   - env: { "proposals": <your proposal list with user decisions> }
   - If the result is 0, report "No proposals approved. Memory unchanged." and exit.

### Phase 5 — Execute

1. For each approved proposal, execute the corresponding tool call:

   **Memory hygiene proposals:**
   - `memory_insert` for "add_consonant" and "link" strategies.
   - `memory_update` for "reduce_importance", "raise_confidence", "lower_confidence", "reset_confidence" strategies.
   - `memory_resolve_contradiction` for "remove_dissonant", "merge" strategies.

   **Reification proposals (the learning step):**
   - Create the skill/template/rule as approved by the user. Use `write_file` to write SKILL.md, .j2 templates, or .rules entries.

   **Post-reification hygiene (forgetting — runs after reification, not part of learning):**
   - **Purge**: `memory_resolve_contradiction` with strategy "delete" for each source h_mem.
   - **Condense**: `memory_insert` to create the summary h_mem, then `memory_resolve_contradiction` with strategy "expire" for each source h_mem.
   - **Keep sources**: if the user chose to keep the source memories, no action.

2. For corpus memory targets, use the corpus-specific tools to modify the chunk database.
3. For swarm memory targets, use the swarm-specific tools to modify the swarm database.

4. Record the outcome of each execution (success/failure).

5. Call `lisp_eval` to verify all executions succeeded:
   - form: "(length (filter (lambda (r) (eq (assoc \"success\" r) nil)) results))"
   - env: { "results": <your execution results> }
   - If the result is > 0, report the failures and suggest manual remediation.

### Phase 6 — Report

1. Call `render_template` to render the report template:
   - template: `therapy/report.j2`
   - variables: { "target": "<target name>", "findings": <scan findings>, "proposals": <approved proposals>, "executions": <execution results> }

2. Following the template's guidance, produce a therapy session report:
   - **Memory hygiene summary**: how many contradictions resolved, how many fragmented memories merged/linked, how many confidence values recalibrated.
   - **Reification summary**: how many skills/templates/rules created, how many source memories purged or condensed, the cognitive load shed (estimated reduction in memory database size or retrieval noise).
   - **Per-issue detail**: what was found, what was proposed, what was approved, what was executed, what was the outcome.
   - **Recommendations**: follow-up actions for the user (e.g., "run another therapy session after the next consolidation cycle", "review the new skill's effectiveness after N invocations", "consider reifying the remaining reification candidates in a future session").

3. Present the report to the user.

## Constraints

- **User sovereignty.** The memory system is transparent to the user and respects user sovereignty. The user can see what's in memory (read-only tools available to all threads), approve all modifications (no autonomous editing), run without memory loops (the zed agent has no memory), and purge any memory at any time. The system serves the user, not the other way around.
- **Therapy on curator memory must run from a Curator agent panel session.** The curator must remember the act of therapy — the forgetting, the reification, the lessons learned. Therapy run from the zed agent modifies curator memory without the curator's awareness, breaking the cybernetic loop. Forgetting works as long as it is done with awareness and has a purpose.
- **User approval required for all modifications.** The skill proposes; the user approves. No autonomous memory modification or skill creation.
- **Evidence-grounded proposals.** Every proposal must cite the specific h_mems and data that support the finding. No free-association.
- **Two distinct processes — do not conflate.** Memory hygiene (resolving contradictions, purging, condensing) is forgetting, not learning. Reification (extracting meaning, creating skills/templates/rules) is learning. Forgetting is a hygiene side-effect of successful reification, not part of the learning loop.
- **Festinger's three strategies only for contradictions.** Every contradiction resolution must use one of: reduce importance, add consonant, remove dissonant.
- **Reification requires user review of the proposed skill/template/rule content.** The user must see and approve the actual content before it is written.
- **Post-reification forgetting requires separate approval.** The user approves reification and forgetting as separate decisions — they may reify a lesson but choose to keep the source memories.
- **Curator-only writes for curator memory.** The `memory_insert`, `memory_update`, and `memory_resolve_contradiction` tools are curator MCP server tools, restricted to curator threads (enforced in `enabled_tools`). Read-only curator tools remain available to all threads.
- **No deletion without reason.** Every `memory_resolve_contradiction` call must include a reason citing the contradiction or the reification.
- **Confidence floor.** New memories inserted via therapy start at confidence 0.5.
- **Report honestly.** If no issues are found, say so. If executions fail, report the failures. Do not fabricate success.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `therapy/scan.j2` | Guide the scan phase: how to query the memory database for contradictions, fragmentation, miscalibrated confidence, and reification candidates. Defines the expected output shape (findings list). |
| `therapy/classify.j2` | Guide the classification phase: how to map findings to resolution strategies (Festinger + reification) and propose specific tool actions. Defines the expected output shape (proposal list). |
| `therapy/report.j2` | Guide the report phase: how to structure the therapy session report, including memory hygiene summary and reification summary. Defines the expected output shape (session summary). |
