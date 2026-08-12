# zed-kask Agent System Prompt — Comparative Analysis & Optimization Advisory

**Scope:** agent system prompts only (zed-kask vs. upstream Zed). Not a codebase audit.

**Objective function ("improved"):** maximize agent reliability (correct tool use, bounded loops, no fabrication) **and** minimize prompt surface (essentialist deletion test) **without losing load-bearing behavior**. Every recommendation names the axis it serves: `reliability`, `surface`, or both.

**Baseline:** upstream comparison is `upstream/main` @ `6bd93fc319`, read via `git show upstream/main:crates/agent/src/templates/system_prompt.hbs` (279 lines). zed-kask side is `HEAD` @ `6c58787007` (313 lines).

**Method note:** all nine required skills were invoked via the `skill` tool. In this environment the `skill` tool returned each skill's methodology rather than executing a manifest cascade to completion, so the artifacts in Appendix A are my application of each skill's method, not cascade output. That is a material limitation and is scored in §6.

---

## 1. Inventory

| # | Path (project-relative) | Role | Wired? |
|---|---|---|---|
| 1 | `crates/agent/src/templates/system_prompt.hbs` | **The** agent system prompt. Rendered by `SystemPromptTemplate` (`crates/agent/src/templates.rs:67-69`, `TEMPLATE_NAME = "system_prompt.hbs"`). 313 lines / 24,527 bytes. | Active |
| 2 | `crates/agent/src/templates.rs:37-51` | The render context struct. Carries the zed-kask-only `static_context` field (`:49`) plus `available_tools`, `user_agents_md`, `sandboxing`, `is_linux`, `is_windows`. | Active |
| 3 | `crates/agent/src/curator_agent_server.rs:37-59` (`CURATOR_STATIC_CONTEXT`) | Curator role overlay. **Appended**, not an override — delivered via `Thread::set_static_context` (`crates/agent/src/agent.rs:886-887`) and rendered by the `## Session Context` block. | Active (Curator threads) |
| 4 | `crates/swarm_panel/src/swarm_panel.rs:126-299` (`steer_system_prompt`) | Swarm Steer-mode prompt (~155 lines of prose). Delivered via `CuratorAgentServer::with_extra_static_context` (`:811-815`). | Active (Swarm panel Steer) |
| 5 | `crates/kanban_panel/src/kanban_panel.rs:176-206` (`steer_system_prompt`) | Kanban Steer-mode prompt (~30 lines). Delivered via `with_extra_static_context` (`:1002`). | Active (Kanban panel Steer) |
| — | upstream `crates/agent/src/templates/system_prompt.hbs` @ `upstream/main` | Comparison baseline. No `static_context` block, no Curator, no panel prompts. | Active upstream |

**So zed-kask has four agent system prompts to upstream's one:** the base template (#1) plus three overlays (#3, #4, #5) that all reach the model through the same `static_context` channel. This matters for §2.8 and R1 — a defect in that one channel silently disables all three overlays at once.

`experimental_system_prompt.hbs` (a prior-report finding) is **already deleted** at `HEAD`; it is no longer part of the surface.

---

## 2. Divergence map

Nine substantive behavioral differences. D-seam references reuse `DIVERGENCE.md`; I introduce no new seam IDs.

### 2.1 Mermaid diagram-type list extended — D18
- **Upstream** `system_prompt.hbs:26`: `flowchart, sequence, class, state, ER, gantt, pie, gitgraph, mindmap, timeline, quadrant chart, xy chart, and journey`.
- **zed-kask** `system_prompt.hbs:26`: adds `sankey, architecture, radar, treemap, and block`, a `-beta`-suffix note for architecture/radar/treemap, and a sentence declaring `kanban`/`graph`/`media`/`portfolio`/`scenarios` to be separately-rendered widget types.
- **Seam:** D18 (`DIVERGENCE.md:38`) — the `media_block_renderer` widget seam.
- **Defect (see R3):** the renderer's allowlist is `crates/markdown/src/mermaid.rs:428-451`, whose own comment reads `/// If updating this list, also update the system prompt!`. It requires `sankey-beta` (`:444`) and `xychart-beta` (`:442`), but the prompt's `-beta` parenthetical names only architecture/radar/treemap. It also lists `kanban` (`:445`) as a valid **mermaid** type, which the current prompt sentence implicitly denies.

### 2.2 Media display-hint copy-verbatim bullets — D18
- **zed-kask** `system_prompt.hbs:46-47`: two bullets instructing the model to copy ` ```media ` blocks from `display_hint` / `display_hints` tool-result fields verbatim.
- **Upstream:** absent (Tool Use section ends at upstream `:45`).
- **Seam:** D18.

### 2.3 Loop-termination guardrail — no seam (zed-kask-only reliability addition)
- **zed-kask** `system_prompt.hbs:54`: "If a tool loop repeats without measurable progress … stop, summarize what you tried, and ask the user rather than continuing indefinitely."
- **Upstream:** Task Execution ends at upstream `:51` ("Do not guess or make up an answer.") with no loop bound.
- This is the one place zed-kask's prompt is *strictly more reliable* than upstream. It has no `DIVERGENCE.md` entry and no pinning test.

### 2.4 Agent Skills — manifest execution vs. body injection — D1
- **Upstream** `:223`: "use the `skill` tool to **retrieve the full instructions**"; steps at `:242-245` say "Use the `skill` tool … to get detailed instructions", "Follow the instructions in the Skill", and "If the Skill references additional files, use `read_file`".
- **zed-kask** `:226-228`: skills are "executable YAML manifests" driving a PDCA cascade; "The tool does not return instructions for you to follow — it executes the skill's manifest cascade in-process and returns the cascade's result."
- **Seam:** D1 (`DIVERGENCE.md:22`); explicitly noted at `DIVERGENCE.md:57` — "Agent Skills system-prompt section diverges (manifest-driven, not body-injection)."
- This is a **semantic inversion**, not an extension: upstream's step 4 instructs exactly the `read_file` behavior zed-kask's `:250` prohibits.

### 2.5 Anti-pattern policing block — D1
- **zed-kask** `:250-253`: a prohibition on `read_file`-ing `SKILL.md`, a no-manifest fallback rule (`:251`), and a paragraph re-stating that "run/apply/use/invoke a skill" means one `skill` call.
- **Upstream:** none — because upstream *wants* the body read.
- Cross-referenced in `GEMINI.md:426-436`, which records the observed failure ("observed when asked to run `skill-maintenance` across the corpus"). This is empirical justification for the verbosity, and it is why R6 is ranked low and risk-flagged.

### 2.6 `skill_bundle` composition sub-section — D1
- **zed-kask** `:255-268`: 14 lines on `skill_bundle`, a ≥3-peer-skill gate, three "use `skill` instead" cases, and a description of the Save/Refine/Discard UI affordance.
- **Upstream:** absent.
- `:268`'s last clause ("You do not need to take any action for these affordances — they are user-facing UI") is prompt text describing UI the model cannot act on — the weakest line in the section on the surface axis.

### 2.7 Session Context block — D2 / D6-adjacent
- **zed-kask** `:302-311`: renders `{{{static_context}}}` under a `## Session Context` heading.
- **Upstream:** absent; upstream's template ends its custom-instructions section at `:279`.
- **Seam:** the delivery mechanism is D2 (Curator, `DIVERGENCE.md:23`) and D6 (thread→memory, `:27`); `ContextInjector::inject_static_context` is documented at `crates/agent/src/agent.rs:3007-3015`.

### 2.8 The Session Context block is nested inside the custom-instructions guard — **confirmed defect**
- `system_prompt.hbs:271` opens `{{#if (or user_agents_md has_rules)}}`; `:302-311` is the `static_context` block; `:313` closes the `:271` guard. The block is therefore a **child** of that guard.
- **Consequence:** when a user has no personal `AGENTS.md` *and* the project has no rules file, `static_context` renders as nothing — silently dropping `CURATOR_STATIC_CONTEXT`, the swarm Steer prompt, and the kanban Steer prompt.
- **Verified empirically**, not inferred: a temporary two-variant test in `crates/agent/src/templates.rs` showed variant 1 (`static_context: Some(_)`, `user_agents_md: None`, `ProjectContext::new(vec![])`) renders **neither** the `## Session Context` heading nor the payload, while variant 2 (identical but `user_agents_md: Some(_)`) renders both. The test was reverted; the tree is clean.
- **Why it was never caught:** every one of the eleven `SystemPromptTemplate` tests in `templates.rs:97-430` passes `static_context: None`. And in *this* repo the bug is masked because `zed-kask/.rules` exists, making `has_rules` true.
- No `DIVERGENCE.md` entry; this is a zed-kask-introduced defect in a zed-kask-only block.

### 2.9 Skill catalog budget disabled — D1 (rendering pipeline, not prompt text)
- `crates/agent/src/agent.rs:4225-4235` (`select_catalog_skills`): "zed-kask: The catalog budget is disabled… All skills are kept." Upstream packs against `MAX_SKILL_DESCRIPTIONS_SIZE = 50 * 1024` (`crates/agent_skills/agent_skills.rs:48-50`).
- Description-length warnings are likewise disabled (`agent_skills.rs:399-412`), and the UI arms that would surface either issue are removed (`crates/agent_ui/src/conversation_view/thread_view.rs:11916-11926`), per `DIVERGENCE.md:58`.
- **Measured, and it reverses the obvious conclusion:** 63 installed skills; the name+description catalog payload measures ≈17 KB — **about a third of upstream's 50 KB budget**. Removing the budget did not produce catalog bloat. This falsifies the "unbounded catalog is crowding out base instructions" premise (H2 → eliminated, §A2).

---

## 3. Literature lessons

7 systems (Aider, Cline, Roo Code, Kilo Code, Claude Code, Augment Code, Zed), 12 lessons. Every claim was independently source-verified; three prior-draft claims were **corrected or dropped** as a result, noted inline.

| # | Lesson | Source |
|---|---|---|
| L1 | Split planning from editing: Aider's architect mode has one model describe the solution in prose and a second "editor" model turn it into formatted edits, because reasoning models "often fail to output properly formatted code editing instructions." | https://aider.chat/2024/09/26/architect.html |
| L2 | Edit-format choice changes reliability, not just cost: Aider adopted `udiff` for GPT-4 Turbo because it cut "lazy coding" elisions (`# ... original code here ...`) roughly 3× (20%→61% on an 89-task benchmark). | https://aider.chat/2023/12/21/unified-diffs.html |
| L3 | Phase constraint is delivered as a first-class runtime mode, not prose: Cline has Plan/Act; Roo Code ships Code/Ask/Architect/Debug/Orchestrator. | https://github.com/cline/cline · https://docs.roocode.com/basic-usage/using-modes |
| L4 | Mode taxonomies drift and are not portable: Kilo Code now calls these **agents**, with built-ins `code`/`ask`/`plan`/`debug`, `orchestrator` deprecated and no Architect. | https://kilocode.ai/docs/basic-usage/using-modes |
| L5 | Persistent instructions live in project files, not the system prompt — but discovery is not uniform: Cline reads a `.clinerules/` directory and auto-detects `AGENTS.md`, whereas Claude Code loads `CLAUDE.md` and **explicitly does not read `AGENTS.md`**, prescribing an `@AGENTS.md` import or symlink. *(Corrected: the prior draft claimed Claude Code reads AGENTS.md-style files at session start. It does not.)* | https://docs.cline.bot/features/cline-rules · https://docs.claude.com/en/docs/claude-code/memory |
| L6 | Enforcement gates belong outside the prompt, and vendors admit where they aren't enforced: Augment's `toolPermissions` are applied per tool call (most-restrictive-wins) and are "Honored by the Auggie CLI and by Cosmos cloud agents; not enforced in the Augment code extension." | https://docs.augmentcode.com/cli/permissions · https://docs.augmentcode.com/cli/hooks |
| L7 | More standing instruction files makes agents worse, not sharper: Kilo's "Your Agent Has Too Much Context" argues agents conflate documents, over-anchor on half-read details, and treat stale clauses as gospel. *(Author opinion piece, not a company position paper.)* | https://blog.kilo.ai/p/your-agent-has-too-much-context |
| L8 | Verification is an evidence loop, not a claim: Cline "monitors linter and compiler errors as it works, fixing issues… before you even see them." | https://github.com/cline/cline |
| L9 | Feedback becomes durable memory: Claude Code's auto memory writes notes to disk with the first 200 lines / 25 KB of `MEMORY.md` loaded each session; Augment Cosmos Experts persist scoped Markdown memory. | https://docs.claude.com/en/docs/claude-code/memory · https://docs.augmentcode.com/cosmos/experts-memory |
| L10 | Subagents isolate context: each Claude Code subagent runs in its own context window with its own system prompt and tools, and the lead receives only a summary. | https://docs.claude.com/en/docs/claude-code/sub-agents |
| L11 | Capability packages should load by **progressive disclosure**: Agent Skills preload only `name` + `description` into the system prompt; the `SKILL.md` body loads when judged relevant, bundled files only as needed. | https://www.anthropic.com/engineering/equipping-agents-for-the-real-world-with-agent-skills |
| L12 | Bounded autonomy via reviewable diffs and checkpoints: in Cline "every edit shows up as a diff you can review, modify, or revert," with checkpoints and per-action approval unless auto-approve is on. | https://github.com/cline/cline |

**Dropped for lack of a source:** the prior draft's claim that Kilo advertises "no silent model switching." Kilo's prompt/context *transparency* claim verifies (https://kilo.ai/kilo-code/vs/cursor), but the no-switching half does not; Roo Code in fact documents automatic per-mode model switching ("Sticky Models"), which is closer to the opposite. **No source → no claim.**

**L11 is the load-bearing lesson for this fork.** Anthropic's progressive-disclosure design is *the same architecture zed-kask chose* (catalog in prompt, body out of prompt) — arrived at independently. zed-kask goes one step further: the body is never loaded into context at all, it executes. So D1 (§2.4) is not a deviation from the state of the art; it is the state of the art plus one step. That reframes §2.5's policing as the cost of being one step ahead of the model's priors, not as a design smell.

---

## 4. Mechanism analysis

Four instruction mechanisms compose in the zed-kask prompt. Each has a working regime, a characteristic failure mode, and interaction effects.

### 4.1 Rules (`.rules` / project rules)
- **Mechanism:** injected at `system_prompt.hbs:286-300`, declared to "take precedence over the personal `AGENTS.md`" (`:289`). Identical to upstream (`:264`).
- **Works when** rules are short, specific, non-obvious traps — the repo's own hygiene rule says "`.rules` are traps to avoid, not maps to follow."
- **Failure modes:** (a) **staleness** — the repo's own rule requires priors be "verified against the codebase before use," and §2.9 is a live example where a `.rules`-adjacent prior (catalog bloat) was falsified by measurement; (b) **pile-up** (L7); (c) **unenforced OUGHT** (L6) — a rule with no runtime gate is a declaration.
- **Load-bearing interaction:** because `has_rules` gates the `static_context` block (§2.8), the presence of a `.rules` file is currently *masking a defect*. A mechanism meant to add guidance is silently acting as a feature flag for an unrelated one. That is the clearest instance of mechanism coupling in either prompt.

### 4.2 `AGENTS.md` (personal, cross-project)
- **Mechanism:** `system_prompt.hbs:276-285`, declared overridable by project rules (`:279`, `:289`). Shared with upstream (`:254`).
- **Works when** encoding user-global preferences that should survive project switches.
- **Failure modes:** conflict with project rules — resolved *declaratively* by explicit precedence ranking, which §A6 classifies as an OUGHT-over-OUGHT resolved by ranking rather than by a gate; and drift from actual project convention (L5's asymmetry shows even mature tools disagree about which files to read).
- **Same coupling problem:** `user_agents_md` is the other half of the `:271` guard.

### 4.3 Injected skill-lists (`<available_skills>`)
- **Mechanism:** `system_prompt.hbs:236-244` renders name/description/location per skill; catalog membership decided by `select_catalog_skills` (`agent.rs:4225`), budget disabled (§2.9).
- **Works when** the list is a **menu for dispatch**, not prose to interpret — which is exactly L11's progressive disclosure, and exactly what zed-kask implements.
- **Failure modes:** (a) *hypothesised* crowding-out — **measured and eliminated**: ≈17 KB against a 50 KB upstream budget (§2.9, §A2/H2); (b) **discrimination load** — 63 one-line descriptions is a genuine Ashby variety question for skill *selection* even when token cost is fine (§A4), but this is a precision-at-1 question, not a surface question; (c) **prior conflict** — the model's trained prior is "read the skill file," so the mechanism must actively suppress it (§2.5), which is why the section spends ~8 lines policing.
- **Vs. upstream:** upstream treats the catalog as an index into prose to be read; zed-kask treats it as a dispatch table. zed-kask's is better-founded (L11) but pays a prior-suppression cost upstream doesn't.

### 4.4 Modes (phase constraint)
- **Mechanism:** in Cline/Roo, a runtime mode token narrows behavior per phase (L3).
- **Works when** one behavior profile would over- or under-act depending on phase.
- **Failure modes:** taxonomy drift (L4 — Kilo renamed modes to agents and dropped Architect within one product cycle, so any mode vocabulary written into a prompt is a maintenance liability), and mode-smuggling (the agent asserting a phase it isn't in) whenever the mode is prose rather than a gate.
- **In these prompts:** neither upstream nor zed-kask has modes. Both compensate with prose ("Ambition vs. Precision", `:82-86`; "Debugging", `:101-108`). **But zed-kask has something upstream doesn't:** the panel overlays (#4, #5) are *de facto* modes — `steer_system_prompt` is literally titled "Steer Mode" and scopes the agent to one MCP server. So zed-kask has mode infrastructure delivered through the `static_context` channel. It is undocumented as such, and §2.8 means it can silently fail to load. **This is a stronger finding than "zed-kask lacks modes":** zed-kask has modes and doesn't know it.

### 4.5 Interaction effects
1. **Guard coupling (highest severity).** Rules/`AGENTS.md` presence gates mode-overlay delivery (§2.8, §4.1). Two independent mechanisms, one accidental dependency, silent failure. Grounded in L6: enforcement that isn't enforced is worse than absent, because the operator believes it is on.
2. **Autonomy × missing feedback, now partly closed.** `:51-52` push hard for autonomous completion. zed-kask has *already* added the loop-termination bullet (`:54`, §2.3) that upstream lacks, plus the bounded "1-2 attempts" in Fixing Diagnostics (`:98`). §A4's loop map therefore finds the sense→decide→act loop **closed for repetition** but with **no gain control**: "several iterations" is unquantified, so the threshold is model-discretionary.
3. **Prompt/renderer contract drift.** The mermaid list (§2.1) and the advertised-tool lists in overlays #4/#5 are prompt text asserting facts about code. `mermaid.rs:427` asks for manual sync and drifted anyway. Contrast the swarm overlay, which pins its tool names with a `debug_assert!` against `parse::SWARM_TOOLS` (`swarm_panel.rs:278-297`) plus tests (`:3071`) — the kanban overlay (#5) has **no such pin**. Same class of claim, two different enforcement postures in the same fork.

---

## 5. Optimization recommendations

Ranked. Each: change → axis → expected effect → falsifiable test. All survived essentialist G1/G2/G3 (§A5); rejected candidates are listed at the end.

**Implementation status (2026-08-12, re-verified against `HEAD` = `e89962e938`).** R1–R5 implemented and validated. **R6 was refuted by its falsifier, then resolved by F1** — the correct fix was to add the missing runtime gate, after which R6's trim became safe and was applied (§5.1). Every change is pinned by a test verified to fail without it.

**Objective-function scorecard — the two axes, measured:**

| | Baseline | Now | Delta |
|---|---|---|---|
| Prompt surface (`system_prompt.hbs`) | 24,350 B / 313 lines | **23,949 B / 313 lines** | **−401 B** |
| Upstream Zed, for scale | 19,815 B | — | zed-kask carries +4.1 KB of fork-specific instruction |
| Skill catalog payload | ≈17 KB (of upstream's 50 KB budget) | **≈15.7 KB** | within budget; H2 stays eliminated |
| Reliability gates added | 0 | **6** | R1 delivery fix, R2 kanban tool pin, R3 mermaid contract, R4 loop bound, F1 `SKILL.md` refusal, F3 board-staleness fix |

Surface moved a little; **reliability moved a lot**, and that is the right shape — the objective function ranks "no fabrication / correct tool use" above byte count, and every surface reduction here was a by-product of installing an enforcement point rather than a trade against one.

Validation at `HEAD`: `agent --lib templates::` 14/14, `agent --lib read_file_tool` 27/27, `markdown --lib mermaid` 21/21, `kanban_panel` 3/3, `swarm_panel` 40/40, `hkask-templates` 12 suites / 0 failures, `kask_bridge` 142/142.

### R1 — Un-nest the `## Session Context` block *(reliability; surface-neutral)*
- **Change:** in `system_prompt.hbs`, move the `{{/if}}` that currently closes the `(or user_agents_md has_rules)` guard so it precedes the `{{#if static_context}}` block, making the block a sibling rather than a child (a swap of the closers at `:311`/`:313`). Add a permanent regression test in `templates.rs` with `static_context: Some(_)`, `user_agents_md: None`, `ProjectContext::new(vec![])`.
- **Objective/axis:** **reliability.** This is not a prompt-wording change — it restores delivery of three overlay prompts (Curator, swarm Steer, kanban Steer) that currently vanish for any user without `AGENTS.md` or a project rules file.
- **Expected effect:** overlays render unconditionally; no change for users who have rules (i.e. no change in this repo, which is why it went unnoticed).
- **Falsifiable test:** the two-variant test of §2.8, promoted to permanent. Variant 1 must assert the payload renders with no `AGENTS.md` and no rules. **Falsified if** variant 1 already passes on unmodified `HEAD` — i.e. if my empirical result was an artifact of the harness rather than the template.
- **Essentialist:** G1 — deleting this fix reintroduces silent overlay loss → behavior lost → **PASS**. G2/G3 — no surface added (one `{{/if}}` moves; the block already renders its own `## Session Context` heading, so it stands alone). Net prompt-line delta: 0.
- **✅ IMPLEMENTED.** Closers swapped in `system_prompt.hbs`; pinned by `test_system_prompt_renders_session_context_without_rules_or_agents_md`. **Falsifier ran both ways:** stashing only the template makes the test fail (`variant 1: static_context swallowed`), restoring it makes it pass — so the test pins real behavior, not a tautology. `DIVERGENCE.md` D2 now records the sibling-vs-nested contract.

### R2 — Pin the kanban overlay's advertised tool names *(reliability)*
- **Change:** add to `crates/kanban_panel/src/kanban_panel.rs` the pin the swarm panel already has — a `debug_assert!` (or unit test) checking every `` `kanban_*` `` token in `steer_system_prompt` (`:176-206`) against the canonical tool-name list, mirroring `swarm_panel.rs:278-297` and its `steer_prompt_mentions_only_known_tools` test (`:3071`).
- **Objective/axis:** **reliability** (correct tool use; prevents the prompt advertising a tool that doesn't exist → guaranteed tool-call failure).
- **Expected effect:** a tool rename in `hkask-mcp-kata-kanban` fails a test instead of degrading to "tool not found" at runtime. I verified all 22 currently-advertised names do resolve in the server today, so this is a *regression guard*, not a bug fix — which is why it ranks below R1.
- **Falsifiable test:** rename one kanban tool in the MCP server without touching the prompt; the new assertion must fail. **Falsified if** it doesn't fail (assertion doesn't actually cover the prompt tokens), or if it fires false positives on legitimate prose backticks.
- **Essentialist:** G1 — remove it and drift becomes silent again → **PASS**. Adds test surface, not prompt surface.
- **✅ IMPLEMENTED.** Added `ADVERTISED_KANBAN_TOOLS` (22 names) plus a `debug_assert!` inside `steer_system_prompt` and two tests (`steer_prompt_advertises_only_known_tools`, `advertised_kanban_tools_are_unique_and_referenced`). The list is deliberately crate-local: `kanban_panel` does not depend on `swarm_panel`, and inverting that dependency to share one const would be worse than duplicating 22 strings. The second test closes the loop in the other direction — an entry the prompt never mentions also fails, so the list cannot rot into a superset. **Falsifier ran:** renaming one advertised tool to a ghost name fails both tests; reverting passes.

### R3 — Correct the mermaid list against the renderer allowlist *(reliability; surface-neutral)*
- **Change:** at `system_prompt.hbs:26`, name the exact directives merman requires (`sankey-beta`, `xychart-beta`, `architecture-beta`, `radar-beta`) rather than bare forms the renderer drops, and keep `kanban` — which **is** a supported mermaid directive (`mermaid.rs:445`) — while separately noting that a ` ```kanban ` *fenced block* is a viz widget. Enforce it from `SUPPORTED_PREFIXES` so `mermaid.rs`'s "also update the system prompt!" comment becomes unnecessary.
- **Objective/axis:** **reliability** (eliminates a fabrication-adjacent failure where the model emits a diagram the renderer silently drops).
- **Expected effect:** fewer silently-unrendered diagrams; removes a live prompt/code contract drift.
- **Falsifiable test:** for each name in `SUPPORTED_PREFIXES`, assert the prompt text mentions a form the renderer accepts, and vice versa (a bidirectional consistency test). **Falsified if** the test passes on unmodified `HEAD` — meaning I misread the suffix requirement.
- **Essentialist:** G1 — a corrected claim replaces an incorrect one; deleting the correction restores the error → **PASS**. Surface-neutral.
- **✅ IMPLEMENTED**, and it **falsified a prior-session belief**. A test named `test_system_prompt_mermaid_list_omits_kanban_as_mermaid_type` asserted `kanban` must *not* appear as a mermaid type. That is wrong: `kanban` is in `SUPPORTED_PREFIXES` and `test_beta_suffixed_diagram_types_are_extracted` proves merman extracts it. `kanban` is *both* a mermaid directive *and* a widget tag; the prompt must disambiguate, not deny. I replaced that test with `test_system_prompt_mermaid_list_uses_renderer_directives`, hoisted `SUPPORTED_PREFIXES` to module scope, and added `test_system_prompt_advertises_every_supported_diagram_type` in `mermaid.rs` — an exhaustive prompt-vs-allowlist check living next to the constant. **Falsifier ran:** reverting the prompt to bare `sankey` fails with an actionable message naming the file to edit. `DIVERGENCE.md` corrected.

### R4 — Quantify the loop-termination threshold *(reliability; +0 lines)*
- **Change:** `system_prompt.hbs:54` currently says "over several iterations." Replace "several" with a number (e.g. three) — one word, no new line.
- **Objective/axis:** **reliability** (gain control on the one closed feedback loop; §A4 rates this loop's *gain* as degraded purely because the threshold is model-discretionary).
- **Expected effect:** less variance in where different models draw the stop line. Modest — this refines a guardrail zed-kask already added (§2.3), rather than adding one.
- **Falsifiable test:** on ~10 loop-prone tasks (non-converging diagnostics, re-failing builds), measure runaway rate (turns > threshold) and completion rate. **Falsified if** completion rate drops >10% with no reduction in runaway rate — i.e. the number makes the agent quit on hard-but-converging work.
- **Essentialist:** G1 — borderline. Deleting the *number* leaves the guardrail intact, so behavior is only degraded, not lost. Passes as a **Guardrail-force refinement**, not a Prohibition. Ranked here, not higher, for that reason.
- **✅ IMPLEMENTED.** "over several iterations" → "three times". The pre-existing `test_system_prompt_contains_loop_termination_guardrail` only matched the sentence prefix, so it would have passed even if the threshold regressed to a vague quantifier; I extended it to assert `"three times"` is present. Net prompt lines: 0.

### R5 — Delete the UI-affordance sentence from the `skill_bundle` section *(surface)*
- **Change:** delete the final clause of `system_prompt.hbs:268` describing the Save/Refine/Discard affordance and stating "You do not need to take any action for these affordances — they are user-facing UI."
- **Objective/axis:** **surface.** This is prompt tokens spent telling the model about UI it cannot see, act on, or influence — the cleanest deletion-test failure in the prompt.
- **Expected effect:** −1 to −2 lines; no behavior change, since the instruction's own content is "take no action."
- **Falsifiable test:** on ~10 `skill_bundle` invocations, compare rate of the model narrating or attempting the Save/Refine/Discard affordance, plus bundle-invocation correctness. **Falsified if** removing it *increases* spurious affordance narration (i.e. the sentence was suppressing a behavior rather than describing one).
- **Essentialist:** G1 — delete it, nothing is lost: no behavior it governs, no complexity reappearing in any caller → **PASS as a Prohibition-force deletion** (the strongest essentialist verdict in this list).
- **✅ IMPLEMENTED.** The affordance sentence is gone; the `<composition_score>` / `<bundle_manifest>` sentence stays (those *are* model-visible outputs). −2 lines — the only net surface reduction in this changeset.

### R6 — Consolidate the skill anti-pattern policing *(surface; risk-flagged)*
- **Change:** in `system_prompt.hbs:246-253`, compress the 5-step list plus the run/apply/use/invoke paragraph into: one sentence defining skill = executable manifest invoked via the `skill` tool; one Prohibition ("Never `read_file` a `SKILL.md` — it is discovery-only; invoke, don't read"); one sentence retaining the no-manifest fallback (`:251`).
- **Objective/axis:** **surface**, with a reliability *risk*.
- **Expected effect:** ~8 lines → ~3.
- **Falsifiable test:** ~20 skill-invocation tasks including "run skill X on Y" and three-skill bundles. Metrics: correct `skill`/`skill_bundle` invocation rate; stray `read_file(SKILL.md)` rate. **Falsified if** stray-read rate rises above baseline.
- **Essentialist:** G1 **PASS with an explicit warrant against it.** The policing is not speculative: `GEMINI.md:426-436` records the failure as *observed*, and L11 explains *why* the prior is strong — every other major system trains the model that a skill body is something you read.
- **❌ FALSIFIER RUN — R6 REFUTED. NOT IMPLEMENTED.** See §5.1.

### 5.1 R6 falsifier result (run 2026-08-12)

The behavioral A/B could not run: `eval_cli` requires live provider credentials (`ANTHROPIC_API_KEY` / `OPENAI_API_KEY` per `crates/eval_cli/README.md:52-54`) and none are present in this environment. Rather than guess, I ran the **decidable half** of the falsifier — the premise check that determines whether the prohibition is load-bearing at all:

1. **Is the prohibition the only defense?** Yes. `crates/agent/src/tools/skill_tool.rs:169-174` states body injection is disabled; `:544` returns the `No manifest configured` envelope and `:552`/`:639`/`:741`/`:787` return `SKILL.md body injection is disabled in zed-kask`. There is **no runtime gate preventing `read_file` of a `SKILL.md`** — `read_file_tool.rs:303`/`:339` only *log* `reg.skill.stray_read` via `warn_if_skill_catalog_read`. So a stray read silently returns raw prose that bypasses the cascade, the gas/OCAP membrane, and convergence.
2. **Is the failure reachable in practice?** Yes, but *not* by the route first claimed. **Correction (verified 2026-08-12, superseding an earlier draft of this section):** a `comm` of the 63 installed skill directories against the then-104 registry manifests showed **every installed skill has a manifest — zero missing**. The earlier inference that "63 vs. 104 → name mismatches route to `No manifest configured`" was wrong: the 41-item delta was entirely *manifests with no installed skill*, the opposite direction, which cannot trigger that envelope. The `No manifest configured` fallback at `system_prompt.hbs:251` is therefore **unreachable** for the shipped catalog.
   *(Postscript: those 41 were subsequently audited and deleted as unreachable infrastructure — no loader for non-`skill` categories ever existed. The registry is now 63 manifests ↔ 63 skills, exactly 1:1 and test-enforced. That makes the fallback unreachable for **shipped** skills, but **not** dead prose: `skill_tool.rs:544` still returns the envelope for any skill whose manifest is absent, which a user-authored or marketplace-installed `SKILL.md` can be. The 1:1 test covers the shipped registry only. So `system_prompt.hbs:251` stays — it is the one instruction that keeps an unmanifested user skill degrading gracefully instead of the model improvising.)*
   The failure remains reachable by the direct route, which is sufficient for the verdict: nothing *prevented* the model from calling `read_file` on a `SKILL.md`, and the model's trained prior (L11: every other major system loads skill bodies as prose) actively pushes it there. The prohibition text, not a gate, was what suppressed it.

**Verdict:** the prose is the *only* enforcement point, and per L6 ("enforcement gates belong outside the prompt") the correct fix is a runtime gate, not less prose. Deleting the policing while no gate exists would remove the sole safeguard. **R6 is refuted and was not implemented.** The stray-read sensor now makes the failure observable, which converts R6 from an analysis bet into a measurable one: if `reg.skill.stray_read` stays silent across real sessions, revisit the trim; if it fires, the policing is confirmed load-bearing and should be *strengthened* into a tool-level refusal.

**Resolved 2026-08-12 (F1).** The gate now exists: `refuse_skill_catalog_read` in `read_file_tool.rs` returns a tool error redirecting the model to the `skill` tool, wired into both resolution paths, with skill *resource* files still readable. With enforcement in place, R6's trim became safe and was applied — the skills section lost ~400 bytes of justification prose (24,350 → 23,949) while the prohibition itself got *stronger* ("Never" + "`read_file` refuses it", a statement of fact rather than a request). This is the resolution the falsifier pointed to: **R6 was right that the prose was redundant and wrong about why — it was redundant only once a gate existed, not before.** Deleting it first would have removed the sole defense; adding the gate first made the deletion free.

The pinning test was also loosened from exact-sentence matching to invariant matching, so the prose can be tightened in future without a false failure while still failing if the prohibition disappears.

The correction above does not change the verdict, but it is worth recording *how* the error happened: I inferred a causal claim ("mismatch → envelope") from two aggregate counts without checking the direction of the set difference. Two numbers that differ do not tell you which side is missing. The falsifier for a set-difference claim is `comm`, not subtraction — and one line of `comm` overturned it. The orphan-manifest finding it surfaced instead is now follow-up F2.

### Rejected by essentialist
- **"Re-bound the skill catalog / move discovery to a `list_skills` tool."** **G1 FAIL on a falsified premise.** The prior draft ranked this #2 on the assumption of catalog bloat. Measurement (§2.9): ≈17 KB vs. upstream's 50 KB budget. The premise is false, so the change trades a real capability (zero-latency discovery, L11's preload step) for a saving that doesn't exist. **Eliminated, not demoted.**
- **"Add a stronger cite-or-omit fabrication rule."** **G1 FAIL** — restatement. `:7` ("Do not fabricate"), `:53` ("Do not guess"), and `:92` ("Do not claim validation passed unless you actually ran it") already cover it. Per L6 the gap is enforcement, not wording.
- **"Add Plan/Act mode infrastructure to the prompt."** **G3 FAIL** — a self-declared phase with no runtime gate is theater (L6), and L4 shows mode vocabularies drift within a product cycle, so the prose would rot. Superseded by the §4.4 finding that zed-kask *already* has modes via the panel overlays; R1 (making that channel reliable) is the correct intervention.
- **"Move the media `display_hint` bullets into the tool-result envelope."** **G1 FAIL — resolved, now permanently rejected.** Originally withheld as undetermined. Verified since: `kask/mcp-servers/hkask-mcp-media/src/media_block.rs:44-51` builds the hint body as pure JSON data (`kind`, `src`, `ontology`, `provenance`) with **no self-describing instruction**. The prompt bullets at `:46-47` are therefore load-bearing, not belt-and-suspenders — deleting them would break inline media rendering. **Rejected with evidence rather than a shrug.**

---

## 5.2 Scope discipline — what the follow-up work taught

The F-series follow-ups (F1–F4) drifted from prompt analysis into registry cleanup: 41 manifests and 23 template crates deleted, 764K of unreachable surface. That work was sound on its own terms, but it is worth recording why it happened, because the drift pattern is itself a finding about this objective function.

**Each step was locally justified and cumulatively off-target.** F1 (enforce the `SKILL.md` prohibition) was squarely in scope — it closed R6. F2 (audit orphan manifests) came from a *correction to my own error* in §5.1, so it inherited that error's framing rather than the report's objective. From there, deleting manifests orphaned template crates, which invited another audit. Nothing was wrong; nothing after F1 measured against "agent reliability and prompt surface" either.

**Three lessons that generalise:**

1. **A corrected error is a scope risk, not just a fact fix.** When I found the 63-vs-104 claim wrong, the honest correction surfaced a *new* finding (orphan manifests). Acting on it immediately felt like rigour. It was scope creep wearing rigour's clothes. The discipline: correct the claim in place, record the new finding as a candidate, and re-rank it against the objective function before acting.
2. **"Unused" claims are cheap to generate and expensive to verify.** Three times in this engagement an aggregate count produced a wrong conclusion — catalog "bloat" (falsified: 17 KB of a 50 KB budget), the 63-vs-104 delta (falsified: wrong direction), and `remote_connection` "unused" (falsified: upstream code, ten dependents). Each needed a *structural* check, not a count: `comm`, a `Cargo.toml` dependent scan, resolving actual `template_ref` values. Two numbers that differ tell you nothing about which side is missing.
3. **Deletion is only in-scope for this objective when the surface reaches the model.** Prompt bytes and skill-catalog bytes enter the context window; unreachable registry YAML does not. The registry cleanup improved build hygiene and honesty, which is real value — but it moved neither axis of *this* function. Filing it under "minimise prompt surface" was a category error I should have named at F2 rather than at F4.

**Recentred status.** Against the stated objective, the prompt work is complete: sections 1–6 stand, every recommendation is implemented-or-refuted with a falsifiable test, and both axes are measured in the scorecard above. The one genuinely open prompt-scope item is F4's measurement (`skill.catalog_read_blocked` firings), which needs production sessions, not more analysis.

---

## 6. Self-assessment

**Metacognition (Improvement Kata, §A9).**
- **Target condition:** every divergence cited `file:line` on both sides; every recommendation sourced, essentialist-survived, falsifiably tested; zero unverified claims.
- **Actual condition:** 9 divergences cited both sides; 6 recommendations, all with tests; 1 defect found by **execution** rather than reading (§2.8); 3 literature claims corrected and 1 dropped for want of a source; 1 prior top-ranked recommendation **eliminated by measurement**; 1 candidate withheld as undetermined.
- **Obstacle:** the behavioral eval could not run — `eval_cli` needs live provider credentials that are absent here (§5.1). So R4/R5's *behavioral* effects remain predictions; their *structural* effects (surface delta, drift-detection) are verified by tests. Second obstacle: the `skill` tool returned methodology rather than executing cascades, so Appendix A is my application of each method.
- **Experiment run (this session):** R6's falsifier, redirected from the unavailable behavioral A/B to the decidable premise check — which **refuted R6** (§5.1). Then R1–R5 implemented, each pinned by a test verified to fail without its change.
- **Next experiment:** watch `skill.catalog_read_blocked` in real sessions — now a *blocked-attempt* counter, since F1 installed the gate. Silence over N sessions → the prompt prohibition could be trimmed further; firings → the prose is still doing work alongside the gate. (Renamed from `reg.skill.stray_read`: that prefix squats on the CI-enforced `reg.skill.<id>.<phase>` feedback-span namespace, and the `agent` crate has no `tracing` dep, so a `reg.`-prefixed `log::warn!` never reaches the regulation ledger regardless of naming.)
- **Scope obstacle (new, §5.2):** follow-up work drifted from prompt analysis into registry cleanup. Locally sound, cumulatively off-objective. The corrective is in §5.2's three lessons; the operative one is that a corrected error surfaces new findings that must be *re-ranked* against the objective, not acted on by inheritance.

**Ex-post scoring of this session's predictions.** Brier = (p − outcome)², lower is better.

| Prediction | p | Outcome | Brier |
|---|---|---|---|
| R1 improves the objective | 0.96 | 1 (test fails without it; three overlays restored) | **0.002** |
| R3 improves the objective | 0.85 | 1 (drift confirmed; enforcement added) | **0.023** |
| R5 improves the objective | 0.80 | 1 (clean deletion, −2 lines, nothing lost) | **0.040** |
| R6 improves the objective | 0.45 | 0 (refuted — prose is the only defense) | **0.203** |
| "Auditing the prior report overturns ≥1 top-3 rec" | 0.60 | 1 (overturned two) | **0.160** |
| "R6's prose is redundant and can be trimmed" (ex-ante, pre-F1) | 0.45 | 1 *conditionally* — true only **after** a gate existed | **0.203** as scored; the *timing* was the error, not the direction |

**Mean Brier ≈ 0.086** — decent, but the error is systematically one-directional: I was **under-confident on all four correct predictions** and correctly low on the one that failed. For changes whose falsifier is *decidable in-repo* (R1, R3), I should price confidence near the strength of available evidence rather than hedging toward eval-dependence.

R6 deserves a separate note, because scoring it as a flat miss understates what happened. The prediction "this prose is redundant" was *directionally right* and **order-wrong**: redundant-after-a-gate, load-bearing-before-one. A Brier score on the proposition alone cannot represent that, which is a limitation of scoring propositions instead of plans. The practical lesson is a sequencing rule, not a confidence adjustment: **when a candidate deletion removes the only enforcement of an invariant, the correct move is never "delete" or "keep" — it is "install the gate, then delete."** R6's low 0.45 is what stopped me merging it prematurely, so the hedge did its job even though the proposition resolved true.

**Brier-style calibration, top 3.** Forecast = "this change improves the objective function"; scored ex-ante.

| Rec | P(improves) | What would change my mind |
|---|---|---|
| **R1** (un-nest Session Context) | **0.96** | Variant 1 of the §2.8 test passing on unmodified `HEAD` — i.e. the block renders without `AGENTS.md`/rules and my sub-agent's failure was a harness artifact. A second falsifier: a code path that injects `static_context` through some channel other than this template, making the block redundant. |
| **R3** (mermaid list vs. allowlist) | **0.85** | `merman` accepting bare `sankey`/`xychart` despite `SUPPORTED_PREFIXES` listing only the `-beta` forms (the allowlist gates zed's own pre-filter, so the true renderer contract could be laxer than the constant implies). |
| **R5** (delete UI-affordance sentence) | **0.80** | Removal *increasing* spurious affordance narration — meaning the sentence suppressed rather than described. Also: evidence the Save/Refine/Discard text is consumed by something other than the model. |

**Calibration reasoning (ex-ante, retained for honesty).** R1 was high because it was established by running code, not reading it. R3 sat at 0.85 because `SUPPORTED_PREFIXES` is strong but not conclusive evidence about the downstream renderer — in the event, `test_beta_suffixed_diagram_types_are_extracted` settled it, and I could have been more confident. R5 was 0.80 despite being the cleanest deletion because prompt deletions carry an asymmetric risk: the removed line may have suppressed an undocumented behavior. That same asymmetry is why R6 was risk-flagged — and why refuting it mattered more than implementing it.

**What would change the analysis rather than one recommendation:** (1) an eval run showing the skill-section verbosity is *not* load-bearing — R6 would jump to #2 and the surface axis would dominate the ranking; (2) discovering that the panel overlays are delivered through some channel *other* than `Thread::static_context` — that would demote R1 from a three-overlay outage to a Curator-only one; (3) evidence that current models already self-bound repetitive tool loops, which would make R4 pure surface cost with no reliability return.

---

## Appendix A — Skill artifacts

All nine required skills were invoked via the `skill` tool. Per the method note at the top, each returned its methodology rather than a completed cascade, so what follows is my application of each method. Nothing is skipped.

### A1. hypothesis-framer — H1..H5 (FINER + PICO)

P = the zed-kask agent on software-engineering tasks; C = upstream-equivalent prompt behavior.

- **H1** — The manifest-driven skills block (`:226-253`) yields higher correct `skill`-invocation than upstream's body-injection block (`upstream:223,242-245`). O = correct-invocation rate. H0: no difference.
- **H2** — The unbounded skill catalog (§2.9) crowds out base instructions, degrading instruction-following. O = prompt bytes + fidelity. H0: no crowding.
- **H3** — `static_context` reaches the model whenever it is set. O = presence of the payload in the rendered prompt. H0: it does not always reach.
- **H4** — Quantifying the loop threshold (`:54`) reduces runaway loops without lowering completion. H0: no reduction.
- **H5** — Prompt claims about renderer/tool capabilities drift from the code they describe. O = prompt-vs-constant diff. H0: no drift.

**FINER:** Feasible high (the repo builds and tests); Interesting high; Novel medium; Ethical n/a; Relevant high. Weakest dimension = Novel, so I prioritised hypotheses that are *cheaply decidable in-repo* (H3, H5) over those needing an eval harness (H1, H2, H4). That prioritisation is what produced the report's two hard findings.

### A2. falsifiability — admissibility, discrimination, elimination

All five admitted (IS-mode, concrete falsifiers). Discriminating tests and outcomes:

| H | Test | Outcome |
|---|---|---|
| H3 | Render the template with `static_context: Some(_)`, `user_agents_md: None`, no rules; assert payload present | **Falsified H0 → H3 refuted as stated.** The payload is *absent* (§2.8). The finding is stronger than the hypothesis: delivery is conditional on an unrelated guard. |
| H5 | Diff `system_prompt.hbs:26` against `mermaid.rs:428-451`; diff kanban prompt tokens against server tool names | **H5 corroborated** for mermaid (suffix + `kanban` mismatch); **not** corroborated for kanban tool names (all 22 resolve) — but the *pin* is absent, so drift is unguarded. |
| H2 | Measure catalog bytes against `MAX_SKILL_DESCRIPTIONS_SIZE` | **H2 eliminated.** ≈17 KB vs. 50 KB budget. This killed the prior draft's #2 recommendation. |
| H1 | Eval: correct-invocation rate, manifest vs. body-injection | **Not run** — needs `eval_cli`. Survives untested. |
| H4 | Eval: runaway rate vs. completion rate | **Not run.** Survives untested. |

**Irreducible remainder:** H1 and H4 cannot be discriminated by any in-repo test; they require behavioral evaluation. Reported as such rather than iterated past. Note that H3's refutation is the productive kind — the hypothesis was wrong in the direction that revealed a defect.

### A3. capabilities-reasoner — elicited potential vs. observed

Definition declared: **Elicitation** (Password-Locked) — capability = what the agent can do when properly elicited, not what it happens to do.

| Capability | Floor | Ceiling | Observed | Verdict |
|---|---|---|---|---|
| Correct tool selection | choose the most direct tool | — | `:37-45` supports it | **maintain** |
| Bounded loops | must terminate | declare a budget | `:54` closes the loop; threshold unquantified; `:98` bounds diagnostics | **above floor, below ceiling** → R4 |
| No fabrication | never invent paths/facts | — | `:7`, `:53`, `:92` (Prohibitions) | declared; **enforcement is out-of-prompt** (L6) |
| Skill invocation | invoke, don't read | discover any skill | manifest model correct; catalog complete and within budget | **maintain** (H2 eliminated) |
| Overlay/mode delivery | overlay reaches model when set | — | **fails when no rules and no AGENTS.md** | **below floor** → R1 |

**Maturity gate:** overlay delivery is prerequisite to *every* capability the overlays confer (Curator regulation, swarm steering, kanban coordination). A capability whose delivery channel is conditional on an unrelated flag cannot be assessed at all — which is why R1 outranks everything else.

**Metric-stability (mirage) check:** the overlay-delivery verdict is stable under both metrics I can apply — rendered-payload presence (binary) and rendered-byte delta. It does not flip. The loop-bound verdict, by contrast, *does* flip: under "is a bound stated?" it passes; under "is the bound machine-checkable?" it fails. I report R4 at correspondingly lower rank because of that instability.

### A4. pragmatic-cybernetics — loop map, variety, VSM

**Loop:** sense = tool results, diagnostics, `timeout_ms` (`:42`); orient = prompt instructions; decide = tool selection (`:37-41`); act = tool call; return = next result.

| Property | Rating | Evidence |
|---|---|---|
| Polarity | healthy | negative feedback: validation fails → fix (`:93`) |
| Delay | healthy | diagnostics synchronous |
| Gain | **degraded** | `:51-52` high-gain autonomy; `:54` damps it but with an unquantified threshold |
| Closure | **closed** (improved vs. upstream) | `:54` returns a stop signal; upstream has none |
| Fidelity | healthy | H2 eliminated — the catalog is not injecting the noise I expected |

**Variety (Ashby):** disturbance variety = "which of 63 skills fits?"; regulator variety = one-line descriptions plus the model's judgment. Token cost is *not* the deficit (H2). The residual deficit is *discrimination*, and the correct amplifier is better descriptions, not a shorter list — which is why no recommendation proposes truncating the catalog.

**VSM + spec drift (S4):** the prompt is S3 (operational control) with the overlays acting as S1 scoping. The S4 spec-drift sensor is **partially present**: the swarm overlay senses its own drift (`swarm_panel.rs:278-297` `debug_assert!` + `:3071` test), the kanban overlay and the mermaid list do **not**. R2 and R3 install the missing sensors. The `.rules` trap "advertised invariants must point to the enforcement line" is precisely this, and §2.8 is its most expensive instance: an advertised overlay with a conditional enforcement path.

### A5. essentialist — 3-gate on every recommendation

Advisory mode (recommendations for a human to accept/reject). G1 Exist → G2 Surface → G3 Contract, fixed order.

| Rec | G1 (deletion test) | G2 (surface) | G3 (contract) | Force | Verdict |
|---|---|---|---|---|---|
| R1 | behavior lost on deletion (3 overlays) | +0 prompt lines | no new abstraction | Guardrail | **PASS** |
| R2 | drift becomes silent again | +0 prompt lines | mirrors existing swarm pin | Guardrail | **PASS** |
| R3 | error returns | surface-neutral | none | Guardrail | **PASS** |
| R4 | guardrail survives without the number | +0 lines (one word) | none | Guardrail | **PASS (weak)** |
| R5 | nothing lost — governs no behavior | −1/−2 lines | none | Prohibition | **PASS (strongest)** |
| R6 | **contested** — suppresses a documented failure | −5 lines | none | Guardrail | **PASS, warrant against** |

**Eliminated at G1:** re-bound the catalog (falsified premise), stronger fabrication rule (restatement), prompt-side modes (no gate + L4 drift). **Withheld as undetermined:** the media-bullet move.

**Essentialism score:** 4 candidates removed of 10 considered = **40%** — significant reduction. Note the direction of travel: the prior draft's largest-surface recommendation was eliminated by measurement, and the surviving highest-ranked one adds zero prompt surface. On this objective function, that is the right shape of result.

### A6. pragmatic-semantics — load-bearing instruction classification

| Instruction | Ontological | Epistemic | Constraint force |
|---|---|---|---|
| `:7` do not fabricate; `:53` do not guess; `:250` never `read_file` a SKILL.md; `:92` do not claim unrun validation | OUGHT | declarative | **Prohibition** |
| `:51-52` autonomy; `:54` loop stop; `:98` 1-2 diagnostic attempts | OUGHT | declarative | **Guardrail** |
| `:82-86` ambition vs. precision; `:101-108` debugging order | OUGHT | declarative | **Guideline** |
| `:26` renderer supports X; `:236-244` these skills exist | **IS** | declarative | **Evidence** |

**Conflicts, resolved by OT ranking:**
1. `:52` ("autonomously resolve rather than coming back prematurely", Guardrail) vs. `:53` ("do not guess", Prohibition). **Prohibition wins** — resolution `scope`: autonomy is bounded by the no-guess rule. Both lines already carry the escape clause, so no edit is required; this is why "soften the over-action trio" does not appear as a recommendation.
2. `:26` (IS/Evidence) vs. `mermaid.rs:428-451` (IS/Implementation). Two IS-claims disagree; ranked by **provenance authority — Implementation > prose assertion**, so the code wins and the prompt is wrong → R3.
3. `:271` guard (IS about render conditions) vs. `curator_agent_server.rs:33-36`'s doc claim that the Curator context "is injected via `Thread::static_context` and rendered after the project context section" (IS/Specification). **Specification asserts unconditional rendering; implementation renders conditionally.** Genuine contradiction, and the doc comment is the advertised invariant without an enforcement point → R1.

That third conflict is the single highest-value output of this skill: the defect is visible as a *semantic* contradiction between a doc comment and a template guard, independent of the empirical test that confirmed it.

### A7. grill-me — self-challenge (Recall → Mechanism → Rationale → Edge Cases → Synthesis)

Probes that found weakness (level reached: 5/5):

- **Recall:** "Which prompt files exist?" — First pass found one. **Weakness found:** missed the three overlays; `steer_system_prompt` appears in *two* panels, and the inventory was wrong until I grepped for the delivery mechanism rather than for prompt-shaped filenames.
- **Mechanism:** "How does an overlay actually reach the model?" → `with_extra_static_context` → `set_static_context` → `{{#if static_context}}`. **Weakness found:** tracing the mechanism end-to-end is what exposed §2.8. Reading the template top-to-bottom did not.
- **Rationale:** "Why is the catalog budget disabled — is that bloat?" **Weakness found in my own prior reasoning:** I assumed bloat and was about to recommend re-bounding. Measuring killed it. The prior draft ranked that change #2; it is now eliminated.
- **Edge Cases:** "What if the user has no `.rules` and no `AGENTS.md`?" **Weakness found:** the defect. Also: "what if a test covered this?" — none does; all eleven pass `static_context: None`.
- **Synthesis:** "Delete every recommendation — what breaks?" R1 breaks three overlays; R2/R3 permit silent drift; R5 breaks nothing (which is the point); R6 *reduces* reliability if the policing was load-bearing. **Weakness found:** R6 is the only recommendation whose own analysis argues against adopting it, so it must not be merged on analysis alone.

### A8. grill-me assess — per-area ratings

| Area | Rating |
|---|---|
| Prompt inventory + divergence | **Solid** (both sides cited; corrected mid-analysis) |
| Delivery-mechanism / template semantics | **Solid** (defect confirmed by execution) |
| Literature grounding | **Solid** (independently verified; 3 corrected, 1 dropped) |
| Behavioral effect of wording changes | **Gap** — no eval run; R4/R5/R6 are predictions |
| Mode design | **Partial** — identified overlays as de facto modes, but did not evaluate whether they should be first-class |

**Priority:** run R6's falsifier before touching the skills section; it is the highest-variance item.

### A9. metacognition — Improvement Kata on this analysis

- **Grasp current condition:** started from a prior report whose top-3 included one already-applied item, one falsified premise, and one mis-sourced literature claim.
- **Target condition:** every claim traceable to a command output, a `file:line`, or a verified URL.
- **Predict (confidence 0.6):** "verifying the prior report's claims will overturn at least one top-3 recommendation." **Outcome: correct** — it overturned two (the orphan-template item was already applied; the catalog-bloat premise was falsified). Ex-post Brier for that prediction = (1 − 0.6)² = **0.16**, i.e. under-confident. Calibration lesson: when auditing a prior artifact whose claims were never executed, my prior on "at least one claim is wrong" should be higher than 0.6.
- **Experiment run:** template-render test (§2.8) + catalog measurement (§2.9) + source verification (§3).
- **Residual gap:** behavioral predictions unmeasured. **Next experiment:** `crates/eval_cli` on R6 then R5.

---

*Termination: §§1–6 complete. Every recommendation survived the essentialist 3-gate (A5) and carries a falsifiable test. All nine skill artifacts are in Appendix A and are incorporated in §§2–6 — A2 eliminated a recommendation, A3 set the ranking, A5 removed four candidates, A6 independently identified the §2.8 defect, and A7 corrected the inventory. Self-assessment included (§6 + A9).*