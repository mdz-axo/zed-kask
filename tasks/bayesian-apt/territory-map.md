---
dcterms:title: "Territory Map — Bayesian Arbitrage Pricing via Composable Prediction-Market Scenarios"
dcterms:creator: "zed-kask research architect agent"
dcterms:date: "2026-08-05"
rdf:type: bibo:Document
pko:procedure-target: "Foundation linking prediction markets, scenario event trees, Bayesian probabilities, and APT"
---

# Territory Map

Every claim below carries a pragmatic-semantics classification:
**[IS/OUGHT · epistemic mode · constraint force · provenance · confidence]**.
Provenance tiers: Specification / Implementation / Observation / Inference / External / Unknown.
Inference-tier claims with confidence ≤ 0.3 are flagged **FRAGILE**.

## 1. Theoretical terrain

### 1.1 Arbitrage Pricing Theory (APT)

- C1. APT is a one-period model: preclusion of arbitrage over **static** portfolios of factor-structured assets implies a linear relation between expected return and factor covariances. **[IS · declarative · Evidence · External: Huberman & Wang, NY Fed Staff Report 216, Aug 2005, abstract verbatim · 0.95]**
- C2. APT does **not** preclude arbitrage over **dynamic** portfolios; applying it to managed portfolios contradicts its no-arbitrage spirit. **[IS · declarative · Evidence · External: sr216 abstract · 0.95]** — *Consequence: any tâtonnement/dynamic-equilibrium layer of the target foundation sits outside classical APT's warrant and must be justified separately.*
- C3. Empirical APT requires identifying features of the underlying factor structure, not merely mean-variance-efficient factor portfolios. **[IS · declarative · Evidence · External: sr216 · 0.9]**
- C4. Modern high-dimensional APT relaxes the few-factor assumption (GIBS / Adaptive Multi-Factor model) and can outperform Fama-French 5 in fit and prediction; AMF betas are time-invariant over <6yr windows where FF5 betas are not. **[IS · probabilistic · Evidence · External: arXiv:1804.08472, arXiv:2011.04171 · 0.8]**

### 1.2 Bayesian arbitrage (the central paper)

- C5. Bhattacharya (arXiv:2211.03244, "Arbitrage from a Bayesian's Perspective") models arbitrage via interactive belief hierarchies: no-arbitrage holds iff every agent optimizes at a sufficiently high order of reasoning about others' strategies; tâtonnement steps weakly raise the operative hierarchy order (Prop. 6), so iterated one-step-ahead trading is outcome-equivalent to deliberate higher-order reasoning. **[IS · declarative · Evidence · External: arXiv:2211.03244 full text via ar5iv incl. appendix proofs · 0.95 — T0 verified]**
- C6. The paper assumes symmetric information about payoffs (physical measure P is common knowledge) and isolates belief-about-strategy updating — it is **not** an empirical-Bayes factor model. **[IS · declarative · Evidence · External: ibid., §1, §4.1 · 0.95 — T0 verified]** — *T0 resolution: the platform's event tree is the paper's model with the roles reversed (tree varies S with the strategic layer frozen into market-implied priors; the paper varies strategies/beliefs at fixed P). The mapping holds approximately — coherence (Eq. 16) exactly, depth-2–3 truncation by level-k evidence, Prop. 6 dynamically; W_i^k-set semantics structurally excluded. See t0-keystone-mapping.md.*
- C7. If the price-aggregation map is one-to-one, no-arbitrage ⇔ all agents optimize over the entire infinite hierarchy — a "one more level than the market" arms race. **[IS · declarative · Evidence · External: ibid., Prop. 4 · 0.9 — T0 verified]** — *Scope note (T0 §5.5): near-one-to-one aggregation is the liquid-market regime the reliability tiers prefer; the platform is exposed only as a price reader, not an arbitrage trader.*

### 1.3 Tâtonnement / equilibrium discovery

- C8. Walras's tâtonnement adjusts prices by the law of excess demand (dp/dt = φ[qᵈ−qˢ], φ′>0); single-market stability requires slope conditions (Samuelson 1941/47 formalization). **[IS · declarative · Evidence · External: hetwebsite.net/het/essays/stable/walrastatonnement.htm · 0.9]**
- C9. Four canonical critiques: (i) no agent whose job is price adjustment (Arrow 1959); (ii) unmotivated behavioral microfoundation (Koopmans 1957); (iii) strategic manipulation of the auctioneer; (iv) decentralization/implementability. **[IS · declarative · Evidence · External: ibid. · 0.9]**
- C10. Bhattacharya Prop. 6 answers critique (iii) in spirit: traders need not be deliberately strategic for tâtonnement outcomes to mimic higher-order reasoning. **[IS · probabilistic · Inference from C5+C9 · 0.6]**

### 1.4 Game theory in finance

- C11. Global games (Carlsson & van Damme 1993; Morris & Shin 1998, 2001) resolve coordination-game multiplicity via noisy private signals — canonical for currency attacks, bank runs, bubbles. **[IS · declarative · Evidence · External: AER 88(3):587–97; Cowles DP 1275R · 0.9]**
- C12. Prices acting as endogenous public signals can reintroduce multiplicity when private information is precise (Angeletos & Werning 2006; Hellwig et al. 2006). **[IS · declarative · Evidence · External: AER 96(5) · 0.85]** — *Direct caution for H1: market prices are not passive probability readouts; they feed back into beliefs.*
- C13. A 2025 PRISMA review (Paseda, JCMS 9(2):106–131, CC BY 4.0) taxonomizes 78 game-theory-in-finance papers (2000–2025) across asset pricing, corporate finance, investment, markets, behavioral; gaps: behavioral integration, empirical validation, decentralized ecosystems. **[IS · declarative · Evidence · External: Crossref API abstract, DOI 10.1108/jcms-04-2025-0044 · 0.85]**

### 1.5 Endogenous risk

- C14. Bookstaber (2007): financial disasters arise from tight coupling + complexity ("normal accidents", Perrow lineage); intricate risk-management structures can make the system worse; a "coarse" adaptive approach often beats fine-grained optimization. **[IS · probabilistic · Evidence · External: Wikipedia summary of *A Demon of Our Own Design* — secondary source · 0.7]** — *Supports the H4 complexity-allocation constraint from an independent direction.*
- C15. Leverage-cycle agent-based models (arXiv:1507.04136; arXiv:1805.00785) show stable/cyclic/chaotic regimes from leverage targeting and VaR constraints — endogenous systemic risk is formally modelable. **[IS · probabilistic · Evidence · External: arXiv abstracts · 0.75]**

### 1.6 Prediction markets as probability sources

- C16. AMM limiting prices converge to belief aggregates whose form depends on trader utility class (geometric/weighted-power means) — the theoretical warrant for reading prices as aggregated probabilities. **[IS · declarative · Evidence · External: arXiv:2205.08913 · 0.8]**
- C17. Informational substitutes characterize best-case immediate aggregation; complements worst-case delayed aggregation (Chen & Waggoner). **[IS · declarative · Evidence · External: arXiv:1703.08636 · 0.8]**
- C18. The law of one price fails across prediction-market venues: ~6% of events cross-listed with persistent 2–4% deviations, structural not informational (100k+ events, 10 venues, 2018–2025). **[IS · probabilistic · Evidence · External: arXiv:2601.01706 · 0.8]** — *A direct obstacle to naive arbitrage arguments across venues; the foundation must be single-venue or venue-adjusted.*
- C19. Parlay/joint-contract AMMs price joint and conditional outcomes coherently, converging to the best approximation of the true joint distribution. **[IS · probabilistic · Evidence · External: arXiv:2603.22596 · 0.7]** — *The market-microstructure analog of scenario event-tree branch pricing.*

### 1.7 Scenario planning & event trees

- C20. GBN/van der Heijden scenario method: scenarios as scaffolds confronted against strategy via the scenario-strategy matrix; option generation; flexibility. **[IS · declarative · Evidence · External: gbn_scen_process.pdf (local Library, read) · 0.85]**
- C21. Scenario trees for stochastic programming can be generated by HMMs capturing autoregression, jumps, skew, kurtosis (validated FTSE-100); economic scenario generators can be calibrated consistently to history + forward views via conditional simulation in a Bayesian setting. **[IS · probabilistic · Evidence · External: arXiv:0904.1131, arXiv:2004.09042 · 0.75]**
- C22. Time-consistent multivariate risk measures admit backward recursion on event trees (set-valued Bellman principle); superhedging sets under transaction costs are computable by backward construction on the tree. **[IS · declarative · Evidence · External: arXiv:1508.02367, arXiv:1107.5720 · 0.8]** — *The computational machinery for the risk-calculation core.*

### 1.8 Equity duration & maturity transformation

- C23. Equity duration literature (Dechow–Sloan–Soliman 2004 implied duration; Leibowitz franchise value) is **not on arXiv** — it lives in JAE/NBER/CFA venues. Local holdings: `Old-Books/Franchise_Value_Liebowitz.pdf` (Leibowitz, franchise-value P/E decomposition), Damodaran valuation corpus, Fabozzi bond math. **[IS · declarative · Observation · Library sweep · 0.85]**
- C24. John Burr Williams' *Theory of Investment Value* (value = PV of dividend stream) is the conceptual root of equity duration. **[IS · declarative · Evidence · External: Researcher/138352485 reading-notes page · 0.8]**
- C25. Maturity transformation + systemic risk (Adrian & Shin "Liquidity and Leverage", JFI 2010) is FRBNY/NBER, not arXiv; nearest arXiv coverage is leverage-cycle models (C15). **[IS · declarative · Observation · arXiv sweep negative result · 0.8]**
- C26. Practitioner 3-stage equity modeling (consensus 12–18mo → normalization 3–5yr → terminal) is an informal equity-duration methodology already in the Library's house notes. **[IS · probabilistic · Evidence · External: Researcher/137682783.time-horizons · 0.7]**

## 2. MCP server current state (grounded in source reading)

### 2.1 `hkask-mcp-scenarios` (17 tools)
- C27. Implements Bayesian event trees: `ScenarioEvent` with `depends_on: Vec<EventDependency>` carrying bitmap-ordered CPTs (length 2^n_parents); `EventTree` with Kahn topo sort, cycle detection, marginal resolution via `hkask_forecast::marginalize` under parent-independence; scalar Bayes update, Brier scoring, calibration curves, Fermi calibration, dragonfly synthesis. **[IS · declarative · Implementation · kask/mcp-servers/hkask-mcp-scenarios/src/types.rs L225–284, superforecast.rs L64–262 · 0.95]**
- C28. Only `depends_on[0]` is used — the Vec holds at most one effective dependency group. **[IS · declarative · Implementation · superforecast.rs L86 · 0.9]** — *Latent limitation for deep trees.*
- C29. `scenario_from_markets` bridges a `MarketRecord` into a **root** event only (depends_on empty), with refusal gates (resolved/closed markets rejected; low reliability ⇒ base_rate withheld, never fabricated) and domain-bias decompression (politics δ=0.3). **[IS · declarative · Implementation · superforecast.rs L1468–1548 · 0.95]**
- C30. No tree-level Bayesian update propagation (update one node → recompute tree); `scenario_update` is per-event scalar Bayes. **[IS · declarative · Implementation · hkask_mcp_scenarios.rs L1342–1390 · 0.9]**
- C31. No APT/factor-exposure mapping anywhere in the server. **[IS · declarative · Implementation · full-crate read · 0.9]**
- C32. `TimeHorizon` is a 3-bucket enum; deadlines are dates; no duration/term-structure concept. **[IS · declarative · Implementation · types.rs L153–160 · 0.9]**

### 2.2 `hkask-mcp-companies` (35+ tools)
- C33. Deep valuation engine: 11-line 2-stage DCF, reverse DCF, Monte Carlo DCF (uniform sampling), tornado sensitivity, economic-profit valuation with competitive fade, expectations-gap analysis, forecast_record with post-hoc gap decomposition (growth/margin/D&A/capex/NWC/multiple/net-debt). **[IS · declarative · Implementation · financial_model.rs L336–785, valuation.rs, economic_profit.rs · 0.95]**
- C34. Its "scenarios" are static Schwartz 2x2 multipliers (growth ×1.5/×0.5, margin ×1.2/×0.8 hardcoded), not probabilistic event trees. **[IS · declarative · Implementation · scenarios.rs L49–97 · 0.9]**
- C35. No factor model, no beta/exposure representation, no equity-duration metric; DCF stage years give real cash-flow timing but no duration summary. **[IS · declarative · Implementation · full-crate read · 0.9]**
- C36. `ResearchClaim` carries source URL/date/provider, but claims are not linked to forecast assumptions; `forecast_record` gap decomposition is the only post-hoc assumption chain. **[IS · declarative · Implementation · research.rs L35–46, financial_model.rs L671–785 · 0.9]**

### 2.3 `hkask-mcp-prediction-markets` (6 tools)
- C37. `MarketRecord` is a richly annotated contract: probability + method (LastTrade|Midpoint), spread, volume + grain, liquidity, open interest, volatility (realized variance + structural flags near deadline/coinflip), status, resolution metadata, calibration (Brier, domain bias, stale flag), reliability tier with negative-feedback demotion, dual-axis ontology block. **[IS · declarative · Implementation · types.rs L30–260 · 0.95]**
- C38. "Never return a bare probability" is enforced structurally — probability always travels with spread/volume/calibration/volatility/tier. **[IS · declarative · Implementation · types.rs assemble L329–370 · 0.9]**
- C39. `deadline` is an RFC3339 string; days-to-deadline computed ad hoc; no first-class time-to-maturity field, no term structure across a contract ladder, no price-history series (snapshot only). **[IS · declarative · Implementation · types.rs L125–153, L292–295 · 0.9]**
- C40. Matching is deterministic (Jaccard × deadline factor, mid-year pivot for year-precision); no LLM. **[IS · declarative · Implementation · matcher.rs L50–149 · 0.9]**

### 2.4 `hkask-mcp-research` (17 tools)
- C41. Multi-provider web search with RRF fusion (k=60), per-provider success/failure provenance, structured extraction (markdown/JSON schema), headless browsing, full RSS reader with FTS5. **[IS · declarative · Implementation · hkask_mcp_research.rs L128–781, providers/mod.rs L130–482, db.rs L20–88 · 0.95]**
- C42. No stable citation IDs, no content-hash pinning (blake3 used only for cache keys), no claim-level extraction with source spans, no durable citation store beyond RSS entries. **[IS · declarative · Implementation · full-crate read · 0.9]**

### 2.5 Integration seams
- C43. Exactly one code-level dependency edge exists: scenarios → prediction-markets (type reuse of `MarketRecord`). All runtime integration is caller-mediated paste bridging; no server calls another over the wire; bridged base rates are frozen at bridge time (stale-anchor risk). **[IS · declarative · Implementation · Cargo.toml L28; superforecast.rs L1468 · 0.95]**
- C44. `companies.research_search` duplicates research-server capability with a weaker parallel client (claims classifier, no RRF fusion). **[IS · declarative · Implementation · companies/research.rs vs research/providers · 0.9]**

## 3. The gap (OUGHT side)

- C45. The target foundation requires: markets→tree composition (N markets wired into dependent trees), tree-level Bayesian propagation, scenario→factor-exposure mapping, first-class duration on both contracts and equity cash flows, and citation-pinned provenance. None exists today. **[OUGHT · declarative · Guideline · Specification: this plan · 0.85]**
- C46. The three-axes collapse (time simple, return simple, risk complex) is a design constraint to be validated, not a premise. **[OUGHT · subjunctive · Hypothesis · Specification: user prompt, H4 · 0.6]**
- C47. The end-state (economy discovering equilibrium between risk and return factors via scenario graphs connecting equity forecasts to market-implied systemic risk) is a target architecture, not an observed capability. **[OUGHT · subjunctive · Hypothesis · Specification: user prompt · 0.6]**

## 4. FRAGILE claims register (Inference, confidence ≤ 0.3)

- F1. That scenario-graph factor exposures will be *arbitrage-pricing-relevant* in the sr216 sense (static-portfolio no-arbitrage ⇒ linear pricing) rather than merely *correlational*. **[IS · subjunctive · Hypothesis · Inference · 0.3 — FRAGILE]** — the linear-pricing conclusion requires a no-arbitrage argument the scenario graph does not yet supply.
- F2. That equity risk is long-duration relative to prediction-market contracts in a way a single duration scalar captures. **[IS · subjunctive · Hypothesis · Inference · 0.3 — FRAGILE]** — equity duration estimates in the literature are model-sensitive (Dechow–Sloan–Soliman sensitivity to ROIC persistence assumptions).
- F3. That LLM-mediated analysis is economically new here rather than a cost reduction on existing research workflows. **[IS · subjunctive · Hypothesis · Inference · 0.25 — FRAGILE]** — motivational only; explicitly load-bearing for "why now", not for the math.
