# Suggested .rules additions (for the PR description)

> Per `.rules` hygiene: do NOT edit `.rules` inline during normal work. The
> following is a proposed replacement for the stale `.rules` trap at L673-678
> (and its mirror in `GEMINI.md` L673-678, which is root-level and outside the
> `kask/` seam — not edited in-session). Paste this into the PR description as a
> "Suggested .rules additions" entry for reviewer approval.

## Replace

The existing `.rules` trap "## `input_mapping` bindings must propagate taint
before `context.insert`" (L673-678) references the removed function
`propagate_taint_for_binding` (`executor.rs:282`). A whole-project grep finds
zero matches for that symbol in any `.rs` file — the executor was refactored
into `step_actions.rs` / `step_context.rs` / `step_machine.rs`, and taint is
now a field on `StepResult` (`step_context.rs:40`). The trap as written
misdirects agents to grep for a phantom enforcement point (verified by the
2026-08-11 seam audit, finding KS-03).

## With

```markdown
## `input_mapping` taint propagation — gate is present but NOT YET ENFORCED

Taint is carried as a field on `StepResult.taint` (`kask/crates/hkask-templates/src/step_context.rs:40`),
written by `StepContext::store_result`/`store_named` and read via
`StepContext::taint_of` (`step_context.rs:135`). The legacy
`propagate_taint_for_binding` function was removed when `executor.rs` was
refactored into `step_actions.rs`/`step_context.rs`/`step_machine.rs` — do not
grep for it; if an audit tells you to, the audit template is stale (see
`kask/registry/templates/kali-audit/select-surface.j2`, updated 2026-08-11).

The FIDES Source→Sink gate is **structurally present but operationally inert**
(two gaps, audit findings KS-01/KS-02 in `kask/research/seam-audit/security-review.md`):
1. `check_untrusted_input` (`step_actions.rs:705`) reads taint from legacy
   `__taint__{key}` map markers, but the write side no longer emits those
   markers — `has_untrusted_input` is always `false`.
2. `McpRuntime::get_tool_info` hardcodes `ToolTaint::Pure` for every MCP tool
   (`kask/crates/hkask-mcp/src/runtime.rs:370`), so the `Sink` arm of
   `DefaultPolicy::check` (`kask/crates/hkask-regulation/src/runtime_policy.rs:71`,
   Rule 2 at `:86`) never matches.

The lattice itself is live: `ToolTaint::can_flow_to`
(`kask/crates/hkask-capability/src/tool_taint.rs:34`, matrix pinned by
`can_flow_to_matrix` test at `:57`). Until KS-01 (bridge the read path to
`StepResult.taint`) and KS-02 (per-tool taint labels) land, the Source→Sink
block never fires — treat the FIDES layer as defense-in-depth degradation, not
an enforced membrane. The primary security membrane is the OCAP capability
match + gas gate in `McpRuntime::invoke` (`runtime.rs:426`).
```

## Note for reviewers

- `GEMINI.md` mirrors `.rules` and carries the same stale trap at L673-678.
  `GEMINI.md` is root-level (outside `kask/` and not a listed D-seam), so it was
  not edited in-session per the seam discipline. If this `.rules` update is
  accepted, apply the same change to `GEMINI.md` in the same PR.
- The `kask/registry/templates/kali-audit/select-surface.j2` FIDES-taint check
  was already corrected (2026-08-11) to point at the live `StepResult.taint` /
  `check_untrusted_input` mechanism and warn against grepping for the removed
  function.
- `kask/crates/hkask-templates/src/step_context.rs:16` carries a `//!` module
  doc comment describing the OLD `propagate_taint_for_binding` design. It is a
  historical comment, not an enforcement-point claim; left as-is (low value to
  rewrite a source comment that is clearly historical).