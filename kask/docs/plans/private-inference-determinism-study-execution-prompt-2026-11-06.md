# Determinism-study execution prompt — the engine-compute study, its purpose stated as the invariant represent/execute question

**Handoff target:** any agent with repo access and no conversation history.
**Composed:** 2026-11-06 (evening), on the operator's direction — this prompt's
composition IS the spend authorization for THIS study, within the design's
declared bound (≤ ~70 engine requests, one session, no new hardware). No
other spend rides this authorization.

**The operator's goal, in their words:** "identify how we can accurately and
consistently REPRESENT/REFERENCE invariants in prompts and also accurately
and consistently EXECUTE invariants in the processes." The engine-compute
determinism study is the first concrete measurement of that gap at the
inference substrate: an invariant REPRESENTED in configuration (temperature
0, the frozen fresh-slot protocol, `--no-cache-prompt`) is a claim in a
probabilistic medium; the invariant EXECUTED by the engine's compute is the
process; the two measurement records proved the gap is real (the represented
determinism invariant FAILED in execution). The study characterizes the gap
so invariant claims can be made honestly and enforced in the right layer.

**Read first:** the worktree-root `.rules` (required);
`kask/docs/research/private-inference-engine-compute-determinism-design-2026-11-06.md`
(THE pre-registered design — its hypotheses, gate, arms, analysis rules and
stopping are frozen; this prompt adds execution steps and the goal layer,
never a new analysis);
`kask/docs/research/private-inference-p2-streaming-parity-measurement-2026-11-05.md`
(§2 P2-B — the refined confound) and
`kask/docs/research/private-inference-unsloth-mtp-measurement-2026-11-05.md`
(§3 D-G — the original observation).

## The study in one paragraph

Why do 256-token temperature-0 completions flip knife-edge tokens between
reps at reduced context (ctx 16384/32768) while 1024-token completions on
the same engine can be byte-stable, and within-leg determinism holds at
ctx 65536? Both prior records were instrument-confounded on it (MTP D-G;
P2 P2-B — the fresh-slot protocol did NOT restore determinism), so no
cross-engine identity claim has been made and none can be trusted until
this question is answered. The four rival hypotheses (H1 compute-path
layout coupling; H2 completion-cap/scheduler coupling; H3 the observation
confound; H4 residual pool/workspace state) each predict a distinct
observation — the design's §2 discrimination matrix adjudicates them.

## The work, in order

0. **Pre-flight (no engine requests).** Verify the resident engine's
   exact-commit pin (the manifest's `VersionPin`; the probe refuses any
   other binary — `cargo test -p hkask-private-inference` gates the
   fixtures; the D-S2 discipline). Prepare the artifacts dir
   (`~/Documents/zk-data/private-inference/measurements/determinism-2026-11-06/`).
   Re-read the P2 parity harness's comparison discipline (the hardened
   forms, `ac40450c3f`: the combined content+reasoning surface, the
   zero-class guard, the empty-side refusal).
1. **Launch the resident mainline HIP engine at the canonical root** — the
   P0 bench discipline (by hand, the placement compiler's rendered
   LaunchPlan; the P1 plan §12 records the live-launch procedure, and the
   drift-watch runbook's step-3 gates apply: liveness, endpoint, health,
   served-model identity — never the file alone). Record endpoint, PID,
   and the server log path.
2. **Phase 0 — the instrument gate (≤10 requests).** P1 class + the long
   class, ctx 16384, 5 reps, temperature 0, the full fresh-slot protocol.
   **Acceptance: the P2-B asymmetry reproduces** (short-class flips;
   long-class byte-stable). Else HALT — the premise itself is unstable;
   record a measurement_void and stop for the operator. (The gate is the
   represent/execute discipline in miniature: the premise claim gets an
   executable check before any confirmatory spend.)
3. **Phase 1 — the arms, in order, fixed-N, no interim peeking** (the
   design's §5): O1 the cap sweep {128, 256, 512, 1024} × 5 reps at fixed
   prompt+ctx (H2); O2 the prompt-padding alignment sweep × 3 reps (H1);
   O3 the ctx sweep {16384, 32768, 65536} with 2 relaunches (H1's ctx
   coupling, replicating D-G's boundary under the hardened instrument);
   O4 the rep-order analysis within O1's legs + one restart arm (H4).
4. **The frozen analysis, exactly as pre-registered** (the design's §5
   decision rules per hypothesis; a falsified hypothesis is recorded
   falsified, never bent).
5. **The record.** A measurement doc under
   `kask/docs/research/private-inference-engine-compute-determinism-measurement-2026-11-06.md`
   — the deviations with the data, per-hypothesis outcomes in the typed
   vocabulary (corroborated / rejected / inconclusive / measurement_void),
   lisp receipts for every arithmetic claim, and the raw artifacts saved.
6. **The deliverable layer (the operator's goal).** One of:
   (a) **The determinism RECIPE** — the configuration under which
   byte-identity holds: the EXECUTION invariant any future identity
   instrument cites as its premise (and the precondition for any streaming
   reopen the operator might ever consider); or
   (b) **The honest negative** — byte-identity unattainable at reduced ctx
   on these engines: the REPRESENTATION discipline changes instead —
   identity claims become N-rep prefix-match statistics, never single-run
   byte-identity, and the streaming-unroutable bar stays closed.
   AND either way, the represent/execute statement: which invariants the
   substrate honors at which configuration, and the executable-check
   pattern (the instrument gate) that pins the claim — an invariant stated
   in a prompt without an executable check is a hypothesis, not an
   invariant.

## Boundaries

The spend bound: ≤ ~70 engine requests, one session, no new hardware, no
spend beyond this study. Release builds only — no instrumented engine
builds (the observer effect; the D-S2 pin). The forks (MTP `6fcaa16`,
ServeurpersoCom `7174388`) enter only if the operator extends scope. The
streaming-unroutable bar stays unless a recipe exists AND the operator
re-opens it. No verification claim carries into the next commit without
its oracle re-run.

## Acceptance criteria (each can fail)

1. The gate ran and either reproduced the asymmetry or halted honestly.
2. Every hypothesis adjudicated against its pre-declared prediction, with
   the falsifiers named.
3. The record committed with the deviations, the typed outcomes, lisp
   receipts, and the artifact paths.
4. The recipe OR the honest negative stated — one of the two, explicitly.
5. The represent/execute statement present (the operator's goal, answered).
6. The report cites commit hashes, the gates run (with commands), and any
   uncommitted state plainly.

## Known traps

Beyond the worktree-root `.rules`: the fresh-slot protocol does NOT
restore within-leg determinism at reduced ctx — measure the gate, never
assume D-G's fix transfers (P2-B). The thinking model emits
`reasoning_content` first — compare the COMBINED surface and refuse
empty-side comparisons. Read TSVs with field-aware probes (`awk -F'\t'`,
NF) — never a tab-collapsed display (the D-E lesson, frozen in the MTP
record's §8 amendment). `cmp` under `set -e` needs `|| true`. The shared
tree is busy (pathspec commits, numstat verification, disclose co-streamed
content). The engine pin is exact-commit — a wrong binary refuses at the
probe (D-S2), which is the represent/execute discipline working as
designed.
