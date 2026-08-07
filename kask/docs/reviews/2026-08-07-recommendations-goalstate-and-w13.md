# Recommendations: GoalState Deletion (Item 2) and W-13 ADR Moves (Item 3)

**Date:** 2026-08-07
**Context:** Follow-up to the 2026-08-07 kask code review and self-review

---

## Item 2: `GoalState` deletion recommendation

### Evidence

`GoalState` (`kask/crates/hkask-types/src/goal.rs`, 149 lines) is a public enum with 5 variants (Pending/Active/Completed/Blocked/Abandoned), `as_str`/`parse_str`/`is_terminal`/`can_transition_to` methods, `FromSql`/`ToSql` impls in `sql_impls.rs`, and 6 unit tests.

**Consumer audit (code only, excluding docs/READMEs):**
- Zero production code consumers. The only references are in doc comments (`hkask-regulation/src/types/loops/channels.rs:44,46,54` — describing `GoalState` as a contrast to the local `GoalLifecycle` enum).
- Zero test consumers outside `goal.rs`'s own `#[cfg(test)] mod tests`.
- The `hkask-goal` crate was deleted; `Goal`/`GoalArtifact`/`GoalCriterion` types were removed. `GoalState` was retained "for rusqlite FromSql/ToSql orphan rule" but the impls are themselves dead (no code reads/writes `GoalState` to/from SQL).

### Recommendation: **Delete `GoalState`**

**Rationale:**
1. **Essentialist G1 (deletion test):** Deleting `GoalState` causes zero behavior loss — no code uses it. The 149 lines of enum + methods + SQL impls + tests are dead.
2. **Pragmatic-semantics:** The doc comment claiming it's retained "for rusqlite FromSql/ToSql orphan rule" is stale — the impls exist but are never exercised. This is an advertised invariant (orphan rule compliance) without an enforcement point (no SQL read/write).
3. **`.rules` "Convention helpers with only test callers are dead code":** `GoalState`'s only callers are its own tests. The type is dead surface area.
4. **Public API concern is mitigated:** `GoalState` is in `hkask-types` (a kask-internal crate, not published to crates.io). Downstream forks would see the deletion in a git diff and can restore it if needed.

**Files to touch:**
- Delete `kask/crates/hkask-types/src/goal.rs`
- Remove `pub mod goal;` and `pub use goal::GoalState;` from `kask/crates/hkask-types/src/hkask_types.rs`
- Remove `use crate::goal::GoalState;` and the `FromSql`/`ToSql` impls from `kask/crates/hkask-types/src/sql_impls.rs` (L21, L101-125)
- Update doc comments in `kask/crates/hkask-regulation/src/types/loops/channels.rs:44,46,54` to remove `GoalState` references
- Update `kask/crates/hkask-types/README.md` and `kask/crates/hkask-services-core/README.md` to remove `GoalState` references

**Risk:** Low — zero code consumers. One commit. Validation: `cargo check -p hkask-types --all-targets` + `./script/clippy`.

**Rollback:** Trivial — `git revert` restores the file and references.

---

## Item 3: W-13 ADR moves recommendation

### W-13a: `regulation` types → `hkask-regulation`

**Evidence:**
- `kask/crates/hkask-types/src/regulation.rs` (549 lines) — `RegulationSpan`, `ToolSubsystem`, `LedgerHealth`, `RegulationHealth`, `QueueDepth`.
- No internal `hkask-types` dependencies (`use crate::` — none in production code).
- One internal `hkask-types` test reference: `observable_span.rs:174` — `use crate::regulation::RegulationSpan` in a `#[test]` verifying `RegulationSpan` implements `ObservableSpan`.
- External consumers: `hkask-mcp-curator` (3 files), `hkask-memory` (2 files), `hkask-regulation` itself (6 files).

**Blocker: `hkask-memory` is out of scope (memory refactor).** Moving `regulation` to `hkask-regulation` would force `hkask-memory` to add a `hkask-regulation` dependency. `hkask-memory` is in the memory-refactor flight and should not be touched.

**Recommendation: Defer until the memory refactor lands.** After the memory refactor is complete, re-evaluate:
1. Move `regulation.rs` to `hkask-regulation/src/types/regulation.rs`
2. Move the `reg_span_implements_observable_span` test to `hkask-regulation` (it can use `hkask_types::ObservableSpan` + `hkask_regulation::RegulationSpan`)
3. Add `hkask-regulation` dep to `hkask-mcp-curator` (new edge — acceptable, curator is regulation-heavy)
4. Add `hkask-regulation` dep to `hkask-memory` (if still needed after memory refactor)
5. Remove `pub mod regulation` and root re-exports from `hkask-types`

**Risk:** Medium — new dependency edges, test move, touches the memory refactor boundary. Wait for the memory refactor to settle.

### W-13b: `tool_taint` → `hkask-capability`

**Evidence:**
- `kask/crates/hkask-types/src/tool_taint.rs` (120 lines) — `ToolTaint` enum (Source/Sink/Pure/Endorser), `can_flow_to` method.
- No internal `hkask-types` dependencies (`use crate::` — none).
- No internal `hkask-types` consumers (no other `hkask-types` module references `tool_taint`).
- External consumers:
  - `hkask-regulation/src/runtime_policy.rs` — would need `hkask-capability` dep (currently doesn't have it)
  - `hkask-mcp/src/runtime.rs` — already depends on `hkask-capability` ✓
  - `hkask-templates/src/executor.rs` — already depends on `hkask-capability` ✓
  - `hkask-capability/src/tool_port.rs` — already in `hkask-capability`'s domain ✓

**Recommendation: Move `tool_taint` to `hkask-capability` now.** This is feasible and adds only one new edge (`hkask-regulation` → `hkask-capability`). The move is clean:
1. Copy `tool_taint.rs` to `hkask-capability/src/tool_taint.rs`
2. Add `pub mod tool_taint;` to `hkask-capability` lib root
3. Update `hkask-regulation/src/runtime_policy.rs` to use `hkask_capability::tool_taint::ToolTaint` (and add `hkask-capability` dep to `hkask-regulation/Cargo.toml`)
4. Update `hkask-mcp/src/runtime.rs` and `hkask-templates/src/executor.rs` to use `hkask_capability::tool_taint::ToolTaint` (they already depend on `hkask-capability`)
5. Remove `pub mod tool_taint;` and `pub use tool_taint::ToolTaint;` from `hkask-types`
6. Delete `kask/crates/hkask-types/src/tool_taint.rs`

**Risk:** Low — one new dependency edge (`hkask-regulation` → `hkask-capability`), no internal `hkask-types` blockers, no memory-refactor involvement. One commit. Validation: `cargo check -p hkask-types -p hkask-capability -p hkask-regulation -p hkask-mcp -p hkask-templates --all-targets` + `./script/clippy`.

**Rollback:** Trivial — `git revert`.

---

## Summary

| Item | Recommendation | Risk | When |
|------|---------------|------|------|
| 2: `GoalState` deletion | **Delete now** — zero consumers, dead code | Low | Now |
| 3a: `regulation` → `hkask-regulation` | **Defer** — blocked by memory refactor (out of scope) | Medium | After memory refactor lands |
| 3b: `tool_taint` → `hkask-capability` | **Move now** — clean, one new edge, no blockers | Low | Now |
