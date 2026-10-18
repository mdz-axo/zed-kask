---
name: skill-maintenance
description: "Maintain existing skills: validate canonical SKILL.md instructions and companion .j2 templates, audit staleness, and compare distinct skill designs on fixed tasks, filing the result as a proposal for the operator's algedonic review."
---

# Skill Maintenance

Skill lifecycle management and maintenance. SKILL.md is the canonical source
of truth — the process instructions the agent reads and follows. .j2 templates
are companion resources that define prompt structure. Maintains skills that
already exist: validate them, audit their staleness, and compare candidate
designs (filed as proposals) before they change. Creating and translating
skills belongs to `create-skill`; finding coverage gaps to `skill-discovery`.

## The skill model

A kask skill has two artifacts, in **two different locations**:

```
.agents/skills/<name>/
└── SKILL.md              # Process instructions (source of truth)

kask/registry/templates/<name>/
└── <phase>.j2            # Prompt templates (companion resources)
```

Templates do NOT live next to the SKILL.md — `render_template` resolves
refs against the registry base path (`kask/registry/templates/`), so a
template placed in `.agents/skills/<name>/` is unreachable (see check T5).

The agent reads the SKILL.md, follows its instructions, and calls tools
(`lisp_eval`, MCP tools, `read_file`, `skill`) as directed.

## Reference models

Fagan, "Design and code inspections to reduce errors in program development," IBM Systems Journal 15(3) (1976) — the audit method's spine: defect detection separated from correction (the audit finds and scores; the score is advisory; deletion and deprecation belong to the operator), severity classification (critical/high/medium/low), and follow-up verification (an accepted proposal is applied by a delegated agent and done only when verified). The inspection is a three-role discipline — author, inspector, follow-up verifier; a decoupled critic (an agent that neither designed nor edits nor judges acceptance) is the inspector role. Goodhart's law (`onto_anchor` → derived `goodharts_law`, operator ruling 2026-09-24) governs the evaluation separation: the session that designed a candidate cannot also be the judge that accepts it. The optimize loop copies Deming's PDCA; the S13 condition vocabulary follows Toyota Kata (Rother). The check set S1–S14/T1–T5, the Algedonic board, the gemba walk, and the proposal-card workflow are project machinery — unanchored, labeled as such.

## Initial and target condition

- **Initial condition:** the target skill's canonical SKILL.md (and, for the template-logic audit, the target `.j2` with its parsed goal), the operator's task/outcome for optimize work, and — for validate — the check set S1–S14/T1–T5 itself. For optimize: the baseline behavior and the fixed, independently judged task set with expected results, specified before any candidate is designed.
- **Target condition:** validate — every check evaluated with per-check evidence, failures carrying file:line and fix suggestions; audit — every signal verified against the actual tree (or reported `unverified` without penalty) and the health score computed by the pinned `lisp_eval` form; optimize — a measured comparison over the fixed tasks with a proposal card (or the baseline kept with gaps reported), never an applied change; template-logic audit — a comparison-backed proposal or the kept baseline. A failed Check (a candidate that misses a hard contract or regresses) keeps the baseline and reports the gap.

## Step types (D/P labelling)

| Step | Type | Oracle / critique |
|------|------|-------------------|
| Validate — mechanical checks (S1–S11, T1–T5) | D | the sweep scripts, the prescreen, the render_template_tool.rs corpus tests, file reads |
| Validate — read-triage adjudication (S10 body usages, S13 loop-anatomy recognition) | P | the operator's rulings cited in the check text; the prescreen flags, the auditor adjudicates |
| Audit — signal verification | D | the actual tree and live tool surface (file:line evidence); the vague-instruction judgment is P, critiqued by the operator |
| Audit — health score | D | `lisp_eval` (the health-score form; receipts in the regression case) |
| Optimize — candidate design | P | the fixed-case comparison (the evaluator); the operator judges in the gemba walk |
| Optimize — measurement (Check) | D | the same tasks and evaluator run on every candidate; recorded harness outputs |
| Template-logic audit — goal loading, reconciliation, hard-gate counts | D | `read_file`, `lisp_eval`, recorded harness outputs |
| Template-logic audit — critique and candidate design | P | `logic-critique-template` critiqued by the separate `logic-critique-critique` render; candidates by the fixed-case comparison |
| Proposal card handoff | D | the kanban tool receipts (board ID, card ID); an explicit operator direction may re-target the filing; agent-invented fallbacks forbidden |

## When to Use

- When you need to validate a skill's SKILL.md structure and template quality.
- When you need to audit the canonical SKILL.md and referenced templates for staleness signals and health scoring.
- When you need to improve a skill's actual task performance, not merely its structural health score.

## When NOT to Use


- Authoring a new skill, or translating one from another agent system — use `create-skill` (it delegates validation back here at Phase 4).
- Mapping task patterns for coverage gaps — use `skill-discovery` (its detect-gap phase); matching tasks to installed skills is its route phase.

## Instructions

### skill-maintenance-validate

1. Validate the specified skill or all skills in `.agents/skills/` against:
   - **S1**: SKILL.md exists in `.agents/skills/<name>/SKILL.md`
   - **S2**: SKILL.md frontmatter has `name` and `description` fields
   - **S3**: SKILL.md `name` matches the directory name
   - **S4**: SKILL.md `description` is present and non-empty (1-500 chars)
   - **S5**: SKILL.md has a "When to Use" section
   - **S6**: SKILL.md has an "Instructions" section with numbered steps
   - **S7**: SKILL.md instructions reference concrete tools (`lisp_eval`,
     MCP tools, `read_file`, `render_template`, `skill`) — not abstract "the system will" language
   - **S8**: SKILL.md has a "Constraints" section
   - **S9**: No `visibility` field in frontmatter. Mechanical enforcement
     point: `kask/scripts/audit/skill-corpus-s9-s10-sweep.sh` (frontmatter
     dispatch-key sweep).
   - **S10**: SKILL.md does not use removed vocabulary (`compute_ref`,
     `action:`, `template_ref` as a manifest dispatch key, `convergence_signal`,
     `input_mapping`, `on_failure`, `ordinal:`, `category:`, a `shipped` key or
     release-state wording for skills and templates) or vestigial `steps`
     frontmatter with `id`/`tools` dispatch structure (manifest-executor
     remnant). The `render_template` tool's `template_ref` parameter, named
     in call instructions, is the live contract — not a violation. Quoting
     the removed tokens to define this check (here and in the Constraints)
     is definitional use, not a violation. Mechanical
     enforcement point: `kask/scripts/audit/skill-corpus-s9-s10-sweep.sh`
     (frontmatter dispatch keys + body key-form sweep of the no-live-contract
     tokens; `template_ref`/`action` body usages adjudicate at read-triage
     under this check).
   - **S11**: If `core: true` is declared, the name must be in
     `CORE_SKILL_NAMES` (enforced by `agent_skills` at load time)
   - **S12**: Every `lisp_eval` form pinned in a SKILL.md's instructions
     evaluates against the interpreter's builtin surface. Method:
     extract each pinned form; run it via `lisp_eval` with a stub env
     (empty list for list-shaped variables, 0 for numeric ones); an
     `unbound symbol: X` error where X is not one of your stub variables
     is a broken form (missing builtin or special form). Type or runtime
     errors over stub values are fine — the form's symbols resolved.
   - **S13**: Every SKILL.md body states an observable initial condition
     and target condition and contains a bounded PDCA (Plan→Do→Check→Act)
     loop as its core process. The loop lives in the SKILL.md body —
     templates are leaves, never its carrier. Check measures the gap to
     that target and names a threshold or convergence criterion; Act
     closes on a passing first check or re-enters a named phase on a gap,
     within a bound. A body with phases but no observed Check→Act return,
     or with no initial/target condition, fails this check.
     Loop vocabulary recognition (auditor guidance): the loop may be
     carried in any of these forms — (a) explicit PDCA phases; (b) a
     convergence gate (lisp_eval or otherwise) plus a named re-entry
     point; (c) bounded rounds or escalation (max rounds, max
     attempts, escalate-after-N); (d) cycle vocabulary
     (red-green-refactor with routing re-entry, zero-delta abort,
     per-turn gate with a revision bound, elimination-to-survivor with
     a materiality guard). In every form the auditor requires the four
     anatomy parts: a named Check signal, a threshold or convergence
     criterion, a bound (a maximum iteration count; a stability/abort
     condition may stop earlier but does not replace it), and an Act re-entry (the phase that re-enters, or an
     explicit terminal action: escalate/halt/report). A loop whose
     Check→Act lives only in a template's purpose text fails — the
     loop lives in the body (composition law).
     The former DR-S13a exemptions were superseded by the operator's
     2026-09-25 all-skills direction. A role guide or one-shot actuator
     can close after a single passing Check; it is not exempt from
     stating the initial/target condition and what a failed Check does.
     Do not add a useless second run merely to display PDCA vocabulary.
     Mechanical enforcement point: `kask/scripts/audit/skill-corpus-prescreen.sh`
     checks that every body states both an initial and a target condition;
     `kask/scripts/check-skill-corpus-dp-labels-selftest.sh` pins canonical,
     compact, initial-only, target-only, and frontmatter-only controls. This is
     the syntax floor only — Check signal, threshold, bound, and Act re-entry
     remain read-triage judgments under this check.
   - **S14**: No model-typed deterministic output. For every instruction
        step and template output: a value that is a function of inputs
        already available to a deterministic engine — a count or other
        arithmetic over in-scope data, a verification verdict on a quoted
        substring, a parsed value — is computed by that engine, never typed
        by the model. Classify each such output: template-computable (render
        via Jinja filters over the template's own variables —
        `selectattr`|`length`, the therapy report-counts precedent, commit
        54eabe4f8e), `lisp_eval`-computable (a form over the data), or
        genuinely semantic (judgment — keep). A model-filled placeholder
        over in-scope data ("<count of ...>" while the list is a template
        variable) is the canonical violation. Render call points carry
        complete, copyable context shapes (a fenced block naming every
        contract field at the call point), never prose field lists an agent
        must reconstruct — reconstruction drops fields (the prompt-enhance
        friction class, commit 992f456790). Mechanical enforcement point:
        none as a standalone script — the D/P label presence check
        (prescreen) already enforces the labels exist; S14 is the
        content-level complement (the labelled D steps are actually D),
        judged at read-triage and by the periodic systematic sweep (card
        c30df9f3 is the first corpus-wide run).
      - **T1**: Each `.j2` template referenced in SKILL.md instructions exists
     in the skill's registry template crate
     (`kask/registry/templates/<name>/`)
   - **T2**: Each `.j2` template carries a `{# goal: ... #}` annotation
     describing its purpose — verified deterministically by `read_file` +
     `lisp_eval` gate **`SM-1`** in the template-logic audit; a comment header
     in any other form is a fail (an unparseable goal is unauditable). Mechanical
     enforcement point: `kask/scripts/audit/skill-corpus-prescreen.sh` (goal
     presence, length, content-overlap and goal-wrap pre-screen over the whole
     corpus, plus the body-side D/P labelling presence check — P8.4's audit
     floor; flagged items get read-triage under this check).
   - **T3**: Each `.j2` template defines expected output fields (as comments
     or schema description)
   - **T4**: a `.j2` template carries at most one `[inference]` block — the
     header, holding only `contract.input`/`contract.output`, terminated by
     a lone `---` line. `render_template` reads `contract.input`
     (`validate_contract_inputs`) and strips the header; nothing reads any
     other key. A `visibility:` key or a body `[inference]` parameter
     stanza (temperature/work_effort/verbosity/thinking_budget) is
     performative and a fail (removed corpus-wide 2026-09-28). A header
     missing its `---` terminator is a fail. Mechanically enforced by
     `corpus_templates_strip_without_leaking_metadata`
     (crates/agent/src/tools/render_template_tool.rs).
   - **T5**: If a template is referenced for rendering via `render_template`,
     it is reachable from the `render_template` base path (registry templates
     directory, not the skill directory). Mechanically enforced corpus-wide
     by `corpus_templates_render_without_error` and
     `corpus_includes_resolve_and_strip`
     (crates/agent/src/tools/render_template_tool.rs).
2. Evaluate every check for every targeted skill without omissions, and verify 19-check ID completeness deterministically via `lisp_eval` gate **`SM-2`**:
   - form: `(and (= (length evaluated_checks) 19) (member "S1" evaluated_checks) (member "S2" evaluated_checks) (member "S3" evaluated_checks) (member "S4" evaluated_checks) (member "S5" evaluated_checks) (member "S6" evaluated_checks) (member "S7" evaluated_checks) (member "S8" evaluated_checks) (member "S9" evaluated_checks) (member "S10" evaluated_checks) (member "S11" evaluated_checks) (member "S12" evaluated_checks) (member "S13" evaluated_checks) (member "S14" evaluated_checks) (member "T1" evaluated_checks) (member "T2" evaluated_checks) (member "T3" evaluated_checks) (member "T4" evaluated_checks) (member "T5" evaluated_checks))`
   - env: `{ "evaluated_checks": <array of check IDs evaluated> }`
3. Include specific evidence for any fail results (file path, line number).
4. Provide actionable fix suggestions for any failures.
5. Respond with a JSON object containing validation results and fix suggestions.

### skill-maintenance-audit

0. **Predict before auditing (calibration, P).** From the skill's size, age of last edit (`git log -1 --format=%cs -- .agents/skills/<name>/SKILL.md`) and template count alone, state the expected number of distinct verified defects per severity and the expected band, with one sentence of basis. Record before reading the body; step 5 reconciles.
1. Read `.agents/skills/<name>/SKILL.md` as the canonical process; if it is missing, report that as a critical loss of the skill. Inspect only the `.j2` templates the body references under `kask/registry/templates/<name>/`. Manifests do not dispatch skills and must not supply health penalties, retirements, or a substitute for a missing SKILL.md.
2. Confirm each signal against the actual tree and live tool surface, citing file:line (or the expected path and directory listing for a missing file): missing SKILL.md; removed/nonexistent tool references; referenced templates that are missing or unreachable; removed manifest-dispatch vocabulary used as instructions (not historical quotations or live `render_template` parameters); missing Constraints; vague instructions with no actionable tool steps; malformed `[inference]` headers or more than one `[inference]` block (per T4); a skill that creates temporary state (local agent cards or swarms, kanban boards, scratch or `/tmp` files, jobs) with no step that deletes it or lists what it deliberately keeps (medium; `kask/docs/architecture/standardized-artifact-storage.md` Cleanup rule); a skill that names an artifacts folder after a UUID, hash or random run id instead of a readable `{YYYY-MM-DD}-{subject}` name (medium; same document, Readable names rule); or a concrete contradiction with the runtime or project constraints. Do not penalize speculative or unverified claims. Keep real broken references and malformed templates as findings.
3. Score each **distinct verified defect** once from 1.0, floor at 0.0: critical −0.50 (missing SKILL.md, nonexistent tool, missing/unreachable referenced template); high −0.15 (contradictory instructions or malformed template contract); medium −0.10 (removed vocabulary used as operative dispatch, missing Constraints, vague instructions); low −0.05 (verified minor staleness with a specific behavioral impact). Do not double-count the same root cause. Include evidence per penalty and compute the score with `lisp_eval` (always registered): form `(max 0 (- 1 (+ (* 0.50 critical) (* 0.15 high) (* 0.10 medium) (* 0.05 low))))`, env the counts of distinct verified defects per severity. No verified defects → 1.0.
4. Classify 0.00–0.19 as retirement candidate, 0.20–0.49 as critical revision, 0.50–0.79 as stale warning, 0.80–1.00 as active. A score is advisory: propose repair first and never delete or mark a skill deprecated without explicit operator approval. Explain when a low score is caused by multiple repairable faults.
5. Respond with staleness report, health score and traceable penalties, coverage limitations (checks not performed), the predicted-vs-verified defect counts from step 0 with the gap (`lisp_eval` over the two count vectors) and the predicted vs computed band, and recommendations. For any unverified signal report `unverified` separately without a penalty. A score of 1.0 establishes only the absence of verified staleness defects, not skill effectiveness; route requests to improve behavior to `skill-maintenance-optimize`.

### Proposal card handoff (all proposal-producing phases)

Use the single **Algedonic review** board, not a file or a second board. `kanban_board_list` must succeed before treating the board as absent; if it returns exactly one matching board, use its ID. If none exists, call `kanban_board_create` with name `Algedonic review`, default columns, and the shared `idempotency_key: "algedonic-review-board"` so concurrent first-use calls cannot create a second board; if more than one matches, stop and request resolution instead of guessing. For `kanban_task_create`, supply the board ID, a descriptive title, a description with target, full diff, evidence, missing evidence and verification criteria, `criteria` listing the observable checks, `advances: []` unless citing a known goal criterion, and an `idempotency_key` reused on retries. Keep the returned card ID; attach existing output files or URLs using `kanban_task_add_deliverable` when applicable. Failed creation is a blocked filing, never a file fallback. An explicit
   operator direction may re-target the filing (e.g. a program's gate
   reports replacing the card for its duration); agent-invented fallbacks
   remain forbidden.

### skill-maintenance-optimize — Plan → Do → Check → Act (proposal only)

This loop runs within one session and ends in a **proposal card**, never an applied change or a verdict. Skill evaluation belongs to the operator in `algedonic-review`'s gemba walk (operator ruling 2026-09-24; Goodhart's law): the session that designed a candidate cannot also be the judge that accepts it.

**Registry routing (repair plan §P8.3 — the single record path).** A fixed-task comparison is an experiment: render `experimentation/experiment-protocol` and declare it with `experiment_propose` (layer `skill`; genotype_refs the compared SKILL.md and its companion `.j2` templates together — the composite genotype, §P8.7-Q2; eval_set the predeclared fixed tasks; fitness_fn the chosen measures; noise_band the declared cost band; prediction the expected winner with confidence; budget the run budget; experiment_key a stable comparison key); register every candidate — the baseline included — with `variant_register` (parent lineage where a candidate derives from another); record each candidate's measured results with `fitness_record` (runs the recorded harness report references — never simulated). Record the outcome with `selection_record` when the operator's card verdict is known — verdict `selected` with the proposed candidate on acceptance, `rejected` with reasons otherwise, `algedonic_reference` the card ID; the deciding session writes it, and a session that files and exits without a verdict leaves the experiment `running`.

1. **Plan:** Read the canonical SKILL.md and referenced .j2 templates via `read_file`; obtain the operator's task/outcome and relevant historical constraints. Specify baseline behavior and a fixed, independently judged set of representative, negative, boundary and held-out tasks with expected results before revising anything. Select success, regression, safety and cost measures and a feasible run budget. `skill-maintenance-audit` and `skill-maintenance-validate` identify defects, but passing them does not establish task success. If the objective or oracle cannot be established, ask for it or return `unverified`; do not optimize to a proxy health score.
2. **Do:** Render `skill-maintenance-optimize` to lay out at most four genuinely different candidates: unchanged baseline, surgical repair, remove/merge/simplify, and replacement of the process architecture. Permit elimination of a template, reallocation of responsibilities or a new skill boundary if the task justifies it; preserve only externally required contracts. Record why a candidate class is inapplicable rather than forcing a change. For .j2 reasoning defects, run the template-logic audit below on the template as a leaf; the optimize loop owns SKILL.md changes and integration. A template-contract change lands with its live render pin — a render on known inputs asserting the changed behavior, its counts checked against hand-computed expectations — and the corpus suites green; the pin is a step of this Do phase, not a precedent. Do not use a legacy manifest as the process specification.
3. **Check (measure, do not judge):** Run baseline and candidate implementations against the **same** tasks and evaluator when a deterministic harness can run them; validate S1–S14/T1–T5 and render reachability for every finalist. Call `lisp_eval` to reconcile task IDs, run counts, arithmetic and hard-gate results from recorded data; retain measured run evidence with the proposal card (attach existing harness artifacts using `kanban_task_add_deliverable` when applicable). A self-scored answer, static template overlap, or a green structural check is not evidence of improved task outcomes. Missing runs and unavailable harnesses are `unverified`, never wins. The local swarm runtime executes a card's declared skills through `host/skill`, so `swarm_eval_agent_local` can run a skill-declaring agent on the fixed task set.
4. **Formal gate when applicable:** Invoke `lean-prover` only if a candidate depends on a precisely stated finite decision rule or safety invariant whose proof changes the choice (e.g. no unapproved write transition). State assumptions, compile the exact declaration in the pinned Lean version, inspect `#print axioms` and negative controls, and test that the production decision rule matches the model. Lean cannot prove semantic quality or an absolute optimum. If no such obligation exists, record `not applicable`; if needed but uncheckable, record `unverified`.
5. **Act (card or drop):** A candidate that satisfies hard constraints and has measured evidence becomes a proposal card using the Proposal card handoff above. Give it a title naming the skill and change, and a description containing the full diff, predeclared tasks, measured before/after (or `unverified`), verification criteria and open falsifiers. Attach existing harness artifacts with `kanban_task_add_deliverable` when relevant. Keep the returned card ID as the proposal reference. Otherwise drop the candidate and keep the baseline. For a new falsifier, revise the design and rerun the same held-out cases at most once in this session. Do not edit the SKILL.md, record a verdict, or claim an improvement in this session. The proposal is decided in the gemba walk by the operator, or by the Curator under an operator grant; an accepted proposal is applied by a delegated agent and is done only when verified (`algedonic-review`, Proposal authority and done).

### skill-maintenance template-logic audit (formerly skill-logic-audit)

Audits one `.j2` template's logic against its `{# goal: ... #}` annotation and the SKILL.md phase that invokes it, and files a comparison-backed proposal (folded back in 2026-09-25; it was split out 2026-08-14). A template is a step-leaf: its goal must serve the invoking phase, its inputs must match what that phase passes, and its outputs must feed the phase that consumes them. SKILL.md bodies are not targets here — they go through `skill-maintenance-optimize`. A legacy `manifest.yaml` is inert: audit only its stated textual goal, when explicitly asked.

**D/P labelling.** Goal loading, case/candidate reconciliation and hard-gate counts are D (`read_file`, `lisp_eval`, recorded harness outputs). Critique and candidate design are P: `logic-critique-template` is critiqued by the separate `logic-critique-critique` render, and candidates by the fixed-case comparison. The operator judges in the algedonic review (Goodhart's law, `onto_anchor` → derived `goodharts_law`).

1. **Plan — load the goal (`SM-1`, D).** `read_file` the target `.j2` template and verify its `{# goal: ... #}` annotation deterministically via `lisp_eval` gate **`SM-1`**:
   - form: `(and (ends-with ".j2" target_path) (string-contains "{# goal:" target_content) (string-contains "#}" target_content) (>= goal_char_len 10))`
   - env: `{ "target_path": <path>, "target_content": <file content>, "goal_char_len": <character length of extracted goal text> }`
   If `SM-1` returns `false`, report the template unauditable and stop (never invent a goal). Read the invoking phase and the inputs it passes and outputs it consumes; record any mismatch. Fix the acceptance cases before revising: one representative success, one failure, and for a live template one downstream handoff, with expected outputs, hard contracts, costs and one evaluation method for every candidate. No fixtures or trustworthy evaluator → mark the comparison `unverified`.
2. **Do — critique and generate.** Render `skill-maintenance/logic-critique-template` with `goal`, `target_path`, `target_content`, `template_type`, `invoking_phase`, and `acceptance_cases`; cite concrete defects against goal and phase; reject style-only concerns. `kask/scripts/audit/skill-corpus-prescreen.sh` and `skill-corpus-contract-audit.sh` give shape leads, not proof. Render `skill-maintenance/logic-critique-critique` with `goal` and `prior_critique` to drop unsupported concerns. Generate at most four distinct candidates — unchanged baseline, localized repair, subtraction, a replacement structure — and say why a class is inapplicable rather than forcing it.
3. **Check — compare.** Render `skill-maintenance/logic-compare-candidates` with `goal`, `invoking_phase`, `acceptance_cases`, `candidates`, and `observations`; run the same cases on every candidate through the available harness, and check rendering, contract and handoff shape separately. Judge semantic correctness against the predeclared expected behavior or independent human judgment, never the candidate's own critique. Call `lisp_eval` on the recorded case/candidate records for completeness, counts, hard gates and arithmetic; missing observations or a failed hard gate block an improvement claim. Invoke `lean-prover` only for an exact, finite decision rule (e.g. selection never permits an unapproved edit); Lean does not prove a prompt good.
4. **Act — card or keep.** Name a candidate only if it clears every hard contract, improves the fixed outcome over the baseline, and has no unacceptable regression; otherwise keep the baseline and report the gaps. Re-enter generation at most once for a newly found falsifier on the same held-out cases. For a qualifying candidate, render `skill-maintenance/logic-compose-proposal` with `goal`, `target_path`, `original_content`, `valid_concerns`, `comparison`, `winning_content`, and `user_counter_proposal` (the simplest passing candidate, with its full unified diff), recheck its goal, contract and rendering. Use the Proposal card handoff above with a title naming the target change and a description containing the target, goal, full diff, case evidence, verification criteria and open falsifiers. Attach existing artifacts via `kanban_task_add_deliverable` when relevant; retain its card ID. Do not edit the target; the operator accepts, rejects or counters it on the card in the gemba walk, and an accepted diff is applied only if it still matches the current file.

## Regression case

Validate both directions: render `skill-maintenance/skill-maintenance-validate` with `target` and `skill_name` naming a known-good skill (e.g. `superforecasting`) and a known-bad fixture (a SKILL.md missing When to Use and Constraints) — the good target passes S5/S8, the bad target fails them with file:line evidence, and `SM-2` verifies all 19 check IDs (`S1`–`S14`, `T1`–`T5`) are present. Audit: render `skill-maintenance/skill-maintenance-audit` with `skill_name` and `workspace_context`, then run the health-score form three ways via `lisp_eval` — `{0,0,0,0}` → 1.0, one critical → 0.5, `{2 critical, 1 high, 1 medium, 1 low}` → 0 (the floor). Template-logic chain: run `read_file` + `SM-1` on a `target_path` and `target_content` carrying a `{# goal: ... #}` block (`true`) and one without (`false`); render `logic-critique-template` then `logic-critique-critique` over the critique (the decoupled second pass). Optimize: render `skill-maintenance-optimize` with `skill_name`, `objective`, `baseline`, `tasks`, `candidates`, `observations`. The `SM-1`, `SM-2`, and health form receipts above are executed live through `lisp_eval`.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `logic-critique-template.j2` | Template-logic audit: adversarial, grounded critique of a template against its goal, invoking phase and fixed cases. |
| `logic-critique-critique.j2` | Template-logic audit: keep only concerns that link a concrete defect to the goal. |
| `logic-compare-candidates.j2` | Template-logic audit: compare baseline and distinct candidates on fixed cases from observed evidence; names a candidate to propose, never applies one. |
| `logic-compose-proposal.j2` | Template-logic audit: compose the comparison-backed diff for a proposal card, or keep the baseline. |
| `skill-maintenance-validate.j2` | Validate a skill or all skills against S1–S14 / T1–T5 with per-check evidence and fix suggestions. |
| `skill-maintenance-audit.j2` | SKILL.md-first staleness audit: verified dead tools, missing referenced templates, removed dispatch vocabulary, vague instructions, malformed templates; traceable health penalties and advisory recommendations. |
| `skill-maintenance-optimize.j2` | Compare distinct SKILL.md and companion-template architectures on fixed task outcomes, hard constraints and regressions, and package the measured comparison as a proposal for the operator's algedonic review; never select or apply a winner. |

To render a template, call the `render_template` tool with the template ref (e.g., `skill-maintenance/skill-maintenance-validate`) and a context object with the required variables.

Copyable `render_template` context blocks (field names and types from each template's `[inference] contract`):

`skill-maintenance/logic-critique-template`:
```json
{
  "goal": "<the target template's {# goal: ... #} annotation text>",
  "target_path": "<repo-relative path to the .j2 under audit>",
  "target_content": "<the full current content of the .j2>",
  "template_type": "<live template or legacy manifest>",
  "invoking_phase": "<the SKILL.md phase that invokes the template>",
  "acceptance_cases": ["<fixed acceptance case: representative success, failure, or downstream handoff>"]
}
```

`skill-maintenance/logic-critique-critique`:
```json
{
  "goal": "<the target template's {# goal: ... #} annotation text>",
  "prior_critique": { "flaws": ["<flaw>"], "valid_concerns": ["<goal-anchored concern>"], "spurious_concerns": ["<spurious concern>"], "verdict": "<the prior critique's verdict>" }
}
```

`skill-maintenance/logic-compare-candidates`:
```json
{
  "goal": "<the target template's {# goal: ... #} annotation text>",
  "invoking_phase": "<the SKILL.md phase that invokes the template>",
  "acceptance_cases": ["<fixed acceptance case: representative success, failure, or downstream handoff>"],
  "candidates": ["<candidate id and artifact path>"],
  "observations": ["<recorded observation with evaluator provenance>"]
}
```

`skill-maintenance/logic-compose-proposal`:
```json
{
  "goal": "<the target template's {# goal: ... #} annotation text>",
  "target_path": "<repo-relative path to the .j2 under audit>",
  "original_content": "<the full current content of the .j2>",
  "valid_concerns": ["<goal-anchored concern kept by the critique-critique pass>"],
  "comparison": { "comparisons": ["<per-candidate comparison>"], "proposed_candidate": "<candidate named by the comparison, or null>", "proposal_status": "<status>", "evidence_gaps": ["<missing observation or failed hard gate>"] },
  "winning_content": "<the simplest passing candidate's full content>",
  "user_counter_proposal": "<the operator's counter-proposal, or null>"
}
```

`skill-maintenance/skill-maintenance-validate`:
```json
{
  "target": "<the target to validate>",
  "skill_name": "<the skill name being validated, or null>"
}
```

`skill-maintenance/skill-maintenance-audit`:
```json
{
  "skill_name": "<the skill name to audit>",
  "workspace_context": {}
}
```

`skill-maintenance/skill-maintenance-optimize`:
```json
{
  "skill_name": "<the skill name being optimized>",
  "objective": "<the operator's task/outcome for the optimization>",
  "baseline": {},
  "tasks": ["<predeclared task with expected result: representative, negative, boundary, or held-out>"],
  "candidates": ["<candidate design: id and artifact path>"],
  "observations": ["<recorded observation from running the fixed tasks on a candidate>"]
}
```


## Constraints

- SKILL.md is the source of truth. When SKILL.md and templates disagree,
  SKILL.md wins.
- `lisp_eval` is available for deterministic computation. Use it for
  convergence signals, invariant checks, scoring, and arithmetic on
  structured data.
- The interpreter supports prefix `(+ a b)` and infix `a + b` operator
  notation. Use infix for simple scoring, prefix for complex nested logic.
- No `visibility` field in frontmatter.
- SKILL.md must not use removed vocabulary — the S10 check list is
  authoritative (including `category:` and the vestigial `steps`
  frontmatter); quoting the tokens to define the check is definitional
  use, not a violation.
- Skills and templates are evolving drafts with no release state (operator
  ruling 2026-09-26): never describe a skill or template as shipped,
  unshipped, released or done. "Shipped" survives only as Rust identifiers,
  whose change cadence is quarterly or annual, not weekly or monthly.
- Core skills (`core: true`) must have names in `CORE_SKILL_NAMES`.
- Material SKILL.md process changes are never self-accepted. This skill
  files them as cards on **Algedonic review**; the operator
  accepts or rejects them in the gemba walk on that board, where the
  verdict is recorded. Measurements travel with the proposal as evidence;
  where no harness could run, the proposal says **unverified**.
  An edit that cannot name a measurable behavior change is a documentation
  edit — label it as such.
