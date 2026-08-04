# Pre-Release Final Summary

Date: 2026-08-04. Scope: zed-kask workspace, 13 commits ahead of `origin/main` (+8415/-4582, 107 files).
Phases: A (security skills ×4), B (code review of full diff), C (fresh refactor survey). Reports: `phase-a-security-review.md`, `phase-b-code-review.md`, `phase-c-refactor-survey.md` in this directory.

## Release-readiness verdict: READY WITH ONE RECOMMENDED FIX

No blockers. One Medium security finding (F1) should be fixed before release if the fix is quick; it is a latent gap in a prior-pass fix, not a regression, and the pre-hardening code had the same exposure.

## What this pass verified

- All 10 prior-pass fixes hold (redact_spans sort, GuardedStream cap, IPC timeout/guard-nulling, memory boundaries, atomic debit, error mappers, typed errors, tool_schema move, ProvisionError).
- Layer 4 OCAP gate traced end-to-end: fail-closed on both the match path and the governance-absent path. All 8 defense layers present.
- Diff introduces no regressions and no `.rules` violations (no `unwrap_or(0)` on signals, no `let _ =` on fallible ops, no tokio-in-`background_spawn`, no `AsyncApp` in `Send+Sync`).
- Prior refactors are net-positive: new modules pass the deletion test, interfaces ≤7 public items, dependency direction clean, 50-tool surface pinned by test.

## Open findings (ranked)

| # | Severity | Finding | Recommended action |
|---|----------|---------|---------------------|
| F1 | Medium | `redact_spans` skips overlapping spans → secret-suffix leak (`hkask-guard/src/pipeline.rs:409-417`); warn also on non-canonical `hkask.guard.redact` target | Merge-on-overlap (`cursor = max(cursor, end)`); move warn to `reg.guard.*`. Confirm whether pinned llm-guard emits substring-pattern pairs. **Fix before release if < 1 day.** |
| F2 | Low | `out_of_order_secrets_all_redacted` may not exercise out-of-order matches | Add direct unit test of `redact_spans` with constructed Match vecs (also pins F1 fix) |
| — | Medium | RR-0044 not promotable: 54 production blanket `internal(format!` sites remain; clear mis-classifications in media (`processing.rs:198,724`) and training (`submit.rs:146,285`) | Keep `status: pending`; fix the 4 mis-classified sites; consider refining the grep pattern to exclude genuine-internal classes |
| — | Low | `deny.toml` stale vs current lockfile (12 unlisted advisories, 3 non-matching ignores — all transitive/upstream) | Refresh `kask/deny.toml`; consider scoping kask vs upstream |
| — | Low | 3 orphaned `log` deps (scenarios/portfolio/kanban widgets) | Remove |
| — | Low | runtime-posture-monitor manifest: `signal_sources`, `signal`, `convergence_metric` referenced but undeclared/unwired; skill runnable but degraded (no span-query MCP server) | Declare inputs or add `input_mapping`; accept degraded-mode documentation |
| — | Low | Full WebID in provision log (`identity.rs:259`) | Use `redacted_display()` |
| — | Low | No `check-unsafe-forbid.sh`; several crate roots lack `forbid(unsafe_code)` | Add script or cargo-test RR entry + missing attributes |
| M1/M2 | Medium (arch) | `swarm_panel.rs` 4,720-line remainder; `map_join_error` duplicated across research/companies | Post-release refactor candidates |

## Deferred items resolved

- **RR-0044 promotion**: do NOT promote — production sites remain (see above).
- **`hkask-test-harness` `Result<_, String>` (4 sites)**: acceptable as-is; test-oracle API, changing it would break all oracle consumers for no runtime benefit.
- **Release version/changelog scope**: still needs user confirmation.

## Suggested .rules additions

1. **Redaction passes must merge overlapping spans, not skip them.** A sorted forward-pass redactor that skips `start < cursor` spans emits the skipped span's uncovered tail verbatim — a silent partial-secret leak. Merge (`cursor = max(cursor, end)`) or reject the whole output. Applies to any span-based sanitizer, not just `redact_spans`. (Non-obvious: the prior-pass review fixed the out-of-order sibling and missed this; specific: one-line code pattern.)

2. **Diagnostic/warn span targets must use canonical `reg.*` namespaces.** A `tracing::warn!` on a non-canonical target (`hkask.guard.redact`) is invisible to every runtime-posture consumer filtering `reg.*` — the failure signal exists but no monitor can see it. Same class as "hooks need a startup-failure signal": a signal on a channel nobody reads is not a signal.

## Cross-cutting critic notes

- Metacognition: predicted the prior pass left 1–2 latent security gaps and that the refactors were sound — borne out (F1 + RR-0044 residual; Phase C net-positive). Brier ~0.1.
- Essentialist: F1/F2/RR-0044/media+training mis-classifications survive all 3 gates; several candidate nits (e.g. renaming `lifecycle_tools.rs` pre-release) were dropped as non-essential.
- Pragmatic-semantics: F1 is Inference-tier on exploitability (depends on unverified llm-guard pattern registry — flagged as fragile, confidence ~0.5 on substring-pair existence); the code-path mechanics are IS-tier verified.
- Pragmatic-cybernetics: the F1 warn-on-wrong-target issue is a broken feedback loop (signal emitted, no consumer) — captured in .rules suggestion #2.
