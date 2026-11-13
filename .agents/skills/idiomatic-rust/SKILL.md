---
name: idiomatic-rust
description: "Type-driven Rust design through eight design principles, grounded by rust-analyzer and clippy as extrinsic oracles. Propose type-driven solutions informed by LSP code actions and refine through adversarial review verified against compiler feedback."
audience: user
---

# Idiomatic Rust

Type-driven Rust design, grounded by rust-analyzer and clippy as extrinsic
oracles. The eight principles below are a **design lens** — a synthesis of
widely used Rust design mantras, not a citable spec. The citable anchors are
the Rust API Guidelines (rust-lang.github.io/api-guidelines) and *The Rust
Programming Language* (Klabnik & Nichols). Provenance notes:

- "Make invalid states unrepresentable" is widely attributed to **Yaron
  Minsky** (OCaml), not Graydon Hoare; it is a general type-system mantra.
- The Rust API Guidelines are the canonical, versioned reference for API
  design and are enforced as a checklist gate in the design phase (see
  *API Guidelines checklist gate*).

The LLM reasons about design through the eight principles; the compiler and
clippy verify whether the reasoning is correct.

## The compiler as extrinsic oracle

Without compiler feedback, the skill operates on intrinsic reasoning alone —
the LLM assesses and critiques its own proposals. This is the Dunning-Kruger
trap: the LLM may confidently propose a design the borrow checker would reject.
The compiler is the extrinsic oracle that closes the gap:

| Phase | Tool | What it grounds |
|-------|------|-----------------|
| Inquiry | `./script/clippy -p <crate> --message-format=json` on the existing code (the baseline) | rustc errors and warnings and clippy lints, each with its code, file and line — the starting point for the assessment |
| Inquiry | `go_to_definition` | The ownership graph as the code has it |
| Design | `get_code_actions` | Compiler-verified refactor suggestions as design starting points |
| Challenge | the same clippy command on the applied proposal | Whether the proposal compiles, which lints it introduces and which it resolves |
| Challenge | `find_references` | The actual blast radius |
| Challenge | `cargo +nightly miri test` where the proposal contains `unsafe` | Undefined behaviour |

One capture serves both oracles: clippy runs rustc, so its JSON output carries
rustc's errors and warnings as well as the lints.

Each compiler diagnostic is interpreted through the type-driven lens:
- Borrow checker error → Principle 2 (ownership is architecture)
- Type mismatch → Principle 1 (invalid states unrepresentable)
- Lifetime error → Principle 2 (ownership) or Principle 5 (explicit)
- Missing trait impl → Principle 6 (composition over inheritance)
- Panic path → Principle 7 (errors as values)
- Unsafe block → Principle 8 (unsafe as contract)

## D/P labelling

Design proposals and the adversarial review's findings are P — judgment,
critiqued by the compiler and clippy. Everything the gates read is D: the
**Compiler facts** filter (`jq`) turns the captured tool output into statuses,
code lists and counts, and `IR-1` and `IR-2` (`lisp_eval`) decide from those
and from the templates' outputs as returned. No count, status or flag is
typed. A finding is compiler-confirmed only when every diagnostic code it
cites is in the measured list; a proposal the compiler has not seen is
`undetermined`, never converged.

## Initial and target condition

- **Initial condition:** the Rust design problem and constraints, the existing code, and a run directory holding the baseline capture of that code.
- **Target condition:** `IR-2` returns `converged`: the applied proposal compiles, introduces no lint the baseline did not have, at least one changed type had its references examined, no API-guideline violation is left `open`, and Miri reported no undefined behaviour. `continue` re-enters inquiry with the challenge directives; `oscillating` and `stop_bound` go to the operator; `undetermined` is reported as unverified. Three iterations is the bound.

## When to Use

- Assessing a Rust design problem against the eight design principles to identify invariants, invalid states, ownership graphs, and error domains.
- Proposing type-driven Rust solutions with code examples, applying algebraic types, ownership patterns, error propagation, and trait design.
- Conducting adversarial reviews of a Rust design proposal to find gaps, test edge cases, challenge assumptions, and identify deeper ecosystem connections.
- Deciding whether an idiomatic-rust design has converged, from compiler diagnostics and clippy counts rather than a self-scored critique.

## When NOT to Use

- Non-Rust languages — this skill's oracles are rust-analyzer and clippy.
- Machine-checked proof — use `lean-prover`.
- Reviewing a change against its spec — use `code-review`; this skill designs, it does not adjudicate diffs.

## PDCA Loop

```
Plan:  Inquiry   → capture the baseline, assess against the eight principles, gate with IR-1
Do:    Design    → propose type-driven solutions informed by get_code_actions
Check: Challenge → apply the proposal, capture it, review it adversarially, gate with IR-2
Act:   Loop      → on `continue`, re-enter inquiry with the challenge's directives
```

The loop targets inquiry, not design, so challenge findings re-inform the
assessment, and the inquiry reads the diagnostics of the code as it now stands.
At most three iterations: `IR-2` counts them from the experiment record and
returns `stop_bound` on a third iteration that still has an open reason, and on
any attempt at a fourth.

## Run directory and compiler facts

Each design problem has one run directory,
`~/Documents/zk-data/skills/idiomatic-rust/{date}-{run}/` (`{run}` a readable
slug naming the problem). Create it with `terminal` — the built-in file tools
cannot write there — with its five capture files empty:

```
mkdir -p <run_dir> && touch <run_dir>/baseline.jsonl <run_dir>/proposed.jsonl <run_dir>/unsafe.txt <run_dir>/miri.txt <run_dir>/experiments.jsonl
```

| File | Holds | Written |
|------|-------|---------|
| `baseline.jsonl` | the existing code's diagnostics | once, before any edit |
| `proposed.jsonl` | the proposal's diagnostics | after each applied revision |
| `unsafe.txt` | the proposal's `unsafe` lines | after each applied revision |
| `miri.txt` | Miri's output | only when `unsafe.txt` is not empty |
| `experiments.jsonl` | the loop's history | one line after each `IR-2` (see *Experiment record*) |

```
./script/clippy -p <crate> --message-format=json > <run_dir>/baseline.jsonl 2> <run_dir>/baseline.err
./script/clippy -p <crate> --message-format=json > <run_dir>/proposed.jsonl 2> <run_dir>/proposed.err
grep -n -w unsafe <each file the proposal changed> > <run_dir>/unsafe.txt
cargo +nightly miri test -p <crate> > <run_dir>/miri.txt 2>&1 && echo miri-exit=0 >> <run_dir>/miri.txt || echo miri-exit=nonzero >> <run_dir>/miri.txt
```

The clippy command exits nonzero whenever it reports anything (it denies
warnings), and `grep` exits 1 when it finds nothing; neither is a failure of
the capture. An empty `baseline.jsonl`, `proposed.jsonl` or `miri.txt` means that
tool did not run; an empty `unsafe.txt` means the grep found no `unsafe` line
or was not run, so run it after every applied revision.

### Compiler facts

Save the filter below verbatim as `<run_dir>/compiler-facts.jq` when the run
directory is created, and run it after every capture:

```
jq -n --rawfile baseline <run_dir>/baseline.jsonl --rawfile proposed <run_dir>/proposed.jsonl --rawfile unsafe_grep <run_dir>/unsafe.txt --rawfile miri <run_dir>/miri.txt --rawfile record <run_dir>/experiments.jsonl '$ARGS.named' | jq -c -f <run_dir>/compiler-facts.jq > <run_dir>/facts.json
```

```jq
def records: split("\n") | map(fromjson? | select(type == "object"));
def diagnostics: map(select(.reason == "compiler-message") | .message
    | select((.level == "error" or .level == "warning") and (.spans | length) > 0)
    | {code:(.code.code // (if .level == "error" then "rustc-error" else "rustc-warning" end)),
       level:.level, message:.message,
       file:([.spans[] | select(.is_primary) | .file_name][0]),
       line:([.spans[] | select(.is_primary) | .line_start][0])}) | unique;
def status: if (any(.[]; .reason == "build-finished") | not) then "not_run"
  elif any(.[]; .reason == "build-finished" and .success == false) and (diagnostics | length) == 0
  then "unexplained_failure" else "ran" end;
def hard: .code == "rustc-error" or (.code | test("^E[0-9]{4}$"));
def key: [.code, .file, .message] | tojson;
def tally: reduce .[] as $d ({}; .[$d | key] += 1);
def beyond($seen): group_by(key)
  | map((length - ($seen[.[0] | key] // 0)) as $n | select($n > 0) | .[0] + {count:$n});
def miri_status: if test("^\\s*$") then "not_run" elif test("Undefined Behavior") then "ub"
  elif test("miri-exit=0\\s*$") then "passed" else "failed" end;
(.baseline | records) as $b | (.proposed | records) as $p | (.record | records) as $r
| if all($r | to_entries[]; .value.iteration == .key + 1 and (.value.confirmed | type) == "number")
  then . else error("experiment record out of sequence or without a confirmed count") end
| ($b | status) as $bs | ($p | status) as $ps
| ($b | diagnostics) as $bd | ($p | diagnostics) as $pd
| (if $ps == "ran" then $pd else $bd end) as $current
| [$bd[] | select(hard | not)] as $bl | [$pd[] | select(hard | not)] as $pl
| {baseline_status:$bs, proposed_status:$ps,
   open_codes:($current | map(.code) | unique),
   diagnostics:$current,
   compile_errors:(if $ps == "ran" then [$pd[] | select(hard)] else [] end),
   new_lints:(if $ps == "ran" then $pl | beyond($bl | tally) else [] end),
   resolved_lints:(if $ps == "ran" then $bl | beyond($pl | tally) else [] end),
   unsafe_lines:(.unsafe_grep | split("\n")
     | map(select(length > 0 and (test("^([^:]+:)?[0-9]+:\\s*//") | not)))),
   miri:(.miri | miri_status)}
| . + {gate:{baseline_status, proposed_status, open_codes, miri,
   compile_errors:(.compile_errors | length), new_lints:(.new_lints | length),
   unsafe_lines:(.unsafe_lines | length),
   history:($r | map(.confirmed)), iteration:(($r | length) + 1)}}
```

`facts.json` is the templates' `compiler_facts` input, whole. It holds:

- `baseline_status`, `proposed_status` — `ran`, `not_run` (the capture is empty or holds no build result), or `unexplained_failure` (the build failed and reported no diagnostic: a toolchain or dependency failure, not a finding).
- `diagnostics`, `open_codes` — what is reported on the code as it now stands: the proposal's diagnostics once it has run, the baseline's before. With warnings denied every lint arrives at level `error`, so the filter tells compile errors from lints by code (`E` plus four digits, or no code at all), never by level.
- `compile_errors`, `new_lints` (reported on the proposal more times than on the baseline, matched by code, file and message so that a moved line is not a new lint), `resolved_lints`.
- `unsafe_lines` (comment lines dropped), `miri` (`not_run`, `passed`, `ub`, or `failed` when Miri did not run to completion).
- `gate` — the statuses, the codes, the counts of the lists above, and `history` and `iteration` read from the experiment record. This object is what the two gates take. The filter refuses a record whose lines are out of sequence or lack a `confirmed` count.

## Gates

Both gates take the `gate` object of `facts.json` unchanged (print it with
`jq -c .gate <run_dir>/facts.json`) beside the template outputs as returned.
Type no count, status or code into an env.

### IR-1 — the inquiry accounts for what the compiler reported

```lisp
(begin (define codes-of (lambda (rows) (if (is_null rows) (list) (append (assoc! "diagnostic_codes" (car rows)) (codes-of (cdr rows)))))) (define without (lambda (xs ys) (if (is_null xs) (list) (if (member (car xs) ys) (without (cdr xs) ys) (cons (car xs) (without (cdr xs) ys)))))) (let ((grounded (string= (assoc! "baseline_status" gate) "ran")) (unexplained (without (assoc! "open_codes" gate) (append (codes-of principle_violations) (codes-of invalid_states))))) (list (list "verdict" (cond ((= (length invariants) 0) "incomplete") ((not grounded) "ungrounded") ((not (is_null unexplained)) "incomplete") (t "pass"))) (list "unexplained_codes" unexplained) (list "invariants" (length invariants)))))
```

- env: `{ "gate": <facts.json's gate>, "invariants": <inquiry output>, "principle_violations": <inquiry output>, "invalid_states": <inquiry output> }`
- `pass` — at least one invariant, the baseline ran, and every open diagnostic code is cited by a principle violation or an invalid state.
- `incomplete` — no invariant was listed, or `unexplained_codes` names diagnostics the assessment does not account for. Re-run the inquiry once with those codes named in `iteration_focus`; a second `incomplete` stops the run and is reported with the codes.
- `ungrounded` — the baseline did not run. The design may proceed, and the report says it is unverified; `IR-2` will return `undetermined`.

### IR-2 — convergence

```lisp
(begin (define codes-of (lambda (rows) (if (is_null rows) (list) (append (assoc! "diagnostic_codes" (car rows)) (codes-of (cdr rows)))))) (define without (lambda (xs ys) (if (is_null xs) (list) (if (member (car xs) ys) (without (cdr xs) ys) (cons (car xs) (without (cdr xs) ys)))))) (define confirmed-in (lambda (rows codes) (if (is_null rows) 0 (+ (if (and (not (is_null (assoc! "diagnostic_codes" (car rows)))) (is_null (without (assoc! "diagnostic_codes" (car rows)) codes))) 1 0) (confirmed-in (cdr rows) codes))))) (define open-in (lambda (rows) (if (is_null rows) (list) (if (member (assoc! "status" (car rows)) (list "fixed" "justified")) (open-in (cdr rows)) (cons (assoc! "guideline" (car rows)) (open-in (cdr rows))))))) (let ((open (assoc! "open_codes" gate)) (findings (append gaps edge_cases)) (history (assoc! "history" gate)) (iteration (assoc! "iteration" gate)) (miri (assoc! "miri" gate)) (has-unsafe (> (assoc! "unsafe_lines" gate) 0))) (let ((confirmed (confirmed-in findings open)) (unsupported (without (codes-of findings) open)) (open-guidelines (open-in guideline_violations))) (let ((series (append history (list confirmed))) (reasons (append (if (> (assoc! "compile_errors" gate) 0) (list "compile_errors") (list)) (if (> (assoc! "new_lints" gate) 0) (list "new_lints") (list)) (if (= (length reference_results) 0) (list "references_not_examined") (list)) (if (is_null open-guidelines) (list) (list "guideline_open")) (if (and has-unsafe (string= miri "ub")) (list "miri_ub") (list))))) (let ((n (length series))) (list (list "verdict" (cond ((> iteration 3) "stop_bound") ((or (not (string= (assoc! "baseline_status" gate) "ran")) (not (string= (assoc! "proposed_status" gate) "ran"))) "undetermined") ((and (not (is_null reasons)) (>= iteration 3)) "stop_bound") ((not (is_null reasons)) "continue") ((and (>= n 3) (< (* (- (nth (- n 2) series) (nth (- n 3) series)) (- (nth (- n 1) series) (nth (- n 2) series))) 0)) "oscillating") (t "converged"))) (list "reasons" reasons) (list "iteration" iteration) (list "confirmed" confirmed) (list "unsupported_citations" unsupported) (list "open_guidelines" open-guidelines) (list "notes" (if (and has-unsafe (member miri (list "not_run" "failed"))) (list "unsafe_not_miri_checked") (list)))))))))
```

- env: `{ "gate": <facts.json's gate>, "gaps": <challenge output>, "edge_cases": <challenge output>, "guideline_violations": <design output>, "reference_results": <the list passed to the challenge render> }`
- Run it after every challenge. `verdict`, in the form's clause order:
  - `stop_bound` — this would be a fourth iteration, or it is the third and `reasons` is not empty. Stop; report the reasons to the operator.
  - `undetermined` — the baseline or the proposal did not run (the proposal was not applied, or its build failed without a diagnostic). Never converged: report the design as unverified. If the cause is a failed capture, repeat the capture once and run the gate again.
  - `continue` — `reasons` lists what is open: `compile_errors`, `new_lints`, `references_not_examined` (no `find_references` output was passed), `guideline_open` (`open_guidelines` names the rows neither `fixed` nor `justified`), `miri_ub`. Write the experiment record and re-enter inquiry.
  - `oscillating` — the confirmed-finding count went up then down, or down then up, across the last three iterations. Goes to the operator rather than looping.
  - `converged`.
- `confirmed` counts the gaps and edge cases whose cited codes are all in `open_codes`; a finding with an empty citation list is unconfirmed. `unsupported_citations` lists cited codes the compiler did not report: those findings are unconfirmed, and the report says so.
- `notes` carries `unsafe_not_miri_checked` when the proposal has `unsafe` lines and Miri did not pass or report: a `converged` verdict with that note is reported as converged with `unsafe` soundness unchecked.
- The gate checks that a `find_references` output was passed, not what it says, and that a guideline row is marked `fixed` or `justified`, not whether the justification is sound. Those remain judgment.

## Experiment record

After each `IR-2`, append one line to `<run_dir>/experiments.jsonl`, taking
`iteration` and `confirmed` from the gate's result:

```
jq -n -c --argjson iteration <IR-2 iteration> --argjson confirmed <IR-2 confirmed> --arg outcome <IR-2 verdict> --arg next_step "<the single change the challenge prescribes>" --arg prediction "<what that change is expected to do to the next gate>" '$ARGS.named' >> <run_dir>/experiments.jsonl
```

Write it after the gate runs, so `outcome` is observed rather than predicted.
Never rewrite a line — a changed prediction is a new iteration. The record is
what makes the three-iteration bound hold: the filter derives `iteration` and
`history` from it, so a resumed session continues the count instead of
restarting it. A new design problem starts a new run directory and a new
record; a record is never shared between problems, and nothing is written to
the skill folder.

## Instructions

### idiomatic-rust-inquiry

1. Create the run directory, save the filter, and capture the baseline before any edit; on a later iteration, the captures already hold the current state. Run the **Compiler facts** filter.
2. Use `go_to_definition` to trace the ownership graph through the codebase. Issue it together with the baseline capture: neither reads the other.
3. Set `iteration_focus`: empty on the first iteration; afterwards, the previous challenge's `refinement_directives` and the last experiment record's `next_step`, `prediction` and `outcome`.
4. Render `idiomatic-rust/idiomatic-rust-inquiry` with `facts.json` as `compiler_facts` and the code as it now stands as `existing_code`, and execute it: evaluate the design against each of the eight principles; list the invariants; identify the invalid states; map who creates, observes, mutates and destroys each value; define the error domain; rank the violations; order the improvement targets. Cite the diagnostic codes behind each compiler-confirmed entry.
5. Run `IR-1`.

### idiomatic-rust-design

1. Run `get_code_actions` on the types the improvement targets name. Rust-analyzer's refactor suggestions are compiler-verified starting points; pass each output as returned in `code_actions`.
2. Design types that make wrong usage impossible by replacing `String` with validating newtypes, `bool` with two-variant enums, `Vec<T>` with non-empty types, raw integers with unit-aware newtypes, and invalid `Option<T>` with non-nullable types. Where a plain replacement type is not enough, extend the same move with the deeper pattern menu (absorbed from actionbook/rust-skills `m05-type-driven`): typestate — state machines as type parameters so invalid transitions do not compile (`Connection<Disconnected>` consumed to produce `Connection<Connected>`); sealed traits — a closed implementor set without closing use (`mod private { pub trait Sealed {} }`); marker traits — compile-time capability flags (`trait Validated {}`); `PhantomData<T>` — type-level relationships at zero runtime cost; ZST markers — unit structs as compile-time tags (`struct Invariant;`). Evaluate and extend the compiler's suggestions against the design goals.
3. Map the ownership DAG for each value, explicitly choosing single owners, shared immutable access, shared mutable access, or borrowed access.
4. Ensure every fallible function returns `Result<T, E>`, using `thiserror` for libraries and `anyhow` for applications, avoiding `unwrap()` in library code, and documenting all panics.
5. Design traits that define capabilities rather than taxonomies, preferring many small traits, using `impl Trait` in return positions, deriving common traits, and implementing `From<T>` for conversions.
6. Reference the current Rust ecosystem for each design decision, citing std library patterns, key crates, relevant RFCs, and API guidelines.
7. Record which design decisions were compiler-suggested vs LLM-originated in `compiler_actions_used`.
8. **Run the Rust API Guidelines checklist** against the proposed public API (https://rust-lang.github.io/api-guidelines/checklist.html). Record every violated guideline in `guideline_violations` with its guideline ID, the proposed item and a `status`: `fixed`, `justified`, or `open`. `IR-2` does not converge while a row is `open`.

### idiomatic-rust-challenge

1. **Apply the proposal to the crate** with `edit_file`, so the compiler can see it. Applying edits the working tree: do it when the request is to make the change. When the request is for a design only, do not apply — `IR-2` then returns `undetermined`, and the design is delivered as unverified with the offer to apply it.
2. Issue together, since none reads another's result: the clippy capture into `proposed.jsonl`, **`find_references`** on each type being changed (pass each output as returned in `reference_results`), and the `unsafe` grep. Then run Miri if `unsafe.txt` is not empty, and run the **Compiler facts** filter.
3. Render `idiomatic-rust/idiomatic-rust-challenge` with the new `facts.json` and execute it: find the gaps where the design fails the original problem, misses scenarios or leaves state transitions unhandled, citing specific types or functions and the diagnostic codes that confirm them; test edge cases for each type (empty inputs, maximum values, concurrent access, errors at each step, mid-operation shutdown); challenge the proposal's claims of correctness, performance and safety with counterexamples; connect the design to std types, popular crates, RFCs and API guidelines; produce a refinement directive for each gap or edge case.
4. Run `IR-2`, then append the experiment record.
5. If the run ends on anything but `converged` with `compile_errors` among the reasons, the tree must not be left broken: undo your own edits with `edit_file` and say so in the report.

## Curated external idiom annex

Six rules from leonardomso/rust-skills (MIT), pinned to upstream commit
`fd2a861ab0406a4ac536a55274d14ea6fd1ca9c9` (master as of 2026-06-14; 265 rules,
"current for Rust 1.96 / 2024 edition" per the upstream README). Selected from
categories clippy does not lint — the gap the codified-lint oracle cannot
close. Every entry is intrinsic guidance (P) until verified against the
compiler/clippy oracles: cite the rule, then check the code.

**Refresh trigger:** refresh this annex when the local toolchain's minor
version advances past the pinned Rust version (the workspace is on 1.98.1), or
when upstream changes a pinned rule. Last refreshed 2026-11-09. The pinned
commit is what the links below resolve against; update the pin and the links
together.

- [serde-enum-representation](https://raw.githubusercontent.com/leonardomso/rust-skills/fd2a861ab0406a4ac536a55274d14ea6fd1ca9c9/rules/serde-enum-representation.md) — choose enum tagging deliberately: externally, internally, adjacently tagged, or untagged.
- [serde-try-from-validate](https://raw.githubusercontent.com/leonardomso/rust-skills/fd2a861ab0406a4ac536a55274d14ea6fd1ca9c9/rules/serde-try-from-validate.md) — validate while deserializing with `#[serde(try_from = "Raw")]`.
- [mem-drop-order](https://raw.githubusercontent.com/leonardomso/rust-skills/fd2a861ab0406a4ac536a55274d14ea6fd1ca9c9/rules/mem-drop-order.md) — struct fields drop top-to-bottom, locals in reverse declaration order; control drop order deliberately.
- [unsafe-extern-block](https://raw.githubusercontent.com/leonardomso/rust-skills/fd2a861ab0406a4ac536a55274d14ea6fd1ca9c9/rules/unsafe-extern-block.md) — Rust 2024: wrap `extern` blocks in `unsafe extern { }` and annotate each item `safe` or `unsafe`.
- [unsafe-no-mangle-unsafe](https://raw.githubusercontent.com/leonardomso/rust-skills/fd2a861ab0406a4ac536a55274d14ea6fd1ca9c9/rules/unsafe-no-mangle-unsafe.md) — Rust 2024: `#[unsafe(no_mangle)]`, `#[unsafe(export_name = "...")]`, `#[unsafe(link_section = "...")]` — not the bare attribute forms.
- [async-cancel-safety](https://raw.githubusercontent.com/leonardomso/rust-skills/fd2a861ab0406a4ac536a55274d14ea6fd1ca9c9/rules/async-cancel-safety.md) — futures used in `tokio::select!` branches must be cancellation-safe.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `idiomatic-rust-inquiry.j2` | Assess the design problem against the eight principles, starting from the measured diagnostics. Returns invariants, invalid states, the ownership graph, the error domain, ranked principle violations and improvement targets; each compiler-confirmed entry cites its diagnostic codes. |
| `idiomatic-rust-design.j2` | Propose type-driven solutions with code examples, informed by the `get_code_actions` output. Returns the design, its rationale, ecosystem references, the code actions used, and the API-guideline rows with a `status`. |
| `idiomatic-rust-challenge.j2` | Adversarial review of the proposal against what the compiler, clippy, `find_references` and Miri measured on it. Returns gaps, edge cases, challenged assumptions, connections and refinement directives; each compiler-confirmed finding cites its diagnostic codes. |

Render with `render_template` and `context_path` (the contexts carry code and
tool output, too large to inline). Copyable context shapes, variable names from
each template's `[inference]` contract:

`idiomatic-rust/idiomatic-rust-inquiry`:
```json
{ "design_problem": "<the problem>", "existing_code": "<the code under design>", "design_constraints": {}, "compiler_facts": { "<the whole facts.json>": "" }, "iteration_focus": "" }
```

`idiomatic-rust/idiomatic-rust-design`:
```json
{ "design_problem": "<the problem>", "existing_code": "<the code under design>", "assessment": { "<the whole inquiry output>": "" }, "code_actions": ["<each get_code_actions output, as returned>"], "iteration_focus": "" }
```

`idiomatic-rust/idiomatic-rust-challenge`:
```json
{ "design_problem": "<the problem>", "design_proposal": { "<the whole design output>": "" }, "assessment": { "<the whole inquiry output>": "" }, "design_constraints": {}, "compiler_facts": { "<the whole facts.json>": "" }, "reference_results": ["<each find_references output, as returned>"] }
```

`code_actions` and `reference_results` are `[]` only when the tool was not run.

## Regression case

The tests evaluate the forms and the filter authored in this file, and render
the templates:

- `bash kask/scripts/check-skill-lisp-forms.sh` — `IR-1` and `IR-2` over fixed envs: a passing inquiry, an uncited diagnostic code, no invariants, no baseline; a converged proposal, each `continue` reason alone, a finding citing an unmeasured code, an empty citation list, the third-iteration stop, the iteration-four stop, an uncompiled proposal, a tail oscillation, and `unsafe` with and without a Miri result.
- `bash kask/scripts/cargo-test-nonzero.sh -p agent --lib idiomatic_rust_` — the **Compiler facts** filter over cargo JSON records (a clean build, lints, a type error, a build that fails with no diagnostic, non-JSON lines mixed in, a lint present in both captures, the experiment-record sequence, the three Miri outcomes), and the three templates in their not-run, clean and reported states.

These are computation and handoff checks. No test establishes that a design
the gates pass is a good design.

## Constraints

- The loop targets inquiry, not design — challenge findings must re-inform the assessment.
- `IR-2` runs after every challenge, never only at the end; `oscillating` and `stop_bound` go to the operator rather than looping.
- The gates read the saved facts and the templates' outputs as returned. Never type a count, a status or a diagnostic code into an env.
- Without the compiler the skill still produces a design, labelled for what it is: `IR-1` returns `ungrounded` and `IR-2` returns `undetermined`, and the report says the design is unverified.
- Run artifacts live in the run directory, never in the skill folder.
