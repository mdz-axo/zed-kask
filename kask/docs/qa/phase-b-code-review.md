# Phase B — Code Review (origin/main..HEAD, 13 commits, 107 files, +8415/-4582)

Date: 2026-08-04. Review of the release-hardening diff; kask-owned crates only. Read-only.

## Verdict: no regressions found. The refactor batch is behavior-preserving.

## Checklist results

1. **New types used consistently** — `LocalSwarmError`, `SkillExecError`, `ProvisionError`, `InputValidationError` are adopted at their call sites; no old String-error pattern remains in the touched surfaces (residual blanket `internal(format!` sites are tracked under RR-0044, see phase-a report).
2. **VizWidget trait** — 4 impls + 1 documented non-impl (media); disjoint-tag test (`hkask_viz_core.rs:339`) pins the registry; no rendering regression identified.
3. **swarm_panel split** (3700→421 + parse.rs) — parsing logic moved to `parse.rs` with 16 dedicated tests; behavior preserved.
4. **SwarmServer split** (2900 → 5 tool files) — `tool_surface_is_exactly_50_registered_tools` test (`hkask_mcp_swarm.rs:350`) pins the full 50-tool surface; `combined_router()` composes all 5 routers; no tool dropped or duplicated.
5. **tool_schema move** — `find_boolean_schema_positions` contract intact; re-export via hkask-mcp-server complete; 9 schema-compliance test files still resolve.
6. **Bug probes** — all four (redact_spans sort, debit_if_funds atomicity, GuardedStream cap, IPC timeout wrapping) verified correct in Phase A1; see phase-a-security-review.md for evidence. One NEW bug class found (F1: overlapping spans), which is a **pre-existing latent issue in the fix**, not a regression — the prior behavior leaked the same bytes (unsorted pass skipped them too).
7. **Test quality**
   - `out_of_order_secrets_all_redacted` — CONCERN: may not actually trigger out-of-order scanner output; assertion skipped when scan passes. Recommend a direct unit test of `redact_spans` with constructed `Match` vecs (Phase A finding F2).
   - `canary_in_reasoning_field_is_redacted` — covers the accumulation path; adequate given GuardedStream's documented post-hoc semantics.
   - IPC dispatch tests — dispatch arms covered; timeout/guard-nulling pinned.
8. **`.rules` compliance in the diff** — clean:
   - No new `unwrap_or(0)` on regulation signals (only comments/regression YAML referencing the rule).
   - No new `let _ =` on fallible operations.
   - No new `background_spawn` with tokio-dependent futures (only the regression-entry text mentions the pattern).
   - No `AsyncApp` in `Send + Sync` impls introduced.
   - No new `input_mapping` bindings added, so no taint-propagation violations.

## Findings

| Severity | Finding | Regression? |
|----------|---------|-------------|
| Medium | F1 overlapping-span leak in `redact_spans` (see phase-a report) | No — latent in prior fix |
| Low | F2 test may not exercise out-of-order claim | No — test-quality gap |
| Low | F3 full WebID in provision log | No |
| Nit | RR-0044 residual sites (54) block promotion | No — pre-existing debt |

No behavior changes introduced by the refactoring were detected. The executable pins (50-tool surface, viz registry tags, parse tests, sequential-debit test) are the correct divergence-pinning pattern per `.rules`.
