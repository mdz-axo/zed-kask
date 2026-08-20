---
name: principle-constraints
core: false
description: "Compiles a stated principle into a set of checkable, code-path-anchored constraints with named falsifiers. Each constraint carries an assertion, an enforced_at location (file:line or UNKNOWN), a falsifier (test name that must exist and pass), and a status (enforced | gap | unverified). Constraints without a located enforcement path are flagged as gaps; the gap is the skill's highest-value output. The output is a proposal — a human reviews it before any constraint becomes permanent. Sibling to constraint-forces-recast (which generates concepts satisfying a constraint set) — this skill generates the constraint set from a principle."
---


# Principle Constraints

Compiles a stated principle into a set of checkable, code-path-anchored constraints with named falsifiers. The output is a *proposed* constraint set — a human reviews it before it becomes permanent, the same gate the paper ("Where Agency Actually Lives", Axolotl Partners, 2026, §4) demands for everything that touches permanent memory.

## When to Use

- You have a principle (from a paper, a `.rules` entry, a doc comment, or an architectural decision) and want to know: is it enforced in code? Where? Which tests pin it?
- You want to surface gaps where a principle is asserted but enforcement is missing or untested — the skill's highest-value output.
- You want to maintain a constraint set over time: re-check it against the codebase after architectural changes to detect drift.
- You are designing a new architectural principle and want to compile it into checkable constraints before committing, so future changes that violate it have a test that goes red.

## When Not to Use

- You want to generate concepts satisfying a constraint set — use `constraint-forces-recast` (the sibling skill; inverse direction of derivation).
- You want to review a code change against an existing spec — use `code-review`.
- You want to audit a skill manifest for logic errors — use `skill-logic-audit`.
- You want to maintain skill manifests — use `skill-maintenance`.

## Inputs

| Input | Type | Default | Description |
|-------|------|---------|-------------|
| `principle` | string | (required) | The principle statement, quoted verbatim from its source. |
| `principle_source` | string | (required) | Citation for the principle: paper section, doc path, or rule file. A principle without a source is Unsourced. |
| `mode` | `derive` \| `verify` | `derive` | `derive` = produce a new constraint set; `verify` = re-check an existing set against the codebase. |
| `existing_constraints` | array | `[]` | For `verify` mode: the previously-derived constraint set to re-check. |
| `enforcement_hints` | array | `[]` | Optional: known files/types/functions likely to enforce the principle. Speeds location but is not trusted — the skill verifies. |

## Instructions

### Derive mode (process step 1 — `principle-derive.j2`)

The skill calls `render_template` with only `principle-derive` (step 1); both `derive` and `verify` modes route through this single template via the `mode` input. The `principle-verify.j2` template is registered in the crate but NOT referenced by the skill — see the legacy note under Verify mode below.

1. **Locate enforcement code.** Use `grep`, `codegraph_query`, `codegraph_traverse`, and `read_file` to find the actual code that enforces the principle. Cite file:line. Do not infer enforcement from doc comments alone — read the code that does the enforcement.
2. **Identify existing falsifiers.** For each enforcement location, search for tests that pin it. A falsifier is a test that constructs the specific scenario and asserts the required behavior — it would go red if the enforcement were weakened.
3. **Derive the constraint set.** For each enforcement path + falsifier pair, emit a constraint with: `id`, `assertion`, `kind` (structural | review), `enforced_at` (file:line or UNKNOWN), `enforcement_mechanism`, `falsifier` (test name or `MISSING: <suggested>`), `status` (enforced | gap | unverified), `notes`.
4. **Flag gaps.** A gap means: the principle is asserted but the code doesn't enforce it, or the code enforces it but no test pins it. Gaps are the skill's highest-value output. For each gap, emit the principle asserted, the enforcement status, and a proposed remediation.
5. **Emit the summary.** Counts: total, enforced, gaps, unverified. `human_review_required` is true if any gaps or unverified.

### Verify mode (legacy — `principle-verify.j2` is registered in the crate but NOT invoked by the skill execution; verify mode runs through `principle-derive.j2` at step 1 via the `mode: verify` input)

1. For each constraint in `existing_constraints`, read the file at `enforced_at` and confirm the enforcement logic is still there.
2. Grep for the falsifier test name. If it exists and passes, the constraint is still enforced. If deleted or renamed, the constraint is stale.
3. Compare the principle to the current enforcement — has it been weakened or strengthened?
4. Emit a drift report per constraint: previous_status, current_status, drift kind, action_required.

## Constraints

- **No fabricated file:line citations.** If you did not read the file, you cannot cite it. Use `UNKNOWN` and let the human investigate.
- **No fabricated test names.** If you did not find the test via grep, you cannot name it as a falsifier. Use `MISSING: <suggested name>`.
- **No constraint without an `enforced_at` field.** Even if it's `UNKNOWN`, the field must be present — absence is not a verdict.
- **No promotion of inference to `enforced` status.** A constraint you believe is enforced but did not verify by reading the code is `unverified`, not `enforced`.
- **The output is a proposal.** Do not write files, modify code, or register constraints. A human reviews before anything becomes permanent.
- **The code enforces more than the prose states.** Read the enforcement code carefully — there may be cases the principle's prose doesn't explicitly articulate but the code enforces. Capture those.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `principle-derive.j2` | Take a principle as input (prose statement + source citation) and emit a proposed constraint set. Each constraint is test-shaped: assertion, enforced_at (file:line or UNKNOWN), falsifier (test name), status (enforced | gap | unverified). The template instructs the agent to locate enforcement code via grep/codegraph, identify existing tests that pin the enforcement, and flag gaps where the principle is asserted but enforcement is missing. The output is a proposal — a human reviews it before any constraint becomes permanent. |
| `principle-verify.j2` | Take a previously-derived constraint set and verify each constraint against the current codebase: does enforced_at still point to real code? Does the falsifier test still exist and pass? Has the principle been weakened or strengthened since the constraint was derived? Emit a verification report with per-constraint status and a list of stale constraints requiring human review. This is the maintenance mode — run on architectural changes to detect constraint drift. |

To render a template, call the `render_template` tool with the template ref (e.g., `essentialist/essentialist-flow`) and a context object with the required variables.

## Reference: Manual Pass on P1

The manual pass on the principle "conclusions never promoted to verified fact" (paper §4, last paragraph) produced five constraints from `kask/crates/hkask-verification/src/grounding.rs`:

1. `Derived` from `Inferred` is `Inferred`, not `Derived` (L1001-1022, falsifier: `derived_from_inferred_is_inferred`)
2. `Derived` from `Unsourced` (nulled) is nulled (L988-998, falsifier: `derived_from_nulled_is_nulled`)
3. `Derived` from absent source is `Unsourced` (L981-987, falsifier: `derived_from_absent_is_unsourced`)
4. `Derived` from `Sourced` keeps `Derived` tag (L1023-1028, falsifier: `derived_from_sourced_is_derived`)
5. Weakest-link propagates through derived chains (L1001-1022, falsifier: `derived_chain_recurse_to_weakest`)

All five tests pass. The code enforces more than the prose states — the manual pass found three constraints the prose didn't explicitly articulate. This is the skill's value running in reverse: the code teaches the principle's full shape. Derivations should aim for the same completeness by reading the enforcement code carefully, not just parsing the principle's prose.

## Ontological Anchors

- **Pragmatic Semantics** — distinguishes IS (structural invariant, checkable in type system) from OUGHT (design preference, checkable by review). Each constraint is labeled `structural` or `review`.
- **Verification for Agent Ecologies** — the six-valued grounding vocabulary (Sourced / Inferred / Derived / UncommissionedInference / Narrative / Unsourced) is the model for the skill's own output discipline: a constraint without a code path is Unsourced, not enforced.
- **Where Agency Actually Lives (Axolotl Partners, 2026)** — the paper's §4 "gate at the mouth of every loop" is the discipline applied to the skill's own output: verify before it counts as fact.

## Relationship to Sibling Skills

- **`constraint-forces-recast`** — sibling. CFR generates concepts satisfying a constraint set (constraints → concepts). `principle-constraints` generates a constraint set from a principle (principle → constraints). Inverse directions of derivation; both treat constraints as generative.
- **`pragmatic-semantics`** — provides the IS/OUGHT vocabulary the skill uses to label constraint `kind`.
- **`skill-maintenance`** — the verify mode is the same pattern (audit staleness, detect drift) applied to constraint sets instead of skill manifests.
- **`code-review`** — distinct. `code-review` reviews a change against a spec; `principle-constraints` maintains the constraint set itself.