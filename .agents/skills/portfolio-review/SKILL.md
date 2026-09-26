---
name: portfolio-review
description: "Performance review of a transaction-ledger portfolio: seed prices from live quotes, compute TWR/MWR returns, attribute what moved the portfolio, and record the review as a durable note. Reifies the attribution-review discipline (time-weighted/money-weighted returns + Brinson-style contribution analysis) over the portfolio and companies MCP servers."
---

# Portfolio Review

Review what a portfolio did over a date range: total return (TWR and
Modified Dietz / MWR), what each position contributed, and what the
portfolio now owns. The portfolio server reads prices ONLY from its
seeded cache — this skill is the operational loop for seeding that cache
from live quotes and interpreting the results honestly.

## Reference models and labels

Time-weighted return and Modified Dietz money-weighted return (CFA Institute GIPS standards); contribution analysis; Brinson-Fachler attribution (Brinson & Fachler, 1985) when a benchmark portfolio exists. Every step is D: the portfolio server computes returns and contributions from the seeded price cache and refuses to run on missing prices. The only P elements are the review window and the benchmark choice (the operator's) and the narrative note.

- **Initial condition:** the ledger (`ledger_read`) and the seeded prices for every symbol held at `from` and `to`.
- **Target condition:** `portfolio_returns` and `portfolio_contribution` both succeed for the window, and the note is recorded.

## When to Use

- The operator asks "how did my portfolio do?" over a date range.
- A periodic portfolio review is due (monthly/quarterly attribution).
- After material ledger changes, to recompute returns and attribution.

## When NOT to Use

- The portfolio is a CMP index or prediction-contract portfolio
  (prediction contracts have no traded price — their value is the index
  probability). Use `portfolio_snapshot` for composition only and stop;
  returns are not computable from prices that do not exist.
- The operator only wants current holdings — `portfolio_snapshot` alone
  answers that.

## Instructions

### Phase 1 — Orient

1. Call `portfolio_list` (portfolio server). If empty, stop and say so.
2. Call `ledger_read` with the portfolio name (no filters) to see the
   full transaction history. Identify: holdings (Buy/Sell/Roll), cash
   flows (Deposit/Withdrawal), and the earliest/latest transaction
   dates. Pick the review window `[from, to]` with the operator if not
   given.

### Phase 2 — Seed prices (the loop the servers do NOT run for you)

3. For every symbol held at `from` or `to`, fetch prices via the
   companies server:
   - `stock_quote` for the current price (the `to` date, if `to` is today).
   - `historical_price` with `from`/`to` for the start date.
4. Call `portfolio_seed_price` once per (symbol, date) with the fetched
   close. The resolver is as-of: a price seeded on or before a date
   carries forward (weekends use Friday's close), so seeding the
   `from`-date and `to`-date closes is sufficient for `portfolio_returns`.
5. Call `portfolio_returns` with the window. If it errors naming
   missing prices, that is the honest gate — seed the named
   (symbol, date) pairs and retry. NEVER treat a missing price as zero.

### Phase 3 — Returns and attribution

6. Call `portfolio_materialize_returns` for the window, then
   `portfolio_daily_returns` to read the daily series.
7. Call `portfolio_contribution` (portfolio server) with the window for
   per-security contributions. It refuses to run when a price is missing
   at `from` or `to` and names the (symbol, date) pairs — seed those and
   retry; a missing price is never a zero. Its rows include a
   "Cash / unassigned" residual, so contributions reconcile to total
   profit by construction.
8. Only if the operator names a benchmark portfolio in the same ledger
   store: call `portfolio_attribution` (portfolio server) with
   `benchmark` and the window for Brinson-Fachler allocation, selection
   and interaction effects. Without a benchmark, skip it and say so.
9. Call `portfolio_characteristics` (portfolio server) at the `to`
   date for the composition and weighted metrics of what is owned.

### Phase 4 — Verify and record

10. Convergence gate — call `lisp_eval` with:
   - form: `(and (> start_value 0) returns_ok contribution_ok)`
   - env: `{ "start_value": <returns.start_value>, "returns_ok": <true if portfolio_returns succeeded>, "contribution_ok": <true if portfolio_contribution succeeded> }`
   If false, return to Phase 2 and seed the (symbol, date) pairs the
   failing tool named. Do not report numbers that fail this gate. Bound:
   if a seeding pass leaves the same missing pairs (a quote failed),
   stop and ask the operator — do not loop.
11. Report the top contributors by absolute `contribution_bps`, and the
    size of the "Cash / unassigned" row: a large residual means a
    dividend or ledger effect not assigned to a symbol, worth naming.
12. Call `note_add` (companies server) with portfolio, date = `to`,
    a title like "Portfolio review {from}..={to}", and a body carrying
    total_return, modified_dietz, top-3 contributors, and any
    missing-price caveats. Tags: ["portfolio-review"].

## Constraints

- A missing price is a data gap, not a zero valuation. The servers
  error rather than fabricate; this skill seeds and retries.
- Never invent a price. If a quote fails, surface it and ask the
  operator.
- Dividends are recorded as ledger transactions; `portfolio_contribution`
  assigns a symbol-tagged dividend to its security and anything
  unassigned to "Cash / unassigned" — say which when reporting.
- If any MCP tool call fails, call `curator_report_skill_use_issue`
  with skill_name "portfolio-review", the tool name, and the error;
  continue with the best available information.
