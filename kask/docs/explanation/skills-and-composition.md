---
title: "Skills and Composition"
audience: [developers, operators, users]
last_updated: 2026-08-20
version: "0.37.0"
status: "Active"
domain: "Skill System"
mds_categories: [domain, composition, lifecycle, trust]
---

# Skills and Composition

Design, invoke, audit, publish, and compose hKask skills. Skills execute via **upstream Zed body injection**: `SkillTool::run` (`crates/agent/src/tools/skill_tool.rs:266`) reads the `SKILL.md` body from disk and injects it into the agent's context via `render_skill_envelope`. The model reads the body and follows the instructions. The agent is the executor — there is no `ManifestExecutor`, no `StepMachine`, no PDCA cascade machinery. The prior manifest-driven cascade model (`hkask-templates`, `registry/manifests/`, `FlowDef`) was deleted (commit `5f4cf5f10d`).[^anthropic-skills]

This guide also covers building MCP servers that provide tool surfaces for skills and agents — in zed-kask, MCP servers register as builtins inside the editor and are launched as child processes over stdio by zed's `context_server` host (D3); the standalone `kask mcp start <id>` CLI is deleted.

---

## Skill Anatomy

A skill is a directory under `.agents/skills/<name>/` (repo root, not under `kask/`) containing a `SKILL.md` file:

```
.agents/skills/my-skill/
└── SKILL.md          ← YAML frontmatter + markdown body (process instructions)
```

- **`SKILL.md`** has YAML frontmatter (`name`, `description`, and optional metadata) and a markdown body. The body is the process instructions the model reads and follows when the skill is invoked. This is the source of truth — there is no derived manifest.
- **Template crates** under `kask/registry/templates/<name>/` are optional companion resources. A skill body may instruct the model to call the `render_template` tool to render a Jinja2 template from a template crate. The template crate is not required for skill execution — it is a resource the skill body may reference.

### The Body-Injection Model

When the agent invokes the `skill` tool with a skill name:

1. `SkillTool::run` (`crates/agent/src/tools/skill_tool.rs:172`) receives the skill name from `SkillToolInput`.
2. It resolves the skill directory and reads the `SKILL.md` body from disk.
3. It calls `render_skill_envelope(&skill, &body)` (`skill_tool.rs:47`), which wraps the body in a structured envelope.
4. The envelope is returned to the agent as the tool result (`SkillToolOutput::Found { rendered }`).
5. The agent reads the envelope content (the skill body) and follows the instructions — calling `lisp_eval` for deterministic computation, `render_template` for structured prompt scaffolding, and MCP tools for external capabilities.

There is no cascade, no convergence loop, no gas budget, no `StepMachine`. The model is the executor. Convergence is the model's judgment, optionally checked by `lisp_eval` when the skill body instructs it.

### Two Supporting Tools

| Tool | Location | Purpose |
|------|----------|---------|
| `lisp_eval` | `crates/agent/src/tools/lisp_eval_tool.rs` | Sandboxed Lisp interpreter (`hkask_lisp::eval_sandboxed_with_budget`). No I/O, no `eval`, no network. Bounded by `max_steps` (default 100000) and `max_depth` (default 64). The model calls it when a SKILL.md instructs deterministic computation (convergence signals, invariant checks, scoring). |
| `render_template` | `crates/agent/src/tools/render_template_tool.rs` | Renders Jinja2 templates from `kask/registry/templates/` using `minijinja`. Strips YAML frontmatter. Path traversal protection via `canonicalize` + `starts_with` check. Template base path wired via `agent::set_template_base_path()` (OnceLock) in `crates/zed/src/main.rs:776`. |

### PDCA Loops Are Model-Coordinated

A skill body may describe a PDCA (Plan-Do-Check-Act) loop with convergence criteria. The model self-iterates: it reads the instructions, performs the plan step, calls `lisp_eval` to check convergence, and loops until the convergence criterion is met or the model judges the task complete. There is no runtime that drives the loop — the SKILL.md body describes the convergence criteria; the model coordinates the iteration using `lisp_eval` for deterministic checks and `render_template` for structured prompt scaffolding.

This is the "model-coordinated PDCA" pattern: the skill body is the process specification, the model is the executor, `lisp_eval` is the deterministic oracle, and `render_template` is the scaffolding tool.

---

## Listing and Checking Skills

Skill listing, status, and auditing are performed in-process through the zed-kask agent panel or the skill maintenance tooling. The former `kask skill list`, `kask skill status`, and `kask skill audit` standalone CLI commands have been removed.[^fagan-skill-audit]

### List Available Skills

Invoke the skill-listing surface from the agent panel. The output shows the skill directory layout with name, description, and namespace:

```
  .agents/skills/:
    coding-guidelines     description="Enforce Karpathy's four coding principles"
    diagnose              description="Disciplined diagnosis loop"
    ...
```

### Skill Auditing

Run a dual-layer audit to check skill health through the skill maintenance tooling or agent panel. The audit checks:
- `SKILL.md` presence and frontmatter validity
- Template crate existence (if the skill body references `render_template`)
- Content consistency between SKILL.md and any companion template crates

---

## Designing a Skill

### Writing a `SKILL.md`

Create `.agents/skills/my-skill/SKILL.md`:

```markdown
---
name: my-skill
description: A custom skill for automated code review
---

# My Skill

This skill performs an automated code review using a PDCA cycle:
- **Plan:** Analyze the code structure and identify review targets
- **Do:** Execute the review using available tools
- **Check:** Validate findings against quality criteria (use `lisp_eval` to check invariants)
- **Act:** Produce a review report with recommendations

## When to Use

Use this skill when reviewing Rust code for idiomatic patterns and correctness.

## Process

1. Read the target file(s) using `read_file`.
2. Identify review targets (functions, types, modules).
3. For each target, check against the criteria below.
4. Use `lisp_eval` to verify structural invariants (e.g., function count, complexity thresholds).
5. Produce a structured report with findings and recommendations.

## Convergence

The skill is complete when all identified targets have been reviewed and the `lisp_eval` invariant check passes.
```

The `description` field in the frontmatter is what the agent sees in the skill catalog (preloaded into the system prompt). The body is injected only when the skill is invoked — this is progressive disclosure.[^anthropic-skills]

### Writing Templates (`.j2` Files)

Templates are optional Jinja2 files rendered with context variables at invocation time via the `render_template` tool. A skill body may instruct the model to call `render_template` with a template path and context variables:

```jinja2
{# registry/templates/my-skill/plan.j2 #}
You are executing the "my-skill" skill. This is the PLAN phase.

Context: {{ context }}

Based on the context above, develop a structured plan for achieving the goal.
Consider:
1. What information is needed
2. What tools should be used
3. What intermediate outputs are required

Return your plan as a numbered list.
```

The model calls `render_template(template_path="my-skill/plan.j2", context={...})` and receives the rendered text. The template crate at `kask/registry/templates/my-skill/` is the companion resource; the `SKILL.md` body is the source of truth for the skill's process.

### Context Variables

The `render_template` tool accepts a `context` map. The skill body instructs the model on what variables to pass. There are no automatically-injected context variables — the model constructs the context from its current state and prior tool results.

---

## Testing a Skill Locally

### Step 1: Verify Discovery

List skills through the agent panel. Your skill should appear in the list.

### Step 2: Invoke from the Agent Panel

Open the zed-kask agent panel and invoke the skill:

```
/skill my-skill "Review the authentication module in src/auth.rs"
```

The agent panel routes this through `SkillTool::run` (D1), which:
1. Resolves the skill directory
2. Reads the `SKILL.md` body
3. Calls `render_skill_envelope(&skill, &body)` (`skill_tool.rs:47`)
4. Returns the envelope to the agent
5. The agent reads the body and follows the instructions

---

## Invoking Skills

Skills are invoked in-process through `SkillTool::run` (`crates/agent/src/tools/skill_tool.rs:172`), which reads the `SKILL.md` body and injects it via `render_skill_envelope`. The former manifest-cascade model (`BridgeManifestExecutor`, `ManifestExecutor`, `StepMachine`) was deleted (commit `5f4cf5f10d`).[^mcp-spec-skill-invoke]

### Via the Agent Panel

Open the zed-kask agent panel and invoke a skill:

```
/skill diagnose "My application crashes on startup"
```

The agent panel routes this through the `skill` tool, which calls `SkillTool::run` directly in-process.

### What Happens During Execution

When a skill is invoked in-process:

1. **Lookup** — The skill name is resolved against the loaded skill catalog (from `agent_skills`). The `SkillTool` reads the `SKILL.md` body from disk.
2. **Envelope rendering** — `render_skill_envelope(&skill, &body)` (`skill_tool.rs:47`) wraps the body in a structured envelope.
3. **Return to agent** — The envelope is returned as `SkillToolOutput::Found { rendered }` (`skill_tool.rs:268`).
4. **Agent follows instructions** — The agent reads the envelope content (the skill body) and follows the instructions — calling `lisp_eval` for deterministic computation, `render_template` for structured prompt scaffolding, and MCP tools for external capabilities.
5. **Regulation span** — `reg.tool.skill_execute` is emitted with the skill ID and result.

### Convergence (Model-Coordinated)

A skill body may describe convergence criteria. The model self-iterates:
1. Performs the plan step (may call `render_template` for scaffolding)
2. Performs the do step (may call MCP tools)
3. Performs the check step (may call `lisp_eval` for deterministic invariant checks)
4. If convergence is not reached, loops back to plan with refined context
5. If convergence is reached, produces the final output

The convergence signal is typically produced by a `lisp_eval` call that deterministically computes a gap score from the model's output. The model reads the score and decides whether to iterate.

### Composition Principles for Skill Design

Five principles discovered through the co-evolution of skills and MCP tools. Apply these when designing the process instructions in a SKILL.md body.

#### 1. The Determinism Frontier

Every skill has a boundary between deterministic steps (output fully determined by inputs) and probabilistic steps (LLM exercises judgment). Push as much work as possible to the deterministic side.

- Use `lisp_eval` for math, invariant checks, convergence signals.
- Use MCP tool calls (via the agent's tool-use loop) for data retrieval with deterministic inputs.
- Use LLM judgment only for steps that require synthesis, reasoning, classification, or prediction.

The test: "Could a deterministic function produce this output from these inputs?" If yes, it should be `lisp_eval` or a direct tool call, not LLM judgment.

#### 2. Persistence-Grounded Learning

Every skill that produces forecasts, analyses, or recommendations should read its own prior outputs from MCP persistence before starting. This closes the feedback loop: the skill's current invocation is informed by its past performance.

The pattern: the skill body instructs the model to call the relevant MCP tool (e.g., `scenario_calibration`) at the start of the process to read prior runs, then thread the results into the first reasoning step.

#### 3. Failure Surfacing

Every MCP tool call the skill instructs should have a failure path. The skill body should instruct the model to report failures to the Curator (via `curator_report_skill_use_issue`) before escalating. Without this, a failed tool call silently propagates and the operator sees no context.

#### 4. The Lisp Scaffold Pattern

When an LLM step produces structured output with invariant properties (count, completeness, diversity, mutual exclusivity), follow it with a `lisp_eval` call that checks those invariants deterministically. The Lisp step's output (defect list or gap score) feeds the convergence signal.

Pattern: LLM generates → `lisp_eval` checks → LLM repairs (on next iteration).

#### 5. The Co-Evolution Loop

Skills and MCP tools evolve together. Skills reveal MCP tool design issues (missing inputs, confusing schemas) via failure reports. The Curator reads skill-use reports and issues `EvolveMcpToolSchema` directives. MCP tools gain new capabilities that skills should adopt.

See [`skill-mcp-integration.md`](skill-mcp-integration.md) §Co-Evolution Patterns for the three feedback loops.

### Gas Consumption

Skill execution is bounded by the **per-agent call cap** (System A): every governed MCP tool call via `McpRuntime::invoke` charges one call against the agent's `CallCap` (`CallCapManager::charge_metered` → `CallMeterOutcome`). The cap resets to its ceiling each regulation tick. An agent with no registered cap is **auto-registered** at `DEFAULT_RUNAWAY_CALL_CEILING` (10 000) and the wiring gap is logged — a missing seed is a wiring omission, not an authorization decision (RR-0057).

There is no per-cascade gas budget or rJoule tracking — the prior `BudgetTracker` and `gas`/`rjoule` manifest fields were deleted with the `hkask-templates` crate. Tool-call bounding is solely the per-agent `CallCap`.

Gas/cost consumption is observable via Regulation spans. Query the in-process Regulation span surface (agent panel) and look for `reg.tool.invoked` (pre-invocation) and `reg.tool.completed` (post-invocation).

### Error Handling

| Error | Cause | Resolution |
|-------|-------|------------|
| `Skill 'X' not found` | Skill name not in the loaded catalog | List skills through the agent panel to see available names; ensure zed-kask was launched from the project root containing `.agents/skills/` |
| `Inference failed` | Inference port error | Check inference backend configuration via zed-kask's `CredentialsProvider` (D9); ensure the provider API key is set |
| `lisp_eval` error | Lisp evaluation exceeded budget or depth | Check the Lisp form for infinite recursion or excessive steps; increase `max_steps` if needed |
| `render_template` error | Template not found or Jinja2 syntax error | Verify the template path exists under `kask/registry/templates/`; validate Jinja2 syntax |

---

## Composing Skill Bundles

Bundle composition is driven by the **skill-bundler** skill. The former `BundleService` in the deleted `hkask-services-skill` crate and the `kask bundle compose/list/show/apply/evolve/skills/off` CLI commands have been removed.[^ousterhout-bundle]

### Creating a Bundle

Invoke the skill-bundler skill from the agent panel with the skills to compose:

```
skill: skill-bundler
skills: coding-guidelines,idiomatic-rust
name: rust-review-bundle
```

The skill-bundler performs inference-driven analysis to produce a coordinated bundle manifest.

### Bundle Management

Bundle management (list, show, apply, evolve) is performed in-process through the agent panel. The former `kask bundle list/show/apply/evolve/skills/off` CLI commands have been removed. Bundles are session-scoped: applying a bundle activates its composition for the current agent session; deactivating is a no-op since bundles do not persist beyond the session.

---

## Skill Routing and Discovery

Two meta-skills govern how tasks find the right skills: **skill-router** matches tasks to installed skills, and **skill-discovery** acquires new skills when gaps are found. They compose in a feedback loop.[^beer-feedback-loop]

### How It Works

```
task-breakdown (decompose)
  → emits skill_match_query per slice
    → skill-router (match)
      → full coverage → ranked recommendations with invocation hints
      → partial/none → uncovered_capabilities
        → skill-discovery (detect-gap → search → evaluate → install)
          → new skill installed → catalog grows → router has better coverage
```

### skill-router

Given a task description and the installed skill catalog, skill-router scores each skill 0.0–1.0 on three dimensions:

| Dimension | Weight | What it measures |
|-----------|--------|------------------|
| Capability overlap | 0.50 | Does the skill description cover the task core need? |
| Lexicon alignment | 0.25 | Do task verbs/nouns overlap with the skill's lexicon terms? |
| Trigger alignment | 0.25 | Does the task match the skill When-to-Use conditions? |

Coverage assessment: **full** (fit >= 0.80), **partial** (0.40-0.79), **none** (< 0.40). Partial/none emits `uncovered_capabilities` as gap signals for skill-discovery.

### skill-discovery

Four-phase pipeline: **detect-gap** (classify gaps: coverage, feature, automation, knowledge, governance, quality) → **search** (rank catalog candidates by fit) → **evaluate** (score format/quality/safety) → **convergence-check** (is the gap resolved?).

### Regulation Spans

| Span | When emitted |
|------|-------------|
| `reg.skill.routing.matched` | skill-router produces a ranked recommendation |
| `reg.skill.routing.uncovered` | skill-router finds no matching skill (gap signal) |
| `reg.skill.discovery.gap_detected` | skill-discovery classifies a capability gap |
| `reg.skill.discovery.searched` | skill-discovery searches the catalog for candidates |
| `reg.skill.discovery.evaluated` | skill-discovery scores a candidate skill |

---

## Building MCP Servers

zed-kask hosts 10 MCP servers as child processes over stdio via zed's `context_server` host (companies, corpus, curator, kata-kanban, portfolio, prediction-markets, research, scenarios, swarm, training). Every server follows the same bootstrap pattern defined in `hkask-mcp-server`. In zed-kask, MCP servers register as built-in context servers inside the editor (D1–D3): the `context_server` host launches them as child processes over stdio, and servers run standalone with identity from `ServerContext.webid` (resolved from `HKASK_WEBID`) — there is no `KaskCore` singleton (the composition root wires individual components directly; see `zed-host-architecture-plan.md` §13.3). The former `kask mcp start <id>` CLI and the old per-crate `BUILTIN_SERVERS` tuple registry have been superseded by in-process registration against the canonical `kask_bridge::BUILT_IN_MCP_SERVERS` list.[^mcp-spec-build][^ousterhout-mcp-build]

### Prerequisites

- zed-kask source tree with `crates/hkask-mcp-server/` built
- A new crate under `mcp-servers/` named `<your-mcp-package>`
- Familiarity with the `rmcp` crate (the MCP protocol library hKask uses)

Add to your new crate's `Cargo.toml`:

```toml
[dependencies]
hkask-mcp-server = { path = "../../crates/hkask-mcp-server" }
hkask-types = { path = "../../crates/hkask-types" }
hkask-inference = { path = "../../crates/hkask-inference" }  # if you need inference
rmcp = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
dotenvy = { workspace = true }
```

### Step 1: Define the Server Struct

Use the `mcp_server!` macro from `hkask-mcp-server`. It generates the struct with a mandatory `webid` field plus your domain-specific fields, along with a `new()` constructor and a `ToolContext` implementation.

```rust
// mcp-servers/<your-mcp-package>/src/lib.rs

use hkask_mcp_server::mcp_server;
use std::sync::Arc;
use hkask_types::InferencePort;

mcp_server! {
    /// Example MCP server — demonstrates the bootstrap pattern.
    pub struct ExampleServer {
        /// Optional inference port for LLM calls.
        inference_port: Option<Arc<dyn InferencePort>>,
        /// Your domain-specific state.
        items: std::collections::HashMap<String, String>,
    }
}
```

### Step 2: Define Tool Methods

Annotate methods with `#[tool(description = "...")]` and use `execute_tool` for Regulation span emission:

```rust
use hkask_mcp_server::server::execute_tool;
use rmcp::tool;

#[tool(description = "Liveness check")]
async fn example_ping(&self) -> String {
    execute_tool(self, "example_ping", async {
        Ok(serde_json::json!({
            "status": "ok",
            "server": "example",
        }))
    }).await
}
```

### Step 3: Apply the `tool_router` Macro

Use rmcp's `#[tool_router(server_handler)]` attribute on the `impl` block that contains your `#[tool]`-annotated methods.

```rust
use rmcp::tool_router;

#[tool_router(server_handler)]
impl ExampleServer {
    #[tool(description = "Liveness check")]
    pub async fn example_ping(&self) -> String {
        execute_tool(self, "example_ping", async {
            Ok(serde_json::json!({"status": "ok", "server": "example"}))
        }).await
    }
}
```

### Step 4: Write the `run()` Function

Every hKask MCP server has a `run()` function that calls `run_server()` with a factory closure:

```rust
use hkask_mcp_server::{McpError, run_server, ServerContext};

pub async fn run() -> Result<(), McpError> {
    run_server(
        "example",
        env!("CARGO_PKG_VERSION"),
        |ctx: ServerContext| {
            let server = ExampleServer::new(
                ctx.webid,
                /* your custom fields */
            );
            Ok(server)
        },
        vec![],  // CredentialRequirements
    ).await
}
```

### Step 5: Write the Binary Entry Point

```rust
// mcp-servers/<your-mcp-package>/src/main.rs

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_example::run().await
}
```

### Step 6: Register as an In-Process Builtin

Add your server to the canonical registry in `crates/kask_bridge/src/mcp_servers.rs` so zed-kask's in-process transport can discover and load it:

```rust
pub const BUILT_IN_MCP_SERVERS: &[BuiltinMcpServer] = &[
    // ... existing entries ...
    BuiltinMcpServer {
        id: "example",
        binary: "<your-mcp-package>",
        description: "Example — what it does",
    },   // ← add this entry
];
```

### Testing the Server

Manual test (stdio, for development):

```bash
cargo build -p <your-mcp-package>
HKASK_WEBID=<webid-uuid> cargo run -p <your-mcp-package>
```

In-process test (production path): launch zed-kask and verify the server appears in the agent panel tool list.

### Common Pitfalls

| Pitfall | Fix |
|---------|-----|
| Missing `#[tool]` attribute | Every public async method that should be an MCP tool must have `#[tool(description = "...")]` |
| Duplicate `ToolContext` impl | `mcp_server!` already calls `impl_tool_context!` — do not duplicate it |
| No Regulation spans emitted | Always wrap tool logic in `execute_tool(self, "tool_name", async { ... }).await` |
| Server starts as `"anonymous"` | Set `HKASK_WEBID` before starting (the server reads it at startup and falls back to anonymous if unset) |
| Server not loaded by zed-kask | Add a `BuiltinMcpServer { id, binary, description }` entry to `BUILT_IN_MCP_SERVERS` in `crates/kask_bridge/src/mcp_servers.rs` |
| Tool name conflicts | Tool names are global across all MCP servers. Use a prefix convention (e.g., `example_ping`) |

---

## Common Skill Pitfalls

### Skill Not Found in Agent Panel

**Symptom:** `/skill my-skill` says "Skill 'my-skill' not found."

**Fix:** Ensure zed-kask was launched from the project root containing `.agents/skills/`. Skills are loaded from the `.agents/skills/` directory at the project root.

### Template Rendering Fails

**Symptom:** `render_template` returns an error.

**Fix:** Validate Jinja2 syntax in all `.j2` files. Ensure the template path exists under `kask/registry/templates/`. Verify the template base path is wired (check `agent::set_template_base_path` in `crates/zed/src/main.rs`).

### Lisp Eval Errors

**Symptom:** `lisp_eval` returns an error.

**Fix:** Check the Lisp form for infinite recursion or excessive steps. The interpreter is bounded by `max_steps` (default 100000) and `max_depth` (default 64). Simplify the form or increase the budget if needed.

---

## Related

- [zed-kask Host Architecture Plan](../architecture/zed-host-architecture-plan.md) — D1 (skill execution), D2 (Curator agent), D3 (MCP tool transport — child processes over stdio)
- [Regulation Explanation](../diataxis/hkask-regulation/explanation.md) — Regulation spans emitted by skill execution
- [Skill ↔ MCP Tool Integration](skill-mcp-integration.md) — how skills invoke MCP tools via the agent's tool-use loop

---

## Footnotes

[^anthropic-skills]: Anthropic. (2025). *Equipping agents for the real world with Agent Skills*. https://www.anthropic.com/engineering/equipping-agents-for-the-real-world-with-agent-skills
    Cited for progressive disclosure (name + description preloaded, body loaded on relevance). zed-kask's body-injection model is this pattern: the catalog is preloaded, the body is injected on invocation.

[^fagan-skill-audit]: Fagan, M. E. (1976). Design and code inspections to reduce errors in program development. *IBM Systems Journal*, 15(3), 182–211. https://doi.org/10.1147/sj.153.0182
    Cited for the inspection-based audit methodology the skill audit applies to SKILL.md and template consistency.

[^mcp-spec-skill-invoke]: Anthropic. (2024). *Model Context Protocol Specification*. Anthropic PBC. https://modelcontextprotocol.io/specification
    Cited for the MCP protocol that skill execution uses for tool invocation.

[^ousterhout-bundle]: Ousterhout, J. (2018). *A Philosophy of Software Design*. Yakny Press.
    Cited for the module-composition discipline the skill-bundler applies when ordering skills into phases.

[^beer-feedback-loop]: Beer, S. (1979). *The Heart of Enterprise*. John Wiley & Sons.
    Cited for the cybernetic feedback-loop design the skill-router/skill-discovery pair implements.

[^mcp-spec-build]: Anthropic. (2024). *Model Context Protocol Specification*. Anthropic PBC. https://modelcontextprotocol.io/specification
    Cited for the MCP protocol every builtin MCP server follows.

[^ousterhout-mcp-build]: Ousterhout, J. (2018). *A Philosophy of Software Design*. Yakny Press.
    Cited for the deep-module principle that the composition root wires individual components directly instead of a `KaskCore` singleton.
