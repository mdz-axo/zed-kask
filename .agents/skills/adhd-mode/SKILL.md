---
name: adhd-mode
description: "Shape output for a reader with ADHD: lead with the next action, number multi-step work, restate state across turns, suppress tangents, cap lists at five, no preamble or closers. Session-scoped output mode with a deterministic pre-send gate (render_template + lisp_eval) and an optional caveman compression variant for connective prose. Activate with 'adhd mode on'; add compression with 'compressed'; deactivate with 'adhd mode off'."
---

# ADHD Mode

The reader has ADHD. Output is not just brief. It is shaped so an ADHD brain
can act on it. This is a mode, not a pipeline: while on, every response you
compose passes the per-turn gate below before sending.

## When to Use

- The reader has said "adhd mode on" (or asked for ADHD-shaped output) and the mode is active for the session
- Composing any response — answer, report, status, error explanation — while the mode is on
- A process skill (e.g., coding-guidelines) is active and its report must be delivered ADHD-shaped: content obligations ship, shape rules arrange them

## When NOT to Use

- The mode is off (default). Do not activate it unprompted
- As a substitute for a process skill — this skill shapes output; it does not govern the work
- To delete content an active process skill requires — shape arranges content, never deletes it (override 5 below)

## Instructions

### Mode convention

The mode is session-scoped. It turns on when the reader says "adhd mode on"
and off when they say "adhd mode off" or "stop adhd mode". Confirm either
transition in one line, then apply or release the rules. If unsure whether the
mode still applies, it does. The compression variant is a modifier: "adhd mode
on, compressed" activates it; "compression off" releases it while the mode
stays on.

### Compression variant (absorbed from caveman)

The mode has two levels. "adhd mode on" applies the standard shape.
"adhd mode on, compressed" (or an explicit request for caveman compression)
additionally compresses the connective prose between structural elements:
drop articles, filler, pleasantries, and empty hedging; use fragments and
short synonyms; abbreviate common terms (DB, auth, config, req, res, fn,
impl). The structural elements — the leading action, numbered steps,
restated state, time-estimate conditionals, the visible win, the one next
action — are clarity-critical and survive compression unchanged.
Real-uncertainty hedges ("if tests already cover this") are structural and
are never dropped.

Sacred text passes through unchanged: code blocks, error messages quoted
exact, and URLs.

Auto-clarity exceptions suspend compression (not the structural shape):
security warnings, irreversible-action confirmations, and any multi-step
sequence where fragment order risks misread. Resume compression after the
clarity-requiring part. When a process skill is active, its content
obligations ship in full — compression arranges prose, never deletes
content.

### Per-turn gate

Run this gate on every response while the mode is on. Apply the Constraints
(including the six overrides) first — the gate checks the override-adjusted
draft.

1. Compose the draft under Constraints. If a process skill is active, its
   content obligations must all ship. When the turn has a known type (assess,
   directives, verify, code-answer) and you are not yet fluent in its shape,
   call `render_template` with template `adhd-mode/turn-shape` and context
   `{ "turn_type": "<type>", "content_obligations": [<obligations from the
   active process skill, or []>] }`, and compose following the rendered shape
   spec. On repeat turns of the same type, compose from the internalized spec
   without re-rendering.

2. Call `render_template` with template `adhd-mode/pre-send-gate` and context
   `{ "turn_type": "<type>", "content_obligations": [<obligations or []>] }`.
   The rendered checklist defines the gate fields to extract from the draft.

3. Extract the gate fields from the draft following the rendered checklist.
   Extract evidence, not verdicts: `opener` is the actual first words of the
   draft, `closer` the actual last words; counts are counted from the draft.

4. Call `lisp_eval` with Form G below and an env binding every extracted field
   plus `cap` bound to 5 (the list cap from Constraints — the single source).

5. If `content_obligations` is non-empty, call `lisp_eval` with Form C below,
   env binding `required` to the content obligations and `sections` to the
   section headers present in the draft.

6. Verdicts `send` and `content-complete`: send the draft. Any `revise` or
   `missing-content`: apply the named fixes (delete the flagged opener, closer,
   sidebar, hedge, or idiom; split the flagged over-cap lists; restore missing
   sections; reshape the first or last line), re-extract the changed fields,
   and re-run the checks. Bound: 2 revision passes. After the second pass,
   send the best available draft with a one-line note of what could not be
   satisfied.

If `render_template` or `lisp_eval` fails, call
`curator_report_skill_use_issue` with skill_name "adhd-mode" and the failing
tool, then fall back to a prose self-check against Constraints — the gate is
the enforcement layer; the prose rules are the floor, not a replacement.

### Form G — gate

```
(let ((issues (append (if (member opener (list "Let me" "I'll" "Great question" "Sure!" "Looking at" "To answer")) (list (concat "delete-opener:" opener)) nil) (if (member closer (list "Hope that helps" "Let me know" "Happy to clarify" "Feel free to ask")) (list (concat "delete-closer:" closer)) nil) (if (> sidebar_count 0) (list "delete-sidebar") nil) (if (> empty_hedge_count 0) (list "delete-empty-hedge") nil) (if (> idiom_count 0) (list "replace-idiom") nil) (if (> assumptions_count cap) (list "split:assumptions") nil) (if (> risks_count cap) (list "split:risks") nil) (if (> goals_count cap) (list "split:goals") nil) (if (> critical_count cap) (list "split:critical") nil) (if (> minor_groups cap) (list "split:minor_groups") nil) (if (not (= next_actions_count 1)) (list "exactly-one-next-action") nil) (if (not first_line_is_action) (list "reshape:first-line-is-action") nil) (if (not last_line_states_next) (list "reshape:last-line-states-next") nil) (if (not last_line_states_done) (list "reshape:last-line-states-done") nil)))) (if (> (length issues) 0) (list 'revise issues) 'send))
```

env: `opener`, `closer` (strings, verbatim evidence); `sidebar_count`,
`empty_hedge_count`, `idiom_count`, `assumptions_count`, `risks_count`,
`goals_count`, `critical_count`, `minor_groups`, `next_actions_count`
(numbers); `first_line_is_action`, `last_line_states_next`,
`last_line_states_done` (booleans); `cap` (number, from Constraints).

### Form C — content completeness

```
(let ((req required) (secs sections))
  (begin
    (define check-all (lambda (r missing)
      (if (= (length r) 0)
        missing
        (if (member (car r) secs)
          (check-all (cdr r) missing)
          (check-all (cdr r) (append missing (list (car r))))))))
    (let ((missing (check-all req nil)))
      (if (= (length missing) 0) 'content-complete (list 'missing-content missing)))))
```

env: `required` (the active process skill's content obligations), `sections`
(section headers present in the draft).

## Registry Templates

| Template | Purpose |
|----------|---------|
| `pre-send-gate.j2` | Extraction checklist for the per-turn gate: gate fields extracted as evidence (not verdicts) from the composed draft, feeding lisp_eval Forms G and C. |
| `turn-shape.j2` | Per-turn-type shape spec (assess, directives, verify, code-answer), generic over the active process skill's content obligations. |
| `caveman-compress.j2` | Compression pass for the compressed variant: drop articles, filler, pleasantries, and hedging from connective prose while preserving technical substance, sacred text (code, errors, URLs), and clarity exceptions. Absorbed from the caveman skill 2026-09-09. |

To render a template, call the `render_template` tool with the template ref (e.g., `adhd-mode/pre-send-gate`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `caveman-compress.j2`: `draft_response`,`context_topic`


## Constraints

The list cap is 5 — pass it to Form G as `cap`. This is the single source;
Form G carries no threshold of its own.

The ten rules (faithful to the source; references renamed):

1. **Lead with the next action.** The first line is something the reader can
   do. Not context. Not a plan. The action. If the answer is a command, path,
   or snippet, it goes first; prose comes after, if at all.
2. **Number multi-step tasks.** If the work takes more than one step, write a
   numbered list. Each step is one bounded action. Use the fewest steps that
   still work; cut any step the reader does not need, and fold trivial steps
   into the one before.
3. **End with one concrete next action.** If anything is left open, name ONE
   thing the reader can do in under two minutes. Even "open the file" counts.
   A completion line ("X now works. Try: ...") counts as the one next action.
4. **Suppress tangents.** If a second issue exists, finish the first, then
   offer the second as a separate question. A question that comes up mid-work
   is not a tangent: answer it yourself if you can and fold the result in.
5. **Restate state every turn.** The reader cannot hold "we are on step 3 of
   5" between messages. Restate it. If the harness has a task or plan tool,
   use it: one item per step, one in progress; the checklist does the
   restating — do not also narrate the full plan as prose.
6. **Give specific time estimates.** Vague estimates fail. "About 15 minutes
   if tests already cover this. An afternoon if not." Point estimates at
   whoever executes the steps.
7. **Make completed work visible.** Show what now works, in concrete terms.
   Do not bury wins in a recap.
8. **Matter-of-fact tone for errors.** Never "Uh oh" or "There seems to be a
   problem." State cause and fix.
9. **Cap lists at 5 items.** If a list grows past five, split into "do now"
   vs "later," or "must" vs "nice to have." Five ranked beats ten unranked.
   Enforced by Form G via `cap`.
10. **No preamble, no recap, no closing pleasantries.** Forbidden openers:
    "Great question," "Let me...", "I'll...", "Sure!", "Looking at your...",
    "To answer your question...". Forbidden recaps after a completed task.
    Forbidden closers: "Let me know if you need anything else," "Hope that
    helps," "Happy to clarify," "Feel free to ask." Start with the answer;
    end when the answer is done. The lexical checks are enforced by Form G;
    the full phrase lists live there and in the pre-send-gate template.

Override the defaults when:

1. User asks to "explain" or "walk me through." Explain fully. Still no
   preamble, still no closer, but the body runs as long as the topic needs.
   Add headers so the reader can skim back.
2. Destructive action ahead (`rm -rf`, force push, schema migration, dropping
   a table). Confirm before acting. Safety wins over brevity.
3. Debug spiral. If the last three turns have been "still broken," stop
   iterating on code. Name the assumption that might be wrong. Ask one
   diagnostic question.
4. Real ambiguity in the request. One short clarifying question beats
   guessing and rewriting.
5. A rule fights the task. When a rule would delete the answer itself, the
   task wins; the shape stays. Example: "what are my options" gets 2 to 4
   ranked options with one-line trade-offs, recommendation first. The
   options are the answer.
6. A rule fights the harness. Inside an agent harness, the system prompt
   outranks this skill: announce a tool call when the harness requires it, do
   the work instead of asking "want me to," point time estimates at whoever
   executes the steps. Same principle as 5: the constraint wins, the shape
   stays.

The upstream pre-send check is reified as the per-turn gate: its five
deletions are Form G's lexical and count checks; its two-question verify
("if the reader reads only the first line and the last line, do they know
(a) what to do next, and (b) what just happened?") is embedded as Form G's
`first_line_is_action`, `last_line_states_next`, and `last_line_states_done`
fields.

## Provenance

Translated from `ayghri/i-have-adhd`
(https://github.com/ayghri/i-have-adhd/blob/main/skills/i-have-adhd/SKILL.md,
MIT license), fetched 2026-09-09, translated 2026-09-09 via
skill-maintenance-translate. Renamed i-have-adhd → adhd-mode (operator
decision). Rules and overrides are faithful; the pre-send check is reified
as the deterministic per-turn gate. Source concepts with no kask equivalent,
resolved by operator decision: the `/i-have-adhd` slash-command invocation
and `disable-model-invocation: true` → the chat convention above; session
persistence → the mode convention above. Upstream drift requires
re-translation against this provenance header, not a byte-diff.
