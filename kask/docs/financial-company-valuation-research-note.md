# Financial Company Valuation: Canonical Methods and Integration Plan for the Deep Research Pipeline

## Research Note — August 21, 2026

---

## 1. The Problem

The COF deep research report ran the `ep_valuation` MCP tool, which computed:

- **ROIC: 0.27%** (Return on Invested Capital)
- **WACC: 10%**
- **Signal: "overvalued (value destroyer)"**
- **Invested Capital: $689.3B** (raw), **-$357.2B** (adjusted — negative!)

This signal is **completely wrong for a bank**. The EP valuation tool uses the industrial-company formula:

> Invested Capital = Total Assets - Total Debt (deposits + borrowings)

For a bank, **deposits are the raw material of the business** — they are operating inputs, not financing decisions. Subtracting deposits from invested capital produces a nonsensical negative number and a meaningless ROIC. The "value destroyer" signal is a category error, not a real finding.

---

## 2. Canonical Methods for Financial Company Valuation

### 2.1 Damodaran: Equity Valuation, Not Enterprise Valuation

**Source:** Aswath Damodaran, "Valuing Financial Service Firms" (SSRN 2017; Chapter 21 of *Investment Valuation*; NYU Stern lecture notes)

> "Financial services firms should be valued using **equity valuation models**, rather than enterprise valuation models, and with actual or potential dividends used as cash flows."

Damodaran identifies three fundamental problems with applying industrial valuation to financial firms:

1. **Debt is a raw material, not a financing choice.** For a bank, deposits and wholesale funding are the inventory — the operating inputs. Subtracting them from invested capital is like subtracting raw materials from a manufacturer's invested capital. It produces a meaningless number.

2. **Capital expenditures and working capital are not cleanly separable.** For a bank, "reinvestment" is lending money (adding to the loan book), which is simultaneously the operating activity and the capital deployment. You cannot separate "operating" from "investing" cash flows the way you can for an industrial company.

3. **The definition of "debt" is ambiguous.** For a bank, what counts as "debt"? Deposits? Wholesale borrowings? Subordinated debt? The industrial distinction between operating liabilities and financial debt breaks down.

#### Damodaran's Three Approaches for Banks:

| Approach | Cash Flow | Discount Rate | Notes |
|---|---|---|---|
| **Dividend Discount Model** | Dividends (actual or potential) | Cost of Equity | Simplest; assumes firms pay out FCFE as dividends over time |
| **Modified FCFE** | Net Income - Increase in Regulatory Capital (Book Equity) | Cost of Equity | Adjusts for the fact that banks must hold regulatory capital; the reinvestment is the growth in required equity, not capex + working capital |
| **Residual Income (EBO)** | Residual Earnings = Net Income - (Cost of Equity × Book Value of Equity) | Cost of Equity | Value = Book Value of Equity + PV(Future Residual Earnings); most analytically rigorous for banks |

**Key formula for Modified FCFE (banks):**

```
FCFE_bank = Net Income - (Change in Regulatory Capital)
```

where Regulatory Capital = Book Equity (CET1, Tier 1, or Total Capital depending on the regulatory framework). This replaces the industrial formula:

```
FCFE_industrial = Net Income - (Capex - Depreciation) - ΔWorking Capital - (Debt Repaid - New Debt Issued)
```

### 2.2 Residual Income Model (Edwards-Bell-Ohlson)

**Sources:** Edwards & Bell (1961), Ohlson (1995), Begley/Chamberlain/Li (2006) for banks specifically

The Residual Income Model (RIM) is the canonical approach for bank valuation:

```
Value = Book Value of Equity + Σ [Residual Earnings_t / (1 + r)^t]
```

where:

```
Residual Earnings_t = Earnings_t - (r × Book Value of Equity_{t-1})
                    = (ROE_t - r) × Book Value of Equity_{t-1}
```

- **r = Cost of Equity** (not WACC)
- **ROE = Return on Equity** (not ROIC)
- **Book Value of Equity = Invested Capital** (not Total Assets minus Debt)

For Capital One specifically:
- **ROTCE** (Return on Tangible Common Equity) is the sell-side standard metric, not ROIC
- **Cost of Equity** should be estimated via CAPM or multi-factor models, not WACC
- **P/TBV** (Price to Tangible Book Value) is the standard multiple, not P/E or EV/EBITDA
- The "economic profit" spread is **(ROTCE - Cost of Equity)**, not **(ROIC - WACC)**

### 2.3 Massari, Gianfrate, Zanetti (Wiley 2014)

**Source:** "The Valuation of Financial Companies: Tools and Techniques to Value Banks, Insurance Companies, and Other Financial Institutions" — the only dedicated textbook on financial company valuation.

Key contributions:
- Formal framework for distinguishing operating liabilities from financing debt in financial firms
- Equity Cash Flow (ECF) method adapted for banks: ECF = Net Income - ΔRegulatory Capital
- Residual Income approach applied to bank financial statements
- Treatment of loan loss provisions as operating expenses (not non-cash charges)
- Regulatory capital as the "reinvestment" metric for banks

### 2.4 What the EP Valuation Tool Should Have Computed for COF

| Metric | Industrial (Wrong) | Financial (Correct) |
|---|---|---|
| Invested Capital | Total Assets - Debt = ~$0 or negative | **Equity Capital (Book Value) = ~$133.9B** |
| Return Metric | ROIC = NOPAT / Invested Capital = 0.27% | **ROE/ROTCE = Net Income / Equity ≈ 15-20%** |
| Cost of Capital | WACC = 10% (dragged down by cheap deposits) | **Cost of Equity ≈ 11-13%** (CAPM: beta ~1.0, ERP ~5.5%, Rf ~4.5%) |
| Economic Profit Spread | ROIC - WACC = -9.7% (value destroyer) | **ROE - Cost of Equity ≈ +4-7%** (value creator on normalized earnings) |
| Valuation Signal | "overvalued (value destroyer)" | **"fairly valued to undervalued"** depending on post-Discover normalization |
| FCFE | Net Income - Capex - ΔWC - Net Debt = meaningless | **Net Income - ΔRegulatory Capital ≈ $3B - $0 = ~$3B/qtr** (capital is above need) |

---

## 3. What Went Wrong in the COF Report

### 3.1 The EP Valuation Tool Category Error

The `ep_valuation` MCP tool (Bergen et al. 2025, FAJ — Residual Income Model) correctly uses the residual income framework, but its **invested capital calculation is hardcoded for industrial companies**:

```json
{
  "invested_capital_raw": -357154000000.0,  // NEGATIVE — deposits subtracted
  "invested_capital": 689301000000.0,        // total assets (not equity)
  "roic": 0.002693521312867241,              // ~0.27% — meaningless for a bank
  "wacc": 0.1,                                // WACC, not cost of equity
  "signal": "overvalued (value_destroyer)"    // category error
}
```

The tool's balance sheet adjustment says:
> "hKask non-standard treatment: Treasury Stock is treated as committed capital..."

This is a real adjustment, but the fundamental problem is upstream: the tool treats bank deposits as debt to be subtracted from invested capital, when for a bank they are operating inputs.

### 3.2 The Working Capital Cycle Tool

The `working_capital_cycle` MCP tool returned:
```json
{
  "cfo_working_capital_rating": "stable",
  "spread_stability": 1.0,
  "periods": [],
  "data_points": 0
}
```

Zero data points. Working capital cycle analysis (days payable, days sales outstanding, cash conversion cycle) is **meaningless for a bank** — banks don't have inventory, accounts receivable, or accounts payable in the industrial sense. The tool should detect financial-sector companies and either return a bank-specific liquidity analysis (loan-to-deposit ratio, liquidity coverage ratio, net stable funding ratio) or explicitly flag as not applicable.

### 3.3 Downstream Propagation in the Pipeline

The wrong EP valuation signal propagated through:
- **`company-8part.j2` Step 4 (Financial Profile):** The "normalization" stage cited the EP model's "value destroyer" signal, which I correctly identified as a brownout artifact but couldn't fix because the tool's output was the only data source.
- **`valuation-8step.j2` Step 7a:** If the pipeline had used this template, the DCF valuation would have used WACC as the discount rate, which is wrong for a bank. The cost of equity should be used.
- **`thesis-three-pillars.j2` Valuation pillar:** The 3-stage valuation used "ROIC vs. WACC" framing, which is wrong for a bank.

---

## 4. Proposed Integration Plan

### 4.1 Sector Detection Gate (New Step 0)

Add a sector detection step at the very beginning of the pipeline, before any MCP tool calls:

```
IF company.sector IN ("Financial Services", "Banks", "Insurance", "Credit Services")
    → financial_company_mode = true
    → route to equity valuation methods
    → flag industrial-company tools as inapplicable
```

Detection can use the `company_profile` MCP tool's `sector` and `industry` fields. COF's profile returns:
```json
{"sector": "Financial Services", "industry": "Financial - Credit Services"}
```

### 4.2 EP Valuation Tool: Financial Company Mode

The `ep_valuation` MCP tool needs a `sector_type` parameter (or auto-detection from the company profile):

| Parameter | Industrial Mode (current) | Financial Mode (proposed) |
|---|---|---|
| Invested Capital | Total Assets - Total Debt | **Book Value of Equity** (or Tangible Common Equity) |
| Return Metric | ROIC = NOPAT / Invested Capital | **ROE = Net Income / Book Value of Equity** (or ROTCE) |
| Cost of Capital | WACC | **Cost of Equity** (CAPM: Rf + β × ERP) |
| Economic Profit | (ROIC - WACC) × Invested Capital | **(ROE - Cost of Equity) × Book Value of Equity** |
| FCFE | Net Income - Capex - ΔWC - Net Debt | **Net Income - ΔRegulatory Capital** |
| Signal | value_creator / value_destroyer | Same, but computed on equity metrics |

### 4.3 DCF Valuation Tool: Financial Company Mode

The `dcf_valuation` MCP tool needs a financial-company mode that replaces:
- FCFF discounted at WACC → **FCFE (dividends or modified FCFE) discounted at Cost of Equity**
- Terminal value based on perpetuity of FCFF → **terminal value based on perpetuity of residual earnings or dividends**
- Revenue growth + margin assumptions → **loan growth + NIM + credit cost assumptions**

### 4.4 Working Capital Cycle Tool: Financial Company Mode

For financial companies, replace the working capital cycle analysis with:
- **Loan-to-deposit ratio** (LDR)
- **Liquidity Coverage Ratio** (LCR)
- **Net Stable Funding Ratio** (NSFR)
- **Deposit growth and mix** (retail vs wholesale, insured vs uninsured)
- **Net interest margin** (NIM) stability
- **Asset-liability duration gap**

Or simply flag as "Not Applicable — see liquidity analysis" and redirect to bank-specific metrics already available in the earnings transcript.

### 4.5 Expectations Gap Tool: Financial Company Mode

The `expectations_gap` tool's reverse DCF should use:
- **Equity free cash flow** (dividends or modified FCFE), not FCFF
- **Cost of equity** as the discount rate, not WACC
- **Book value growth** as the implied growth metric, not revenue growth
- **P/TBV** as the valuation multiple for cross-checking, not P/E or EV/EBITDA

### 4.6 Template Changes

#### `company-8part.j2` — Financial Profile Section

Replace the current 3-stage valuation with:

**For industrial companies (current):**
1. Consensus: P/E, EV/EBITDA, analyst targets
2. Normalization: ROIC vs. WACC, EP model
3. Terminal: DCF with FCFF, WACC, terminal growth

**For financial companies (proposed):**
1. Consensus: P/TBV, P/E (for profitable banks), analyst targets, ROTCE
2. Normalization: ROTCE vs. Cost of Equity, Residual Income Model (EBO)
3. Terminal: Dividend Discount Model or Residual Income with cost of equity, terminal book value growth

#### `valuation-8step.j2` — Financial Company Track

Add a parallel track for financial companies:

| Step | Industrial | Financial |
|---|---|---|
| 7a | DCF (FCFF, WACC) | **DDM or Modified FCFE, Cost of Equity** |
| 7b | Comparable Analysis (EV/EBITDA, P/E) | **Comparable Banks (P/TBV, P/E, ROTCE)** |
| 7c | Expectations Gap (reverse DCF on FCFF) | **Expectations Gap (reverse DDM on dividends/FCFE)** |
| 7d | Scenario Impact (FCFF sensitivity) | **Scenario Impact (FCFE sensitivity to NIM, credit costs, capital ratios)** |

#### `lens-five-frameworks.j2` — Lens 1 (The Loop)

Add financial-company variants:
- Value = Profits / (r - g) → for banks: **Value = BV × (ROE / (Cost of Equity - g))** where g = sustainable book value growth
- Target return > 12% → for banks: **Target ROTCE > Cost of Equity + 3%** (economic profit margin)
- Max P/E < 25x → for banks: **Max P/TBV < 2.0x** (typical range for well-run banks: 1.0-2.5x)
- ROIC vs. WACC → for banks: **ROE/ROTCE vs. Cost of Equity**

### 4.7 SKILL.md Changes

Add a new section after "When to Use":

```markdown
## Financial Company Detection

When the company_profile MCP tool returns a sector in
("Financial Services", "Banks", "Insurance", "Credit Services", "Diversified Financials"),
the pipeline MUST:

1. Set financial_company_mode = true before any valuation MCP tool calls
2. Replace ROIC/WACC with ROE/Cost of Equity in all valuation steps
3. Replace FCFF with FCFE (dividends or modified FCFE = Net Income - ΔRegulatory Capital)
4. Replace EV/EBITDA with P/TBV in comparable analysis
5. Replace working capital cycle with liquidity analysis (LDR, LCR, NSFR, NIM stability)
6. Use Book Value of Equity as invested capital, NOT Total Assets minus Debt
7. Flag the EP valuation tool's ROIC signal as inapplicable and use the Residual Income (EBO) model instead

This is a hard gate — applying industrial-company valuation formulas to financial
companies produces category errors (negative invested capital, meaningless ROIC,
misleading WACC). The deposits and wholesale funding of a bank are operating
inputs (raw materials), not financing decisions.
```

### 4.8 .rules Addition

Add to `.rules`:

```
## Financial company valuation

* The `ep_valuation` tool's ROIC/WACC signal is INVALID for financial-sector companies
  (banks, insurance, credit services). Deposits are operating inputs (raw material),
  not debt. For financial companies: invested capital = book value of equity, return
  metric = ROE/ROTCE, cost of capital = cost of equity (not WACC). The tool must detect
  financial sector via company_profile and switch to the Residual Income (EBO) model.
* DCF valuation for financial companies must use FCFE (dividends or Net Income - ΔRegulatory
  Capital) discounted at cost of equity, not FCFF discounted at WACC.
* Working capital cycle analysis is not applicable to financial companies. Use loan-to-
  deposit ratio, liquidity coverage ratio, net stable funding ratio, and NIM stability instead.
* Comparable analysis for financial companies uses P/TBV and ROTCE, not EV/EBITDA and ROIC.
* The LENS "Value = Profits / (r - g)" framework must use book value and ROE for banks:
  Value = BV × (ROE / (Cost of Equity - g)), not FCFF-based DCF.
```

---

## 5. Sources

| Source | Key Contribution | Relevance |
|---|---|---|
| Damodaran, "Valuing Financial Service Firms" (SSRN 2017, Chapter 21) | Equity valuation models, not enterprise; dividends as cash flows | Primary methodology |
| Massari, Gianfrate, Zanetti, "The Valuation of Financial Companies" (Wiley 2014) | Dedicated textbook; ECF and RI for banks | Comprehensive treatment |
| Edwards & Bell (1961), Ohlson (1995) | Residual Income Model (EBO) | Canonical equity valuation model |
| Begley, Chamberlain, Li (2006) "Modeling Goodwill for Banks: A Residual Income Approach" | RIM applied specifically to banks | Empirical validation |
| SCIRP (2017) "Understanding Bank Valuation: ECF and RI Approach" | Practical application of both methods | Implementation guide |
| Damodaran, NYU Stern lecture notes (finsvc.pdf) | FCFE_bank = Net Income - ΔRegulatory Capital | Modified FCFE formula |

---

## 6. Summary

The COF deep research report's EP valuation signal ("overvalued value destroyer, ROIC 0.27% vs. WACC 10%") is a **category error** caused by applying industrial-company valuation formulas to a bank. Bank deposits are operating inputs (raw materials), not financing debt. The correct approach for financial companies uses:

- **Invested Capital = Book Value of Equity** (not Total Assets minus Debt)
- **Return Metric = ROE/ROTCE** (not ROIC)
- **Cost of Capital = Cost of Equity** (not WACC)
- **Cash Flow = FCFE = Net Income - ΔRegulatory Capital** (not FCFF)
- **Valuation Multiple = P/TBV** (not EV/EBITDA)
- **Valuation Model = Residual Income (EBO) or Dividend Discount Model** (not FCFF DCF)

The integration plan proposes a sector detection gate at pipeline entry, financial-company modes for the EP/DCF/working-capital/expectations-gap MCP tools, parallel tracks in the valuation and company-analysis templates, and new `.rules` entries to prevent the category error from recurring.