---
title: "Pragmatic-Semantics Critique — ASTS vs RKLB Comparative Report"
skill: pragmatic-semantics
target: tasks/research/asts-rklb/comparative-report.md
last_updated: 2026-08-15
status: complete
---

> **Note on execution.** The `pragmatic-semantics` skill manifest timed out after 45s on Step 1. Per the skill's fallback protocol, this classification was reconstructed manually by reading the report and applying the IS/OUGHT × epistemic-mode × provenance framework by hand. The classification is grounded in the report's own text and footnote citations; no external claims were introduced.

## Classification Framework

Three orthogonal axes classify every load-bearing claim:

**Certainty (IS vs OUGHT)**
- **IS** — a claim about what is the case (observed state, disclosed fact, realized outcome). Falsifiable against evidence.
- **OUGHT** — a claim about what should be the case, or a normative recommendation. Not falsifiable against evidence alone; requires a value frame.

**Epistemic Mode**
- **Declarative** — asserted as true now. Carries the strongest falsification burden.
- **Probabilistic** — asserted with explicit or implicit probability. Falsifiable only against a distribution of outcomes.
- **Subjunctive** — asserted conditionally ("would," "could," "if X then Y"). Falsifiable only if the antecedent is realized.

**Provenance**
- **Specification** — sourced from a primary disclosure (SEC filing, press release, earnings call transcript, contract announcement). The report's footnotes anchor these.
- **Implementation** — observed operational state (e.g., "Electron has 50+ flights," "BlueBird 7 was lost"). Verifiable against operational record.
- **Inference** — analyst judgment, industry convention, structural interpretation, or composition of disclosed facts into a non-disclosed aggregate. The highest-risk tier for epistemic slippage.

**Flag rule.** A claim is flagged when it is Inference-tier in provenance but presented in declarative IS mode without an explicit Inference label — i.e., the report's prose treats an analyst judgment as though it were a disclosed fact.

## Load-Bearing Claims Classification

Load-bearing = a claim whose falsification would change the comparative verdict (ASTS higher-variance/no-floor vs RKLB lower-variance/defense-floor; both `investment_grade: false`).

| # | Claim (paraphrased) | Certainty | Epistemic Mode | Provenance | Flag |
|---|---------------------|-----------|----------------|------------|------|
| 1 | Both ASTS and RKLB received `investment_grade: false` from the deep pipeline | IS | Declarative | Specification | — |
| 2 | ASTS sells connectivity *capacity* to MNOs, not subscriptions to consumers | IS | Declarative | Specification | — |
| 3 | ASTS has 60+ MNO partners covering 3B+ subscribers, $1.3B contracted backlog (Q2 2026) | IS | Declarative | Specification | — |
| 4 | ASTS FY2025 revenue $70.9M; FY2026 guidance $150–200M; FY2027 target ~$1B | IS | Declarative (realized) / Subjunctive (forward target) | Specification | — |
| 5 | RKLB Space Systems = 66.9% of FY2025 revenue | IS | Declarative | Specification | — |
| 6 | RKLB Launch Services revenue/launch rose $7.8M → $8.5M, "reflecting pricing power in a tight dedicated-launch market" | IS | Declarative | Specification (figures) + Inference ("pricing power" interpretation) | **FLAG** |
| 7 | Iridium acquisition would add $870M/year recurring revenue, 2.5M subscribers | IS | Subjunctive (pending close) | Specification | — |
| 8 | RKLB backlog $2.36B (+137% YoY) | IS | Declarative | Specification | — |
| 9 | ASTS's business model is a *platform*; RKLB's is a *supply chain* | IS | Declarative | Inference (structural framing) | **FLAG** |
| 10 | ASTS revenue, when it scales, should carry higher margins (wholesale infra with 60+ MNOs as distribution) | OUGHT | Subjunctive | Inference | **FLAG** |
| 11 | RKLB revenue is more diversified today but lower-margin (manufacturing + launch services) | IS | Declarative | Inference | **FLAG** |
| 12 | ASTS sits at Custom stage on the Wardley evolution axis; key assets Genesis→Custom | IS | Declarative | Inference (Wardley mapping is analyst judgment) | **FLAG** |
| 13 | RKLB spans wider evolution range; Electron Product, Neutron/Archimedes Genesis→Custom | IS | Declarative | Inference (Wardley mapping) | **FLAG** |
| 14 | ASTS captures margin by owning the custom layer and outsourcing the commodity layer (launch) | IS | Declarative | Inference | **FLAG** |
| 15 | RKLB captures margin by vertically integrating the mid-chain (engines, structures, buses) | IS | Declarative | Inference | **FLAG** |
| 16 | ASTS is a launch *customer*; RKLB is a launch *supplier* — fundamental value-chain divergence | IS | Declarative | Implementation (verifiable from filings/ops) | — |
| 17 | ASTS cash $3.7B+; burn ~$500–700M/qtr; runway ~5–7 quarters at current burn | IS | Declarative (cash) / Probabilistic (runway estimate) | Specification (cash) + Inference (runway) | **FLAG** |
| 18 | ASTS ROIC -6.6% on $5B+ invested capital; EP valuation classified ASTS as "value destroyer" | IS | Declarative | Specification (ROIC) + Inference ("value destroyer" label) | **FLAG** |
| 19 | RKLB cash $1.21B + $178M securities; Q2 2026 non-GAAP FCF $(110)M, accelerating as Neutron capex peaks | IS | Declarative | Specification | — |
| 20 | RKLB dilution High: $1.98B+ raised 2026 via ATM; new $750M ATM Aug 2026; Iridium $8B consideration | IS | Declarative | Specification | — |
| 21 | ASTS dilution Medium (capped calls mitigate); RKLB dilution more immediate and larger in absolute terms | IS | Declarative | Inference (comparative judgment) | **FLAG** |
| 22 | RKLB has a defense-anchored revenue floor ($1B+/year from SDA, Space Force, MDA) that prevents the bear case from reaching zero | IS | Declarative | Inference ($1B+ is a sum of disclosed contracts; "prevents bear from zero" is analyst judgment) | **FLAG** |
| 23 | ASTS has no revenue floor (pre-commercial) | IS | Declarative | Specification | — |
| 24 | SpaceX is the shared existential threat — D2D competitor to ASTS, launch competitor to RKLB | IS | Declarative | Inference ("existential threat" is analyst framing) | **FLAG** |
| 25 | Starlink Direct-to-Cell is operational (SMS); if Starlink achieves broadband D2D, AST's technology lead narrows or disappears | IS (operational) / Subjunctive (conditional) | Declarative / Subjunctive | Specification (SMS operational) + Inference (technology-lead claim) | **FLAG** |
| 26 | BlueBird 7 was lost in a New Glenn anomaly (April 2026) | IS | Declarative | Implementation | — |
| 27 | Neutron first flight window "narrowing"; may slip to 2027; stage test still pending | IS | Probabilistic | Specification (Beck quote) + Inference (slip probability) | — |
| 28 | ASTS revenue ramp depends on satellite deployment; RKLB's depends on Neutron first flight | IS | Declarative | Inference (dependency framing) | **FLAG** |
| 29 | ASTS's path is more granular (each satellite adds capacity); RKLB's is more binary (Neutron either flies or doesn't) | IS | Declarative | Inference | **FLAG** |
| 30 | ASTS bull/bear asymmetry 15–60x across horizons; RKLB 3–20x | IS | Probabilistic | Inference (from analyst-assigned scenario probabilities) | **FLAG** |
| 31 | ASTS bear-case probability 30–60% across horizons | IS | Probabilistic | Inference | **FLAG** |
| 32 | D2D telecom is a winner-take-most platform market | IS | Declarative | Inference (industry-structure claim) | **FLAG** |
| 33 | Launch + space systems is a more fragmented market with room for multiple players; defense floor ensures RKLB survives even without winning commercial launch | IS | Declarative | Inference | **FLAG** |
| 34 | ASTS = higher variance, no floor; RKLB = lower variance, defense floor (comparative verdict) | IS | Declarative | Inference (the verdict itself is an analyst synthesis) | **FLAG** |
| 35 | ASTS revenue split (MNO 60% / Gov 30% / Other 10%) inferred from backlog composition | IS | Declarative | Inference (report self-flags) | — |
| 36 | RKLB revenue split inferred from segment disclosure and contract announcements | IS | Declarative | Inference (report self-flags) | — |
| 37 | RKLB semiconductor supplier not disclosed in 10-K | IS | Declarative | Inference (report self-flags) | — |

## Flagged Claims

The report self-flags three Inference-tier items (Sankey revenue splits #35–36, RKLB semiconductor supplier #37). The following **17 additional** Inference-tier claims are presented in declarative IS mode without an explicit Inference label — they read as disclosed fact but are analyst judgments or compositions:

1. **#6 — "pricing power in a tight dedicated-launch market."** The $7.8M→$8.5M revenue/launch figures are Specification, but the *causal interpretation* ("pricing power") is Inference. Alternative explanations (mix shift, contract repricing, one-off) are not excluded.

2. **#9 — "ASTS is a platform; RKLB is a supply chain."** Structural framing of the business models. Reasonable, but it is an analyst taxonomy, not a disclosure. If the framing is wrong (e.g., ASTS behaves more like a regulated utility, or RKLB behaves more like a platform via Iridium), downstream margin and asymmetry claims shift.

3. **#10 — "ASTS revenue, when it scales, should carry higher margins."** OUGHT-typed subjunctive presented as IS declarative. The "should" is doing load-bearing work but the claim is a forward margin prediction, not an observed fact.

4. **#11 — "RKLB revenue is more diversified today but lower-margin."** "More diversified" is Inference (diversification metric not disclosed); "lower-margin" is Inference (segment margins not broken out in the cited filings).

5. **#12, #13 — Wardley evolution-axis placements.** Wardley mapping is inherently analyst judgment. The report presents Custom/Genesis/Product labels as though they were observed state. A different mapper could place the same assets at different evolution stages.

6. **#14, #15 — "captures margin by owning/vertically integrating."** Margin-capture mechanism claims. Neither company discloses segment-level margin capture by value-chain layer. These are structural inferences from the Wardley map, not from financials.

7. **#17 — "runway ~5–7 quarters at current burn."** The cash figure is Specification; the runway estimate is Inference (assumes constant burn, no revenue, no raises). Presented as a point estimate, not as a conditional.

8. **#18 — "value destroyer" label.** ROIC is Specification; the "value destroyer" classification is an EP-valuation Inference. Presented inline as though it were a disclosed rating.

9. **#21 — "ASTS dilution more elegant; RKLB more immediate and larger."** Comparative dilution judgment. "Elegant" is normative; "more immediate and larger" is Inference (depends on Iridium close timing and ATM execution pace, neither certain).

10. **#22 — "defense-anchored revenue floor ($1B+/year) prevents the bear case from reaching zero."** The $1B+ is a sum of disclosed contract values, but (a) contracts can be terminated/delayed, (b) "prevents bear from zero" is an analyst judgment about downside protection. This is the single most load-bearing flagged claim — it is the structural basis for the "RKLB has a floor" half of the comparative verdict.

11. **#24 — "SpaceX is the shared existential threat."** "Existential threat" is analyst framing. SpaceX competes in both markets, but whether the competition is *existential* (vs. manageable) is an Inference.

12. **#25 — "if Starlink achieves broadband D2D, AST's technology lead narrows or disappears."** The conditional is Subjunctive; "technology lead" is Inference (no disclosed benchmark comparison). Presented as a declarative risk statement.

13. **#28, #29 — revenue-ramp dependency framing ("granular vs binary").** Analyst characterization of the dependency structure. If ASTS's ramp is actually more binary (e.g., regulatory gating) or RKLB's is more granular (e.g., Space Systems revenue independent of Neutron), the comparative risk profile inverts.

14. **#30, #31 — asymmetry ratios and bear probabilities.** Derived from analyst-assigned scenario probabilities in §6. The probabilities are Inference; the ratios inherit that provenance. Presented as computed facts.

15. **#32, #33 — market-structure claims ("winner-take-most" vs "fragmented").** Industry-structure Inferences presented as declarative IS. These underpin the asymmetry comparison in §6 and the cross-industry implication.

16. **#34 — the comparative verdict itself ("ASTS higher variance/no floor; RKLB lower variance/defense floor").** The verdict is an analyst synthesis of Inference-tier claims. It is presented as the report's conclusion, which is appropriate for a verdict, but every load-bearing input to it is Inference-tier and should be tagged as such upstream.

## Recommendations

The rewrite (Stage 5) must address the following to pass the Anne Gentle perspective test and the `pragmatic-semantics` convergence criterion:

1. **Per-claim provenance tagging.** Add inline `[Spec]` / `[Impl]` / `[Inference]` tags to every load-bearing claim, not just the three the report currently self-flags. The current self-flagging is inconsistent: the Sankey splits are tagged but the Wardley placements, margin-capture mechanisms, and market-structure claims are not.

2. **Separate IS from OUGHT in margin claims.** Claims #10 and #11 mix a forward margin prediction (OUGHT/subjunctive) with a present-state description (IS/declarative). Split them: "ASTS's model is wholesale capacity (IS, Spec)" vs "ASTS's margins, at scale, are expected to exceed RKLB's (OUGHT, Inference)."

3. **Mark the defense-floor claim as Inference and condition it.** Claim #22 is the load-bearing input to the "RKLB has a floor" verdict. Rewrite as: "RKLB's disclosed defense contracts sum to ~$1B+ [Spec]; whether this constitutes a revenue floor that prevents the bear case from reaching zero depends on contract execution and termination risk [Inference]."

4. **Mark Wardley placements as Inference explicitly.** Claims #12–15 should read "Wardley placement (Inference): ASTS sits at Custom..." rather than presenting the placement as observed state.

5. **Mark market-structure claims as Inference.** Claims #32–33 ("winner-take-most," "fragmented") are industry-structure Inferences. Tag them and cite the structural reasoning; do not present them as disclosed market facts.

6. **Mark asymmetry ratios as derived from Inference-tier probabilities.** Claims #30–31 should state that the ratios are computed from analyst-assigned scenario probabilities, not from market-implied or disclosed probabilities.

7. **Condition the "existential threat" claim.** Claim #24 should be reframed as Subjunctive/Inference: "SpaceX competes in both markets [Spec]; whether this competition is existential to either company is an analyst judgment [Inference]."

8. **Add a provenance summary to the Executive Summary.** The Executive Summary currently presents the comparative verdict (claim #34) without provenance context. Add a one-line note that the verdict synthesizes Inference-tier structural claims, not just Specification-tier disclosures.

9. **Distinguish realized vs forward in revenue claims.** Claim #4 mixes realized FY2025 revenue (IS/Spec) with FY2027 target (Subjunctive/Spec-as-guidance). Split the sentence so the forward target is not read as a realized fact.

10. **Re-examine the "pricing power" interpretation.** Claim #6's causal claim should either be dropped, hedged ("consistent with pricing power [Inference]"), or supported with a disclosed pricing-power statement from RKLB management.
