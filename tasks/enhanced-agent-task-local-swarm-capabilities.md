# Task: zed-kask Local Swarm Agent — Capabilities & Differentiator Analysis

## Role
You are a capabilities analyst grounded in the zed-kask codebase. You will
orchestrate six skills in a defined order to produce a research/capabilities
analysis report on the potential of local swarm agents in zed-kask and their
differentiators versus cloud swarm agents (ABW), agentic frameworks
(LangChain, CrewAI), and agentic startups (Ninjatech AI).

## Skill Composition (run in this order; outputs feed forward)

1. **pragmatic-semantics** — Classify every capability claim as IS (verified
   in code) vs OUGHT (aspirational/design intent). Tag each claim with
   provenance: file:line for IS, doc/ADR/issue for OUGHT. Reject any claim
   you cannot ground — say "not verified" rather than asserting.

2. **pragmatic-cybernetics** — Model the local-agent feedback loop:
   sense (telemetry, reg.* spans, memory reads) → orient (skill cascade,
   metacognition) → decide (swarm-intelligence plan) → act (swarm_delegate_local,
   tool dispatch) → check (outcome spans, Brier). Identify which loop
   properties (polarity, delay, gain, closure, fidelity) differ between
   local and cloud agents.

3. **sequential-inquiry** — Drive the research as a convergent chain:
   grasp current understanding (read code) → establish target (the
   differentiator questions below) → delegate deep-dives → measure gap.
   Use this as the outer loop; the other skills are delegates.

4. **metacognition** — At each major finding, run a self-reflection pass:
   what is your confidence (Brier-calibrated)? what is the obstacle? what
   prediction did you make about the codebase that the code confirmed or
   refuted? Record calibration, not just conclusions.

5. **mcda** — For the "how do we build on the local-agent advantages"
   question, run a multi-criteria decision analysis: criteria = (leverage
   on zed-kask seam, uniqueness vs competitors, build-cost, risk),
   weighted, with sensitivity analysis. Rank 3-5 build-on-it options.

6. **grill-me** — Before finalizing, self-interrogate the report across
   Recall → Mechanism → Rationale → Edge Cases → Synthesis. The grill is
   decoupled: challenge your own claims as a skeptic, do not defend them.

## Research Questions (each must be answered with file:line grounding where possible)

### R1. Local-vs-cloud capability differential
What capabilities do local swarm agents (those running in the zed-kask
process, dispatched via `swarm_delegate_local`) gain by being local to
zed-kask that cloud swarm agents (ABW REST, `swarm_delegate`) do not have?
Ground each capability in code — e.g., direct `Entity` handles, GPUI
foreground access, in-process `DelegationToken` (no network hop),
shared `LanguageModelRegistry`, `McpRuntime` governed dispatch, the
`ContextServerStore` per-project scoping. For each, state the cloud-agent
equivalent and why it is weaker or absent.

### R2. Local-agent advantages and how to build on them
From R1, synthesize the advantage set. Then propose 3-5 concrete ways to
*build on* those advantages (not just enumerate them). Each proposal must
name the crate/file it would touch and the capability it amplifies. Use
mcda here.

### R3. Semantic and episodic memory confirmation
Confirm whether local agents are building their own semantic and episodic
memory. Ground this in the memory subsystem — grep for the memory port,
the condenser (`set_thread_condenser`), `set_memory_port`, the corpus
store, embedding store, and any per-agent or per-swarm memory namespace.
Answer: (a) is semantic memory being written? where? (b) is episodic
memory being written? where? (c) is the memory scoped per-agent,
per-swarm, or global? (d) is there a gap between OUGHT (docs say agents
have memory) and IS (code actually writes it)? Cite the enforcement point
(the write call), not just the port declaration — per the
"Advertised invariants need enforcement points" rule.

### R4. Skill awareness and access
Verify that local agents have access to and can invoke the zed-kask skill
corpus (the 42+ skills in `.agents/skills/`). Ground this in the skill
invocation path: `SkillTool::run`, `NativeAgent::send_skill_invocation`,
the `skill` tool, the manifest executor, the FlowDef cascade. Answer:
(a) can a local swarm agent invoke a skill? via what tool? (b) does the
agent's context include the skill catalog (descriptions)? where? (c) is
there a gap between "agent has access" and "agent understands what each
skill is for"? how would an agent learn skill purpose — from the catalog
`description`, from `SKILL.md`, or from the manifest?

### R5. MCP tool awareness and curator-as-tool
Verify local agents are aware of MCP tools and that the curator agent is
itself exposed as an MCP tool available to swarm agents. Ground this in:
the `McpRuntime` (app-global, governed), the `ContextServerStore`
(per-project), `KaskMcpDescriptor`, `sync_kask_mcp_servers`, the
`hkask-mcp-*` server binaries, and any curator-exposing MCP server (grep
for curator surfaces). Answer: (a) which MCP tools can a local swarm
agent call? (b) is the curator exposed as an MCP tool? name the server
and the tool. (c) what is the governance membrane on local-agent MCP
calls vs cloud-agent MCP calls?

### R6. Competitive differentiator analysis
Compare the zed-kask local-agent system against:
- **ABW** (cloud swarm agents) — what does zed-kask local gain by being
  in-process?
- **LangChain** and **CrewAI** (agentic frameworks) — what does zed-kask
  gain by being embedded in an editor (GPUI) with a real skill registry
  vs a generic framework?
- **Ninjatech AI** (agentic startup) — what does zed-kask gain from its
  OCAP/governance/Regulation layer vs a startup without that substrate?

For each competitor, state the differentiator as IS (verified capability
zed-kask has) vs the competitor's documented limitation. Do not fabricate
competitor capabilities — if you don't know a competitor's architecture,
say so and mark the comparison as inference.

## Output Structure

Produce a single report with these sections, in order:

1. **Executive Summary** — 3-5 sentences, the headline differentiator.
2. **Method** — which skills ran, in what order, what was grounded vs inferred.
3. **R1–R6 Findings** — one subsection per research question, each with
   file:line citations for IS claims and explicit "OUGHT, not verified"
   labels for ungrounded claims.
4. **MCDA: Build-on-it Options** — the ranked table with weights, scores,
   sensitivity analysis.
5. **Metacognition Log** — your Brier-calibrated confidence per finding,
   predictions made and confirmed/refuted, residual obstacles.
6. **Grill-Me Verdict** — the skeptic's strongest objection to the report
   and your response (or concession).
7. **Gaps & Follow-ups** — what you could not verify, what needs a human
   or a code change to confirm.

## Constraints

- **No fabrication.** Every capability claim is either grounded in
  file:line or explicitly labeled OUGHT/inference. Per pragmatic-semantics,
  never present an OUGHT as an IS.
- **Codebase first.** Before answering any R-question, grep the relevant
  crates (`kask*`, `swarm_panel`, `agent`, `hkask-*`, `hkask-mcp-*`).
  Do not answer from prior knowledge of "how agent systems usually work."
- **Enforcement points, not declarations.** Per the .rules trap, a port
  or doc comment is not a capability — the write call / dispatch site is.
  Cite the enforcement point.
- **Bounded scope.** Do not expand into "and here's how to redesign the
  whole system." R2 is the only forward-looking section; the rest are
  descriptive.
- **Competitor honesty.** For ABW/LangChain/CrewAI/Ninjatech, mark
  inferred claims as inference. If a competitor's architecture is unknown
  to you, say "not verified — requires external research" rather than
  guessing.

## Acceptance Criteria

The report is complete when:
- [ ] All six skills ran and their outputs are visible in the report
      (method section + per-finding citations).
- [ ] Every IS claim has a file:line citation; every OUGHT claim is labeled.
- [ ] R3 (memory) cites the actual write/enforcement point, not just the
      port/condenser declaration.
- [ ] R4 (skills) and R5 (MCP/curator) cite the invocation/dispatch path.
- [ ] R6 marks competitor claims as verified or inference — no fabricated
      competitor architecture.
- [ ] MCDA table is present with weights, scores, and sensitivity analysis.
- [ ] Metacognition log reports Brier-style confidence per finding.
- [ ] Grill-me verdict names the strongest objection, not a softball.
- [ ] Gaps section lists what could not be verified.
