# Private-inference local leg — the position-matched identity protocol (2026-11-06)

The standing recipe for byte-faithful output-identity comparisons on this
substrate, built on the determinism record's §10 finding
(`private-inference-engine-compute-determinism-measurement-2026-11-06.md`):
**temperature-0 completions are fully deterministic given the request
sequence.** The "nondeterminism" that confounded the MTP record (D-G) and
the P2 record (P2-B) was a deterministic, position-dependent server-state
trajectory — byte-identical across independent sessions at matched
positions (20/20 fields, decode speeds 6.6–13.6 tok/s).

## 1. The claim it licenses

Cross-engine (or cross-leg) identity comparisons are valid at MATCHED
REQUEST POSITIONS: same launch flags, same warmup, same prompt sequence →
compare rep-N to rep-N. Byte-identity at matched positions is a real,
reproducible invariant of this substrate. Divergence at matched positions,
between engines, is an engine-level difference — the comparison the prior
records could not make.

## 2. The protocol (frozen)

1. **Launch pin:** identical engine build + flags per the D-S2 exact-commit
   discipline; every flag spelling verified live on each engine's `--help`.
2. **Same warmup:** one discarded warmup — same prompt, same cap — on
   every leg, first request after load.
3. **Same prompt sequence:** the frozen suite bytes, same order (the p0
   suite via `kask/scripts/p0-bench-llama.sh`, or a frozen study spec) on
   every leg. The positions match by construction.
4. **Position-matched comparison:** rep-N vs rep-N on the COMBINED
   content+reasoning surface (the thinking-model discipline), with the
   empty-side refusal and the zero-group guard (the P2 compare discipline).
5. **Settle-first is the alternative** when position-matching is
   impossible: ≥3 same-class settle requests before the measured reps (the
   long-class evidence; settle lengths for other classes are unverified —
   position-matching is the default).
6. **Environment metadata, recorded not adjudicated:** engine-reported
   decode, load, and thermal state (`rocm-smi` junction) recorded per leg.
   The byte-invariance finding (speed-insensitive bytes across
   6.6–13.6 tok/s) is the empirical basis — extreme deviations are
   recorded as limitations on the legs carrying them, not adjudication
   refusals.

## 3. What it does NOT claim

The state-evolution MECHANISM is unknown (a residual question needing its
own pre-registration). The settle length is class-dependent (the long
class: 3; the short classes: unverified beyond 5). Each engine carries its
OWN trajectory — position-matching handles this by construction (same
position, different engine → divergence is engine-level, not position).

## 4. The rebuild constraint (the DME harness)

The Rust rebuild's probe/baseline legs must position-match (or
settle-first) or their baseline counts will drift with session position
exactly as the earlier studies did. This recipe is a design constraint on
the `dme-harness` probe runner: same warmup + same prompt sequence across
every condition being compared, compare at matched positions.

## 5. The typed failures (inherited from the determinism design §6)

`executor_unavailable` (server unreachable / wrong pin),
`protocol_deviation` (any flag or sequence drift),
`measurement_failed` (an empty-side comparison — refused, never
adjudicated).
