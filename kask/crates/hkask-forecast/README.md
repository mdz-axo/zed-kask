# hkask-forecast

Shared superforecasting computation engine implementing Tetlock's Good Judgment Project methodology.

## Purpose

Pure-math superforecasting pipeline with no MCP or server dependencies. Canonical implementations used by the domain MCP servers (`hkask-mcp-scenarios`, `hkask-mcp-prediction-markets`).

## Pipeline

1. **Fermi decomposition** — confidence-weighted sub-question averaging
2. **Outside view** — base rate calibration with shrinkage estimator
3. **Bayesian updating** — P(H|E) = P(E|H) × P(H) / P(E)
4. **Brier scoring** — binary `mean((prediction - outcome)²)`, range [0,1]

Raw loss is not a calibration or usefulness certificate. Compare forecasts only
on matched cases, horizons and weights against a declared reference. This crate
contains arithmetic, not domain admission or resolution policy. A constant .5
forecast has binary loss .25; that is not a universal climatology or quality
threshold. Isotonic fitting remains a descriptive mathematical primitive, not an
automatically deployed correction.

## Dependencies

- `thiserror` — error types

No hKask crate dependencies — this is a standalone computation library.