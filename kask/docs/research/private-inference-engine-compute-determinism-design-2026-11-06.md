# Private-inference local leg — engine-compute determinism, pre-spend research design (2026-11-06)

`design-spec-v1`-shaped (the experimental-design contract), authored BEFORE
any spend; the operator gates every run on this design. The question is the
P2 record's named follow-up (§5) and the precondition for any future
identity-instrument trust — both the MTP and P2 records were
instrument-confounded on it.

## 1. The question and its inherited framing

Why do 256-token temperature-0 completions flip knife-edge tokens between
reps at reduced context (ctx 16384/32768) while 1024-token completions on
the same engine can be byte-stable, and why does within-leg determinism
hold at ctx 65536? The framing and rival ledger are INHERITED from the two
measurement records (not regenerated): D-G (the MTP record — the
slot-KV/preserved-reasoning state hypothesis) was refined by P2-B (the P2
record — the full fresh-slot protocol did NOT restore within-leg
determinism at reduced ctx on either engine; the resident's long class IS
byte-stable, 4350 chars × 3, at the same ctx 16384; short-class reasoning
diverges at bytes ~310–316 across reps).

## 2. The rival hypotheses (each predicts a distinct observation)

- **H1 — compute-path nondeterminism coupled to ctx/prompt layout.** At
  reduced ctx the engine selects per-request compute paths (ubatch splits
  in prefill, attention tiling over the ctx-sized buffer, MoE gate ties)
  whose floating-point reduction orders vary run-to-run; knife-edge tokens
  flip. At ctx 65536 the same prompts select stable paths. *Predicts:*
  stability tracks ctx AND prompt-partition alignment with everything else
  fixed, and the divergence-depth set is a narrow band (a fixed knife-edge
  position ± numeric noise — the P2 ~310–316 shape), not a geometric
  spread.
- **H2 — completion-cap/scheduler coupling.** The max_tokens cap changes
  the decode scheduler's batch composition (tokens in flight, speculative
  lookahead), perturbing shared reductions. *Predicts:* at FIXED prompt
  and ctx, varying ONLY the cap flips determinism (256 → flips, 1024 →
  byte-stable), with reps identical up to the flip byte.
- **H3 — the observation confound (the instrument's own class).** The
  256-vs-1024 contrast is carried by the prompt-class difference (the
  P0 suite's P1/P2/P3 prompts vs the long leg's own prompt), not the cap.
  *Predicts:* at fixed prompt, the cap alone does not move stability;
  stability tracks the prompt.
- **H4 — residual pool/workspace state (the D-G refinement).** Not slot
  KV (P2-B refuted that) but the compute-pool allocator's reuse evolving
  across requests at reduced ctx. *Predicts:* divergence depth correlates
  with rep ORDER within a leg, and a process restart between reps resets
  the pattern.

Falsifiers, pre-declared: H2 is falsified if the cap does not move
stability at fixed prompt+ctx; H1 by O2 showing no alignment coupling; H4
by O4 showing no order correlation; H3 by O1 showing cap-coupled
stability. An observation matching NO rival's prediction sends the
design back for new rivals — it is never bent to fit.

## 3. The instrument gate FIRST (the bounded calibration sub-experiment)

Before any factor arm: re-establish the premise observation on the
hardened P2 parity harness (`kask/scripts/`, `ac40450c3f` — the
zero-class guard, the SYS one-home, the combined content+reasoning
surface, the empty-side refusal), resident engine, ctx 16384, the P1
class + the long class, 5 reps. **Acceptance criterion:** the P2-B
asymmetry REPRODUCES (short-class flips; long-class byte-stable) — else
the design halts: the premise itself is unstable, the confirmatory layer
is a measurement_void, and the operator decides whether to re-ground.
Budget: ≤10 requests.

## 4. The candidate designs

- **C1 (selected) — the one-factor-at-a-time factor sweep.** Arms: O1,
  fixed prompt+ctx with caps {128, 256, 512, 1024} × 5 reps (H2); O2, the
  same prompt padded to lengths ≡ k (mod n_ubatch) × 3 reps (H1); O3, the
  same arm at ctx {16384, 32768, 65536} (H1's ctx coupling — replicating
  D-G's boundary under the hardened instrument); O4, rep-order analysis
  within O1's legs plus one restart arm (H4). Every factor is
  engine-controllable through request parameters; the harness exists; the
  spend is one session.
- **C2 (rejected — the strongest alternative) — differential logit
  logging.** Instrumented engine builds logging per-token logit margins to
  observe knife-edge tokens directly. Rejected for the confirmatory phase:
  ad-hoc builds break the D-S2 exact-commit pin discipline, the observer
  effect changes the compute being measured, and the RELEASE builds are
  the surface whose determinism is in question. Kept as the named follow-up
  if C1 localizes to a mechanism needing direct observation.
- **C3 (rejected) — the multi-engine sweep.** Running every arm on all
  three engines multiplies factors without isolating one; the resident
  mainline build alone answers the question the identity instrument needs.
  Cross-engine variation is exploratory context, never confirmatory.

## 5. Preregistered analysis, stopping, and the spend bound

Primary observable per arm: byte-identity of the combined
content+reasoning surface across reps (all-same / not-all-same) plus the
divergence-depth set. Decision rules frozen here: H2 corroborated iff O1
moves stability with the cap at fixed prompt+ctx (and the 1024 arm is
byte-stable); H1 corroborated iff O2 shows alignment-coupled stability at
fixed cap; H4 iff O4 shows order-coupled divergence that a restart
resets; H3 iff O1 shows no cap coupling (stability tracks the prompt).
Fixed-N, NO interim peeking, no adaptation: gate ≤10 + O1 20 + O2 ≤15 +
O3 9 (+2 relaunches) + O4 riding O1 + one restart arm ≤15 — **≤ ~70
requests, one session, no new hardware.** The fresh-slot protocol (single
slot, --no-reasoning-preserve, --no-cache-prompt, spellings verified live
on the engine's --help per the P2 discipline) holds for every arm.

## 6. Executor binding and typed failures

The resident mainline HIP engine at the canonical root (the P1 launch
discipline), raw capture through `kask/scripts/p0-bench-llama.sh` (the
first-content-line TTFT repair landed 2026-11-06, `e858888a6f`) and the
identity observable through the parity harness's comparison discipline.
The pinned forks (the qwen4exp MTP fork `6fcaa16`, the ServeurpersoCom
fork `7174388`) enter only if the operator extends scope. Typed
failures: `executor_unavailable` (server unreachable or the wrong version
pin), `protocol_deviation` (any flag drift), `measurement_failed` (an
empty-side comparison — refused, never adjudicated).

## 7. Outcome rule and limitations

Per hypothesis, the typed-outcome vocabulary: corroborated / rejected /
inconclusive / measurement_void. The design-level deliverable is one of:
(a) a named mechanism plus a determinism recipe — the configuration under
which byte-identity holds, the precondition any future identity instrument
cites; or (b) the honest negative — byte-identity unattainable at reduced
ctx on these engines, in which case identity claims must use N-rep
prefix-match statistics and never single-run byte-identity, and the
streaming-unroutable bar stays closed. Limitations: single machine, one
8 GB card; any finding transports to the RESIDENT BUILD only (the forks'
compute paths may differ); phase-1 uses release builds only.
