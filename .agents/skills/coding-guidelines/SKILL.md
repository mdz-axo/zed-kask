---
name: coding-guidelines
core: true
description: "Behavioral guardrails for LLM coding based on Karpathy's four principles: Think Before Coding, Simplicity First, Surgical Changes, Goal-Driven Execution. An Encoded Preference skill: constrains HOW the agent codes, not WHAT it codes."
audience: platform
---


# Coding Guidelines

Behavioral guardrails for LLM coding based on Karpathy's four principles: Think Before Coding, Simplicity First, Surgical Changes, Goal-Driven Execution. An Encoded Preference skill: constrains HOW the agent codes, not WHAT it codes.

## Reference model

The four principles are from forrestchang's `CLAUDE.md` (github.com/forrestchang/andrej-karpathy-skills), derived from Andrej Karpathy's public observations on LLM coding failure modes — the wording is forrestchang's, the diagnosis Karpathy's. The seven anti-patterns are this skill's synthesis: six trace to the same CLAUDE.md's bullets; unrequested logging/telemetry is a kask-side addition.

## D/P labelling

The assessment and its constrained plan (step 1), and the audit's violations and four principle scores (step 3), are P — judgment, critiqued by the operator and by the correction pass. Everything the two gates decide is D: `CG-1` counts the criteria and checks each plan step's quote, line estimate and file list; the **Change scope** filter (`jq`) derives the files changed from `git diff --numstat` and sets them against the plan; `CG-2` validates the audit's vocabulary, computes the mean, picks out the critical violations and counts the audit passes. Both gates take the templates' outputs as returned. No count, mean, selection or flag is typed.

## Initial and target condition

- **Initial condition:** the user's coding requirement, the code or tree it applies to, and a run directory holding the change baseline captured before any edit.
- **Target condition:** `CG-1` returns `pass` before implementation, and `CG-2` returns `pass` after it — no critical violation, mean principle score at least 0.7, and no changed file outside the plan or inside its `files_not_to_touch`. A failing audit applies its corrections and is audited once more; a second failure is delivered with its reasons rather than silently accepted.

## When to Use

- Before implementing a coding task, when you need to surface hidden assumptions, simplicity risks, and scope creep warnings
- When transforming a vague coding request into verifiable success criteria with a minimum-step plan
- When generating implementation directives that require surgical file-level scope and explicit guardrails
- After writing code or producing a diff, when you need to audit compliance against the four principles
- When the agent is about to touch code and must be constrained to minimum, surgical, goal-driven changes
- When multiple interpretations of a task exist and silent selection would violate Think Before Coding

## When NOT to Use

- Reviewing a change against its stated spec — use `code-review`; it owns adjudication with severity, falsifiers, and file:line citations.
- Non-coding work — the four principles and seven anti-patterns are defined over code.
- When a task constraint outranks a principle — the task wins, the shape stays (the skill's own override logic).

## Instructions

1. **Assess and plan before implementing (`guidelines-assess`).** Render `coding-guidelines/guidelines-assess` and execute it: analyze the task against the four principles before any implementation begins. Surface every hidden assumption, flag every over-engineering risk, define 2–5 verifiable success criteria, and produce a `constrained_plan` where each step specifies `files_to_touch` (no more, no less), `files_not_to_touch`, `lines_estimate`, `traces_to` (the task's own words that the step serves, quoted exactly) and a verification checkpoint, plus `guardrails`. Treat the seven canonical anti-patterns (`anti-patterns.j2`) as explicitly forbidden: unsolicited docstring/formatting changes, single-use abstractions, unrequested flexibility, adjacent-code refactoring, impossible-scenario error handling, unrequested logging/telemetry, and style changes outside task scope. Do not implement anything in this step.

2. **Gate the plan (`CG-1`), then implement it.** Run `CG-1` on the assess output. `revise` names what is wrong; re-render the assessment once with those reasons stated in `code_context`, and run the gate again. A second `revise` goes to the operator with the reasons — do not implement an ungated plan. On `pass`, create the run directory and capture the change baseline (see *Change scope*) before the first edit, then implement the plan.

3. **Audit the implementation (`guidelines-verify`).** Capture the change again and run the **Change scope** filter. Render `coding-guidelines/guidelines-verify` with the assess output's `assumptions`, `success_criteria` and `constrained_plan`, the diff text, and `scope.json` as `scope`; execute it. It checks that assumptions were stated explicitly, multiple interpretations were presented, unclear points were questioned, and simpler approaches were considered; that every feature was explicitly requested, no single-use abstraction exists, the solution is minimum code, and no speculative feature or impossible-scenario error handling remains; that every changed line traces to the user's request, adjacent code is untouched, existing style is matched, orphan imports from your changes are removed, and pre-existing dead code was left alone (mentioned, not deleted); and that success criteria are defined and verifiable, tests exist for stated goals, and each criterion can be verified independently. It returns violations — each with principle, severity, location and its own correction — and a score per principle (1.0 = full compliance, 0.0 = severe violation).

4. **Close the loop (`CG-2`).** Run `CG-2` on the audit output and `scope.json`, then append the audit record. Act on `verdict`:
   - `pass` — deliver.
   - `correct` — apply each violation's `correction`, and undo or justify to the operator each file under `unplanned_files` and `forbidden_files_touched`; then repeat step 3. This happens at most once.
   - `stop_bound` — this was the second failing audit, or a third was attempted. Deliver the work with the violations, the reasons and the score; the failure is surfaced, never silently passed.
   - `no_implementation` — no file changed, so there is nothing the post-implementation gate can pass. Report that the plan was not implemented.
   - `invalid` — the audit used a principle or severity outside the vocabulary, or a score that is not a number from 0 to 1. Re-render the audit once; a second `invalid` goes to the operator. An invalid audit is not recorded and does not use up the correction pass.

## Run directory and change scope

Each task has one run directory, `~/Documents/zk-data/skills/coding-guidelines/{date}-{run}/` (`{run}` a readable slug naming the task). Create it with `terminal` — the built-in file tools cannot write there:

```
mkdir -p <run_dir> && touch <run_dir>/before.numstat <run_dir>/after.numstat <run_dir>/audits.jsonl
```

Save the assess output as `<run_dir>/assess.json`. Capture the change with the command below, redirected to `before.numstat` once, before the first edit, and to `after.numstat` before each audit:

```
{ git diff --numstat --no-renames HEAD; git ls-files --others --exclude-standard | awk '{print "new\tnew\t" $0}'; } > <run_dir>/after.numstat
```

The baseline is what makes the comparison yours: other sessions hold uncommitted files in the same tree, and a file whose line is identical in both captures was not changed by this task. To audit a change that is already committed, leave `before.numstat` empty and write `git show --numstat --no-renames --format= <commit>` to `after.numstat`.

### Change scope

Save the filter below verbatim as `<run_dir>/change-scope.jq`, and run it before each audit:

```
jq -n --rawfile before <run_dir>/before.numstat --rawfile after <run_dir>/after.numstat --rawfile audits <run_dir>/audits.jsonl --slurpfile assess <run_dir>/assess.json '$ARGS.named' | jq -c -f <run_dir>/change-scope.jq > <run_dir>/scope.json
```

```jq
def lines: split("\n") | map(select(length > 0));
def file: split("\t") | if length == 3 then .[2] else error("not a git --numstat line: \(join(" "))") end;
(.before | lines) as $before
| (.after | lines | map(select(. as $l | $before | index($l) | not) | file)) as $changed
| (.audits | lines | map(fromjson)) as $audits
| if all($audits | to_entries[]; .value.audit == .key + 1 and (.value.verdict | type) == "string")
  then . else error("audit record out of sequence or without a verdict") end
| ([.assess[0].constrained_plan[].files_to_touch[]] | unique) as $planned
| ([.assess[0].constrained_plan[].files_not_to_touch[]] | unique) as $forbidden
| {changed:$changed, unplanned:($changed - $planned), untouched_planned:($planned - $changed),
   forbidden_touched:[$changed[] | select(. as $f | $forbidden | index($f))],
   prior_audits:($audits | map(.verdict))}
```

`scope.json` is the audit template's `scope` input and the `CG-2` gate's, whole: `changed` (the files whose numstat line differs from the baseline), `unplanned` (changed, in no step's `files_to_touch`), `forbidden_touched` (changed, though a step lists it under `files_not_to_touch`), `untouched_planned` (planned, not changed), and `prior_audits` (the verdicts already recorded). It refuses a line that is not `git --numstat` output and an audit record out of sequence. A file another session changes between the two captures shows as `unplanned`: say so to the operator rather than editing the captures.

## Gates

Both gates take the templates' outputs as returned. Type no count, mean or selection into an env.

### CG-1 — the plan is bounded and every step answers the task

```lisp
(begin (define text-of (lambda (row key) (if (stringp (assoc key row)) (assoc key row) ""))) (define quoted (lambda (row) (let ((q (text-of row "traces_to"))) (and (> (length q) 0) (<= (length q) (length task_description)) (string-contains q task_description))))) (define untraced (lambda (rows) (if (is_null rows) (list) (if (quoted (car rows)) (untraced (cdr rows)) (cons (text-of (car rows) "step") (untraced (cdr rows))))))) (define oversized (lambda (rows) (if (is_null rows) (list) (if (and (numberp (assoc "lines_estimate" (car rows))) (<= (assoc "lines_estimate" (car rows)) 50)) (oversized (cdr rows)) (cons (text-of (car rows) "step") (oversized (cdr rows))))))) (define fileless (lambda (rows) (if (is_null rows) (list) (if (is_null (assoc "files_to_touch" (car rows))) (cons (text-of (car rows) "step") (fileless (cdr rows))) (fileless (cdr rows)))))) (let ((criteria (length success_criteria)) (untraced-steps (untraced constrained_plan)) (oversized-steps (oversized constrained_plan)) (fileless-steps (fileless constrained_plan))) (let ((reasons (append (if (and (>= criteria 2) (<= criteria 5)) (list) (list "criteria_count")) (if (is_null constrained_plan) (list "empty_plan") (list)) (if (is_null untraced-steps) (list) (list "untraced_steps")) (if (is_null oversized-steps) (list) (list "oversized_steps")) (if (is_null fileless-steps) (list) (list "steps_without_files"))))) (list (list "verdict" (if (is_null reasons) "pass" "revise")) (list "reasons" reasons) (list "criteria" criteria) (list "untraced_steps" untraced-steps) (list "oversized_steps" oversized-steps) (list "steps_without_files" fileless-steps)))))
```

- env: `{ "task_description": <the text passed to the assess render, unchanged>, "success_criteria": <assess output>, "constrained_plan": <assess output> }`
- `pass` — two to five success criteria, a non-empty plan, and every step quotes the task, estimates at most 50 lines and names at least one file.
- `revise` — `reasons` lists what failed: `criteria_count`, `empty_plan`, `untraced_steps` (a step whose `traces_to` is missing, empty or not found in the task text), `oversized_steps` (over 50 lines, or an estimate that is not a number), `steps_without_files`. The three step lists name the steps.
- The quote check is exact: case, spacing and punctuation must match the task text. It shows a step points at words the user wrote; whether the step is what those words ask for remains judgment.

### CG-2 — the audit, the changed files and the bound

```lisp
(begin (define severities (list "critical" "high" "medium" "low")) (define principles (list "think-before-coding" "simplicity-first" "surgical-changes" "goal-driven-execution")) (define text-of (lambda (row key) (if (stringp (assoc key row)) (assoc key row) ""))) (define malformed (lambda (rows) (if (is_null rows) 0 (+ (if (and (member (text-of (car rows) "severity") severities) (member (text-of (car rows) "principle") principles)) 0 1) (malformed (cdr rows)))))) (define critical (lambda (rows) (if (is_null rows) (list) (if (string= (text-of (car rows) "severity") "critical") (cons (text-of (car rows) "description") (critical (cdr rows))) (critical (cdr rows)))))) (define score (lambda (key) (assoc key compliance_score))) (define score-ok (lambda (key) (and (numberp (score key)) (>= (score key) 0) (<= (score key) 1)))) (let ((scores-ok (and (score-ok "think_before_coding") (score-ok "simplicity_first") (score-ok "surgical_changes") (score-ok "goal_driven_execution"))) (bad-rows (malformed violations)) (audit (+ (length (assoc! "prior_audits" scope)) 1)) (changed (assoc! "changed" scope)) (unplanned (assoc! "unplanned" scope)) (forbidden (assoc! "forbidden_touched" scope))) (cond ((> audit 2) (list (list "verdict" "stop_bound") (list "audit" audit))) ((or (not scores-ok) (> bad-rows 0)) (list (list "verdict" "invalid") (list "malformed_violations" bad-rows) (list "scores_valid" scores-ok) (list "audit" audit))) (t (let ((overall (/ (+ (score "think_before_coding") (score "simplicity_first") (score "surgical_changes") (score "goal_driven_execution")) 4)) (critical-rows (critical violations))) (let ((reasons (append (if (is_null critical-rows) (list) (list "critical_violations")) (if (>= overall 0.7) (list) (list "below_floor")) (if (is_null unplanned) (list) (list "unplanned_files")) (if (is_null forbidden) (list) (list "forbidden_files_touched"))))) (list (list "verdict" (cond ((is_null changed) "no_implementation") ((is_null reasons) "pass") ((>= audit 2) "stop_bound") (t "correct"))) (list "reasons" reasons) (list "overall" overall) (list "critical" critical-rows) (list "unplanned_files" unplanned) (list "forbidden_files_touched" forbidden) (list "audit" audit))))))))
```

- env: `{ "violations": <audit output>, "compliance_score": <audit output>, "scope": <scope.json> }`
- `verdict`, in the form's clause order: `stop_bound` when two audits are already recorded; `invalid` (with `malformed_violations` and `scores_valid`); `no_implementation` when nothing changed; `pass` when `reasons` is empty; `stop_bound` when this is the second audit and `reasons` is not empty; otherwise `correct`.
- `reasons`: `critical_violations` (`critical` lists them — the form selects them from the whole report by the exact severity `critical`), `below_floor` (`overall`, the mean of the four scores, is under 0.7), `unplanned_files`, `forbidden_files_touched`.
- The form does not judge whether a violation's severity is the right one or whether a score is deserved; those are the audit's, critiqued by the operator.

After each `CG-2` that returns `pass`, `correct`, `stop_bound` or `no_implementation`, append the record the bound is counted from:

```
jq -n -c --argjson audit <CG-2 audit> --arg verdict <CG-2 verdict> --argjson overall <CG-2 overall> '$ARGS.named' >> <run_dir>/audits.jsonl
```

## Interaction with output-shaping skills

When an output-shaping skill (e.g., adhd-mode) is active in the session:

1. The content obligations above (assumptions, risks, goals, guardrails,
   violations, scores) are always delivered. Shape rules arrange that content; they
   never delete it.
2. Lead with the verdict or the decision request; the structured report
   follows.
3. When a shape cap would drop required content, split and rank using the
   shape skill's own escape (adhd-mode rule 9's must/nice split) or group by
   anti-pattern — never truncate. The shape gate checks caps and shape
   fields, not absence: truncated content simply isn't there to count, so
   truncation is caught by your content-obligation check, never by the gate.
4. The shape skill's override conditions and the harness system prompt
   outrank these guidelines' presentation defaults.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `anti-patterns.j2` | Shared Jinja2 fragment listing the seven canonical anti-patterns, included by both templates below so the audit checks the same seven the plan forbade. Not a standalone inference template — no `[inference]` header or contract. |
| `guidelines-assess.j2` | Assess a coding task against the four principles before implementation and generate constrained directives (`assumptions`, `simplicity_risks`, `scope_creep_warnings`, `success_criteria`, `constrained_plan`, `guardrails`). |
| `guidelines-verify.j2` | Audit an implementation or diff against the four principles and the seven anti-patterns, with the changed files set against the plan. Returns `violations`, each with its correction, and `compliance_score`. |

Copyable `render_template` contexts (variable names from each template's `[inference]` contract; use `context_path` when the diff is large):

`coding-guidelines/guidelines-assess`:
```json
{ "task_description": "<the user's request, verbatim>", "code_context": {} }
```

`coding-guidelines/guidelines-verify`:
```json
{ "task_description": "<the same text>", "assumptions": [], "success_criteria": [], "constrained_plan": [], "diff_preview": "<the diff text>", "scope": { "<the whole scope.json>": "" } }
```

`anti-patterns.j2` is a fragment — do not render it directly.

## Regression case

The tests evaluate the forms and the filter authored in this file, and render the templates:

- `bash kask/scripts/check-skill-lisp-forms.sh` — `CG-1` over a passing plan and each `revise` reason alone (one and six criteria, an empty plan, a missing, empty, invented and over-long quote, a 51-line and a non-numeric estimate, a step with no file); `CG-2` over a passing audit, each failing reason alone, the mean at and just under the floor, a misspelled severity and principle, a non-numeric and an out-of-range score, no changed file, the second failing audit, and a third attempt.
- `bash kask/scripts/cargo-test-nonzero.sh -p agent --lib coding_guidelines_` — the **Change scope** filter (a change inside the plan, a stray and a forbidden file, another session's unchanged file, an untracked file, a committed change with no baseline, a malformed line, an out-of-sequence audit record), and both templates' renders (the seven anti-patterns in each, the scope lists, the no-change branch).

These are computation and handoff checks. No test establishes that an audit's scores are right or that a passing change is a good one.

## Constraints

- Both gates read the templates' outputs and `scope.json` as returned; never type a count, a mean, a selected list or a file list into an env.
- At most two audits per task: the first, and one after corrections. The count comes from the audit record, so a resumed session continues it.
- An ungated plan is not implemented, and a change with no changed file is not passed.
- Run artifacts live in the run directory, never in the skill folder.
