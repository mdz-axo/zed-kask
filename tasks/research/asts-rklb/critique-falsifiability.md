---
title: "Falsifiability Critique — ASTS vs RKLB Comparative Report"
skill: falsifiability
target: tasks/research/asts-rklb/comparative-report.md
last_updated: 2026-08-15
status: critic-pass-complete
---

> **Critic mode.** The `falsifiability` skill cascade failed at step 4 (JSON parse error in the manifest executor). This analysis was reconstructed manually from the report content using the same discipline: for every comparative claim, state the observation that would falsify it; reject claims with no falsifier as untestable. No softening.

## Falsifiability Framework

A claim is **falsifiable** if there exists a conceivable observation that, if made, would force us to abandon the claim. A claim with no such observation is **untestable** and is rejected here — not because it is wrong, but because it cannot be held accountable to evidence.

Three categories of failure are tracked:

1. **No falsifier exists** (value judgments, undefined terms, counterfactuals with no observable consequence) → **Rejected**.
2. **Falsifier exists but is impractical within the report's horizon** (e.g., 20Y price targets, single-event probability assignments) → **Testable in principle**; flagged with the horizon problem.
3. **Falsifier exists and is observable** (arithmetic on disclosed figures, milestone outcomes, contract values) → **Testable**.

Probability assignments (e.g., "bear case 30–60%") are a special case: a single probability is not falsifiable by any single outcome (any outcome is consistent with any nonzero probability). They are testable *only* via a calibration track record over many forecasts — not by observing whether the bear case "happened." They are marked testable-in-principle with that caveat, not rejected outright, because a falsifier does exist at the population level.

## Claim-by-Claim Falsifiers

| # | Claim (as stated in report) | Falsifier (observation that would disprove) | Testable? | Verdict |
|---|---|---|---|---|
| 1 | ASTS has higher upside asymmetry (bull/bear 15–60x vs RKLB 3–20x across horizons) | Recompute bull/bear ratios from realized 5Y/10Y/20Y price outcomes; if RKLB's realized ratio ≥ ASTS's, the asymmetry ranking reverses. (Arithmetic on the report's own scenario numbers is testable now; the scenario inputs are forecasts.) | Yes (arithmetic now; inputs are forecasts) | Falsifiable |
| 2 | RKLB has a defense-anchored revenue floor of $1B+/year (SDA, Space Force, MDA) | RKLB's annual defense-segment revenue falls below $1B for two consecutive years, or defense contracts are cancelled/not renewed such that contracted defense revenue < $1B/yr run-rate. | Yes | Falsifiable |
| 3 | SpaceX is the shared existential threat (Starlink D2D for ASTS; Falcon 9/Starship for RKLB) | (a) SpaceX exits or de-prioritizes D2D *and* medium-lift launch, removing it as a competitor to either; OR (b) ASTS and/or RKLB fail for reasons unrelated to SpaceX while SpaceX remains active — showing SpaceX was not the binding constraint. The "shared competitor" sub-claim is testable; the "existential" magnitude is a counterfactual judgment (see Rejected). | Partially | Falsifiable on the "shared competitor" dimension; "existential" framing rejected as untestable hyperbole |
| 4 | ASTS revenue depends on satellite deployment (45-sat target by Q1 2027) | ASTS reaches the $1B FY2027 revenue target *without* deploying 45 satellites (e.g., via government contracts or non-BlueBird capacity), OR ASTS deploys 45 satellites and revenue stays far below $1B (showing deployment is not sufficient). | Yes | Falsifiable |
| 5 | RKLB revenue depends on Neutron first flight (binary milestone) | RKLB reaches FY2028 FCF-positive *without* Neutron flying (e.g., via Iridium + Electron + Space Systems alone), OR Neutron flies and RKLB revenue/FCF does not improve materially (showing Neutron was not the binding constraint). | Yes | Falsifiable |
| 6 | ASTS is a platform bet; RKLB is an infrastructure bet | The labels "platform bet" and "infrastructure bet" are definitional. The substance (ASTS = wholesale capacity to MNOs; RKLB = launch + space systems supply) is testable; the labels as stated are not. | No (as labeled) | **Rejected** — labeling, no falsifier. Substance re-entered as claims 7–8. |
| 7 | ASTS's revenue model is wholesale capacity to MNOs (platform) | ASTS's revenue comes primarily from direct consumer subscriptions or per-unit hardware sales rather than wholesale capacity agreements with MNOs. | Yes | Falsifiable |
| 8 | RKLB's revenue model is transactional launch + space systems (supply chain) | RKLB's revenue comes primarily from a platform/wholesale capacity model rather than per-launch/per-bus/per-component contracts. | Yes | Falsifiable |
| 9 | ASTS has a wider outcome distribution but no revenue floor | ASTS secures a multi-year contracted revenue base comparable in visibility to RKLB's defense backlog (e.g., a large fixed MNO capacity prepay or government anchor contract), giving it a structural floor. Note: ASTS already has $70.9M FY2025 revenue and government contracts — "no revenue floor" is already overstated; the falsifier is the emergence of a *structural* floor. | Yes | Falsifiable (and currently weakly supported) |
| 10 | RKLB has a narrower distribution but a defense-anchored floor that caps the bear case | RKLB's bear-case price falls to near zero despite defense revenue (e.g., defense contracts are cancelled, Iridium fails to close, and dilution overwhelms the floor), OR RKLB's outcome distribution widens to match ASTS's. | Yes | Falsifiable |
| 11 | ASTS is the higher-variance bet; RKLB is the lower-variance bet | Realized return variance of ASTS over a comparable window is ≤ RKLB's, or the scenario distributions are recomputed with corrected probabilities and the variance ranking reverses. | Yes (arithmetic on scenarios) | Falsifiable |
| 12 | ASTS dilution is more elegant (low-coupon convertibles with capped calls) | "Elegant" is a value judgment with no observable falsifier. The factual sub-claim (1.625% coupon, capped calls at $149.20, <2% effective dilution) is testable against filings. | No (the word "elegant") | **Rejected** — value judgment. Factual sub-claim re-entered as claim 13. |
| 13 | ASTS's latest raise used 1.625% convertibles with capped calls at $149.20, effective dilution <2% | Filings show a different coupon, no capped calls, or effective dilution ≥2%. | Yes | Falsifiable |
| 14 | RKLB dilution is more aggressive (ATM + large stock-for-stock Iridium consideration) | RKLB's share count growth from ATM + Iridium stock is ≤ ASTS's effective dilution over the same period, or RKLB uses non-equity financing for Iridium. | Yes | Falsifiable |
| 15 | RKLB's dilution is more immediate and larger in absolute terms than ASTS's | Cumulative 2026–2027 share-count growth (diluted basis) for RKLB is ≤ ASTS's, or RKLB's raises are smaller in dollar terms. | Yes | Falsifiable |
| 16 | Both ASTS and RKLB received investment_grade: false from the company-research-deep pipeline | Re-running the pipeline on the same inputs yields investment_grade: true for either company. | Yes | Falsifiable |
| 17 | ASTS burns cash faster ($500–700M/qtr vs RKLB $77–110M FCF) | Quarterly cash burn (capex + OpEx) for ASTS is ≤ RKLB's, or RKLB's FCF burn exceeds ASTS's on a comparable basis. | Yes (arithmetic on filings) | Falsifiable |
| 18 | ASTS has more absolute cash ($3.7B vs RKLB $1.21B + $178M securities) | Most recent balance sheets show ASTS cash ≤ RKLB cash + securities. | Yes (arithmetic on filings) | Falsifiable |
| 19 | RKLB has shorter runway but contracted revenue visibility from defense backlog | RKLB's cash runway (cash ÷ burn) exceeds ASTS's, or RKLB's defense backlog does not convert to revenue at the implied rate. | Yes | Falsifiable |
| 20 | D2D telecom is a winner-take-most platform market | Multiple D2D providers (e.g., ASTS + Starlink + a third) coexist profitably with comparable market share over a sustained period, demonstrating the market supports multiple winners. | Yes (long-horizon) | Falsifiable (in principle) |
| 21 | Launch + space systems is a multi-player fragmented market with room for multiple players | The medium-lift launch market consolidates to a single dominant provider (e.g., SpaceX captures >80% share) and RKLB is unable to sustain a competitive position. | Yes (long-horizon) | Falsifiable (in principle) |
| 22 | If ASTS wins, it wins big; if SpaceX wins, ASTS is marginalized | "Wins big" is undefined. Without a quantitative threshold (e.g., market cap > $X, revenue > $Y), no observation falsifies "wins big." The "marginalized" sub-claim is testable (ASTS becomes a minor player or is acquired). | No ("wins big" undefined) | **Rejected** — undefined term. The "marginalized" sub-claim is re-entered as claim 23. |
| 23 | If SpaceX wins the D2D market, ASTS is marginalized | SpaceX achieves broadband D2D at scale and ASTS retains a dominant, independent, profitable D2D position (not marginalized). | Yes | Falsifiable |
| 24 | The defense floor ensures RKLB survives even without winning the commercial launch market | RKLB files bankruptcy, restructures, or is acquired at a distressed valuation despite receiving defense revenue ≥ $1B/yr. ("Survives" needs operationalization; the falsifier is observable distress despite the floor.) | Yes | Falsifiable |
| 25 | Both depend on access to orbit | Either company generates its core revenue without orbital assets (e.g., ASTS via terrestrial-only, RKLB via ground-only services). Trivially true given current business models. | Yes (trivially) | Falsifiable (trivially true; low information) |
| 26 | If launch capacity is constrained or prices rise, ASTS is a buyer (cost up) and RKLB is a seller (revenue up) — a zero-sum overlap | Launch prices rise and (a) ASTS's launch costs do NOT rise (e.g., fixed-price contracts) or (b) RKLB's revenue does NOT rise (e.g., RKLB is capacity-constrained and cannot capture the price increase). Either breaks the zero-sum framing. | Yes | Falsifiable |
| 27 | SpaceX's IPO (June 2026) triggered capital rotation affecting both stocks | ASTS and RKLB stock prices show no abnormal movement attributable to the SpaceX IPO event (event-study null result), or move for clearly unrelated reasons. | Yes (event study) | Falsifiable (causality is hard but testable) |
| 28 | ASTS's path is more granular (each satellite adds capacity); RKLB's is more binary (Neutron flies or it doesn't) | ASTS revenue turns out to hinge on a single binary event (e.g., one regulatory approval or one gateway), OR RKLB revenue scales incrementally via Electron + Space Systems + Iridium without Neutron ever flying. | Yes | Falsifiable |
| 29 | ASTS's revenue, when it scales, should carry higher margins than RKLB's (wholesale infrastructure) | ASTS reaches scale (> $1B revenue) and its gross margin is ≤ RKLB's gross margin, or ≤ a wholesale-infrastructure benchmark. | Yes (conditional, long-horizon) | Falsifiable |
| 30 | RKLB's revenue is more diversified today but lower-margin than ASTS's (forward) | RKLB's segment margins are NOT lower than ASTS's current/forward margins, or RKLB's revenue is NOT more diversified (e.g., higher single-customer concentration than ASTS). | Yes | Falsifiable |
| 31 | ASTS captures margin by owning the custom layer (satellites + protocol + MNO relationships) and outsourcing the commodity layer (launch) | ASTS's gross margin is NOT higher than what an outsourced-launch cost structure would predict, or ASTS vertically integrates launch (owning the "commodity" layer), contradicting the outsourcing claim. | Yes | Falsifiable |
| 32 | RKLB captures margin by vertically integrating the mid-chain (engines, structures, buses, mission design) | RKLB's gross margin is NOT higher than comparable peers who outsource mid-chain components, or RKLB de-integrates (outsources engines/structures). | Yes | Falsifiable |
| 33 | ASTS is a launch customer; RKLB is a launch supplier | ASTS begins launching its own satellites on its own vehicles, or RKLB exits the launch business entirely. | Yes | Falsifiable |
| 34 | Both share a regulatory regime (FAA/FCC/ITU) and semiconductor supply dependency | Either company operates without FAA/FCC/ITU oversight, or either company's bill of materials contains no semiconductors. (Trivially true; low information.) | Yes (trivially) | Falsifiable (trivially true) |
| 35 | ASTS customer concentration is High (top MNOs + government represent the bulk) | ASTS discloses customer concentration and no single customer or small set represents the bulk, or revenue disperses across many small MNOs. Note: report itself says "No single-customer concentration disclosure was found" — this is an **Inference-tier** claim with thin support. | Yes | Falsifiable (and currently weakly supported) |
| 36 | RKLB customer concentration is Government-anchored + commercial | RKLB's revenue is NOT government-anchored (e.g., commercial > 60% of revenue, defense contracts lapse). | Yes | Falsifiable |
| 37 | ASTS ROIC is -6.6% on $5B+ invested capital (value destroyer on current economics) | Recomputed ROIC from filings is ≥ 0%, or invested capital is materially different from $5B+. | Yes (arithmetic on filings) | Falsifiable |
| 38 | ASTS has 60+ MNO partners covering 3B+ subscribers | ASTS filings disclose fewer than 60 MNO partners or the covered subscriber count is materially below 3B. | Yes | Falsifiable |
| 39 | RKLB backlog is $2.36B (+137% YoY) | RKLB filings disclose a backlog figure materially different from $2.36B or YoY growth materially different from +137%. | Yes | Falsifiable |
| 40 | ASTS backlog is $1.3B | ASTS filings disclose a backlog figure materially different from $1.3B. | Yes | Falsifiable |
| 41 | ASTS Q2 2026 capex was $610M vs $31.5M revenue | Q2 2026 filings show capex or revenue materially different from these figures. | Yes | Falsifiable |
| 42 | RKLB Q2 2026 FCF was $(110)M vs $(77.4)M in Q1 2026 (accelerating burn) | Q2 2026 filings show FCF materially different from $(110)M, or Q1 was not $(77.4)M. | Yes | Falsifiable |
| 43 | BlueBird 7 was lost in a New Glenn anomaly (April 2026) | Launch records show BlueBird 7 was not lost, or was lost in a different anomaly/date. | Yes | Falsifiable |
| 44 | Starlink D2D is operational (SMS) | Starlink has not achieved operational SMS D2D service. | Yes | Falsifiable |
| 45 | Beck acknowledged the window for an end-of-year Neutron launch is narrowing | Q2 2026 earnings call transcript does not contain this acknowledgment, or contains the opposite. | Yes | Falsifiable |
| 46 | ASTS had four consecutive earnings misses | Earnings records show fewer than four consecutive misses. | Yes | Falsifiable |
| 47 | RKLB has 100% Electron mission success | Electron flight records show one or more mission failures. | Yes | Falsifiable |
| 48 | Iridium acquisition would add $870M/year recurring revenue and 2.5M subscribers | Post-close Iridium financials show recurring revenue materially different from $870M/yr or subscriber count materially different from 2.5M. | Yes (post-close) | Falsifiable |
| 49 | ASTS FY 2027 revenue target is ~$1B | FY 2027 actual revenue is materially below $1B (target missed) — note: a missed target falsifies the *target as a credible forecast*, not the fact that management *stated* it. | Yes | Falsifiable (as a forecast) |
| 50 | ASTS path to profitability is FY 2028 (management target) | FY 2028 results show ASTS is not profitable. (Tests the target as forecast; the fact that management *stated* it is separately testable against the call transcript.) | Yes | Falsifiable (as a forecast) |
| 51 | RKLB FCF positive in FY 2028 if Neutron + Iridium integrate | FY 2028 results show RKLB FCF is not positive despite Neutron flying and Iridium closing. | Yes (conditional) | Falsifiable |
| 52 | ASTS bull case is $400–800/share at 5Y (2031) | ASTS share price in 2031 is outside $400–800 in the bull scenario, or the bull scenario does not materialize and the price is elsewhere. (Tests the forecast; single price targets are testable at horizon.) | Yes (at 5Y horizon) | Falsifiable (long-horizon) |
| 53 | RKLB bull case is $150–200/share at 5Y (2031) | RKLB share price in 2031 is outside $150–200 in the bull scenario. | Yes (at 5Y horizon) | Falsifiable (long-horizon) |
| 54 | ASTS bear-case probability is 30–60% across horizons | A calibration track record over many ASTS-style forecasts shows the realized bear-case frequency is outside 30–60%. (Single-outcome falsification is impossible; only population-level calibration falsifies.) | Yes (via calibration only) | Falsifiable (calibration-only; not single-outcome) |
| 55 | ASTS has higher bear-case probability than RKLB (30–60% vs RKLB's 30–50%) | Calibration track records show RKLB's realized bear frequency ≥ ASTS's, reversing the ranking. | Yes (via calibration only) | Falsifiable (calibration-only) |
| 56 | Space Systems grew to 66.9% of FY2025 RKLB revenue | FY2025 segment disclosure shows Space Systems share materially different from 66.9%. | Yes | Falsifiable |
| 57 | Launch Services revenue per launch rose from $7.8M to $8.5M (pricing power) | Per-launch revenue figures from filings differ materially, or the increase reflects mix-shift rather than pricing (volume vs price decomposition falsifies "pricing power"). | Yes | Falsifiable |
| 58 | RKLB SDA Tranche 3 ($816M), RSLP ($266M), MACH-TB 2.0 ($190M) embed RKLB in multi-year defense programs | Contract values or existence differ from disclosed figures, or contracts are cancelled. | Yes | Falsifiable |
| 59 | ASTS revenue recognition is tied to gateway deliveries, government contract milestones, and commercial service activation (not recurring subscriptions) | ASTS revenue is recognized on a recurring subscription basis rather than milestone/delivery basis. | Yes | Falsifiable |
| 60 | ASTS has no single-customer concentration disclosure in reviewed filings | A single-customer concentration disclosure exists in ASTS filings (10-K, 10-Q, press releases) that the report missed. | Yes | Falsifiable (and a direct admission of a research gap) |

## Rejected Claims

The following claims are **rejected as untestable** — they have no observable falsifier and must be either operationalized or removed in the rewrite.

| # | Rejected Claim | Reason | Remedy |
|---|---|---|---|
| R1 | "ASTS is a platform bet; RKLB is an infrastructure bet" (claim 6 as labeled) | The labels "platform bet" and "infrastructure bet" are definitional categorizations, not empirical claims. No observation changes which label applies because the labels are assigned by the analyst, not discovered. | Replace with the operational sub-claims (claim 7: ASTS revenue is wholesale capacity to MNOs; claim 8: RKLB revenue is transactional launch + space systems). These are testable. |
| R2 | "ASTS dilution is more elegant" (claim 12, the word "elegant") | "Elegant" is a value judgment. No observation makes a financing structure more or less "elegant" — only more or less dilutive, cheaper, or more flexible. | Replace with the testable sub-claim (claim 13: 1.625% coupon, capped calls, <2% effective dilution) and a comparative dilution claim (claim 14/15: RKLB's absolute dilution is larger). Drop "elegant." |
| R3 | "If ASTS wins, it wins big" (claim 22, the "wins big" clause) | "Wins big" has no quantitative threshold. Without a defined target (market cap > $X, revenue > $Y, share > $Z), no observation falsifies it — any positive outcome can be relabeled "winning big." | Operationalize: e.g., "ASTS bull case = $400–800/share at 5Y" (already stated as claim 52) or "ASTS 10Y bull = $800–1,500/share" (claim 1). Use those as the testable stand-ins. Drop the unbounded "wins big." |
| R4 | "SpaceX is the shared existential threat" — the "existential" framing (claim 3, magnitude) | "Existential threat" is a counterfactual claim about what *could* cause a company's demise. It is not directly observable. The testable sub-claim — SpaceX competes with both ASTS (D2D) and RKLB (launch) — is observable. The magnitude ("existential") is a judgment that survives any single observation (SpaceX could always be argued to be a latent threat even if neither company fails). | Keep the testable core: "SpaceX competes with both ASTS (Starlink D2D) and RKLB (Falcon 9/Starship)." Drop "existential" or operationalize it as "SpaceX's entry caused >X% revenue loss or >Y% share price decline in either company." |

## Recommendations

The rewrite must address the following to satisfy convergence criterion #4 ("Every load-bearing claim has a falsifier"):

1. **Remove or operationalize the four rejected claims (R1–R4).** Specifically:
   - Drop "platform bet / infrastructure bet" as labels; keep the operational revenue-model claims.
   - Drop "elegant" dilution language; keep the coupon/dilution figures.
   - Drop "wins big"; use the stated price targets ($400–800, $800–1,500) as the testable bull case.
   - Drop "existential threat" or replace with "SpaceX competes with both" + a quantitative impact threshold.

2. **Flag the horizon problem on long-horizon forecasts.** Claims 49–53 (FY 2027/2028 targets, 5Y/10Y/20Y price targets) are testable only at horizon. The report should mark these as "forecast — testable at horizon" rather than presenting them as current facts. The 20Y scenarios (claim 1, 20Y row) are effectively unfalsifiable within any actionable window and should be labeled as illustrative, not load-bearing.

3. **Flag the calibration problem on probability assignments.** Claims 54–55 (bear-case probabilities) are not falsifiable by single outcomes. The report should either (a) label them as subjective priors requiring a calibration track record, or (b) replace point probabilities with directional claims ("bear > bull probability") that are testable via calibration ranking.

4. **Strengthen the two weakly-supported claims.**
   - Claim 9 ("ASTS has no revenue floor") is already overstated — ASTS has $70.9M FY2025 revenue and government contracts. Reframe as "ASTS has no *structural* revenue floor comparable to RKLB's defense anchor" and cite the specific gap (no multi-year contracted base ≥ $1B/yr).
   - Claim 35 ("ASTS customer concentration is High") is an Inference-tier claim the report itself admits has no disclosure support ("No single-customer concentration disclosure was found"). Either find the disclosure or downgrade the claim from "High" to "Inferred high, unconfirmed."

5. **Add falsifiers inline.** The report's convergence criteria require every load-bearing claim to have a falsifier. The cleanest fix is to add a "Falsifier:" line under each `### Comparative Assessment` table and under each scenario probability, stating the observation that would disprove it. This critique provides those falsifiers; the rewrite should surface them in the report body, not hide them in a separate critique file.

6. **Distinguish Specification from Inference per claim.** The Anne Gentle perspective test already flagged this. Claims 35, 60, and the revenue-split inferences in the Sankey (MNO 60% / Gov 30% / Other 10%) are Inference-tier and should be tagged as such inline, not just in a conservation note. A claim tagged "Inference" should carry its falsifier explicitly (e.g., "Falsified if ASTS filings disclose a different revenue split").

7. **The "zero-sum overlap" claim (26) needs a boundary condition.** The claim that RKLB's gain is ASTS's cost holds only if RKLB can actually capture the price increase (i.e., RKLB is not capacity-constrained). The rewrite should state the condition under which the zero-sum framing breaks.
