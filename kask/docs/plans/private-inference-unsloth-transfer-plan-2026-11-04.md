# Research and implementation plan — Unsloth-derived acceleration for the kask private-inference local leg

## 1. Mission and target condition

**Research question.** Which of Unsloth's architectural techniques for fast local LLM work transfer to the kask private-inference local leg — the placement-controlled llama.cpp wrapper — and which of those, once implemented in Rust or absorbed as data/configuration, produce a measured improvement over the recorded P0 baseline on this machine?

**Premise correction (read before scoping).** Unsloth's headline numbers (2x faster training, 70% less VRAM) come from training-time, in-process, hand-written Triton kernels and a manual autograd — work that lives inside a model's compute graph. The kask wrapper never touches tokens: it is a process-level stack (manifest -> constraint forms -> placement compiler -> supervisor) around llama-server's OpenAI-compatible API. "Rust implementations of Unsloth's techniques" is therefore not a kernel-porting project by default. The transferable surface, verified against the tree and the live plan:

- Quantization recipes and policy — the leg already consumes one Unsloth technique: the UD-Q2_K_XL dynamic GGUF quant (78.9 GB, top-1 82.7%).
- Speculative / MTP decoding — the unsloth qwen4exp/mtp fork datapoint: an M4 Max 36 GB machine ran this exact model+quant (UD-Q2_K_XL + MTP draft) at ~26 tok/s decode via streaming, vs the measured 13.0-15.0 tok/s HIP resident leg here.
- Expert-cache / streaming co-execution (Colibri/FreeToken-class; llama.cpp PR #25294 SSD expert streaming, PR #29887 GPU-resident LRU expert cache, PR #26414 hot-expert pinning — already verified in the live plan's research runs).
- Serving-level configuration — KV cache typing + flash-attn (the D1 guard already pins: quantized V cache requires flash-attn, enforced at emit time), chunked prefill, batching/slots, prompt-cache reuse.
- The architectural lesson itself — Unsloth hand-writes what the framework's abstraction cost exceeds; the kask analog (compute the launch, keep the drift boundary as data) is already embodied in the drift-absorption stack. This plan extends that stack; it does not reopen the committed plan.

**Target condition (first increment).** One technique taken end to end — research -> adjudication -> pre-registered experiment -> idiomatic implementation -> invariant proof/gate -> measured before/after against the P0 record — with operator approval at the streaming/MTP checkpoint if the chosen technique is a P2-class one.

**Counterfactual falsifier (the honest exit).** If the interrogation finds only techniques llama.cpp mainline already ships or training-time-only techniques, the deliverable is the adjudicated negative inventory with evidence — not a forced port. A technique that does not beat the P0 baseline within the pre-declared noise band is recorded as a miss (precedent: the P0 CPU prediction miss was recorded, not smoothed).

## 2. Grounded context (verified against the tree, 2026-11-04)

**The implementation surface — all under kask/, no upstream edits expected.**
- Crate: kask/crates/hkask-private-inference/ (src/local_leg.rs, src/local_leg/, src/acquisition.rs) — the drift-absorption stack: L1 engine-surface manifest kask/registry/private-inference/llama-cpp-mainline.toml (pin: v0.9.0-dev mainline 9e2a04a; intent->flag vocabulary; closed-vocabulary constraint kinds + CI fixtures), L2 constraint forms via in-tree hkask-lisp (CI fixture gates; reserved expr escape hatch), L3 placement compiler (GGUF header parse + hardware probe + policy -> typed LaunchPlan; guards folded in), L4 supervisor.
- The live plan this project extends: kask/docs/plans/private-inference-local-leg-llama-wrapper-plan-2026-11-04.md — P0 done; P1 (v1 wrapper) in flight; P2 (gated streaming) is the operator checkpoint; accepted next slices: model brief (M9) -> backend auto-detect -> multi-vendor VRAM probe.
- The fixed measurement instrument (reused, never rebuilt): kask/scripts/p0-bench-llama.sh — byte-frozen suite (P1 short / P2 agent-turn / P3 long-context; 3 non-stream + 1 stream rep each; temperature 0; one discarded warmup); per-request TTFB/TTFT/total, server-log slices, token counts, RAM (VmRSS/VmHWM) + VRAM sampling.
- The measured P0 baseline (2026-11-04, this machine): decode CPU control 7.4-8.3 tok/s, Vulkan 10.1-11.5, HIP 13.0-15.0 (agent-turn 256 tok: 18.4-21.5 s on HIP); HIP + flash-attn + PLE-pinned-to-CPU is the measured fastest policy; 23 tests green, clippy clean at the handoff.
- Out of scope: upstream crates/llama_cpp (llama.app-managed; noted-and-not-used per the live plan), the M3 openai_compatible seam, ollama, FR-4 privacy marking.
- Machine: Framework 16 — Ryzen AI 9 HX 370 (12C/24T), 96 GB DDR5, RX 7700S-class 8 GB gfx1102 (HIP), ROCm kfd present. Model: Qwen3.8-Flash-Next UD-Q2_K_XL.

**The research surface.**
- Repo https://github.com/unslothai/unsloth (Apache-2.0, Python); docs https://unsloth.ai/docs with the llms.txt full index. Pin the exact commit SHA at research time; every inventory row cites file:line against the pin or a docs URL.
- Landmarks to verify (never assume): Dynamic GGUF quant recipes (UD-*/Dynamic V3.1 — selective per-layer quantization designed for llama.cpp); MTP draft models (qwen4exp/mtp fork); the training-kernel family (fused Triton RMSNorm/RoPE/cross-entropy, manual autograd, gradient checkpointing — expected NOT_APPLICABLE to serving); GGUF/NVFP4/FP8 export; their local serving engine (OpenAI-compatible, LAN/HTTPS serving).
- License: technique ideas transfer cleanly; any direct code translation (e.g., quant-generation recipe logic) carries Apache-2.0 attribution duties — record provenance if that happens.

**Anchored terms** (onto_anchor, 2026-11-04): inference -> cso:inference, quantization -> cso:quantization, cache -> cso:cache + the derived registry definition (a cache is deletable exactly when re-derivation is available and cheaper — this recoverability property constrains expert-streaming admission and cache-quant decisions) — CSO 3.5 pinned OWL, domain-supplement tier. matrix multiplication resolved only coarsely (5w1h_core): operator ruling required before it carries definitional weight; never a private definition. No published zk-ref-open reference model exists for this domain (unanchored_in_zk_ref_open) — grounding is the tree's own committed plan + P0 record, cited as tree artifacts.

## 3. Task graph

T1 — Pin and interrogate Unsloth (deep-module) — P — depends: [] — Pin the unsloth commit; read the docs (llms.txt -> Inference & Deployment, Dynamic GGUF, serving pages) and the repo's inference-relevant modules. Output: the technique inventory — one row per technique: name; mechanism (what it fuses/avoids/tunes); evidence (file:line @ pin, or docs URL); inference-relevant per cso:inference scoping; training-time? Apply the deep-module deletion test to each visited module: what does its interface hide, and what is the wrapper's analog? — Oracle: research-run ledger (begin_research_run; server-recorded sources; verification states annotated) — Termination: every row evidenced, or 25 run-scoped probes, whichever first; no unbounded crawl.

T2 — Transfer adjudication (hypothesis-framer) — P — depends: [T1] — For each row, FINER + PICO framing and exactly one verdict from the closed vocabulary: PORT (wrapper-owned Rust: new placement intents, model-brief fields, cache-policy arithmetic) / CONFIGURE (llama.cpp already ships it — absorb as manifest flags/policy; never duplicate in Rust) / MODEL_BRIEF (per-model data: quant recipe, PLE pin, recommended KV — the M9 slice) / GATE (needs the parity/launch gate first — the streaming class) / NOT_APPLICABLE (training-time, in-process kernel work). One line of evidence per verdict. Rank at most 3 candidates by expected measured gain x placement fit x experiment cost — anchors: the M4 Max 26 tok/s MTP datapoint; the D1 constraint; the streaming-divergence retraction. — Oracle: grill-me Recall+Mechanism pass over the adjudication doc — Termination: every row verdicted; shortlist frozen.

T3 — Experiment design (kata-improvement) — P — depends: [T2] — Record on the kanban task (kanban_task_kata_record): direction (this plan); current condition (P0 record numbers, transcribed); target condition (one measurable per candidate: decode tok/s, agent-turn seconds, TTFT, VRAM/RSS — full agent-turn, not decode alone); the ONE focus experiment: next step, prediction (a band with anchors), measurement method (p0-bench-llama.sh; same suite/machine/model; temperature 0), noise band declared from the P0 rep spreads BEFORE the run, when to check. If the candidate is P2-class (streaming/MTP): STOP at the operator checkpoint (live plan step 7) — present the experiment shape, do not launch. — Oracle: lisp_eval over the recorded baseline numbers (band arithmetic, rep spreads, percent deltas) — Termination: one pre-registered experiment; prediction + falsifier written before any run.

T4 — Implement (idiomatic-rust; tdd at the seam under test) — D — depends: [T3] — One technique, one increment. Idiomatic Rust: type-driven, no unwrap, ? propagation, typed fail-closed errors naming the missing thing, deep modules (deletion test per module), defaults in Default impls only. Data-first for CONFIGURE/MODEL_BRIEF verdicts: registry TOML + one decode home; zero policy in templates. Guards and fixture gates updated in the same change. — Oracle: cargo test -p hkask-private-inference (every manifest's constraints pass its fixtures) + ./script/clippy over touched kask crates; tdd red->green at the seam — Termination: green + clippy clean at one pathspec commit in the shared tree, or a typed stop naming the blocker.

T5 — Prove and gate the invariants (lean-prover + lisp_eval) — D — depends: [T4] — Prove in Lean 4, over stated assumptions, only wrapper-owned invariants: guard arithmetic (byte sums bounded; budget never overflowed), partition coverage (a streaming expert-wave partition covers every expert exactly once, no overlap), slot-floor arithmetic. State assumptions explicitly; a proof never establishes that the formalization captures intent — T6's measurement and operator review carry that. Non-formalizable invariants get the lisp_eval fixture gate as their deterministic oracle. — Oracle: lean_check receipts; lisp_eval gates in CI — Termination: every claimed invariant proven or labeled not-formalizable with the reason.

T6 — Adversarial review and honest measurement (grill-me) — P — depends: [T4, T5] — Grill the change at Recall -> Mechanism -> Rationale -> Edge Cases -> Synthesis: does the implementation preserve the technique's mechanism, or only its name? Edge cases on record: OOM mid-session, flag drift, iGPU-vs-dGPU, warm-vs-cold cache, the streaming-divergence retraction class. Then run the pre-registered experiment and adjudicate honestly: within the noise band = tie (say so); below baseline = miss (record, revert or revise); above = evidence-backed gain. — Oracle: the experiment record vs the T3 pre-registration (no post-hoc band moves) — Termination: one grill record + one adjudicated measurement, both written down.

T7 — Deliver and escalate — D — depends: [T6] — Deliverables: the adjudication report (inventory + verdicts + shortlist), the measurement record, the code (net lines; what the change replaces, deleted in the same change), the live plan doc's phase/next-slices update, .rules additions proposed in the report if new traps were found (never edited inline). Operator escalation: P2-class candidates need the step-7 checkpoint; goal lifecycle — goal intake at project start with the operator confirming criteria; worker judgment via kanban_goal_judge against frozen criteria; closure is the human's. Commits cited by hash, never by intention. — Oracle: the report's own checkability (every claim carries its oracle's receipt) — Termination: report delivered; uncommitted work named as uncommitted.

## 4. Skill stations (the operator's named skills, at their posts)

- deep-module -> T1 (interrogate unsloth's module design; deletion test; the analog question).
- hypothesis-framer -> T2 (FINER gate; PICO/counterfactual; the closed verdict vocabulary is the frame).
- kata-improvement -> T3 and throughout (direction/current/target/one-experiment PDCA on the kanban task).
- idiomatic-rust -> T4 (Graydon-Hoare-lens design; rust-analyzer + ./script/clippy as extrinsic oracles).
- lisp_eval -> T3 band arithmetic, T5 fixture gates, T7 report checks — the deterministic oracle.
- lean-prover -> T5 (Lean 4 proofs over stated assumptions).
- grill-me -> T2 (adjudication pass) and T6 (full ladder), decoupled from authorship.
- Platform gates always on: cargo test, ./script/clippy, manifest fixture gates, pathspec commits, DIVERGENCE.md unchanged (target: no zed-side edits).

**The lisp forms target, honestly.** hkask-lisp is the L2 launch/CI gate layer — not in the decode hot loop. Measure once in T3 whether lisp evaluation latency is ever load-bearing at launch (expected absence: CI/startup-scale evaluation). Record the result as evidence either way; do not invent a bottleneck. The Unsloth fusion lesson applies to lisp only if a measured per-request hot path exists; otherwise the honest finding is "lisp is the oracle, not the hot path."

## 5. Honesty and measurement discipline

- Claim labels: IS only with evidence; OUGHT for design prohibitions; hypotheses carry predictions and falsifiers; estimates stay subjunctive until measured.
- One fixed instrument: kask/scripts/p0-bench-llama.sh, same model/machine/suite, temperature 0, one discarded warmup; noise band declared before the run; a miss is recorded as a miss.
- Expected absence is verification evidence, never an incident; missing credentials/flags are typed refusals, never silent fallback (FR-3).
- No benchmark gaming: no re-tuning the suite to the technique; no post-hoc band moves; fixed rep counts.
- Provenance: unsloth sources through run-scoped research tools with verification states; tree claims by file:line against the current tree; completion claims by commit hash.

## 6. Acceptance criteria (falsifiable; check kind in brackets)

1. Every inventory row for a visited inference-relevant unsloth module cites file:line @ the pinned commit or a docs URL. [empirical: research-run ledger]
2. Every row carries exactly one verdict from {PORT, CONFIGURE, MODEL_BRIEF, GATE, NOT_APPLICABLE} with one line of evidence; nothing llama.cpp mainline already ships is implemented in duplicate Rust. [judgment: grill-me pass, spot-checkable against the ledger]
3. Exactly one technique passes through T3-T6, with prediction + falsifier + noise band written before its run. [finite: lisp_eval over the pre-registration vs the record]
4. Every performance claim is a p0-bench-llama.sh measurement vs the recorded P0 numbers, adjudicated inside the pre-registered band; ties and misses named as such. [finite + empirical]
5. cargo test -p hkask-private-inference green and ./script/clippy clean over touched kask crates; guards + fixture gates updated in the same change; net lines and replaced paths reported. [empirical]
6. Every claimed wrapper-owned invariant is either proven in Lean 4 over stated assumptions (lean_check receipt) or labeled not-formalizable with the reason. [formal]
7. The final report carries commit hashes, the plan-doc update, the P2/operator-checkpoint disposition, and any .rules proposals separately. [empirical]

## 7. Deliverables and operator escalation

- A dated adjudication + measurement plan document under kask/docs/plans/ (extending the live plan doc's phases and next-slices sections, per the existing plan-doc conventions).
- Code changes under kask/ only: registry TOML, hkask-private-inference Rust, bench-harness edits only if a method defect is found (patch documented in the report).
- The report to the operator with the goal lifecycle observed (intake with confirmed criteria; judgment against the frozen criteria; human closure).
- Standing checkpoints inherited from the live plan: step-7 streaming approval (any GATE verdict), drift-watch ownership; new checkpoints surfaced in T7, never decided unilaterally.
