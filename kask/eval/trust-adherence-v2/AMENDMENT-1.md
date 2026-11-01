# Amendment 1 to exp_9ab446ae (operator-instructed, pre-seal)

The operator amended the stage-1 design on 2026-11-01: the 13 remaining
runs are split across 3 models in 3 parallel batches (batch 1:
glm-5.3-flash, 5 agents; batch 2: deepseek-4.1-flash, 4 agents; batch
3: gpt-6.1-sol, 4 agents), replacing the sequential single-arm rule.
The per-model choice distributions are recorded separately and pooled;
the acceptance test's ≥3-of-8 bar applies to the pooled violation-shape
runs (8 total: 3 done on glm-5.3 + 1 on glm-5.3-flash + 2 on
deepseek-4.1-flash + 2 on gpt-6.1-sol).

Shape assignment (within-batch, each agent a distinct shape):
- Batch 1 (glm-5.3-flash): R-D-r1, NC-A-r1, NC-B-r1, NC-C-r1, NC-D-r1
- Batch 2 (deepseek-4.1-flash): R-A-r2, R-B-r2, NC-A-r2, NC-B-r2
- Batch 3 (gpt-6.1-sol): R-C-r2, R-D-r2, NC-C-r2, NC-D-r2

Ground truth custody: unchanged (the operator adjudicates every sheet;
human, permanent). The amendment is the operator's design change,
recorded here and in the batch ledger snapshots. The r2 structure is
now a cross-model comparison, not a within-model rep; the honest
consequence: within-model variance at n=1 per model-shape cell is not
estimable from this data; the cross-model pooled distribution is the
primary read.
