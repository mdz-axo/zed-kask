# Multi-Perspective Review — `hkask-templates` Refactor Plan

> Six skills applied to `plan-draft.md`: metacognition, idiomatic-rust,
> grill-me, essentialist, pragmatic-semantics, pragmatic-cybernetics. Each ran
> as an independent sub-agent with full file access. This document synthesizes
> their findings, resolves the conflicts between them, and records the
> consensus that drove the plan revision (`plan-revised.md`).

## Verification greps run by the orchestrator (post-review)

These were run to settle the highest-severity disputes between the reviews.

| Claim | Command | Result | Settles |
|-------|---------|--------|---------|
| `Registry` has zero external consumers | `grep -rEn "hkask_templates::Registry\b\|hkask_templates::SqliteRegistry" kask/crates kask/mcp-servers crates --include="*.rs"` | `Registry`: 1 hit (`tests/bootstrap_test.rs:12`, in-crate). `SqliteRegistry`: 2 hits (`hkask-mcp-kata-kanban` src + tests). | **CAND-1's premise is correct** — `Registry` is test-only. |
| `Effect::ConsumedGas` is produced | `grep -rEn "Effect::ConsumedGas\(" kask/crates/hkask-templates/src --include="*.rs"` | 0 production producers; only the enum def (`step_actions.rs:49`) + the consumer arm (`step_machine.rs:623`). | **Idiomatic-rust was right; metacognition's "documented no-op" reclassification was wrong.** The variant is dead. |
| `dispatch_compute` arm count | `grep -nE '^\s+"[a-z_.]+"\s*=>' compute.rs` | 19 string arms (6 sub-domains: forecast, kata, swarm, listening, lisp, shell) + 2 inner arms. | **Plan said 17 (close); grill-me said 8 (wrong).** Actual: 19. The 6-sub-domain framing is correct. |
| File line counts | `wc -l` | `compute.rs` 3,379; `step_actions.rs` 3,146; `step_graph.rs` 322; `bundle/manifest.rs` 598; `registry_sqlite.rs` 825. | **Plan's `compute.rs`=1,270 and `step_actions.rs`=2,895 are both stale.** |
| Trait ownership | `grep -rn "trait RegistryIndex\|trait SkillRegistryIndex"` | Both in `kask/crates/hkask-types/src/ports/registry.rs:286,309`. Only `BundleRegistryIndex` is in `hkask-templates`. | **Grill-me was right: CAND-1's scope spans two crates.** |

## Per-skill verdict summary

| Skill | Verdict | Key contribution |
|-------|---------|------------------|
| **Metacognition** | RUNNING (hypotenuse 0.673→0.386, −43%) | Added a grounding/verification section to the plan; flagged 5 inaccurate sizing claims, 10 unverified assumptions, 8 typed obstacles. Over-reclassified `Effect::ConsumedGas` (corrected by idiomatic-rust + grep). |
| **Idiomatic-rust** | Overall critique 0.22 (plan survives most challenges) | Per-candidate Hoare P1-P8 assessment; sketched `SubCascade` builder (resolves CAND-2's signature problem), `ComputeRef` enum (CAND-4's missed P1), `StepOrdinal` newtype (CAND-7). Confirmed `Effect::ConsumedGas` is dead. Noted `Box::pin` recursion guard CAND-2 omits. |
| **Grill-me** | NOT READY (Recall=Partial, Mechanism=Gap, Rationale=Gap, Edge=Partial, Synthesis=Gap) | Hardest-hitting: CAND-2's "subtle differences" are 5 concrete items (budget constructor, context merge, depth, manifest_id, post-run merge) the plan hand-waves; CAND-1 conflates re-export demotion (CAND-10) with type deletion; CAND-1's traits are in `hkask-types`; `dispatch_compute` closures are defined once, not "redefined per arm." |
| **Essentialist** | 73% essential, 27% cosmetic | G1/G2/G3 applied per candidate. **CAND-3, CAND-4, CAND-8 FAIL G1+G2** (pass-through extraction, >7 public items). Recommends cutting all three to minimal versions. CAND-7 PASSES G1 after verifying `validate()` does NOT reject per-action field misuse. |
| **Pragmatic-semantics** | Probabilistic-grade (needs verification) | 15 load-bearing claims classified; 5 conflicts resolved via OT ranking. Key: `.rules` "propagate errors" is Prohibition-grade, overrides CAND-9's "may be gated" (Guardrail). RA-01 was NOT deferred (prior audit marked `deferred: no`); plan mis-attributes. "22 of 32" count unreproducible (actual 36 symbols). |
| **Pragmatic-cybernetics** | Degraded (not unviable) | Per-loop analysis of B1-B10: **CAND-9 addresses 3/10, adequately for 1 (B3), partially for 1 (B1), inadequately for 1 (B2). 6/10 broken loops unaddressed.** Variety deficit: 56 disturbances vs 11 responses; critical deficits in broken-loops (E) and coupling (F) classes. Recommends CAND-9b–9e (4 new sub-candidates) + CAND-12–15 (coupling cluster). |

## Cross-skill consensus (what all or most reviews agreed on)

1. **The plan's sizing is stale.** Metacognition, grill-me, and idiomatic-rust all flagged `compute.rs` (1,270→3,379) and `step_actions.rs` (2,895→3,146). The orchestrator's greps confirm. **Action: re-measure all file/line/arm counts in the revised plan.**

2. **CAND-1's scope is wrong.** Grill-me and pragmatic-semantics both found that `RegistryIndex` and `SkillRegistryIndex` live in `hkask-types`, not `hkask-templates`. CAND-1 can only delete `BundleRegistryIndex` locally; the other two require a cross-crate change. **Action: split CAND-1 into CAND-1a (local: delete `Registry` + `BundleRegistryIndex` + `*_owned` forwarders) and CAND-1b (cross-crate: delete `RegistryIndex`/`SkillRegistryIndex` from `hkask-types`, deferred).**

3. **CAND-1's dependency on CAND-10 is false.** Grill-me and pragmatic-semantics both found that CAND-10 (demote re-export) and CAND-1 (delete type) are independent operations. **Action: remove the false dependency; CAND-1a can proceed independently.**

4. **CAND-2's single-signature `run_sub_cascade` is underspecified.** Grill-me enumerated 5 concrete differences between the 3 call sites (budget constructor, context merge, depth, manifest_id, post-run merge). Idiomatic-rust sketched a `SubCascade` builder struct with `for_flowdef`/`for_parallel_branch` constructors that resolves this. **Action: adopt the `SubCascade` builder design; add `Box::pin` recursion guard; address cancellation semantics.**

5. **CAND-9 is a Prohibition, not a Guardrail.** Pragmatic-semantics (OT ranking: `.rules` Prohibition > plan's Guardrail) and pragmatic-cybernetics (6/10 broken loops unaddressed) both found the plan under-scopes the feedback-loop fixes. **Action: reframe CAND-9 as a `.rules`-mandated fix; add CAND-9b–9e for the 6 unaddressed loops (B4-B7, B9); specify algedonic routing (`tracing::warn!` at the SQL boundary).**

6. **CAND-3, CAND-4, CAND-8 are cosmetic (as originally proposed).** Essentialist (FAIL G1+G2 for all three), grill-me (CAND-4's closure mechanism is factually wrong), and idiomatic-rust (CAND-4 misses the `ComputeRef` enum P1 opportunity) converged. **Action: cut CAND-3 to minimal (extract `call_inference_stream*` only); cut CAND-4 to minimal (extract shared `compute_input` helper + add `ComputeRef` enum); defer CAND-8 (1-line guard, not worth the split).** **Subsequent operator-requested audit of `dispatch_compute` (see `dispatch-compute-audit.md`) revised CAND-4 further: the audit found 3 dead forecast arms (0 manifest callers), 3 swarm arms = 44% of the match body (strongest extraction candidate), 0 external callers, and a stale hardcoded error message. The revised CAND-4 deletes the dead arms, adds the `ComputeRef` enum, extracts the swarm sub-domain into `swarm_compute.rs`, adds `ComputeInput` + `CommandRunner`, and leaves the other 5 sub-domains inline.**

7. **CAND-7's serde tagging strategy is the load-bearing unspecified decision.** Idiomatic-rust and pragmatic-semantics both flagged this. `#[serde(tag = "action")]` (internally tagged) breaks `deny_unknown_fields`; adjacent tagging requires YAML shape change. **Action: add a spike for CAND-7's serde strategy before it exits deferral; factor `StepCommon` for shared fields; note the `Arc` consolidation win (18→1 per node).**

8. **CAND-10's "22 of 32" count is unreproducible.** Pragmatic-semantics counted 36 symbols; the plan lists 19 names. The prior seam-audit marked `ManifestLoadError`, `ports::*`, `PromptStrategy` as "live." **Action: re-run the external-consumer grep with per-symbol output; reconcile against the prior audit's live list before CAND-10 executes.**

9. **The plan's sequencing is wrong.** Grill-me (CAND-10 first delivers least leverage), pragmatic-cybernetics (CAND-10 removes spec-drift signal surface before wiring enforcement), and pragmatic-semantics (CAND-9 is Prohibition-grade, should not be gated) all found the sequencing suboptimal. **Action: re-sequence by leverage — CAND-9 (Prohibition fixes) first, then CAND-2 (correctness: MAX_STEPS gate), then the rest.**

## Cross-skill disputes (and resolution)

| Dispute | Skills involved | Resolution |
|---------|-----------------|------------|
| `Effect::ConsumedGas` classification | Metacognition: "documented no-op" (Inference). Idiomatic-rust: "dead, never produced" (compiler_confirmed). | **Grep settled it: 0 producers.** Idiomatic-rust was right. The variant is dead. Metacognition's reclassification was an over-correction. |
| `dispatch_compute` arm count | Plan: 17. Grill-me: 8. Idiomatic-rust: 19. | **Grep settled it: 19 string arms.** Idiomatic-rust was right. Grill-me's "8" was wrong (it only counted the forecast sub-domain). The plan's "17" was close but off by 2. |
| CAND-3 necessity | Essentialist: FAIL G1+G2 (cosmetic). Idiomatic-rust: low-risk, low-leverage but a prerequisite for CAND-7. | **Essentialist wins on G1/G2 grounds (navigability is not behavior). Idiomatic-rust's prerequisite argument is valid but only if CAND-7 lands.** Resolution: cut CAND-3 to minimal (extract `call_inference_stream*` only); keep the per-action split as an optional follow-up gated on CAND-7 proceeding. |
| CAND-8 necessity | Essentialist: BORDERLINE FAIL G1 (1-line guard, not worth split). Idiomatic-rust: sound P1 gain, commit to enum. | **Essentialist wins.** The branching is a 1-line `if kata_enabled()` guard. The P1 gain (inactive model's fields are invalid state) is real but low-severity — a manifest uses one or the other, never both, so the "invalid state" is never reached. Defer CAND-8; extract Kata math into private functions instead. |
| CAND-9 PR boundary | Pragmatic-semantics: Prohibition, not gateable. Metacognition: behavior-change ambiguity (O5). Grill-me: trace the error path to UI. | **Pragmatic-semantics wins (OT ranking).** The `.rules` Prohibition overrides the "callers may depend on silence" Hypothesis. CAND-9 ships behind a release note, not a gate. But grill-me's caller-audit requirement stands: grep all call sites of the 4 write methods + `row_to_skill` + `count` and decide per-caller handling. |
| CAND-1 deferral status | Pragmatic-semantics: RA-01 was NOT deferred (prior audit marked `deferred: no`). Plan: "RA-01/09/10 deferred." | **Pragmatic-semantics wins (primary source).** Strike RA-01 from the deferred grouping. Only RA-09 (`Registry`) and RA-10 (`BundleRegistryIndex`) were deferred. The deferral reason ("user decides") is a meta-reason, not a technical blocker — operator approval of this plan resolves it. |

## What each skill uniquely contributed

- **Metacognition**: the grounding/verification section (now in the plan); the PKO gap analysis (Outcome criteria missing); the Brier-scored prediction that a grounding edit closes the object-space gap but not the process-space gap.
- **Idiomatic-rust**: the `SubCascade` builder design (resolves CAND-2); the `ComputeRef` enum (CAND-4's missed P1); the `StepOrdinal` newtype; the `Box::pin` recursion guard; the `StepCommon` factoring for CAND-7; the `Arc` consolidation win.
- **Grill-me**: the 5 concrete sub-cascade differences; the CAND-1→CAND-10 false dependency; the CAND-1 cross-crate scope; the `dispatch_compute` closure mechanism (defined once, not per arm); the `count`→sensor trap; the critical-path analysis.
- **Essentialist**: the G1/G2/G3 verdicts per candidate; the "cut CAND-3/4/8" recommendation; the verification that `validate()` does NOT reject per-action field misuse (CAND-7 passes G1); the 73% essentialism score.
- **Pragmatic-semantics**: the 15 load-bearing claim classifications; the OT-ranking resolution of the CAND-9 Prohibition vs Guardrail conflict; the RA-01 mis-attribution; the "22 of 32" count unreproducibility; the `BundleManifest` lacks `deny_unknown_fields` finding (CAND-5 spike scope).
- **Pragmatic-cybernetics**: the per-loop analysis of B1-B10 (6/10 unaddressed); the variety deficit (56:11); the VSM map (S2 degraded medium-term, S3 partially unwired, S4 at risk); the CAND-9b–9e and CAND-12–15 amplification proposals; the algedonic routing specification.

## Revised plan: changes made

Based on the consensus above, the revised plan (`plan-revised.md`) makes these
changes:

1. **Re-measured all file/line/arm counts** against the greps.
2. **Split CAND-1** into CAND-1a (local: `Registry` + `BundleRegistryIndex` + `*_owned`) and CAND-1b (cross-crate: `hkask-types` traits, deferred).
3. **Removed CAND-1→CAND-10 false dependency.**
4. **Adopted `SubCascade` builder design** for CAND-2 (replaces single-signature `run_sub_cascade`); added `Box::pin` + cancellation semantics.
5. **Cut CAND-3** to minimal: extract `call_inference_stream*` into `inference.rs` (dedup C5) only; per-action split deferred as optional follow-up gated on CAND-7.
6. **Cut CAND-4** to minimal initially, then **revised again after the operator-requested `dispatch_compute` audit** (`dispatch-compute-audit.md`): the audit grounded the design in measured data (3 dead forecast arms, 3 swarm arms = 44% of match body, 0 external callers, stale error message). The revised CAND-4 deletes dead arms, adds `ComputeRef` enum, extracts swarm sub-domain into `swarm_compute.rs`, adds `ComputeInput` + `CommandRunner`. The other 5 sub-domains stay inline (essentialist G1 FAIL for pass-through extraction).
7. **Deferred CAND-8**; replaced with "extract Kata math into private functions."
8. **Reframed CAND-9** as `.rules` Prohibition (not Guardrail); added CAND-9b–9e for the 6 unaddressed broken loops; specified algedonic routing.
9. **Added CAND-12–15** (coupling cluster: typed `StepResultKey`, dependency-direction audit, crate-root reconciliation, DIVERGENCE seam test).
10. **Added CAND-7 serde strategy spike** as a pre-sequencing task; factored `StepCommon`; noted `Arc` consolidation.
11. **Re-sequenced by leverage**: CAND-9 (Prohibition) → CAND-2 (correctness: MAX_STEPS) → CAND-9b–9e (remaining loops) → CAND-12 (typed key, pairs with CAND-3-minimal) → CAND-1a → CAND-5 (spike-gated) → CAND-6 → CAND-10 (after reconciliation grep) → CAND-11 → CAND-4 (audit-grounded) → CAND-7 (spike-gated, last).
12. **Added per-candidate measurable success criteria** to the verification plan (closes the metacognition Outcome gap).
13. **Added a pre-sequencing experiments section**: CAND-5 spike, CAND-7 serde spike + blast-radius measurement, CAND-10 reconciliation grep.
14. **Corrected `Effect::ConsumedGas`** to "dead (never produced)" per the grep.
15. **Corrected the RA-01 mis-attribution**; cited the prior audit's strangler-fig migration plan as CAND-1a's execution template.
16. **Added `dispatch-compute-audit.md`** as a research spike (operator-requested) grounding CAND-4 in measured per-arm data: manifest usage counts, test counts, line counts, closure mechanism, external callers, dead surface identification.
