# Minimalist Refactor — `EnergyEstimator` trait consolidation

<!--
Task: Minimalist refactor of `{{ target_codebase }}` — resolved to a concrete
target after read-only exploration of the kask/ tree.

Target:        `EnergyEstimator` trait in `kask/crates/hkask-regulation/src/energy_estimator.rs`
Test oracle:   `cargo test -p hkask-regulation` (92 tests) + `cargo test -p hkask-mcp` (10 tests)
Acceptance:    Both oracles green end-to-end after the slice; no OUGHT claims about preserved behavior.
Requirements:  DIVERGENCE.md §13.1 (hKask crates never depend on zed-kask crates);
               `.rules` "Trait-with-one-impl is speculative generality" trap.
-->

## Target selection rationale

Read-only exploration of the kask/ tree (19 crates + 10 MCP servers) verified
that the easy consolidation targets were already folded (DIVERGENCE.md:
"hkask-services-{context,corpus,compose,inference,kata-kanban,runtime} were
folded into their sole MCP server consumers"). Every small crate checked
(`hkask-forecast`, `hkask-email`, `hkask-bridge-dublincore`, `hkask-lisp`,
`hkask-ledger`, `hkask-services-core`) has real cross-crate consumers. The
`AdapterPort`/`AdapterRouter` dead code named in `.rules` was already removed
(confirmed by comments in `hkask-mcp-training`).

A grep-based "trait + 1 impl in same crate" signal initially flagged
`hkask-capability` (`ToolPort`, `TokenRegistry`), but verification across the
whole kask/ tree showed both traits have multiple cross-crate implementors
(`McpRuntime`, `StubToolPort`, `NoopToolPort`, `BridgeToolPort` for
`ToolPort`; `NoOpTokenRegistry`, `TokenRegistryStore` for `TokenRegistry`).
This is the `.rules` "Convention priors drawn from .rules must be verified
against the codebase" trap — the in-crate-only grep missed cross-crate impls.

The surviving candidate is `EnergyEstimator` in
`kask/crates/hkask-regulation/src/energy_estimator.rs` (8 LOC, 1 method):
- Single implementor: `FlatEnergyEstimator` in `kask/crates/hkask-mcp/src/runtime.rs`
- Single composition-root call site: `crates/zed/src/main.rs:679` constructs
  `FlatEnergyEstimator::new()` and passes it as `Arc<dyn EnergyEstimator>`
- Single internal call site: `runtime.rs:522` calls `est.estimate_cost(...)`
- No test uses `with_governance` or `FlatEnergyEstimator` (hkask-mcp tests
  use `McpRuntime::new()` without governance)
- `hkask-types/src/ports/regulation.rs:65` doc references
  `CalibratedEnergyEstimator` and `WalletGasCalibrator` as hypothetical
  future implementors that do not exist

This matches the `.rules` "Trait-with-one-impl is speculative generality"
trap exactly: "A trait with a single implementor... is dead code regardless
of ADR-042 'port promotion' aspirations. The port promotion rule says a
port *moves* to a shared crate when a second consumer materializes — it
does not justify creating the port before the first consumer exists."

## Slice 1 — Deletion test on `EnergyEstimator`

**Verdict target:** `remove` — collapse the trait + `Arc<dyn>` indirection
into the concrete `FlatEnergyEstimator` struct held directly by `McpRuntime`.

**Deletion test (essentialist G1):** Delete the `EnergyEstimator` trait.
Does the complexity it existed to absorb reappear in callers?
- The trait abstracts "estimate tool gas cost before invocation".
- The only implementor returns a flat constant (10 gas).
- The only call site is `McpRuntime::invoke` (runtime.rs:522).
- Replacing `Arc<dyn EnergyEstimator>` with `FlatEnergyEstimator` (a
  `Copy` struct with one `u64` field) removes the dyn-dispatch indirection
  without reappearing complexity. The "estimate cost" operation is a single
  field read; no caller needs the polymorphism.
- Verdict: complexity does NOT reappear. Trait fails the deletion test → remove.

**Surface assessment (essentialist G2):** After removal, `McpRuntime` holds
`FlatEnergyEstimator` directly (one `u64` field). `with_governance` signature
changes from `estimator: Arc<dyn EnergyEstimator>` to `estimator:
FlatEnergyEstimator`. The composition root passes `FlatEnergyEstimator::new()`
directly. Surface narrows. Passes.

**Contract assessment (essentialist G3):** The trait's contract is "given
(server, tool, args), return a u64 gas estimate". The concrete struct's
contract is identical (returns `self.cost`). No contract is weakened; the
trait's advertised polymorphism was never exercised. Passes.

**Behavior preservation:** `FlatEnergyEstimator::estimate_cost` returns
`self.cost` (default 10). The trait method returns the same. The
`McpRuntime::invoke` path that reads `est.estimate_cost(server, tool, &args)`
gets the same value. No test asserts the trait object specifically; tests
use `McpRuntime::new()` (no governance). The composition root is the only
site that wires governance, and it passes `FlatEnergyEstimator::new()`.

**OUGHT claims to promote to IS:**
1. OUGHT: "No test asserts the trait object specifically" → IS: verified by
   grep — `hkask-mcp/tests/` has zero references to `with_governance`,
   `FlatEnergyEstimator`, or `EnergyEstimator`.
2. OUGHT: "The composition root is the only site that wires governance" →
   IS: verified by grep — single `.with_governance(` call site in `crates/zed/src/main.rs:679`.

## Failure modes (per task spec)

- Deletion test fails (complexity reappears): keep, record as escape-hatch.
- Test suite goes silent: revert, investigate.
- OUGHT cannot be promoted to IS within 9 iterations: revert, mark blocked.

## Out of scope

- The `CalibratedEnergyEstimator` / `WalletGasCalibrator` referenced in
  `hkask-types/src/ports/regulation.rs:65` doc comment do not exist. If/when
  a second implementor materializes, the port can be re-introduced at that
  point (port promotion per ADR-042). Removing the trait now does NOT prevent
  re-introduction — it removes speculative generality that has no consumer.
- The doc comment in `hkask-types/src/ports/regulation.rs:65` mentions
  `CalibratedEnergyEstimator` and `WalletGasCalibrator` as examples of "the
  cybernetic regulation layer". This is a doc reference to hypothetical
  components, not a code dependency. Updating the doc comment to remove the
  stale references is in-scope for this slice (same slice, because the doc
  references the trait being removed).