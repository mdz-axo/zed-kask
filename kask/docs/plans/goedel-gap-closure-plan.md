---
title: "Gödel-Machine Gap Closure — Bounded, Evidence-Backed Improvement"
audience: [architects, operators, agents]
last_updated: 2026-09-18
version: "1.0.0"
status: "Active"
domain: "Self-improvement"
mds_categories: [trust, lifecycle, composition]
---

# Gödel-machine gap closure: revised implementation plan

## 1. Recommendation and scope

**Finish and demonstrate the existing process before building a larger improvement subsystem.** Then enforce one bounded improvement lifecycle and measure whether formal checking adds value. This is Gödel-inspired engineering, not an implementation of the original global-optimality theorem.[^goedel][^dgm]

Delivery order: **reconcile records → repair measurements → execute existing proofs and demonstrate one real cycle → enforce traceability → build one bounded promotion boundary → measure formal-checking value**. R2–R4 may proceed independently after R0; R5 needs a validated evaluator, not completion of unrelated infrastructure. R6 requires the core-repair authority and persistence prerequisites.

The shared goal is reliable, inspectable improvement under operator authority. The working mode is one-domain-at-a-time repair/refactoring followed by bounded experiments—not autonomous production self-modification. This revision records the requested plan, not blanket permission to install tools, spend credits, change governance, commit, or deploy.

For each **supported** improvement attempt retain a predeclared objective, baseline, candidate, evaluation protocol, evidence, resource use, and decision authority. Bind acceptance evidence to the exact artifact. Initially support one skill/prompt process cycle, the existing LoRA proof pilot, and subsequently one bounded runtime-policy class—not every arbitrary repository edit.

| Term | Meaning |
| --- | --- |
| Process evidence | Evidence that the specified workflow was followed. |
| Empirical evidence | Measurements under a named protocol, distribution, and budget. |
| Formal evidence | A checker result for stated properties, semantics, assumptions, and bounds. |
| Acceptance | Eligibility under a fixed contract; not permission to deploy. |
| Approval | Host-observed authorization by the designated external authority. |
| Activation | Switching the supported active artifact/state after acceptance and approval. |
| Calibration | Scoring a prediction against an outcome; not measuring utility gain. |

## 2. Baseline, provenance, and retained work

This section indexes inspected implementation and historical reports; it is not a new architecture claim.

**Inspection baseline:** `ba0485f7a98011dd46da21a5da8a54383137b794`, 2026-09-18. Worktree and index were clean at the start of this documentation revision; unrelated agent-source edits appeared during the pass. Recheck before implementation: concurrent streams previously changed both source and commits during research.

| Artifact or claim | Disposition |
| --- | --- |
| Initial research baseline | `a7c54681f6936c799c2ddb95a297df9633c233ad`; subsequent findings must name their actual revision. |
| S2/S3 skill gates, S4 harnesses, training README | Committed in `6054422cc3` under the broader message “Add InferenceUsage reported flag, sweep dead knobs.” Preserve; do not recreate. |
| Registry, fail-loud parse fix, original plan, docs index | Committed in `30548eece0`, alongside unrelated changes. The old W1 “commit remainder” handoff is superseded. |
| Later review follow-up | `ba0485f7a9`; recheck affected MCP boundaries rather than assuming the research snapshot remains current. |
| Program-manager closure-ledger form | Correct in `2988f090f8`; recursive call retains `token`. Earlier arity failures were caller transcription errors, not a skill defect. |
| Kani availability/results | `cargo-kani` not found on inspected PATH. Four harnesses exist; this revision claims no proof execution. |
| Previous validation | Another agent reported 29/29 training tests, 77 skills with zero sweep flags, and registry-check success plus one gap. Historical reports, not independently rerun results here. |
| Documentation count | 68 files under `/home/mdz-axolotl/Clones/zed-kask/kask/docs/`; strict `<70` permits only one additional file. Update this plan rather than creating a parallel plan. |

Durable context, **load if tools are available**: report kind `report`, name `goedel-gap-closure-plan-2026-09-18`; historical research-run identifier `7bbdccd59bb5146a`. Their current contents were not accessed in this revision. Prior plan goal: `5e12d79e-5f9f-4314-8f94-5e5f165e58e4`; prior research goal prefix: `6f0decbf`. Resolve the latter to its full ID and inspect both goals before judging/scoring. Operator confirmation must be real, never inferred from a request to continue.

### Baseline evidence register

These findings describe the §2 inspection baseline, not all later working-tree changes. The R1 entry in §9 supersedes E1; the R2–R4 entry supersedes E2–E4/E6 for the inspected uncommitted implementation.

| ID | Absolute source and supported claim |
| --- | --- |
| E1 | `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-training/src/tools/evaluate.rs:118–175,300–310`: semantic substring verdict parsing and denominators omitting ordinary wrong answers. |
| E2 | `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-training/src/lora_validation/param_gates.rs:464–568`: four cfg-gated harnesses. `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-training/src/providers/types.rs:240–269`: `LoraInit`, with no visible Kani `Arbitrary` derive. |
| E3 | `/home/mdz-axolotl/Clones/zed-kask/kask/scripts/check-principle-constraints.sh:22–41,65–99`: fail-loud parsing, but missing/empty inventory may pass; `MISSING:` falsifiers skip test-name searching. Inventory checking is not semantic enforcement. |
| E4 | `/home/mdz-axolotl/Clones/zed-kask/kask/docs/architecture/principle-constraints.yaml:67–105`: some process-only or missing-falsifier claims are labeled enforced. Reconcile under R3; this plan does not change their runtime status. |
| E5 | `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/service_impl/tests.rs:234–275`: validates supplied citations, not mandatory coverage of all self-changes. `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/service_impl/goals.rs:242–280`: ownership and Brier scoring, not host-observed human confirmation. |
| E6 | `/home/mdz-axolotl/Clones/zed-kask/kask/githooks/commit-msg:17–60`: local message-hygiene hook. No goal-link or Kani job found in `/home/mdz-axolotl/Clones/zed-kask/.github/workflows/kask-invariants.yml` at this baseline. |
| E7 | `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:71–103`: evaluator can invoke `sh -c spec`; this function does not establish independent authority or sandboxing. |
| E8 | `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-regulation/src/strategy_evaluator.rs:104–172`: acceptance-ratio history and rotation between named strategies, not proof of policy improvement. |
| E9 | `/home/mdz-axolotl/Clones/zed-kask/kask/crates/kask_bridge/src/inference_resilience.rs:16–100,288–369`: circuit admission and existing tests; candidate for later pure-policy extraction. |
| E10 | `/home/mdz-axolotl/Clones/zed-kask/kask/docs/plans/hkask-core-mcp-repair-improvement-plan.md:347–374`: invocation/approval and persistence repair packages. Dependency planning, not evidence the repairs are complete. |
| E11 | `/home/mdz-axolotl/Clones/zed-kask/kask/docs/architecture/zed-host-architecture-plan.md:251–269`: dependency direction and D8 host-adapter boundary. |

## 3. Reference model and boundaries

The original manuscript/arXiv submission date to **25 September 2003**. The AGI chapter is publisher-dated **2007**, although arXiv's bibliographic note says 2006. Cite versions distinctly.[^goedel][^chapter]

A strict Gödel machine formally represents relevant machine/environment behavior and utility, searches for proofs, and switches only after proving that a rewrite is preferable to **continuing the current solver and searcher**, including future rewrite opportunities and costs. Conditional optimality is not test success, invariant preservation, benchmark gain, or guaranteed realized success. The 2011 work refines formal implementation details; do not attribute later empirical replacement of proof search to it.[^steunebrink]

**A-R1 retained:** arbitrary Gödel-machine proof search is out of scope. Correcting its old rationale does not reopen the decision. Reasons are unresolved open-world utility formalization, correspondence, reflection, and affordable search—not GitHub popularity or an alleged impossibility theorem.

The research used separate routes: **A, open-source Rust regardless of stars; B, open-source any-language repositories with strictly more than 100 stars**. Rust bounded/heuristic candidates were found; no strict implementation was established in inspected code. Do not repeat the combined “Rust ≥100 stars, clean null” claim. Coverage was bounded, not exhaustive.

| Reference pattern | Adopt | Do not infer |
| --- | --- | --- |
| DGM: empirical archive evolution[^dgm] | Candidate lineage, unsuccessful attempts, independent comparison. | Benchmark gains prove beneficial rewriting. |
| HGM: descendant metaproductivity[^hgm] | Record generator identity and lineage. | Estimated descendant value realizes an oracle; defer optimizing it. |
| NoemaSI/what-is-rsi; PascualMacana/prover[^rust-loops] | Finite proposal/check/measure loop; recompute claims against exact parent/child. | Tests or finite-score certificates prove the original theorem. |
| Symbiont; AROS-SIE; Areev[^rust-boundaries] | Revision-aware evaluation, partial-run costs, shadow comparison, host authority. | Native hot-rewriting or a new identity framework is required. |
| Statistical Gödel Machines[^sgm] | Development/confirmation separation and repeated-testing risk allocation. | Statistical validity under arbitrary adaptive reuse or semantic safety. |
| Kani[^kani] | Narrow executable obligations with stated bounds/assumptions. | Validation correctness proves training quality or utility improvement. |

No Python reference framework becomes a dependency. HyperAgents was source-inspected but excluded from the open-source route because its license is noncommercial; public source is not unrestricted reuse.

**Authority boundary:** no automatic promotion. Green acceptance evidence is not approval. Objective, evaluator, checker, confirmation-data, and approval-policy changes require separate operator ratification. This safeguard is not a solution to reflective trust in the original formal system. The constraint registry is a governance/enforcement inventory, **not a formal axiom system**.

## 4. Architecture and refactoring decisions

Use incremental replacement only for demonstrated shared responsibilities: pin behavior, repair it, route one complete path through the deeper implementation, and delete its superseded logic.[^strangler] DGM motivates evidence retention, not a general self-modification framework.[^dgm]

| Priority / responsibility | Classification | Decision and test surface |
| --- | --- | --- |
| 1. Standard/benchmark counts | Identical accounting | One internal accumulator; public tool-path tests. Repair before extraction. |
| Method/swarm scoring | Divergent | Separate semantics; share only justified evidence envelopes. |
| Existing `run_evaluator` callers | Already shared | Deepen execution boundary, not another forwarding service. |
| 2. Authority/uncertain outcomes | Shared boundary | Reuse E10; spoofing, revocation, partial-effects, recovery tests. |
| 3. Artifact/evidence acceptance | Missing shared domain | Centralize after R5 demonstrates requirements; reject stale evidence. |
| Regulation impact vs promotion | Divergent | Observation is not acceptance or authorization. |
| Skills and JSON/error formatting | Surface-only | Guidance in skills; I/O framing in adapters. |
| Event-store forwarding wrapper | Pass-through | Reject: no meaningful complexity hidden. |
| 4. Circuit transitions | Existing host-coupled behavior | Later extract production logic into a bounded pure domain. |

### Proposed placement — not existing code

| Proposed absolute path | Responsibility |
| --- | --- |
| `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-regulation/src/improvement.rs` | Acceptance contract, comparability, completeness, stale-baseline checks. Exists only if deletion spreads these rules across callers. No OS execution/credentials. |
| `/home/mdz-axolotl/Clones/zed-kask/kask/crates/kask_bridge/src/improvement_host.rs` | Host approval, restricted checking/evaluation, activation. Reuse existing authority. |
| `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-storage/src/improvement.rs` | Durable activation/attempt state only if required; safe transactions and crash tests precede atomicity claims. |
| `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-regulation/src/inference_policy.rs` | Later pure circuit core; bridge retains locks, clocks, receipts, provider effects. |

Keep hKask Zed-free; MCP servers do not depend on `kask_bridge` (E11). Favor additive Rust under `/home/mdz-axolotl/Clones/zed-kask/kask/`; read `/home/mdz-axolotl/Clones/zed-kask/DIVERGENCE.md` before upstream edits. No new MCP server, dashboard, catch-all service crate, or native hot-loading system.

Confirm each manifest before using dependencies. `serde_yaml_neo` already appears in `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-regulation/Cargo.toml:11–29`. A Rust replacement for the pre-existing Python registry checker is bounded tooling work, not a new interpreter dependency. Kani is an approved, pinned development toolchain, not a crates.io runtime dependency.

## 5. Implementation packages R0–R7

Packages separate empirical improvement from formally checked properties.[^sgm][^kani] These are work instructions, not completed implementation. One domain per review/commit; confirm exact callers before editing.

### R0 — Reconcile records and scope

**Owner:** implementing engineer; operator owns outcome confirmation. **Entry:** current status/history/rules read.

- Preserve S2/S3/S4 and fail-loud parsing. Do not replay old W1 or edit the correct program-manager form.
- Reconcile saved report and prior goals when accessible; otherwise record unavailable state and owner without inventing IDs or outcomes.
- Register a new implementation goal before executing, or obtain an explicit exception if goal tooling is unavailable. Never backdate registration.
- Keep records in this plan; registry terminology/status changes belong to R3 and require applicable ratification.

**Files:** this plan and `/home/mdz-axolotl/Clones/zed-kask/kask/docs/README.md`, plus durable report/goal records if tools exist. **Exit:** revision, goals, approvals, open items accurately recorded. **Estimate:** 0.5–1 engineer-day.

### R1 — Repair evaluation correctness

**Owner:** evaluation engineer. **Entry:** E1 rechecked and baseline fixtures identified.

**Files:** `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-training/src/tools/evaluate.rs`, `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-training/src/types.rs`, `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-training/src/hkask_mcp_training.rs`.

- Count every attempted example; distinguish correct, incorrect, generation error, evaluator error, skipped invalid input.
- Parse `CORRECT`/`INCORRECT` exactly; malformed judge output is not a verdict. Reject unsupported methods rather than silent fallback.
- Make judge selection/identity explicit; a different model alone is not independence.
- Include judge work and unknown usage honestly. Share accounting, not unrelated scoring semantics; pin benchmark parsing separately.

**Tests:** `INCORRECT` rejected; one correct plus nine wrong gives 0.1 on both paths; all-wrong is not zero examples; judge/malformed failures and missing usage surface. Exercise public `Parameters<TrainEvaluateRequest>` behavior, not only helpers.

**Exit:** fixtures demonstrate honest totals, outcomes, costs. **Estimate:** 2–5 days. No adjacent training refactor.

### R2 — Complete the existing Kani pilot

**Owner:** formal-methods engineer. **Entry:** approved toolchain environment/version/budget and reproducible source snapshot. Independent of R1.

**Files:** E2, `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-training/README.md`; specifications in `/home/mdz-axolotl/Clones/zed-kask/kask/docs/reference/lora-training-catalog.md`. Optional proposed launcher: `/home/mdz-axolotl/Clones/zed-kask/kask/scripts/check-bounded-proofs.sh`.

1. Provision only with explicit approval; pin/record verifier and Rust toolchain. Never add the crates.io `kani` placeholder.
2. Check installed CLI help; execute separately: `gm3_refuse_iff_degenerate_scaling`, `gm4_findings_follow_rank_thresholds`, `gm1_clean_iff_noop_init`, `safe_region_has_no_refusals`. Initial timebox: 15 minutes each.
3. Check symbolic construction of `Option<LoraInit>`; use a complete bounded generator or justified cfg-gated derive. Attribute spelling is not the only possible failure.
4. Separate pass, counterexample, compilation failure, unsupported analysis, and resource exhaustion. Timeout is unknown, not pass/disproof.
5. Diagnose against the catalog; fix the wrong code/specification, never weaken assumptions just to pass. Check satisfiable assumptions, unwind sufficiency, and finding severity/content rather than only counts.
6. Retain complete raw output/exit status as artifact links/hashes and a concise summary: revision, dirty diff identity, command, environment, checker version, assumptions/bounds, elapsed resources. Do not flood this plan with logs or secrets.

**Exit:** four auditable outcomes; only checked properties marked verified. One pass is not continuing CI enforcement. These proofs concern validation predicates, not threshold optimality, provider/PEFT behavior, training success, or beneficial rewriting. **Estimate:** 2–5 days; compatibility/proof cost uncertain.

### R3 — Make enforcement inventory truthful

**Owner:** governance/tooling engineer; operator ratifies trust-basis changes. **Entry:** E3–E6 reconciled.

**Files:** `/home/mdz-axolotl/Clones/zed-kask/kask/docs/architecture/principle-constraints.yaml`, `/home/mdz-axolotl/Clones/zed-kask/kask/scripts/check-principle-constraints.sh`; choose a minimal Rust checker target after inspecting existing tooling.

- Replace “axiom layer” with governance/enforcement inventory; no duplicate theory framework.
- Distinguish documented process, runtime/CI enforcement, formal verification at a revision, and gaps; record scope, owner, executable check, evidence reference.
- Do not mechanically flip `ax-training-math-proofs` to broadly `enforced` after R2. First record verification; ongoing enforcement needs relevant reruns at a blocking boundary.
- `MISSING:` means explicit unresolved/process-only status, not “OK”. Reject malformed entries, unknown statuses, unsupported enforcement claims.
- Required gate must not pass when its expected registry is missing. Test empty-inventory semantics.
- Do not extend Python. Migrate to Rust using confirmed libraries before making the checker required; this need not block R2 or a separately validated R5.

**Exit:** inventory success cannot be mistaken for executed tests/proofs or broken parsing for an empty success. **Estimate:** 2–4 days.

### R4 — Goal traceability, advisory first

**Owner:** tooling engineer; operator decides blocking policy. **Entry:** scope/privacy reviewed; current hook edits coordinated.

**Files:** `/home/mdz-axolotl/Clones/zed-kask/kask/githooks/commit-msg`, `/home/mdz-axolotl/Clones/zed-kask/kask/scripts/check-commit-msg-selftest.sh`, `/home/mdz-axolotl/Clones/zed-kask/.github/workflows/kask-invariants.yml`; minimal shared checker only if justified.

- Preserve message-hygiene checks. Initially scope to material skill changes, not all self-modification.
- Define commit trailer/PR metadata/portable-record semantics; handle squash, rename/delete, multi-commit PRs.
- Local hooks are bypassable: CI checks independently. Required merge protection is an operator setting, not guaranteed by workflow YAML.
- CI cannot assume local kanban access. Either check citation syntax only and say so, or validate privacy-reviewed portable evidence. A UUID does not prove existence or prior registration.
- Gate changes require external review; exceptions must be visible. Do not require a positive benchmark delta from every repair or documentation change.

**Tests:** absent/malformed citations, unaffected paths, renames/deletions, multiple files, hook bypass detected by CI, explicit exceptions. **Exit:** precisely named traceability requirement demonstrated; advisory/blocking state recorded. **Estimate:** 2–4 days.

### R5 — One real acceptance-or-rollback cycle

**Owner:** implementation agent and operator. **Entry:** genuine bounded candidate, deterministic evaluator validated on pass/fail/error fixtures, explicit compute/effect budget. If using training evaluation, R1 must pass; unrelated repairs need not block.

Use actual available tool schemas; the sequence is semantic, not permission to invent tools:

1. New goal with criteria/prediction → task linked through `advances`.
2. Freeze baseline/candidate identities, evaluator/protocol, development/held-out inputs, cost cap before evaluation.
3. One small justified skill/prompt candidate in isolation; no write access to evaluator/sealed inputs.
4. Run baseline/candidate through a real supported harness, e.g. `swarm_eval_agent_local` when suitable. Check limits/repeat handling. A substring matcher must genuinely discriminate task success, not reward answer gaming.
5. Deterministic acceptance calculation; `lisp_eval` can calculate but does not authenticate evidence or establish utility independently.
6. Durable verdict with full goal/task IDs, digests, harness/evaluator IDs, retrievable outcomes, costs, rationale. Memory may link evidence, not be its only retained copy.
7. Judge → actual operator ground truth → score original prediction. Never retroactively change prediction or equate goal ownership with human confirmation.

Negative controls: missing evidence, wrong artifact, controlled regression, evaluator failure, pending approval. If current code cannot reject a bypass, record an R6 gap; manual compliance is not runtime enforcement. Do not damage production to test rollback.

**Exit:** genuine traversal and negative-control results, with confirmation or explicitly pending owner. Rejection proves workflow operation, not candidate improvement. **Estimate:** 1–3 days after prerequisites; paid inference needs approval.

### R6 — One bounded promotion lifecycle

**Owner:** runtime/storage engineers and operator. **Entry:** R5 requirements learned; E10 authority/persistence prerequisites verified in code/tests, not presumed complete.

Use proposed §4 modules only where deletion would spread complexity. Reuse existing invocation/approval facts and connection-owning storage; candidate search stays outside trusted acceptance/activation.

- Retain objective version, candidate/baseline digest, parent/generator, evaluator/data/protocol identities, confirmation exposure, outcomes/costs, verification assumptions/bounds, rejection/unknown reason.
- Separate eligibility, approval, activation, recovery. Proposed lifecycle: proposed → evaluated → eligible/rejected/unknown → externally approved → activated → retained/reverted/reconciliation-required. Reject bypass transitions.
- Candidate cannot manufacture authority or widen permissions. Restricted worker: no ambient credentials/network, read-only trusted inputs. Refuse arbitrary shell evaluators on initial promotion path (E7).
- Host-owned hard wall-time, CPU, memory, call/cost ceilings; failed/partial work charged. Stopping a wait is not proof execution stopped.
- Reject stale artifacts/specifications/approvals; serialize competing promotions; activate checked bytes rather than mutable paths.
- Persist reconciliation state before switching; retain prior supported policy/state. Test every crash boundary and failed restoration. Rollback cannot reverse remote effects or necessarily remove contaminated memory.
- Preserve load-bearing evidence across memory decay/event compaction. Digests establish identity, not authorship.

**Tests:** spoofed/revoked/wrong-scope authority; stale candidate/evaluator/baseline; duplicate evidence; budget exhaustion/cancellation; competing promotions; failed writes/commits; crash/restart and incomplete rollback. **Exit:** one supported path rejects bypass and recovers truthfully. **Estimate:** 2–5 weeks, strongly dependent on core repairs.

### R7 — Measure bounded proof-guided improvement

**Owner:** research/runtime engineer; operator approves experiment/canary. **Entry:** reliable evidence and checking; shadow experiment needs no live deployment.

Extract E9's existing circuit transitions into a Zed-free pure domain; preserve semantics first. Bridge retains locks, clock conversion, receipts, effects. Verify the production function, not a disconnected model. Existing safety controls remain active until separately approved replacement.

Compare fixed incumbent, empirical-only search, and the **same candidate stream** with formal checks plus identical empirical criteria. Finite policy/configuration space, no arbitrary native code. Retain lineage/rejections; report seeded unsafe mutants separately from naturally generated candidates.

**Proposed budget:** 32 candidates per optimizer arm; 100 development and 200 sealed traces including held-out fault families; two hours per arm, four CPU cores, 8 GiB RAM; 60-second per-candidate checking ceiling. No GPU, paid inference, live providers, automatic activation. Checking counts against equal total budgets. Pilot sample size is not a power guarantee.

Formal obligations: one half-open probe, rejection before cooldown, defined transitions/time boundaries, arithmetic safety, artifact/evidence binding. Empirical objective: operator-approved useful completions minus failed attempts, recovery delay, search cost; safety violations disqualify. Pre-register a meaningful margin (e.g. 5% operational-cost reduction) and sensitivity analysis. Preserve a sealed audit beyond adaptive development; budget global error if confirming multiple candidates.[^sgm]

**Exit:** unsafe acceptance, false rejection, held-out outcomes, overhead, unknown rates, reproducibility, investigation cost measured. Stop/narrow if unsafe mutants pass, model diverges, sealed data leaks, stale evidence remains valid, budget is exceeded, no useful candidate emerges, or checking adds cost without value. Negative results legitimately close research.

Only positive evidence justifies a human-approved local-policy canary through R6. Multiple optimizers need separate analysis of shared budgets, objectives, baseline races, and interacting assumptions. **Estimate:** 1–3 weeks after policy extraction; no guaranteed improvement.

## 6. Verification and completion contract

Use one-domain migration and checker-specific evidence, not blanket “all green” claims.[^strangler][^kani] Commands are future authorized checks, not results of this documentation revision.

From `/home/mdz-axolotl/Clones/zed-kask`, select affected checks first:

```bash
cargo test --locked -p hkask-mcp-training -- --test-threads=1
cargo test --locked -p hkask-mcp-swarm -- --test-threads=1
cargo test --locked -p hkask-mcp-kata-kanban -- --test-threads=1
cargo test --locked -p hkask-regulation -p kask_bridge -- --test-threads=1
/home/mdz-axolotl/Clones/zed-kask/script/clippy -p hkask-mcp-training -p hkask-regulation -p kask_bridge
bash /home/mdz-axolotl/Clones/zed-kask/kask/scripts/check-hkask-no-zed-deps.sh
bash /home/mdz-axolotl/Clones/zed-kask/kask/scripts/check-mcp-tool-tests.sh
bash /home/mdz-axolotl/Clones/zed-kask/kask/scripts/check-commit-msg-selftest.sh
bash /home/mdz-axolotl/Clones/zed-kask/kask/scripts/audit/skill-corpus-s9-s10-sweep.sh
```

After approved Kani provisioning, confirm CLI flags and run each R2 harness; package entry point: `cargo kani -p hkask-mcp-training`. Ordinary Cargo success is not evidence cfg-excluded proofs type-check or execute. Missing verifier is blocked/unknown, not a passing fallback.

Before integration, run applicable invariants, affected-crate clippy, formatting, serial subtree tests, and host check:

```bash
cargo fmt --all -- --check
cargo test --locked --tests --no-fail-fast -p 'hkask-*' -p kask_bridge -- --test-threads=1
cargo check --locked -p zed
```

Mocks, in-memory stores, temporary files only; no real user DB/provider effects without approval. Separate environmental failures from semantic failures. After two failed repair/reverify cycles for a domain, halt it with evidence/owner rather than extending scope indefinitely.

Retain per package: starting/ending revision, paths, commands/results, coverage/exclusions, raw-log references, approval status, remaining items. Closure ledger states: `verified`, `counterexample`, `blocked`, `deferred`, `not-started`, with actual owner and next action. Never invent completion to remove pending items.

| Delivery | Definition of done |
| --- | --- |
| Existing capability | Accurate provenance; R1 wherever relied upon; R2 outcomes; R3 honest statuses; R4 implemented/advisory/declined explicitly; R5 traversal and actual confirmation or named pending owner. |
| Bounded architectural closure | One class with independent evidence, host authority, resource containment, stale rejection, durable activation/recovery, negative seam tests. |
| Research decision | Equal-budget R7 results justify expansion or explicit stop. No forced positive conclusion. |
| Deliberately unclosed | Whole-machine/environment formalization, universal proof search, original target theorem, mutable trusted verifier/objective, global utility optimality, multi-instance composition. |

## 7. Operator decisions and ownership

External approval is an engineering constraint, not the original Gödel-machine mechanism.[^goedel][^rust-boundaries]

| Decision | Safe default and closure |
| --- | --- |
| Commits | Follow explicit current instruction; plan is not permission. One domain per commit; inspect index and staged diff immediately before committing. Never sweep others' files or rewrite shared history. |
| Kani provisioning | Explicit approved environment/version/budget; no runtime dependency. |
| Automatic promotion | None; operator approves activation. |
| Goal gate | Advisory first; blocking and merge protection require operator decision. |
| Evidence privacy/retention | Reproducible records without secrets or publishing private goals; operator approves portable-evidence policy. |
| Prior goals | Inspect full records, obtain ground truth, then score. |
| Experiment effects/costs | Approve R5/R7 budgets and paid inference separately; shadow experiments do not deploy. |

Roles are accountable defaults, not assigned individuals: replace them with actual owners at intake. Advance independent authorized work when blocked; never bypass dependencies. Estimates are uncertain, not additive delivery promises.

## 8. Copy-ready implementation-agent continuation

Copy this block to the implementing agent. It is self-contained given repository access and the plan; it does not require the earlier conversation. Staged replacement and protected confirmation guide execution.[^strangler][^sgm]

```text
Implement the revised Gödel-machine gap-closure program in:
/home/mdz-axolotl/Clones/zed-kask

AUTHORITATIVE PLAN
Read completely before editing:
/home/mdz-axolotl/Clones/zed-kask/kask/docs/plans/goedel-gap-closure-plan.md
It supplies evidence E1–E11, R0–R7, files, dependencies, budgets, acceptance
tests, commands, decisions, and completion criteria. Update its execution
record as you work; do not create a parallel plan. Read current .rules,
AGENTS.md, relevant crate rules, DIVERGENCE.md, and E10's core/MCP repair plan.
Activate coding-guidelines and refactor-architecture; use TDD for code.

GOAL / LIMITS
Finish and demonstrate the existing process and LoRA proof pilot first.
Then enforce ONE bounded promotion lifecycle and test formal checking's
value. No universal proof search or original switch-versus-continuing-search
utility theorem. A-R1 stays out of scope. No auto-promotion. Process,
empirical, and formal evidence differ; passing checks is not deployment
approval. Successful research may conclude the formal gate is not worthwhile.

FIRST MOVES — SHARED TREE
1. Run pwd, git status --short, git log --oneline -5, git rev-parse HEAD,
   git diff --cached --stat. Baseline was ba0485f7a98011dd46da21a5da8a54383137b794;
   never assume it is current. Preserve unrelated edits. Never reset, stash,
   stage broadly, or rewrite history to simplify this task. Coordinate same-
   file edits; recheck state before each edit/commit boundary.
2. If available, use report_load: kind=report,
   name=goedel-gap-closure-plan-2026-09-18. Reconcile with source, not blind
   trust. Do not invent unavailable tools or inaccessible report contents.
3. Inspect prior goal 5e12d79e-5f9f-4314-8f94-5e5f165e58e4; resolve research
   prefix 6f0decbf to full ID. Ask operator ground truth before scoring;
   do not rescore blindly or infer confirmation from continuation.
4. Register a NEW implementation goal with criteria/honest prediction before
   execution; link task via advances. If tools unavailable, surface that and
   request explicit process exception. Never backdate registration.
5. Batch needed operator decisions: commit permission; Kani environment/
   version; advisory vs blocking gate; evidence privacy; experiment/paid-call
   budgets. Missing approval blocks dependent work only. This prompt does
   not itself authorize installations, spending, commits, trust-policy
   changes, real-user DB mutations, or production deployment.

PRESERVE, DO NOT REDO
- 6054422cc3 contains three skill-gate changes, four Kani harnesses, training
  README, despite its broader commit message.
- 30548eece0 committed registry, parse-failure fix, original plan, index.
  Old W1 “commit the remainder” is stale. Do not recommit existing work.
- program-manager Phase 5.5 recursion was correct in 2988f090f8; earlier
  errors were caller transcription. Do not “fix” that skill.
- Prior reported 29/29 tests and 77-skill sweep are historical, not current
  execution evidence. Rerun checks affected by your changes; don't repeat
  the entire research survey.
- Docs count was 68: strict <70 allows ONE extra, not two. Update this plan.
  Keep raw logs in approved artifact storage outside the docs corpus.

IMPLEMENTATION ORDER
R0 reconcile baseline/report/goals/approvals and preserve current work.
R1 FIRST code repair: training_evaluate contains("CORRECT") accepts INCORRECT;
   standard AND benchmark totals omit wrong answers. Write failing public
   Parameters-seam tests, repair both, distinguish errors, record judge/cost.
   Share accounting only; no speculative evaluator framework.
R2 finish EXISTING Kani harnesses after provisioning approval. Pin versions;
   run four harnesses individually, initial 15 minutes each. Keep raw output,
   exit code, source identity, bounds/assumptions/cost. No crates.io kani dep.
   Preflight Option<LoraInit> arbitrary generation; normal Cargo success is
   not proof compilation. Diagnose counterexamples against catalog; never
   weaken assumptions merely to win. Timeout/unsupported means unknown.
R3 make inventory truthful: process, runtime/CI, verified-at-revision, gap.
   One proof pass plus grep is NOT continuing enforcement. MISSING is not OK.
   Preserve fail-loud parsing; do not extend Python. Use confirmed Rust
   libraries for replacement before making the checker required.
R4 advisory goal-link checks; coordinate existing commit-msg/self-tests.
   Hook is bypassable, CI must check independently. Define syntax-only vs
   portable evidence; CI cannot assume private kanban DB access. Test squash,
   rename/delete, bypass, missing/malformed citations, exceptions. Blocking
   and branch protection require operator approval.
R5 ONE genuine isolated skill/prompt candidate or deliberate rejection:
   new goal -> freeze baseline/evaluator/data/budget -> candidate -> real
   harness baseline/candidate -> deterministic decision -> durable verdict
   with goal/run/artifact IDs -> judge -> operator confirmation -> Brier.
   Validate evaluator pass/fail/error first; add missing-evidence, wrong-
   artifact, regression, pending-approval controls. Manual compliance is not
   runtime enforcement; rejection is not utility gain. Training evaluator
   use requires R1; a separately validated evaluator need not wait for it.
R6 deepen acceptance/activation using core-repair P2/P3 authority and safe
   persistence; verify current status. Exact artifact/evaluator/baseline
   binding, host approval, worker limits, stale rejection, serialized
   promotion, crash reconciliation, honest rollback for ONE class. No new
   identity system, extra generic McpRuntime permission gate, or unsafe shell
   evaluator. Safe code alone does not make OS process execution contained.
R7 extract existing circuit transitions into pure Zed-free policy without
   behavior change first. Compare incumbent, empirical-only, same-candidate
   proof-guided arms with equal budgets/sealed evaluation. Keep production
   safety behavior unchanged. Negative research result may close the package.

DEPENDENCIES / SCOPE
R2–R4 may advance independently after R0. R5 needs validated evaluation,
not unrelated infrastructure completion. R6 requires learned evidence needs
and actual core authority/persistence guarantees. R7 shadow work needs
reliable checking; activation waits for R6 and separate approval. If blocked,
advance independent authorized work and preserve an owned blocker.
Proposed §4 modules are not mandatory scaffolding: apply deletion test.
No new framework/MCP server/UI/hot-loading/multiple optimizers. Keep hKask
Zed-free, servers independent of kask_bridge, host adapters in D8. Consult
DIVERGENCE and pin tests before upstream changes. Never race tests with edits
of their inputs or with another stream's shared mutating fixtures.

VERIFY / COMMIT
- One domain per review/commit; no stubs, todo!, unimplemented!, deprecated
  compatibility layers, swallowed errors, or unrelated formatting changes.
- Public seam tests plus pure unit/property/proof tests; mocks/disposable
  state. R6 needs failure-injected integration, not just compiler success.
- Use /home/mdz-axolotl/Clones/zed-kask/script/clippy. Run plan §6 focused
  checks then applicable invariants, serial subtree tests, host check.
  Missing tool/timeout is not pass; distinguish environment from semantics.
- Material skill edits run S9/S10 and measured evaluation. Proofs bind to
  exact source/spec/checker configuration; relevant edits invalidate results.
- Two failed repair/reverify cycles halt the domain with evidence/owner.
- Before EVERY authorized commit inspect git diff --cached --stat AND
  staged diff. Stage only owned paths/hunks; never git add -A. Check commit
  contents afterward; a clean tree does not prove correct ownership.

FINISH / HANDOFF
Update plan execution record, registry only to supported evidence tier, and
saved report if available. Name exact commits or say uncommitted. Include
commands/results, raw proof/test artifacts, approvals, empirical results,
coverage limits, and closure ledger with actual owner/next action per blocker.
Never call the whole program complete while required packages are parked.
Separate delivered milestones from deliberate research non-goals. End with
a concise factual operator decision list; never invent ground truth or claim
the original Gödel theorem.
```

## 9. Execution record and bibliography

This section indexes provenance/verification tiers from §§2 and 6, not new enforcement.

| Record | State |
| --- | --- |
| Original first pass | Artifacts in `6054422cc3` and `30548eece0`; preserve history. |
| Revised plan | Documentation-only revision grounded at `ba0485f7a9`; add its commit only after it exists. No implementation/proof completion implied. |
| R0–R7 implementation | R1 implemented and locally tested in the uncommitted working tree; see execution entry below. Other packages are not complete. |
| Prior goals/report | Unverified here; actual tools and operator confirmation required. |

**Documentation validation for this revision:** whitespace check passed; corpus count remains 68; a local Markdown file-link sweep found zero unresolved targets (anchors and external URLs were not checked). All 14 cited source-range expressions resolve; semantic spot checks covered evaluator, registry, Kani, goal, and circuit evidence. Footnotes, fences, and R0–R7/handoff sections pass structural checks. Edited files have required metadata. The wider metadata sweep found one unrelated existing exception: `/home/mdz-axolotl/Clones/zed-kask/kask/docs/plans/logisheets-spreadsheet-capability-plan.md:6` uses `Chartered`, outside the documented status set; left unchanged for its owning documentation stream. No runtime tests or Kani proofs ran in this documentation-only pass.

### Execution entry — R1, 2026-09-18 (uncommitted)

The operator subsequently requested execution. Implementation began at `ba0485f7a98011dd46da21a5da8a54383137b794`; another stream advanced HEAD to `9c078d6186100f3eae2006ead2c8d424a8d60cc7` with rule changes during validation. This agent created no commit and did not modify the other stream's upstream source changes. No dependencies, provider calls, installations, or deployment were required.

**R0 limitation/process deviation:** kanban/report tools were unavailable in this session. No goal was registered, no prior goal inspected/scored, and no Brier prediction was fabricated. Local implementation proceeded under the direct execution request, but this is **not** claimed to satisfy R0's explicit goal-registration/exception gate or R5's demonstrated workflow. Operator must authorize/resolve that exception before further governed cycles. Current owner: implementing agent for code/evidence, operator for goal/approval decisions.

**Subsequent authorization, 2026-09-18:** operator replied “Approved 1, 2 and 3” to pinned Kani installation/execution, a temporary local-record goal exception, and R3 registry/R4 advisory-only checks. This resolves the continuation approval blocker, not historical goal outcomes. Local execution record `local-goedel-r2-r4-2026-09-18` replaces unavailable kanban registration for this authorized slice; it is not a server-issued goal ID. Criteria: four honest Kani outcomes; Python-free truthful inventory checker with negative tests; nonblocking goal-trailer checks exercised locally and wired independently to advisory CI. No pre-execution probability was recorded for this slice, so no retrospective Brier score will be manufactured. Implementing agent owns delivery; operator owns outcome acceptance. Commits, deployment, paid inference, and a blocking goal gate remain unauthorized.

**R1 behavior delivered:** shared internal evaluation accounting; all attempted examples in standard/benchmark denominators; exact semantic verdicts; explicit `judge_model` with `llm_judged` labeling; separate generation/evaluator errors; invalid method/zero-limit refusal; skipped-invalid versus capped-valid row accounting; strict available-letter benchmark parsing; candidate plus judge resource accounting with partial sums and null totals when reports are incomplete. `adapter_id` remains report metadata, not a deployment selector. No artifact promotion or independent-judge guarantee is claimed.

**Changed implementation:** `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-training/src/tools/evaluate.rs` (`EvaluationSummary`, `training_evaluate`, `eval_benchmark`), `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-training/src/types.rs` (`TrainEvaluateRequest`), and `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-training/src/hkask_mcp_training.rs` (nine `evaluation_*` public-seam tests and local inference fixture). README and LoRA catalog describe the changed contract. No manifest/lockfile changes.

| Verification | Observed outcome / local evidence |
| --- | --- |
| Denominator RED → GREEN | One correct/nine wrong initially returned total 1 and accuracy 1.0; fixed both paths. `/tmp/hkask-r1-red-denominator.log`, `/tmp/hkask-r1-green-denominator.log`. |
| Semantic/config RED → GREEN | Initially accepted `INCORRECT`/prose and invoked inference for unknown method; explicit routing/error tests now pass. `/tmp/hkask-r1-red-semantic.log`, `/tmp/hkask-r1-green-semantic.log`. |
| Usage/error RED → GREEN | Initially omitted judge tokens and treated incomplete usage as complete; fixed shared accounting. `/tmp/hkask-r1-red-usage.log`, `/tmp/hkask-r1-green-usage.log`. |
| Input/benchmark RED → GREEN | Initially admitted whitespace-only answers and extracted letters from prose; fixed parsing and row counts. `/tmp/hkask-r1-red-input.log`, `/tmp/hkask-r1-green-input.log`. |
| Full training tests | `CARGO_BUILD_JOBS=4 cargo test --locked -p hkask-mcp-training -- --test-threads=1`: **38 passed**, binary/doc-test targets pass with zero tests; `/tmp/hkask-r1-final-tests.log`, SHA-256 `786053cde9cf6c6692c7c19bccf4f76df32dd7b850a006279c279b4c3973e584`. Includes bounded count combinations, all-wrong/error paths, invalid input, and measured-zero versus unknown usage. |
| Clippy wrapper | `HKASK_BUILD_JOBS=4 /home/mdz-axolotl/Clones/zed-kask/script/clippy -p hkask-mcp-training`: compiler phase passes all targets/features with warnings denied; wrapper **exits 1** in subsequent repository-wide `cargo machete` on unused `tracing` in `hkask-mcp-spreadsheet`. `/tmp/hkask-r1-final-clippy.log`, SHA-256 `28c18017062dbfee25761efc81ba9862785583a164780da69b18ece660cadd62`. Not a wholly green gate; unrelated dependency untouched. |
| Boundary/format checks | `check-hkask-no-zed-deps.sh` passes; `check-mcp-tool-tests.sh` reports zero violations; targeted rustfmt and `git diff --check` pass. |

Logs in `/tmp/` are local, ephemeral evidence, not a durable report-store upload. Archive them before cleanup or integration. Tested source SHA-256: evaluation `2e8d0a86a62c761425812e8d7153544020da10a9825d29ab982d5ef5bc8f4678`; request types `f73efc0e020c255389797be01ad3f6981ebb3b2a23afa2dd8e7b5ee904fa3eff`; training lib/tests `31f448fd4495f428db304419d0aaaa0a1bfe50b6035fbafde710fb46aaa87002`. Relevant subsequent edits invalidate these results.

| Open item | State | Owner / next action |
| --- | --- | --- |
| R0 goal/report loop | exception approved | Operator approved local records for continuation; prior server goals remain uninspected/unscored. |
| R1 integration | locally verified, uncommitted | Implementing agent: archive evidence; reviewer/operator: review schema changes and authorize integration. Full subtree/host checks not run in this slice. |
| Clippy wrapper's spreadsheet dependency finding | blocked outside R1 scope | Spreadsheet maintainer: assess/remove unused `tracing`, then rerun wrapper; no suppressing baseline added. |
| R2 Kani | attempted, no success established | Pinned toolchain installed and cfg-only compile defects repaired; see resource-limited outcomes below. |
| R3/R4 governance changes | locally tested, uncommitted | Operator approved; Rust inventory checker and advisory hook/CI wiring implemented. Remote CI not executed here. |
| R5 real cycle | blocked | Operator/agent: resolve R0 and evaluator/budget/confirmation prerequisites. R1 tests are not this cycle. |
| R6/R7 | deferred by prerequisites | Runtime/storage/research owners: follow packages; no new promotion/search framework created. |

Scoped review found no remaining blocker in the covered deterministic evaluation contracts. Residual limits: prompt injection and correlated semantic judges remain possible; requested judge identity is not authenticated evaluator independence; resource reporting is not a hard budget; no production inference, formal verification, runtime authority, or deployment testing occurred. Coding-guideline review: assumptions/scope/tests/surgicality checked, no critical violations found; the larger test fixture is justified by public-seam coverage rather than a new framework. Automated `lisp_eval` audit was unavailable, so no machine-evaluated audit score is claimed.

### Execution entry — approved R2–R4, 2026-09-18 (uncommitted)

**Baseline/authority:** `9c078d6186100f3eae2006ead2c8d424a8d60cc7`; local record exception and R2/R3/advisory R4 explicitly approved in the subsequent user message. Other agents changed the staged index during execution; this agent did not stage or commit. Preserve their staged work. No blocking goal gate, deployment, paid inference, or historical goal confirmation is authorized or claimed.

**R3:** `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-regulation/src/bin/check_principle_constraints.rs` is a small standalone tooling binary using the crate's existing serde/YAML dependencies. The bash launcher resolves the workspace and fails if the required inventory is missing. Strict schema rejects unknown fields/statuses, empty principles, duplicate IDs, missing owners/scopes/checks, missing/escaping paths, and unsupported enforcement/evidence forms. `verified_at_revision` requires full revision ID, checker/assumptions, and a referenced artifact; these are structural checks, not authentication of evidence or proof replay. Runtime/CI labels describe recorded scopes, not something the inventory checker proves. Every process/gap entry is visible. No registry command is executed.

The registry is now explicitly a governance inventory, not an axiom system. Narrowed claims match inspectable responsibilities; procedure-only requirements are `documented_process`; LoRA proofs remain `gap`. `ci_enforced` is not used to claim that newly written workflow YAML already ran. The old checker was observed accepting a missing registry (RED); the replacement refuses it (GREEN). Four Rust tests cover malformed/empty/unknown inventory, honest process/gap reporting, path/duplicate checks, and enforcement/evidence requirements.

**R4:** `/home/mdz-axolotl/Clones/zed-kask/kask/scripts/check-goal-traceability.sh` checks Git-parsed trailers for all changed `.agents/skills/` paths; determining materiality remains human review. Local mode examines the staged diff; CI range mode examines each commit, including squash commits and both sides of renames. `Goal: <full UUID>` is syntax only, never proof of existence/preregistration/approval. `Goal-Exception: <reason>` is an exception claim for operator review, not self-authorization. Missing/malformed data and evaluation failures emit `ADVISORY` and return success. Local staged-path coverage can over/under-approximate a pathspec-only commit; independent CI examines actual committed paths and is the authoritative advisory report.

Existing message-hygiene rejection rules remain. The new `goal-traceability-advisory` job in `/home/mdz-axolotl/Clones/zed-kask/.github/workflows/kask-invariants.yml` fetches history, tests the checker, and independently reports the PR/push range with `continue-on-error: true`. Nothing configures branch protection. Invalid/unavailable base refs surface an advisory rather than silently claiming coverage. Tests use disposable Git repositories, never create commits in the shared worktree, and cover malformed/missing/full trailers, body-text false positives, exceptions, add/rename/delete, multi-file squash, hook bypass, actual hook wiring, unaffected paths, and invalid refs.

**R2 installation:** Kani 0.68.0, CBMC 6.11.0, `nightly-2026-08-21` (rustc `1.100.0-nightly (8925ea358 2026-08-20)`) installed via `cargo install --locked kani-verifier --version 0.68.0` and `cargo kani setup`. Workspace default toolchain/manifests/lockfile unchanged. The old installation URL returned 404; use the versioned official source: https://github.com/model-checking/kani/blob/kani-0.68.0/docs/src/install-guide.md.

Kani exposed two cfg-excluded compilation defects: missing `LoraInit: kani::Arbitrary` and using `init` after moving it. Fixed with `#[cfg_attr(kani, derive(kani::Arbitrary))]` and computing the expected predicate before the move. Normal runtime semantics are unchanged. After these two repair cycles, no production refactor or weakened proof assumptions were attempted. The first CBMC retry was stopped as its RSS approached 4 GiB while exploring formatting/allocation, without a proof result. Each final harness attempt uses `--default-unwind 12`, **all safety/unwinding checks enabled**, 2 GiB address-space limit and 120-second wall limit. Resource exhaustion is **unknown**, even when Kani's top-level summary says `VERIFICATION:- FAILED`; it is not a property counterexample.

Artifacts are retained outside the docs corpus at `/home/mdz-axolotl/.local/state/zed-kask/verification/goedel-2026-09-18/`: install/setup/help, initial compilation errors, bounded harness logs with start/end times and exit files, source/command manifest and checksum list, regression tests, and clippy output. Final per-harness outcomes are recorded with the completed run manifest; no pass is inferred from harness existence or compilation.

| Final bounded harness | Outcome |
| --- | --- |
| `gm3_refuse_iff_degenerate_scaling` | Unknown: `Out of memory`, CBMC status 6, cargo-kani exit 1. |
| `gm4_findings_follow_rank_thresholds` | Unknown: `Out of memory`, CBMC status 6, cargo-kani exit 1. |
| `gm1_clean_iff_noop_init` | Unknown: `Out of memory`, CBMC status 6, cargo-kani exit 1. |
| `safe_region_has_no_refusals` | Unknown: `Out of memory`, CBMC status 6, cargo-kani exit 1. |

All four attempts terminated within their wall limits; no proof worker remains. The resource budget was reduced after the first diagnostic run grew toward 4 GiB; no input domain, property, or safety/unwinding check was weakened. R2's installation/execution milestone is complete, but its proof-success exit criterion is **blocked by verification cost**, not satisfied. Registry remains `gap`. Production validators construct diagnostic strings and vectors; a future behavior-preserving separation of predicate decisions from diagnostic rendering is a hypothesis for reducing proof cost, not a repair silently included in this pass.

**Local validation:** 38 training tests, 90 regulation library tests, and four inventory tests passed; binary/doc-test targets with no cases passed. Advisory self-test and nine existing commit-message fixtures passed. ShellCheck, bash syntax, actionlint, boundary/test-coverage scripts and scoped formatting/whitespace checks pass. Scoped clippy's Rust phase passes under warnings denied; the wrapper still exits 1 at repository-wide cargo-machete's unrelated spreadsheet `tracing` finding. No remote CI, full host check, proof CI gate, R5 real skill cycle, or R6/R7 runtime promotion/search is claimed.

**Next owner/action:** implementing agent retains responsibility for final proof result classification and artifact manifest; if bounded proofs do not succeed, formal-methods owner must propose a separately reviewed pure-predicate extraction or revised resource budget. Operator owns R5 scope/budget and actual outcome confirmation; runtime/storage owners retain R6 prerequisites. Do not silently increase proof budgets, disable checks, or flip the proof inventory to enforced.

### R2 unblocking and foundation triage — 2026-09-18

**Baseline:** operator committed prior work in `ff43b4e80cb22dacf7d31ec9bdbec91b47b7bd6a`; tree/index clean at intake. Operator then authorized the recommended predicate extraction and foundation triage. The local-record exception remains applicable: `local-goedel-r2-core-2026-09-18`, implementing agent responsible, operator responsible for acceptance. Criteria are unchanged runtime findings, successful bounded checks of shared decision logic, reproducible evidence, and an owned next-phase blocker list. No retrospective prediction/Brier score, new commit, deployment, or paid inference.

**Extraction:** `MathDecisions`/`math_decisions` in `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-training/src/lora_validation/param_gates.rs:38–76` produce eight optional severities. The existing production G-M1–G-M4 renderers consume those outputs; their predicate logic is not duplicated in a separate proof-only implementation. Diagnostic strings, order, source citations, and remediations remain unchanged. `LoraBias` gains only a cfg-gated `kani::Arbitrary` derive for enum coverage. No solver stubs, disabled safety checks, or restricted integer domains were introduced. The original safe-region assumptions remain `1 <= r <= 128` and `alpha >= 1`; warning/refusal policy is not revised or claimed mathematically optimal.

**Proof boundary, explicitly revised:** Kani establishes decision outputs/severities, not allocation/formatting/serialization. The original harnesses tried to check the entire diagnostic-producing functions and exhausted memory. This is an explicit decomposition of that obligation: shared core formally checked; diagnostic/MCP connection regression-tested. It is not an end-to-end formal proof relabeled as complete. Before extraction, a public `training_validate_config` test captured all output across **1,944** rank/alpha/rsLoRA/initializer/bias combinations, including `PissaNiter(u32::MAX)`. Independent gate/severity/verdict assertions and SHA-256 `ea9d10460a38b80097632971751ad1815aa19f166ff4c78d7b59b17f2af2d58e` pin full output. The same test passes after extraction.

| Harness | Initial successful core run (CBMC verification time) | Domain / reachability |
| --- | --- | --- |
| `gm3_refuse_iff_degenerate_scaling` | 0.074s, exit 0 | Arbitrary u32 rank/alpha and bool rsLoRA; zero and high-rank branches covered. |
| `gm4_findings_follow_rank_thresholds` | 0.072s, exit 0 | Arbitrary u32 rank; low/warning/refusal regions covered, severity asserted. |
| `gm1_clean_iff_noop_init` | 0.113s, exit 0 | All optional initializer variants and payloads; no-op/base mutation/MAX payload covers. |
| `safe_region_has_no_refusals` | 0.271s, exit 0 | Existing safe region; all eight decision outputs checked; boundary/MAX alpha cover. |
| `gm2_warns_iff_bias_breaks_merge` (additional) | 0.049s, exit 0 | Every bias variant; each covered, warning iff merge-breaking. |

All five completed with Kani 0.68.0/CBMC 6.11.0, unwind 12, all default safety/unwinding checks, **same 2 GiB/120-second limits**. All `kani::cover!` obligations satisfied. Times exclude compilation and are not production speedups. The reproducible runner `/home/mdz-axolotl/Clones/zed-kask/kask/scripts/check-bounded-proofs.sh` repeated all five successfully, exit 0, and verified source hashes unchanged during the run. It refuses a wrong verifier version, evidence-directory overwrite, absent success summary, failed run, or source change. No automatic tool installation or CI promotion gate is added.

**Artifacts:** `/home/mdz-axolotl/.local/state/zed-kask/verification/r2-core-2026-09-18/final-run/` holds exact commands, HEAD plus uncommitted source diff, source hashes, full logs, statuses/times, and `SHA256SUMS`. Earlier `reproducible-run/` is **invalid**: editing the script while it was running caused a shell parse error after the proofs; final-run is the rerun of the finalized script without concurrent edits. Standalone initial successful logs and before/after characterization are in the parent directory. Proof-core SHA-256 `f3f95708363a30261790e49222675c789da87495432b5e41239e24c8d905e9d6`; provider types `dcd23b0705070fcfb93d75bc9829881e1678531d00d2851b558f8b6682cbc320`. Results bind the working tree, not an invented future commit. Registry remains `gap` for committed evidence binding/continuous coverage and explicitly records local core success.

**Small foundation fixes:** added `repository_inventory_is_structurally_valid` in `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-regulation/src/bin/check_principle_constraints.rs:217–227`, so the existing subtree CI test command checks the actual inventory as well as synthetic fixtures. Removed the unused `tracing` declaration from `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-spreadsheet/Cargo.toml` after checking its source for references; Cargo.lock changes only that package's dependency edge. Scoped clippy over training/regulation/spreadsheet now passes **including cargo-machete and buf**, exit 0. This closes the previously recorded wrapper blocker without suppression.

**Broader validation:** `CARGO_BUILD_JOBS=4 cargo test --locked --tests --no-fail-fast -p 'hkask-*' -p kask_bridge -- --test-threads=1` passed: **2,443 passed, 0 failed, 1 ignored across 70 target summaries**. `cargo check --locked -p zed` passed. Separately enabling the MCP process fixture (`cargo test --locked -p hkask-mcp --features test-fixture --test reconnect_integration -- --test-threads=1`) exposed **12 passed / 2 failed**: `foreground_progresses_during_reconnect` and `reconnect_from_a_non_tokio_executor_does_not_panic`. Both panic at `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-mcp/src/runtime.rs:1712` because `tokio::time::timeout` is constructed/polled without a reactor. This is a reproduced baseline foundation blocker, not an R2 regression (runtime file unchanged). The default suite did not enable this fixture. Logs: `subtree-tests.log`, `zed-check.log`, `reconnect-tests.log`, `clippy.log` in the artifact directory. Do not describe the entire validation matrix as green.

### Foundation triage and next-phase admission

Read-only triage used current source and tests at `ff43b4e80c`, with the above fixes applied afterward. These are scoped code findings, not production exploitation claims. No global safety or proof-absence claim follows from passing tests.

| Priority / item | Evidence and disposition | Owner / next test before advancing |
| --- | --- | --- |
| High, reproduced: off-runtime dispatch panic | `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-mcp/src/runtime.rs:1711–1729`; the two fixture tests above fail despite reconnect's existing executor hop. | MCP runtime owner: first next repair. Run deadline/call on its owning Tokio runtime while preserving cancellation and uncertain-delivery semantics. Tests must cover off-runtime calls, foreground progress, timeout, cancellation, and no unsafe replay. Do not merely remove the deadline or detach a call that survives cancellation. |
| High: evaluator preflight executes effects | `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:80–103,3143–3165`: preflight calls `run_evaluator("", ...)`, including `sh -c`, before agent existence check. Confirmed source; not exercised on live data. | Swarm maintainer: separate side-effect-free spec validation; initial R5 route must reject shell evaluators. Handler test: invalid agent must not execute evaluator command. |
| High: requested versus observed model | `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-types/src/ports/inference_port.rs:174–181` default drops model override; `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-training/src/tools/evaluate.rs:192–215` records requested selection without binding returned model. R1 routing tests use an honoring mock; they do not prove every port honors selection. | Inference/evaluation maintainer: test a port returning another model, retain requested/observed identities and explicit mismatch/unknown semantics; account for provider aliases instead of naive equality. |
| High: partial direct-HTTP usage | `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-inference/src/hkask_inference.rs:517–540,724–736`: `{}` is reported usage with zero totals; component-only reports lose total; cost discarded. | Inference maintainer: wire fixtures for absent/empty/partial/genuine zero/complete usage and supported cost fields; use explicit total or checked complete-component sum before claiming total reported. |
| High: cache tokens omitted | `/home/mdz-axolotl/Clones/zed-kask/kask/crates/kask_bridge/src/inference_chat.rs:334–344` sums only uncached input/output. `/home/mdz-axolotl/Clones/zed-kask/crates/language_model_core/src/chat_completion.rs:829–859` demonstrates prompt 12 split into 4+3+5. | Bridge maintainer: bridge test expects prompt 12/total 19, not 4/11; include cache categories and preserve cost. Consult D8 seam. |
| High prerequisite: approval evidence | `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-mcp/src/runtime.rs:1686–1703` dispatches tool/arguments; `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:420–464` accepts caller confirmation boolean. Host metering identity is not host-observed approval. | Core-repair P2 owner: bind actor/action/artifact approval from host, reject bare `true`; keep caller claim distinct. No automatic R6 activation before this seam is repaired. |
| High prerequisite: proof evidence binding | `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-regulation/src/bin/check_principle_constraints.rs:168–179`: structural checker validates hex shape/file presence, not commit existence/content hashes/proof success. Intentionally honest inventory, unsuitable as runtime authorization. | R6/R7 owner: separate artifact-bound verification receipt/checker from inventory; test changed source/spec/log or nonexistent revision rejected. Do not overstate existing `verified_at_revision`. |
| Medium latent: transaction ownership | `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-storage/src/database/transaction.rs:19–53` commits flag before DB success; `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-storage/src/database/sqlite.rs:270–290,332–337` independently leases connections. Production misuse not established. | Core-repair P3 owner: inventory callers; delete unused facade or replace with connection-owned transactions. Pin failed commit and pooled concurrency before promotion persistence. |
| Medium: strategy telemetry is not acceptance | `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-regulation/src/strategy_evaluator.rs:119–151` rotates named strategies based on acceptance ratios. | Regulation owner: keep out of promotion authority; replace misleading effectiveness/promotion claims only with actual paired evidence in its own slice. |
| Closed local: inventory only tested on fixtures | Actual-registry test added and passes; existing subtree CI includes the binary tests. Remote CI not observed. | Governance maintainer: retain structural-only semantics. |
| Closed local: clippy wrapper dependency finding | Removed unused spreadsheet dependency; complete scoped wrapper passes. | Implementing agent: retain lockfile's single corresponding edge deletion. |

**Next sequence:** repair reproduced off-runtime dispatch panic → evaluator preflight/containment for the chosen R5 route → observed model and complete usage semantics on that route → one manual bounded process cycle with held-out evidence → host-authenticated approval and connection-owning recovery before R6 activation. R7 shadow modeling may proceed only with honest evidence labels. Kani success is not permission to bypass these blockers.

[^goedel]: Schmidhuber, J. (2003; revisions through 2006). *Gödel Machines: Self-Referential Universal Problem Solvers Making Provably Optimal Self-Improvements*. IDSIA-19-03. https://arxiv.org/abs/cs/0309048. Theorem: https://people.idsia.ch/~juergen/gmweb4/node11.html. Limits: https://people.idsia.ch/~juergen/gmweb4/node6.html.
[^chapter]: Schmidhuber, J. (2007). *Gödel Machines: Fully Self-referential Optimal Universal Self-improvers*. In *Artificial General Intelligence*, pp. 199–226. https://doi.org/10.1007/978-3-540-68677-4_7.
[^steunebrink]: Steunebrink, B. R., & Schmidhuber, J. (2011). *A Family of Gödel Machine Implementations*. https://people.idsia.ch/~juergen/agi2011bas.pdf. Formal continuation-based refinements, not empirical replacement of the target theorem.
[^dgm]: Zhang, J., Hu, S., Lu, C., Lange, R., & Clune, J. (2025). *Darwin Gödel Machine*. https://arxiv.org/abs/2505.22954. Implementation: https://github.com/jennyzzt/dgm.
[^hgm]: Wang, W., et al. (2025). *Huxley-Gödel Machine*. https://arxiv.org/abs/2510.21614. Implementation: https://github.com/metauto-ai/HGM.
[^rust-loops]: NoemaSI. (2026). *what-is-rsi*. https://github.com/NoemaSI/what-is-rsi. PascualMacana. (2026). *prover*. https://github.com/PascualMacana/prover. Bounded empirical/finite-certificate reference patterns.
[^rust-boundaries]: MathisWellmann. (2026). *Symbiont*. https://github.com/MathisWellmann/symbiont. AROS-Lab. (2026). *AROS-SIE*. https://github.com/AROS-Lab/aros-sie. AreevAI. (2026). *Areev*. https://github.com/AreevAI/areev. Patterns, not adopted dependencies or complete security audits.
[^sgm]: Wu, X., et al. (2025; revised September 2026). *Statistical Gödel Machines: From Formal Proofs to Risk-Controlled Self-Improvement*. https://arxiv.org/html/2510.10232. Use revised full text's narrower evidence claims; abstract/historical empirical descriptions differ.
[^kani]: Kani contributors. (n.d.). *Kani Rust Verifier*. https://github.com/model-checking/kani. Version-pinned installation guide: https://github.com/model-checking/kani/blob/kani-0.68.0/docs/src/install-guide.md (the old `/install.html` URL returned 404 during R2).
[^strangler]: Fowler, M. (2004). *Strangler Fig Application*. https://martinfowler.com/bliki/StranglerFigApplication.html. Incremental complete behavioral seams, not permanent parallel implementations.
