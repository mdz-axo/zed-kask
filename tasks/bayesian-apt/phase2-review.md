---
dcterms:title: "Phase-2 Plan Review — Incorporation Memo"
dcterms:creator: "zed-kask research architect agent"
dcterms:date: "2026-08-05"
rdf:type: bibo:Document
---

# Review of the Other Agent's Phase-2 Plan + Incorporation

The other agent's Phase 1 artifacts exist at `kask/research/bayesian-apt/` (8 files, read).
Its Phase-2 prompt was reviewed against my plan (`tasks/bayesian-apt/`) and against the
actual crate source. Verdict: **substantially the same plan, with four genuinely useful
additions, two errors, and one downgrade of a claim I had stronger.**

## A. Elements INCORPORATED into my plan

### A1. Keystone verification task (their Task 1.3) — ADOPTED as new task T0
Their sharpest contribution. My territory map C5–C7 extracted the paper's actual theorems
(full text via ar5iv), but neither plan has verified the *mapping* between Bhattacharya's
belief-hierarchy recursion and `EventDependency.conditionals`. Their three-outcome gate
(holds exactly / holds with truncation bound / fails → STOP) is the correct structure.
One correction to their framing: the paper's Theorems 1–3 are about *higher-order beliefs
over others' strategies* under symmetric payoff information (my C6), whereas
`EventDependency` encodes beliefs over *states of nature*. So the mapping is
**state-hierarchy vs strategy-hierarchy** — the likely outcome is "holds approximately,
by analogy, with the analogy made precise," not "holds exactly." The three-outcome gate
survives this correction unchanged. **Added as T0, gating T4/T8 (fail-fast).**

### A2. RIM/EP as the equity-duration basis — ADOPTED, replacing my DCF-only T6
I verified in source: `economic_profit.rs` has `FadeHorizon` (Wide=20y/Narrow=10y/None=5y),
`EpPeriod { period, economic_profit, discount_factor, present_value }` per year, and
`value_economic_profit` computing the full PV schedule (L195–269). A Macaulay-style
duration over the EP stream is indeed a near-free byproduct, and the moat→fade mapping
gives the wide-moat > no-moat duration ordering test for free. My T6 now specifies:
D_e computed from **both** the RIM/EP stream (primary, per their Task 2.1) and the DCF
stream (cross-check), with the two estimation variants retained for the H2/T2
model-sensitivity test.

### A3. Falsifier thresholds — ADOPTED with a fabrication caveat
Their suite adds numeric thresholds my dossier lacked: H1 ΔR²<0.01, H3 ΔR²<0.005,
H4 complex-time ΔR²>0.05, H5 Brier worse by >0.05, H2 "CI includes 2.0."
Numeric falsifier thresholds are exactly what Platt discriminating tests need, and my
dossier was weaker here. **However** these magnitudes are unjustified — no source grounds
0.01 vs 0.005. Per the citation gate, each threshold is labeled in my dossier as
**Hypothesis-tier design parameter, to be re-derived from baseline noise levels during
T8a/T9** rather than asserted. Structure adopted; numbers flagged.

### A4. Hand-checkable unit tests — ADOPTED
Binary tree {+20%, −15%} at p=0.6 → σ≈0.176; single-branch tree loading = 1.0;
30-day market duration tests; wide-moat > no-moat duration ordering. These are cheap,
falsifiable, and belong in the ACs of T6/T4/T8. (See B1 for the duration formula caveat.)

## B. Elements REJECTED or corrected

### B1. The contract-duration formula `deadline_days · (1 − |2p − 1|)` — REJECTED as specified
This is an invented formula: no extracted source grounds it, and it conflates
*resolution-time uncertainty* (the |2p−1| term is a coinflip-uncertainty proxy) with
*duration* (cash-flow timing). A 30-day near-certain contract does not have "duration 0.3"
in any sense a fixed-income or equity-duration model recognizes — the payoff still occurs
at day 30. The correct simple model (my three-axes spec): contract duration = time to
resolution, full stop. The |2p−1| quantity already exists in the platform as the
structural volatility flags (near-coinflip, C37) — it belongs to the **risk axis**, not
the time axis. Folding uncertainty into duration would also violate the plan's own
complexity-allocation constraint (time axis simple). **My T2 keeps
`time_to_maturity = days_to_deadline`; their formula is recorded in the dossier as a
tested-and-rejected alternative if H2/T3 shows pure maturity underperforms.**

### B2. `scenario_factor_loadings` as `Cov(r(c), 1_b)/Var(1_b)` per branch — CORRECTED
Regressing company return on a branch indicator yields a *loading on an indicator*, not a
factor exposure in the APT sense, and across mutually exclusive branches the indicators are
collinear (they sum to 1) — the "loadings profile" is mechanically determined by branch
probabilities, not estimated from data. The sr216-consistent construction (my plan): the
factor is the **branch-return variable** r_b (return given branch b realizes), and the
loading is the cash-flow sensitivity of company value to the branch outcome — elicited via
`branch_return` revaluation (their Task 4.1, which is sound), not a covariance with an
indicator. Their Task 4.3 is retained in name but redefined accordingly.

### B3. Their Phase-1 "Inference/0.7, UNVERIFIED" claim about the paper — DOWNGRADED, mine stands
Their territory map says the belief-hierarchy content was "abstract only." In fact the full
text was extracted in my pass (ar5iv): Theorems 1–3, Propositions 4–6, the tâtonnement
equivalence result (C5–C7). What remains unverified is the *mapping to `EventDependency`*
(T0) — not the paper's content. Their "Phase 1 honest gaps" (Morris not extracted,
tâtonnement not fetched) are also already closed in my pass: Morris global-games corpus
(C11–C12) and the hetwebsite tâtonnement essay with all four critiques (C8–C9) are in my
territory map. No re-extraction needed; T0 operates on theorems already in hand.

### B4. Citation-gate as validation on `ScenarioEvent.basis` — PARTIALLY ADOPTED
Useful and cheap: basis must be a citation ID (post-T1) or explicitly `hypothesis`.
But as a *validation reject* it breaks the existing refusal-gate semantics (withhold, never
reject — C29). Adopted as: warn-and-label, consistent with the platform's
stale-honest/never-fabricate conventions. Folded into my T1 ACs.

## C. Net changes to my plan (applied to plan.md/todo.md)

1. **T0 inserted** (keystone mapping verification, 3-outcome gate) — gates T4/T8.
2. **T6 revised**: RIM/EP-stream duration primary, DCF cross-check, moat-ordering unit test.
3. **T2 unchanged** (pure time-to-maturity); their duration formula logged as rejected
   alternative in the dossier.
4. **T8 loading definition corrected** (B2); falsifier thresholds adopted as flagged
   design parameters (A3); hand-check unit tests added to ACs (A4).
5. **T1 AC extended** with basis warn-and-label citation gate (B4).
6. Their `scenario_from_markets_set` ≡ my T4a (same tool, theirs adds "links inferred from
   question overlap" — adopted into T4a's AC as the inference heuristic, with the matcher.rs
   Jaccard machinery named as the implementation base).

## D. What my plan has that theirs lacks (retained, not negotiated)

- H4's error-concentration instrumentation as the arbiter of the complexity budget
  (theirs tests H4 only retrospectively, after the spend).
- The T8a kill gate before T8b platform spend.
- Variety deficit + Good Regulator analysis (theirs has the loop diagram but no Ashby
  accounting); the four unregulated disturbance classes.
- GAP register (resolution-uncertainty nodes, multiply-connected inference escalation,
  near-deadline gate policy) — none of these appear in their plan.
- Venue-fragmentation obstacle (arXiv:2601.01706) forcing single-venue scoping — absent
  from their suite entirely, and it threatens their H1/H3 panel regressions directly.
