---
name: calibration-stewardship
description: "Maintain the prediction-market calibration loop: run resolution scans so pre-resolution price snapshots accumulate, pair websocket resolution notifications with manually recorded observations, read per-bucket Brier scores, and verify that poorly-calibrated buckets are demoted. Reifies the Tetlock record-score-recalibrate cycle applied to market domain buckets, over the prediction-markets MCP server."
---

# Calibration Stewardship

The prediction-markets server annotates every market probability with a
reliability tier derived from per-bucket Brier scores. That signal is
only as good as the observations feeding it — and the honest observation
is the price the scanner FIRST saw, never the post-resolution price.
This skill is the maintenance procedure for that loop.

## Reference models

Brier (1950) — `onto_anchor` → derived `brier_score`; calibration in Tetlock & Gardner's sense (2015) — derived `forecast_calibration`: stated confidences match observed frequencies across many resolved cases.

**D/P labelling.** Every tool call and the Convergence gate are D (the server computes snapshots, Brier and tiers; `lisp_eval` evaluates the gate). The cadence recommendation is P and is the operator's decision. The one judgment input that is not tool-produced — an operator's own pre-resolution observation in Phase 3 — is recorded as the operator's, never inferred.

## When to Use

- Periodic stewardship of the calibration store (the operator asks for
  a calibration health check, or a review cycle is due).
- After running `market_check_resolutions`, to interpret the counts and
  act on them.
- A Polymarket websocket notification arrived and needs recording.
- Before relying on `market_lookup` reliability tiers for a forecast —
  check how fresh the underlying calibration is.

## When NOT to Use

- A one-off market lookup — the tiers are already annotated on records.
- You want to score your OWN forecasts — that is the scenarios server's
  `scenario_score` / `scenario_calibration` loop.

## The loop and why it is two-phase

`market_check_resolutions` works in two phases: (1) every OPEN market's
current price is snapshotted as the pre-resolution
probability-at-observation (the earliest snapshot per market is kept),
and (2) newly resolved markets consume their snapshot — the Brier loop
scores the price the scanner first saw. A market that resolves before
its first scan is counted in `resolved_without_snapshot` and skipped:
its terminal price is the outcome declaration, not an observation, and
scoring it would be self-fulfilling (Brier ≈ 0 by construction).

Consequence: scans must run OFTEN ENOUGH that open markets get
snapshotted before they resolve. A high `resolved_without_snapshot`
rate means the scan cadence is too slow — that is the primary signal
this skill manages.

## Instructions

### Phase 1 — Scan

1. Call `market_check_resolutions` (prediction-markets) with a limit
   (default 100; raise toward 500 for a catch-up scan after a gap).
   Read the output: `snapshotted` (new open-market snapshots),
   `recorded` (resolutions consumed from snapshots),
   `resolved_without_snapshot` (resolutions that arrived too late),
   `skipped_ambiguous` (50-50 resolutions, never fabricated),
   `already_known` (idempotent re-observations), `warnings`.
2. If `resolved_without_snapshot` is a large fraction of
   (`recorded` + `resolved_without_snapshot`), tell the operator the
   scan cadence is too slow and recommend a shorter interval.

### Phase 2 — Read the signal

3. Call `market_calibration` for each bucket of interest (e.g.
   "politics", "economics", or a series ticker). A bucket with no
   resolved data returns `stale: true` — never a synthetic Brier of 0.
   Report stale buckets as unknown, not as well-calibrated.
4. For each non-stale bucket, report the Brier score with its
   interpretation (excellent < 0.05, good < 0.10, fair < 0.20,
   poor < 0.33) and the sample size. Small samples are weak evidence —
   say so.

### Phase 3 — Notifications and manual recording

5. A `market_subscribe_resolutions` notification carries NO
   pre-resolution probability (the wire cannot carry one without
   fabricating it). To record a resolution from a notification, obtain
   the probability-at-observation from the pending snapshot journal
   (the scan's Phase 1 output) or the operator's own recorded
   observation, then call `market_record_resolution` with
   (bucket, probability, outcome). Never pass the terminal price as
   the probability.
6. If the operator has their own observation of a market's
   pre-resolution price, `market_record_resolution` is the manual arm —
   use it directly with the operator's number.

### Phase 4 — Verify the act arm

7. The server's demotion rule (`reliability_tier`,
   `hkask-mcp-prediction-markets/src/types.rs`) fires only when the
   bucket is not stale, has ≥ 5 resolved markets, and Brier > 0.25; it
   lowers High to Medium and leaves Medium and Low unchanged. Pick a
   bucket that meets all three from Phase 2, and call `market_lookup`
   for a market in it whose volume and spread would otherwise rate
   High (volume ≥ 50,000, spread ≤ 0.04). That record should read
   Medium; if it reads High, report the discrepancy — the demotion gate
   is not firing. A Medium or Low record in a poor bucket is not
   evidence either way. If no bucket meets the three conditions, the
   act arm is untested this run — say so, do not report it verified.

### Convergence

8. Gate — call `lisp_eval` with:
   - form: `(and (eq stale_buckets 0) (< without_snapshot_rate 0.2))`
   - env: `{ "stale_buckets": <buckets of interest reporting stale>,
            "without_snapshot_rate": <resolved_without_snapshot / (recorded + resolved_without_snapshot)> }`
   The loop is healthy when no bucket of interest is stale and the
   too-late rate is under 20%. Otherwise, recommend (or schedule, if
   the operator asks) a more frequent scan and re-run Phase 1.
   Bound: one cadence adjustment per stewardship run; if the next scan
   still shows `without_snapshot_rate` ≥ 0.2, report the cadence
   problem to the operator instead of re-running.

## Constraints

- NEVER record a post-resolution price as the probability-at-observation.
- Ambiguous (50-50) resolutions are skipped by design — do not retry
  them hoping for a different result.
- The scan is idempotent; re-running it is always safe.
- If any MCP tool call fails, call `curator_report_skill_use_issue`
  with skill_name "calibration-stewardship", the tool name, and the
  error; continue with the best available information.
