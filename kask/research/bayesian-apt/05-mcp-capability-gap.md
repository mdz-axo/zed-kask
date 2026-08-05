# MCP Capability Gap Report

**Method:** capabilities-reasoner — register the four MCP servers' actual
capabilities against a typed registry with floor/ceiling/maturity-gate
limits; elicit capability (not just observed behavior); evaluate; report the
gap to the target foundation. Grounded in actual source reading, not server
descriptions alone.

---

## 1. Capability registry (typed)

| Capability type | Floor (must have) | Ceiling (target) | Maturity gate |
|---|---|---|---|
| `MarketIngest` | Ingest ≥1 market with probability + deadline | Ingest all relevant markets for a company's environment, normalized | Polymarket + Kalshi both live |
| `MarketAnnotate` | Reliability tier + structural flag | Calibration-adjusted probability + duration | Brier data accruing |
| `ScenarioCompose` | Build a single event from one market | Compose N markets into one event tree with conditional tables | `EventDependency` algebra correct |
| `ScenarioProbability` | Marginal probability via topo order | Full joint distribution per branch + variance contribution | Parent-independence assumption documented |
| `CompanyForecast` | DCF + RIM with fade horizon | Probabilistic forecast envelope from scenario tree | Continuous duration |
| `DurationModel` | Categorical fade (5/10/20y) | Continuous equity + market duration, matched | Ratio test (H2) |
| `RiskCore` | `variance_contribution` proxy | σ_scenario + factor loadings + volatility fusion | Out-of-sample test (H1) |
| `EquilibriumFraming` | Absent | Tâtonnement-style convergence feedback loop | Stability test (H5) |
| `ResearchGrounding` | Web search + arxiv provider | Citation-gated extraction into scenario events | Citation gate enforced |

---

## 2. Per-server assessment

### 2.1 `hkask-mcp-prediction-markets`

**Observed capabilities (from source):**
- `MarketIngest`: **Floor met, ceiling partial.** Polymarket + Kalshi both
  live (`provider_polymarket.rs`, `provider_kalshi.rs`). Unified
  `MarketRecord` with 20+ fields. *Gap:* no composition into trees; ingests
  markets one at a time.
- `MarketAnnotate`: **Floor met, ceiling partial.** `reliability_tier`
  (High/Medium/Low) from volume/spread/calibration; `structural_flag`
  (NearDeadline/NearCoinflip); `calibration` (Brier, sample_size, stale,
  domain_bias). *Gap:* no `duration` field; no continuous calibration
  adjustment (domain_bias is a static string, not a numeric correction).
- `MarketMatch`: **Floor met.** `market_match` matches natural-language
  questions to markets; `market_ontology_map` maps to PKO. *Gap:* no
  matching of *sets* of markets to a company's risk surface.

**Elicited capability (not just observed):** The `assemble` function is the
single annotation seam — both providers route through it. Adding a `duration`
field there propagates to all records automatically. This is a high-leverage
extension point.

**Deep-module deletion test:** Delete the prediction-markets server → the
foundation loses its market-implied probability feed; scenario trees must be
built from analyst estimates only (slower, less calibrated). Complexity
reappears as *manual probability elicitation*. → Earns its place.

**Gap to target:** Add `duration`; add `market_compose` (set-of-markets →
scenario events); promote `domain_bias` from string to numeric correction.

### 2.2 `hkask-mcp-scenarios`

**Observed capabilities (from source):**
- `ScenarioCompose`: **Floor met, ceiling partial.** `scenario_build`
  emits an LLM prompt producing `ScenarioEvent` arrays with
  `depends_on`/`conditionals`. `scenario_from_markets` bridges from a single
  market. *Gap:* no `scenario_from_markets_set` (compose many markets);
  composition is LLM-mediated, not algebraic.
- `ScenarioProbability`: **Ceiling met (for the tree).**
  `compute_marginal_probabilities` does full joint conditional-table
  marginalization in topo order; `EventTree` carries `joint_probability` and
  per-node `variance_contribution`. *Gap:* no per-branch joint probability
  (only the all-events-occur proxy); no company-return-per-branch.
- `ScenarioCalibrate`: **Floor met.** Brier scoring, calibration curves,
  Bayesian update, dragonfly-eye synthesis, cross-validation. *Gap:* no
  link from calibration back to factor-exposure adjustment.

**Elicited capability:** The `EventDependency` conditional-table algebra
(bitmap-indexed `conditionals`, length 2^num_parents) is the *exact*
structure Bhattacharya's belief-hierarchy recursion needs. The recursion is
a tree of conditional tables; this is a tree of conditional tables. The
elicited capability is "Bayesian belief-hierarchy host" — not currently
exercised as such, but structurally present.

**Deep-module deletion test:** Delete the scenarios server → the foundation
loses its event-tree algebra; risk reduces to historical volatility; H1, H3
untestable. → Earns its place (load-bearing).

**Gap to target:** Add `scenario_from_markets_set`; add per-branch joint
probability; add `scenario_factor_loadings`; add `scenario_risk_measure`.

### 2.3 `hkask-mcp-companies`

**Observed capabilities (from source):**
- `CompanyForecast`: **Floor met, ceiling partial.** DCF (two-stage, Gordon
  terminal) + RIM (Bergen et al. 2025) with fade horizons. `StoredForecast`
  persists. *Gap:* deterministic — no probabilistic envelope; no
  scenario-conditional re-evaluation.
- `DurationModel`: **Floor met (categorical), ceiling unmet (continuous).**
  `FadeHorizon` ∈ {5,10,20y}. *Gap:* no continuous duration; no
  market-duration matching.
- `Learning`: **Floor met.** Learning loop records forecast outcomes and
  adjusts. *Gap:* no link to scenario-tree calibration.

**Elicited capability:** The RIM's `EP_t = (ROIC − WACC) × IC_t` per-year
decomposition is the *natural feed* for a continuous duration: the duration
is the weighted average of the *t* values, weighted by `EP_t / (1+r)^t`. The
data is already computed; the duration is a byproduct not yet exposed.

**Deep-module deletion test:** Delete the companies server → the foundation
has no equity cash flows to duration-match; the maturity-transformation
thesis (H2) is untestable. → Earns its place.

**Gap to target:** Add continuous `duration` to valuation output; add
`forecast_envelope` (probabilistic); add `branch_return` (re-evaluate
DCF/RIM under a scenario branch).

### 2.4 `hkask-mcp-research`

**Observed capabilities (from source):**
- `ResearchGrounding`: **Floor met, ceiling partial.** Web search (brave,
  tavily, exa, serapi, firecrawl, browserbase, raw_fetch), arxiv provider,
  semantic_scholar, RSS suite, web_extract, web_browse. *Gap:* no
  citation-gated extraction *into* scenario events (the extraction is
  general-purpose; no structured handoff to `scenario_research`).
- `ArxivProvider`: **Floor met.** The Bhattacharya paper (2211.03244) is
  reachable. *Gap:* no PDF OCR fallback in the research server itself (that
  lives in the corpus server); research server fetches metadata + abstract.

**Elicited capability:** The research server is the *grounding* layer for
the belief hierarchy — it fetches the literature that justifies each
scenario event's probability. The elicited capability is "citation gate
enforcer," not currently wired as such.

**Deep-module deletion test:** Delete the research server → scenario events
are ungrounded LLM guesses; the citation gate fails; H5 (LLM enables new
analyses) becomes unfalsifiable (no baseline to compare against). → Earns
its place.

**Gap to target:** Add `research_to_scenario_events` (structured handoff);
enforce citation gate (every scenario event's `basis` must point to a
research-server-extracted source or be labeled `hypothesis`).

---

## 3. Aggregate gap summary

| Capability | Current | Target | Gap size |
|---|---|---|---|
| Market composition | 1 market → 1 event | N markets → 1 tree | Large (new algebra) |
| Market duration | Absent | Continuous | Small (one field + formula) |
| Equity duration | Categorical | Continuous | Small (expose existing computation) |
| Duration matching | Absent | Ratio + transformation weight | Small (one function) |
| Per-branch probability | All-events proxy | Per-branch joint | Medium (extend EventTree) |
| Company return per branch | Absent | DCF/RIM re-eval per branch | Medium (loop over branches) |
| Factor loadings | Absent | β(c, b) per branch | Medium (APT bridge) |
| Risk fusion | `variance_contribution` | σ_scenario + volatility fusion | Large (the risk core) |
| Equilibrium framing | Absent | Tâtonnement feedback loop | Large (design + validation) |
| Citation gate | Manual | Enforced in `basis` field | Small (schema + validation) |

**High-leverage extension points (small gap, large payoff):**
1. `duration` field on `MarketRecord` (one field, unlocks H2).
2. Continuous duration on valuation output (expose existing RIM loop, unlocks H2).
3. `basis` citation-gate validation (schema check, unlocks H5 falsifiability).

**Large gaps requiring new work:**
1. Market composition algebra (N markets → tree).
2. The risk core (σ_scenario + factor loadings + fusion).
3. Equilibrium framing (tâtonnement feedback loop).
