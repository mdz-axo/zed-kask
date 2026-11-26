---
name: diagnose
description: "Disciplined diagnosis loop for hard bugs and performance regressions. Cybernetic debugging: build feedback loop → reproduce → hypothesise → instrument → fix → regression-test. Aligned with Regulation sense→orient→decide→act."
audience: user
---


# Diagnose

Disciplined diagnosis loop for hard bugs and performance regressions. Cybernetic debugging: build feedback loop → reproduce → hypothesise → instrument → fix → regression-test. Aligned with Regulation sense→orient→decide→act.

## Reference models

Zeller, *Why Programs Fail: A Guide to Systematic Debugging* (2009) — scientific debugging (observe, hypothesize, predict, experiment, conclude); Agans, *Debugging: The 9 Indispensable Rules* (2002) — make it fail, quit thinking and look, change one thing at a time. Hypothesis elimination delegates to `falsifiability` (`onto_anchor` → derived `falsifiability`). `onto_anchor` → derived `scientific_debugging` (operator ruling 2026-09-25).

## D/P labelling

Hypothesis generation (delegated to `falsifiability`), probe placement, and
fix design are P — judgment, critiqued by the user's hypothesis review (the
ranked list is presented before instrumenting) and by the observed loop. The
feedback loop is D: the reproduction (failing test, CLI invocation, bisect)
and the regression test (written before the fix, run by `cargo test`) are
deterministic oracles — a fix is verified by the loop's output, never by the
diagnoser's confidence. The hypothesis invariant check is D (the
`lisp_eval` form over count, completeness, diversity, mutual exclusivity —
instrumentation is gated on it), and the convergence arithmetic is D (the
`lisp_eval` form normalizing the five met/unmet readings by 0.85 —
step 7). An unreproduced symptom is not yet an
admitted target for root-cause elimination.

## Initial and target condition

- **Initial condition:** a specific symptom anchored to a real code entity/call chain, its fastest observed reproduction loop (or an explicit inability to reproduce), and the current ranked hypothesis set with prior evidence.
- **Target condition:** `DIAG-2` is true, the convergence gap is at most 0.25, the original reproduction is fixed, the regression test and clippy pass, all diagnostic tags are removed, and the strengthened contract is recorded. Failed invariant/probe checks re-enter hypothesize or instrumentation with one variable changed. Eleven hypothesis cycles is the bound; exhaustion reports surviving hypotheses and missing discriminating evidence.

## When to Use

- A hard bug or performance regression resists quick fixes and needs disciplined root-cause analysis
- You need to anchor a bug to the code structure before debugging (Phase 0)
- You need to build a fast, deterministic feedback loop to reproduce the bug
- You need to generate multiple falsifiable hypotheses rather than anchoring on the first plausible idea
- You need to instrument code with targeted probes mapped to specific hypotheses
- You need to apply a fix with a regression test written *before* the fix, then clean up instrumentation
- You need to measure whether diagnosis convergence is sufficient to exit the loop
- The bug spans multiple Dublin Core entity types or PKO procedure paths and needs ontological classification
- A code structure gap may be the real finding — the affected entity has no callers or is orphaned

## When NOT to Use

- Simple, obvious bugs — if the fix is a one-liner you already understand, the loop's overhead exceeds its value.
- Exploratory hunting for unknown bugs — use `bug-hunt`; diagnose starts from a specific symptom.
- Spec-conformance review of a change — use `code-review`.

## Instructions

1. **Anchor to the code structure (Phase 0 + step 1).** Step 0 uses `grep` to pre-compute the blast radius — search for all call sites of the affected symbol to understand impact (deterministic; on failure, report surfaces the gap without blocking the diagnosis). Step 1 (`diagnose-spec-anchor.j2`) then anchors the bug to the actual code structure using a dual ontology: use `grep` to find the affected symbol and `read_file` to trace the call chain (callers + callees). Classify the THING using Dublin Core (`dcterms:type` = entity kind, `dcterms:identifier` = qualified name, `dcterms:source` = file:line, `dcterms:subject` = MDS category derived from code position). Classify the FLOW using PKO (`pko:Procedure` = the call chain, its steps related by `pko:hasStep` — each function is a step of the Procedure, `pko:IssueOccurrence` = the bug at a specific `pko:StepExecution`). Every diagnosis must be anchored to a real code entity or note that the structure is unavailable.

2. **Build the feedback loop and reproduce the bug.** Construct the fastest, most deterministic feedback loop. Try strategies in order: failing test at the reaching seam → `cargo test` with specific test name → CLI invocation with fixture input → HTTP script → replay captured input → throwaway harness → property/fuzz loop → `git bisect run` → differential loop. Prioritize speed, signal sharpness, and determinism — a 2-second deterministic loop beats a 30-second flaky loop. For non-deterministic bugs, the goal is a higher reproduction rate, not a clean repro: loop 100×, parallelise, add stress. Do not proceed to hypothesising without a loop you believe in. Record the observed failing input and result; a report of an unreproduced symptom is not yet an admitted target for root-cause elimination. Pass `code_context` to the loop template: the anchoring's call-chain evidence plus the reproduction evidence, assembled by the agent. Gate entry into Step 3 via `lisp_eval` (`DIAG-1`):
   - form: `(and (eq can_reproduce true) (eq repro_exit_nonzero true) (member repro_confidence (list "high" "medium" "low")))`
   - env: `{"can_reproduce": <true|false>, "repro_exit_nonzero": <true|false>, "repro_confidence": <"high"|"medium"|"low"|"cannot_reproduce">}`
   - receipts (beside the form; `check-skill-lisp-forms` evaluates each line's env over the form above and compares the expected result exactly):
     ```lisp-receipts
     {"gate":"DIAG-1","case":"pass","env":{"can_reproduce":true,"repro_exit_nonzero":true,"repro_confidence":"high"},"expected":true}
     {"gate":"DIAG-1","case":"cannot-reproduce","env":{"can_reproduce":true,"repro_exit_nonzero":true,"repro_confidence":"cannot_reproduce"},"expected":false}
     {"gate":"DIAG-1","case":"wrong-case-confidence","env":{"can_reproduce":true,"repro_exit_nonzero":true,"repro_confidence":"Cannot_Reproduce"},"expected":false}
     {"gate":"DIAG-1","case":"string-false-boolean","env":{"can_reproduce":"false","repro_exit_nonzero":true,"repro_confidence":"high"},"expected":false}
     {"gate":"DIAG-1","case":"null-confidence","env":{"can_reproduce":true,"repro_exit_nonzero":true,"repro_confidence":null},"expected":false}
     ```

3. **Generate falsifiable root-cause hypotheses (delegate to falsifiability).** Avoid single-hypothesis anchoring by delegating this step to the `falsifiability` skill's hypothesize stage — the shared Chamberlin/Platt method diagnose would otherwise reimplement. Invoke `falsifiability/falsifiability-hypothesize` with `admitted_target` = the reproduced symptom and its observed failing case from step 2, `domain` = "bug diagnosis", `context` = code_context including the reproduction evidence. This is diagnosis's existing admission gate; do not add a second Popper prompt for a reproduced bug. falsifiability-hypothesize generates 3–7 ranked candidate root causes with forced diversity (≥1 unlikely, ≥1 challenging the obvious explanation, ≥1 embarrassing-if-true), each carrying a Platt-form prediction ("if X is the case, then observation Y under condition Z") and a falsifier; it discards any candidate that cannot be made falsifiable (a vibe) at generation, recording why. Map each returned hypothesis's `prediction` into the bug-debugging form ("if X is the cause, then changing Y will make the bug disappear") and treat its `falsifier` as the falsification condition that step 5's probes must be able to trigger. Rank by likelihood, not by ease of testing. Present the ranked list for user review before testing any hypothesis — the user often has domain knowledge that re-ranks instantly. Do not proceed to instrumenting until the user has reviewed.

4. **Hypothesis invariant check (step 4, call `lisp_eval` — no template, deterministic).** Before instrumenting, call `lisp_eval` to deterministically evaluate four structural invariants on the root-cause hypothesis set: count (3–7), completeness (every hypothesis has `prediction` + `falsifier` keys), diversity (≥2 distinct likelihoods), and mutual exclusivity (no duplicate hypothesis text). Returns a list of defect strings — no LLM round-trip. Step 5 (instrument) is gated on this check: if the check returns defects, re-enter the cycle at step 3 (hypothesize) to repair them before any instrumentation runs.

   Pinned form (live-validated 2026-09-09):

   ```lisp
   (if (not (listp hypotheses))
     (list "invalid_hypotheses")
     (let ((hyps hypotheses))
       (begin
         (define nonblank (lambda (v) (and (stringp v) (> (length v) 0))))
         (define walk-check (lambda (hs defects)
           (if (is_null hs)
             defects
             (let ((h (car hs)))
               (if (not (listp h))
                 (walk-check (cdr hs) (append defects (list "invalid_hypothesis_entry")))
                 (walk-check (cdr hs)
                   (append defects
                     (if (not (nonblank (assoc "hypothesis" h))) (list "missing_hypothesis") nil)
                     (if (not (nonblank (assoc "prediction" h))) (list "missing_prediction") nil)
                     (if (not (nonblank (assoc "falsifier" h))) (list "missing_falsifier") nil)
                     (if (not (nonblank (assoc "likelihood" h))) (list "missing_likelihood") nil))))))))
         (define get-texts (lambda (hs acc)
           (if (is_null hs) acc (get-texts (cdr hs) (append acc (list (if (listp (car hs)) (assoc "hypothesis" (car hs)) nil)))))))
         (define get-likes (lambda (hs acc)
           (if (is_null hs) acc (get-likes (cdr hs) (append acc (list (if (listp (car hs)) (assoc "likelihood" (car hs)) nil)))))))
         (define has-dupes (lambda (ts)
           (if (is_null ts) nil
             (if (and (nonblank (car ts)) (member (car ts) (cdr ts))) (list "duplicate_hypothesis_text") (has-dupes (cdr ts))))))
         (define diff-exists (lambda (ls first)
           (if (is_null ls) nil
             (if (and (stringp (car ls)) (stringp first) (not (string= (car ls) first))) (list "diverse") (diff-exists (cdr ls) first)))))
         (define texts (get-texts hyps nil))
         (define likes (get-likes hyps nil))
         (define defects
           (append
             (if (or (< (length hyps) 3) (> (length hyps) 7)) (list "count_out_of_range_3_to_7") nil)
             (walk-check hyps nil)
             (has-dupes texts)
             (if (is_null likes) nil
               (if (is_null (diff-exists (cdr likes) (car likes))) (list "no_likelihood_diversity") nil))))
         (if (> (length defects) 0) defects 'ok))))
   ```

   env: `{ "hypotheses": <the ranked candidate root causes from step 3> }`.

   ```lisp-receipts
   {"gate":"step-4-hypotheses","case":"ok-3-diverse","env":{"hypotheses":[{"hypothesis":"h1","prediction":"p1","falsifier":"f1","likelihood":"high"},{"hypothesis":"h2","prediction":"p2","falsifier":"f2","likelihood":"low"},{"hypothesis":"h3","prediction":"p3","falsifier":"f3","likelihood":"medium"}]},"expected":"ok"}
   {"gate":"step-4-hypotheses","case":"malformed-and-dupes","env":{"hypotheses":["bad",{"hypothesis":"h2","prediction":"","falsifier":"f2","likelihood":null},{"hypothesis":"h2","prediction":"p3","falsifier":"f3","likelihood":"low"}]},"expected":["invalid_hypothesis_entry","missing_prediction","missing_likelihood","duplicate_hypothesis_text","no_likelihood_diversity"]}
   {"gate":"step-4-hypotheses","case":"non-list-hypotheses","env":{"hypotheses":"bad"},"expected":["invalid_hypotheses"]}
   ```

5. **Instrument with targeted probes mapped to hypotheses.** Design probes where each probe maps to exactly one hypothesis — no scattergun logging. Pass `selected_hypothesis` = the hypothesis the user's review (step 3) selected for first testing. Change one variable at a time; never test multiple hypotheses simultaneously. Tool preference order: `rust-lldb`/`rust-gdb` breakpoint (one breakpoint beats ten logs) → targeted `tracing::debug!` with unique `[DIAG-xxxx]` prefix → `RUST_LOG` per-module tracing. Never "log everything and grep." Tag every diagnostic log with a unique `[DIAG-xxxx]` prefix so cleanup is a single grep. For performance bugs, use `cargo bench`, `criterion`, or `flamegraph` — measure first, fix second.

6. **Apply fix with regression test written before the fix.** If a correct seam exists: turn the minimised reproduction into a failing regression test at that seam, watch it fail, apply the fix, watch it pass, re-run the original feedback loop. If no correct seam exists, that itself is the finding — the architecture is preventing the bug from being locked down. Document this for architecture review. Do NOT write a shallow regression test that gives false confidence. Clean up: remove all `[DIAG-...]` instrumentation, delete throwaway prototypes, state the confirmed hypothesis in the commit/PR message. Verify `./script/clippy -p <crate>` and `cargo test -p <crate>` pass, and gate fix validation (`f = 0` in Step 7) via `lisp_eval` (`DIAG-2`):
   - form: `(and (eq original_repro_fixed true) (numberp diag_tags_remaining) (numberp clippy_exit_code) (numberp test_exit_code) (= diag_tags_remaining 0) (= clippy_exit_code 0) (= test_exit_code 0))`
   - env: `{"original_repro_fixed": <true|false>, "diag_tags_remaining": <int>, "clippy_exit_code": <int>, "test_exit_code": <int>}`
   - receipts (beside the form; `check-skill-lisp-forms` evaluates each line's env over the form above and compares the expected result exactly):
     ```lisp-receipts
     {"gate":"DIAG-2","case":"pass","env":{"original_repro_fixed":true,"diag_tags_remaining":0,"clippy_exit_code":0,"test_exit_code":0},"expected":true}
     {"gate":"DIAG-2","case":"string-false-repro","env":{"original_repro_fixed":"false","diag_tags_remaining":0,"clippy_exit_code":0,"test_exit_code":0},"expected":false}
     {"gate":"DIAG-2","case":"diag-tags-remaining","env":{"original_repro_fixed":true,"diag_tags_remaining":1,"clippy_exit_code":0,"test_exit_code":0},"expected":false}
     {"gate":"DIAG-2","case":"clippy-failed","env":{"original_repro_fixed":true,"diag_tags_remaining":0,"clippy_exit_code":1,"test_exit_code":0},"expected":false}
     ```
   Every input is a value the CALLER measures, never one `diagnose-fix.j2` types: `original_repro_fixed` from re-running the feedback loop, `diag_tags_remaining` from a `grep -c "\[DIAG-"` over the touched files, `clippy_exit_code` from `./script/clippy`, `test_exit_code` from `cargo test`. Write a post-mortem: what was the bug, root cause, fix, and what would have prevented it. If the fix reveals an architectural issue (no good test seam, tangled callers, hidden coupling), document it in an architecture note.

7. **Check convergence (step 7).** Measure whether root cause and fix confidence are sufficient to exit the diagnosis loop. Each unmet check adds its weight: root cause ambiguous (0.25), bug not reproduced (0.15), fix not validated (0.20), alternatives not eliminated (0.15), contract not strengthened (0.10). The weights sum to 0.85, so normalize by it: all met = 0.00, none met = 1.00. The five met/unmet readings are P (judged from the loop's evidence); the arithmetic is D — compute with `lisp_eval`:
   - form: `(if (and (member a (list 0 1)) (member r (list 0 1)) (member f (list 0 1)) (member e (list 0 1)) (member c (list 0 1))) (/ (+ (* 0.25 a) (* 0.15 r) (* 0.20 f) (* 0.15 e) (* 0.10 c)) 0.85) "invalid_readings")`
   - env: `{ "a": <1 if root cause ambiguous else 0>, "r": <1 if not reproduced else 0>, "f": <1 if fix unvalidated else 0>, "e": <1 if alternatives not eliminated else 0>, "c": <1 if contract not strengthened else 0> }`
   - receipts (beside the form; `check-skill-lisp-forms` evaluates each line's env over the form above and compares the expected result exactly):
     ```lisp-receipts
     {"gate":"step-7-convergence","case":"all-unmet","env":{"a":1,"r":1,"f":1,"e":1,"c":1},"expected":1.0000000000000002}
     {"gate":"step-7-convergence","case":"root-cause-met","env":{"a":0,"r":1,"f":1,"e":1,"c":1},"expected":0.7058823529411765}
     {"gate":"step-7-convergence","case":"all-met","env":{"a":0,"r":0,"f":0,"e":0,"c":0},"expected":0.0}
     {"gate":"step-7-convergence","case":"out-of-domain-reading","env":{"a":2,"r":1,"f":1,"e":1,"c":1},"expected":"invalid_readings"}
     ```
   Bound: at most 11 hypothesis cycles (operator ruling 2026-09-25); on the 11th without convergence, stop and report the gap score, the surviving hypotheses and the missing discriminating evidence. Convergence threshold is 0.25 — diagnosis can't improve past evidence, so a looser threshold is appropriate. 0.00 = root cause confirmed, fix validated, regression tests pass. 0.50 = competing hypotheses, insufficient evidence to discriminate. 1.00 = no root cause identified, no fix proposed. If blockers remain, state the specific gap preventing convergence.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `diagnose-spec-anchor.j2` | Anchor a bug diagnosis to the actual code structure using a dual ontology: Dublin Core classifies the THING affected (entity type, identifier, source, subject domain derived from code position) and PKO classifies the FLOW the bug occurs in (call chain as Procedure, each function as Step, bug as IssueOccurrence at a StepExecution). Uses `grep` and `read_file` to locate symbols and trace call chains. Phase 0 of the diagnosis pipeline. Replaces spec-anchoring — the bug is grounded in the real code, not a specification document. |
| `diagnose-loop.j2` | Build a feedback loop for the bug. Evaluate repro strategies, select the fastest deterministic signal, and confirm the bug reproduces before hypothesising. |
| `diagnose-instrument.j2` | Instrument the code with targeted probes mapped to specific hypotheses. Change one variable at a time. Use tagged diagnostic logs or breakpoints. |
| `diagnose-fix.j2` | Apply fix with regression test (before the fix). Verify original repro no longer reproduces. Clean up instrumentation. Write post-mortem. |

To render a template, call the `render_template` tool with the template ref (e.g., `diagnose/diagnose-spec-anchor`) and a context object with the required variables. Every render carries the same `bug_description` (the user's report, verbatim) and, where asked, the same `crate_scope` (the crate under diagnosis):

- `diagnose/diagnose-spec-anchor`: `{"bug_description": "Parser panics on empty input", "symptom": "panic at index out of bounds", "target_file": "src/parser.rs", "symbol_name": "parse_tokens", "blast_radius": ["src/lib.rs:42"]}`
- `diagnose/diagnose-loop`: `{"bug_description": "Parser panics on empty input", "crate_scope": "parser", "symptom": "panic at index out of bounds", "code_context": "parse_tokens called from lib.rs:42"}`
- `diagnose/diagnose-instrument`: `{"bug_description": "Parser panics on empty input", "hypotheses": [{"hypothesis": "Empty slice indexed at 0", "prediction": "guarding len==0 stops panic", "falsifier": "panic persists with len==0 guard", "likelihood": "high"}], "selected_hypothesis": "Empty slice indexed at 0", "crate_scope": "parser"}`
- `diagnose/diagnose-fix`: `{"bug_description": "Parser panics on empty input", "hypothesis": "Empty slice indexed at 0", "fix_description": "Return Ok(Vec::new()) when input is empty", "crate_scope": "parser", "has_correct_seam": true}`

## Regression case

All receipts executed live through `lisp_eval` (2026-10-01, backfill pass),
over the convergence normalization form:

- All five met/unmet readings 1 → `1.0` (the 0.85-normalized ceiling).
- Root cause met (`a=0`), rest unmet (`r=f=e=c=1`) → `0.7058823529411765` — the gap is
  measured, not judged.

The skill's forms are executed at use time, never anchored in code.

## Constraints

- Step 3 (hypothesize) is delegated to falsifiability/falsifiability-hypothesize
- Hypothesis count: 3–7 (step 3 delegates to `falsifiability/falsifiability-hypothesize`) — every hypothesis must have a falsifiable prediction
- Every probe must map to exactly one hypothesis; every diagnostic log must have a unique `[DIAG-xxxx]` tag
- Write the regression test BEFORE the fix; if no correct seam exists, do not write a shallow test that gives false confidence
- All `[DIAG-xxxx]` instrumentation tags must be removed before declaring done
- The commit/PR message must state the confirmed hypothesis
- Do not fabricate code entities — derive from actual `grep` and `read_file` results or note the structure is unavailable
