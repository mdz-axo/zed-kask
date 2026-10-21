# Goal closure, outcome assessment and forecast verification

Date: 2026-10-20. Status: **P1–P6 approved by the operator; code cutover implemented; docs/prompt/skill cutover implemented in the shared tree.** Deployment and real end-to-end editor acceptance are not claimed.
Task: `03f242e1-4a92-4fd0-b8c4-9973379daa42`, board `8e1c3324-2745-4272-bd5c-b026f651a9ca`.
Specification: user's amended six-section request, retained in task description/activity and this session. T1/T2 independent; T3 waited for both; T4 followed T3; T5 followed T4; T6 integrates the result.

## Decision summary

**Recommend verification-primary goals plus optional eligible forecasts.** Keep human specification and acceptance authority. Automate evidence-assessment orchestration, score arithmetic, delivery, reconciliation and retry administration. Do not infer achievement from closure, substitute an assessor for the human, or make collecting a score a prerequisite to closing ordinary work.

Retain the shared binary Brier arithmetic. Replace coupled close-and-score, response-only memory publication, deletion-on-acknowledgment and single-score-to-memory-confidence policy. Across forecast domains, replace universal score bands, fabricated priors and unsupported score-driven interpretation/adjustment. Keep useful market/scenario/experiment forecasting capabilities with explicit prospective admission and domain-specific resolution rules.

This deepens the human's seat at the bench: the person holds the question and veto; the system supplies evidence and bookkeeping. It answers the project's V2/V3/V4/V6 charter by making disagreement challengeable and eliminating repetitive scoring labor, rather than automating human judgment away (`kask/docs/architecture/core/magnifica-humanitas.md:36–74`). That is a project-policy argument, not statistical evidence.

### What the inquiry found (historical pre-cutover observations)

- **Observed implementation:** `(p-y)^2` is correct. **Delegated live-data observation:** 191 surviving numeric intake/score pairs match it within `1e-12`. This is not evidence of calibrated probabilities or valid labels.
- **Observed implementation:** scoring does not require authenticated human confirmation, a prior assessment, a structured horizon or independent evidence. This is an enforcement gap, not proof anyone closed a goal without permission.
- **Observed implementation:** memory capture preserves abbreviated tool responses, not complete original criteria and judgment evidence; acknowledgment deletes the full goal record. One-score Brier also changes historical memory confidence and therefore recall/retention behavior.
- **Delegated live-data observation:** nine retained resolved goals lack surviving per-goal score memory; eight extra score rows repeat eight goal identities. Historical denominator and outcome-origin evidence are incomplete.
- **Observed implementation:** universal raw-score quality labels reach tools/prompts; empty qualifying calibration bins can become `well_calibrated`; missing equity probability can become `.7`. Raw-score policies affect later weighting, correction and reported market reliability tiers. The inspected High-to-Medium demotion does not cross the downstream Low-only admission gate.
- **Not established:** Goodhart effects, self-confirmation as a cause of observed scores, unauthorized human closure, or genuine learning. Selection/agent influence are hypotheses. Short horizon alone does not invalidate a forecast.

## 1. Historical evidence register and pre-cutover lifecycle

Paths below are relative to the canonical repository. Labels distinguish source inspection, documentation and inference. Source lines were inspected in the live shared tree; HEAD moved during this read-only inquiry. T1 closing HEAD was `bf45c8990e5e6e87cc0467011db12feea080bbe8`; T5 assessed `1793662db4aeaaded66e1c67d8693cae724d712d`; parent checkpoint HEAD was `ecb7e42f233857d01cee82581971cf75047b803a`. This is not an atomic single-commit audit. Re-pin citations before implementation.

| Claim / stage | Actual source | Strength and limit |
|---|---|---|
| Declaration: text, 1–4 criteria, optional p in [0,1], owner and persistence | `kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/service_impl/goals.rs:59–107` | Code inspection; individual criterion descriptions are not checked for meaningful content here |
| Current schema lacks structured deadline/resolver | `kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/types/goal.rs:20–70` | Code inspection; a deadline may occur in free text |
| Progress judgment checks ownership and complete criterion-index coverage | `kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/service_impl/goals.rs:225–282` | Code inspection; `done` is not cross-checked against all `passed` booleans |
| Human confirmation required by prompt, not authenticated by scoring API | `crates/agent/src/templates/system_prompt.hbs:340–350`; kata-kanban `src/types.rs:142–156`; `service_impl/goals.rs:298–325` | Prompt OUGHT versus code IS; same owner is not evidence of human-origin confirmation |
| Binary arithmetic; no p means no score; same-outcome replay accepted, conflicting resolution rejected | `kask/crates/hkask-forecast/src/hkask_forecast.rs:214–237`; kata-kanban `service_impl/goals.rs:304–331` | Code inspection; preserves useful protections |
| Atomic transition re-reads under storage lock | kata-kanban `service_impl/goals.rs:374–401`; `kask/crates/hkask-storage/src/hmem.rs:489–549` | Existing substantive mechanism to retain, not replace with a new repository trait |
| Tool responses omit criteria and rich judge evidence | kata-kanban `src/hkask_mcp_kata_kanban.rs:475–483,526–532`; `crates/agent/src/thread.rs:307–324,4465–4474` | Response-only capture plus static inference of what cannot be reconstructed from it |
| Publication: embedding required; failed events isolated; score depends on create memory | `kask/crates/kask_bridge/src/memory/ingest.rs:176–218,342–425,511–615` | Code inspection; real safeguards, incomplete durable replay source |
| Ack deletes full goal; no receipt authentication at server | kata-kanban `service_impl/goals.rs:353–371` | Code inspection; projection becomes the only surviving structured record |
| Automatic ack bypasses acknowledgment-event purge path | `crates/agent/src/thread.rs:327–359`; `crates/zed/src/main.rs:3595–3619`; bridge `memory/ingest.rs:332–340` | Static call-path finding, not runtime reproduction; manually supplied event tests do not prove production route |
| Individual Brier mutates create confidence using clamp(1−2BS,.05,.95) | bridge `memory/ingest.rs:163–250`; recall `kask_bridge/src/memory.rs:1044–1061`; cleanup `hkask-memory/src/consolidation_service.rs:41–48` | Code inspection; possible selective survival of historical evidence is inference |
| No wired goal-cohort dashboard found; other forecast populations are separate | `crates/hkask-scenarios-widget/src/view.rs:203–235`; scenarios `src/hkask_mcp_scenarios.rs:240–290`; experimentation `src/types.rs:416–469` | Bounded whole-repository name/consumer census, not proof of absence outside inspected surface |
| Metacognition reads resolved goals from a list whose acknowledged rows disappear | `.agents/skills/metacognition/SKILL.md:62–65`; `kask/registry/templates/metacognition/meta-grasp-current.j2:51–58` | Live instruction conflicts with successful-pruning lifecycle |

Pre-cutover ownership: human declares/accepts in conversation → worker calls create/judge/score → kanban stores full row → thread buffers response events → bridge embeds abbreviated responses and mutates confidence → autoack prunes kanban row. Numerical scoring, semantic judgment, evidence custody and delivery are coupled across these boundaries.

### Cross-domain consumers that change the redesign scope

| Surface | Evidence | Disposition proposed, not implemented |
|---|---|---|
| Universal excellent/good/fair/poor labels | `hkask-forecast/src/hkask_forecast.rs:240–253,672–677`; scenarios `superforecast/math.rs:379–398`, `hkask_mcp_scenarios.rs:1129,1168–1192,1458`, `equity_forecast.rs:508`, `types.rs:650–652` | Delete labels and enforcing tests, not arithmetic |
| Empty or signed-cancelling bias becomes calibration label | scenarios `superforecast/assess.rs:631–652`; widget `block.rs:84–94`, `view.rs:217,252–255,660–666` | Explicit insufficient evidence; retain per-bin data, no certification from signed mean |
| Synthetic probability/performance defaults | equity `equity_forecast.rs:77–87,421–425`; synthesis `superforecast/assess.rs:489–496`; market bridge `superforecast/bridge.rs:242–255` | Missing stays missing; scaffold `.5` is not a score-eligible declaration |
| Raw inverse-Brier synthesis weights; non-comparable or missing history | scenarios `superforecast/assess.rs:475–511` | Remove unsupported weighting/default; preserve explicitly declared equal-weight synthesis unless a defensible method is approved |
| Five-observation raw-Brier policy demotes High to Medium | prediction-markets `src/types.rs:251–272,433,463`; scenarios `superforecast/bridge.rs:233–255` | Remove unsupported score-based reliability claim; preserve liquidity/spread and raw history. This demotion does not trigger the inspected Low-only admission rejection |
| Historical correction feeds later forecasts | scenarios `hkask_mcp_scenarios.rs:1294–1307`, `superforecast/bridge.rs:143–184,223–225` | Withhold unsupported adjustment; retain pairs for evaluation under approved cohort policy |
| Resolution can create scenario forecast record; equity can mix stored p with submitted return target | scenarios `hkask_mcp_scenarios.rs:1211–1235`; equity `equity_forecast.rs:421–439` | Require frozen forecast ID/proposition and resolver, not post-outcome reconstruction |
| Denominators differ | scenario `superforecast/math.rs:365–379`, `superforecast/store.rs:144–148,214–218`, `assess.rs:600–642`; market `calibration.rs:134–165`; experiment `types.rs:416–468` | Report per-domain identity and weights. Repeated outcomes must not add weight; do not pool unrelated domains |

Scenario historical resolved selection already checks outcome presence; its downstream false fallback is not evidence that unresolved cases enter that mean. Experiment eligibility excludes missing probabilities and typed void/nonbinary outcomes. These legitimate distinctions must survive cutover.

## 2. Statistical purpose: sources, conventions and transfer

Corpus-first retrieval consulted source-bearing `zk-ref-open` passages; episodic memories and ontology text were not statistical authority. Research run `85a4e5dcbc78a962` is **partial**, with access states and retained excerpts. Exact short quotations below carry the limitations of their accessed carriers.

| ID | Original work and identifier | Access / exact passage | Claim supported |
|---|---|---|---|
| B50 | Brier (1950), *Verification of Forecasts Expressed in Terms of Probability*, MWR 78(1):1–3. DOI `10.1175/1520-0493(1950)078<0001:VOFEIT>2.0.CO;2` | Existing three-page corpus OCR read; remote publisher extraction only watermark. Verification Formula excerpt: “he is encouraged to state unbiased estimates of the probability of each event”. Comparison section excerpt: “knowledge of a good relationship between forecast and observed probabilities is not sufficient to indicate how useful the forecasts are”. Section locator across pp.1–3, not visually collated page assignment | Vector score, honest reporting, calibration not equivalent to usefulness; six-hour forecasts refute blanket long-horizon requirement |
| M73 | Murphy (1973), *A New Vector Partition of the Probability Score*, JAM 12:595–600. DOI `10.1175/1520-0450(1973)012<0595:ANVPOT>2.0.CO;2` | Full publisher PDF extracted. Abstract p.595: “The new partition consists of three terms”; §3 Eq.(6), §5 Table 2: .480+.136−.330=.286. Mathematical OCR imperfect | Uncertainty + reliability − resolution, not raw score = calibration |
| G14 | Mellers et al. (2014), *Psychological Strategies for Winning a Geopolitical Forecasting Tournament*, Psychological Science 25:1106–1115. DOI `10.1177/0956797614524255` | Main abstract only: “improved both calibration and resolution.” Full publisher supplement: DS11 “Your score for that forecasting question equals the unweighted average of your daily Brier scores”; DS12 “Calibration, alone, is not enough.” | Training/teaming/tracking interventions; daily averaging, carry-forward and missing-participation rules, not score collection alone as learning |
| G15 | Mellers et al. (2015), *Identifying and Cultivating Superforecasters as a Method of Improving Probabilistic Predictions*, PPS 10:267–281. DOI `10.1177/1745691615577794` | Full paper: p.268 “clairvoyance test”; p.270 “we standardized Brier scores within each question and averaged across questions”; p.279 “Real-world accuracy data are noisy.” | Precise questions, difficulty/comparability, separate diagnostics, later-year validation and limits of transfer |
| TG15 | Tetlock & Gardner (2015), *Superforecasting*, Crown; ISBN 9780804136693, ebook 9780804136709 | Local calibre-repaginated full PDF, not authenticated publisher edition. PDF leaf49: “requires two more things: benchmarks and comparability”; leaf109: “Brier scores only have meaning in relation to the difficulty of the problem at hand” | Corroborating book text only; **edition provenance remains operator-ruling gap**. Not necessary as sole authority for recommendation |

Primary URLs: [B50 publisher](https://journals.ametsoc.org/downloadpdf/view/journals/mwre/78/1/1520-0493_1950_078_0001_vofeit_2_0_co_2.pdf), [M73 publisher](https://journals.ametsoc.org/downloadpdf/view/journals/apme/12/4/1520-0450_1973_012_0595_anvpot_2_0_co_2.pdf), [G14 supplement](https://journals.sagepub.com/doi/suppl/10.1177/0956797614524255/suppl_file/ds_10.1177_0956797614524255_supplmethod.pdf), [G15 full paper](https://stanford.edu/~knutson/jdm/mellers15.pdf).
Local carriers: `/home/mdz-axolotl/Documents/zk-data/corpus-mcp/extracted/zk-ref-open/brier-1950.pdf.txt`; `/home/mdz-axolotl/Documents/zk-data/corpus-mcp/sources/capabilities-reasoner/Superforecasting_tetlock.pdf`. Do not redistribute book full text as part of this plan.

Binary convention: BS=mean((p−y)^2), range [0,1]. Original categorical convention: mean(sum over categories (p_k−y_k)^2), range [0,2], for mutually exclusive/exhaustive outcomes and probabilities summing to one. Two-category vector BS=2×binary BS; constant .5 gives .5 vector or .25 binary. More categories do not permit arbitrary comparison without identifying the convention/population.

**Derived expected-loss identity, not an unread-source quotation:** E[(p−Y)^2]=(p−q)^2+q(1−q), uniquely minimized at p=q for a fixed outcome law. Strict propriety is about expected reporting incentives with fixed target/selection/weights; it neither proves a realized label nor prevents gaming of task choice, effort or inclusion. An agent influencing success does not automatically invalidate forecasting: it changes the target to success under specified execution policy, resources and stopping rule. GJP did not establish efficacy for agent-controlled software tasks.

Murphy's binary partition is BS=REL−RES+UNC. REL compares probabilities with their conditional observed frequencies; RES measures separation of those frequencies from the overall frequency; UNC is base-rate uncertainty. Exact distinct-probability grouping yields the identity; arbitrary bins need approximation disclosure. A signed mean bias can cancel errors and is not REL. Frequency estimates and decomposition require adequate data; one score is not a calibration diagnostic.

Use a declared reference scored on the **same cases/weights/horizons**. Skill score 1−BS/BS_ref requires BS_ref>0; otherwise undefined, not perfect. Retrospective sample frequency is a descriptive reference, not a prospectively available forecast. Constant .5 is not universal climatology. Dependence does not prevent a mean, but repeated updates do not supply independent outcomes: report events, forecast versions and clusters separately; uncertainty intervals must respect shared-event/session dependence. No universal tiny N proves calibration.

Freeze update rules and lead-time weighting in advance. Either score a fixed lead-time snapshot or a declared time-weighted trajectory; do not let extra submissions improve weight. GJP's daily averaging, carry-forward and missing-participation conventions are specific methods, not defaults to copy without decision. Missing/abandoned/late-resolving cases change the evaluated population; no accessed source supplies a general cure for informative selective resolution. Compare matched task populations and versions, not raw scores across shifting work mixes.

**Ontology conflict:** live `onto_anchor("Brier score")` returned the correct binary formula but said the score “measures calibration, not luck” and called .5 the baseline. Murphy/GJP take precedence for statistical interpretation. Issue recorded via `curator_report_skill_use_issue` for `onto-anchor`; correct definition and consumers after policy approval. No private replacement ontology was invented. Verification resolves to CSO verification/DPV Verification; provenance also resolves to SEPIO:0000058 process history. Those anchors name concepts, not evidence that an outcome occurred.

## 3. Competing explanations and population audit

Initial expectation, stated before probes: event/outcome meaning and interpretation are likelier weaknesses than binary arithmetic. Falsifier: arithmetic failures or a complete authenticated prospective lifecycle with valid interpretation would reverse or narrow that expectation.

The explanations are **not mutually exclusive**; do not manufacture a unique causal winner.

| Explanation | Discriminating observation | Result and disconfirmer |
|---|---|---|
| H1 Correct arithmetic, unsuitable event/outcome process | Compare numerical pairs with original proposition/horizon/resolver provenance | Formula survives; structured horizon/resolver and retained full criteria missing. Suitability concern supported for current contract, not every task. Disconfirm with complete pre-outcome record and externally traceable resolver on a defined eligible cohort |
| H2 Suitable scoring undermined by bugs | Reconcile retained rows, memory events, identities and arithmetic | Arithmetic-error branch contradicted on inspected code/191 pairs; preservation/publication defects remain. Disconfirm persistence branch with production restart/replay showing exact event coverage and no losses |
| H3 Easy/selected/self-judged goals reassure falsely | Count missing/unresolved records; audit outcome source independently of score | Strong selection/provenance limitations, **causal claim untested**. Missing human-origin evidence is not evidence of unauthorized scoring. Disconfirm with population-preserving admission and blinded evidence reassessment across representative successes/failures, showing interpretation robust |
| H4 Useful scoring, inadequate feedback | Trace reader/decision consumers; test score versus calibration and baseline counterexamples | Actionable forecasting may survive; raw bands and feedback are demonstrably inadequate. Disconfirm with matched baseline/diagnostics and measured improvement on future held-out work, not merely lower historical loss |

T3 selection rule was declared before sampling: all current retained goals for census; latest three unique surviving score identities by score valid_from; latest three pending by original created_at. All sampled scored goals happened to be achieved; selection was chronological, not success-filtered. This is a survivor convenience sample, not population evidence of capability.

Observation windows: 2026-10-20 21:02:13 and 21:04:11 UTC; headline counts unchanged. Data accessed read-only in SQLCipher; key privately supplied over stdin, never printed. Raw results remain in delegate tool transcript, **not a complete hashed standalone export**. Counts below are delegated observations, not a new independent replication by this report.

| Distinct population | Reconciliation |
|---|---|
| Retained kanban goals | 61 = 52 unresolved + 9 achieved + 0 not achieved; 60 numeric p + 1 null |
| Unresolved latest verdicts | 52 = 38 done + 8 continue + 6 absent; done is not resolution |
| Surviving per-goal curator identities | 305; intersection with retained=55; union=311=61+305−55 |
| Surviving create / judge / score | 289/289 rows/IDs; 272/245; 208/200 |
| Unique scored outcomes | 200 = 194 achieved + 6 not achieved; numeric scores194, null6 |
| Numeric surviving intake-score pairs | 191 = 189 achieved + 2 not achieved; 200−6 missing creates−3 null intake predictions |
| Score repeats | 208−200=8 additional rows across eight IDs, no conflicting labels observed |
| Retained resolutions without surviving per-goal score | All 9; three judge-only memory, six no per-goal memory |
| Wider historical carrier | Bulk ledger211 entries, predictions reconstructed from logged Briers; identity discrepancies. Expanded literal-ID union511 is **not** 511 distinct real-world goals or all-time denominator |

Sample scored chains (create / judge / score h_mem IDs):
- Goal `2a768256-da00-40a2-a850-af330a586239`: `f369bb54-7b47-4f2e-9046-e9eaff86be04` / `ba12c360-9d2d-4ad5-af25-d6b4858a08b8` / `eed80333-b32d-4b09-a9c9-4686c0355740`; p=.85, achieved, BS=.0225.
- Goal `3333a3e4-175b-49ef-96e1-ed7d2fc865d3`: `d4e28ec5-8fbd-4f1a-8fa2-dbed707f0667` / `45259eff-d004-42e2-a9fc-83914e447aad` / `59ca2cfa-9050-46f8-973f-ceef2a4d2a08`; p=.8, achieved, BS=.04.
- Goal `db524319-a84a-4af3-b423-5566fd02b270`: `accfe5b8-9e20-483b-a0c1-22d43d4bda4c` / `e66cfea2-1744-41a8-8f63-5985ba3b3fb4` / `e43e7e69-f13a-4a58-972e-9593cf765224`; p=.55, achieved, BS=.2025.
Pending sample: `ad2d62e8-1ada-41c5-8f71-6da33a37208a` p=.87; `08948ee3-915d-4696-923f-6bae9a23cee0` p=.91; `8a56a82f-2aa1-4c54-92c1-50df43eee4c5` p=.8; all judged done, unresolved.

Surviving create timestamps in scored sample can follow judge timestamps because confidence updates replace create memories; they cannot prove original pre-outcome timing. Stored `tool_observed` means a tool result was observed, not that the human authenticated its label. Human source turns were not recovered. Six scored IDs lack surviving creates; nine retained resolutions lack score memory. Do not reconstruct missing p from a score, manufacture criteria or silently merge near-identical IDs.

## 4. Approved closure contract — P1–P6

### Lifecycle and ownership

1. **Human specification:** preserve explicit goal text, criteria and source turn, with immutable version/digest. Worker may suggest criteria; human approval is separately sourced. Progress evidence references that version. Changed requirements create a version/successor relation, never rewrite a scored proposition.
2. **Optional forecast declaration:** only where a decision warrants it; freeze proposition, p, convention, deadline/lead time, execution policy/resources, resolver/source, baseline and cohort/update weights before outcome visibility. A missing p is unscored. Eligibility is a deterministic check of declaration requirements, not a claim the forecast is useful.
3. **Human closure:** trusted user interaction records accomplished OR stop, separately achieved/not achieved/unknown, actor/source-turn ID and time. If an utterance is ambiguous, ask that functional question; do not ask the human to run score tools. A model-supplied actor string is not authentication. Closing work never fabricates an outcome.
4. **Curator orchestration:** enqueue assessment after closure; send original spec/criteria and evidence manifest to assessor before worker verdict, human label, forecast p or previous score. Record exact assessor inputs and identity/model/tool versions; later disclose labels and retain differences. Evidence selection can still be biased; assessor can request missing evidence. Shared model/context is not independent ground truth.
5. **Assessment:** record each criterion supported/contradicted/unknown, evidence locators/digests, source reliability, check command/oracle and limitations. Machine evidence, semantic findings, worker claim and human judgment remain distinct. Failed dispatch/assessment is pending/failed, never passed; retry does not reopen work automatically.
6. **Resolution and score:** after assessment attempt, deterministic resolver applies the approved predeclared source rule, or records pending/disputed/ineligible. Never change resolver to whichever party agrees. Keep append-only source corrections with superseding score versions, not silent outcome flips. No human arithmetic or score administration.
7. **Durability and recall:** atomically store transition and pending jobs/events in canonical goal storage; stable event ID plus payload digest, conflicting replay rejected. Resume assessment/delivery from persistent pending work after restart without a new model turn. Project complete provenance into curator memory; ack validates event/digest receipt and marks delivered, never deletes canonical history. Failed semantic indexing is visible separately from durable record storage. Partial success and sibling failures do not erase each other.

The canonical ledger must not be eligible for confidence-based forgetting. Memory is a rebuildable retrieval projection; an inaccurate forecast is valuable history, not less authentic history. Retention/privacy deletion remains a separate explicit user policy, not a score side effect. Delivery guarantees are at-least-once with idempotent effects, not unsupported exactly-once transport claims.

### Approved human decisions (P1–P6)

| Policy | Recommendation | Alternative / consequence if deferred |
|---|---|---|
| P1 Which outcome resolves a forecast? | Per-declaration resolver: human-acceptance forecast uses authenticated human label; machine-condition forecast uses declared machine evidence; independently assessed attainment requires that explicitly declared source. Default ordinary goal has no forecast | One global resolver is simpler but conflates different propositions; no eligible scoring until decided |
| P2 Disagreement in both directions | Preserve both. Human yes/assessor no: close as human-accepted with contradiction; reverse: close as human-not-accepted with supporting evidence. Score only the declared target; disputed evidence-based target stays pending | Consensus requirement would make score availability depend on agreement and pressure convergence; no silent consensus default |
| P3 Unknown/partial/stop/abandonment | Unknown unscored. Per-criterion partial evidence retained, never convert a fraction into binary y. Stop is closure, not failure unless frozen proposition/deadline makes it a resolved negative. Abandoned remains visible | Automatically treating every stop as failure changes the forecast target; excluding every stopped case conceals selection. Choose explicit rules before admission |
| P4 Changed goals and corrections | Version/supersede; original forecast evaluated only under original rule or explicitly voided with reason. Outcome corrections append provenance and trigger deterministic re-scoring | Rewriting original criteria destroys auditability. Historical incomplete cases remain provenance-limited, not repaired by guessing |
| P5 Metric policy and authority | Verification-primary plus optional eligible forecasts; retire raw universal bands, synthetic scoring priors, one-score memory confidence and unsupported adjustments. New automatic outcome/score records cannot be authored as human receipts by worker tools | Retaining mandatory task forecasts preserves weak semantics; no replacement feedback policy deployed until approval |
| P6 Source sufficiency ruling | Accept primary Murphy/GJP plus explicitly limited Brier OCR for this recommendation; treat local Tetlock copy as corroboration only, not edition-verified authority | If authenticated book/scan collation is required before acceptance, authorize that bounded acquisition. This gap does not change present recommendation |

## 5. Structure, deletion test and metric use

Both `deep-module` and `refactor-architecture` were actually invoked and applied. Mechanical production inventory (split before cfg(test)): goals.rs six methods/297 nonblank non-comment lines, types/goal.rs six types plus two methods/89 lines; combined14 items/386 lines. The skill's 27.57 lines/item is a diagnostic, not evidence of poor function. One assessment only: no three-iteration convergence claim.

**Deletion test both directions:** inline existing goal logic and ownership, identity, criterion coverage, atomic transition and replay/conflict handling reappear in callers; replace it with raw storage/math calls and those protections disappear. Therefore retain/deepen the existing goal boundary, not another wrapper. Deleting score-confidence policy removes real behavior: retirement is a policy recommendation, not a false dead-code finding. A universal scoring wrapper adds no invariant beyond existing math and would wrongly unite divergent outcome policies; do not create it.

Home: existing `kanban/types/goal.rs` and `kanban/service_impl/goals.rs` hold durable transitions and provenance. Existing curator/bridge orchestrates inference; goal service must not depend on inference or other servers. Conceptual operations: declare, get, list, record assessment/progress, record human closure, enumerate pending work, acknowledge receipt. The approved public shape is now pinned in `GOAL_INTERFACE.md`; avoid inventing config/traits to hit a numerical style target. Shared `hkask-forecast` remains pure math; no universal resolution engine across domains.

Consumer map includes defining crate, tests, panels, inline calls and prompt files: MCP goal wire types/endpoints; task CriterionCitation in `kanban/types/task.rs:29`, `task_spec.rs:19`, `service_impl/service.rs:384–413`; `crates/agent/src/agent.rs:3144–3155`, `thread.rs`, `kask_thread_state.rs:62,138–145`; `hkask-types/src/ports/memory_port.rs:50–96,144–154`; bridge memory and ingestion; `crates/kanban_panel/src/kanban_panel.rs:364–384`; experiment linked_goal_id in `server.rs:144,273`, `store.rs:360–387`. Linked experiment and goal must not become duplicate independent forecasts.

| Alternative | Benefit | Defect left / verdict |
|---|---|---|
| Repair existing universal scoring | Better persistence, few workflow changes | Still forces task outcomes into underspecified binary events; insufficient alone |
| Restrict scoring to actionable prospective forecasts | Preserves useful probability feedback and automatic administration | Requires explicit admission/cohort/resolver policy; recommended alongside verification |
| Verification/grounding replaces routine task scoring | Completion evidence directly answers criteria; honest partial/unknown/disagreement | Does not itself measure probability calibration; recommended default, not anti-Brier replacement everywhere |

### Retained measurement contracts

No metric is a general certificate of truth or competence. Every retained metric has a consumer and decision:

| Metric | Prediction / outcome and eligible population | Baseline / limits | Reader, changed decision and feedback |
|---|---|---|---|
| Criterion evidence status (not probabilistic score) | Original requirement; cited checks and human/assessor labels separately; all admitted goals | Original acceptance criteria; unknown is not pass; check only verifies stated obligation | Human decides accept/rework/stop; curator surfaces gaps, worker improves evidence |
| Eligible Brier and reference-relative skill | Frozen probability; declared resolver; eligible same-domain/horizon/policy cohort | Matching prospective reference; undefined skill if reference loss0; dependence and selective resolution disclosed | Forecaster/curator chooses forecast method only after prospective comparison |
| Calibration/reliability and resolution diagnostics | Frozen p versus outcome frequencies, event identities and lead times | Display counts/uncertainty and grouping; no data insufficient, not zero error; no universal thresholds | Forecaster checks bias/discrimination and tests a declared correction on later held-out events |
| Closure/assessment disagreement and unresolved fractions | No probability prediction; all admitted goals, per closure/source status | Prior version/same admission policy; disagreement is a diagnostic, not a target to minimize | Human inspects material differences; curator adjusts evidence collection, never pressures agreement |
| Delivery/assessment backlog and age | Persisted event/job versus receipt; all pending jobs | Expected delivery contract; no success from empty scan without known positive control | Curator retries/escalates failure, operator sees exception only, not scoring administration |

Retire metrics with no defensible decision use: routine single-goal Brier as memory truth/confidence, universal quality labels, fabricated historical performance, raw score as market reliability. Retain market snapshots/liquidity, empirical bins and forecast histories; replacement adjustment/weighting must earn prospective evidence, not inherit old names.

**Learning test:** preregister a representative admitted population and matched reference; compare forecasting/evidence practice on later held-out event clusters and comparable lead times. Look for better reliability with uncertainty reported, preserved/improved resolution, reference-relative performance and honest unknown detection without a changed easy-case mix or increased unresolved exclusions. Also test whether users distinguish low base-rate loss from calibration and identify contradictions in evidence. A lower score, fewer disagreements or passing tests alone does not demonstrate learning. No benefit experiment was run here.

## 6. Dependency-ordered implementation and checks

P1–P6 and the implementation sequence below are approved. The sequence remains the acceptance contract, not evidence that every deployment check has run. No compatibility shims: update every affected caller and delete old policy in the same cutover; preserve unrelated work and keep the tree buildable. Staged internal changes may prepare a slice but must not ship competing public lifecycle APIs.

1. **Policy/specification lock — human + program-manager.** Record P1–P6 decisions, falsifiable acceptance criteria and immutable source turns. Fix terms in ontology/instructions against primary references. Exit: disagreement/unknown/stop table agreed; no agent invents outcome policy. Deferred risk: ambiguous resolver makes scores uninterpretable.
2. **Canonical lifecycle and authority — deep-module + refactor-architecture; tdd at implementation time.** Deepen existing goal transition, versioned records and trusted human closure ingress. Preserve citations, owner boundaries and atomicity. Replace `goal_score`/prune-on-ack coupling (`goals.rs:292–371,405–411`) and generic GoalResolution. Exit: authorized closure and separate findings replay without overwrite; malformed or forged human receipts fail.
3. **Durable orchestration/projection — program-manager + tdd.** Persist assessment/outbox state atomically; blinded dispatch, stable event IDs/digests, receipt validation, restart drainer and exact/semantic recall. Replace response-only buffer and score-specific autoack (`thread.rs:307–363`, `kask_thread_state.rs:62,138–145`, bridge `memory/ingest.rs:321–425`). Delete confidence mutation and judge purge (`ingest.rs:163–300`) and obsolete calibration receipt accommodation (`hkask-memory/src/memory_store.rs:145,1632`). Exit: production closure-to-recall after restart, fault-injected recovery, no original criteria lost.
4. **Forecast admission/interpretation — superforecasting for practice, pure math for computation, falsifiability for competing explanations.** Make goal forecasts optional; freeze scenario/equity/market/experiment identities at declaration. Deduplicate resolution by declared event/version; reject late declarations as eligible. Delete universal bands/tests, synthetic `.7`/`.25` scoring, ungrounded weighting/demotion/correction; retain distinct domain stores/eligibility. Exit: fixtures and matched reference output correct; no claim of calibration from raw score or no evidence.
5. **All-consumer cutover/deletion — refactor-architecture.** Update MCP schemas and registration/ontology mappings; task citations, ports, panel/widgets, experiment links, prompts, settings explanatory copy and docs. No goal-specific setting was proven removable: do not delete shared retention controls. Replace obsolete tests of pruning/confidence with retained obligations. Census all symbols and docs before deletion verdicts.
6. **Evidence acceptance and deployment — grounding-verify + program-manager.** Independent source review, real workflow, fault controls, denominator reconciliation, downstream build/lints. Human judges functional result, not arithmetic. Report exact commands, pass/fail/unrun checks, removals/net lines and commit hash. No green capability claim from fixtures alone.

Deletion/caller checklist: system prompt `crates/agent/src/templates/system_prompt.hbs:337–354` and pins `templates.rs:674–695`; panel guidance; `.agents/skills/{program-manager,product-manager,kanban-task-management,metacognition,eqm,scenario-planning,self-improvement,hypothesis-testing,superforecasting}/SKILL.md`; metacognition and experimentation templates; `kask/registry/templates/superforecasting/README.md`; settings copy `crates/settings_ui/src/pages/kask_page/memory.rs:338,368`; kata-kanban README/reference; memory-system specification; functional-interaction-spec; verification-ladder; loop-register; DIVERGENCE D6/D40/D58 and affected widget seams. Preserve historical audit documents as historical. Old test obligations for atomicity, ownership, sibling isolation and recall remain; only obsolete policy assertions disappear.

### Acceptance matrix with positive and negative controls

| Obligation | Positive control | Negative/boundary control |
|---|---|---|
| Original human authority | Source-backed human closure, human yes/assessor no retained; reverse disagreement retained | Worker claiming actor=human cannot authenticate; assessor disagreement cannot flip human acceptance |
| Closure distinct from success | Stop+not achieved and stop+unknown close work visibly | Neither silently becomes success or binary failure; unknown evidence does not pass |
| Partial/changed goals | Criterion-level partial findings; successor specification linked | No fraction as binary outcome; old forecast not scored against revised criteria |
| Assessor separation | Captured dispatch includes spec and evidence, excludes p/worker verdict/human label initially | Hidden-context leak fixture detected; unavailable assessor leaves failed/pending assessment, not green |
| Score arithmetic/conventions | p=.8 true=.04, false=.64; .5=.25; vector two-category=.08 for .8 true; perfect0, maximally wrong1 binary/2 vector | Out-of-range/nonfinite p, malformed categories, duplicate event IDs rejected; missing p unscored, never0 or.7 |
| Accuracy is not calibration | Same BS=.25 with different REL/RES; declared reference scored on same cases | .55 true=.2025 must not be described worse than .5=.25; empty bins unknown; signed cancellation not calibrated |
| Reference comparison | Matched baseline/weights/time yields auditable BS and skill | Reference loss0 undefined skill; unlike populations/horizons not silently pooled |
| Denominators | All admitted partition into open, closed-pending, eligible-resolved, explicitly unscored/void; event/version/job counts distinct | Dropped failed/abandoned goal or duplicate score increases denominator mismatch and fails reconciliation |
| Retry/atomicity | Same ID/digest converges; crash after commit/before delivery and after delivery/before ack recover | Different payload same ID conflicts; bad receipt cannot delete canonical record; sibling failure remains isolated |
| Durable real workflow | Human closes disposable integration goal → blinded assessor → eligible resolver/score → projection receipt → restart → full original criteria/evidence exact+semantic recall | Missing embedding/index/receipt surfaced and retryable; restart without model turn drains backlog; no hand-created memory substitute |
| Instrument validity | Known violation fixture trips every admission/grounding/denominator check; known legitimate case passes | Vacuous zero-test run fails; deliberately altered source quote and missing criterion are caught |

TDD is routed to later implementation, not ceremonially loaded in this investigation. Passing any test establishes its named obligation, never metric usefulness. Run tests through `bash kask/scripts/cargo-test-nonzero.sh ...`; scoped `./script/clippy --deny warnings`, `cargo check -p zed` for public API cutover; revalidate actual CLI arguments/package/test names before running. No full-tree formatting or unrelated cleanup.

### Historical cutover policy

Preserve old records as provenance-limited history with explicit eligibility reasons. Do not retroactively mark legacy predictions prospective, recreate lost human confirmations, or use reconstructed probabilities as independent intake evidence. Identity corrections require source evidence and an audit trail. Operator may choose archival-only legacy scores. No backward API compatibility is required; evidence preservation is not a compatibility shim.

## Historical research checkpoint, receipts and limitations

Initial evidence passes T1/T2/T3 completed; one targeted T3 reconciliation follow-up, one T5 consumer/interpretation follow-up. Stop: evidence discriminates numeric correctness from lifecycle and interpretation defects sufficiently for the recommendation. No further research needed to choose architecture. Remaining empirical Goodhart/learning questions cannot be settled from surviving convenience records; propose a separately authorized prospective study only if needed for a later feedback policy.

Executed inspection commands included whole-repository `rg -n/-l --hidden`, targeted `nl -ba ... | sed -n ...`, file tools, `git rev-parse HEAD`, `git --no-optional-locks status --short`, and `git --no-pager diff --cached --stat`. Parent checkpoint returned HEAD `ecb7e42f233857d01cee82581971cf75047b803a` and empty status/index output. SQLCipher used `-readonly`, attached kanban with `mode=ro`, `PRAGMA query_only=ON`, SELECT transactions; no outcome mutation. Tables were discovered from sqlite_master, not assumed. Reproduction shapes: retained `kb.hmems WHERE entity='kanban:goal'`; events `hmems WHERE entity LIKE 'curator:goal:%' GROUP BY attribute`; normalize value via `coalesce(json_extract(value,'$.content'),value)`; join exact IDs; sample ORDER BY timestamps LIMIT3. Key management details and values deliberately omitted.

DB locations: `/home/mdz-axolotl/.local/share/zed-kask/agents/curator/curator.db` and `/home/mdz-axolotl/.local/share/zed-kask/mcp/kata-kanban/kanban.db`. The counts describe a time-bounded live snapshot, not an immutable exported dataset. The incomplete temporary output file is not cited as a complete receipt. Parent audit did not independently rerun the full SQL census. Delegate sessions: T1 `f6f5dac4-74bf-4bb8-8b92-4698f60e46a6`; T2 `e61e2df4-8eb2-430b-834d-3764d1aaf37d`; T3 `9740f036-48ad-4a6e-892a-cdea2ec542f3`; T5 `481c0237-2430-491f-832d-ce8dfe072bec`.

Deterministic Lisp checks in the inquiry corroborated convention arithmetic, Murphy's example, equal-score/different-calibration construction, metadata/count partitions and raw-band counterexamples. They are mathematical checks, not production closure runs. Existing Rust tests were inspected, **not run**. No build, clippy, production closure mutation, restart test or prospective usefulness experiment was run. These remain implementation acceptance work, not silently passed checks.

Evidence gaps requiring disclosure/ruling: Brier scan collation; authenticated Tetlock edition; G14 main text full access (supplement and G15 available); complete all-time ledger/human-origin history; representative prospective agent-task validity/learning. Primary Murphy/GJP are enough for current statistical separation. Do not expand research solely to close incidental bibliography gaps. Seek authorization only if a missing answer could change a policy recommendation.

Outcome: decision-ready investigation, not a functioning redesigned capability. No production code removed or added; no commits made. This document is the only intended repository change, uncommitted. Implementation removal/net-line accounting is deferred until an approved cutover. Learning: valid arithmetic can coexist with lost evidence and unjustified feedback; preserve outcome provenance and decision purpose before optimizing the score.

### Independent review and final mathematical receipts

Separate audit session `9da74f8a-7888-4fec-a483-857a93e53c45` applied grounding-verify to this draft and available sources. Its materiality scope was 22 load-bearing groups plus four quotation assertions, not every sentence. It found one medium error (High-to-Medium market demotion was incorrectly claimed to affect a Low-only admission gate) and two low quotation-boundary errors. All three were corrected; a correction-only recheck closed those findings. This does not certify the whole report. The outer reviewer was separately dispatched but shared model/tool environment and supplied evidence; it did not independently rerun the SQL census. Its initial local `in_thread` label did not describe the outer dispatch. Original audit findings remain part of the record rather than being erased by correction.

Parent `lisp_eval` receipt: retained partition61=52+9+0 true; union311=61+305−55 true; paired191=200−6−3 true; excess8=208−200 true. Deliberately adding one retained row made the denominator control false. For p=.8, binary true=.03999999999999998, false=.6400000000000001, vector true=.07999999999999996; Murphy example=.286. On a constructed 1% event population, reference p=.01 gives .0099 while p=.20 gives .04600000000000001: the latter would receive the current “excellent” label despite worse reference-relative performance. These calculations use cited inputs, not live score mutations or production Rust tests.

Concrete same-population calibration control for later tests: outcomes `[0,0,0,1,0,1,1,1]`. Forecast A=.5 throughout: BS=.25, REL0, RES0, UNC.25. Forecast B=`[0,0,0,0,1,1,1,1]`: BS=.25, REL.0625, RES.0625, UNC.25, signed bias0. This refutes raw score or zero signed bias as a sufficient calibration diagnostic without changing the outcome population. The reviewer checked this construction mathematically; it is an acceptance-fixture design, not an executed production test.

## Historical first-cutover implementation checkpoint — 2026-10-20

This checkpoint and its receipts describe the first cutover, not the latest
implementation. Its reference-only assessor limit, date-only scenario admission
and remaining wire-type note are superseded by the appended bounded-artifact/
precise-cutoff follow-up. Original observations and run receipts remain unchanged.


Operator approval covers all P1–P6; no pending functional-policy vote remains.
The canonical implemented contract is
`kask/mcp-servers/hkask-mcp-kata-kanban/GOAL_INTERFACE.md`. Production code is
recorded in `f21ab8d61b` and subsequent shared-tree host changes; the prompt/skill
slice was captured by the concurrent owner in `05a9ffbe73` while this docs pass
continued. This agent made no commit; remaining reference/docs and cleanup edits
are uncommitted until the owner records their receipt. Historical sections above
retain their original evidence limitations and line citations as audit history,
not current operational instructions.

- MCP create/judge/get/list require source-backed immutable specification and
  return complete goals with `id`, stable criterion IDs and digest. Routine work
  omits forecast; eligible optional declarations freeze target/timing/resolver/
  policy/resources/baseline/cohort. Human closure and source corrections are
  separate native ActionChoice interactions, never worker-authored outcomes.
- The host stores assessment/projection work durably, supplies fresh blinded
  spec/evidence requests, validates projection receipts and retains canonical
  history. Human labels, worker progress, machine evidence and findings remain
  separate. Current assessor requests have **no evidence-fetch capability**;
  unknown is required, not support/contradiction based on a locator alone.
  Scheduling follows `curator.always_on`; disabled or failed work stays pending.
- Forecast guidance removes universal raw-loss bands, synthetic scoring priors,
  inverse-loss reliability weights and automatic correction. Scenario records
  require prospective `scenario_declare` and matching frozen resolver/events for
  `scenario_score`; legacy undeclared records stay unscored. Market tiers remain
  liquidity/spread-only. Murphy/GJP provide interpretation with P6's source limits.
- Owned skills, templates, system prompt/pins, functional/memory/type maps,
  forecasting references, diagrams and D6/D18/D40/D58 divergence entries now
  describe this separation. This replaces obsolete score/prune instructions,
  tests and comments; it does not alter unrelated code or `.rules`.

### Historical first docs/prompt pass: checks and owner follow-up

`bash kask/scripts/cargo-test-nonzero.sh -p agent --lib templates::tests::test_goal_ -- --nocapture`
ran 2 tests, both passed (nonzero wrapper confirmed coverage).
`bash kask/scripts/check-skill-crossrefs.sh` passed.
`bash kask/scripts/audit/skill-corpus-prescreen.sh` checked 222 templates and
59 skills: no D/P or initial/target flags; one advisory LOW OVERLAP flag on the
unchanged `metacognition/falstaffian-rotation.j2` (22%). Owner: skill-maintenance
review; retained as a visible warning, not hidden by changing the acceptance ledger.
Further final prompt-suite/sweep receipts are recorded below by this pass or may
be appended by the owner; no broad build or editor deployment is implied.

The live ontology gloss still overclaims raw Brier loss as calibration. Owner:
ontology implementation; routed to `skill_use_issue:onto-anchor` via the issue tool
(semantic_definition, tool_implementation). Consumer guidance now qualifies it;
this out-of-scope implementation warning is not claimed fixed. End-to-end editor
closure, actual independently fetched evidence, deployment/restart acceptance and
any prospective benefit study remain owner acceptance work, not a fabricated human
outcome or task-completion claim. Learning: freshness of assessor context does not
supply evidence access, and arithmetic correctness does not establish calibration.

### Historical first docs/prompt verification receipt

- Wrapped command: `bash kask/scripts/cargo-test-nonzero.sh -p agent --lib templates::tests -- --nocapture` — **31 passed**, 0 failed; wrapper confirmed 31 executed. The corpus pin walks live skills/templates rather than a handpicked file list. No broad build was run for this docs/prompt scope.
- `bash kask/scripts/check-skill-crossrefs.sh` — passed. `bash kask/scripts/audit/skill-corpus-prescreen.sh` — 222 templates / 59 skills; only the unchanged 22% overlap advisory described above. `git --no-pager diff --check` — passed.
- Live renders of `metacognition/meta-predict` and `superforecasting/stage_7_record` preserve the measured input, optional/null routine confidence and prospective-declaration/resolver instructions. These are prompt renders with fixture inputs, not live forecast or human-outcome records.
- Full exact-symbol sweep: 285 live skill/template/system-prompt files have zero retired score-route, receipt-ack route, interpretation-helper or calibration-note tokens. The 80 owned doc/map files retain one occurrence of each retired goal route on the same explicitly historical L9 audit line; no helper or note-field occurrences remain there. Whole-repository residuals additionally comprise negative/provenance fixtures, the out-of-scope historical `kask/docs-alignment-run.md`, and unchanged root `.rules:73`.
- Warning routing receipt: parent task comment `21b8455a-0ee8-43f2-a3f8-042a23b56d94` assigns the prescreen advisory to skill-maintenance review, ontology gloss to its implementation owner, and the contradictory root rule to the principal/host for a dedicated rules-only disposition. None was suppressed or silently called fixed.
- Shared-tree checkpoint: the owner captured earlier slices in `05a9ffbe73` and `0b40eca775`. This agent issued no commit, stash or reset. Later reference-map/wording/receipt edits remain uncommitted for the owner. The type map explicitly marks the still-exported `GoalEvent` as historical-only with no host-ingestion consumer; removing that out-of-scope wire type remains the code owner's work, not a deletion claimed by this docs pass.

## Latest approved implementation follow-up — bounded sources and precise cutoffs

P1–P6 remain approved. This is a new source-inspection/update receipt, not a
rewrite of the original inquiry's findings or a claim of editor redeployment.
Authority: current `kask/mcp-servers/hkask-mcp-kata-kanban/GOAL_INTERFACE.md`,
`kask/crates/kask_bridge/src/goals.rs`, `goals/artifacts.rs`, native
`crates/agent/src/tools/goal_tools.rs`, scenarios `requests.rs`,
`hkask_mcp_scenarios.rs` and `superforecast/store.rs`. The latest request explicitly
supersedes the earlier no-fetch and date-only limitations. Integration acceptance
belongs to the parent; this docs pass does not manufacture human outcomes.

### Bounded independent artifact observations and consent

Native closure discloses exact paths/hashes, configured assessor sharing including
scheduled retries, retention in canonical attempts/curator recall and sharing
retained projection contents with the configured semantic-indexing provider.
Approval permits only absolute canonical local paths/local file URIs and nofollow
UTF-8 regular-file reads: at most 16 artifacts, 64 KiB content each, 256 KiB total
reads per attempt including growth probes. No crawling, network retrieval,
symlink traversal or commands. Historical absent approval stays absent; corrections
grant no new access. These are host consent fields, never worker inputs.

The host freezes exact fetched contents or unavailable/mismatch read receipts
with the spec/manifest and input digest before inference. With no fetched contents
it records deterministic unknown findings without a model call. Otherwise supported/
contradicted findings require actual fetched, digest-matched evidence IDs; missing
or failed evidence stays unknown. A fresh blinded request excludes human/worker
labels and forecast probabilities/scores, while keeping untrusted contents as data.
This verifies observed bytes, not producer authenticity or that alleged tests ran.

### Narrow machine forecast source, not an arbitrary outcome writer

Supported resolvers are HumanAcceptance and exactly
`MachineArtifactDigest { locator, expected_sha256 }`. The native prompt separately
discloses the machine locator/hash/window even with an unrelated/empty assessment
manifest. `MachineArtifactDigestApproval` binds the complete frozen forecast/spec;
manifest approval alone is insufficient. The first actual scheduled bounded read
in inclusive `[horizon, resolution_deadline]` derives true for SHA-256 equality,
false for a successful different digest. Missing/denied/unsafe/non-UTF-8/oversized/
late reads produce unknown, unscored; the first receipt including unknown is
immutable. No pre-horizon assessment snapshot, worker label or model finding can
supply this observation. Resolver-only contents are discarded, not sent to assessor/
indexer; retain only typed source/time/digest/count receipts. Equality means file
bytes match, never that tests ran. Scoring still waits for an assessment attempt.
Arbitrary old MachineEvidence forecasts reconcile to UnsupportedResolver with
historical observations/scores preserved, not newly eligible or silently erased.

### Prospective scenario identity and descriptive metrics

`scenario_declare` now requires `resolution_at`, exactly every event ID mapped to
an RFC 3339 UTC timestamp; no missing/extra keys. Cutoff UTC dates match descriptive
deadlines. Admission is strictly `declared_at < cutoff`; scoring is `now >= cutoff`.
The complete manifest is one synced journal batch before any member is admitted.
Identical same-ID retries preserve original timestamps/probabilities even after
cutoff; changed membership/proposition/probability/cutoff/resolver conflicts. Old
date-only/undeclared records stay unscored history. A named resolver remains a
caller-reported source rule, not authenticated evidence.

The current diagnostic is `signed_probability_error` (mean p−outcome over
qualifying bins), with `signed_probability_error_sample_count`,
`signed_probability_error_qualifying_bins` and
`signed_probability_error_bin_min_count`. Null/insufficient is not zero. Signed
cancellation and event complementation prevent interpreting it as confidence or
calibration certification. The widget map now carries those actual fields and
`evidence_status`, not the retired confidence/interpretation fields.

### Native authorization and source-map reconciliation

D58 records `crates/agent_ui/src/conversation_view.rs`: generic authorization and
granularity shortcuts require PermissionGrant; ActionChoice needs a concrete
offered ID. Source pins are `test_action_choice_requires_an_explicit_offered_option`
and `test_permission_shortcuts_preserve_permission_grants` (inspected here, not run
by this docs pass). Native write-profile entries in `assets/settings/default.json`
and permission-setup exclusions in `crates/settings_ui/src/pages/tool_permissions_setup.rs`
are mapped without editing those files. D6/type maps reflect the parent's removal
of the obsolete goal-event type/re-export. Other streams' existing modifications,
including review-skill/template and unrelated divergence edits, are preserved.

This follow-up replaces stale live limitation/admission/metric copy and extends
prompt pins only; no production code, settings, `.rules`, or scripts are edited.
Validation and exact residual counts follow below; no running editor redeployment,
real user acceptance, actual test execution by an artifact producer, or prospective
benefit/calibration experiment is claimed.
