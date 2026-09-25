---
name: tdd
description: "Test-driven development: red → green, one vertical slice at a time, tests only at agreed seams. Use to build a feature or fix a bug test-first, or when the user mentions red-green or integration tests."
---

# TDD

TDD is the red → green loop. This skill is the reference that makes the loop
produce tests worth keeping: what a good test is, where tests go, the
anti-patterns, and the rules of the loop. Every section applies on every cycle.

Adapted from Matt Pocock's `tdd` skill
(<https://github.com/mattpocock/skills/tree/main/skills/engineering/tdd>) and
fitted to zed-kask: the expectation contract comes from
`kask/docs/reference/testing-protocol.md`, the cycle gate is a `lisp_eval`
check, and the loop is anchored as a `pko:Procedure` whose cycles are
`pplan:Step`s, each closed by a `pko:StepVerification` (resolve other domain
terms with `onto_anchor`).

## When to Use

- Building a feature or fixing a bug test-first.
- The user asks for red-green, a regression pin, or integration tests.
- Executing a slice from a `task-breakdown` plan.

## When NOT to Use

- Exploratory spikes where the behavior cannot yet be named — spike first.
- Refactoring with no behavior change — existing tests govern; review owns it.
- Hunting for unknown bugs — use `bug-hunt`.

## What a good test is

Tests verify behavior through public interfaces, not implementation details.
Code can change entirely; tests shouldn't. A good test reads like a
specification — `record_resolution_feeds_calibration_read` says what
capability exists and survives refactors because it ignores internal
structure. In Rust: one `#[test]` / `#[gpui::test]` / `#[tokio::test]` per
behavior, named as the sentence it proves, with one logical assertion.

Before the first test, state the **expectation contract** for the slice
(testing-protocol §Expectation contract): who expects what outcome under which
conditions, the variables that change it, what may vary incidentally, and the
**falsifier** — the observation that would contradict it, measured
independently of the implementation. Only the user changes an expectation.

## Seams: where tests go

A **seam** is the public boundary where you observe behavior without reaching
inside: a crate's `pub` API, an MCP tool called through `Parameters`, a GPUI
entity driven through `TestAppContext`, a CLI or script exit code.

**Test only at agreed seams.** Before writing any test, write down the seams
under test and confirm them with the user. No test is written at an
unconfirmed seam. Ask: "What's the public interface, and which seams should we
test?" When the interface's shape is itself in question, consult `deep-module`.

Pick the layer the obligation needs (testing-protocol §The four layers):

| Layer | Use when | Cannot prove |
|---|---|---|
| Example test | a known input → output, a regression pin | unseen inputs |
| Property (proptest crate) | an invariant over an input domain | exhaustiveness |
| Proof (Kani / `lean-prover`) | panic-freedom of a pure, allocation-free core | anything outside its assumptions |
| Loop-closure | write → observe → recall, or a degradation being surfaced | domain invariants |

## Mocking

Mock only at system boundaries: provider HTTP, the clock, randomness, and
sometimes the filesystem. Never mock your own modules or internal
collaborators. Construct the capability under test — an MCP tool test builds
the server with its real store and inference port, not a stripped constructor
(`.rules`). Prefer in-memory or temp-dir stores over fakes; use file-backed
stores or real child processes when the obligation is reopen or transport
failure. Inject boundaries through traits the code already takes
(`InferencePort`, `ToolPort`, `Fs`), one method per external operation.

## Anti-patterns

- **Implementation-coupled** — mocks internal collaborators, tests private
  functions, or verifies through a side channel (querying SQLite instead of
  reading through the API). The tell: it breaks on a refactor that kept
  behavior.
- **Tautological** — the expected value is recomputed the way the code
  computes it, or a constant is asserted equal to itself. Expected values come
  from an independent source: a known literal, a worked example, the spec.
- **Degradation-as-success** — asserting an empty result equals success on a
  degraded path. Assert the surfaced note, status, or error kind instead.
- **Horizontal slicing** — all tests first, then all code. Tests written in
  bulk verify imagined behavior. Work in vertical slices: one test → one
  implementation → repeat, each a tracer bullet informed by the last.

## Rules of the loop

- **Red before green.** Run the new test and see it fail for the expected
  reason before writing code. Then write only enough code to pass it.
- **One slice at a time.** One seam, one test, one minimal implementation.
- **Refactoring is not in the loop.** It belongs to review (`code-review`).
- **Close each cycle with evidence.** Call `lisp_eval` with the cycle record;
  the next slice starts only on `closed`:

  form: `(if (and (string= red "failed") (string= green "passed") (string= seam_confirmed "yes") (not (string= oracle "implementation"))) (quote closed) (quote open))`

  env: `{"red": "<observed result of the first run>", "green": "<observed result after the change>", "seam_confirmed": "yes|no", "oracle": "literal|worked-example|spec|implementation"}`

  An `open` cycle names what is missing: a test that never failed, a seam
  never agreed, or an expected value derived from the code.

## Templates

| Template | Use |
|---|---|
| `tdd/tdd-seams` | Before the first test: propose seams, layers and expectation contracts for the user to confirm. |
| `tdd/tdd-cycle` | One cycle: the failing test, the observed red, the minimal change, the observed green. |

Render with `render_template` and the context the template's contract names.

## Constraints

- A green claim names the command that ran and its observed output; a claim
  without a run is not a result.
- `./script/clippy` and the crate's tests run before a slice is reported done.
- Stale comments describing old behavior are updated in the same change.
- No `unwrap()` in production code; tests may use `expect("reason")`.

## Relationship to Other Skills

- `task-breakdown` produces the slices this skill executes one at a time.
- `deep-module` supplies the interface vocabulary when a seam's shape is unclear.
- `code-review` owns refactoring and the post-change review.
- `bug-hunt` explores for unknown defects; confirmed defects return here as
  new red tests.
- `lean-prover` handles proofs; a passing property is evidence, not proof.
