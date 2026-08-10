# Pre-Release Hardening & Polish Pass — Continuation Prompt

## Context

The zed-kask workspace at `/home/mdz-axolotl/Clones/zed-kask` has undergone a comprehensive release-hardening pass (13 commits ahead of `origin/main`, 107 files changed, +8415/-4582 lines). The prior session completed:

- **Phase 1 (CI/build hygiene):** All 9 CI jobs green, 1386+ kask tests pass, clippy clean, formatting clean
- **Phase 2 (Refactor survey + implementation):** 5 High refactors (H1–H5), 8 Medium refactors (M1–M8), 9 Low fixes (L1–L9) — all implemented
- **Phase 3 (Skill maintenance):** 4 security skills audited, 3 fixed (adversarial-red-team, supply-chain-sentinel, kali-audit, runtime-posture-monitor)
- **Phase 4 (Security audit):** 0 blockers, 2 High (both fixed), 5 Medium (all fixed), 3 Low (all known/accepted). 8/8 defense layers present. 7 regression entries added (RR-0040–RR-0046)
- **Bug hunt (9 findings):** 1 CRITICAL (secret leak in `redact_spans` — fixed), 3 HIGH (swarm slug panic, IPC guard-nulling, IPC timeout — all fixed), 5 MEDIUM (TOCTOU, prompt injection via memory, dead scoring code, unbounded accumulation, missing GuardedStream tests — all fixed)

The full commit history is:
```
e1d2cc014e Extract shared panel button and viz widget logic
fdf72aad4d Extract swarm panel parsing and improve error types
fe74a48180 Classify MCP tool errors per domain variant
7771803e3e Replace String errors with typed enums
21bdacc572 Move tool_schema to hkask-types
ebf0c49cbd Refactor memory injection and add data boundaries
72a25fb628 Add atomic debit and scored media dispatch
1aded9a05c Fix out-of-order secret redaction and guard stream leaks
34cf063d5b Add OCAP and anti-pattern regression checks
9cc2d33e06 Update runtime-posture-monitor skill and misc kask fixes
0a5372aaf3 Add source field to SwarmCard and audit reports
fc8106c165 Bump version to 0.31.2
7905c4823c Update README.md
```

Baseline: `59f0834c96 (origin/main)` — all work is on `main` ahead of origin.

---

## Goal

Perform a rigorous final security review and code-quality polish pass before releasing an update. This is the last gate — assume the prior pass found and fixed the obvious issues; your job is to find what was missed.

## Scope (anchored)

- **"kask codebase"** = every crate under `kask/` and every kask-owned crate at the repo root (`kask_bridge`, `hkask-*`, `swarm_panel`, `crates/hkask-*-widget`, `crates/hkask-viz-core`, `crates/kask_extensions_ui`, `crates/marketplace_ui_common`).
- **"the seam"** = the D1–D23 divergence seams in `DIVERGENCE.md` plus any `// zed-kask:` comments in upstream crates.
- **Out of scope:** upstream Zed crates outside the D-seams. Do NOT modify, rename, or "fix" upstream files speculatively — see the "Tests must pin deliberate zed-kask deviations" trap in `.rules`.

## Backward-compatibility constraint

No additional backward-compatibility constraints within kask-owned crates: you may rename, restructure, or delete kask-owned code. The D-seam boundary and the "do not touch upstream" rule apply in full.

---

## Phase A — Security skills (run all four)

### A1: `kali-audit` — full security review

Run `kali-audit` against ALL kask-owned crates. The prior session's Phase 4 found and fixed 2 High + 5 Medium findings. This pass should verify those fixes hold and find anything new.

Specific areas to probe (these were changed in the prior session and need re-verification):
1. **`redact_spans` fix** (`hkask-guard/src/pipeline.rs`) — verify the sort-by-`span.start` fix handles ALL edge cases: overlapping spans, zero-length spans, empty text, Unicode boundaries. Check if the regression test (`out_of_order_secrets_all_redacted`) actually exercises the out-of-order scenario or if the scanner happens to emit in-order for the test input.
2. **GuardedStream accumulation cap** (`hkask-guard/src/guarded_inference.rs`) — verify the 256KB cap is checked correctly (after both `text_delta` and `reasoning_delta` accumulation, not just one). Check if the error chunk is well-formed and the `scanned` flag is set correctly to prevent re-scanning.
3. **IPC timeout** (`hkask-inference/src/inference_ipc_client.rs`) — verify the 120s timeout wraps the entire `read_line` call, not just part of it. Check if the timeout error correctly nulls the cached stream (via the guard-nulling fix).
4. **Memory recall data-boundary wrapping** (`kask_bridge/src/context_injector.rs`) — verify the boundary markers are actually effective (the model is told to treat content as data). Check if the `format_recall_context` helper is used consistently in both injectors.
5. **`Ledger::debit_if_funds`** (`hkask-ledger/src/hkask_ledger.rs`) — verify the `BEGIN IMMEDIATE` transaction actually closes the TOCTOU window. Check if the balance re-check is inside the transaction, not before it.
6. **`LocalSwarmError` + `map_local_swarm_error`** (`hkask-mcp-swarm/src/error.rs`) — verify the error classification is correct: `NotFound` → `not_found`, `InvalidInput` → `invalid_argument`, etc. Check for any remaining `McpToolError::internal(format!` call sites in the swarm server that bypass the mapper.
7. **`SkillExecError` type** (`hkask-types/src/ports/inference_port.rs`) — verify the `From<String>` conversion doesn't lose information. Check if the D1-seam-constrained `BridgeManifestExecutor` still propagates errors correctly.
8. **Per-variant error mappers** across 5 MCP servers — verify the mappers classify correctly. For corpus, check that `map_corpus_io_error` is used at ALL file I/O sites (the prior pass found 22 sites; verify none were missed).
9. **`tool_schema.rs` extraction** (`hkask-types/src/tool_schema.rs`) — verify the move didn't break the `AnyJsonValue` / `find_boolean_schema_positions` contract. Check that `hkask-mcp-server`'s re-export is complete and all MCP server imports resolve.
10. **`ProvisionError` type** (`kask_bridge/src/identity.rs`) — verify the typed error preserves all error information that the prior `String` errors carried.

Additional probes (not covered in prior session):
11. **SQL injection** — grep all kask crates for string-interpolated SQL (not parameterized `?N` queries). The prior session checked `consent.rs` and found it clean; check ALL other SQL sites (`local_swarms.rs`, `local_registry.rs`, `local_knowledge.rs`, `hkask-storage`, `hkask-ledger`).
12. **Path traversal** — verify all file-path inputs from MCP tool arguments are contained (not just the swarm paths checked before). Check `hkask-mcp-corpus` file tools, `hkask-mcp-research` file tools, and `hkask-mcp-curator` file tools.
13. **Secret handling in logs** — grep for `tracing::` / `log::` calls that might log sensitive data (API keys, passphrases, DB paths, WebIDs). Check if any `tracing::info!` / `tracing::debug!` includes raw credential values.
14. **`unsafe` audit** — verify the `check-unsafe-forbid.sh` script covers ALL kask crate roots (31 currently). Check if any new crates were added without the attribute. Read the `unsafe` blocks in `hkask-storage` and `hkask-mcp-codegraph` to verify the FFI is sound.
15. **Defense layer coverage** — re-verify all 8 layers are present and enforced. Check Layer 4 (OCAP) — verify `McpRuntime::invoke` actually rejects unmatched tokens (not just that the code exists — trace the control flow).

### A2: `supply-chain-sentinel` — dependency audit

Run a full dependency audit. The prior session found the supply chain clean. Verify:
1. No new deps were added without workspace pinning (check `Cargo.lock` for non-workspace deps in kask crates)
2. `deny.toml` is still valid (no new advisory ignores needed)
3. The `schemars` dep removed from `hkask-mcp-server` didn't break any transitive resolution
4. The `thiserror` dep added to `kask_bridge` is used (not an orphan)
5. Check for any `cargo-machete` findings across all kask crates (not just the ones checked before)

### A3: `runtime-posture-monitor` — runtime posture (gap check)

The prior session fixed the skill's deleted data source (`hkask-mcp-regulation` → documented fallback) and corrected `reg.regulation` → `reg.outcome`. Verify:
1. The SKILL.md no longer references `hkask-mcp-regulation` or `reg.regulation`
2. The templates no longer reference `hkask.*` performative spans
3. The manifest `inputs` section is aligned with template contracts
4. Document whether the skill can actually run (no MCP server exposes span querying — is the documented fallback adequate?)

### A4: `adversarial-red-team` — adversarial assessment

Run `adversarial-red-team` against the deployed defense stack. The prior session fixed the Layer 4 OCAP description (removed "ed25519") and the Layer 7 GuardedStream caveat. This pass should:
1. Verify the defense-layer descriptions in the templates are now accurate
2. If a live target is available, generate adversarial inputs across all 7 attack categories
3. If no live target, document the gap and assess the defense layers statically
4. Probe the `redact_spans` fix with adversarial inputs (can an attacker craft input that causes out-of-order secret matches to still leak?)

---

## Phase B — Code review (`code-review` skill)

Run the `code-review` skill against the diff from `origin/main` to `HEAD` (13 commits, 107 files). The review should:

1. **Scope the diff** — `git diff origin/main..HEAD` — 107 files, +8415/-4582 lines
2. **Focus on the changed files** — the prior session made large structural changes (split files, new types, error handling rewrites). The review should verify:
   - No behavior was accidentally changed during refactoring
   - All new types (`LocalSwarmError`, `SkillExecError`, `ProvisionError`, `InputValidationError`) are used consistently
   - The `VizWidget` trait doesn't introduce a regression in widget rendering
   - The `swarm_panel.rs` split (3700→421 lines + `parse.rs`) preserves all behavior
   - The `SwarmServer` split (2900 lines → 5 tool files) preserves the 50-tool surface
   - The `tool_schema.rs` move doesn't break the `schemars` boolean-schema-position check
3. **Check for introduced bugs** — large refactors can introduce subtle bugs:
   - Off-by-one in the `redact_spans` sort (does it handle empty matches?)
   - Race condition in the `debit_if_funds` atomic check (is `BEGIN IMMEDIATE` sufficient?)
   - Memory leak in the GuardedStream accumulation cap (is the cap checked correctly?)
   - Timeout correctness in the IPC client (does `tokio::time::timeout` wrap the right future?)
4. **Test coverage** — verify the new tests actually test what they claim:
   - `out_of_order_secrets_all_redacted` — does it actually trigger out-of-order matches?
   - `canary_in_reasoning_field_is_redacted` — does it test the streaming path or just `guard_output`?
   - IPC dispatch tests — do they cover all dispatch arms or just a subset?
5. **`.rules` compliance** — verify the changes don't violate any `.rules` traps:
   - No `unwrap_or(0)` on regulation signals (except the documented `tool_stats.rs` case)
   - No `let _ =` on fallible operations
   - No `background_spawn` with tokio-dependent futures
   - No `AsyncApp` in `Send + Sync` trait impls
   - All `input_mapping` bindings call `propagate_taint_for_binding`

---

## Phase C — Refactor-architecture survey (`refactor-architecture` skill)

Run a fresh `refactor-architecture` survey on the post-refactor codebase. The prior session implemented all findings from the first survey; this pass should find what the refactoring itself introduced:

1. **New shallow modules** — did the file splits (swarm_panel, SwarmServer, viz-core) create new shallow modules? Apply the deletion test to each new file.
2. **New duplication** — did the error mapper additions (H2) create new duplication across servers? Are the `map_*_error` functions following the same pattern, or did each server diverge?
3. **Dependency direction** — verify the `tool_schema.rs` move (H4) didn't create a circular dependency or violate the §13.1 invariant
4. **Interface width** — check if the new types (`LocalSwarmError`, `SkillExecError`, `ProvisionError`, `InputValidationError`, `VizWidget`) have minimal interfaces (≤7 public items per Ousterhout)
5. **Strangler-fig candidates** — any new cross-surface duplication that would warrant extraction?
6. **The `SwarmServer` split** — verify the 5 tool files are cohesive (each handles one concern) and that `combined_router()` composes them correctly. Check if any tool method is in the wrong file.
7. **The `VizWidget` trait** — verify it's justified (5 impls) and not speculative generality. Check if the registry pattern is sound.
8. **The `PanelToggleButton` extraction** — verify it reduced duplication without over-abstracting. Check if the generic-over-action design is appropriate.

---

## Cross-cutting critics (apply in every phase)

- **`metacognition`**: before each phase, record a prediction of what it will surface; after, score with Brier.
- **`essentialist`**: run the 3-gate (Exist → Surface → Contract) on every proposed finding before it enters a report.
- **`grill-me`**: at the end of each phase, run one Recall+Mechanism self-challenge on the phase's claims.
- **`pragmatic-semantics`**: classify every finding by IS/OUGHT, epistemic mode, constraint force, and provenance; flag Inference-tier claims (confidence ≤ 0.3) as fragile.
- **`pragmatic-cybernetics`**: for any feedback loop the phase touches, check polarity / delay / gain / closure / fidelity; surface broken loops.

---

## Failure modes

- If a security skill crashes on the current crate layout, treat the crash itself as a finding.
- If the `code-review` finds a behavior change introduced by the refactoring, flag it as a **regression** — the refactoring was supposed to be behavior-preserving.
- If the `refactor-architecture` survey finds that the prior session's refactoring introduced NEW friction (e.g., the `VizWidget` trait is over-abstracted, or the `SwarmServer` split created cohesion problems), flag it — the cure can be worse than the disease.
- Bound every phase: if a phase runs > 30 minutes wall-clock without producing a report, stop and surface what blocked it.

---

## Deliverables

1. **`phase-a-security-review.md`** — kali-audit + supply-chain + runtime-posture + adversarial findings, with the 8-layer defense coverage matrix updated for the post-fix state
2. **`phase-b-code-review.md`** — code-review findings against the `origin/main..HEAD` diff, with regression flags for any behavior changes
3. **`phase-c-refactor-survey.md`** — fresh refactor-architecture survey of the post-refactor codebase
4. **`pre-release-final-summary.md`** — one-page exec summary with a release-readiness verdict (ready / blocked, with blockers listed) and a "Suggested .rules additions" section if any new non-obvious patterns were discovered

---

## Deferred for user verification

- **"the next release/update"** — which release version, what changelog scope? Not specified; the phases above assume "next scheduled release" but the user should confirm.
- **RR-0044 promotion** — the MCP error classification regression is `status: pending` (115 sites fixed by H2). Should it be promoted to `status: enforced`? Verify the grep pattern no longer matches any production code before promoting.
- **The `hkask-test-harness` `Result<_, String>` (4 sites)** — test oracle API callbacks. Changing the closure signature would break all oracle consumers. Deliberately skipped; confirm this is acceptable.