# Prediction Markets: Research Report

**Audience:** zed-kask forecasting/scenario integration
**Date:** 2026-08-04
**Scope:** Polymarket (Gamma/CLOB/Data APIs), Kalshi (Predictions REST/WS), and the academic literature on prediction-market calibration, volatility, and data services.
**Method:** Sources fetched live (Polymarket docs, Kalshi `llms.txt`, arXiv 2604.20421v1 full text, arXiv 2601.18815/2602.19520/2607.08199 abstracts). Analytical lenses: pragmatic-semantics (IS/OUGHT), pragmatic-cybernetics (feedback-loop), essentialist (deletion test), grill-me (adversarial check), hypothesis-framer, metacognition.

---

## 1. What a prediction market is (IS, not OUGHT)

A prediction market trades contingent claims on future event outcomes. The dominant contract is a **binary option**: payoff $X = \mathbf{1}\{E\} \in \{0,1\}$ — pays 1 if event $E$ occurs, 0 otherwise. A contract price $p_t \in [0,1]$ is conventionally read as a **market-implied probability** $p_t \approx \Pr_t(E=1)$ (arXiv:2604.20421 §2.1).

This is an **IS** statement with a known caveat: the price-as-probability interpretation is a _convention_ supported by the payoff structure, **not** a guarantee of calibration. The academic literature (below) treats "price = probability" as a **Hypothesis** to be tested per-domain, not an axiom. We adopt the same posture: market prices are _evidence about beliefs_, and their relationship to _true_ probabilities must be measured (Brier score) before being consumed by a forecaster.

### 1.1 Polymarket lifecycle (from 2604.20421, the "data services" paper)

Polymarket organizes predictions in a hierarchy:

```
Series (e.g. "2028 U.S. Presidential Election")
  └─ Event ("Who will win the 2028 U.S. presidential election?")
       ├─ Market 1 ("Will Donald Trump win?")  — condId, YES/NO tokens
       ├─ Market 2 ("Will Joe Biden win?")
       └─ Market 3 ("Will Gavin Newsom win?")
```

A market lifecycle spans six stages: **creation → token registration → trading → oracle interaction → dispute → settlement** (2604.20421 §1). Trading uses a central limit order book (CLOB) off-chain; custody/settlement on Polygon smart contracts; resolution via the **UMA Optimistic Oracle** (request → proposal → optional dispute → settlement). Prices lie in $[0,1]$ and are interpreted as implied probabilities.

The dataset constructed by Jia et al. spans Oct 2020 – Mar 2026: **770,880 market records, 943,548,464 OrderFilled records, 1,988,150 oracle-resolution events**, with 99.40% oracle-event linkage to canonical markets (2604.20421 Table 2). It is the first continuously-maintained, full-lifecycle dataset — directly relevant to us because it defines the _canonical data model_ a downstream forecasting consumer needs.

### 1.2 Two platforms, two regimes

| Dimension         | Polymarket                                                             | Kalshi                                                                                             |
| ----------------- | ---------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| Infrastructure    | Decentralized (Polygon, UMA oracle, on-chain fills)                    | Regulated CFTC exchange (US)                                                                       |
| Discovery API     | **Gamma** (`gamma-api.polymarket.com`) — events/markets metadata       | **Predictions REST** — events, markets, series, candlesticks, orderbook                            |
| Live state API    | **CLOB** (`clob.polymarket.com`) — orderbook/prices                    | REST + **WebSocket** ticker/orderbook/trades                                                       |
| Activity API      | **Data** (`data-api.polymarket.com`) — account/market activity         | portfolio/fills/orders endpoints                                                                   |
| Streaming         | WebSocket APIs (market/account/sports/RFQ)                             | WebSocket (ticker, orderbook, trades, lifecycle)                                                   |
| Auth for read     | Mostly public                                                          | Public market data **without** auth; auth for trading/portfolio                                    |
| Forecast metadata | Implicit (price = probability)                                         | **Explicit**: `GET /events/{ticker}/forecast-percentile-history` — historical forecast percentiles |
| Historical        | Data API + on-chain (the 2604.20421 pipeline)                          | `GET /historical/*` (markets, trades, candlesticks post-cutoff)                                    |
| Resolution        | UMA Optimistic Oracle (dispute < 1% of settled, but trading continues) | Exchange settlement (market lifecycle docs)                                                        |

**Key asymmetric finding (pragmatic-semantics, IS):** Kalshi exposes a _first-class forecast-percentile-history_ endpoint — Polymarket does not (probabilities must be reconstructed from CLOB prices/trades, as 2604.20421 §6.2 does for CPI). This shapes the integration: Kalshi gives us a ready-made "expected probabilities over time" feed; Polymarket gives us raw price/volume from which we (or the 2604.20421 reconstruction method) derive probabilities.

---

## 2. The academic signal: can market prices be trusted as probabilities?

Four 2026 arXiv papers frame the answer. Applying **hypothesis-framer** (FINER + PICO):

**Focal question (PICO):** For a population of binary event contracts, does consuming the market-implied probability $p_t$ as a calibrated forecast _intervention_ improve _outcome_ (Brier score / decision quality) relative to a no-market baseline, and under what _domain/horizon/volume_ conditions does it fail?

### 2.1 Calibration is structured and multidimensional (2602.19520, Le)

Using **292 million trades across 327,000 binary contracts on Kalshi and Polymarket**, Le shows calibration decomposes into four components that explain **87.3% of calibration variance** on Kalshi:

1. a **universal horizon effect** (prices compress toward 50% near resolution? — actually a deadline effect),
2. **domain-specific biases** (politics persistently _underconfident_ — prices chronically compressed toward 50% on both exchanges),
3. **domain × horizon interactions**,
4. a **trade-size scale effect** (large trades amplify political underconfidence on Kalshi: Δ=0.53 [0.29, 0.75], but does _not_ replicate on Polymarket: Δ=0.11 [-0.15, 0.39] — platform-specific microstructure).

A **Bayesian hierarchical model** confirms the frequentist decomposition with 96.3% posterior predictive coverage.

**Implication (IS):** "Consumers who treat market prices as face-value probabilities will systematically misinterpret them; the direction of misinterpretation depends on _what_ is predicted, _when_, and _by whom_." This is the single most important caveat for our integration: a naive `market_price → base_rate` mapping is a known bias channel.

### 2.2 Market inference is an inverse problem (2601.18815, Madrigal-Cianci et al.)

Formulates prediction markets as **Bayesian inverse problems**: infer unknown outcome $Y \in \{0,1\}$ from a history of market-implied probabilities and traded volumes, with a latent mixture of trader types (informed / uninformed / adversarial). The framework provides:

- **identifiability criteria** in terms of KL separation between outcome-conditional increment laws,
- **posterior concentration** rates and finite-sample error bounds,
- **information gain** (realized + expected) via posterior-vs-prior KL divergence and mutual information,
- **stability diagnostics** for regimes where inference is informative vs. ill-posed (type-composition confounding, outcome–nuisance symmetries).

**Implication (IS/OUGHT boundary):** There _exist_ principled mechanisms to convert price+volume histories into calibrated posteriors with uncertainty quantification — but they require _more_ than the face-value price. OUGHT: our data service should carry **volume** and **time-series of prices**, not just the latest price, so a future Bayesian-revision consumer (superforecasting stage 4) can apply likelihood ratios rather than treat the price as a point estimate.

### 2.3 Volatility is structural (2607.08199, Xi, Moallemi, Pai, Wang)

Develops a volatility model for binary prediction markets combining a **Wright-Fisher deadline-resolution** component (remaining binary uncertainty forced to resolve over time) and a **Glosten-Milgrom order-flow** component (volatility from informed trading, reflected in spreads and volume). On a large Kalshi panel, structural variables carry substantial forecasting power and dominate plain ARCH/GARCH; combining structural + residual GARCH is best. Volatility is highest near 50/50 prices, rises near resolution, and varies by category: economics ≈ smooth deadline-resolution; sports ≈ jump-like, event-concentrated.

**Implication (IS):** Volatility/spread/volume are _informative covariates_ about price reliability, not noise to discard. A market at 0.50 with a wide spread and thin volume is far less informative than one at 0.72 with a tight spread and heavy volume. OUGHT: expose spread + volume + last-update-time alongside the probability.

### 2.4 The data-services suite (2604.20421, Jia et al.) — the paper the user flagged as central

This is the one most directly about _data services for forecasting_. It constructs a **unified relational data system** integrating three canonical layers:

1. **Market metadata** (Gamma + on-chain recovery): question, slug, condId, tokens, deadline, category, tags.
2. **OrderFilled trades** (on-chain, fill-level): tx hash, maker/taker, token, size, price, fee, block.
3. **Oracle resolution events** (UMA): request, proposal, dispute, settlement — the ground-truth labels for Brier scoring.

Plus bridge/cache/synchronization layers for cross-source identifier resolution and continuous incremental updates. Two downstream applications demonstrate utility:

- **NBA calibration (§6.1):** pre-game volume-weighted market probabilities on NBA winner markets are **already well-calibrated** — raw probability Brier 0.2034, LogLoss 0.592, ECE 0.027; isotonic calibration adds nothing. Markets are good forecasts _where the event is well-defined and the reference class is clean_.
- **CPI reconstruction (§6.2):** reconstructs a continuous CPI point estimate from Polymarket's _discrete bucket markets_ (each bucket = an inflation range). Method: value-weighted bucket probabilities → fit a unimodal Gaussian per timestamp → take the mean. The reconstructed series is **closer to realized BLS CPI than the Cleveland Fed nowcast** in 2 of 3 representative months and is more responsive to incoming information.

**This paper is the template for what we are building.** It defines: (a) the canonical event/market/trade/oracle relational model, (b) the value-weighted probability reconstruction method, (c) continuous synchronization with checkpoints (resumable, duplicate-safe), (d) downstream Brier calibration of market prices against oracle outcomes, (e) a public interface (`polymonitor.club`) — exactly the "data service for forecasting" shape our scenarios server needs.

---

## 3. The data we can actually retrieve (API surfaces, verified)

### 3.1 Polymarket Gamma API (`gamma-api.polymarket.com`) — discovery + metadata

Per 2604.20421 §4.2.1, the Gamma API is the primary source of market metadata: Gamma market/event endpoints provide `g_i` (Gamma id), `c_i` (on-chain condition id), `q_i` (question id), `o_i` (oracle address), token identifiers, and metadata (slug, title, description, timestamps, category, tags). The full-lifecycle paper normalizes these into Definition 4.1. Concretely we can retrieve, per event/market:

- **Event:** id, slug, title, description, category, tags, start/end dates.
- **Market:** question, slug, condId, YES/NO token ids, deadline, category, resolution source, status (active/closed/resolved), **outcome prices (last, bid/ask)**, volume, liquidity.
- **Series hierarchy** (event → markets).

Note: the Polymarket docs site is a Mintlify SPA (direct endpoint pages 404 via fetch), but the Gamma API itself is a public REST JSON service and is exactly what 2604.20421's pipeline queries. **Resolution outcome** is obtained either from Gamma's status/outcome fields or from the **Data API** / on-chain oracle events (the paper's oracle layer).

### 3.2 Polymarket CLOB API (`clob.polymarket.com`) — live state

Verified read endpoints (docs.polymarket.com/api-reference, fetched 2026-08-04): `Get order book` / batch books, `Get market price` (best bid/ask per token+side), `Get midpoint price` / batch, `Get last trade price` / batch (max 500 token IDs; defaults to `"0.5"` with no trades), `Get spread` / batch, `Get prices history` / batch (historical price series — the calibration-backtest feed), `Get open interest`, `Get live volume for an event`. Read-only market state needs no auth. This is where _spread_ and _depth_ (the volatility-informing covariates from 2607.08199) come from.

**Rate limits (verified, Cloudflare IP-based, throttled not rejected):** CLOB general 9,000 req/10s; `/book`, `/price`, `/midpoint` 1,500/10s; `/prices-history` 1,000/10s; Gamma general 4,000/10s with `/events` 500/10s and `/markets` 300/10s; Data API general 1,000/10s with `/trades` 200/10s.

**Identifier quirk (verified):** markets are keyed by on-chain `condition_id`, but all price/book endpoints take **outcome token IDs** (one ERC1155 per outcome). Gamma market objects carry both; `Get market by token` resolves token → parent market. Prices are decimal probabilities in [0,1] (dollar-denominated), not cents.

**Resolution (verified via docs.polymarket.com/concepts/resolution):** UMA Optimistic Oracle — proposal with bond (typically $750), ~2h challenge period undisputed; disputed outcomes escalate to a DVM token-holder vote (~4–6 days total). "Unknown/50-50" resolves each side at $0.50.

### 3.3 Polymarket Data API (`data-api.polymarket.com`) — activity & history

Public endpoints (verified): trades for a user or markets, current/closed positions per user, positions per market, top holders per market (concentration risk), user activity, trader leaderboard. Useful for backfilling realized outcomes to compute Brier scores (the oracle layer in 2604.20421) and for holder-concentration as a manipulation covariate (2601.18815's adversarial-flow type).

### 3.4 Kalshi Predictions REST — the richer metadata surface

From Kalshi's `llms.txt`, the read endpoints most relevant to a forecasting data service:

- `GET /events` / `GET /events/{ticker}` — events (real-world occurrences: elections, sports, economic releases); events contain one or more markets.
- `GET /events/{ticker}/metadata` — event metadata.
- **`GET /events/{ticker}/forecast-percentile-history`** — _historical raw and formatted forecast numbers for an event at specific percentiles._ This is a first-class probability-timeseries feed — no reconstruction needed.
- `GET /markets` / `GET /markets/{ticker}` — markets (binary outcomes within events): yes/no positions, current prices, volume, settlement rules. Status filter: `unopened|open|closed|settled`.
- `GET /markets/{ticker}/orderbook` / `GET /markets/{ticker}/candlesticks` — orderbook + OHLC (1m/1h/1d).
- `GET /series` / `GET /series/list` — series templates (recurring events: "Monthly Jobs Report", "Weekly Initial Jobless Claims", "Daily Weather NYC") — _these are the natural reference classes for outside-view forecasting_.
- `GET /trades` — all public trades (price, qty, timestamp).
- `GET /events/{ticker}/candlesticks` — aggregated across markets in an event.
- `GET /live_data/...` — event-keyed live data (crypto price charts, commodity timeseries, weather) — the _underlying_ the market references.
- `GET /historical/*` — archived markets/trades/candlesticks/orders/positions past the live cutoff.
- WebSocket: `ticker`, `orderbook`, `public trades`, `market & event lifecycle` — for streaming updates.

**Kalshi auth & limits (verified via docs.kalshi.com getting-started pages, fetched 2026-08-04):** REST prod base `https://external-api.kalshi.com/trade-api/v2` (demo at `external-api.demo.kalshi.co`). Public market data works without auth over REST; **the WebSocket handshake requires RSA-signed auth even for public channels**. Signed requests use an RSA keypair over the request path. Rate limits are tiered token-buckets with separate read/write buckets: Basic 200 read/100 write tokens/s (default cost 10 tokens/request) up to Prestige 6,000/8,000; 429s return no Retry-After header. Tiers above Advanced are earned by 30-day volume share — for a read-only data service the Basic tier plus a TTL cache is sufficient.

**Data-format quirks (verified):** (a) the orderbook endpoint returns **yes bids and no bids only — no asks** (binary equivalence: yes-bid at X ≡ no-ask at 1−X), so spread computation requires the 1−no-bid transform; (b) Kalshi is mid-migration from integer **cents** to fixed-point **dollar** price fields (`price_dollars`) — both representations appear in responses and our parser must handle each; (c) tickers are hierarchical, e.g. `FED-23DEC-T3.00` = `{SERIES}-{DATE}-{TYPE}{STRIKE}` (T = target/above, B = between-range) — the series prefix is the reference-class key; (d) market status enum is `unopened|open|closed|settled`.

**WebSocket channels (verified, single connection):** `orderbook_delta`, `ticker`, `trade`, `fill`, `market_positions`, `market_lifecycle_v2`, plus `communications` (RFQ). Subscribe by `market_ticker(s)` or `market_id(s)`; server pings every 10s; `seq` numbers order snapshot/delta consistency.

### 3.5 What we can compute from these (the "data service" output contract)

For each event/market we can produce a record aligned to 2604.20421's model:

| Field                                                        | Source                                            | Why                                                                                        |
| ------------------------------------------------------------ | ------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `event_id`, `market_id`/`ticker`/`slug`                      | Gamma / Kalshi                                    | canonical identity                                                                         |
| `question`, `description`                                    | both                                              | maps to `ScenarioEvent.question`                                                           |
| `category`, `tags`, `series`                                 | both                                              | reference-class for outside view                                                           |
| `deadline` / `close_time`                                    | both                                              | horizon (drives 2602.19520 horizon effect, 2607.08199 deadline-resolution)                 |
| `outcome` (yes/no), `probability`                            | price (mid or last)                               | the market-implied probability                                                             |
| `spread` (bid/ask), `volume`, `liquidity`, `open_interest`   | CLOB / Kalshi orderbook                           | reliability covariates (2607.08199)                                                        |
| `last_trade_time`                                            | both                                              | staleness                                                                                  |
| `price_history` (candlesticks / forecast-percentile-history) | Kalshi percentile-history; Polymarket CLOB trades | time-series for Bayesian revision (2601.18815) and volatility (2607.08199)                 |
| `status` (open/closed/settled/resolved)                      | both                                              | lifecycle                                                                                  |
| `resolved_outcome`                                           | Data API / Kalshi settlement                      | ground-truth label for Brier                                                               |
| `resolution_source` / oracle                                 | both                                              | provenance of the label                                                                    |
| `brier_score` (computed)                                     | resolved_outcome vs probability at horizon        | calibration of _this market_ — the "accuracy of the markets themselves" the user asked for |
| `calibration_bucket`, `domain`                               | derived (2602.19520)                              | the known bias correction                                                                  |

---

## 4. Adversarial test of the core hypothesis (grill-me + pragmatic-semantics + cybernetics)

**Hypothesis H1 (the integration premise):** "Prediction-market prices are a useful outside-view data source for the scenarios MCP server and superforecasting skill."

### 4.1 grill-me — escalating challenge

- **Recall:** Do markets produce probabilities? Yes, by payoff structure (IS). Are they calibrated? Domain-dependent (2602.19520). Are outcomes verifiable? Yes, via oracle/settlement (2604.20421 oracle layer). → H1 survives recall.
- **Mechanism:** How does a market price improve a forecast? It aggregates dispersed information into a continuously-updated posterior (2601.18815). The mechanism is _information aggregation via trading_. For this to help _our_ forecast, the market must contain information our forecaster doesn't already have. → H1 survives mechanism, but introduces a **redundancy condition**: if our LLM forecaster already read the same news the market priced in, the market adds little.
- **Rationale:** Why use markets at all vs. just asking the LLM? Because (a) markets provide a _quantitative, time-stamped, falsifiable_ probability with a _measurable track record_ (Brier) — an LLM gut estimate has neither; (b) markets aggregate _price-weighted_ conviction (volume), filtering talk-vs-money; (c) Kalshi series give clean reference classes for outside-view base rates. → H1 rationale holds.
- **Edge cases:** Thin/illiquid markets (price = noise, not information — 2607.08199: wide spread ⇒ low reliability). Manipulated markets (2601.18815: adversarial flow can make inference ill-posed). Resolved markets used as "live" (stale). Multivariate/bucket markets requiring reconstruction (2604.20421 CPI). Politics domain (systematic underconfidence — face-value use is biased). → H1 holds **conditionally**: gate on liquidity/volume/spread/staleness/domain.
- **Synthesis:** H1 is corroborated (not confirmed — Popper) _as a conditional data service_: market prices are useful outside-view evidence **when** annotated with reliability covariates and calibration history, and **when** the consumer applies domain-aware calibration rather than face-value ingestion.

**Verdict:** H1 → **adopt as a conditional, annotated data service**, not a face-value probability injection. This is the load-bearing design decision for the integration report.

### 4.2 pragmatic-semantics — IS/OUGHT classification of the design space

| Claim                                          | Tier                       | Notes                                                                                    |
| ---------------------------------------------- | -------------------------- | ---------------------------------------------------------------------------------------- |
| "Market price ≈ probability"                   | Hypothesis (per-domain)    | IS of the payoff structure; OUGHT-as-probability is the convention, tested by 2602.19520 |
| "Polymarket resolves via UMA oracle"           | IS (Core)                  | 2604.20421, verifiable                                                                   |
| "Kalshi exposes forecast-percentile-history"   | IS (Core)                  | verified in `llms.txt`                                                                   |
| "Politics markets are underconfident"          | IS (Domain, stat.AP)       | 2602.19520, both exchanges                                                               |
| "We should ingest market prices as base rates" | OUGHT (Hypothesis)         | this is _our_ design choice, not a fact — must be gated by calibration                   |
| "Volume/spread are reliability signals"        | IS (Core, q-fin.TR)        | 2607.08199                                                                               |
| "A naive price→probability map is safe"        | FALSE (Hypothesis refuted) | 2602.19520 refutes; 2601.18815 requires inverse-problem machinery for rigor              |

**Constraint-force ranking:** "Never ingest face-value politics prices as calibrated probabilities" is a **Guardrail** (evidence-backed, repeatable). "Annotate every market probability with volume+spread+staleness" is a **Guideline**. "Prefer Kalshi forecast-percentile-history over reconstructed Polymarket prices when available" is a **Guideline**.

### 4.3 pragmatic-cybernetics — the feedback loop

Treating the integration as a control system (market → our forecaster → decision → outcome → calibration update):

- **Loop present?** Yes. Market prices (sense) → superforecasting Bayesian update (decide/act) → recorded forecast (store) → outcome resolution (sense) → Brier + calibration curve (feedback) → adjust market weight in next forecast (corrective). This is a _proper_ negative-feedback loop **iff** the calibration signal actually feeds back into the market's weight.
- **Polarity:** Must be **negative** (calibration reduces the weight of poorly-calibrated market sources). The failure mode the `.rules` "unwrap_or(0) on regulation sense inputs" trap warns about: a DB outage returning Brier=0 read as "perfect calibration" would be a **positive** (reinforcing) loop — over-weighting broken markets. **Mitigation:** propagate calibration-read errors as "signal stale", never `unwrap_or(0)`.
- **Delay:** Market resolution delay (days–months) introduces feedback lag. The loop must tolerate unresolved markets (carry them as "pending") rather than silently dropping them.
- **Variety (Ashby):** The market-data channel carries variety (domain, horizon, volume). Our consumer must model at least that much variety (per-domain calibration), or it will be under-actuated — exactly the 2602.19520 finding that a single "market = probability" model loses 87.3% of calibration variance.
- **Good Regulator:** The forecasting consumer must _model_ the market it regulates — i.e., it must know the market's domain, horizon, and calibration history to use it well. A consumer that ignores these is not a good regulator of the market-signal subsystem.

### 4.4 essentialist — deletion test on the proposal

**Exist gate (G1):** If we delete "prediction-market data service" from the scenarios/forecasting pipeline, does complexity reappear? Yes — the forecaster currently has _no_ quantitative outside-view base rate with a measurable track record; it relies on the LLM's parametric memory and the `research_text` the agent pastes in. The gap is real; the module earns its existence.

**Surface gate (G2):** How many public surfaces does the data service need? One MCP server exposing ≤7 tools (lookup, search, timeseries, calibration, cross-validate-against-market, status, anchor). Within the deep-module budget.

**Contract gate (G3):** Is there a simpler interface? The minimal contract is: `market_implied_probability(question, deadline, domain) → {probability, lower/upper via spread, volume, staleness, calibration_history, source}`. Everything else (timeseries, bucket reconstruction, volatility) is optional depth behind the same interface. The interface is minimal; do not bloat it.

### 4.5 metacognition — calibration of our own confidence

- **What we are confident about (high):** Markets produce prices interpretable as probabilities (payoff structure); Kalshi has a forecast-percentile-history endpoint; 2604.20421 provides a working canonical data model and reconstruction method; the scenarios server has no HTTP path today, so a separate data-service server is the clean seam.
- **What we are _not_ confident about (medium):** That _our specific_ consumers (an LLM-driven scenario cascade) will consistently apply domain-aware calibration rather than face-value ingestion. Mitigation: make the interface _return calibration warnings_, not just numbers.
- **What we cannot verify yet (low → must defer):** Live Polymarket Gamma response shape (docs SPA 404'd via fetch; the API is public but we did not issue a live request in this session). **Action:** a Phase-0 spike must hit `gamma-api.polymarket.com/events` and `docs.kalshi.com` endpoints to pin exact field names before implementation. This is recorded as the top open question / risk.

---

## 5. Summary of research findings

1. **Prediction markets produce continuously-updated, price-implied probabilities** with verifiable outcomes (oracle/settlement) — a genuinely useful outside-view signal for forecasting (2604.20421 §6.1: NBA markets already well-calibrated; §6.2: Polymarket CPI reconstruction beats a Fed nowcast in 2/3 months).
2. **Face-value ingestion is a known anti-pattern.** Calibration is multidimensional (2602.19520): a universal horizon effect, persistent political underconfidence on both exchanges, domain×horizon interactions, and a platform-specific trade-size effect. A consumer must apply domain-aware calibration, not `price → base_rate`.
3. **Reliability is observable.** Spread, volume, liquidity, open interest, and price history are informative covariates (2607.08199; 2601.18815). The data service should carry them, not just the probability.
4. **Kalshi offers a first-class probability-timeseries endpoint** (`forecast-percentile-history`) and clean _series_ reference classes; Polymarket offers volume/liquidity and the 2604.20421 reconstruction path. Both are complementary, not redundant.
5. **The canonical data model exists** (2604.20421): event/market/trade/oracle relational layers with continuous sync, checkpoints, and a public interface — this is the template for our data service.
6. **The integration is a conditional, annotated data service**, gated on liquidity/volume/spread/staleness/domain, with a negative feedback loop (Brier calibration) that must propagate "signal stale" on read failures (not `unwrap_or(0)`).

These six findings are the empirical ground for the integration report (`02-zed-kask-integration.md`) and the phased implementation plan (`tasks/plan.md`).

---

## References

- Jia, Zhou, Zhang, Cong, Li, Sun (2026). _Unlocking the Forecasting Economy: A Suite of Datasets for the Full Lifecycle of Prediction Market._ arXiv:2604.20421 [cs.LG]. — full text read.
- Madrigal-Cianci, Monsalve Maya, Breakey (2026). _Prediction Markets as Bayesian Inverse Problems._ arXiv:2601.18815 [q-fin.MF]. — abstract.
- Le (2026). _Decomposing Crowd Wisdom: Domain-Specific Calibration Dynamics in Prediction Markets._ arXiv:2602.19520 [stat.AP]. — abstract.
- Xi, Moallemi, Pai, Wang (2026). _Volatility in Prediction Markets: A Structural Approach._ arXiv:2607.08199 [q-fin.TR]. — abstract.

### Sources fetched live (2026-08-04, this session's verification pass)

- `https://docs.polymarket.com/api-reference/predictions/overview`, `/concepts/resolution`, `/api-reference/rate-limits`, `/llms.txt`; `https://github.com/Polymarket` (102 repos; notable: `polymarket-cli` Rust 2.8k★, `real-time-data-client`, `py-clob-client-v2`, `rs-clob-client-v2`, `uma-ctf-adapter`).
- `https://docs.kalshi.com/welcome`, `/llms.txt`, `/getting_started/api_environments.md`, `/getting_started/rate_limits.md`, `/getting_started/market_settlement.md`, `/websockets/websocket-connection.md`.

**Remaining unverified (open questions, deferred to the Phase-0 spike):** exact Gamma market status enum values and machine-readable resolution fields (`umaResolutionStatus`/`resolvedBy` hypothesized, not confirmed); Polymarket WebSocket message-type schemas; Kalshi official SDK repo names; whether 2604.20421's dataset is bulk-downloadable or terminal-only at `polymonitor.club`.

- Polymarket Documentation, `docs.polymarket.com` — Gamma/CLOB/Data/Relayer/WebSocket/Bridge overview.
- Kalshi API Documentation, `docs.kalshi.com` — `llms.txt` full endpoint index (events, markets, series, candlesticks, orderbook, forecast-percentile-history, historical).
