# hkask-forecast

Shared superforecasting computation engine implementing Tetlock's Good Judgment Project methodology.

## Purpose

Pure-math superforecasting pipeline with no MCP or server dependencies. Canonical implementations used by both `hkask-mcp-scenarios` and `hkask-mcp-companies`.

## Pipeline

1. **Fermi decomposition** — confidence-weighted sub-question averaging
2. **Outside view** — base rate calibration with shrinkage estimator
3. **Bayesian updating** — P(H|E) = P(E|H) × P(H) / P(E)
4. **Brier scoring** — (prediction - outcome)²

## Dependencies

- `thiserror` — error types

No hKask crate dependencies — this is a standalone computation library.