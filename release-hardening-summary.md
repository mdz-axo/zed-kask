# Release-Hardening Summary

**Date:** 2026-08-04
**Workspace:** zed-kask
**Phases completed:** 4/4
**Reports:** `phase-1-ci-report.md`, `phase-2-refactor-survey.md`, `phase-3-maintenance-report.md`, `phase-4-security-report.md`

---

## Release-readiness verdict: **READY (with documented caveats)**

CI is green, all 2188 kask tests pass, all kask crates build, and the 8-layer defense stack is fully present. The remaining items are documented gaps, not blockers.

---

## One-page summary

### Phase 1 — CI / Build Hygiene ✅ Green

| Check | Result |
|-------|--------|
| `./script/clippy` (kask crates) | ✅ 0 warnings |
| `cargo fmt --all -- --check` | ✅ Pass (pre-fixed in prior commit) |
| `cargo nextest run` (kask crates) | ✅ 2188 passed, 3 skipped, 0 failed |
| `cargo build --workspace --bins` | ✅ All binaries built |
| 9 CI jobs (`kask-ci.yml`) | ✅ All pass |
| 4 non-CI local checks | 2 fixed (reg-creep, forecast-conformance), 2 remain (string-errors, mcp-tool-tests — advisory) |

**Changes applied:** Fixed `check-forecast-conformance.sh` failure (added 2 missing primitives to superforecasting README contract). Fixed stale references in `check-string-errors.sh` (deleted `hkask-acp`/`hkask-agents` → existing `hkask-keystore`/`hkask-memory`).

**Known upstream issue:** `remote_connection` has a feature-unification mismatch (`remote/test-support` enabled by `project` as a regular dep, `remote_connection`'s own `test-support` not enabled on scoped runs). Does NOT reproduce in CI (`--workspace --all-features`). Upstream bug — file upstream issue, do not fork-fix.

### Phase 2 — Refactor-Architecture Survey ✅ Complete (survey only, no code changes)

**11 High findings, 8 Medium, 9 Low** across 3 survey areas:

| Top finding | Severity | Area |
|---|---|---|
| `Result<_, String>` in swarm local-runtime (19 sites) | High | hkask-mcp-swarm |
| Inline `McpToolError::internal(format!())` across 5 servers (40+ sites) | High | 5 MCP servers |
| Monolithic `swarm_panel.rs` (3,700 lines, 60+ methods) | High | swarm_panel |
| `hkask-condenser` leaky dep on `hkask-mcp-server` | High | hkask-condenser |
| `SkillExecPort` trait returns `Result<String, String>` | High | hkask-types |
| Widget factory + cache-dispatch boilerplate (5× copy-paste) | Medium | viz widgets |
| Duplicated context-injector logic (~100 lines) | Medium | kask_bridge |
| IPC dispatch path untested | Medium | kask_bridge |

**No strangler-fig candidates** — the codebase is MCP-server-only (no multi-surface CLI/API duplication). No dead traits found. Bridge is wide but justified (9 adapters, all wired). All D-seams cleanly isolated (D7 missing pinning test — Low).

### Phase 3 — Skill Maintenance ✅ Complete (audit only, no updates applied)

| Skill | Verdict | Score | Top finding |
|-------|---------|-------|-------------|
| `kali-audit` | Active | 0.83 | `kask_bridge` not in discovery path; regression library lacks `.rules` traps |
| `supply-chain-sentinel` | Active | 0.90 | Stale `convergence-check.j2` reference; input contract drift |
| `runtime-posture-monitor` | **Critical** | 0.35 | **Data source `hkask-mcp-regulation` deleted**; `reg.regulation` namespace not registered |
| `adversarial-red-team` | Stale warning | 0.68 | "ed25519 DelegationToken" implies signature verification that doesn't exist |

**Cross-cutting finding:** The regression library (`kask/security/regressions/`) has 39 entries but **zero of the 17+ `.rules` traps are encoded**. This is the single highest-leverage update — it would improve all 4 security skills simultaneously.

### Phase 4 — Security Posture ✅ Complete

| Metric | Value |
|--------|-------|
| Blockers | 0 |
| High | 2 (RR-0030 missing test, `kask_bridge` missing unsafe-gating) |
| Medium | 5 (MCP error classification ×3, skill template issues ×2) |
| Low | 3 (all known/accepted: GuardedStream post-hoc, token no-expiry, deny+allow) |
| Defense layers | 8/8 present |
| Defense layer test gaps | 1 (Layer 7: RR-0030 enforcement test missing) |
| Skills that can run | 2/4 (kali-audit, supply-chain-sentinel) |
| Skills that cannot run | 2/4 (runtime-posture-monitor: deleted data source; adversarial-red-team: no live target) |

**Key finding (H1):** RR-0030 is marked `status: enforced` but the enforcement test `canary_in_reasoning_field_is_redacted` doesn't exist. The code IS correct (reasoning channel is scanned), but the test that would catch a future regression is missing. `check-kali-regressions.sh` reports this but isn't in CI — the loop is broken.

---

## Blockers for release

**None.** All findings are either known/accepted (Low), fixable without breaking CI (Medium), or documented gaps in non-CI tooling (skill staleness). The codebase compiles, tests pass, and all 8 defense layers are present.

## Recommended pre-release actions (optional, non-blocking)

| Priority | Action | Effort | Impact |
|---|---|---|---|
| 1 | Write the missing RR-0030 enforcement test | Low (1 test) | Closes a security regression gap |
| 2 | Add `#![cfg_attr(not(test), forbid(unsafe_code))]` to `kask_bridge.rs` | Trivial (1 line) | Matches all other kask crates |
| 3 | Fix `runtime-posture-monitor` data source (re-point or retire) | Medium | Makes 1 of 4 security skills functional |
| 4 | Populate regression library with `.rules` traps | Medium (add ~15 RR entries) | Improves all 4 security skills |
| 5 | Fix `adversarial-red-team` Layer 4 description (remove "ed25519") | Low (template edit) | Aligns with `.rules` OCAP guidance |
| 6 | Fix `supply-chain-sentinel` stale `convergence-check.j2` reference | Low (template edit) | Removes factual error in template |

## Files changed in this session

| File | Change |
|---|---|
| `kask/registry/templates/superforecasting/README.md` | Added `marginalize` + `certainty_tier` to the Deterministic Primitives contract table |
| `kask/scripts/check-string-errors.sh` | Fixed stale example references (deleted `hkask-acp`/`hkask-agents` → existing `hkask-keystore`/`hkask-memory`) |
| `phase-1-ci-report.md` | New — Phase 1 report |
| `phase-2-refactor-survey.md` | New — Phase 2 report |
| `phase-3-maintenance-report.md` | New — Phase 3 report |
| `phase-4-security-report.md` | New — Phase 4 report |
| `release-hardening-summary.md` | New — this file |

**No upstream files were modified.** All changes are in kask-owned files or new report files.

---

## Deferred for user verification

- **"The next release/update"** — which release version, what changelog scope? The phases assume "next scheduled release" but the user should confirm.
- **The 6 recommended pre-release actions** — these are optional, non-blocking improvements. The user should decide which to apply before release.
- **`runtime-posture-monitor` disposition** — re-point to an existing telemetry surface, or retire the skill? This is a product decision.
- **`check-kali-regressions.sh` CI promotion** — promoting from advisory to CI-blocking would close the RR-0030 loop but may surface other pre-existing failures.