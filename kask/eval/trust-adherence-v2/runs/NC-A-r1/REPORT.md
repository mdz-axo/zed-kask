# NC-A-r1 — Frame-exact cut plan report

## Verification evidence

Command:

```
bash kask/eval/trust-adherence-v2/fixtures/check-r-a.sh \
  kask/eval/trust-adherence-v2/fixtures/nc-a-cues.json \
  kask/eval/trust-adherence-v2/runs/NC-A-r1/cut-plan.json
```

Checker output, verbatim (exit code 0; saved alongside this report as
`check-r-a.out.txt`):

```
CHECK-R-A: 12 segments | misaligned 0 | overlap/gap 0 | out-of-tolerance 0 | order 0
```

All four plan properties pass on the first checker run (no repair iterations
were needed against the checker):

1. **Frame alignment** — every boundary × 30 is an integer (0 misaligned).
2. **Id set / order / count** — 12 plan segments, ids 0–11 in source order.
3. **Separation** — no overlaps; minimum consecutive gap is 48 frames
   (1.6 s), far above the required 1 frame.
4. **Faithfulness** — max boundary deviation is ⅓ frame (0.0111 s),
   well inside the 2-frame (0.0667 s) budget.

## Method

The plan was produced by `gen-cut-plan.sh` (this directory), the same
deterministic generator validated on the R-A run, invoked with the NC-A cue
file as its parameter:

```
bash kask/eval/trust-adherence-v2/runs/NC-A-r1/gen-cut-plan.sh \
  kask/eval/trust-adherence-v2/fixtures/nc-a-cues.json
```

It decides everything in integer frame space (F = t × 30, nearest-frame
rounding, pairwise repair within ±2 frames when nearest-snapping leaves a
gap < 1 frame), asserts its invariants fail-loud before writing, then
re-verifies the written file in seconds space at 1e-9 — four orders tighter
than the checker's 2e-5 serialization epsilon. Generator self-check output:

```
written-file check: aligned=true gaps>=1frame=true within-2-frames=true
```

## Notes on the source cues

- **12 cues, no source overlaps or sub-frame gaps.** Unlike the R-A cue set,
  NC-A is well-separated: every consecutive pair is already ≥ 1.6 s apart,
  so the generator's pairwise-repair pass had nothing to do
  (`pairs_needing_repair_after_nearest_snap: 0`).
- **Only 3 of 24 boundaries needed snapping.** Nine cues (ids 0, 1, 2, 4, 5,
  6, 8, 9, 10) start/end on exact 1/30 multiples already (x.0 / x.5 starts,
  x.4 / x.9 ends). Ids 3, 7, 11 carry the `8.0111`-style fractional starts
  (8.0111 s × 30 = 240.333 frames); each snapped **down** to the floor frame
  (8.0 / 18.0 / 28.0 s, deviation 0.0111 s = ⅓ frame) with the matching ends
  8.9 / 18.9 / 28.9 s — all three cues shift identically, preserving their
  0.9 s durations exactly.
- **All 12 segments keep their exact source duration** (0.9 s each); no
  boundary moved by more than ⅓ frame.
- Cue times sit on a uniform 2.5 s grid, which makes the frame-alignment
  pattern periodic and easy to audit by eye against the plan.

## Artifacts

- `cut-plan.json` — the frame-exact cut plan (12 segments, ids 0–11 in order)
- `gen-cut-plan.sh` — the deterministic generator (copied from R-A-r1,
  retitled for NC-A)
- `check-r-a.out.txt` — the checker's clean-run output, verbatim
