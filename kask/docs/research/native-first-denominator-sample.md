---
title: "Native-first denominator sample — first per-task skill-applicability adjudication"
audience: [architects, developers, operators]
last_updated: 2026-10-24
version: "1.0.0"
status: "Active"
domain: "Agent instrumentation"
mds_categories: [domain, trust, lifecycle]
---

# Native-first denominator sample — first per-task skill-applicability adjudication

Date: 2026-10-24 (UTC−7 evening of 2026-10-23). Status: first historical sample,
single-adjudicator, bounded. Companion to `instrument-affordance-experiment.md`
(spec D built the numerator; this supplies the per-applicable-task denominator its
v1 aggregate window cannot yet produce).

## 1. Question and method

The `native_first_rate` sensor (commit `c41f61b707`) counts substantive turns that used
zero kask instruments, but has no per-task-class denominator. This sample answers, over
real stored sessions: of substantive turns where at least one skill's trigger contract
applied, how often was the skill actually activated — split by whether the operator
named the skill (**compliance**) or the model had to discover it (**discovery**, the
H4 arrow).

- **Population**: the 14 most-recently-updated threads in the threads database
  (`~/.local/share/zed-kask/threads/threads.db`, D28 per-message storage — 531 messages),
  updated 2026-10-23/24. Turns assembled mechanically: a user message plus every
  subsequent agent tool call before the next user message (Resume/Compaction ignored);
  substantive = prompt ≥ 20 words, the sensor's own `SUBSTANTIVE_MIN_WORDS`.
- **Population split**: 5 turns are delegated critic threads (`spawn_agent` children
  whose prompts prohibit tool use **by protocol**) — reported separately; the v1 sensor
  cannot exclude them.
- **Extraction (read-only, reproducible)**:
  `sqlite3 -readonly <db> ".mode tabs" "SELECT thread_id, ix, data FROM thread_messages
  WHERE thread_id IN (…14 ids…)"`, then a jq walk of the externally-tagged message JSON
  (`{"User":{"content":[{"Text":…}]}}` / `{"Agent":{"content":[{"ToolUse":{"name":…}}]}}`)
  collecting `ToolUse.name` per turn; skill-call skill names parsed from the ToolUse
  input's `name` field.
- **Rubric**: each turn adjudicated against every SKILL.md description in the 59-skill
  catalog. A skill **matches** when its stated when-to-use conditions apply to the
  request's substance; **named** when the operator's prompt names the skill or its
  invocation ("run through the prompt enhance skill", "Load X via the skill tool");
  **activated** when the turn contains a `skill` tool call for it. Clear vs borderline
  vs not-applicable recorded per turn.

## 2. Adjudication table — interactive turns (n=19)

| # | thread (title) | prompt gist | words | class | matched contract(s) | activated |
|---|---|---|---|---|---|---|
| 1 | 04a3ee9b (underuse commission) | the capability-underuse diagnosis commission | 1126 | named-in-body | gradient-hunter | ✓ |
| 2 | 0a184454 (smell/code-graph audit) | "Load idiomatic-rust and coding-guidelines via the skill tool before composing" | 957 | named | idiomatic-rust, coding-guidelines | ✓ both |
| 3 | 0daf97b9 (PE underuse) | "run the following through prompt enhance skill" | 169 | named | prompt-enhance | ✓ |
| 4 | 24b76846 (kata-kanban finish) | finish work stream; "essentialist review" requested | 1394 | named | essentialist | ✓ |
| 5 | 2723fd9c (PE smell) | "run through the prompt enhance skill" | 62 | named | prompt-enhance | ✓ |
| 6 | 3f62c69c (kata-kanban server review) | MCP-server review against published reference models | 945 | **discovery** | mcp-tool-review (primary); refactor-architecture, kata-improvement (adjacent) | adjacent ✓✓, **primary ✗** |
| 7 | 99aa57b1 (eureka) | eureka-machine research question | 29 | borderline | research-run instruments served the turn | — |
| 8 | 99aa57b1 | "using coding guidelines, refactor architecture, grill-me, essentialist, pragmatic cybernetics, pragmatic semantics, hypothesis framer and falsifiability skills" | 39 | named | all eight | ✓ (8 calls) |
| 9 | 99aa57b1 | what are we adding / integration / duplication? | 48 | borderline | essentialist, refactor-architecture | ✗ |
| 10 | 99aa57b1 | harden soft assumptions / co-scientist comparison? | 42 | borderline | grill-me, essentialist | ✗ |
| 11 | 99aa57b1 | proceed with the updated plan | 21 | borderline | experiment instruments served the turn | — |
| 12 | 99aa57b1 | commit + "proceed with the algedonic review" | 23 | named | algedonic-review | ✓ |
| 13 | b8494b95 (underuse investigation) | the standing underuse complaint as investigation request | 161 | **discovery** | gradient-hunter (investigation class) | ✗ (research-run + curator instruments used) |
| 14 | b8494b95 | double-check loose ends, verify wiring, commit | 25 | borderline | code-review | ✗ |
| 15 | b8494b95 | rebuke: concurrent agents | 40 | n/a | — | — |
| 16 | b8494b95 | discussion: territoriality over code | 208 | n/a | — | — |
| 17 | b8494b95 | "what have we learned and what should we do next?" | 60 | **discovery** | metacognition, kata-improvement | ✗ |
| 18 | b8494b95 | discussion: LLM excuses | 91 | n/a | — | — |
| 19 | ee735121 (PE kata review) | "prompt enhance skill at high effort" | 66 | named | prompt-enhance | ✓ |

## 3. Delegated-critic turns (n=5, separate population)

Threads 32bbe115, 4d9d6b00, 6b9a7601, 7e947a8f, eba66f34 — prompt-enhance verification
delegations whose prompts **prohibit tool use by protocol** ("Do NOT use any tools —
this is pure analysis"). Zero tools, zero skills — by instruction, not drift. The v1
sensor counts these as native-first turns; per-class breakdown (spec D's known
limitation) is the fix for this overcount.

## 4. Rates

Interactive substantive turns (n=19):

- **Skill-applicable (clear): 11. Any-skill activation 9/11 (82%); primary-contract
  activation 8/11 (73%).**
  - **Operator-named: 8/8 activated — 100% compliance.**
  - **Discovery-required: 3/3 primary-contract misses (0%); 1/3 loaded adjacent skills
    (thread 3f62c69c). Borderline-applicable (5): 0 activations.**
- **Native-first** (sensor definition — no context-server call, none of
  skill/lisp_eval/render_template/onto_anchor): 6/19 (**32%**), of which 3 are
  rebuke/discussion turns carrying no applicable contract. Including the 5
  protocol-prohibited delegated turns: 11/24 (**46%**) — the number the v1 sensor
  will read in mixed windows.

## 5. Findings

1. **The broken arrow is match, not fetch.** Named skills load at 100%;
   discovery-required primary contracts loaded 0/3 (one partial — adjacent process
   skills found, the best-fitting contract missed: 3f62c69c loaded
   refactor-architecture and kata-improvement but not `mcp-tool-review`, whose
   description names that exact task).
2. **The operator is already hand-dispatching.** Turn 2's prompt instructs "Load
   idiomatic-rust and coding-guidelines via the skill tool before composing anything"
   — the human performing the dispatcher's job that spec B's per-turn suggestion
   automates. The 100% named-compliance rate is the model executing the operator's
   dispatch, not retrieving from the catalog.
3. **The sensor's aggregate will overcount drift** while delegated tool-prohibited
   turns are in the window (46% vs 32% here) — the per-class breakdown is
   load-bearing for threshold calibration, and spec B's task classifier produces it
   as a byproduct.
4. **Non-skill instruments are not the problem in this sample**: the discovery-missed
   turns used research-run, curator, and experiment instruments heavily (turns 6, 11,
   13). The underuse concentrates in the skills layer — the H4 pull-dispatch seam.

## 6. Limits

n=19 interactive turns over ~2 days (recency-biased, prompt-enhance-workflow-heavy);
single adjudicator (the Z-K agent) applying the recorded rubric — matches and
borderline calls remain judgment, mitigated by naming the matched condition per turn;
turn assembly is mechanical but Resume boundaries attach pre-resume tool calls to the
prior turn; non-substantive turns excluded (sensor-consistent); the 14-thread window
excludes older sessions.

## 7. Implications for the intervention plan

- **Spec B (per-turn instrument-affordance injection)** — its target arrow is now
  confirmed by measurement: the injection's top-3 skill suggestion replaces the
  operator's hand-dispatch (finding 2) and attacks the 0/3 discovery rate directly.
- **Trigger-contract sharpening (intervention 3)** is cheap and directly implicated:
  the `mcp-tool-review` miss is a description-retrievability failure in a
  59-skill catalog.
- **Sensor threshold calibration** should interpret the first field readings with
  finding 3's overcount until the per-class breakdown lands.