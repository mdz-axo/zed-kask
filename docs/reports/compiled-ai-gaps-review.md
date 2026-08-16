# Compiled AI Gaps 1-3: Skill Review and Revised Plan

## Review Methodology

This review applies five evaluation lenses to the proposed plan in
`compiled-ai-gaps-1-3-plan.md`:

1. **Essentialist** — eliminative challenge (Exist/Surface/Contract)
2. **Pragmatic-cybernetics** — feedback loop integrity verification
3. **Pragmatic-semantics** — IS/OUGHT classification of claims
4. **Grill-me** — adversarial interrogation of assumptions
5. **Idiomatic-rust** — Rust design quality

Each lens produced findings. The findings that survived cross-validation
are consolidated below. Findings that were refuted by codebase evidence
are documented as refuted.

---

## Critical Finding: The Plan's Foundation for Item 2 Does Not Exist

### What the plan claims

> kask already has the human drift detection channel (`result_feedback` →
> `operator_feedback` spans → `SkillSpanStore` → gemba walk review).

### What the codebase actually has

`RegulationLedger::record_skill_span` is **defined but never called**.
A grep for `record_skill_span(` across the entire codebase returns exactly
one hit: the definition itself in `runtime.rs:670`. No code anywhere
calls it.

This means:

- `SkillSpanStore` exists but is empty at runtime — no spans are ever
  recorded into it
- `SkillFeedbackSpan::Outcome` and `SkillFeedbackSpan::OperatorFeedback`
  exist as enum variants with namespace strings, but nothing emits them
- `result_feedback` (in `hkask-mcp-companies/src/tools/valuation.rs:1157`)
  records to an in-process `learning` state for provider quality tracking —
  it does **not** call `record_skill_span` and does **not** emit
  `reg.skill.<id>.operator_feedback` spans
- `RegulationLedger::query_skill_feedback` exists and works, but will
  always return an empty `Vec` because nothing populates the store

### Impact on the plan

**Item 2 (drift detection) cannot be built as proposed.** The plan proposes
a `FeedbackDriftDetector` that trends `operator_feedback` disposition rates
from `SkillSpanStore`. There is no data to trend. The plan must be revised
to first wire the emission path before building detection on top of it.

**The gemba loop specification has the same problem.** It claims
`reg.skill.<id>.outcome` and `reg.skill.<id>.operator_feedback` are
existing feedback signals. They are defined as infrastructure but not
wired. The gemba briefing would have no data to aggregate.

### Pragmatic-semantics classification

The plan's claim that these spans are "existing infrastructure" is an
**IS claim that is false**. The infrastructure exists (types, storage,
query path), but the emission path does not. The correct claim is an
**OUGHT**: "the infrastructure should be wired to emit spans." The plan
must be revised to reflect this.

---

## Essentialist Review: Exist / Surface / Contract

### Item 1: Golden-Output Validation

**Exist gate:** Does `GoldenOutputFixture` need to exist?

The plan proposes a new struct with `input`, `expected_output`,
`comparison`, `threshold` fields. But `BundleManifest` already has
`output_schema: Option<serde_json::Value>` (in `bundle/manifest.rs:88`)
and templates already have `contract.output` frontmatter. The question
is whether golden-output validation is a new concept or an extension of
existing output schema validation.

**Verdict:** `GoldenOutputFixture` survives the Exist gate — it serves a
different purpose than `output_schema`. `output_schema` validates
structure (does the output conform to the JSON Schema?).
`GoldenOutputFixture` validates semantics (does the output match the
expected value for a known input?). These are genuinely different checks.

**Surface gate:** Is the purpose already served by something else?

The test suite (`manifest_load_validation.rs`, `yaml_schema_validation.rs`)
validates manifest structure at build time. But it doesn't validate skill
_outputs_ against expected values. The `ConvergenceTracker` checks
convergence metrics, not output accuracy. No existing mechanism does
golden-output comparison.

**Verdict:** Survives the Surface gate.

**Contract gate:** Can it be contracted to something simpler?

The plan proposes four comparison strategies (`exact`, `json_subset`,
`regex_match`, `mermaid_valid`). This is speculative generality — no
existing skill needs all four. The `.rules` say "trait-with-one-impl is
speculative generality." Four comparison strategies when zero skills
currently use golden outputs is the same pattern.

**Verdict:** Contract to one strategy. Start with `exact` string
comparison. Add more only when a skill actually needs them. The
`comparison` field should be a plain `String` matched at validation time,
not an enum with four variants.

**Revised proposal:** Add `golden_outputs: Option<Vec<(String, String)>>`
to `BundleManifest` — a list of `(input_context_key, expected_output)`
pairs. Validation runs the skill with the input and compares the output
string exactly. No new module — put the validation function in
`output_schema.rs` since it's already the output-validation module. No
new `GoldenOutputFixture` struct.

### Item 2: Drift Detection

**Exist gate:** Does `FeedbackDriftDetector` need to exist?

As established above, the data source (`operator_feedback` spans) is not
populated. Building a detector over an empty store is dead code.

**Verdict:** `FeedbackDriftDetector` fails the Exist gate **as proposed**.
It cannot exist until the emission path is wired.

**Revised proposal:** The first step is wiring `record_skill_span` calls
into the skill execution path. Specifically:

1. `BridgeManifestExecutor::execute_skill` should call
   `record_skill_span(skill_name, "outcome", payload)` after the cascade
   completes (success or failure)
2. `result_feedback` should be extended (or a new path added) to call
   `record_skill_span(skill_name, "operator_feedback", payload)` when
   feedback is provided for a skill result

Once the emission path is wired, the drift detector becomes viable. But
the detector itself should be contracted: instead of a new
`drift_detector.rs` module with a `FeedbackDriftDetector` struct, add a
`feedback_trend` method to `MetacognitionLoop` that queries
`SkillSpanStore` directly and computes the trend inline. No new struct,
no new module.

### Item 3: Typed Failure Classification

**Exist gate:** Does `SkillExecutionError` need to exist?

The plan proposes a two-variant enum (`CompileTime`, `Runtime`). The
alternative is keeping `Result<String, String>` and embedding the
classification in the error string (e.g., `"compile_time:load: ..."`).
The string approach is simpler but loses type safety and makes pattern
matching impossible.

**Verdict:** `SkillExecutionError` survives the Exist gate. The
classification is load-bearing — it determines whether the caller retries
(runtime) or suggests maintenance (compile-time). String parsing for this
decision is fragile.

**Surface gate:** Is the purpose already served?

No. The current `Result<String, String>` conflates all failures. The
caller (`SkillTool::run`) has no way to distinguish them.

**Verdict:** Survives the Surface gate.

**Contract gate:** Can it be contracted?

The plan proposes `phase: String` on both variants. This is fine — it's
a diagnostic field, not a routing field. The routing decision is
`CompileTime` vs `Runtime`, which is the variant itself.

But `partial_output: Option<String>` on `Runtime` is questionable. The
cascade already returns partial output on gas exhaustion and convergence
failure (via `extract_final_step_result`). If the cascade returns a
partial result, it's not an error — it's a degraded success. The error
type should only represent actual failures, not degraded successes.

**Verdict:** Drop `partial_output` from the error type. If the cascade
produces a partial result, return it as `Ok(partial_result)` with a
warning embedded in the output. The error type represents failure only.

**Revised proposal:**

```rust
pub enum SkillExecutionError {
    CompileTime { skill_name: String, phase: &'static str, message: String },
    Runtime { skill_name: String, phase: &'static str, message: String },
}
```

`phase` is `&'static str` because the phases are known at compile time
(`"load"`, `"input_validation"`, `"inference"`, `"gas_exhausted"`,
`"convergence_failed"`). No `partial_output` — degraded success returns
`Ok` with a warning.

---

## Pragmatic-Cybernetics Review: Feedback Loop Integrity

### Loop 1: Skill execution → outcome span → gemba review → skill refinement

```
Skill executes → record_skill_span("outcome") → SkillSpanStore
    → gemba walk queries → operator reviews → skill refined
    → next skill execution senses the refinement
```

**Loop integrity:** **Broken.** The `record_skill_span` call is missing.
The loop opens at the skill execution point and never closes. The gemba
walk queries an empty store.

**Fix:** Wire `record_skill_span` into `BridgeManifestExecutor::execute_skill`
after the cascade completes. This is the missing wire — not a new
component.

### Loop 2: Operator feedback → operator_feedback span → drift detection → alert

```
Operator rates result → record_skill_span("operator_feedback")
    → SkillSpanStore → drift detector trends → alert if declining
    → operator reviews in gemba walk → skill refined
```

**Loop integrity:** **Broken at two points.**

1. `result_feedback` doesn't call `record_skill_span` — the feedback
   never reaches `SkillSpanStore`
2. No drift detector exists — even if the data were there, no alerting
   on trends

**Fix:** Wire `result_feedback` (or a new skill-specific feedback path)
to call `record_skill_span`. Then add trend analysis to
`MetacognitionLoop::sense()`.

### Loop 3: Failure classification → remediation routing

```
Skill fails → SkillExecutionError classified → SkillTool routes
    → CompileTime: suggest skill-maintenance
    → Runtime: surface error or partial result
```

**Loop integrity:** **Broken at the classification point.** All failures
return `String`, so routing is impossible.

**Fix:** Change the return type to `SkillExecutionError` and classify at
the failure point.

### Loop 4: Golden-output validation → drift detection

```
Skill executes → golden-output check → pass/fail recorded
    → trend over time → drift detected → skill refined
```

**Loop integrity:** **Non-existent.** No golden-output check exists, and
no trend path exists.

**Fix:** This is genuinely new infrastructure, but it should be built
last (after loops 1-3 are closed) because it serves the narrowest case
(deterministic-output skills only).

### Cybernetics verdict

The plan's priority ordering (3 → 2 → 1) is **correct** but incomplete.
Item 3 (failure classification) closes loop 3. But item 2 (drift
detection) requires closing loop 1 first (wiring `record_skill_span`),
which the plan doesn't mention. The revised priority:

1. **Wire `record_skill_span` emission** (prerequisite for items 1 and 2)
2. **Item 3: typed failure classification** (closes loop 3 independently)
3. **Item 2: drift detection** (depends on step 1)
4. **Item 1: golden-output validation** (depends on step 1 for trend data)

---

## Grill-Me Review: Adversarial Interrogation

### Q: "The plan says `result_feedback` feeds `operator_feedback` spans. Show me the call site."

**Answer:** There is no call site. `result_feedback` records to an
in-process `learning` state in `hkask-mcp-companies`. It does not call
`record_skill_span`. The plan's claim is false.

**Impact:** Item 2's foundation must be rebuilt.

### Q: "The plan says convergence metrics aren't persisted. But `SkillFeedbackSpan::Convergence` exists. Why isn't it being emitted?"

**Answer:** `SkillFeedbackSpan::Convergence` exists as an enum variant
with a namespace string. But `ConvergenceTracker` lives in
`hkask-templates` and has no dependency on `RegulationLedger` (which
lives in `hkask-regulation`). The two crates are separate. The emission
would require either:

- `BridgeManifestExecutor` (which has access to both) calling
  `record_skill_span` after convergence check, or
- A new dependency from `hkask-templates` on `hkask-regulation` (wrong
  direction — templates shouldn't depend on regulation)

**Impact:** The bridge is the right place for the emission. This is
consistent with the revised proposal for wiring `record_skill_span` into
`BridgeManifestExecutor::execute_skill`.

### Q: "The plan proposes `partial_output: Option<String>` on `Runtime` errors. Where does the partial output come from?"

**Answer:** The cascade returns `CascadeOutcome` which contains
`last_result_step`'s value. `extract_final_step_result` extracts it. If
the cascade fails due to gas exhaustion, the `CascadeOutcome` may still
contain partial results from completed steps. But the current error path
(`run_manifest_cascade_with_manifest` returns `Err(String)`) discards
the partial outcome — it's mapped to a string error at line 317:
`.map_err(|e| format!("Manifest execution failed: {e}"))`.

**Impact:** To support `partial_output`, the error mapping would need to
preserve the `CascadeOutcome`. But this is complexity for a degraded-success
case that would be better handled as `Ok(degraded_result)` with a warning.
The essentialist review already identified this — drop `partial_output`.

### Q: "The plan proposes four comparison strategies for golden outputs. Which existing skills would use each?"

**Answer:** No existing skill would use any of them — no skill currently
declares golden outputs. The four strategies are speculative.

**Impact:** Contract to one (`exact`). Add more when needed.

### Q: "Item 3 changes the `SkillManifestExecutor` trait. How many implementations exist?"

**Answer:** Two production implementations (`BridgeManifestExecutor` in
`kask_bridge`, and the stub in tests), plus three callers (`SkillTool`,
`PipelineTool`, `SkillBundleTool`). The trait change propagates to all of
them.

**Impact:** The change is mechanical but touches the D-seam boundary.
The `.rules` say "Don't 'fix' upstream files speculatively — push fixes
into `kask/` behind a D-seam." The trait is in `crates/agent/src/tools/skill_tool.rs`
which is upstream Zed territory. The error enum should live in the agent
crate (it's a trait return type), but the classification logic lives in
`kask_bridge`. This is the standard D-seam pattern — no issue.

---

## Idiomatic-Rust Review

### `SkillExecutionError` design

The revised enum:

```rust
pub enum SkillExecutionError {
    CompileTime { skill_name: String, phase: &'static str, message: String },
    Runtime { skill_name: String, phase: &'static str, message: String },
}
```

**Rust idioms check:**

- `&'static str` for `phase` is correct — phases are compile-time constants
- `String` for `skill_name` and `message` is correct — they're dynamic
- No `#[derive]` needed beyond `Debug, Clone]` — this is an error type,
  not a value type
- Should implement `std::fmt::Display` for user-facing messages
- Should implement `std::error::Error` if it's ever used with `?` in
  non-agent contexts (currently the trait returns `Result<String, String>`,
  so the error is just converted to string at the boundary)

**Verdict:** The design is idiomatic. One concern: the `skill_name` field
is redundant — the caller already knows which skill it asked to execute.
But the error may propagate through channels where the skill name is
useful for logging. Keep it.

### `FeedbackDriftDetector` → contracted to `MetacognitionLoop::feedback_trend`

**Rust idioms check:**

- Adding a method to `MetacognitionLoop` is simpler than a new struct
- The method can borrow `&self.ledger` and query `SkillSpanStore` directly
- No new allocation, no new struct, no new module
- The trend computation is a simple rolling-window average — inline it

**Verdict:** The contraction is idiomatic. A method on the existing loop
is simpler than a new detector struct.

### Golden-output validation in `output_schema.rs`

**Rust idioms check:**

- Adding `validate_golden_outputs` to `output_schema.rs` is correct —
  it's the output validation module
- The function signature should be `fn validate_golden_outputs(
manifest: &BundleManifest, executor: &ManifestExecutor) ->
Result<GoldenOutputReport, GoldenOutputError>`
- But wait — running the skill requires the executor, which requires
  inference and tools. This can't be a pure function in
  `output_schema.rs`. It needs to be async and take executor
  dependencies.

**Revised approach:** The golden-output validation is a maintenance-time
check, not a runtime check. It should be a method on
`BridgeManifestExecutor` (or a standalone async function that takes
`&BridgeManifestExecutor`), not a pure function in `output_schema.rs`.
The manifest field (`golden_outputs`) goes in `bundle/manifest.rs`; the
validation logic goes in `kask_bridge/src/skill_executor.rs` (where the
executor is).

---

## Revised Plan

### Step 0: Wire `record_skill_span` emission (prerequisite)

**Problem:** `RegulationLedger::record_skill_span` is defined but never
called. `SkillSpanStore` is empty at runtime. All downstream feedback
loops are broken.

**Fix:** In `BridgeManifestExecutor::execute_skill`, after the cascade
completes (success or failure), call `record_skill_span` with the
outcome payload. This requires:

- Access to `RegulationLedger` from `BridgeManifestExecutor` (currently
  not held — need to add it as a field)
- An outcome payload struct (skill name, success/failure, duration,
  convergence result if available)

**Also fix:** Wire `result_feedback` to emit
`reg.skill.<id>.operator_feedback` spans. Currently `result_feedback`
records to in-process `learning` state only. It needs to also call
`record_skill_span` when the feedback is about a skill result (not just
a data provider result). This requires `result_feedback` to have access
to `RegulationLedger` — which means either:

- Passing the ledger to the companies MCP server (cross-server
  dependency), or
- Creating a separate skill-feedback MCP tool on the curator server
  that records the span, or
- Having `BridgeManifestExecutor` intercept skill-result feedback and
  record it

The cleanest approach: add a `curator_record_feedback` MCP tool on the
curator server that calls `record_skill_span`. The agent calls it after
a skill invocation when it has feedback. This avoids cross-server
dependencies and uses the existing curator MCP infrastructure.

**Estimated scope:** Medium. One new field on `BridgeManifestExecutor`,
one new MCP tool, emission calls at two points.

**Files:**

- `kask/crates/kask_bridge/src/skill_executor.rs` — add ledger field,
  emit outcome span
- `kask/mcp-servers/hkask-mcp-curator/src/` — new
  `curator_record_feedback` tool
- `kask/crates/hkask-regulation/src/runtime.rs` — no changes (already
  has `record_skill_span`)

### Step 1: Item 3 — Typed failure classification

**As revised above.** Replace `Result<String, String>` with
`Result<String, SkillExecutionError>` on `SkillManifestExecutor::execute_skill`.

**Estimated scope:** Small-medium. Mechanical propagation through the
trait, bridge, and three callers.

**Files:**

- `crates/agent/src/tools/skill_tool.rs` — error enum, trait change
- `kask/crates/kask_bridge/src/skill_executor.rs` — classify at failure
  points (lines 594, 597-645, 680)
- `crates/agent/src/tools/pipeline_tool.rs` — update
- `crates/agent/src/tools/skill_bundle_tool.rs` — update
- `crates/agent/src/tools/skill_tool.rs` tests — update stubs

### Step 2: Item 2 — Drift detection (revised)

**As revised above.** Add `feedback_trend` method to
`MetacognitionLoop` that queries `SkillSpanStore` and computes rolling
acceptance rate. Alert via existing channel when declining.

**Depends on:** Step 0 (emission must be wired first).

**Estimated scope:** Small. One method on `MetacognitionLoop`, one
alert condition in `sense()`, configuration in `MetacognitionConfig`.

**Files:**

- `kask/crates/hkask-regulation/src/metacognition.rs` — add
  `feedback_trend` method, wire into `sense()`
- `kask/crates/hkask-regulation/src/metacognition.rs` — add
  `feedback_drift_threshold` to `MetacognitionConfig`

### Step 3: Item 1 — Golden-output validation (revised)

**As revised above.** Add `golden_outputs: Option<Vec<(String, String)>>`
to `BundleManifest`. Add `validate_golden_outputs` method to
`BridgeManifestExecutor`. Wire into `skill-maintenance`.

**Depends on:** Step 0 (for trend data from golden-output pass/fail
results).

**Estimated scope:** Small. One manifest field, one async method, one
skill-maintenance wiring.

**Files:**

- `kask/crates/hkask-templates/src/bundle/manifest.rs` — add field
- `kask/crates/kask_bridge/src/skill_executor.rs` — add
  `validate_golden_outputs` method
- `.agents/skills/skill-maintenance/` — add golden validation step

---

## What Was Rejected

| Proposal                                                                         | Reason                                                                                              |
| -------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `GoldenOutputFixture` struct with 4 fields                                       | Contracted to `(String, String)` tuple — speculative generality                                     |
| 4 comparison strategies (`exact`, `json_subset`, `regex_match`, `mermaid_valid`) | Contracted to 1 (`exact`) — no existing skill needs them                                            |
| `golden_validation.rs` new module                                                | Moved to `BridgeManifestExecutor` method — no new module needed                                     |
| `FeedbackDriftDetector` struct + `drift_detector.rs` module                      | Contracted to `MetacognitionLoop::feedback_trend` method — no new struct/module                     |
| `partial_output: Option<String>` on `Runtime` error                              | Dropped — degraded success should return `Ok` with warning, not `Err` with partial data             |
| `drift_threshold` in `settings_content.rs`                                       | Moved to `MetacognitionConfig` — regulation config lives with the loop, not in user-facing settings |

## What Was Accepted (with revisions)

| Proposal                           | Revision                                                                                             |
| ---------------------------------- | ---------------------------------------------------------------------------------------------------- |
| Item 3: `SkillExecutionError` enum | Accepted with `phase: &'static str` and no `partial_output`                                          |
| Item 2: drift detection            | Accepted as `MetacognitionLoop::feedback_trend` method, not new struct                               |
| Item 1: golden-output validation   | Accepted as `(String, String)` pairs, single comparison strategy, method on `BridgeManifestExecutor` |

## What Was Added (not in original plan)

| Addition                                  | Reason                                                                                       |
| ----------------------------------------- | -------------------------------------------------------------------------------------------- |
| Step 0: Wire `record_skill_span` emission | Prerequisite — the store is empty, all feedback loops are broken                             |
| `curator_record_feedback` MCP tool        | Clean path for operator feedback to reach `SkillSpanStore` without cross-server dependencies |

## Revised Priority

```
Step 0: Wire record_skill_span emission (prerequisite for 1 and 2)
Step 1: Item 3 — typed failure classification (independent, highest value)
Step 2: Item 2 — drift detection (depends on step 0)
Step 3: Item 1 — golden-output validation (depends on step 0, narrowest scope)
```

## Verification Status

All claims in this review were verified against the codebase:

- `record_skill_span` never called: verified via grep — 1 hit (definition)
- `result_feedback` doesn't emit spans: verified by reading
  `valuation.rs:1157-1230` — records to `self.learning` only
- `SkillFeedbackSpan::Convergence` exists but isn't emitted: verified via
  grep — only test references
- `execute_skill` returns `Result<String, String>`: verified at
  `skill_executor.rs:590`
- Error points are at lines 594, 597-645, 680: verified by reading
  `execute_skill` body
- `BundleManifest` has `output_schema`: verified at `manifest.rs:88`
- `ConvergenceTracker` is in `hkask-templates`, `RegulationLedger` is in
  `hkask-regulation`: verified via module paths
