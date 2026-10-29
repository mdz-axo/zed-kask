---
title: "Instrument-affordance experiment: compliance baseline, harness bring-up record, and the production sensor/injection specs"
audience: [architects, developers, operators]
last_updated: 2026-10-28
version: "1.0.1"
status: "Active"
domain: "Agent instrumentation"
mds_categories: [domain, trust, lifecycle]
---

# Instrument-affordance experiment: compliance baseline, harness bring-up record, and the production sensor/injection specs

Date: 2026-10-23. Status: experiment resolved (rejected at the declared bar; mechanism-consistent
fossil retained). Registry: experiment `exp_ecb2a2494a8d44e0947630ddcbb9a9aa`, seal
`seal_8385cfcc204147fc9a7642f7587e99a9` (fingerprint `6196fdac…`), selection
`sel_8b81beed03a64b9d83498ae9a22d6002`.

## 1. Background and diagnosis

The operator's standing problem: LLM agents running the curator role in zed-kask default to
native generation and under-use the kask capability layers — `lisp_eval`, `render_template`,
`onto_anchor`, the 9 built-in MCP servers (258 registered tools), and the skill library —
despite extensive standing instructions in the system prompt.

The diagnosis established in this session (research run `85d5b8b2850e3d4c`, sources
server-recorded): the failure is a **channel-authority** problem, not a wording problem.
Five stacked mechanisms:

1. **Advisory prompts vs a frozen trained prior.** Post-training optimizes direct answer
   production; tool use was reinforced only in harnesses where the call was *necessary* for
   reward. The trained policy's tool-call threshold is poorly calibrated in both directions
   (SMART, Qian et al., Findings of ACL 2025; the opposite failure — tool overuse — dominates
   chat-shaped harnesses). Harness design decides which way the error tips.
2. **Attention economics.** The system prompt is one static, byte-stable render
   (`crates/agent/src/thread.rs:5888`) carrying ~57 skills, ~50 rules and the tool list;
   positional attention is U-shaped (Liu et al., *Lost in the Middle*, TACL 2024) and the
   instrument-use requirements sit in the sagging middle.
3. **Discovery cost asymmetry.** Native generation has zero recall cost; instrument use is a
   recall→match→fetch→follow→parse chain over 258 tools, each arrow a dropout.
4. **Reinforcement asymmetry.** Skipping instruments is invisible and consequence-free within
   a turn; *using* them is sometimes punished by confusing failures (curator memory holds the
   specimens: the `render_template`/`prompt-enhance` cascade ending in hand-execution).
5. **The unsensed leg.** Nothing measures "substantive turn completed with zero kask-instrument
   calls" — the feedback loop is broken on exactly that leg (sense → compare → act → re-sense;
   "broken when any leg is missing, delayed beyond usefulness, or silent", Ashby 1956).

The tree already proves the fix class: everywhere the harness refuses (corpus QA gate,
`annotate_research_run` verified-only-if-server-recorded, `finish_research_run` requiring
recorded sources, human ActionChoice at goal closure), compliance is structural; everywhere
the prompt asks, drift governs. The two highest-leverage interventions short of fine-tuning:

- **D — the native-first-rate sensor**: make the skip visible to Regulation.
- **B — per-turn instrument-affordance injection**: convert instrument discovery from recall
  into perception, over the existing per-turn injection channel
  (`crates/agent/src/thread.rs:5910` — "Memory recall is per-turn via `inject_context`").

This document records the first controlled measurement of B's mechanism (a static proxy), the
harness bring-up work it required, and the production specs for B and D.

## 2. Harness bring-up record (the seams found and fixed, 2026-10-23)

This section stands as the bring-up record: the measured findings and the
delegation-grant seam (item 2) survive on the worktree path; the
executor-side mechanisms it names went with the server.

Local swarm agents could not use MCP tools at session start. Three seams, each found by
evidence and fixed on the designed mechanism:

1. **Qualified names required.** Card `mcp_tools` entries must be `server/tool` qualified
   (the swarm server's `agent_executor.rs`, removed 2026-10-28); bare names were rejected
   before inference (tested: `invalid_tool_name_is_rejected_before_inference`). The rejection
   is silent to the caller: the first pilot delegation ran with zero tools and the model
   reported "tools not connected or accessible in this execution environment" — an
   observability gap that reads as a capability gap (filed, §6).
2. **Parent grant required.** Tool advertising was denied at the zed-side IPC boundary unless
   BOTH the card allowlist and the parent delegation grant permitted the qualified name —
   the grant machinery survives on the worktree path
   (`kask/crates/kask_bridge/src/delegation_grants.rs`: grants minted from
   `kask.mcp.delegated_tools`, narrowing checked at dispatch), while the swarm-children
   advertising gate went with the server. The swarm server additionally and
   unconditionally received the host skill tools (`host/skill`, `host/render_template`,
   `host/lisp_eval`, `host/onto_anchor` — "Local swarm agents get the Curator's skill access
   by default"); that unconditional host-tool grant is gone with it. The `delegated_tools`
   entry for the swarm server did not exist.
3. **Round budget.** `MAX_TOOL_ROUNDS = 4` (the removed swarm executor's round budget) bounded each delegation;
   parallel tool calls in one assistant message count as one round. Full instrumented flows
   must batch or the delegation errors ("exhausted 4 tool-call rounds without a final
   answer"). Both experiment arms carry an explicit round-budget line in their prompts.

Configuration change made this session (disclosed; reversible by removing the key):
`~/.config/zed-kask/settings.json` →

```json
"kask": { "mcp": { "delegated_tools": { "swarm": [
  "research/begin_research_run", "research/web_search",
  "research/annotate_research_run", "research/finish_research_run",
  "curator/curator_federated_search", "curator/curator_memory_recall",
  "corpus/corpus_query", "prediction-markets/market_ontology_map",
  "scenarios/scenario_quantify"
] } } }
```

Host tools rode a separate path: declaring any skill on a card auto-added the host quartet to
the executor's allowlist (the removed swarm executor); `host/skill` was further limited to the
card's declared skill names. Host tools were granted without a `delegated_tools` entry
(the removed `host_skill_tools.rs`).

Defects observed during bring-up (filed to the audit board, §6):

- Intermittent swarm delegation memory-tagging failure: `invalid passage-tagging response:
  unknown correlation ID 'item-0'` (surfaced as a non-fatal degradation; did not recur on
  later delegations).
- Card create/reconfigure silently drops `host/*` entries from declared `mcp_tools`
  (entries were declared and not stored; the caller was never told).
- Unqualified declared names are dropped silently (seam 1 above): the delegation result
  carries `tool_calls: []`, indistinguishable from "agent chose not to call".

## 3. The experiment

**Design.** Two arms, identical standing-discipline prompts (7 rules mirroring the editor
agent's condition: rules present, affordances absent) plus a round-budget line, identical
tool allowlists (9 MCP tools via the parent grant; host quartet via the `superforecasting`
skill declaration), identical model (the user-configured swarm default,
`openrouter/sakana/fugu-max`). The challenger differs by one manipulation: a recency-position
affordance block — "THIS TASK'S INSTRUMENTS" — naming the exact advertised tool names per
task class. This is the static proxy for the production per-turn injection.

**Task set** (artifact-instructed, both arms identical text; response-predicate evaluators):

| id | instrument class | evaluator (regex) |
|---|---|---|
| t1_research_run | research ledger | `[0-9a-f]{16}` (run id) |
| t2_memory_provenance | curator memory | `skill_use_issue\|<uuid>` |
| t3_corpus_citation | sealed corpus | `corpus:\|entity_ref` |
| t4_ontology_vocabulary | published ontology anchoring | `qb:\|dcmitype:\|bibo:\|pko:\|dc:` |
| t5_deterministic_verification | deterministic computation | `lisp_eval` |

**Results** (10 rollouts per arm, 2 repeats × 5 tasks, chunked `swarm_eval_agent_local`
calls; selection set 5 fresh rollouts for the challenger):

| task | baseline | challenger |
|---|---|---|
| t1 research run | 2/2 | 2/2 |
| t2 memory | 2/2 | 2/2 |
| t3 corpus | 0/2 | 1/2 |
| t4 ontology | 2/2 | 2/2 |
| t5 lisp_eval | 0/2 (2 errors) | 1/2 |
| **aggregate** | **0.60** | **0.80** |

Selection set (challenger, fresh disjoint run): 4/5 = 0.80 (t2 miss is single-rollout
variance; 2/2 in the arm measurement).

**Verdict: rejected at the declared bar** (`sel_8b81beed…`). The +20pp aggregate delta equals
the sealed 20pp noise band exactly — a tie; the pre-registered prediction (exceeds by *more
than* the band, confidence 0.6) is not upheld at n=10 per arm. The per-task pattern is
mechanism-consistent: all movement is on the two floor tasks (t3 0.0→0.5, t5 0.0→0.5 — the
instruments the baseline could not dispatch or ground), ceiling tasks unchanged. The t5
mechanism is direct: the baseline prompt names the tool unqualified (`lisp_eval`) while the
advertised name is `host/lisp_eval`; calls fall outside the declared allowlist and rounds
exhaust. The affordance block naming the exact handle is the fix and the thesis in one
specimen.

Disclosure: the challenger's affordance block was refined to exact advertised names
(`host/lisp_eval`, `host/onto_anchor`, `host/skill`) after the baseline measurement
identified the unqualified-name miss, before challenger registration and spend; the eval
contract was already sealed and unmodified. This is variation design informed by feedback
(the GEPA house pattern), recorded in the challenger's fitness record.

**Registry identifiers.** Experiment `exp_ecb2a2494a8d44e0947630ddcbb9a9aa`; baseline
`var_432694aba2684e3e80989e434c25d10d` (fitness `fit_dda1dfa4…`); challenger
`var_d3ece96044ca413bb2771a6419f6e181` (fitness `fit_6e0da3fd…`); final evaluation
`fev_9c220ef9…`; selection `sel_8b81beed…`. Arm cards:
`affordance_baseline_v1`, `affordance_challenger_v1` under
`~/.local/share/zed-kask/mcp/swarm/agents/curated/`.

## 4. Spec D — the native-first-rate sensor (BUILT 2026-10-23)

The sensor shipped in the same session this spec was written. What exists, at the seams:

- **Capture (zed side):** `Thread::run_turn`'s Ok branch computes the completed turn's
  instrument summary (`crates/agent/src/kask_thread_state.rs::turn_instrument_summary` —
  pure over the turn's message span from the pre-turn `message_ix`): a turn is
  **native-first** when it contains no context-server (MCP) tool call and none of
  `KASK_HOST_INSTRUMENT_TOOLS` (`lisp_eval`, `render_template`, `skill`);
  **substantive** = user prompt ≥ `SUBSTANTIVE_MIN_WORDS` (20) words, so acknowledgments
  and mode toggles are excluded; editor-native tools (terminal, file tools) are not kask
  instruments — a terminal-only turn is still native-first for this sensor. Fired through
  the process-global `agent::NATIVE_FIRST_TURN_RECORDER` hook (the `record_mcp_tool_outcome`
  family), wired in `crates/zed/src/main.rs` to `RegulationLedger::record_agent_turn` on
  the GPUI-global tokio runtime. Only completed turns count; failed turns are not drift
  evidence.
- **Storage (kask side):** `RegulationLedger` keeps a capped (200) rolling window of
  substantive turns (`kask/crates/hkask-regulation/src/runtime.rs`); an empty window
  reads as no data — `None`, never a fabricated 0.0 (the `unwrap_or(0)` trap).
- **Sensing:** `NativeFirstRateSensor` (`sensor_provider.rs`), registered in
  `CyberneticsLoop::build` next to `ToolReliabilitySensor`, emits `native_first_rate`
  with set point `set_points.native_first_rate_threshold` (default **0.50**, bounds
  [0.0, 1.0) — 1.0 is rejected as silently-vacuous, the mirror of the tool_reliability
  0.0-floor rule). The metric carries an Escalate→Curation policy rule
  (`native_first_rate_exceeded`), so deviations reach the algedonic board with
  rate/threshold evidence; healthy readings flow to the compare phase (the sensor does
  not decide — the Sensor trait contract).
- **Known limitation (v1):** no per-task-class breakdown — the aggregate rate + sample
  count first, because classifying a turn's task from its text is the injection
  channel's job (spec B builds that classifier anyway); the window is a plain recency
  window, not per-class. All context-server tools count as instruments (in this
  deployment the registry is kask's own servers; a hypothetical external server's tools
  would under-count drift — the conservative direction).

**Calibration note:** the operator's standing complaint says the live rate sits well
above the 0.50 set point, so the first readings are expected to deviate — that is the
sensor working, not a misconfiguration. Recalibrate the threshold once the baseline
window fills (the same set-points config path `tool_reliability` uses).

**Why first (unchanged):** this experiment measured instructed-artifact compliance in
the local runtime — a proxy. The field quantity the operator cares about is spontaneous
drift in the editor agent, which only this sensor can measure. It also makes every later
intervention falsifiable, including the injection below — the next step is reading the
baseline the sensor now produces, then mounting the dynamic-injection experiment at
n ≥ 30.

## 5. Spec B — per-turn instrument-affordance injection (production)

**Mechanism.** The per-turn injection channel already exists and is proven
(`crates/agent/src/thread.rs:5910` — memory recall rides `inject_context` as a per-turn
Role::System message; it does not touch the digest-cached system prompt, so the provider
prefix cache is preserved). Extend the same channel:

1. Embed the user message (existing embedding port; the memory recall path already embeds
   queries per turn).
2. Retrieve the top-3 matching skills (the skill-discovery catalog becomes a harness step,
   not a model choice) plus the task-class-relevant tool names (qualified, from the session's
   available tool set and the 9-server registry).
3. Inject as a bounded block at the highest-attention position (end of context, immediately
   before generation): "THIS TURN'S INSTRUMENTS" — exact names, one line each, with the
   artifact each produces (run id, record ids, entity refs, vocabulary prefixes, evaluated
   form).
4. The block names instruments exactly as the model must call them (this experiment's t5
   specimen is the design rule: `host/lisp_eval`, not `lisp_eval`).

**Gate (companion).** A turn-end deterministic checker (generalizing the `adhd-mode`
pre-send gate from skill scope to harness scope) over gated claim classes — numeric claims
without deterministic-verification evidence, domain classifications without anchoring
citations, research claims without run ids — re-prompts with the named gap. Within-session,
this is the cheap approximation of a corrective training signal; it also feeds the sensor.

**Next experiment.** With the injection implemented, rerun this experiment's protocol with
the dynamic injection as the challenger (n ≥ 30 per arm for a 20pp band at ~0.8 power), with
the sensor reading the editor agent's field baseline alongside.

## 6. Limitations and filed follow-ups

- Response-text predicates proxy instrument use (fabrication risk disclosed; the t2 pilot was
  cross-checked against delegation `tool_calls` telemetry — real calls, real record ids).
- Local-agent proxy: the swarm runtime proxies the editor-side curator agent; host-tool and
  MCP-grant seams differ from the editor path (§2, §4).
- The static affordance block is a conservative proxy for the dynamic injection; the
  production mechanism is expected to be at least as strong (task-keyed, higher-attention
  position) but is untested.
- `MAX_TOOL_ROUNDS=4` bounds multi-call flows; the arms' round-budget line is itself part of
  the measured environment. A cap raise is a separate, tested change candidate.
- Filed to the MCP audit board: the three swarm-server observability/ergonomics defects of
  §2 (intermittent tagging correlation failure; silent `host/*` drop at card create/reconfigure;
  silent unqualified-name drop leaving `tool_calls: []` indistinguishable from choice).
- Closed on the audit board 2026-10-24 (card 7dfb817f): the tagging correlation
  failure was root-caused to the shared `tag-passages-batch` template's
  caller-specific `item-<number>` ID claim contradicting the swarm caller's
  `chunk-<number>` IDs (template fixed caller-neutrally, pinned by a
  reproducing render test); the create/reconfigure and delegation paths now
  surface every dropped `mcp_tools` entry with its reason, and the delegation
  result carries `advertised_tools` to disambiguate the empty-`tool_calls`
  state.