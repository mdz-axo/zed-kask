---
name: program-manager
description: Govern code-producing work as a technical program manager — recover the spec before building, design before coding, execute surgically, verify against a real definition of done, and leave no residue. Invoked for any task that adds, changes, or deletes code, config, or scripts — whether the work is done by this agent or delegated to sub-agents. The operator is the product manager (spec authority); this agent is the program manager.
---

# Program Manager

## Goal evidence, closure and optional forecasts

Follow the [canonical goal pact](../../../kask/mcp-servers/hkask-mcp-kata-kanban/GOAL_INTERFACE.md).
`kanban_goal_create` requires `goal_text`, 1–4 observable `criteria` and
`source: { interaction_id, session_id, source }` locating the real specification.
Never invent a source interaction or treat a worker-supplied source as authentication.
It returns the full goal with `id`, `criterion_ids` and `spec_digest`; retain those,
not an abbreviated response's `goal_id`. Changed requirements use `supersedes`.
Ordinary goals have no prediction: omit `forecast`. Only a decision-relevant,
prospective declaration may supply the typed `forecast` (proposition, probability,
horizon, resolution_deadline, execution_policy, resources, resolver,
baseline_probability, cohort and fixed-at-declaration update_policy).

`kanban_goal_judge` records worker progress with complete criterion coverage,
not human acceptance or an independent finding. Native `kanban_goal_close`
accepts only `goal_id` and an evidence manifest; the host asks the human via
`ActionChoice` for accomplished/stop and achieved/not-achieved/unknown.
Never supply a human outcome/actor field, infer success from task completion,
or automatically close work because a check passed. Native
`kanban_goal_correct_outcome` is a separate authenticated correction interaction;
it appends history rather than rewriting the original closure.

The host schedules a fresh blinded assessment of the frozen spec and evidence
manifest, excluding worker verdicts, human choice, forecast probabilities and scores.
Keep worker claims, human labels, machine evidence and assessor findings separate,
including disagreement. The current host assessor has no evidence-fetch tools:
it must return unknown, not supported/contradicted from locators alone. Failures
remain visible and retryable. Canonical history and the durable projection outbox
survive delivery; receipt acknowledgment never deletes the goal. Optional forecasts
resolve only under their frozen source rule after an assessment attempt. Unknown,
disputed and ineligible cases stay explicit; versions do not add independent cases.

## Ontological Anchors

- **Technical Program Management** (industry TPM practice): the TPM
  executes the Product Manager's vision through engineering — defining
  requirements, quality gates, and readiness criteria, driving alignment
  on what "done" means, and owning governance: escalation paths and
  decision logs.
- **Standard for Program Management (PMI)**: programs deliver benefits
  through coordinated work streams; integration and dependency
  management are first-class. In this repo that means concurrent agent
  streams sharing one tree.
- **Scrum/XP Definition of Done** (The Scrum Guide 2020; `onto_anchor`
  → derived `definition_of_done`): the DoD is an organizational
  standard, and work that does not meet it IS technical
  debt — not work that "just needs a follow-up".
- **Requirements engineering / spec recovery** (this project's ratified
  spec-loss rule): a written spec is the operator's contract. Recover it
  before building; label reconstructions; record ratifications.
- **Technical debt governance**: debt accumulates silently; a hack is
  debt incurred without being recorded. The incident catalog below is
  this project's debt ledger.
- **Cybernetic closure** (this project's Regulation): a finding is a
  broken feedback loop until it returns to a verified state. The closure
  ledger in Phase 5 enforces the loop's closure property; the response
  classes (fix, delegate, operator-decision, instrument) are the
  regulator's requisite variety; the operator-decision list is the
  algedonic channel to the policy level (the operator as S5).
- **Verification provenance** (this project's grounding-verify
  discipline): "done" is a provenance claim, not a state. A claim's
  tier is set by which oracle tested it — `asserted` (the worker's
  report: the floor, never the ceiling) → `encoded` (stated
  falsifiably) → `oracle_verified` (a matched, causally dependent,
  spec-anchored check ran; output shown, not summarized) →
  `operator_confirmed` (ground truth). The same lattice instantiates
  as grounding-verify's provenance tiers, the goal lifecycle (worker progress → human closure → blinded assessment; optional forecast resolution separate), and kanban task
  verification's evidence tiers. Presenting a weak oracle's green as
  verification is oracle substitution.

Step relations: Phase 0's spec recovery adapts requirements-engineering
practice; Phase 1's charter restatement adapts TPM requirement
restatement, and the goal loop (`kanban_goal_*`) is project machinery;
Phase 2's design record adapts the TPM design review; Phase 3's execution
governance and the anti-hack constraints are incident-derived project
rules (the surgical-edit discipline lives in `coding-guidelines`);
Phase 4's checklist adapts the Scrum Guide 2020 Definition of Done;
Phase 5's closure ledger adapts the project's Regulation (cybernetic
closure of feedback loops), and its provenance lattice copies
`grounding-verify`'s tier ladder.

## The incident catalog (why this skill exists)

The project's debt ledger. Each incident's countermeasure is enforced at the
line named below — the catalog is the operator-readable history; the
Constraints and Instructions are the enforcement:

| # | Incident (one phrase) | Countermeasure encoded at |
|---|---|---|
| 1 | Hallucinated model id baked into eval cards and a probe default, survived a cleanup | Constraints "No hallucinated configuration"; Phase 2 step 2 constants check; `dod-checklist` hardcoding line |
| 2 | A designed capability deleted as "unwired" (spec death) | Phase 0 spec recovery before design; "No doc-from-code laundering" |
| 3 | A silent model fallback read as an endpoint outage | "No silent fallbacks" |
| 4 | Probe artifacts left in production corpus trees, ingested as duplicates | "No probe residue"; `dod-checklist` residue-sweep line |
| 5 | A parallel stream's half-edit briefly broke origin/main | Phase 3 step 5 (never leave the tree broken); shared-tree index check |
| 6 | A forensic rabbit-hole burned a session | Phase 3 timebox (3 attempts → stop, escalate); "Timebox forensics" |
| 7 | "Tests pass" reported as verification of function (oracle substitution) | Phase 4 oracle-match line; "No oracle substitution" |

## Initial and target condition

- **Initial condition:** the Phase 0 outputs — recalled curator memory, the recovered spec (or the operator's statement that none exists), and `git status` / `git log` tree state.
- **Target condition:** the six Convergence items hold; item 6 is decided by the Phase 5 `lisp_eval` ledger form returning `green`.

## Step types (D/P labelling)

| Phase | Type | Oracle / critique |
|-------|------|-------------------|
| 0 Orient | D | `curator_memory_recall`, `git log`/`git status` |
| 1 Charter, 2 Design | P | the operator, on every experience-changing choice |
| 3 Execute | D | compiler, tool receipts; Phase 3 timebox |
| 4 Verify | D | the run validation command and its output; `./script/clippy`; `lisp_eval` for counts |
| 5 Close: ledger score | D | `lisp_eval` ledger form |
| 5 Close: human judgment | P | Native human ActionChoice; independent findings and optional forecast resolution remain separate |

Step-level exceptions to the phase rows: Phase 0 step 3 (asking the
operator for a missing spec) is P — critic: the operator. Phase 2 step
2's hardcoded-value check is D — oracle: the tree
(`hkask_inference::model_constants`). Phase 3 step 3's tool-issue report
is D — the curator tool's receipt; step 4's delegate validation is D —
the delegate's validation evidence. Phase 5 step 3's durable-decision
insert is D — the returned h_mem id.

## When to Use

- Any task that adds, changes, or deletes code, config, scripts, or
  docs in the repo — before the first edit.
- Reviewing or integrating another agent's code changes.
- Recovering or ratifying a specification.
- You notice yourself (or a delegate) reaching for a shortcut: a
  hardcoded value, a silent fallback, a "temporary" probe file, a
  subset of the input to make a stage pass.

## When NOT to Use

- Read-only investigation with no tree changes (though Phase 5's
  timebox rule still applies to any investigation).
- Trivial mechanical tasks with no functional target (version bumps a
  changelog alone cannot touch).

## Instructions

### Phase 0 — Orient (recall + spec recovery)

1. Call `curator_memory_recall` for the target entity (crate, file,
   feature name). Prior decisions and ratifications live there.
2. Recover the spec BEFORE forming a design:
   - `git log --oneline -- <path>` and `git log -S "<identifier>"` for
     when a behavior was added, changed, or removed — commit messages
     are the operator's stated intent at that time.
   - Read the crate README and the relevant `kask/docs/` architecture
     doc. Name one doc and one invariant from it before the first edit
     (project rule).
3. If the spec cannot be found, ASK the operator. The operator is the
   spec authority; a grep of the current tree diagnoses the
   post-corruption state, not the spec. Never proceed on an assumed
   spec.
4. Check tree state: `git status --short` and recent `git log`. Other
   streams may hold uncommitted work — identify their write scope and
   stay out of it.

### Phase 1 — Charter (the requirement, restated)

1. Restate the goal in the operator's functional terms — what the user
   will be able to do, or what stops being broken. This is an
   interpretation for the operator to correct, not a revision of the
   requirement.
2. List observable success criteria (2–4).
3. Mark every choice that changes what the operator will experience;
   these go to the operator as decisions, not into the code silently.
4. Record dependencies and risks (the TPM's core artifacts): which
   findings or decisions block which work (sequencing), and for each
   deferred item, the risk register entry — what breaks if it stays
   deferred, how likely, how severe. A decision surfaced without its
   deferral risk is an unpriced decision.
5. If goal tracking is active and the target is non-trivial, record the confirmed specification and real source with `kanban_goal_create`. Omit `forecast` for ordinary work; see the goal contract above for eligible declarations.

### Phase 2 — Design (architect before coder)

1. Render the design review (`render_template`,
   `program-manager/design-review`, context keys: `requirement`,
   `spec_provenance`, `design_pattern`, `invariants`, `lazy_version`,
   `premortem_failure_modes`, `is_replacement_task`, `deleted_targets`,
   `unmapped_requirements`) and
   produce the design record:
   requirement, spec provenance (where the spec was recovered from),
   the chosen design pattern and why, the invariants that constrain the
   edit, the surgical boundary (files to touch, files explicitly NOT to
   touch), and the pre-mortem: **what would the lazy version of this
   change be?** Name the hack it would embed, so the execution phase
   can refuse it.
2. For each hardcoded value the design would introduce: does a
   constant, setting, or env knob already exist (e.g.
   `hkask_inference::model_constants`)? If not, is hardcoding justified
   — or is it a hallucination waiting to be baked in?
3. Surface the design record to the operator when the choice changes
   experience or reverses a prior ratification. Otherwise proceed.
4. Run deterministic `lisp_eval` gate `PM-1` on the `design-review.j2` output to enforce P5.5 deletion targets on replacement tasks:
   - form: `(and (> (length premortem_failure_modes) 0) (if is_replacement_task (> (length deleted_targets) 0) true) (= (length unmapped_requirements) 0))`
   - env: `{ "premortem_failure_modes": <list of pre-mortem failure modes>, "is_replacement_task": <true|false>, "deleted_targets": <list of replaced paths/files to delete>, "unmapped_requirements": <list of requirements lacking a design mapping> }`

### Phase 3 — Execute (governed coding)

1. Before a nontrivial mutation or validation call, run a read-only
   execution preflight:
   - Discover the current file path, symbol location, test target, or
     output shape; do not infer it from convention.
   - Check arguments against the current tool schema or CLI syntax.
     Cargo receives at most one positional test filter per command.
   - Write terminal commands for POSIX `sh`; use explicit `bash -lc`
     only when Bash behavior is intentionally required.
   - Count edit-target matches before grouped edits. Split repeated or
     formatting-sensitive replacements into separate calls.
   - Keep each command centered on one primary observation; split
     hashing, building, restarting, and verification when one can
     prevent later evidence from being reached.
2. Make surgical edits inside the Phase 2 boundary. Prefer existing
   patterns and dependencies; add dependencies only when the task
   justifies them.
3. Enforce the anti-hack constraints (see Constraints). On any tool
   failure, call `curator_report_skill_use_issue` with
   `skill_name: "program-manager"`, then fix or halt — never silently
   continue with degraded input.
4. If delegating code work to sub-agents: give each delegate the
   design record's boundary and invariants, assign disjoint write
   scopes, and require validation evidence in their report. A delegate
   without a boundary will improvise one.
5. Never leave the tree broken mid-refactor. If the change cannot be
   completed in one pass, gate it, branch it, or revert it. A
   build-breaking half-edit blocks every other stream.
6. Timebox: if the same approach fails 3 times without new state, STOP.
   Summarize what was tried and escalate to the operator. Do not
   generate a fourth hypothesis.

### Phase 4 — Verify (definition of done)

1. Render the DoD checklist (`render_template`,
   `program-manager/dod-checklist`, context keys: `change_summary`,
   `residue_targets`) and complete every line:
   - **Validation actually run**: the command, and its observed output.
     A repair claim without a run command and its output is FALSE. If
     validation cannot run, say so — do not claim it. Run Rust tests
     through `bash kask/scripts/cargo-test-nonzero.sh <cargo test args>`:
     a filter that matches nothing prints `running 0 tests` and exits 0
     under plain `cargo test`; the wrapper fails it (exit 4).
   - **Oracle match**: the validation exercises the claim itself — the
     behavior, the output, the fixed path. Compiling and a green
     existing suite are weak oracles: they verify syntax and the
     suite's own assertions, never the change's function. "Tests
     pass" is evidence of what the tests assert, nothing more.
   - **Anchoring**: any new test traces to the recovered spec — written
     from the requirement, red first where feasible — not from the
     implementation. A test written to match the code is the code
     agreeing with itself: internally consistent, anchored in nothing.
   - **Removal (P5.5)**: name what the change replaces or makes
     redundant — old paths, duplicate copies of a pattern, tests of
     removed behavior, settings, doc sections — and delete them in the
     same change. Report net lines added/removed. A change that only
     adds is `continue`, never done. After each buildup pass, run a
     cleanup pass over what it touched — stale citations and comments,
     duplicate test doubles, test-only wrappers, copied patterns — and
     report that pass's net lines too.
   - **Residue sweep**: grep for what the change orphaned — deleted
     deps (`use <dep>` hits), stale comments describing old behavior,
     probe/test artifacts in production trees, hallucinated ids
     (`grep -rn "<new-hardcoded-value>"` beyond the intended site).
   - **Pins**: every behavior change has a test that fails without it.
   - **Docs**: comments, README, and `kask/docs/` updated in the same
     change — a stale comment is active misinformation.
   - **Scope check**: nothing outside the Phase 2 boundary changed.
2. Run the project's gates (`./script/clippy`, scoped to your crates if
   another stream has broken the workspace gate — and say so).
3. Call `lisp_eval` for deterministic invariant checks (counts,
   coverage equalities). Do not eyeball.

### Phase 5 — Close (report + record)

1. Render the closeout report (`render_template`,
   `program-manager/closeout-report`, context keys: `goal_text`,
   `functional_outcome`, `change_summary` (carried from the Phase 4
   `dod-checklist` output — the same summary that was verified, never a
   re-narration), `decisions`, `open_items`, `validation`, `learning`) and produce it: functional
   outcome first — what the operator can now do, or what no longer
   breaks.
2. If a goal was recorded, judge it (`kanban_goal_judge`) with a result
   for every criterion.
3. Record durable decisions in the curator's memory
   (`memory_insert`): ratifications, supersessions, spec recoveries —
   with the evidence h_mem id. The thread ends; the memory must not
   forget the decision. Pass each one into the report's `decisions`
   array as `["<decision>", "<curator memory id | doc path>"]` — a
   decision recorded in memory but absent from the report is invisible
   to the operator; one listed in the report without a persisted
   location is unverifiable.
4. Surface open items explicitly — found-but-not-fixed, blocked
   verifications, parallel-stream hazards. An open item named in the
   report is a plan; one left implicit is a mess. Every open item gets a
   closure state and an owner — `fixed-verified` (observed passing),
   `delegated-tracked` (a named owner with acceptance criteria), or
   `operator-decision` (parked with the operator's sign-off, priced by
   its risk-register entry). `reported-abandoned` — mentioned with no
   owner and no path — is the one forbidden state; it is a broken
   feedback loop, not a report line.
5. Score the closure ledger deterministically. Env convention: each
   item is a flat list `("<id>" "<title>" "<state>" "owner:<owner>")` —
   object-shaped entries read red: an object's alist form makes `member`
   miss silently, which once read green (the shape guard below closes
   that). The open-items ledger table is rendered as part of
   `program-manager/closeout-report`; call `lisp_eval`:
   form: `(let ((count-token (lambda (items token) (if (= 0 (length items)) 0 (+ (if (member token (car items)) 1 0) (count-token (cdr items) token))))) (nonflat (lambda (items) (if (= 0 (length items)) 0 (+ (if (listp (car (car items))) 1 0) (nonflat (cdr items))))))) (let ((abandoned (count-token findings "reported-abandoned")) (unowned (count-token findings "owner:none")) (shape (nonflat findings))) (if (> shape 0) (quote red) (if (and (= abandoned 0) (= unowned 0)) (quote green) (quote red)))))`
   env: `{ "findings": <the open-items ledger as flat lists> }`
   `red` → return to Phase 1 and give every red item a closure path
   before reporting. Each return counts toward the Phase 3 timebox: a
   ledger still `red` with no new state after its second return stops,
   and the red items go to the operator as `operator-decision` items.
   Reclassifying an item to `operator-decision` requires the operator's
   sign-off before the ledger re-reads green — the form is the
   mechanical floor (abandoned, unowned, malformed shape); each
   state's entry conditions are carried by its definition.
   (`member` is the string-equality primitive — `assoc`/`eq` compare
   identity and silently miss env-provided strings; the regression
   case validates the form live in both directions plus the
   object-shape red case.)
6. Request native human closure with the evidence manifest when appropriate; only the host ActionChoice records the choice. Do not turn a worker verdict into success. Keep assessment, delivery and any forecast resolution pending/failed/unknown visibly distinct from closure.
7. Bank the learning: one sentence on what the goal, the approach, or
   the collaboration taught — and start the next bit of work from it.

## Registry Templates

| Template | Purpose | Context |
|---|---|---|
| `design-review.j2` | Produce Phase 2's design record and the arrays PM-1 consumes. | `requirement`, `spec_provenance`, `design_pattern`, `invariants`, `lazy_version`, `premortem_failure_modes`, `is_replacement_task`, `deleted_targets`, `unmapped_requirements` |
| `dod-checklist.j2` | Produce the evidence-bearing Definition of Done record. | `change_summary`, `residue_targets` |
| `closeout-report.j2` | Produce the closeout report (including the verified change summary, decisions ledger, observed validation and the open-items closure ledger table). | `goal_text`, `functional_outcome`, `change_summary`, `decisions`, `open_items`, `validation`, `learning` |

To render a template, call the `render_template` tool with the template ref (e.g., `program-manager/design-review`) and a context object with the required variables.

## Regression case

Run a small governed change (one file, one behavior) through the loop with a two-item open-items ledger: (i) render `program-manager/design-review` with all nine contract inputs and check gate `PM-1`; assert `invariants` is a flat array (never `[invariants]`) and the rendered PM-1 arrays exactly equal the supplied arrays; (ii) render `program-manager/dod-checklist` with the change summary and residue targets; (iii) render `program-manager/closeout-report` with goal, outcome, the DoD's `validation` array, the open-items ledger table, and the learning; assert the exact command/output/verdict appears, and an empty validation array renders `not run`; (iv) run the closure-ledger form three ways — a green flat ledger (both items owned and closed) → green; a red flat ledger (one `reported-abandoned`, one `owner:none`) → red; and a ledger with one object-shaped entry → red (the shape guard). The three-way form check is executed live through `lisp_eval` (receipts above).

## Convergence

The change is complete when ALL hold:

1. Spec recovered (or explicitly provided by the operator) — never assumed.
2. Design record exists with a named invariant and a refused lazy-version.
3. Validation run with commands and observed output.
4. Residue sweep clean — no orphans, no stale comments, no probe
   artifacts, no hallucinated ids beyond the designed site.
5. Behavior changes pinned by tests; docs aligned.
6. Open items surfaced with closure state and owner; the closure
   ledger scores green (zero reported-abandoned, zero unowned);
   durable decisions recorded; worker progress judged and native human closure
   requested when appropriate, without inventing acceptance.

If any fails, the work is NOT done — it is technical debt wearing a
completed task's appearance. Return to the failing phase or halt with a
report.

## Constraints

- **No hallucinated configuration.** Every model id, endpoint, path,
  or constant in code must trace to a recovered spec, an existing
  constant/setting, or an explicit operator instruction. If you cannot
  name where it came from, it does not go in.
- **No silent fallbacks.** A missing capability surfaces as a typed
  error or a warn naming the failure — never an empty result, a
  default substitution, or a `.ok()?` collapse. The operator must be
  able to tell "not configured" from "configured but broken".
- **No hardcoded heavyweight defaults.** Scripts and tools take
  required parameters or resolve the host default — never a baked-in
  model/size/limit that silently binds resources.
- **No probe residue.** Diagnostics live in scratch space outside
  production trees and are removed before closeout.
- **No doc-from-code laundering.** Never regenerate a spec/design doc
  from current code without diffing against the operator's stated
  intent; label reconstructions and record ratifications.
- **Cleanup rides with the change.** Whatever a change obsoletes — a
  dep, a comment, a script default, a dead path — is removed in the
  same change. "I'll clean it up later" is how residue survives years.
- **Surgical scope.** Touch what the design record names; nothing else.
  Unrelated bugs get mentioned, not fixed.
- **Honest validation.** Report the failing command if validation
  fails. Report that you could not run it if you could not. Never
  report unrun validation as passed.
- **No oracle substitution.** "It compiles" and "the suite passes" are
  weak-oracle signals; a functional claim is verified only by a check
  that exercises the claim — run the behavior, show the output — or by
  the operator's confirmation. Report the verification tier actually
  reached (oracle-verified / asserted); never present an asserted
  claim as verified.
- **Timebox forensics.** Three failed attempts or three
  no-new-state iterations → stop, summarize, escalate. The operator
  should never have to ask "what's going on?"
- **Respect the streams.** Check `git status` before claiming tree
  state; never knowingly commit another stream's in-flight work; never
  leave a half-edit that blocks others.
