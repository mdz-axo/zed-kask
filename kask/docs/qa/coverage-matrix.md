---
title: "hKask MCP Server QA Coverage Matrix"
audience: [QA engineers, agents]
last_updated: 2026-08-05
version: "0.3.3"
status: "Active"
domain: "trust"
mds_categories: [trust, composition, lifecycle]
---

# hKask MCP Server QA Coverage Matrix

Rewritten 2026-08-05 against the actual `tests/` directories across all 12
servers in `kask/mcp-servers/`. The prior version of this document claimed
"4 of 10 servers with contract tests" citing `qa_contract.rs` for codegraph
and condenser — that was **false**: only `hkask-mcp-curator` and
`hkask-mcp-kata-kanban` have `tests/qa_contract.rs` (2 of 12). Codegraph and
condenser have `schema_compliance.rs` / `edge_resolution.rs` only; their
stub-based `qa_contract.rs` files were deleted in favor of schema-compliance
and property tests. `hkask-mcp-swarm` and `hkask-mcp-prediction-markets`
were absent from the matrix entirely and are included below.

Tool counts verified 2026-08-05 via `grep -c '#\[tool('` over each server's
`src/` tree (rmcp `#[tool(description = ...)]` attribute — the
`#[rmcp::tool_handler]` entry point is not counted).

## Per-server coverage (actual `tests/` contents)

| Server | Tools | `tests/` contents | `#[cfg(test)]` modules in `src/` | Verdict |
|---|---|---|---|---|
| hkask-mcp-codegraph | 9 | `edge_resolution.rs` (integration: cross-file call-graph topology), `schema_compliance.rs` (AnyJsonValue schema scan) | 12 | moderate inline + integration; no per-tool QA contract |
| hkask-mcp-companies | 42 | — (no `tests/` dir) | 22 | moderate inline only; **no integration/contract coverage** |
| hkask-mcp-condenser | 4 | `schema_compliance.rs` | 0 | **gap — no inline tests**; schema-compliance only |
| hkask-mcp-corpus | 27 | `compose_contract.rs` (proptest: cosine distance), `compose_contract.proptest-regressions`, `corpus_config_parse_test.rs` (operator YAML parses), `corpus_properties.rs` (proptest: pure `ModelInfo` conversions), `gentle_lovelace_corpus_test.rs` (EmbedService corpus parsing), `schema_compliance.rs` | 27 | moderate inline + property tests; no per-tool QA contract |
| hkask-mcp-curator | 8 | `qa_contract.rs` (7-category per-tool contract), `schema_compliance.rs` | 2 | **qa_contract.rs present**; minimal inline |
| hkask-mcp-kata-kanban | 18 | `qa_contract.rs` (7-category per-tool contract), `schema_compliance.rs` | 5 | **qa_contract.rs covers all 18 tools** |
| hkask-mcp-media | 42 | `schema_compliance.rs` | 8 | low inline + schema-compliance; no per-tool QA contract |
| hkask-mcp-prediction-markets | 13 | `calibration.rs` (calibration store + reading), `cmp.rs` (CMP construction), `market_lookup.rs` (annotated MarketRecord contract), `market_match.rs` (event↔market matcher), `provider_kalshi.rs`, `provider_polymarket.rs` (live-captured fixture tests), `residual.rs` (residual decomposition), `fixtures/` (kalshi_events.json, kalshi_markets.json, polymarket_events.json) | 4 | good component/fixture coverage; no per-tool QA contract |
| hkask-mcp-research | 17 | `research_contract.rs` (HTML stripping, cache ops, request types) | 6 | low; contract-adjacent but not the 7-category matrix |
| hkask-mcp-scenarios | 21 | `market_consumer.rs` (zero-edit prediction-market consumption seams), `scenarios_contract.rs` (event-tree forecasting invariants), `schema_compliance.rs` | 3 | minimal inline; contract-adjacent |
| hkask-mcp-swarm | 51 | `schema_compliance.rs`, `swarm_gap_properties.rs` (proptest over `pub(crate)` helpers via `test-utils`), `swarm_properties.rs` (proptest over the public surface) | 16 | good property coverage; no per-tool QA contract |
| hkask-mcp-training | 8 | `schema_compliance.rs` | 14 | moderate inline + schema-compliance; no per-tool QA contract |

**Fleet total: 260 tools across 12 servers.**

## Servers with the 7-category QA contract (`tests/qa_contract.rs`)

| server | tools covered | status | evidence |
|---|---|---|---|
| hkask-mcp-curator | 8 | pass | `tests/qa_contract.rs` |
| hkask-mcp-kata-kanban | 18 | pass | `tests/qa_contract.rs` |

## Servers without the 7-category QA contract

| server | tools | nearest existing coverage | status |
|---|---|---|---|
| hkask-mcp-codegraph | 9 | `edge_resolution.rs`, `schema_compliance.rs` | gap |
| hkask-mcp-companies | 42 | inline `#[cfg(test)]` only — no `tests/` dir | gap |
| hkask-mcp-condenser | 4 | `schema_compliance.rs` | gap |
| hkask-mcp-corpus | 27 | property + parsing tests | gap |
| hkask-mcp-media | 42 | `schema_compliance.rs` | gap |
| hkask-mcp-prediction-markets | 13 | component + fixture tests | gap |
| hkask-mcp-research | 17 | `research_contract.rs` | gap |
| hkask-mcp-scenarios | 21 | `scenarios_contract.rs`, `market_consumer.rs`, `schema_compliance.rs` | gap |
| hkask-mcp-swarm | 51 | property tests, `schema_compliance.rs` | gap |
| hkask-mcp-training | 8 | `schema_compliance.rs` | gap |

## Summary

- **Servers with the 7-category QA contract (`tests/qa_contract.rs`):** 2 of 12 (curator, kata-kanban)
- **Servers without:** 10 of 12 (codegraph, companies, condenser, corpus, media, prediction-markets, research, scenarios, swarm, training)
- **Convergence:** not reached — 10 servers have no `tests/qa_contract.rs` file.
- **Correction (2026-08-05):** the earlier "4 of 10 servers" claim miscited `schema_compliance.rs`/`edge_resolution.rs` as contract tests for codegraph and condenser. Those files exist but are schema/topology tests, not the 7-category contract. The deleted stub-based `qa_contract.rs` files were replaced by design; the replacement coverage is narrower than the contract matrix assumes.
- **Next step:** write `qa_contract.rs` for the 10 missing servers per `mcp-server-qa-strategy.md` Phase 4 Deliverable 2. For prediction-markets, the existing fixture/component tests already cover much of Categories 4–5; the missing cells are the schema-violation and adversarial categories per tool.
