---
title: "Test Harness Trace Filesystem Schema"
audience: [agents, developers, ci]
last_updated: 2026-08-04
version: "0.31.1"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, lifecycle]
---

# Test Harness Trace Filesystem Schema

**Location:** `kask/traces/<run-id>/` (gitignored, like `target/`).

**Run ID format:** `<YYYYMMDDTHHMMSSZ>-<git-sha-short>` (e.g. `20260803T120000Z-a1b2c3d`).

This document is the single source of truth for the trace filesystem layout.
All skills that write or read traces (`proptest`, `bug-hunt`, `harness-optimize`)
reference this schema. Agents that write traces via file tools MUST produce JSON
matching the shapes defined here.[^claessen-quickcheck]

## Artifact Families

### 1. `manifest.json` — run manifest

**Path:** `<run-id>/manifest.json`
**Producer:** `scripts/test --trace`
**Consumer:** `harness-optimize` (context)

```json
{
  "run_id": "20260803T120000Z-a1b2c3d",
  "timestamp": "2026-08-03T12:00:00Z",
  "packages": ["hkask-types", "hkask-capability", ...],
  "harness_revision": "a1b2c3d"
}
```

### 2. `nextest-output.json` — raw test event stream

**Path:** `<run-id>/nextest-output.json`
**Producer:** `scripts/test --trace` (nextest `--message-format json`)
**Consumer:** `harness-evolve-cycle.sh` (failure splitting), `harness-optimize` (raw traces)

Nextest JSON event stream (one JSON object per line). Each event has a `type`
field (`"test"`, `"suite"`, etc.) and test events carry `event` (`"started"`,
`"finished"`), `name`, `status`, `stdout`.

### 3. `metrics.json` — run-level aggregate

**Path:** `<run-id>/metrics.json`
**Producer:** `scripts/test --trace` (pass_rate, total_tests, cost_tokens),
`stability-gate.sh` (mutation_score writeback), `cargo-llvm-cov` (coverage_pct,
coverage_by_crate)
**Consumer:** `TestCoverageSensor`, `MutationScoreSensor`, `stability-gate.sh`,
`harness-optimize`

```json
{
  "run_id": "20260803T120000Z-a1b2c3d",
  "pass_rate": 42,
  "total_tests": 42,
  "coverage_pct": 0.87,
  "coverage_by_crate": {
    "hkask-types": 0.92,
    "hkask-capability": 0.85
  },
  "mutation_score": 0.78,
  "cost_tokens": 34
}
```

**Field semantics:**
- `pass_rate` (int): number of tests that passed
- `total_tests` (int): total tests run
- `coverage_pct` (float 0–1): aggregate covered lines / total lines across all
  measured crates. Absent when `cargo-llvm-cov` is not installed.
- `coverage_by_crate` (object): per-crate coverage percentages. Absent when
  `cargo-llvm-cov` is not installed.
- `mutation_score` (float 0–1): killed mutants / total mutants. Absent when
  `cargo-mutants` is not installed or hasn't run.
- `cost_tokens` (int seconds): wall-clock duration of the test run. Always
  present in `--trace` mode.

**Missing-field convention:** sensors and the stability gate treat a missing
field as "no signal" (skip that axis), NOT as zero. This prevents spurious
convergence on absent metrics.

### 4. `coverage/<crate>.lcov` — per-crate line coverage

**Path:** `<run-id>/coverage/<crate>.lcov`
**Producer:** `scripts/test --trace` (split from `coverage-all.lcov`)
**Consumer:** `harness-optimize` (raw coverage traces)

Standard lcov format. One file per measured crate. A combined
`coverage-all.lcov` is also written at `<run-id>/coverage-all.lcov`.

### 5. `failures/<test-name>/` — per-failure artifacts

**Path:** `<run-id>/failures/<test-name>/`
**Producer:** `harness-evolve-cycle.sh` (failure splitting from
`nextest-output.json`), `qa-triage` classifier (classifier.json)
**Consumer:** `stability-gate.sh` (eir_classifier), `harness-optimize` (raw
failure traces)

Test names are sanitized: `/`, `\`, `:` replaced with `_`.

#### 5a. `output.txt`

Full stdout/stderr of the failed test.

#### 5b. `shrunk.txt`

Proptest shrunk counterexample (if the failure is a property test). Absent for
non-proptest failures.

#### 5c. `classifier.json`

qa-triage classifier output:

```json
{
  "failure_type": "assertion_mismatch",
  "root_cause": "off-by-one in loop bound",
  "confidence": 0.92,
  "is_real_bug": true,
  "is_flake": false,
  "proposed_fix": "change <= to < in loop condition",
  "suggested_fuzz_target": "compute_budget"
}
```

Absent when the qa-triage classifier or credentials are unavailable. The
stability gate's `eir_classifier` component stays 0 in that case (graceful
degradation).

### 6. `{kind}-{name}.json` — per-probe trace records

**Path:** `<run-id>/<kind>-<name>.json` (e.g. `proptest-prop_round_trip.json`)
**Producer:** `proptest` skill (standalone mode), `bug-hunt` skill (per-finding),
any skill that writes per-probe traces
**Consumer:** `harness-optimize` (raw probe traces)

Matches the `TraceEntry` shape from `hkask-test-harness`. When a file with the
same `(kind, name)` already exists, a `-N` suffix is appended (starting at 2).

```json
{
  "kind": "proptest",
  "name": "prop_round_trip",
  "result": "pass",
  "duration_ms": 42,
  "shrunk_counterexample": "",
  "oracle_type": "invariant",
  "metadata": {
    "crate": "hkask-templates",
    "target": "serialize"
  }
}
```

**`kind` values:** `proptest`, `bug-hunt`, `test-run`, or custom.[^claessen-quickcheck]
**`result` values:** `pass`, `fail`, `flaky`, or custom.
**`oracle_type` values:** `hardcoded`, `reference`, `invariant`, or empty.

### 7. `bug-hunt-report.json` — expedition-level report

**Path:** `<run-id>/bug-hunt-report.json`
**Producer:** `bug-hunt` skill (report phase)
**Consumer:** `harness-optimize` (semantic bug findings)

Fixed filename (not `{kind}-{name}.json`) so `harness-optimize` can find it
without scanning. The expedition ID is inside the JSON, not the filename.

```json
{
  "expedition_id": "charter_hkask_mcp_20260803",
  "charter_statement": "Explore hkask-mcp invoke path using boundary testing to discover governance bypass threats",
  "findings": [
    {
      "id": "F1",
      "summary": "Wrong-token invoke returns NotFound instead of CapabilityDenied",
      "location": { "file": "kask/crates/hkask-mcp/src/runtime.rs", "line_approx": 142 },
      "verdict": "BUG",
      "confidence": 0.92,
      "reproducibility": "reproduced",
      "beizer_category": "interface",
      "severity": "HIGH",
      "evidence": "assert!(matches!(result, Err(ToolPortError::NotFound(_))))",
      "pattern_signature": "ToolPortError::NotFound where CapabilityDenied expected",
      "fix_suggestion": "Map NotFound to CapabilityDenied when a token is present but mismatches"
    }
  ],
  "lessons_learned": ["async lock held across .await at 3 sites; next charter should target all .await points in lock scope"],
  "pattern_signatures": [
    { "signature": "ToolPortError::NotFound where CapabilityDenied expected", "beizer_category": "interface", "derived_from": "F1" }
  ]
}
```

### 8. `.run-history` — cycle state (not a trace artifact)

**Path:** `kask/traces/.run-history` (at the trace dir root, not inside a run-id)
**Producer:** `harness-evolve-cycle.sh`
**Consumer:** `harness-evolve-cycle.sh` (cross-invocation loop state)

Plain text, one run-id per line, newest first. Persists the run history across
invocations so the stability gate has N−1 to compare against. Delete this file
to reset the cycle.

## Compatibility Notes

- The trace filesystem extends `qa-triage-cycle` (which runs `cargo test` and
  classifies failures). This schema wraps that in trace collection.
- `harness-optimize` reads raw traces (nextest-output.json, failures/*/output.txt,
  bug-hunt-report.json) for causal hypotheses — NOT compressed summaries.
- Sensors and the stability gate read `metrics.json` (the compressed summary)
  for homeostatic control. Both coexist by design (Meta-Harness paper).[^beizer-testing]

---

## References

[^claessen-quickcheck]: Claessen, K., & Hughes, J. (2000). QuickCheck: A lightweight tool for random testing of Haskell programs. *Proceedings of the 5th ACM SIGPLAN International Conference on Functional Programming (ICFP '00)*, 268–279. https://doi.org/10.1145/351240.351266
    Cited for the property-based testing paradigm that the trace schema supports (proptest traces, shrunk counterexamples, oracle types).

[^beizer-testing]: Beizer, B. (1990). *Software testing techniques* (2nd ed.). Van Nostrand Reinhold. https://archive.org/details/softwaretestingt00beiz
    Cited for the bug taxonomy and failure-classification foundation underlying the trace filesystem's raw-trace vs. compressed-summary duality.
