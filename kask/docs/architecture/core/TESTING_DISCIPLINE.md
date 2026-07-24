---
title: "hKask Testing Discipline"
audience: [engineers, agents, userpods]
last_updated: 2026-07-04
version: "0.31.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# hKask Testing Discipline

**First principles:** Tests are the system’s regulator model (P9). They must (1) enforce boundaries (P4), (2) ground claims in observable invariants (P8), and (3) minimize action by preferring deep seams over shallow ones (P5).  
**Method (default):** Property-Based Testing (QuickCheck, Claessen & Hughes, 2000) verified through Regulation observability.  
**Internal bridge:** TDD skill (`.agents/skills/tdd/SKILL.md`) — the process for writing verified tests.  
**Governing principles:** P4 (Clear Boundaries), P5 (Essentialism), P8 (Semantic Grounding), P9 (Homeostatic Self-Regulation).

**Supersedes:** `docs/specifications/specs/test-program.md` (archived 2026-06-15), `docs/specifications/standards/TESTING_STANDARDS.md` (archived 2026-06-15). This document is the single authoritative reference for all hKask testing practices, standards, and philosophy.

---

## 1. Verification From First Principles

### 1.1 Why tests exist

Tests are not “quality checks.” They are **regulatory instruments**: the system’s model of itself. If tests do not detect drift, the regulator is blind (P9). If tests probe shallow seams, they waste action and increase coupling (P5). If tests do not align with OCAP and sovereignty boundaries, they invalidate P4.

### 1.2 What a test must prove

Every test must answer one of these first-principle questions:

1. **Boundary:** Did the operation respect OCAP and consent boundaries (P4, P1–P3)?
2. **Invariant:** Did a defined invariant hold across valid inputs or sequences (P8)?
3. **Equivalence:** Did the same operation yield the same result across surfaces (CLI/API/MCP) where required?
4. **Homeostasis:** Did the feedback loop restore stability after perturbation (P9)?

If a test answers none, it is ontological noise (P5.2) and should be deleted.

## 2. The Verification Method: Property-Based Testing

### 2.1 The Principle

A property-based test does not test a single example. It tests an invariant across randomly generated inputs:

```rust
proptest! {
    #[test]
    fn compression_is_idempotent(input in any::<String>()) {
        prop_assume!(!input.is_empty());
        let once = compress(&input);
        let twice = compress(&once);
        prop_assert_eq!(once, twice);
    }
}
```

`prop_assume!` enforces preconditions — inputs that violate them are skipped. `prop_assert!` verifies postconditions. Proptest generates 10,000+ random inputs and shrinks failures to minimal counterexamples.

### 2.2 When to Use Property-Based Tests

| Situation | Use PBT? | Reason |
|-----------|----------|--------|
| Function has mathematical invariants | **Yes** | One proptest replaces dozens of examples |
| Function is pure (no side effects) | **Yes** | Deterministic, easy to verify |
| Function is stateful but has invariants across operation sequences | **Yes** | State machine PBT (generate sequences, verify invariant holds throughout) |
| Function is I/O-bound (network, filesystem) | **No** | Use integration tracer bullet instead |
| Function has no meaningful invariant beyond "doesn't panic" | **Fuzz only** | `catch_unwind` + arbitrary input |

### 2.3 The Test Pyramid

| Layer | What | Verification |
|-------|------|-------------|
| **Unit** | Single function's behavior | Proptest on the function directly |
| **Integration** | Cross-function chains | Proptest on the entry point; Regulation spans verify called functions' behavior |
| **State machine** | Invariants across operation sequences | Proptest on operation sequences; Regulation `reg.gas` spans track budget invariants |
| **Fuzz** | Input surface robustness | `catch_unwind` + arbitrary input; verifies no panic |
| **System** | End-to-end workflows | Integration tracer bullet (TDD skill); verifies full vertical slice |

### 2.4 Deployment Testing

Deployment testing covers the provisioning surface — the operations that initialize and configure a running hKask server:

| Domain | Test Type | Example |
|--------|-----------|---------|
| Server init | Integration | `init_server_creates_config_and_keychain_entries` |
| Sidecar generation | Integration | `deploy_sidecar_generates_valid_docker_compose` |
| OAuth callback | Integration | `oauth_callback_provisions_human_user_and_session` |
| Health endpoint | Unit | `health_endpoint_returns_reg_status` |
| Single binary | Smoke | `single_binary_contains_all_components` |
| Docker build | CI | `docker_build_produces_working_image` |

---

## 3. Ontology — Testing Vocabulary

| Term | Definition | Domain |
|------|-----------|--------|
| **Seam** | A public interface (`pub` trait, `pub` fn, `pub` struct with `pub` methods) that is the test surface | FlowDef |
| **Invariant** | A behavioral property that must hold for all valid inputs or across all operations on a type | KnowAct |
| **Tracer-bullet** | A vertical RED→GREEN cycle: one invariant, one test, one implementation. Never horizontal slices | FlowDef |
| **Behavioral test** | A test that exercises the public seam and verifies *what* the system does, not *how*. Survives refactors | KnowAct |
| **Structural test** | A test coupled to implementation detail. Must be rewritten or documented as debt | KnowAct |
| **Deep seam** | Small interface, high leverage (few methods, many behaviors). Prefer testing at deep seams | WordAct |
| **Shallow seam** | Interface as complex as implementation. Deepening candidate, not a testing target | WordAct |
| **Test cycle** | `tracer-bullet` (first test for new behavior), `regression` (test after fix), `property` (proptest/fuzz) | FlowDef |
| **Test debt** | Implementation-coupled tests that exist because a clean seam hasn't been extracted yet | KnowAct |

---

## 4. Test Classification

Every test falls into one of three categories:

| Category | Definition | Survives Refactor? | Priority |
|----------|-----------|-------------------|----------|
| **Public Interface** | Tests behavior through a module's public API or trait seam | ✅ Yes | **Required** |
| **Seam Integration** | Tests interaction between two modules through a shared trait | ✅ Yes | **Required** |
| **Implementation-Coupled** | Tests private methods, internal state, or mocked collaborators | ❌ No | **Flag for rewrite** |

### 4.1 Classifying an Existing Test

Ask: *"If I rewrote the entire internals of this module, would this test still pass?"*

- **Yes** → Public Interface test. Keep.
- **Only if the new internals use the same trait** → Seam integration test. Keep.
- **No** → Implementation-coupled test. Flag for rewrite or removal.

### 4.2 Implementation-Coupled Tests Are Technical Debt

Implementation-coupled tests are not forbidden — they exist because some code currently lacks a clean seam. But they must be tracked:

- Add a `// TEST-DEBT: tests private <detail>` comment above the test
- The debt is resolved when a deeper interface makes the test unnecessary

---

## 5. MDS Category → Test Strategy

### 5.1 Domain (REQ-DOM-*)

| Strategy | Details |
|----------|---------|
| **Primary seam** | `WebID`, `RegulationRecord` public APIs |
| **Test type** | Unit: type construction, parsing, validation. Serialization round-trips |
| **Key invariant** | Lexicon round-trips (markdown → YAML → loaded vocabulary) |
| **Anti-pattern** | Testing internal hashmap structure of lexicon types |

### 5.2 Capability (REQ-CAP-*)

| Strategy | Details |
|----------|---------|
| **Primary seam** | `CapabilitySpec`, `DelegationToken`, capability verification traits |
| **Test type** | Integration: capability attenuation chains, per-userpod key derivation |
| **Key invariant** | Fail-closed: no checker = denied, not open |
| **Anti-pattern** | Testing HMAC internals rather than attenuation behavior |

### 5.3 Interface (REQ-IFC-*)

| Strategy | Details |
|----------|---------|
| **Primary seam** | CLI ↔ API ↔ MCP equivalence (core operations) |
| **Test type** | Integration: cross-surface parity (same operation, same result, via all three surfaces) |
| **Key invariant** | `MCP ≡ CLI ≡ API` for core operations; spec lifecycle is CLI + API + QA only |
| **Anti-pattern** | Testing only one surface and assuming the others work |

### 5.4 Composition (REQ-COM-*)

| Strategy | Details |
|----------|---------|
| **Primary seam** | `SqliteRegistry`, `TemplateResolver` |
| **Test type** | Integration: register → resolve → render round-trips; cascade depth enforcement |
| **Key invariant** | Template cascade terminates within depth limit |
| **Anti-pattern** | Testing Jinja2 string manipulation in isolation |

### 5.5 Trust & Security (REQ-TRU-*)

| Strategy | Details |
|----------|---------|
| **Primary seam** | `GovernedTool`, key derivation, OCAP verification |
| **Test type** | Unit + integration: deterministic key derivation, attenuation depth limits, fail-closed |
| **Key invariant** | Security boundaries are never relaxed by default |
| **Anti-pattern** | Only testing the happy path; not testing invalid, expired, or wrong tokens |

### 5.6 Observability (REQ-OBS-*)

| Strategy | Details |
|----------|---------|
| **Primary seam** | `RegulationLedger`, `AlgedonicManager`, `RegulationSink` |
| **Test type** | Unit: span emission, variety counter thresholds; Integration: Regulation feedback loop closure |
| **Key invariant** | Algedonic alerts fire at threshold; homeostasis restores after perturbation |
| **Anti-pattern** | Testing `tracing::info!` output format rather than the observer's behavior |

### 5.7 Persistence (REQ-PER-*)

| Strategy | Details |
|----------|---------|
| **Primary seam** | Repository traits (`hMemStore`, `SpecStore`, `WalletStore`) |
| **Test type** | Integration: round-trip through SQLite with `TestDb` from harness crate |
| **Key invariant** | Bitemporal queries return correct results; encrypted storage fails without key |
| **Anti-pattern** | Testing SQL query strings rather than repository behavior |

### 5.8 Lifecycle (REQ-LIF-*)

| Strategy | Details |
|----------|---------|
| **Primary seam** | `main()` entry point, migration functions, bootstrap sequence |
| **Test type** | Integration: bootstrap sequence, schema migration |
| **Key invariant** | Forward-only evolution — no rollback paths |
| **Anti-pattern** | Testing CLI argument parsing in isolation when the real risk is bootstrap ordering |

### 5.9 Curation (REQ-CUR-*)

| Strategy | Details |
|----------|---------|
| **Primary seam** | `SpecCurator`, `SpecStore`, MCP spec tool handlers |
| **Test type** | Integration: spec capture → evaluate → cultivate round-trip |
| **Key invariant** | Coherence threshold gates curation decisions |
| **Anti-pattern** | Testing Jaccard similarity in isolation without testing the full curation pipeline |

### 5.10 CodeGraph (REQ-CG-*)

| Strategy | Details |
|----------|---------|
| **Primary seam** | `GraphStore` (SQLite), `IndexPipeline` (tree-sitter + SHA-256), FTS5 search, recursive CTE traversal |
| **Test type** | Integration: schema idempotency, insert+query round-trip, traversal correctness, impact classification |
| **Key invariant** | Incremental indexing — SHA-256 hash match skips re-parsing; recursive CTE depth-bounded |
| **Anti-pattern** | Testing tree-sitter AST structures directly rather than through symbol/edge extraction |

---

## 6. Principle Alignment

### 6.1 P4 — Clear Boundaries (OCAP)

Invariants at crate boundaries detect **semantic drift** — when a type changes in a way that's type-compatible but behaviorally different. The compiler can't catch this. Property-based tests can. Regulation spans (`reg.gas`, `reg.tool.*`) provide runtime verification at every boundary.

### 6.2 P8 — Semantic Grounding

Every test verifies an IS claim about system behavior. The Regulation span registry (`RegulationSpan` in `crates/hkask-types/src/regulation.rs`) defines the canonical observability namespace. Test output is traceable to span types.

### 6.3 P9 — Homeostatic Self-Regulation

**The test suite is a feedback loop.** Under the Good Regulator Theorem (Conant & Ashby, 1970), every good regulator must be a model of the system it regulates. The test suite IS that model.

- **Regulation spans provide runtime observability.** `reg.gas` spans on `reserve`/`settle`/`consume`/`reset_to` track budget invariants in production. Type-enforced invariants (private fields on `GasBudget`) prevent violations structurally.
- **Test coverage is variety.** The Regulation tracks test coverage per domain as variety (Ashby's Law). A drop in variety triggers an alert.
- **Mutation testing measures regulator quality.** `cargo-mutants` injects bugs; the percentage caught measures how well the test suite models the system.

### 5.4 P6 — Space for Human Users and Their Tools

Developers (and AI coding agents acting on their behalf) propose tests for behavior. The author identity is the authenticated user (P12); a userpod may carry the WebID. A human operator provides affirmative consent (P2) to merge. Post-pivot, the human user is the principal; the userpod is the sovereign container that carries their identity.

### 5.5 P7 — Evolutionary Architecture

Tests evolve from actual failures, not speculation. When a bug escapes to production:
1. Write a proptest that captures the failure mode
2. Verify it fails (reproduces the bug)
3. Fix the implementation
4. The proptest now permanently guards against that class of bug

Tests accumulate the scar tissue of every production incident. They become the real engineering artifact — the implementation is replaceable; the invariants are not.

---

## 6. Rules for the Testing Program

### 6.1 Test Location

| Test Type | Location | Convention |
|-----------|----------|------------|
| Unit (same-module) | `#[cfg(test)] mod tests` inside source file | For testing public interface of a single module |
| Integration | `tests/` directory at crate root | For testing cross-module behavior through crate public API |
| Fuzz (property-based) | `src/` module with proptest strategies | For panic-free verification on arbitrary input via proptest `#[test]` functions |
| MCP server fuzz | `mcp-servers/*/fuzz/fuzz_targets/` directory | For full tool dispatch path fuzzing under `catch_unwind` |

### 6.2 Testing Rules

| Rule | Description |
|------|-------------|
| **T1** | Prefer property-based tests for functions with mathematical or state-machine invariants |
| **T2** | Property-based tests use `prop_assume!` to enforce preconditions |
| **T3** | Property-based tests use `prop_assert!` to verify postconditions and invariants |
| **T4** | Integration tracer bullets follow the TDD skill's vertical slice pattern |
| **T5** | Fuzz tests verify that all input surfaces handle arbitrary input without panicking |
| **T6** | No `todo!()`, `unimplemented!()`, or `#[deprecated]` in test code (P5) |
| **T7** | Use `#[cfg(test)]` module for unit tests; `tests/` for integration; `#[tokio::test]` for async |
| **T8** | Use `tempfile` or `hkask-test-harness` for filesystem/database — never write to the project tree |
| **T9** | Prefer `assert!` with meaningful messages; test error paths, not just happy paths |

### 6.3 Process Rules

| Rule | Description |
|------|-------------|
| **P1** | Test first, implementation second (TDD) |
| **P2** | One test per TDD cycle (vertical slice, not horizontal) |
| **P3** | Refactor only when GREEN — never while RED |
| **P4** | After every bug fix, add a regression test that captures that class of bug |
| **P5** | Developers and AI coding agents may propose tests; the human user provides consent to merge (P2, P6). The userpod carries the author WebID (P12) |
| **P6** | Every test action carries an authenticated author (TestWebId or userpod WebID) (P12) |

### 6.4 Quality Rules

| Rule | Description |
|------|-------------|
| **Q1** | Mutation testing runs periodically; target ≥70% mutant detection |
| **Q2** | Regulation monitors test coverage as variety per domain; drops trigger algedonic alerts |
| **Q3** | Type-enforced invariants (private fields, constructor validation) preferred over runtime assertions |

---

## 7. Verification & Audit

### 7.1 Verification Gates

| Gate | Command | Expected |
|------|---------|----------|
| Build | `cargo check --workspace` | Pass |
| Tests | `cargo test --workspace` | All pass |
| Lint | `cargo clippy --workspace -- -D warnings` | No warnings |
| Format | `cargo fmt --check` | No diffs |
| Prohibitions | `grep -r "todo!\|unimplemented!\|#\[deprecated\]" crates/ --include="*.rs"` | Zero |
| Headless | `grep -r "grafana\|prometheus\|dashboard\|visual.*ui" crates/ --include="*.rs"` | Zero |
| Regulation daemon | `kask daemon start` (smoke test) | Binds socket, loops active |
| Deployment smoke | `kask init --profile server && kask daemon` | Server starts, health endpoint responds |
| Deployment sidecar | `kask matrix deploy-sidecar --domain localhost` | Valid docker-compose.yml generated |

---

## 8. References

### External Discipline

- Claessen, K. & Hughes, J. (2000). "QuickCheck: A Lightweight Tool for Random Testing of Haskell Programs." *ICFP*. The foundational PBT text.
- Meyer, B. (1986). "Design by Contract." *IEEE Computer*.

### Cybernetic Foundation

- Conant, R.C. & Ashby, W.R. (1970). "Every Good Regulator of a System Must Be a Model of That System." *Int. J. Systems Sci.* The theoretical basis for tests as regulator model.
- Ashby, W.R. (1956). *An Introduction to Cybernetics*. The theoretical basis for variety-based coverage monitoring.

### Software Engineering Foundations

- Beck, K. (2003). *Test-Driven Development: By Example.* Addison-Wesley. The red-green-refactor cycle.
- Feathers, M. (2004). *Working Effectively with Legacy Code.* Prentice Hall. The seam model and test classification.
- Meszaros, G. (2007). *xUnit Test Patterns: Refactoring Test Code.* Addison-Wesley. Shared fixtures and test infrastructure patterns.

### hKask Internal

- `docs/architecture/core/PRINCIPLES.md` — P1–P12 governing principles
- `.agents/skills/tdd/SKILL.md` — TDD process (RED→GREEN→REFACTOR with spec anchoring)
- `docs/architecture/core/MDS.md` — Minimal Domain Specification
- `docs/architecture/qa/QA_PLAN.md` — QA architecture (fuzz, mutation, LLM triage)

---

## 8. Property-Based Testing

hKask uses **proptest** (already in `hkask-test-harness`) for property-based testing.
Property tests live in `#[cfg(test)]` modules alongside the code they verify and
are run via standard `cargo test`. No external fuzzing frameworks are used.

### 8.1 Running Property Tests

```bash
# All property tests run via standard cargo test
cargo test --workspace

# Fast property tests (CI on every push)
cargo test -p hkask-regulation -p hkask-types -p hkask-storage
```

### 8.2 Fuzz Seed Corpora

The `hkask-test-harness::fuzz` module provides pre-built seed corpora for
input surface testing:

```rust
use hkask_test_harness::fuzz::{cli_fuzz_seeds, json_fuzz_seeds};

// Test CLI argument parser resilience
for seed in cli_fuzz_seeds() {
    // no panic
}

// Test JSON deserializer resilience
for seed in json_fuzz_seeds() {
    // no panic on malformed input
}
```

## 9. Mutation Testing (removed)

Mutation testing via `cargo-mutants` was removed. The fuzz crate infrastructure
(`hkask-*-fuzz`) was also removed. Property-based testing via proptest, combined
with the QA manifest system, provides equivalent coverage without external tooling.

### 9.1 Integration with QA Pipeline

Property test failures that reach the QA system are triaged through the
classifier (Gemma 4 26B) and routed by confidence — same as any other test
failure in a QA manifest.

---

## 10. QA System — Contract Gate Automation

### 10.1 Overview

The QA system runs YAML-defined test manifests through `hkask_test_harness::qa_script::run_script()`.
Each manifest executes `cargo test` commands, optionally classifies failures via Gemma 4 26B,
and routes to terminal states (PASS/FAIL/WARN) based on branch conditions.

| Confidence | Route | Regulation Span |
|-----------|-------|----------|
| ≥ 0.95 | High confidence branch | `reg.qa.repair_verified` |
| 0.70–0.94 | Medium confidence branch | `reg.qa.repair_exhausted` |
| < 0.70 | Low confidence branch | `reg.qa.repair_exhausted` |
| Classifier unavailable | `classifier_unavailable` branch | `reg.qa.repair_exhausted` |

### 10.2 Regulation QA Spans

| Span | Meaning | Emitted |
|------|---------|---------|
| `reg.qa.repair_attempted` | QA script started | ✅ On every `run_script()` call |
| `reg.qa.repair_verified` | Script completed successfully | ✅ On PASS terminal |
| `reg.qa.repair_exhausted` | Script failed or errored | ✅ On FAIL/WARN/error |
| `reg.qa.mutant_survived` | Test suite has a gap | 🔜 Planned |

### 10.3 Architecture

```
kask qa run --script <manifest.yaml>
    │
    ▼
hkask-test-harness/src/qa_script.rs
├── parse YAML manifest
├── validate branch targets
├── execute run_command steps (shell, 5-min timeout)
├── execute classify steps (Gemma 4 26B via DeepInfra)
├── execute loop steps (retry with max_iterations guard)
├── execute mcp_tool steps (stub — routes to failure)
├── enforce gas budget (hard_limit)
└── emit Regulation spans (start/complete/error)
```

### 10.4 Executable Manifests

Four manifests run today via `cargo test -p hkask-test-harness -- qa_script`:

| Manifest | Crate Tested | Tests |
|----------|-------------|-------|
| `qa-comm-integration-gate` | `hkask-mcp-communication` | 5 |
| `qa-condenser-health-check` | `hkask-mcp-condenser` | 11 |
| `qa-keystore-security-gate` | `hkask-keystore` | 16 |
| `qa-memory-privacy-boundary` | `hkask-mcp-memory` | 6 |

Without `DI_API_KEY`, classify steps gracefully degrade through `classifier_unavailable`.

### 10.5 Implementation Components

| Component | Location | Responsibility |
|-----------|----------|----------------|
| QA script runner | `crates/hkask-test-harness/src/qa_script.rs` | Manifest parsing, step execution, gas, Regulation |
| CLI subcommand | `crates/hkask-cli/src/commands/qa.rs` | `kask qa run --script`, `kask qa list` |
| Regulation QA spans | `crates/hkask-types/src/regulation.rs` | 4 `RegulationSpan` variants |
| Classifier config | `registry/classify/qa-triage.yaml` | Canonical classifier triage prompt (HKASK_CLASSIFIER_MODEL) |
| QA manifests | `registry/manifests/qa-*.yaml` | 9 manifests (4 executable, 5 planned) |

### 10.6 Planned (not yet built)

- MCP tool dispatch — call MCP server tools from QA manifests (callback infrastructure ready)
- Cross-crate invariant manifest — verify invariants spanning multiple crates

### 10.7 Anti-Patterns

| Anti-pattern | Why Avoided |
|-------------|-------------|
| No `#[contract]` annotations | Removed — suffocated the code |
| No pre/post/invariant DSL | Same reason as above |
| No new model deployment | Uses existing Gemma 4 26B via DeepInfra API |
| No new binary | `kask qa run` is a CLI subcommand |
| No visual QA dashboard | P3 Prohibition #1 — Regulation spans + CLI only |
| No auto-merge to main | P1 User Sovereignty — human always reviews the PR |

## 11. Updated Test Pyramid

| Layer | What | Verification |
|-------|------|-------------|
| **Unit** | Single function's behavior | Proptest on the function directly |
| **Integration** | Cross-function chains | Proptest on the entry point; Regulation spans verify called functions' behavior |
| **State machine** | Invariants across operation sequences | Proptest on operation sequences; Regulation `reg.gas` spans track budget invariants |
| **Fuzz** | Input surface robustness | Fuzz seed corpora (cli_fuzz_seeds, json_fuzz_seeds); verifies no panic |
| **Triage** | Failure diagnosis | LLM classifier (Gemma 4 26B); routes by confidence via QA manifests |
| **System** | End-to-end workflows | Integration tracer bullet (TDD skill); verifies full vertical slice |

---

## References

### Property-Based Testing

- Claessen, K. & Hughes, J. (2000). "QuickCheck: A Lightweight Tool for Random Testing of Haskell Programs." *ICFP.*
- MacIver, D. (2019). "Property-Based Testing: What Is It?" The theoretical basis for PBT.

### Cybernetic Foundations
