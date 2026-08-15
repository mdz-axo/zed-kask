---
title: "ASTS vs RKLB Comparative Report — Socratic Interrogation (grill-me)"
skill: grill-me
target: tasks/research/asts-rklb/comparative-report.md
last_updated: 2026-08-15
status: complete
note: "Skill cascade timed out after 30s on Step 1; interrogation reconstructed manually per fallback instruction using the 5-level framework (Recall → Mechanism → Rationale → Edge Cases → Synthesis) grounded in the report's actual text."
---

## Interrogation Summary

This critique applies a Socratic interrogation across five escalating levels — **Recall** (what does the report say?), **Mechanism** (how does it claim the mechanism works?), **Rationale** (why is the claim justified?), **Edge Cases** (where does the claim break?), and **Synthesis** (does the claim survive integration?) — to the five central comparative claims of the ASTS vs RKLB report. The report is well-structured and footnoted, but several load-bearing comparative claims rest on **Inference-tier** reasoning that the report itself flags as pending Stage 4 falsifiability critique. The Anne Gentle perspective test already fails on this exact weakness (Quality Log, §10). The interrogation below surfaces 14 specific gaps the report must close before its comparative verdicts can be considered load-bearing.

The five claims interrogated:
1. ASTS has higher upside asymmetry but no revenue floor.
2. RKLB has a defense-anchored floor that prevents the bear case from reaching zero.
3. SpaceX is the shared existential threat.
4. ASTS's revenue ramp depends on satellite deployment while RKLB's depends on Neutron first flight.
5. Both companies received `investment_grade: false`.

---

## Gap Analysis by Claim

### Claim 1 — "ASTS has higher upside asymmetry but no revenue floor"

**Recall.** The Executive Summary states: "ASTS has a wider outcome distribution (bear 30–60% probability across horizons; bull $400–800/share at 5Y) but no revenue floor." §6's Upside Asymmetry Comparison table computes bull/bear ratios of 15–20x (5Y), 15–25x (10Y), and 30–60x (20Y) for ASTS vs. 3–4x / 6–8x / 10–20x for RKLB. §3's Comparative Assessment table lists ASTS's "Revenue floor" as "None (pre-commercial)."

**Mechanism.** The report's mechanism is: ASTS's revenue is "milestone-dependent and lumpy" (§1) and tied to "gateway deliveries, government contract milestones, and commercial service activation — not recurring subscriptions." Because no commercial service is yet active, there is no recurring revenue stream to floor the bear case. Asymmetry is asserted to follow from "winner-take-most platform economics" (§6, §7) — if ASTS wins D2D, it wins big; if SpaceX wins, ASTS is marginalized.

**Rationale.** The rationale is internally consistent *for the asymmetry half* — the bull/bear ratios are computed from the scenario tree's own numbers, so the asymmetry claim is arithmetically grounded in the report's own scenarios. The "no revenue floor" half is weaker: it conflates "no *recurring* revenue floor" with "no revenue floor at all." §1 discloses $1.3B contracted backlog and government customers (DoD, SDA). §9's Sankey attributes 30% of ASTS revenue to "Government." A $1.3B backlog is a *contractual* floor over the contract horizon even if it is not recurring. The report does not reconcile "no revenue floor" with "$1.3B backlog including government contracts."

**Edge Cases.**
- *What if commercial service activates in Q4 2026 as targeted?* The report's own milestone calendar (§5) lists "Beta commercial service (target)" for Q4 2026. If activation occurs, ASTS transitions from "no floor" to "lumpy but real revenue" within the 5Y horizon. The "no floor" framing is horizon-sensitive and the report does not state the horizon over which "no floor" applies.
- *What if the government contracts are cancellable?* Defense contracts are often subject to termination-for-convenience and continuing resolutions. The report treats $1.3B backlog as if it were a hard floor for RKLB (Claim 2) but as if it did not exist for ASTS (Claim 1). This asymmetry in treatment is unexplained.
- *What if ASTS's MNO partners pay capacity minimums?* Wholesale capacity agreements often include take-or-pay or minimum-commitment clauses. The report does not disclose whether ASTS's MNO agreements contain floor provisions.

**Synthesis.** The asymmetry claim survives; the "no revenue floor" claim does not survive without reconciliation. The report must either (a) restate Claim 1 as "no *recurring* revenue floor" and explicitly distinguish recurring from contracted/milestone revenue, or (b) explain why ASTS's $1.3B backlog and government contracts do not constitute a floor while RKLB's $2.36B backlog and government contracts do. The current treatment applies a stricter floor standard to ASTS than to RKLB without justifying the asymmetry.

**Gaps for Claim 1:**
- G1.1 — "No revenue floor" conflates recurring revenue with contractual backlog; the $1.3B ASTS backlog is not reconciled against the "no floor" assertion.
- G1.2 — The horizon over which "no floor" applies is unspecified; the milestone calendar shows commercial service activation within 5Y.
- G1.3 — No disclosure of whether ASTS MNO capacity agreements contain take-or-pay or minimum-commitment floor provisions.
- G1.4 — Asymmetric floor standard: RKLB's backlog counts as a floor (Claim 2), ASTS's backlog does not (Claim 1), with no stated justification.

---

### Claim 2 — "RKLB has a defense-anchored floor that prevents the bear case from reaching zero"

**Recall.** Executive Summary: "RKLB has a narrower distribution but a defense-anchored revenue floor ($1B+/year from SDA, Space Force, MDA contracts) that prevents the bear case from reaching zero." §1 lists SDA Tranche 3 ($816M), RSLP Space Force ($266M), MACH-TB 2.0 ($190M). §3's table lists RKLB "Revenue floor: $1B+/year (defense-anchored)." §6's bear cases for RKLB are $40–60 (5Y), $30–60 (10Y), $20–50 (20Y) — none reach zero.

**Mechanism.** The mechanism is: defense contracts are multi-year, cost-plus or fixed-price, and embedded in programs (SDA, Space Force, MDA) that are politically durable. Because these contracts recur and are not contingent on Neutron's success, RKLB retains revenue even in the bear case where Neutron slips or fails. The floor "prevents the bear case from reaching zero."

**Rationale.** The rationale is plausible but under-specified. Three problems:
1. **The $1B+/year figure is not derived.** The report lists three contracts totaling $1.272B ($816M + $266M + $190M) but does not state the *annual* revenue recognition schedule, the contract duration, or whether these are backlog (multi-year) or annualized revenue. $1.272B in *backlog* spread over, say, 5 years is ~$254M/year — well below the "$1B+/year" floor asserted. The report does not show the arithmetic.
2. **Cost-plus does not equal revenue floor.** Cost-plus contracts guarantee *margin*, not *revenue volume*. If the government cuts program scope, revenue can fall even while margin is preserved. The report does not distinguish revenue floor from margin floor.
3. **"Prevents the bear case from reaching zero" is a low bar.** RKLB's 20Y bear case is $20–50/share. The report does not show that $20–50/share is *above* the liquidation or dilution-adjusted zero. With $2B+ in 2026 dilution and an $8B Iridium stock consideration (§3, §4), share count could grow faster than the defense floor grows equity value. A revenue floor that does not prevent equity value from reaching zero is not a floor for the *shareholder*.

**Edge Cases.**
- *What if SDA Tranche 3 is recompeted and lost?* SDA contracts are competitively recompeted across tranches. The report treats $816M as anchored but does not assess recompetition risk.
- *What if the Iridium deal closes and the stock consideration dilutes below the defense floor's equity contribution?* The report flags $8B Iridium consideration as dilution risk (§3, §4) but does not net it against the defense floor in the bear case.
- *What if defense budgets are cut?* The report assumes defense spending durability but does not cite a baseline defense budget assumption or SDA program-of-record status.
- *What if Neutron failure triggers a securities class action judgment that exceeds the defense floor's annual contribution?* §8's RKLB counter-evidence mentions a "securities class action" but the bear-case math does not net litigation liability.

**Synthesis.** The claim is directionally defensible — RKLB does have defense-anchored revenue that ASTS lacks in scale — but the specific assertion "$1B+/year floor prevents bear case from reaching zero" is not arithmetically demonstrated and conflates backlog with annualized revenue, revenue floor with equity-value floor, and contract value with contract durability. The claim should be restated as "RKLB has a defense-anchored *revenue* floor (magnitude and duration to be specified) that likely prevents *operational* zero, though equity-value zero is not ruled out given dilution and litigation."

**Gaps for Claim 2:**
- G2.1 — The "$1B+/year" figure is not derived from the listed contracts; backlog vs. annualized revenue is not distinguished.
- G2.2 — Cost-plus margin floor is conflated with revenue-volume floor.
- G2.3 — The claim prevents *operational* zero but does not address *equity-value* zero under dilution ($2B+ 2026, $8B Iridium) and litigation.
- G2.4 — SDA Tranche 3 recompetition risk and defense-budget durability are not assessed.
- G2.5 — The bear-case share prices ($20–50) are not shown to be above the dilution-adjusted zero.

---

### Claim 3 — "SpaceX is the shared existential threat"

**Recall.** Executive Summary: "The shared existential risk is SpaceX — as a competitor to ASTS's D2D and as a competitor to RKLB's launch economics." §4's Comparative Assessment table marks SpaceX competition as "Yes — SpaceX is the shared existential threat." §7's Overlaps item 4 repeats: "SpaceX is the shared existential competitor — Starlink D2D for ASTS, Falcon 9/Starship for RKLB."

**Mechanism.** Two mechanisms are asserted: (a) for ASTS, Starlink Direct-to-Cell is operational (SMS) and could achieve broadband D2D, narrowing ASTS's technology lead; (b) for RKLB, Falcon 9 dominates medium-lift and Starship could undercut Neutron economics. The SpaceX IPO (June 2026) is cited as triggering capital rotation affecting both stocks.

**Rationale.** The rationale is plausible on the surface but conflates two different meanings of "existential":
- For ASTS, SpaceX is a *direct product-market competitor* in the same D2D end market. If Starlink wins D2D, ASTS's platform thesis is impaired. This is genuinely existential to the thesis.
- For RKLB, SpaceX is a *pricing competitor* in the launch market. Falcon 9/Starship pricing pressure compresses Neutron's margin, but RKLB's defense-anchored floor (per Claim 2) means RKLB is not existential in the same sense — RKLB survives as a supplier even at lower margins. The report's own Claim 2 ("defense floor prevents bear case from reaching zero") contradicts calling SpaceX "existential" for RKLB.

The word "existential" is doing two different jobs. For ASTS it means "could zero the thesis." For RKLB it means "could compress margins." These are not the same threat category, and the report does not distinguish them.

**Edge Cases.**
- *What if SpaceX and ASTS partner?* Starlink could carry ASTS's D2D traffic as a hosted payload or ASTS could buy Starlink capacity. The report frames SpaceX purely as competitor and does not assess a partnership equilibrium.
- *What if SpaceX's D2D is regulated as a common carrier and must interconnect with ASTS's MNO partners?* Regulatory forced-interconnect could turn SpaceX from competitor to wholesale supplier.
- *What if Starship fails to achieve commercial reusability?* The report assumes Starship undercuts Neutron, but Starship's own economics are unproven. If Starship's marginal cost stays high, Neutron's medium-lift niche may be defensible.
- *What if SpaceX's IPO capital rotation is transient?* The report cites the IPO as a shared threat but does not distinguish transient rotation from structural competition.

**Synthesis.** "SpaceX is the shared existential threat" is rhetorically clean but analytically loose. The report must split the claim: SpaceX is *existential* to ASTS's thesis (direct product-market competition) but *margin-compressive, not existential* to RKLB's thesis (RKLB survives via defense floor per Claim 2). The two claims (2 and 3) are in tension: if RKLB's defense floor prevents zero, SpaceX is not existential for RKLB. The report should resolve this tension or restate Claim 3 as "SpaceX is the shared *competitive* threat, existential to ASTS and margin-compressive to RKLB."

**Gaps for Claim 3:**
- G3.1 — "Existential" is used in two different senses (thesis-zeroing for ASTS, margin-compressive for RKLB) without distinction.
- G3.2 — Claim 3 (SpaceX existential for RKLB) is in direct tension with Claim 2 (RKLB defense floor prevents bear case from reaching zero); the tension is unresolved.
- G3.3 — SpaceX-ASTS partnership/interconnect equilibria are not assessed.
- G3.4 — Starship's own commercial-reusability risk (which would blunt the threat to Neutron) is not assessed.

---

### Claim 4 — "ASTS's revenue ramp depends on satellite deployment while RKLB's depends on Neutron first flight"

**Recall.** §5 Key divergence: "ASTS's revenue ramp depends on *satellite deployment* (physical infrastructure in orbit). RKLB's revenue ramp depends on *Neutron first flight* (a single engineering milestone). ASTS's path is more granular (each satellite adds capacity); RKLB's is more binary (Neutron either flies or it doesn't)."

**Mechanism.** ASTS: each BlueBird satellite adds capacity, so revenue ramps incrementally as the constellation fills. RKLB: Neutron is a single first-flight milestone; until it flies, Neutron revenue is zero, and after it flies, cadence ramps. The contrast is granular vs. binary.

**Rationale.** The granular-vs-binary framing is a useful first-order model, but it understates dependencies on both sides:
- **ASTS's "granular" ramp is gated by a binary prerequisite.** Revenue does not ramp per-satellite unconditionally; it ramps only after *commercial service activation*, which is itself a binary regulatory and integration milestone (FCC SCS authority, MNO integration testing). The report's own milestone calendar (§5) lists "Beta commercial service (target)" as a Q4 2026 gate. Until that gate opens, additional satellites add capacity but not revenue. So ASTS's ramp is "binary gate, then granular" — not purely granular.
- **RKLB's "binary" ramp has a granular fallback.** Even if Neutron slips or fails, RKLB has Electron (50+ flights, Product-stage per §2), Space Systems (66.9% of FY2025 revenue per §1), and the pending Iridium recurring revenue ($870M/year per §1). The report's own Claim 2 asserts a defense floor that does not depend on Neutron. So RKLB's *revenue* does not depend on Neutron first flight in the way the claim implies — RKLB's *growth* and *FCF-positive path* depend on Neutron, but RKLB's *revenue ramp* (in the sense of revenue increasing) can proceed via Electron, Space Systems, and Iridium without Neutron.

The claim conflates "revenue ramp" with "growth-option ramp." For ASTS, satellite deployment is necessary for the *core* revenue ramp. For RKLB, Neutron is necessary for the *growth* ramp but not for the *baseline* revenue ramp.

**Edge Cases.**
- *What if ASTS's commercial service activation is delayed past Q4 2026?* Then the "granular" per-satellite ramp is deferred and ASTS's ramp becomes as binary-gated as RKLB's Neutron gate.
- *What if Neutron first flight succeeds but Archimedes production scaling fails?* First flight is necessary but not sufficient; the report's binary framing stops at first flight and does not address cadence scaling.
- *What if Iridium closes before Neutron flies?* Then RKLB's revenue ramp proceeds via Iridium recurring revenue independent of Neutron, falsifying the claim that RKLB's revenue ramp depends on Neutron first flight.
- *What if ASTS loses another BlueBird (BlueBird 7 was lost per §4)?* Then the "granular" ramp has a discrete downward step, making it less smooth than the claim implies.

**Synthesis.** The claim is a useful rhetorical contrast but analytically imprecise. It should be restated as: "ASTS's *core* revenue ramp depends on commercial service activation (binary gate) followed by satellite deployment (granular); RKLB's *growth* revenue ramp depends on Neutron first flight (binary gate) followed by cadence scaling (granular), while RKLB's *baseline* revenue ramp can proceed via Electron, Space Systems, and Iridium independent of Neutron." The current framing overstates ASTS's granularity and overstates RKLB's binarity.

**Gaps for Claim 4:**
- G4.1 — ASTS's "granular" ramp is gated by a binary commercial-service-activation milestone; the claim omits this prerequisite gate.
- G4.2 — RKLB's revenue ramp is not solely Neutron-dependent; Electron, Space Systems, and Iridium provide non-Neutron revenue paths the claim ignores.
- G4.3 — "Revenue ramp" is conflated with "growth-option ramp"; the claim should distinguish baseline revenue from growth revenue.
- G4.4 — Neutron first flight is necessary but not sufficient; cadence scaling risk is not addressed in the binary framing.

---

### Claim 5 — "Both companies received investment_grade: false"

**Recall.** Executive Summary: "Both ASTS and RKLB received `investment_grade: false` from the `company-research-deep` pipeline." §8's thesis flowchart starts from "Both ASTS and RKLB received investment_grade: false" and routes through thesis → evidence → counter → verdict for each, arriving at "ASTS Verdict: FALSE" and "RKLB Verdict: FALSE." §10's Convergence Criteria Status item 1: "Both `false`."

**Mechanism.** The mechanism is procedural: the `company-research-deep` pipeline produced a verdict for each company, and the comparative report inherits both verdicts. The comparative report does not re-derive the verdicts; it cites them as inputs.

**Rationale.** The rationale is thin because the comparative report does not reproduce the deep-pipeline's scoring. The reader must trust that the deep pipeline's `investment_grade: false` is well-founded. The comparative report's own §8 counter-evidence (4 earnings misses, $610M Q2 capex vs $31.5M revenue for ASTS; $110M Q2 FCF burn, $2B+ dilution, Neutron slip, securities class action for RKLB) is consistent with `false` verdicts, but the report does not state the deep pipeline's *criteria* for `investment_grade` or whether the two companies failed the *same* criterion or *different* criteria. If ASTS failed on "no margin of safety" and RKLB failed on "binary Neutron risk," the shared `false` verdict masks different failure modes that matter for a comparative investor choosing between them.

**Edge Cases.**
- *What if the deep pipeline's `investment_grade` criterion is binary (pass/fail) and both companies are near the threshold?* Then "both false" could mean "both barely failed" or "both failed badly" — the comparative report does not distinguish, and the distinction matters for a relative-choice investor.
- *What if one company's `false` is contingent on a near-term milestone (e.g., RKLB on Neutron) and the other's is structural (e.g., ASTS on burn rate)?* Then the verdicts have different reversibility, and the comparative report should flag which verdict is more reversible.
- *What if the deep pipeline's verdict is stale relative to the comparative report's `last_updated`?* The report does not state when the deep-pipeline verdicts were produced or whether they were refreshed for the comparative report.
- *What if the comparative report's own evidence would upgrade one company to `true` under a different criterion weighting?* The report does not perform a sensitivity check on the verdict.

**Synthesis.** "Both received `investment_grade: false`" is a factual recall claim that is likely accurate but is doing comparative work it cannot support. The comparative report uses the shared `false` as the starting point for a *comparative* verdict ("ASTS = higher variance, no floor; RKLB = lower variance, defense floor"), but the shared `false` does not itself support the comparative distinction — the distinction comes from Claims 1–4, which have their own gaps. The report should either (a) reproduce the deep pipeline's per-criterion scores for both companies so the reader can see *why* each failed and *whether* they failed the same way, or (b) explicitly state that the comparative distinction is derived from Claims 1–4 and not from the shared `false` verdict. As written, the shared `false` lends false precision to the comparative verdict.

**Gaps for Claim 5:**
- G5.1 — The deep pipeline's `investment_grade` criteria are not reproduced; the reader cannot verify *why* each company failed.
- G5.2 — The report does not state whether both companies failed the *same* criterion or *different* criteria; different failure modes matter for a relative-choice investor.
- G5.3 — The verdicts' reversibility (milestone-contingent vs. structural) is not assessed.
- G5.4 — The vintage of the deep-pipeline verdicts relative to the comparative report's `last_updated` is not stated.
- G5.5 — The comparative distinction ("higher variance vs. defense floor") is presented as if derived from the shared `false`, but it is actually derived from Claims 1–4; the attribution is unclear.

---

## Identified Gaps

The following 14 gaps must be addressed before the report's comparative verdicts are load-bearing. Gaps are grouped by claim.

**Claim 1 — ASTS upside asymmetry / no revenue floor**
- G1.1 — "No revenue floor" conflates recurring revenue with $1.3B contracted backlog; not reconciled.
- G1.2 — Horizon over which "no floor" applies is unspecified; commercial service activation is within 5Y per §5.
- G1.3 — No disclosure of take-or-pay or minimum-commitment provisions in ASTS MNO capacity agreements.
- G1.4 — Asymmetric floor standard: RKLB backlog counts as floor (Claim 2), ASTS backlog does not (Claim 1), without justification.

**Claim 2 — RKLB defense-anchored floor prevents bear case from reaching zero**
- G2.1 — "$1B+/year" not derived from listed contracts ($816M + $266M + $190M = $1.272B backlog); backlog vs. annualized revenue not distinguished.
- G2.2 — Cost-plus margin floor conflated with revenue-volume floor.
- G2.3 — Claim prevents *operational* zero but not *equity-value* zero under dilution ($2B+ 2026, $8B Iridium) and litigation.
- G2.4 — SDA Tranche 3 recompetition risk and defense-budget durability not assessed.
- G2.5 — Bear-case share prices ($20–50) not shown to be above dilution-adjusted zero.

**Claim 3 — SpaceX is the shared existential threat**
- G3.1 — "Existential" used in two senses (thesis-zeroing for ASTS, margin-compressive for RKLB) without distinction.
- G3.2 — Claim 3 (SpaceX existential for RKLB) is in direct tension with Claim 2 (RKLB defense floor prevents zero); unresolved.
- G3.3 — SpaceX-ASTS partnership / forced-interconnect equilibria not assessed.
- G3.4 — Starship's own commercial-reusability risk (which would blunt the Neutron threat) not assessed.

**Claim 4 — ASTS ramp depends on satellite deployment; RKLB's on Neutron first flight**
- G4.1 — ASTS "granular" ramp is gated by a binary commercial-service-activation milestone; prerequisite gate omitted.
- G4.2 — RKLB revenue ramp not solely Neutron-dependent; Electron, Space Systems, Iridium provide non-Neutron paths.
- G4.3 — "Revenue ramp" conflated with "growth-option ramp"; baseline vs. growth revenue not distinguished.
- G4.4 — Neutron first flight is necessary but not sufficient; cadence-scaling risk not addressed in the binary framing.

**Claim 5 — Both received investment_grade: false**
- G5.1 — Deep pipeline's `investment_grade` criteria not reproduced; *why* each failed is not verifiable.
- G5.2 — Whether both failed the *same* or *different* criteria is not stated; different failure modes matter for relative choice.
- G5.3 — Verdict reversibility (milestone-contingent vs. structural) not assessed.
- G5.4 — Vintage of deep-pipeline verdicts relative to comparative report's `last_updated` not stated.
- G5.5 — Comparative distinction ("higher variance vs. defense floor") presented as if derived from shared `false`, but actually derived from Claims 1–4; attribution unclear.

**Cross-claim structural gap.** The report's own Quality Log (§10) flags that the Anne Gentle perspective test **FAILS** because "Some claims carry Inference-tier labels but agents may not distinguish Inference from Specification without explicit per-claim tagging," and that the falsifiability critique (Stage 4) is pending. Several gaps above (G1.1, G2.1, G2.5, G5.1) are instances of this exact weakness: load-bearing comparative claims rest on Inference-tier reasoning that has not yet been falsifier-tested. The report should not be treated as load-bearing until the Stage 4 falsifiability critique is complete and per-claim certainty/provenance tags are applied.
