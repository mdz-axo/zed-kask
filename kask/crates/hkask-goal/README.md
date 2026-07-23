# hkask-goal — Goal Coordination Types

Minimal coordination substrate for multi-agent collaboration. Goals are scoped by `&WebID`; multiple loops interact with them (Curation evaluates, Cybernetics allocates energy, Communication coordinates). Extracted from `hkask-services-core` (see `tasks/plan-core-scope-contraction.md`, Task 2.1).

**Version:** v0.31.0 | **Crate:** `hkask-goal`

## Exports

| Type | Purpose |
|------|---------|
| `Goal` | Coordination substrate (id, webid, text, state, visibility, depth, parent, display_name) — `new`, `with_display_name`, `with_parent`, `transition`, `can_have_subgoals` |
| `GoalCriterion` | Completion condition (LLM-judged) — `new`, `mark_satisfied` |
| `GoalArtifact` | Output produced while working toward a goal — `new` |
| `GoalID` / `GoalState` | Re-exported from `hkask-types` (orphan rule: SQL impls live there) |

`IllegalGoalTransition` is `pub(crate)` — returned by `Goal::transition`, not consumed across crate boundaries.

## Nesting

Max goal nesting depth is 7 (mirrors `hkask-capability::SYSTEM_MAX_RECURSION`, hardcoded to avoid a cross-crate dependency).

## Dependencies

- `hkask-types` — `GoalID`, `GoalState`, `WebID`, `Visibility`
- `chrono` — `DateTime<Utc>` timestamps
- `serde` — (de)serialization
- `uuid` — criterion/artifact IDs
- No coupling back to `hkask-services-core`