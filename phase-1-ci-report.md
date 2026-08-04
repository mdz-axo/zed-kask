# Phase 1 — CI / Build Hygiene Report

**Date:** 2026-08-03
**Scope:** kask-owned crates (`kask/crates/*`, `kask/mcp-servers/*`, `crates/hkask-*-widget`, `crates/kask_extensions_ui`, `crates/swarm_panel`, `crates/marketplace_ui_common`) + the D-seam boundary.
**Backward-compat note:** No backward-compatibility constraints apply within kask-owned crates. Renames, restructures, and deletions are permitted. The D-seam boundary and "do not touch upstream" rule apply in full.

---

## Metacognition record

| | Prediction | Actual | Brier |
|---|---|---|---|
| Clippy warnings in kask crates | 2–5 (conf 0.6) | 0 | 0.36 |
| Workflow YAML issues | 1–2 (conf 0.6) | 4 info-level | 0.16 |
| Overall "issues will surface" | Yes (conf 0.8) | Yes (non-CI checks) | 0.04 |

Combined Brier ≈ 0.19. The direction was correct (issues exist) but the category was wrong — issues surfaced in non-CI local checks, not clippy.

---

## CI-enforced checks (`.github/workflows/kask-ci.yml`)

All 9 CI jobs verified locally. **CI is green.**

| Job | Command | Result | Notes |
|---|---|---|---|
| fmt | `cargo fmt --all -- --check` | ✅ PASS | Pre-fixed in commit `7e043dfcf5`. |
| clippy | `./script/clippy` (scoped to kask crates + `remote_connection`) | ✅ PASS | 0 warnings. See "Known upstream issue" below for scoped-run caveat. |
| test | `cargo nextest run` (scoped, `--all-features`) | ✅ PASS | 2188 tests run, 2188 passed, 3 skipped. |
| build | `cargo build --release --workspace --bins` | ✅ PASS | All kask MCP server binaries + `zed-kask` built. |
| skill-span-namespace | `kask/scripts/check-skill-span-namespace.sh` | ✅ PASS | 95 skill manifests conform. |
| reg-canonical | `kask/scripts/check-reg-canonical.sh` | ✅ PASS | All `reg.*` references canonical. |
| mcp-servers | `kask/scripts/check-mcp-servers.sh` | ✅ PASS | 11 servers match `BUILTIN_SERVERS`. |
| hkask-no-zed-deps | `kask/scripts/check-hkask-no-zed-deps.sh` | ✅ PASS | §13.1 invariant holds. |
| deps | `cargo machete` | ✅ PASS | Unused `theme` dep in `hkask-portfolio-widget` removed in commit `59f0834c96`. |

### Clippy scoped-run caveat

The `./script/clippy` script defaults to `--workspace --all-features`. When scoped to kask crates via `-p` flags, a feature-unification mismatch in the upstream `remote_connection` crate surfaces:

- `remote/test-support` is enabled as a regular dep by `project` and others, making `RemoteConnectionOptions::Mock` visible.
- `remote_connection`'s own `test-support` feature is NOT enabled (it's a dependency, not a targeted package), so the `Mock` match arm in `remote_connection.rs:243` is not compiled.
- Result: `non-exhaustive patterns: &RemoteConnectionOptions::Mock(_)` compile error.

**This does NOT reproduce in CI** — `--workspace --all-features` enables `test-support` on all workspace crates including `remote_connection`. The workaround for scoped runs is to add `-p remote_connection` to the target list. This is an upstream bug (feature leak through `project`'s regular deps), not a kask issue. Filing an upstream issue is the correct action — per `.rules`, do not fork-fix.

---

## Non-CI local checks (not in `kask-ci.yml`)

| Check | Result | Finding |
|---|---|---|
| `check-reg-creep.sh` | ✅ PASS | 6 missing canonical namespaces fixed in commit `59f0834c96`. |
| `check-forecast-conformance.sh` | ✅ FIXED | Added `marginalize` and `certainty_tier` to the superforecasting README contract table. **New change in this session.** |
| `check-string-errors.sh` | ❌ FAIL | `Result<_, String>` patterns found in `hkask-templates`, `hkask-test-harness`, `hkask-mcp-swarm`. Script's example references updated (were stale — pointed to deleted `hkask-acp` / `hkask-agents` crates). **New change in this session.** |
| `check-mcp-tool-tests.sh` | ⚠️ 7 violations | 7 MCP servers (`codegraph`, `companies`, `condenser`, `corpus`, `media`, `swarm`, `training`) lack tool-behavior contract tests. Script exits 0 (advisory). |
| `check-kali-regressions.sh` | ⚠️ 1 issue | RR-0030 orphaned: missing enforcement test `canary_in_reasoning_field_is_redacted` in `hkask-guard`. Script exits 0 (advisory). |
| `check-convergence-weights.sh` | ✅ PASS | 0 checked (no weights found). |
| `ci-contract-check.sh` | ✅ PASS | No contract violations. |

### `check-string-errors.sh` findings (detail)

The script found `Result<_, String>` anti-pattern in:

| Crate | File | Count | Severity |
|---|---|---|---|
| `hkask-templates` | `src/inputs.rs` | 1 | Medium |
| `hkask-test-harness` | `src/hkask_test_harness.rs` | 4 | Low (trait bounds — may be intentional for test flexibility) |
| `hkask-mcp-swarm` | `src/local_runtime.rs`, `consent.rs`, `local_registry.rs`, `a2a_http.rs`, `abw_util.rs`, `local_knowledge.rs`, `local_swarms.rs` | 15 | Medium |

The `hkask-mcp-swarm` crate has the most violations (15 `Result<_, String>` return types). These should be converted to `thiserror` enums for structured error handling. This is a refactor-architecture finding for Phase 2.

### `check-mcp-tool-tests.sh` findings (detail)

7 of 11 MCP servers have no tool-behavior contract tests (no `Parameters(` in their `tests/` directory):

| Server | Has tool tests? |
|---|---|
| `hkask-mcp-codegraph` | ❌ |
| `hkask-mcp-companies` | ❌ |
| `hkask-mcp-condenser` | ❌ |
| `hkask-mcp-corpus` | ❌ |
| `hkask-mcp-curator` | ✅ |
| `hkask-mcp-kata-kanban` | ✅ |
| `hkask-mcp-media` | ❌ |
| `hkask-mcp-research` | ✅ |
| `hkask-mcp-scenarios` | ✅ |
| `hkask-mcp-swarm` | ❌ |
| `hkask-mcp-training` | ❌ |

### `check-kali-regressions.sh` finding (detail)

RR-0030 is orphaned — it describes a guard against `reasoning` / `reasoning_delta` channels bypassing `scan_output` in `GuardedInferencePort` (OWASP LLM07), but the enforcement test `canary_in_reasoning_field_is_redacted` does not exist in `hkask-guard`. The regression is marked as enforced but has no live test. This is a security coverage gap — see Phase 4.

---

## Actionlint findings (`.github/workflows/kask-ci.yml` + `kask-release.yml`)

4 shellcheck info-level warnings (none are errors):

| File | Line | Rule | Description | Severity |
|---|---|---|---|---|
| `kask-ci.yml` | 93 | SC2012 | `ls -t traces` in mutation testing step — use `find` for non-alphanumeric filenames | Info |
| `kask-release.yml` | 78 | SC2086 | Unquoted `$packages` — intentional word splitting for `--package` args | Info (false positive) |
| `kask-release.yml` | 92 | SC2035 | Glob `*` without `./` prefix in archive step | Info |
| `kask-release.yml` | 136 | SC2035 | Glob `*` without `./` prefix in release artifact step | Info |

All 4 are in `continue-on-error` or non-critical paths. No action required for release.

---

## Changes applied in this session

| File | Change | Reason |
|---|---|---|
| `kask/registry/templates/superforecasting/README.md` | Added `marginalize` and `certainty_tier` rows to the Deterministic Primitives contract table | `check-forecast-conformance.sh` failure — 2 `#[must_use]` pub fns not in contract |
| `kask/scripts/check-string-errors.sh` | Updated example file references from deleted `hkask-acp`/`hkask-agents` to existing `hkask-keystore`/`hkask-memory` | Stale references to non-existent crates |

**Previously committed** (commit `59f0834c96`, prior session):
- Removed unused `theme` dep from `hkask-portfolio-widget/Cargo.toml`
- Added 6 missing canonical namespaces to `event.rs`

**Previously committed** (commit `7e043dfcf5`, prior session):
- `cargo fmt` fixes to `open_ai/src/completion.rs` and `hkask-mcp-corpus/src/runtime/classify_impl.rs`

---

## Pragmatic-semantics classification

| Finding | IS/OUGHT | Epistemic mode | Constraint force | Provenance | Confidence |
|---|---|---|---|---|---|
| CI is green | IS | Declarative | Hard | Tool output | 1.0 |
| `remote_connection` upstream feature leak | IS | Declarative | Soft (upstream issue) | Compiler error | 0.95 |
| `Result<_, String>` anti-pattern | OUGHT | Normative | Soft (local check, not CI) | Script output | 0.9 |
| 7 MCP servers lack tool tests | IS | Declarative | Soft (advisory) | Script output | 1.0 |
| RR-0030 missing enforcement test | IS | Declarative | Hard (security regression) | Script output | 0.95 |
| `check-string-errors.sh` example refs were stale | IS | Declarative | Soft | File system check | 1.0 |

---

## Grill-me self-challenge

**Recall:** What 3 checks pass in CI but have related local checks that fail?
**Mechanism:** CI runs `check-reg-canonical` (hierarchical, passes) but NOT `check-reg-creep` (exact match, was failing on 6 targets — now fixed). CI runs `cargo machete` (passes) but `check-forecast-conformance` (local, was failing on 2 primitives — now fixed). CI runs `cargo nextest` (passes) but `check-mcp-tool-tests` (local, advisory, reports 7 gaps). The pattern: CI enforces the minimum bar; local checks surface higher-bar quality gaps that haven't been promoted to CI gates yet.

---

## Phase 1 verdict

**CI is green.** All 9 CI-enforced jobs pass. Three non-CI local checks were fixed (reg-creep, forecast-conformance, stale script refs). Two non-CI checks remain failing (check-string-errors: `Result<_, String>` anti-pattern — deferred to Phase 2 refactor survey; check-mcp-tool-tests: 7 servers lack tool-behavior tests — documented as coverage gap). One security regression (RR-0030) has a missing enforcement test — escalated to Phase 4.

No upstream files were modified. The `remote_connection` feature-leak is an upstream bug documented for upstream issue filing.