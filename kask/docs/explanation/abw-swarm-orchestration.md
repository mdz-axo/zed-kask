---
title: "Agent Bestiary World — Swarm Interaction and Agentic Orchestration"
audience: [developers, architects, operators]
last_updated: 2026-08-04
version: "0.36.0"
status: "Active"
domain: "Composition"
mds_categories: [composition, trust, lifecycle, curation]
---

# Agent Bestiary World — Swarm Interaction and Agentic Orchestration

**Diataxis type:** Explanation

This document explains *how zed-kask orchestrates agent swarms through Agent
Bestiary World (ABW)* — the concepts, the moving parts, and why they're shaped
the way they are. For tool-by-tool reference see
[the swarm MCP server reference](../reference/mcp-servers/swarm.md); for the
full design rationale see
[the integration plan](../plans/abw-swarm-intelligence.md).

## The mental model

ABW is a hosted **ecology of AI agents**: a catalogue of specialized agents
(research, creative, OSINT, meta), a credit-based economy, and **workspaces** —
containers where you "hire" agents so they collaborate with shared knowledge,
git-backed files, and real-time chat. A **swarm** in zed-kask's vocabulary is an
ABW workspace. A **compound agent** is an ABW agent that orchestrates others
(it delegates sub-tasks to specialists).

zed-kask adds three things on top of ABW's web UI:

1. **A governed substrate** — the `hkask-mcp-swarm` MCP server, which proxies
   ABW's REST API through the kask MCP runtime (per-agent call metering, gas
   budgeting, Regulation spans). What an agent may reach is set by its card's
   `mcp_tools` allowlist, checked before dispatch; the runtime itself does not
   re-authorize (RR-0056).
2. **A management surface** — the Agent Swarm panel, a center-pane `Item` for
   browsing, authoring, composing, and steering swarms.
3. **A decision process** — the `swarm-intelligence` skill, a PDCA loop that
   reasons about *how* to compose a swarm toward a target condition.

The key architectural claim: **ABW agents are leaf tools from zed-kask's
perspective.** ABW's own delegation is one level deep (a compound agent can
delegate to a specialist, but the specialist cannot delegate further). zed-kask
does not try to map its skill cascade onto ABW workspaces — instead, each ABW
agent/compound-agent is a single callable tool, and zed-kask's own cascade (the
`swarm-intelligence` skill, the panel, the kask agent) does the
meta-orchestration. This respects both systems' invariants.[^reynolds-flocking]

## The four modes of the Agent Swarm panel

The panel (View → Agent Swarm, or the status-bar button) has four modes:[^mcp-spec]

| Mode | Surface | What you do |
|---|---|---|
| **Browse** | Discovery / sharing | Search and filter the ABW catalogue (agents) and your workspaces (swarms) as cards. The sharing surface — published Apps are reusable team manifests. |
| **Author** | Agent creation | Fill a form (name, description, multi-line system prompt) → `swarm_create_agent`. The agent appears in your ABW library as a draft. |
| **Compose** | Team building | Name a swarm, give it a mission, list agents to hire → `swarm_create_swarm`. Includes a **★ Xaman Ek** consultant box that plans the team for you. |
| **Steer** | Live orchestration | A `ConversationView` scoped to the swarm server. You ask the curator to compose/steer; it invokes the `swarm-intelligence` skill cascade. |

## Authoring: from intent to agent

Authoring an agent is a three-step pipeline, each step a tool:[^mcp-spec]

1. **`swarm_generate_prompt`** — draft a system prompt from a natural-language
   description of what the agent should do.
2. **`swarm_generate_ontology`** — draft a seed ontology (an entity-relationship
   model, rendered as a Mermaid diagram) for the agent's knowledge domain. The
   ontology is what the agent *remembers* and how its knowledge connects.
3. **`swarm_create_agent`** — assemble the full agent card (name, system prompt,
   model, temperature, tags, sample queries) and create it. For a **compound
   agent**, declare `dependencies` (required/optional sub-agents) — ABW
   auto-hires the team when the compound agent is hired into a workspace.

## Composition: from agents to swarm

Composition has two paths:

**Direct (the Compose form):** name + mission + a list of agent names →
`swarm_create_swarm`. The server creates the workspace (free) and hires each
agent — each hire is individually consent-gated (see below).

**Curated (Xaman Ek):** Xaman Ek is ABW's platform curator — a sessioned
navigator that knows every agent and composition pattern. In a
`composition_design` session you describe the team's goal; Xaman Ek recommends
agents, checks I/O compatibility between them, and flags valence homophily (a
team that all thinks alike). The panel's Compose surface calls `swarm_xaman`,
shows the recommendation, and offers a one-click "Use team" pre-fill. A
composition session can then be materialized into a reusable **App** via
`swarm_create_app` — the sharing unit.[^reynolds-flocking-sep]

## The cost/consent gate (why every spend needs a token)

ABW charges credits for actions: hiring an agent (5 cr), a delegation (1 cr +
tokens), execution (token fees). Because an *agent* (not just a human) can call
these tools, zed-kask inserts a **consent gate** so an agent can't spend money
without the operator explicitly authorizing it.[^ocap-miller]

The gate is a single-use, action-scoped, target-scoped token:

1. The panel (or operator) calls `swarm_hire_cost` to get a **pre-flight
   estimate** — what this hire will actually cost, from ABW's dependencies
   endpoint.
2. The operator confirms; the panel calls `swarm_request_consent`, which **mints
   a token** recording "the operator authorizes up to N credits for action X on
   target Y."
3. The spend tool (`swarm_hire`/`swarm_delegate`) **consumes** the token. It
   verifies the token is for *this exact* action and target, and **re-fetches
   the real cost from ABW** to confirm it hasn't exceeded the authorized
   ceiling. Only then does it spend.

The token is single-use (no replay), scope-bound (a token for agent A can't
hire agent B), and the mint is auth-gated (a prompt-injected agent can't
self-authorize). This is the enforcement point — the spend *refuses* without a
valid token, per the `.rules` "advertised invariants need enforcement points"
trap.

## The algedonic channel: you always see the wallet

In cybernetic terms, the credit balance is the **algedonic signal** — the pain
signal that bypasses normal channels to reach the operator directly. Every
authenticated tool response carries `wallet.balance`, so a spend is never out of
sight. A failed balance query emits a warning and returns "unknown" — never a
fabricated zero, because a measured-zero and a failed-to-measure are opposite
truths.[^beer-heart]

## The `swarm-intelligence` skill: reasoning about composition

The panel and server are the *substrate*. The `swarm-intelligence` skill is the
*decision process* that acts on it — a SENSE → ORIENT → DECIDE → ACT → CHECK →
CONVERGE loop:[^pso-kennedy][^aco-dorigo]

- **SENSE** the current swarm state against the Onto4MAT multi-agent teaming
  ontology (alignment, cohesion, separation) and the ABW workspace/wallet APIs.
- **ORIENT** by classifying the gap (variety deficit, coherence deficit,
  loop-break) via Ashby's requisite variety and PSO cognitive/social balance.
- **DECIDE** composition adjustments isomorphic to PSO velocity tuning, ACO
  pheromone deposition, and Reynolds separation/alignment/cohesion.
- **ACT** via gated `swarm_update_swarm`/`swarm_delegate` calls.
- **CHECK** spend and curator data-sharing against the algedonic channel.
- **CONVERGE** via a Cauchy criterion on the swarm-state distance metric.

You invoke it from **Steer mode**: ask the curator to "compose a research team
for X" or "rebalance this swarm toward Y," and the skill cascade runs.

## Trust boundaries

Three boundaries shape the design:[^ocap-miller-trust]

- **ABW output is untrusted.** ABW agents and the curator are third-party LLMs.
  All output is sanitized (`sanitize_abw_response`) and wrapped in a
  `{content, source: "abw", trust: "untrusted"}` container before it reaches
  the zed-kask agent — closing the prompt-injection → unauthorized-spend chain.
- **The API key is a credential, not config.** It lives in the keychain and is
  injected only into this server's process — never other kask servers, never
  the config env map.
- **Execution is owner-funded.** Running an ABW agent draws on the agent
  *owner's* configured LLM key, not the caller's credits alone — surfaced as a
  typed `AgentNotFunded` error, not masked.

## Cross-links

- [Swarm MCP Server Reference](../reference/mcp-servers/swarm.md) — the 52 tools (27 ABW + 25 local), dual mode
- [Integration plan](../plans/abw-swarm-intelligence.md) — full design + API surface
- [Cybernetic Swarm Plan](../plans/cybernetic-swarm-plan.md) — the swarm-intelligence skill design, C0–C8 components, steering modes

## Footnotes

[^reynolds-flocking]: Reynolds, C. W. (1987). Flocks, herds and schools: A distributed behavioral model. *ACM SIGGRAPH Computer Graphics*, 21(4), 25–34. https://doi.org/10.1145/37402.37406
    Cited for the separation/alignment/cohesion model underlying the leaf-tool architectural claim — ABW agents flock as peers, not as a recursive delegation tree.

[^mcp-spec]: Anthropic. (2024). *Model Context Protocol Specification*. Anthropic PBC. https://modelcontextprotocol.io/specification
    Cited for the MCP tool-dispatch protocol the Agent Swarm panel modes operate over.

[^reynolds-flocking-sep]: Reynolds, C. W. (1987). Flocks, herds and schools: A distributed behavioral model. *ACM SIGGRAPH Computer Graphics*, 21(4), 25–34. https://doi.org/10.1145/37402.37406
    Cited for the valence-homophily check (separation heuristic) that flags a team where all agents think alike.

[^ocap-miller]: Miller, M. S. (2006). *Robust Composition: Towards a Unified Approach to Access Control and Concurrency Control* (Doctoral dissertation, Johns Hopkins University). http://www.erights.org/talks/thesis/markm-thesis.pdf
    Cited for the object-capability principle that a delegated agent must not self-authorize a spend — authority only attenuates, never amplifies.

[^beer-heart]: Beer, S. (1979). *The Heart of Enterprise*. John Wiley & Sons.
    Cited for the algedonic-signal concept (cybernetic pain channel that bypasses normal feedback loops) underlying the wallet-balance visibility design.

[^pso-kennedy]: Kennedy, J., & Eberhart, R. (1995). Particle Swarm Optimization. *Proceedings of IEEE International Conference on Neural Networks*, 1942–1948. https://doi.org/10.1109/ICNN.1995.488968
    Cited for the PSO velocity-tuning metaphor the DECIDE step uses to propose composition adjustments.

[^aco-dorigo]: Dorigo, M., & Stützle, T. (2004). *Ant Colony Optimization*. MIT Press. https://mitpress.mit.edu/9780262042192/
    Cited for the ACO pheromone-deposition metaphor the DECIDE step uses to reward successful agent compositions.

[^ocap-miller-trust]: Miller, M. S. (2006). *Robust Composition: Towards a Unified Approach to Access Control and Concurrency Control* (Doctoral dissertation, Johns Hopkins University). http://www.erights.org/talks/thesis/markm-thesis.pdf
    Cited for the object-capability trust-boundary design: untrusted ABW output is data, not authority, and credentials are scoped per-server.
