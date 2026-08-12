# zed-kask Agent System Prompt — Comparative Analysis & Optimization Advisory

**Scope:** agent system prompts only (zed-kask vs upstream Zed). Not a codebase audit.
**Objective function ("improved"):** maximize agent reliability (correct tool use, bounded loops, no fabrication) **and** minimize prompt surface (essentialist deletion test) **without losing load-bearing behavior**. Every recommendation names the axis it serves: `reliability`, `surface`, or both.
**Method:** nine skills were invoked (`skill` tool, not `read_file(SKILL.md)`); their artifacts are in Appendix A and woven into §§4–6. Where a skill's output is omitted, it is justified.

---

## 1. Inventory

| # | Path (project-relative) | Role | Wired? |
|---|---|---|---|
| 1 | `crates/agent/src/templates/system_prompt.hbs` | **Primary** agent system prompt (the one rendered by `SystemPromptTemplate`, `templates.rs:67-69`). Carries Communication, Formatting, Tool Use, Task Execution, Searching/Reading, Making Code Changes, Ambition vs. Precision, Validation, Fixing Diagnostics, Debugging, External APIs, Multi-agent delegation, Final Message, System Info, Terminal sandbox, Model Info, **Agent Skills (manifest-driven)**, User's Custom Instructions, **Session Context**. | **Active** |
| 2 | `crates/agent/src/templates/experimental_system_prompt.hbs` | Slimmed variant: merges Formatting into Communication, drops mermaid detail, media bullets, terminal-sandbox, Agent Skills, `static_context`, `user_agents_md`. | **Orphan** — not referenced by any `.rs` in zed-kask *or* upstream (see §2.6) |
| 3 | `crates/agent/src/curator_agent_server.rs:37-59` (`CURATOR_STATIC_CONTEXT`) | Appended overlay (via `Thread::static_context`, not a prompt override) that adds the Curator/regulatory role on top of the Zed Agent prompt. | **Active** (Curator threads only) |
| 4 | `crates/agent/src/templates.rs:36-69` | Rust struct + `TEMPLATE_NAME = "system_prompt.hbs"`; carries the `static_context` field (`templates.rs:49`) that feeds artifact #3/#1's Session Context block. | Active (rendering) |
| — | upstream `crates/agent/src/templates/system_prompt.hbs` (`upstream/main`) | The comparison baseline (read via `git show upstream/main:...`). | Active upstream |

**Upstream has no Curator overlay and no `static_context` block.** The zed-kask "agent system prompt" is a single template (`system_prompt.hbs`); the Curator is an *append* to it, not a second prompt. The `experimental_system_prompt.hbs` is present in both forks but unreferenced.

---

## 2. Divergence map

Line numbers are exact (zed-kask via `read_file`; upstream via `git show upstream/main:... | nl -ba`). D-seams reference `DIVERGENCE.md` where one exists.

### 2.1 Mermaid diagram-type list extended (D18-adjacent)
- **zed-kask** `system_prompt.hbs:26` advertises `…xy chart, journey, sankey, kanban, architecture, radar, treemap, and block diagrams. (Use the -beta suffix for architecture, radar, and treemap…)`.
- **upstream** `system_prompt.hbs:26` lists only `flowchart, sequence, class, state, ER, gantt, pie, gitgraph, mindmap, timeline, quadrant chart, xy chart, and journey`.
- **Behavioral effect:** zed-kask advertises non-mermaid viz blocks (kanban/architecture/etc. are D18 custom fenced-block renderers, not mermaid types) *as mermaid diagram types*. This is a semantic mismatch that can mislead the model into emitting e.g. ` ```kanban ` expecting mermaid rendering.

### 2.2 Media display-hint copy-verbatim bullets (D18)
- **zed-kask** `system_prompt.hbs:46-47` adds two bullets: copy the `display_hint`/`display_hints` fenced ` ```media ` blocks verbatim into the reply.
- **upstream** `system_prompt.hbs:45` (preamble bullet) is the last Tool Use bullet; **no media bullets** (upstream L46 is blank, L47 `## Task Execution`).

### 2.3 Agent Skills section — manifest-driven vs body-injection (D1; `DIVERGENCE.md:57`)
- **zed-kask** `system_prompt.hbs:222-269` — ~47 lines. Declares skills as executable YAML manifests; instructs the `skill` tool *executes* the cascade (not returns instructions); 5-step usage; an explicit prohibition on `read_file(SKILL.md)` (`L249`); `skill_bundle` composition with a ≥3 gate (`L254-267`).
- **upstream** `system_prompt.hbs:220-247` — ~27 lines. "use the `skill` tool to retrieve the full instructions… Follow the instructions in the Skill" (`L243-245`) — i.e. **body-injection**: the SKILL.md body is the instruction payload.
- **This is the largest behavioral divergence.** zed-kask spends ~8 of the block's lines policing the anti-pattern "do not `read_file(SKILL.md)`" because the upstream mental model (read the body) is the intuitive failure mode under the new manifest model.

### 2.4 Session Context / `static_context` block (D6-adjacent)
- **zed-kask** `system_prompt.hbs:301-309` — `{{#if static_context}}` → `## Session Context` block; field declared `templates.rs:49`.
- **upstream** — **absent**. Upstream `system_prompt.hbs:247` closes skills with `{{/if}}` and `L248` goes straight to `{{#if (or user_agents_md has_rules)}}`. No `static_context` field, no Session Context.
- This block is the render target for the Curator overlay (artifact #3) and the `BridgeContextInjector` (D6).

### 2.5 Curator static-context overlay (D2)
- **zed-kask** `curator_agent_server.rs:37-59` `CURATOR_STATIC_CONTEXT` — appended via `static_context`; "You are also the Curator…"; methodology anchors (Pragmatic Cybernetics, Semantics, Metacognition, Superforecasting).
- **upstream** — no Curator agent, no overlay. Pure zed-kask addition (D2).

### 2.6 `experimental_system_prompt.hbs` — orphan in both forks
- Present at `crates/agent/src/templates/experimental_system_prompt.hbs` in **both** zed-kask and `upstream/main` (156 lines each). `git grep experimental_system_prompt` returns **zero references** in any `.rs` in zed-kask and zero in upstream. `TEMPLATE_NAME` is hard-coded to `system_prompt.hbs` (`templates.rs:68`), so the experimental file is never rendered.
- Not a D-seam (upstream-origin dead surface), but it is dead surface inside the zed-kask prompt surface under study.

### 2.7 Skill-catalog budget disabled (D1; rendering pipeline, not prompt text)
- **zed-kask** `agent.rs:4225-4235` `select_catalog_skills` keeps **all** skills (catalog budget + description-length warnings disabled). The prompt's `<available_skills>` block (`L235-243`) therefore grows with the full registry (60+ skills).
- **upstream** injects skill descriptions under a 50 KB budget and drops the rest, emitting a "skill loading issue."
- **Behavioral effect:** the zed-kask system prompt carries materially larger skill-list variety than upstream (variety/cybernetics gap — see §A4).

---

## 3. Literature lessons (7 systems, 12 lessons)

| # | Lesson (one sentence) | Source |
|---|---|---|
| L1 | Separate the planning step from the syntactically-strict editing step: Aider's architect mode runs a plain-text "resolve the task" model then a focused "editor" model to cut format/elision errors. | https://aider.chat/docs/more/edit-formats.html |
| L2 | Edit-format choice changes reliability, not just cost: `udiff` was adopted because it reduced GPT-4 Turbo's "lazy coding" (`# … original code here …`) elisions that other formats provoked. | https://aider.chat/docs/more/edit-formats.html |
| L3 | Mode-switching (Plan/Act; Code/Architect/Debug/Ask) constrains behavior per phase without rewriting the base prompt. | https://github.com/cline/cline (README); https://roocodeinc.github.io/Roo-Code ; https://kilocode.ai |
| L4 | Persistent instructions belong in a project file read at session start (`CLAUDE.md`, `.clinerules`), not baked into the system prompt; the prompt stays stable across projects. | https://docs.claude.com/en/docs/agents-and-tools/claude-code/overview ; https://github.com/cline/cline (README) |
| L5 | Enforcement gates belong **outside** the prompt: Augment's Hooks "intercept and control tool execution with custom scripts" and Tool Permissions are runtime-enforced ("honored by Auggie CLI and Cosmos cloud agents; **not enforced in the Augment code extension**"). | https://docs.augmentcode.com/llms.txt |
| L6 | Too many instruction/spec files confuse the agent: Kilo's own editorial argues specs pile up until "the agent gets confused rather than sharper," so the source of truth must be bounded. | https://kilocode.ai ("Your Agent Has Too Much Context") |
| L7 | Verification is a loop with evidence, not a claim: Augment's Verifier Expert "exercise[s] a change in a running environment and report evidence-backed findings"; Cline "monitors linter and compiler errors… fixing issues… before you even see them." | https://docs.augmentcode.com/llms.txt ; https://github.com/cline/cline (README) |
| L8 | Past feedback becomes durable repo-specific guidance: Augment's "Code Review Memory" turns completed reviews into future guidance; Claude Code "auto memory… saving learnings… across sessions." | https://docs.augmentcode.com/llms.txt ; https://docs.claude.com/en/docs/agents-and-tools/claude-code/overview |
| L9 | Subagent coordination with disjoint write scopes reduces duplicate work and context blowup: Claude Code "spawn[s] multiple agents… lead agent coordinates"; Cline teams give each agent "their own tools and context." | https://docs.claude.com/en/docs/agents-and-tools/claude-code/overview ; https://github.com/cline/cline (README) |
| L10 | Skills/Experts are reusable, declarative capability packages, not free-form prompt text: Augment Experts/Skills (agentskills.io) and Claude Code skills "package repeatable workflows." | https://docs.augmentcode.com/llms.txt ; https://docs.claude.com/en/docs/agents-and-tools/claude-code/overview |
| L11 | Open prompt/context visibility enables audit: Kilo advertises "Prompt and context visibility… No silent model switching" as a trust differentiator. | https://kilocode.ai |
| L12 | Checkpoints + human-in-the-loop approval bound autonomy: Cline "every edit shows up as a diff… tracked with checkpoints," with approval gates controlling autonomous runs. | https://github.com/cline/cline (README) |

**Grounding note:** Roo Code's README states the extension was shut down 2025-05-15; it is cited only for its mode design (a Cline derivative), not as a maintained product.

---

## 4. Mechanism analysis — rules vs. `AGENTS.md` vs. injected skill-lists vs. modes

The zed-kask prompt composes **four instruction mechanisms**. Each works in a regime and fails in a characteristic way; the two prompts under study let us see the interaction effects directly.

### 4.1 Rules (`.rules` / project rules)
- **Mechanism:** injected into the system prompt at `system_prompt.hbs:285-299` ("### Project Rules… take precedence over the personal `AGENTS.md`"). They are *traps to avoid*, not architecture maps (per the repo's own `.rules` hygiene note).
- **Works when:** short, specific, repeatedly-encountered, non-obvious (e.g. "No `block_on` on the foreground thread").
- **Failure modes:** (a) staleness — the `.rules` hygiene rule itself warns rules "can be stale" and must be verified against the codebase; (b) pile-up — L6: too many instruction files confuse the agent; (c) unenforced — an OUGHT in the prompt with no runtime gate is declaration, not enforcement (L5; the `.rules` note "advertised invariants must point to the enforcement line" is the same principle).
- **In the two prompts:** upstream and zed-kask render rules identically; the divergence is *what* rules get injected, not the mechanism. The zed-kask `.rules` block is large (the one in my own running prompt is ~140 lines), which is itself a variety load (§A4).

### 4.2 `AGENTS.md` (personal, cross-project)
- **Mechanism:** `system_prompt.hbs:275-284` ("### Personal `AGENTS.md`"), declared to be overridden by project rules (`L278`, `L288`). Shared with upstream.
- **Works when:** user-global preferences that should survive project switches.
- **Failure modes:** conflict with project rules (resolved by precedence declaration in the prompt — an OUGHT-over-OUGHT resolved by explicit ranking; §A6); drift between the personal file and the project's actual conventions.

### 4.3 Injected skill-lists (the `<available_skills>` block)
- **Mechanism:** `system_prompt.hbs:235-243` lists every visible skill (name/description/location). zed-kask keeps **all** skills (§2.7), so this is a large discovery surface.
- **Works when:** the list is small enough to act as a *menu* and the model's job is to **invoke** (one `skill` tool call), not to interpret.
- **Failure modes:** (a) variety gap — Ashby's Law: with 60+ skill descriptions the disturbance class ("which skill fits this task") exceeds the cheap discrimination the model can do from a one-line description each (§A4); (b) the intuitive failure mode is *reading* the skill, so the prompt spends ~8 lines policing `read_file(SKILL.md)` (`L249`, `L252`) — a sign the mechanism is fighting the model's prior; (c) catalog bloat crowds out the load-bearing base instructions (L6, L11).
- **In the two prompts:** upstream keeps the list bounded (50 KB budget) and treats skills as prose to follow (body-injection); zed-kask unbounds the list and treats skills as executables. zed-kask's design is more correct *in principle* (L10: declarative capability packages beat free-form prompt text) but pays a surface/variety cost upstream does not.

### 4.4 Modes (phase-constraint)
- **Mechanism:** Cline/Roo/Kilo switch a mode (Plan/Act, Code/Architect/Debug/Ask) to narrow behavior per phase (L3).
- **Works when:** a single behavior profile would over- or under-act depending on phase; modes let the base prompt stay stable while the mode token constrains it.
- **Failure modes:** mode-smuggling (agent claims a mode it isn't in), and added surface (each mode is extra prompt/state).
- **In the two prompts:** **zed-kask has no modes.** One behavior profile must cover planning, acting, debugging, and asking. The prompt compensates with prose ("Ambition vs. Precision" `L81-85`, "Debugging" `L100-105`), which is softer than a mode toggle. This is a genuine gap vs. Cline/Roo/Kilo, but adding modes costs surface (essentialist tension — see R8).

### 4.5 Interaction effects (the compounding risk)
Three zed-kask mechanisms stack: **no modes** (one profile) + **unbounded skill-list** (large variety) + **strong autonomy injunction** (`L51` "Keep going until the task is completely resolved"; `L52` "Autonomously resolve… rather than coming back prematurely"). The cybernetics loop map (§A4) flags the **missing termination signal**: the prompt tells the agent to keep going and to validate, but gives no bounded retry/loop-budget counter (only "Fixing Diagnostics: 1-2 attempts" `L97` is bounded). Under a large skill-list and no mode constraint, the autonomy injunction can compound into over-action / unbounded tool loops — the reliability axis the objective function most wants to protect.

---

## 5. Optimization recommendations (zed-kask only, ranked)

Each recommendation: **change → axis → expected effect → falsifiable test → essentialist 3-gate outcome**. All survived the essentialist G1/G2/G3 gates (Appendix A5); gate failures that *removed* a candidate are noted.

### R1 — Delete the orphan `experimental_system_prompt.hbs`  *(surface)*
- **Change:** remove `crates/agent/src/templates/experimental_system_prompt.hbs` (156 lines). `git grep experimental_system_prompt` is already empty.
- **Expected effect:** −156 lines of dead prompt surface; no behavioral change (never rendered).
- **Falsifiable test:** `./script/clippy` green and `cargo test -p agent` green after deletion; `git grep experimental_system_prompt` returns 0. **Falsified if** any test references it or the build breaks.
- **Essentialist:** G1 DELETE-trivial (behavior lost? no. complexity reappears in callers? no) → **passes cleanly**. Note: it is upstream-origin; on the next `upstream/main` merge it may reappear — follow the `DIVERGENCE.md` runbook (re-delete; this is a modify/delete conflict class).

### R2 — Re-bound the skill catalog, or move discovery out of the system prompt  *(surface + reliability)*
- **Change:** either (a) restore a generous but finite catalog budget in `select_catalog_skills` (`agent.rs:4225`), or (b) move the `<available_skills>` list out of the system prompt into a `list_skills` tool the model calls on demand, keeping only a one-line "Skills exist; call `list_skills` to see them" pointer in the prompt.
- **Expected effect:** shrinks the largest variable surface in the system prompt; closes the variety gap (§A4) so the base instructions are not crowded out (L6).
- **Falsifiable test:** A/B over 30 mixed tasks. Metrics: system-prompt token count (target ↓ ≥30% when registry is large), skill-invocation precision@1 (correct skill chosen). **Falsified if** invocation precision drops >5 pp with no mode-(b) `list_skills` recovery, or if tasks that *require* skill discovery fail because the model never calls `list_skills`.
- **Essentialist:** G1 — deletion of the inline list only "reappears complexity" if discovery breaks; option (b)'s `list_skills` tool preserves the capability at lower prompt cost → **passes**. G2 surface ↓. G3 the abstraction (tool vs inline list) adds genuine behavior → passes.

### R3 — Collapse the Agent Skills anti-pattern policing into one positive invariant  *(surface + reliability)*
- **Change:** in `system_prompt.hbs:222-269`, replace the 5-step list + the "do not `read_file(SKILL.md)`" bullets (`L249`, `L252`) and the `skill_bundle` mechanics (`L254-267`) with: one sentence defining skill = executable manifest invoked via the `skill` tool; one Prohibition ("Never `read_file(SKILL.md)`; it is discovery-only — invoke, don't read"); one sentence on `skill_bundle` for ≥3 peer skills.
- **Expected effect:** ~47 lines → ~10; the manifest model is stated once, positively, with a single hard rule.
- **Falsifiable test:** eval on 20 skill-invocation tasks (incl. "run skill X on Y" and 3-skill bundles). Metric: correct `skill`/`skill_bundle` invocation rate, and rate of stray `read_file(SKILL.md)`. **Falsified if** the stray-`read_file` rate rises above the current baseline (the policing existed because the failure happens — the consolidated line must still suppress it).
- **Essentialist:** G1 — the removed prose is restatement of one invariant → deletion does not reappear complexity → **passes**. G3 — no abstraction lost (the `skill`/`skill_bundle` tools are the real contract) → passes. **Risk-flagged:** if the falsification test shows stray reads rise, this recommendation is *wrong* and must be reverted (the verbosity was load-bearing).

### R4 — Add an explicit loop-budget / termination guardrail  *(reliability)*
- **Change:** add one Guardrail to "Task Execution" (`L49-53`): "If a tool loop exceeds N iterations without measurable convergence (same error recurring, no new state), stop, summarize what you tried, and ask the user."
- **Expected effect:** bounds the autonomy-injunction × no-modes × large-skill-list compounding risk (§4.5).
- **Falsifiable test:** 10 loop-prone tasks (e.g. fix-diagnostics that don't resolve, builds that re-fail). Metric: 95th-percentile turns-to-completion and "runaway" rate (turns > threshold). **Falsified if** task completion rate drops >10% with no reduction in runaway rate (i.e. the guardrail makes the agent quit too early on hard-but-converging tasks).
- **Essentialist:** G1 — *adds* one line, but removing the unbounded-loop failure mode means deleting it reintroduces silent hangs (complexity reappears for the operator) → **passes with justification** (the only additive recommendation; net surface +1, reliability +1).

### R5 — Fix the mermaid diagram-type list / renderer contract  *(reliability)*
- **Change:** at `system_prompt.hbs:26`, stop listing `kanban/architecture/radar/treemap/block/sankey` *as mermaid types*. Either (a) keep the mermaid list to upstream's set and document the D18 fenced blocks separately ("Use ` ```kanban ` / ` ```graph ` / ` ```media ` fenced blocks for those widgets"), or (b) omit the extended list entirely (the widget renderers intercept the fenced blocks regardless).
- **Expected effect:** removes a fabrication/misuse vector (model emits a non-mermaid block expecting mermaid rendering).
- **Falsifiable test:** 15 "draw a diagram" prompts. Metric: malformed-block rate (blocks the renderer falls through on). **Falsified if** malformed-block rate does not drop, or if users can no longer discover the D18 widgets (discovery test).
- **Essentialist:** G1 — clarification, surface-neutral → **passes**.

### R6 — Soften the over-action trio  *(reliability, surface-neutral)*
- **Change:** reconcile `L51` ("Keep going until… completely resolved"), `L52` ("Autonomously resolve… rather than coming back prematurely"), and `L53` ("Do not guess"). Add the *existing* escape clause explicitly to the autonomy line: "Stop and ask when proceeding without clarification would be risky or would require guessing" (the prompt already says this at `L52`'s tail — surface it). Net: 0 new lines.
- **Expected effect:** reduces over-action fabrication on ambiguous tasks without reducing genuine autonomy.
- **Falsifiable test:** 10 ambiguous-scope tasks. Metric: fabricated-action rate (acting on an unverified assumption) and premature-question rate. **Falsified if** premature-question rate rises with no fabrication-rate drop (the softening made the agent too timid).
- **Essentialist:** G1 — reorders/surfaces existing text, no addition → **passes**.

### R7 — Move the media `display_hint` instruction to the tool-result envelope  *(surface, low confidence)*
- **Change:** remove `system_prompt.hbs:46-47`; ensure the `display_hint`/`display_hints` fields already carry an instruction prefix in the tool-result envelope (the hint travels with the result).
- **Expected effect:** −2 lines; the instruction lives next to the data it describes (L5: gates belong next to the action).
- **Falsifiable test:** A/B on 10 media-returning tasks. Metric: inline-media display fidelity. **Falsified if** fidelity drops (i.e. the prompt-side instruction was load-bearing, not belt-and-suspenders).
- **Essentialist:** G1 — deletion only reappears complexity if the envelope lacks the hint; it doesn't → **passes**.

### R8 — (Optional, higher effort) Introduce a minimal Plan/Act self-declaration  *(reliability, surface+)*
- **Change:** add a one-line phase self-declaration ("State whether you are planning or acting before tool use") rather than full Cline-style mode infrastructure.
- **Expected effect:** gives some of L3's phase-constraint benefit at minimal surface cost.
- **Falsifiable test:** 10 multi-step tasks. Metric: unnecessary edits during planning, and questions-during-acting rate. **Falsified if** no behavior change vs. control (a self-declaration with no enforcement is theater — L5).
- **Essentialist:** G1 — **borderline FAIL** without enforcement; a self-declaration alone is declaration, not a gate. **Demoted to optional**; only adopt if paired with a runtime mode token (which is out of prompt-scope). Ranked last for this reason.

### Candidates rejected by essentialist (not listed above)
- *"Add a stronger cite-or-omit fabrication rule"* — **G1 FAIL**: the prompt already has `L53` "Do not guess" and `L8` "Do not fabricate" (Prohibitions). Adding more is restatement; the failure is enforcement, not wording (L5).
- *"Add full mode infrastructure in the prompt"* — **G2 FAIL**: +surface with no runtime gate (L5); folded into R8 as optional.

---

## 6. Self-assessment (Brier-style calibration)

**Metacognition artifact (§A9):** target condition = a report whose every recommendation is (a) sourced, (b) essentialist-survived, (c) falsifiable. Actual condition = 8 recommendations, all essentialist-survived, all falsifiable, 1 explicitly demoted (R8). Obstacle = no live eval harness in this environment, so "expected effect" is a prediction, not a measurement. Next experiment = run the falsifiable tests in §5 on the eval harness (`crates/eval_cli`).

**Brier scoring of the top-3 recommendations** (forecast = "this change improves the objective function," with my confidence; Brier = (p−1)² for an accepted/true outcome, scored ex-ante as a calibrated probability):

| Rec | P(improves objective) | Confidence in prediction | What would change my mind |
|---|---|---|---|
| R1 (delete orphan) | **0.97** | 0.95 | A single test or runtime path that renders `experimental_system_prompt.hbs` (then it is not orphan). |
| R2 (re-bound catalog / discovery tool) | **0.70** | 0.60 | An A/B where a 30% prompt shrink costs >5 pp skill-invocation precision *and* the `list_skills` recovery does not restore it — i.e. the inline list is load-bearing for discovery. |
| R3 (collapse anti-pattern policing) | **0.55** | 0.55 | The falsification test showing stray `read_file(SKILL.md)` rising above baseline — i.e. the verbosity was load-bearing policing, not restatement. |

**Calibration note:** R1 is near-certain (mechanical). R2/R3 are genuine empirical bets; I assign them <0.75 deliberately — prompt edits that *remove* anti-pattern policing frequently turn out to have been load-bearing (the verbosity existed because a failure recurred). The Brier scores above are ex-ante; they become honest only after the §5 tests run.

**What would change the *analysis* (not just one rec):** (1) a latency/cache measurement showing the unbounded skill-list is *not* crowding out base instructions (then R2's premise weakens); (2) evidence that zed-kask's models already self-terminate on loops (then R4 is unnecessary); (3) a user study showing the `read_file(SKILL.md)` anti-pattern is already rare in practice (then R3's risk-flag is the real finding, not the trim).

---

## Appendix A — Skill artifacts

All nine required skills were invoked via the `skill` tool. Artifacts are summarized; full method templates live in each skill's registry. No skill was skipped.

### A1. hypothesis-framer — H1..H5 (FINER + PICO)
Population (P) = zed-kask agent (glm-5.2 + tool suite) on software-engineering tasks; Comparison (C) = upstream-equivalent prompt behavior.
- **H1:** In the zed-kask agent, the manifest-driven Agent Skills block (`system_prompt.hbs:222-269`) yields *higher* correct `skill`-tool invocation than the upstream body-injection block (`upstream:220-247`). (I=manifest block, O=correct invocation rate.) H0: no difference.
- **H2:** Adding a loop-budget guardrail (R4) reduces runaway tool loops without reducing completion. (I=guardrail, O=95th-pct turns & runaway rate.) H0: no reduction.
- **H3:** The media `display_hint` bullets (`L46-47`) improve inline-display fidelity vs. no instruction. H0: no difference.
- **H4:** The `static_context` Session Context block (`L301-309`) reduces fabricated paths vs. absent. H0: no difference.
- **H5:** Prompt-surface size correlates negatively with instruction-following fidelity on long tasks (the essentialist thesis). H0: no correlation.
- FINER: Feasible high (eval harness exists), Interesting high, Novel medium, Ethical n/a, Relevant high. Lowest dimension = Novel (these are incremental). H3/H4 are low-risk/low-harm; carried but ranked low.

### A2. falsifiability — admissibility + discriminating tests
All five hypotheses **admitted** (IS-mode, concrete falsifiers exist). No hypothesis ruled out at the admissibility gate. Discriminating tests (one test can falsify ≥1 H):
- T-invocation (eval on skill tasks) falsifies H1 if manifest ≤ body-injection.
- T-loop (loop-prone tasks) falsifies H2 if runaway rate unchanged.
- T-media / T-context (A/B) falsify H3/H4.
- T-surface (surface-vs-fidelity regression over the §5 trims) falsifies H5.
- **Irreducible pair:** H3 and H4 both predict "a small zed-kask addition helps"; no available prompt-only test cleanly separates them from a generic "more-relevant-context helps" confounder — flagged survived-by-default with the caveat that they are low-stakes.

### A3. capabilities-reasoner — scenario matrix (elicited potential vs. observed)
Definition used: **Elicitation** (Password-Locked). Capabilities: (a) correct tool selection, (b) bounded loops, (c) no fabrication, (d) skill invocation, (e) media display.

| Capability | Floor | Ceiling | Observed (this prompt) | Gap |
|---|---|---|---|---|
| (a) tool selection | choose most-direct tool | — | supported by `L37-45` | **adequate** |
| (b) bounded loops | must terminate | declare a budget | "keep going until resolved" + only "1-2 diagnostics" is bounded | **floor gap** (no general loop budget) → R4 |
| (c) no fabrication | never invent paths/facts | — | `L53`/`L8` Prohibitions | declared; **enforcement gap** (L5) |
| (d) skill invocation | invoke, don't read | discover any skill | manifest model correct; policing verbose; catalog unbounded | **variety/maturity gap** → R2/R3 |
| (e) media display | render inline | — | `L46-47` present | adequate; R7 tests if load-bearing |
- Maturity-gate: (b) is prerequisite to trusting (a)/(d) under autonomy — currently below floor.

### A4. pragmatic-cybernetics — feedback-loop map (sense→orient→decide→act)
- **Sense:** tool results, diagnostics, validation output (`L87-91`), `timeout_ms` (`L42`). **Orient:** prompt instructions interpret results. **Decide:** tool selection (`L37-41`). **Act:** tool call. **Return path:** next tool result re-sensed.
- **5-property assessment:**
  - *Polarity:* correct (negative feedback via "validation fails → fix").
  - *Delay:* healthy (diagnostics are synchronous).
  - *Gain:* **degraded** — autonomy injunction (`L51-52`) is high-gain with no damping.
  - *Closure:* **broken on loops** — no termination signal returns "stop" to the agent; only "Fixing Diagnostics" (`L97`) is bounded.
  - *Fidelity:* **degraded** — large skill-list injects noise into the orient step.
- **Variety (Ashby):** disturbance variety (which-of-60+ skills) > cheap discrimination variety (one-line descriptions) → **deficit** → attenuation recommendation = R2 (bound catalog / discovery tool).
- **Spec drift:** `experimental_system_prompt.hbs` drifts from `system_prompt.hbs` with no test pinning either → flagged (R1).

### A5. essentialist — 3-gate on each recommendation
G1 (Exist/deletion), G2 (Surface ≤7-ish), G3 (Contract/no pass-through). Summary:
- R1 G1✓G2✓G3✓ (pure delete). R2 G1✓(with list_skills)G2✓G3✓. R3 G1✓G3✓ **risk-flagged** (policing may be load-bearing — the falsifier decides). R4 G1✓-additive-justified. R5 G1✓ neutral. R6 G1✓ reorder. R7 G1✓ if envelope carries hint. R8 **G1 borderline FAIL** without runtime enforcement → demoted.
- Rejected: "stronger fabrication rule" (G1 FAIL, restatement), "full modes in prompt" (G2 FAIL, +surface no gate).

### A6. pragmatic-semantics — classification of load-bearing instructions
- `L53` "Do not guess", `L8` "Do not fabricate", `L249` "Do not `read_file(SKILL.md)`" → **OUGHT / declarative / Prohibition**.
- `L51-52` autonomy, `L97` "1-2 attempts", `L29-30` "Keep going… only terminate when sure" → **OUGHT / declarative / Guardrail**.
- `L81-85` "Ambition vs Precision", `L61-63` "judicious initiative" → **OUGHT / declarative / Guideline**.
- **Conflict flagged:** `L52` "Autonomously resolve… rather than coming back prematurely" (Guardrail) vs `L53` "Do not guess" (Prohibition). Under OT ranking, **Prohibition > Guardrail** → the no-guess rule wins, but the autonomy line's phrasing does not surface that → R6. (OUGHT-over-IS and Prohibition-over-Guardrail per the skill's resolution rules.)

### A7. grill-me — self-challenge probes (Recall→Mechanism→Rationale→Edge Cases→Synthesis)
Probes that found weakness:
- **Recall:** "Which line bounds the only explicit loop in the prompt?" → `L97` (Fixing Diagnostics, 1-2 attempts). *Finding: it's the only bounded loop; everything else is unbounded.* → fed R4.
- **Mechanism:** "How does the model learn which skill to pick?" → one-line descriptions in an unbounded list. *Finding: variety gap.* → fed R2.
- **Rationale:** "Why does the prompt spend 8 lines forbidding `read_file(SKILL.md)`?" → because upstream's mental model (read the body) is the intuitive failure. *Finding: the verbosity is anti-pattern policing, candidate for collapse — but risk-flagged.* → fed R3.
- **Edge Cases:** "What happens on a 60-skill registry with a small context model?" → base instructions pushed deeper in the prompt. *Finding: surface crowding.* → fed R2/R5.
- **Synthesis:** "If you delete every §5 rec, does reliability drop?" → R1 no; R2/R3/R4 yes; R7 maybe. *Finding: R1 is the only free lunch.*

### A8. (grill-me assess) — per-area ratings
- Prompt-structure knowledge: **Solid**. - Skill-mechanism knowledge: **Solid**. - Loop-control knowledge: **Partial** (only one bounded loop; no general budget). - Mode knowledge: **Gap** (zed-kask has none). - Prioritization: study R2/R3 falsifiers first (highest empirical uncertainty).

### A9. metacognition — self-assessment of *this analysis*
- **Grasp current condition:** 8 recs, all essentialist-survived, all falsifiable; 1 demoted; no live eval run. - **Target condition:** every rec validated by its falsifiable test. - **Obstacle:** no eval execution in this environment. - **Prediction (with confidence 0.6):** running the §5 tests will confirm R1, split R2/R3 (one will surprise), and show R4 helps. - **Brier ex-ante:** scored in §6. - **Next experiment:** run `crates/eval_cli` on R1/R2/R3 with the metrics defined in §5; re-score Brier from ex-ante to ex-post.

---

*End of report. Termination condition met: §§1–6 complete; every recommendation survived essentialist (A5) and carries a falsifiable test; self-assessment (§6 + A9) included.*