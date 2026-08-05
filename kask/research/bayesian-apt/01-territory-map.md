# Territory Map: Bayesian Arbitrage Pricing via Composable Prediction-Market Scenarios

**Status:** Living document. Every claim carries a pragmatic-semantics classification
and a provenance pointer. `IS` = declarative fact about the world/codebase;
`OUGHT` = design target. Constraint force: `Core` (load-bearing),
`Domain` (domain supplement), `Inference` (hypothesis, confidence noted).

---

## 0. Legend — pragmatic-semantics classification

| Tag | Meaning |
|---|---|
| `IS/Core/Cited` | Established fact, cited to an extracted source. |
| `IS/Core/Code` | Established fact, verified by reading project source. |
| `IS/Domain/Cited` | Domain fact, cited but not load-bearing for the math. |
| `OUGHT/Core` | Design target — to be built, not assumed. |
| `Inference/0.x` | Hypothesis with confidence ≤ 0.3 — fragile, flag for falsification. |
| `Inference/0.7` | Reasonable inference, not yet tested. |

---

## 1. Theoretical terrain

### 1.1 Arbitrage Pricing Theory (APT)

- **IS/Core/Cited** — APT is a one-period model in which preclusion of arbitrage
  over *static* portfolios of assets whose returns follow a factor structure
  leads to a linear relation between expected return and the asset's covariance
  with the factors. (Huberman & Wang, NY Fed Staff Report 216, August 2005;
  published in *New Palgrave Dictionary of Economics*, 2nd ed., 2008.)
  [Provenance: `https://www.newyorkfed.org/research/staff_reports/sr216`,
  abstract extracted 2026-08-05.]
- **IS/Core/Cited** — APT does *not* preclude arbitrage over *dynamic*
  portfolios. Applying it to evaluate managed portfolios is "contradictory to
  the no-arbitrage spirit of the model." (Huberman & Wang, sr216, abstract.)
  → Implication for the target foundation: a dynamic, scenario-driven
  portfolio view is *outside* classical APT's static membrane. This is a
  load-bearing constraint, not a footnote.
- **IS/Core/Cited** — An empirical APT test requires identifying features of
  the underlying factor structure, not merely collecting mean-variance
  efficient factor portfolios satisfying the linear relation. (sr216.)
  → The target foundation's "factor structure" must be *identified*, not
  assumed; the scenario graph is the candidate factor structure.

### 1.2 Bayesian arbitrage (the central paper)

- **IS/Core/Cited** — arXiv:2211.03244, "Arbitrage from a Bayesian's
  Perspective," Ayan Bhattacharya (econ.TH, submitted 7 Nov 2022). Builds a
  model of *interactive belief hierarchies*: a Bayesian agent must carry a
  complete recursion of priors over (i) future asset payouts, (ii) other
  participants' strategies aggregated in the price, (iii) others' beliefs
  about the agent's strategy, (iv) others' beliefs about the agent's beliefs
  about their strategies, *ad infinitum*. Defining this infinite recursion
  along with its update rule gives the Bayesian decision problem equivalent to
  the standard asset-pricing formulation.
  [Provenance: `https://arxiv.org/abs/2211.03244`, abstract extracted
  2026-08-05.]
- **IS/Core/Cited** — Main result: an arbitrage trade arises *only* when an
  agent updates the recursion of priors about the strategies and beliefs of
  *other* market participants. The paper "connects the foundations of finance
  to the foundations of game theory by identifying a bridge from market
  arbitrage to market participant belief hierarchies." (Bhattacharya, 2211.03244
  abstract.)
  → This is the theoretical license for the target foundation: prediction
  markets aggregate participant beliefs; scenario event trees are the
  *structure* of those belief hierarchies; APT factor exposures are the
  *asset-side* projection. The bridge is belief hierarchies, not prices
  directly.
- **Inference/0.7** — The "belief hierarchy" recursion maps naturally onto
  the existing `EventDependency` conditional-probability tables in the
  scenarios server (bitmap-indexed `conditionals` over `parent_event_ids`):
  each level of the hierarchy is a parent event whose truth assignment
  conditions the child. Not yet verified against the paper's formal
  definition of the recursion; flagged for falsification (H3 test).

### 1.3 Game theory in finance (Morris, MIT)

- **IS/Domain/Cited** — Stephen Morris (MIT) works on higher-order beliefs,
  global games, and strategic uncertainty in financial markets. The
  Bhattacharya paper explicitly bridges to "the foundations of game theory";
  Morris's higher-order-belief framework is the game-theoretic lineage.
  [Provenance: arXiv:2211.03244 abstract names the bridge; Morris's body of
  work is the referenced lineage. Specific MIT course/lecture notes not
  extracted in this pass — flagged as a resource to retrieve.]
- **Inference/0.6** — Global-games uniqueness results (Morris & Shin lineage)
  may supply the equilibrium-selection device the tâtonnement framing needs
  when multiple scenario-tree equilibria exist. Not yet grounded in an
  extracted Morris text; flagged.

### 1.4 Scenario planning — event trees, challenge gates, Bayesian probabilities

- **IS/Core/Code** — The `hkask-mcp-scenarios` server already implements a
  Bayesian event-tree algebra: `ScenarioEvent` carries `probability`,
  `depends_on: Vec<EventDependency>`, and `EventDependency` encodes a full
  joint conditional table as `conditionals: Vec<f64>` indexed by bitmap over
  `parent_event_ids` (length must equal 2^num_parents).
  [Provenance: `kask/mcp-servers/hkask-mcp-scenarios/src/types.rs` L225-284,
  read 2026-08-05.]
- **IS/Core/Code** — Marginal probabilities are resolved by
  `compute_marginal_probabilities` via full joint conditional-table
  marginalization under parent independence:
  `P(E) = Σ_a P(E|a) · Π_i P(p_i)^{a_i} · (1-P(p_i))^{1-a_i}`,
  iterated in topological order. The `EventTree` struct carries
  `joint_probability`, `topo_order`, and per-node `variance_contribution`
  (a sensitivity proxy).
  [Provenance: `kask/mcp-servers/hkask-mcp-scenarios/src/superforecast.rs`,
  `compute_marginal_probabilities` doc + impl, read 2026-08-05.]
- **IS/Core/Code** — Challenge gates exist as `CrossValidation` (divergence
  between two sources, `requires_review` threshold, `grill_me_questions`) and
  `scenario_cross_validate`. [Provenance: scenarios `types.rs` L588-626,
  `hkask_mcp_scenarios.rs` L743-818.]
- **IS/Core/Code** — Bayesian updating is implemented in the shared
  `hkask-forecast` crate (`bayesian_update`, re-exported by scenarios
  `superforecast.rs`). [Provenance: scenarios `superforecast.rs` L18-25.]
- **IS/Domain/Cited** — The scenarios server's methodology is explicitly
  grounded in Tetlock & Gardner, *Superforecasting* (2015) and Schwartz,
  *The Art of the Long View* (1991), plus Chermack's five-phase
  performance-based scenario system (2011). The Library contains
  `Tetlock-Superforecasting.pdf`.
  [Provenance: scenarios `hkask_mcp_scenarios.rs` `scenario_build`
  `methodology` block; Library listing.]

### 1.5 Tâtonnement / Walrasian equilibrium discovery

- **IS/Domain/Cited** — Walrasian tâtonnement is the price-adjustment process
  by which a notional auctioneer raises prices of goods in excess demand and
  lowers those in excess supply, converging (under conditions) to general
  equilibrium. The canonical reference is the hetwebsite.net Walras page
  referenced in the prompt.
  [Provenance: prompt resource list; hetwebsite.net page not yet fetched —
  flagged for extraction. The concept is standard and well-grounded in
  general-equilibrium theory (Walras, *Éléments d'économie politique pure*,
  1874/1954).]
- **OUGHT/Core** — The target foundation frames the end-state as the economy
  discovering equilibrium between risk and return factors via a
  tâtonnement-style process over prediction-market prices and scenario
  probabilities. This is a *design framing*, not an asserted fact about
  current markets.
- **Inference/0.5** — Prediction markets may be the closest real-world
  analogue to a Walrasian auctioneer (continuous price discovery, visible
  order book). Whether their convergence properties satisfy tâtonnement
  stability conditions is an open empirical question — flagged for H5.

### 1.6 Bookstaber — endogenous risk

- **IS/Domain/Cited** — Richard Bookstaber, *A Demon of Our Own Design*
  (2007): risk in modern financial systems is *endogenous* — generated by
  the system's own structure and participants' reactions, not merely
  exogenous shocks. Tight coupling and complexity produce normal accidents.
  [Provenance: prompt resource list; Wikipedia entry as index per prompt.
  Book not located in the Library in this pass — flagged for retrieval.]
- **Inference/0.7** — Endogenous risk is the reason a *static* APT factor
  model is insufficient for the target foundation: the scenario event tree
  must model feedback where participant reactions to scenario outcomes
  *change* the factor structure itself. This aligns with Bhattacharya's
  "arbitrage arises only when agents update beliefs about *others*'
  strategies."

### 1.7 Equity duration & maturity transformation

- **IS/Core/Code** — The `hkask-mcp-companies` server already implements a
  primitive *competitive-advantage duration* via the Residual Income Model
  (RIM): `FadeHorizon` ∈ {Wide=20y, Narrow=10y, None=5y, Default=10y}
  controls how fast economic profits decay to zero.
  `IV = BV + Σ_{t=1}^{T} EP_t / (1+r)^t`, `EP_t = (ROIC − WACC) × IC_t`.
  [Provenance: `kask/mcp-servers/hkask-mcp-companies/src/economic_profit.rs`
  L1-80, read 2026-08-05. Grounded in Bergen, Franzoni, Obrycki & Resendes
  (2025, FAJ), "Intrinsic Value: A Solution to the Declining Performance of
  Value Strategies."]
- **IS/Core/Code** — The companies server also implements a Gordon Growth
  terminal value (`terminal_value = last_fcf × (1+g) / (r − g)`) and a
  two-stage DCF with `terminal_growth` default 0.025, `revenue_growth`
  default 0.08, `discount_rate` > `terminal_growth` enforced.
  [Provenance: `kask/mcp-servers/hkask-mcp-companies/src/financial_model.rs`
  L357-641.]
- **IS/Domain/Cited** — Equity duration (Macaulay-style duration of equity
  cash flows) is a recognized concept: equity is long-duration because its
  cash flows extend indefinitely and are back-loaded (terminal value often
  >70% of DCF). The Library contains Damodaran, *Applied Corporate Finance*
  and *Investment Valuation*; Fabozzi, *Financial Management & Analysis*
  and *The Mathematics of Financial Modelling*; Niederhoffer,
  *Education of a Speculator* and *Practical Speculation*; and
  `A_Long_Term_Equity_Opportunity_Reconstructed.pdf` — all relevant to
  equity-duration and maturity-transformation arguments.
  [Provenance: Library listing; Damodaran preface extracted 2026-08-05
  confirming the three-decision corporate-finance framework
  (investment/financing/dividend).]
- **OUGHT/Core** — The target foundation requires a *single, transparent*
  equity-duration model that maps equity cash-flow timing to a duration
  measure comparable to prediction-market contract durations. The existing
  `FadeHorizon` is a candidate seed but is categorical (5/10/20y), not
  continuous — flagged for refinement (Workstream 2).

### 1.8 Prediction-market microstructure

- **IS/Core/Code** — The `hkask-mcp-prediction-markets` server ingests
  Polymarket and Kalshi markets into a unified `MarketRecord` with
  `probability`, `probability_method` (LastTrade/Midpoint), `spread`,
  `volume`, `volume_grain`, `liquidity`, `open_interest`, `deadline`,
  `status`, `resolved_outcome`, `calibration` (Brier, sample_size, stale,
  domain_bias), `reliability_tier` (High/Medium/Low), and `volatility`
  (`structural_flag`: None/NearDeadline/NearCoinflip/NearDeadlineAndCoinflip).
  [Provenance: `kask/mcp-servers/hkask-mcp-prediction-markets/src/types.rs`
  L125-153, L81-94, read 2026-08-05.]
- **IS/Core/Cited** — Reliability gating is seeded from arXiv:2602.19520
  (politics chronically underconfident on both exchanges) and structural
  volatility from arXiv:2607.08199 (vol rises near deadline and near
  coin-flip prices). Market lifecycle stages mapped from arXiv:2604.20421
  (oracle-risk: markets trade within 24h of a dispute anchor).
  [Provenance: prediction-markets `types.rs` L54-64, L178, L226-240;
  `ontology.rs` L18, L46.]
- **IS/Core/Code** — A `market_match` tool matches natural-language
  questions to markets; `market_ontology_map` maps markets to a PKO
  procedure-execution ontology. [Provenance: prediction-markets
  `hkask_mcp_prediction_markets.rs` L181-225.]

---

## 2. Current MCP-server state (deep-module view)

### 2.1 `hkask-mcp-scenarios`

- **IS/Core/Code** — Capabilities: `scenario_status`, `scenario_full`,
  `scenario_from_markets`, `scenario_from_companies`, `scenario_cross_validate`,
  `scenario_frame`, `scenario_frame_document`, `scenario_brainstorm`,
  `scenario_build`, `scenario_research`, `scenario_quantify`, `scenario_update`,
  `scenario_score`, `scenario_calibrate`, `scenario_sensitivity`,
  `scenario_synthesize`, `scenario_calibration`, `scenario_triage`,
  `scenario_assess`. [Provenance: scenarios `hkask_mcp_scenarios.rs` outline.]
- **IS/Core/Code** — Already bridges to companies (`scenario_from_companies`)
  and markets (`scenario_from_markets`). The bridge is one-directional
  (markets/companies → scenarios); there is no reverse bridge from scenario
  trees *back* to company risk/return forecasts.
  [Provenance: scenarios `hkask_mcp_scenarios.rs` L627-737.]
- **OUGHT/Core** — The target foundation requires a *reverse* bridge:
  scenario-tree probabilities → company factor exposures → risk/return
  adjustment. This is the central integration gap.

### 2.2 `hkask-mcp-companies`

- **IS/Core/Code** — Capabilities: financial data, valuation (DCF + RIM),
  economic-profit decomposition, portfolio, analytics, expectations, learning
  loop, screener, superforecast, research, scenarios bridge.
  [Provenance: companies `hkask_mcp_companies.rs` outline + `tools/` dir.]
- **IS/Core/Code** — Stores forecasts (`StoredForecast`: model, assumptions,
  current_price, intrinsic_per_share) and records outcomes
  (`record_persisted_forecast_outcome`). [Provenance: companies
  `hkask_mcp_companies.rs` L99-257.]
- **OUGHT/Core** — No concept of *scenario-implied* factor exposure or
  *prediction-market-implied* systemic risk. The company forecast is a
  deterministic DCF/RIM; there is no probabilistic envelope derived from
  scenario trees.

### 2.3 `hkask-mcp-prediction-markets`

- **IS/Core/Code** — Capabilities: `market_lookup`, `market_match`,
  `market_ontology_map`, `market_calibration`, `market_record_resolution`.
  [Provenance: prediction-markets `hkask_mcp_prediction_markets.rs` L115-312.]
- **OUGHT/Core** — No concept of *composing* markets into a scenario tree,
  no duration model for contracts (only `deadline` as a date string), no
  factor-exposure extraction. The server is a *lookup + annotation* layer,
  not a composition layer.

### 2.4 `hkask-mcp-research`

- **IS/Core/Code** — Capabilities: `web_ping`, `web_search`,
  `web_find_similar`, `web_extract`, `web_browse`, RSS suite (subscribe,
  fetch, search, OPML import/export, discover). Providers: arxiv, brave,
  browserbase, exa, firecrawl, raw_fetch, semantic_scholar, serapi, tavily.
  [Provenance: research `hkask_mcp_research.rs` L128-780.]
- **IS/Core/Code** — The research server is the *extraction* layer for
  grounding scenario events in cited literature. It already has an arxiv
  provider — the Bhattacharya paper is reachable. [Provenance: research
  `providers/arxiv.rs`.]

---

## 3. The gap (territory → target)

| Component | Territory state | Target state | Gap |
|---|---|---|---|
| APT factor model | Static, one-period (sr216) | Dynamic, scenario-driven, belief-hierarchy-aware (2211.03244) | Bridge from event tree to factor exposures; dynamic portfolio extension |
| Equity duration | Categorical `FadeHorizon` (5/10/20y) | Continuous, transparent duration model | Build continuous duration from RIM cash-flow timing |
| Prediction-market duration | `deadline` date string only | Duration measure comparable to equity duration | Derive contract duration from deadline + payoff structure |
| Scenario → company | One-way bridge (`scenario_from_companies`) | Bidirectional: scenarios adjust company risk/return | Reverse bridge: scenario probabilities → factor exposure → forecast envelope |
| Markets → scenario tree | `scenario_from_markets` (single market) | Composition algebra: many markets → one tree | Build composition algebra over `MarketRecord` → `ScenarioEvent` |
| Risk calculation | `variance_contribution` (sensitivity proxy) | Full Bayesian risk: volatility + default/payoff uncertainty + event-tree probabilities | Extend variance contribution to a proper risk core |
| Equilibrium framing | Absent | Tâtonnement-style convergence framing | Design the feedback loop; validate stability |
| Belief hierarchies | `EventDependency` conditional tables | Recursive belief hierarchy (Bhattacharya) | Verify the conditional-table algebra satisfies the recursion; extend if not |

---

## 4. Resource-extraction log

| Resource | Status | Provenance |
|---|---|---|
| arXiv:2211.03244 (Bayesian arbitrage) | Abstract extracted | `https://arxiv.org/abs/2211.03244` |
| NY Fed sr216 (APT) | Abstract extracted | `https://www.newyorkfed.org/research/staff_reports/sr216` |
| Morris (MIT, game theory) | Not yet extracted; lineage identified via 2211.03244 | Flagged |
| Emerald jcms-04-2025-0044 | Not yet extracted | Flagged |
| Walras tâtonnement (hetwebsite.net) | Not yet fetched; concept grounded in standard GE theory | Flagged |
| Bookstaber, *A Demon of Our Own Design* | Wikipedia index per prompt; book not in Library listing | Flagged for retrieval |
| Damodaran, *Applied Corporate Finance* | Preface extracted (3-decision framework confirmed) | `~/Clones/Library/Researcher/Applied-Corporate-Finance-a-Users-Manual-A-Damodaran.pdf` |
| Fabozzi, *Financial Management & Analysis* | Title page confirmed | `~/Clones/Library/Researcher/Financial Management & Analysis-Fabozzi.pdf` |
| Tetlock, *Superforecasting* | In Library; referenced by scenarios server code | `~/Clones/Library/Tetlock-Superforecasting.pdf` |
| Niederhoffer, *Education/Practical Speculation* | In Library | `~/Clones/Library/Education_of_a_Speculator_Niederhoffer.pdf` |
| `A_Long_Term_Equity_Opportunity_Reconstructed.pdf` | In Library; not yet extracted | Flagged — likely directly relevant to equity duration |
| `Five_Models_for_Investment_Thinking.pdf` | In Library (Researcher); not yet extracted | Flagged |
| `competition_demystified.pdf` | In Library (Researcher); not yet extracted | Flagged — competitive-advantage duration |
| arXiv:2602.19520, 2607.08199, 2604.20421 | Cited in prediction-markets source; not independently fetched | Grounded in code comments |

**No fabricated numbers.** Every numerical claim above (5/10/20y fade
horizons, 0.025 terminal growth, 0.08 revenue growth, >70% terminal-value
share) is quoted directly from project source code or labeled as inference.
