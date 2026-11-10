---
title: "Testing system audit — 2026-11-09"
audience: [developers, architects, agents]
last_updated: 2026-11-09
version: "1.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [trust, lifecycle]
---

# Testing system audit — 2026-11-09

Recomposing the kask testing system along the canonical nextest custom-harness
composition, with test-only code removed from production builds. This is the
durable record of the audit; the machine-checkable inventory and dispositions
are in `testing-system-audit-2026-11-09.json`.

## 1. Inventory (command: `jq -f kask/tmp/testing-system-audit/inventory.jq` over `cargo metadata`)

- 34 kask packages; 2,345 unit tests in `src`; 40 integration test targets;
  2 proptest-regression files; 3 fixture sets.
- Harness configuration: `.config/nextest.toml` (`[profile.default]`,
  `[profile.kask]`, `[test-groups] sequential-db-tests`).
- CI gate: `.github/workflows/kask-invariants.yml` `kask-tests` job.

## 2. Staleness map (command: `bash kask/tmp/testing-system-audit/map_tests.sh`)

82 import rows: 75 live module imports, 4 live root-item imports, 0 unresolved,
3 no-crate-import targets. Verdict: no symbol-level staleness. The 3
no-crate-import targets are structural meta-gates (they pin file content —
ontology literals, `Cargo.toml` profiles, provider YAML), classified as
structural rather than stale.

## 3. Test-only symbol dispositions

| Symbol | Before | Disposition | Evidence |
|---|---|---|---|
| `hkask-memory::test_support` | unconditional `pub mod` | gated to `test-fixture` (self dev-dependency) | production rlib `nm` count 0; 3 test callers unchanged |
| `hkask-mcp-server::test_support` | unconditional `pub mod` | gated to `test-fixture` (4 consumer dev-deps) | production rlib `nm` count 0; 4 consumer test builds compile |
| `hkask-mcp-kata-kanban::test_support` | unconditional `#[doc(hidden)] pub mod` | gated to `test-fixture` (self dev-dependency) | production rlib `nm` count 0; integration tests pass |
| `kask_bridge` / research / media / regulation `test_support` | `#[cfg(test)]` | keep (already test-only) | — |
| `kask_bridge::LanguageModelEmbeddingPort::for_tests*` | `#[cfg(test)]` | keep (already test-only) | — |

## 4. Research anchors

- `test_harness` (derived; operator ruling 2026-09-18) — the runner, fixtures
  and reporting that execute checks; distinct from the checks themselves.
- `test_driven_development` (derived; operator ruling 2026-10-14; Beck 2002).
- Best software/process anchors: `verification` → `dpv:Verification` /
  `cso:verification`; `validation` → `pko:Validation` / `cso:validation`;
  `process` → `dpv:Process` / `sumo:Process`; `procedure` →
  `pko:Procedure` / `sumo:Procedure`; `activity` → `pplan:Activity` /
  `prov:Activity` / `crm:E7_Activity`.
- Coarse anchors requiring operator ruling (never privately defined):
  `characterization test`, `test suite architecture`, and every
  testing-specific term (`test`, `testing`, `unit test`, `integration test`,
  `regression test`, `acceptance test`, `test case`, `test execution`,
  `test procedure`, `defect`) — all terminate at `5w1h_core`.
- zk-ref-open nextest composition passages (runner loop, signal handling,
  input handling, recording runs, selecting tests, failing fast, termination
  behavior, command-line options, configuration) — see the research summary
  artifact for the passage records.

## 5. Harness recomposition (committed `a6caa3df7d`)

- `kask/scripts/test` — canonical runner: nextest `--profile kask` over the
  frozen package set, `-p` selection, `--list`, zero-test guard, cargo-test
  fallback.
- `.config/nextest.toml` — `[profile.kask]` carries
  `fail-fast = { max-fail = 1, terminate = "wait" }` plus the existing
  `slow-timeout` grace period and `sequential-db-tests` group.
- CI `kask-tests` job installs cargo-nextest and runs the canonical runner.
- Test-only gating: three `test-fixture` features with self dev-dependencies
  and consumer dev-dependency features.

## 6. Verification (before/after)

| Check | Before | After |
|---|---|---|
| Suite | 3,125 run: 3,120 passed / 5 failed / 13 skipped, 63.4 s | 3,126 run: 3,126 passed / 0 failed / 13 skipped, 60.6 s |
| Stale/disconnected | 0 unresolved imports (3 structural classified) | unchanged |
| Test-only symbols | 3 modules in production rlibs | 0 (`nm` count) |
| Change-failure probe | — | `sumo::CAUSES` → fabricated concept: `named_concepts_are_published` FAILED as predicted; reverted, green |
| Over-pinning probe | — | private `is_sentence_end`→`is_sentence_boundary` rename: 95/95 lib tests pass; reverted |
| Media-widget flakes | 1 failure in 3,125 (retry test) | 0/30 speed-test runs, 0/15 suite runs after fixes |

Gates: `check-mcp-tool-tests` 0 violations · `check-unused-deps` OK ·
duplication ratchet unchanged by these edits (verified by reverting: 2,782
clusters both ways) · dead-code allows OK · `cargo fmt --check` clean ·
`clippy --all-targets --all-features -- -D warnings` clean · production
`cargo check` of all affected crates clean.

## 7. Media-widget bug-hunt findings (expedition 2026-11-09)

- **bh-001 (MEDIUM, fixed):** the retry test fixture's second resolve returned
  a remote URL whose async fetch failure could land inside the test's parking
  window. Now local PNG bytes.
- **bh-003 (HIGH, fixed):** a stale `Opened` event re-applied its `Stopped`
  snapshot over a newer `Playing` state (`sequence >= last_sequence` guard),
  freezing the visible position at 0. Guards are now strictly newer (`>`).
- **bh-002 (LOW observation):** static baseline clean — no production
  unwrap/expect/panic; the crate forbids `unsafe_code`.

Full report: `~/Documents/zk-data/skills/bug-hunt/2026-11-09-hkask-media-widget/bug-hunt-report.json`.

## 8. Residual risk and open operator calls

1. CI now runs the parallel canonical runner; the previous serial
   determinism choice is superseded — the two flakes found under parallel
   load are fixed, but further flakes may surface.
2. The duplication ratchet currently reads 2,782 against a 2,774 baseline —
   the 8 new clusters come from the concurrent session's uncommitted
   `media_viewer.rs` work, not from this audit's edits.
3. Coarse anchors (`characterization test`, `test suite architecture`, and
   the testing-specific terms) still need operator rulings before use.
4. The docs count gate reads 104 files against the <85 cap (advisory, unwired
   from CI by operator mandate 2026-10-31); this audit added 2 files.