---
title: "ABW Swarm Intelligence — Design & Current State"
audience: [architects, developers]
last_updated: 2026-08-20
version: "1.1.0"
status: "Partially Deprecated"
domain: "Swarm"
mds_categories: [domain, composition, trust]
---

# Agent Bestiary World (ABW) Swarm Intelligence — Design & Current State

> **⚠️ Partially deprecated 2026-08-20.** The ABW substrate, tool surface,
> consent gate, and local runtime described here remain current. Declared
> `capabilities.skills` execute via upstream-Zed body injection
> (`SkillTool::run` → `render_skill_envelope`).
>
> Claims below that reference deleted subsystems are historical. The ABW
> REST client, consent store, spend gate, local agent registry, and
> `LocalSwarmRuntime` survive.

> Supersedes the original 2026-08-01 integration plan (removed 2026-08-01 as
> stale in `8f3ebd5a00`). This document is **current-state**: every claim below
> is grounded in the code at the cited paths, not in a design aspiration. When
> this document disagrees with the code, the code wins — file an issue.

## 0. Substrate: ABW swarm semantics (verified live 2026-08-01; lifecycle + gas model re-verified 2026-08-02)

- A swarm is an ABW **workspace** with hired agents (not the deprecated
  "ensemble" model).
- Compound agents declare `dependencies { required, optional }` and auto-hire
  their team.
- Gas (verified live 2026-08-02): add an OWN agent to a workspace = flat 2 cr
  (`gas_charged: 2` on `/add`; owned `/dependencies` quotes `total_hire_cost:
  0`, hence the consent-gate floor). Hire a THIRD-PARTY catalogue agent via
  `/hire` = flat 5 cr base (`gas_charged: 5` on `sensor_advisor` with
  `dependencies_hired: []`); the third-party `/dependencies` quote already
  includes the base (`total = base + required + optional` — a quote of
  `total=10, required=0, optional=5` is base 5 + optional 5). @mention/
  delegation = 1 cr + tokens. `/api/wallet/transactions` verified (the CHECK
  phase's reconciliation read).
- Delegation is one level deep: delegates lose `delegate_to_agent` /
  `execute_agent` (no delegation chains).
- API surface (verified against the live service):
  - Base URL `https://agent-bestiary.world` (no `api.` subdomain).
  - Auth: `Authorization: Bearer <key>` (Pro-tier API key).
  - Open: `GET /api/agents`, `GET /api/models/catalogue`.
  - Authed: `/api/workspaces`, `/api/agents/{name}/execute`,
    `/api/xaman/sessions`, `/api/wallet`.
  - ABW returns HTTP 200 envelopes containing upstream LLM errors in the body
    (e.g. Anthropic credit exhaustion verbatim) and HTTP 500 for domain
    failures (e.g. unfunded agents). Error mapping inspects bodies, not just
    status codes — see `detect_embedded_error` in
    `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs`.

### Verified response shapes (live, 2026-08-02)

Pinned by the `abw_*` live tests (`cargo test -p hkask-mcp-swarm --lib --
--ignored abw`) and unit-test contracts in the panel:

- `GET /api/workspaces` → bare array or `{workspaces: [...]}`; each workspace:
  `id, name, slug, description, origin, owner_id, agent_count,
  agent_previews, workspace_budget, workspace_remaining, workspace_spent`.
  The panel's `agent_count`/`workspace_budget`/`workspace_remaining` parse
  contract matches these names exactly.
- `GET /api/workspaces/{id}` → `{ id, name, slug, mission, description,
  is_composition, members, agents: [{ agent_id, agent_name, agent_type,
  description, display_alias, accepts, produces, tags, sample_queries,
  prompt_template, relationship, total_executions }], workspace_budget,
  workspace_remaining, workspace_spent, coordination_strategist_id,
  coordination_strategist_name }`. The roster drill-down parses the top-level
  `agents` array.
- `GET /api/workspaces/{id}/messages` → `{ messages: [{ message_id,
  message_type, content, sender_id, sender_name, sender_type, created_at,
  metadata }] }`. The run-status strip renders `sender_name`.
- `GET /api/wallet` → `{ balance }`.
- `GET /api/agents/{name}/dependencies` → `{ total_hire_cost, required_cost,
  optional_cost, has_dependencies, required, optional }` (the consent gate's
  re-verification source).

## 3. Design decisions (current state)

### 3.1 `hkask-mcp-swarm` is a standalone MCP server

Not an extension of another server. Registered in
`kask_bridge::BUILT_IN_MCP_SERVERS` (id `"swarm"`, binary `hkask-mcp-swarm`),
launched by both the governed `McpRuntime` (panel + skill cascade) and the
per-project `ContextServerStore` (agent tool picker). The two parallel launch
paths are by design — see `.rules` "Kask MCP servers have two parallel launch
paths by design".

### 3.2 `SwarmPanel` is a center-pane `Item`

`crates/swarm_panel/src/swarm_panel.rs`. Four surfaces: Browse (agents/swarms
cards with cloud/local/synced source badges), Author (new agent), Compose
(swarm + Xaman Ek consultant), Steer (curator `ConversationView` scoped to the
swarm server that invokes the `swarm-intelligence` skill). All ABW calls flow
through the global `ToolInvoker` hook → `McpRuntime` (metered + span-emitting; the
capability-match this line originally credited was removed 2026-08-12 as vacuous,
RR-0056), never ad-hoc HTTP from the UI.

### 3.3 Tool surface (52 tools)

The swarm server exposes **52 tools** — 27 ABW + 25 local — **both sets always
registered in either mode** (`kask.swarm.mode` selects the substrate, not the
surface; pinned by `tool_surface_is_exactly_52_registered_tools`). The full
tool-by-tool reference lives in
[`reference/mcp-servers/swarm.md`](../reference/mcp-servers/swarm.md); the tool
names are generated at build time from `pub(crate) async fn swarm_*`
signatures (`build.rs`) so the surface cannot drift from the documented set.
The panel pins the tool-name contract in `panel_tool_names_match_server`.

### 3.4 One error enum, one config struct

`SwarmError` (Auth / PaymentRequired / AgentNotFunded / UpstreamModelError /
RateLimited / CuratorUnavailable / ApiVersionMismatch / ConsentDenied /
Unavailable) maps into MCP tool errors. `SwarmConfig` (server crate) and
`KaskSwarmSettings` (bridge crate) have deliberately duplicated `Default`
impls — the duplication is the seam between the crates (no circular
dependency). Defaults must stay in sync in both directions (`.rules` "Kask
settings defaults must live in `Default` impls").

### 3.5 Auth model

Pro-tier API key via keychain entry `kask://credentials/hkask_abw_api_key`,
injected as `HKASK_ABW_API_KEY` (per-server credential allowlist — the swarm
server receives only this key, pinned by `swarm_credentials_only_include_abw_key`).
Keyless = catalogue-only mode (`swarm_list_agents` works; authenticated tools
return `Auth` errors with remediation).

### 3.6 The cost/consent gate (the critical build)

The enforcement point for `.rules` "Advertised invariants need enforcement
points". Three layers, all server-side:

1. **Consent tokens** — `swarm_request_consent` mints a single-use,
   action+target-scoped token (in-memory `ConsentStore`, session-scoped; does
   not survive restart). Spend tools (`swarm_hire`, `swarm_delegate`,
   `swarm_xaman`) consume the token and verify scope (`consume` rejects
   unknown/replayed/scope-mismatched/over-spend tokens).
2. **Cost re-verification** — spend tools re-query ABW's
   `/agents/{name}/dependencies` for the actual `total_hire_cost` after
   consuming the token; a consent minted for 1 credit cannot spend 20. A
   missing `total_hire_cost` is *unknown*, never fabricated as zero.
3. **Per-dispatch ceiling** — `max_credits_per_dispatch` (default 50,
   `HKASK_ABW_MAX_CREDITS`) is a hard gate: `swarm_hire` refuses
   `actual_cost > ceiling`; `swarm_delegate` refuses `credits_authorized >
   ceiling`. There is no per-call override path (a prompt-injected agent must
   not be able to talk the operator into raising it mid-session).

Transient failures refund the consumed grant so the operator can retry without
re-confirming (`refund`). The panel renders the gate as a consent banner
(`render_consent_banner`) and disables Confirm when `within_budget: false`.

4. **Zed-side dispatch allowlist (2026-08-02)** — the local delegate loop's
   tool dispatch carries the card's declared `server/tool` allowlist with
   every `tool_invoke` IPC request; the zed-side IPC server refuses any tool
   outside it **before** minting the panel token (fail closed on a missing
   allowlist). The allowlist is therefore enforced at the dispatch boundary,
   not only inside the child process.
5. **Call-cap seed (updated 2026-08-03)** — `CallCapManager::can_proceed` denies
   agents without a registered cap (fail-closed). The composition root seeds a
   `swarm-panel` persona cap (`SWARM_PANEL_CALL_CAP = 10_000` in
   `crates/zed/src/main.rs`, reset to the ceiling each regulation tick), which
   is the account every governed tool call — panel, skill cascade, and swarm
   `tool_invoke` dispatch — charges (one call per invocation).

Known limit → resolved (2026-08-02): consent tokens previously lived in
in-memory stores **per server process**, so the panel's hire flow and the
Steer curator's spend flow (governed `McpRuntime` vs per-project
`ContextServerStore`) could not share a token. The store is now a shared
SQLite file by default (`mcp/swarm/consent.db`, overridable via
`HKASK_SWARM_CONSENT_STORE`) opened by both processes, so a token minted by
the panel is consumable by the Steer curator (and vice versa). Single-use is
enforced atomically across processes (DELETE-affected-rows check — two
processes racing on the same token cannot double-spend it); grants expire
after 1 hour (`CONSENT_TTL_SECS`). On store-open failure the server degrades
to the session-local in-memory store with a loud error — same-process flows
still work, cross-process flows do not.

### 3.7 Curator opt-in is the default

Xaman Ek reads task content, so `curator_consent_default` defaults to
`false`: `swarm_xaman` requires a `swarm_request_consent` token (action
`"curate"`, fixed target `"xaman"` — session-scoped tokens would force a fresh
token per continuation message). Setting
`kask.swarm.curator_consent_default: true` opts in globally.

## 4. Cybernetic analysis (as built)

- **4.1 Feedback loop**: wallet balance rides every tool response
  (`with_wallet`); a failed balance query returns `None`, never a fabricated
  zero (`.rules` `unwrap_or(0)` trap). The panel shows the balance in the
  header.
- **4.2 Ashby variety**: enforced by the `swarm-intelligence` skill's SENSE
  phase (`variety_coverage` = covered/required transforms).

## 13. Companion skill: `swarm-intelligence`

The `swarm-intelligence` skill (`kask/registry/templates/swarm-intelligence/*.j2`):
SENSE → ORIENT → DECIDE → ACT → CHECK → CONVERGE (Cauchy criterion on the
swarm-state distance `d`, algedonic override on 402 / un-acknowledged curator
dispatch).

Mode-aware (v2 §15): SENSE/ACT/CHECK branch on `{{ mode }}` ("abw" | "local").
The mode + `swarm_id` reach the cascade through the `skill` tool's `context`
argument (`SkillToolInput.context`, merged into the cascade context before
`task`). Missing `mode` defaults to `"abw"` (skill-body Jinja default
`{{ mode | default('abw') }}`). The Steer system prompt carries the current
mode and instructs the curator to pass it.

The slash-command path (`/swarm-intelligence ...`, via
`send_skill_invocation`) has no `SkillToolInput.context` channel, so leading
`key=value` pairs in the argument text are parsed as context (see
`parse_slash_command_context` in `agent.rs`). Example:
`/swarm-intelligence mode=local swarm_id=ws-1 compose my swarm` sets
`mode=local`, `swarm_id=ws-1`, and task `compose my swarm`. The slash-command
prefix is stripped before the task reaches the cascade (previously the
`/swarm-intelligence` prefix leaked into `task`).

Known limits (as of 2026-08-02, updated after live verification):
- `swarm_fire` now exists (verified live — `DELETE /workspaces/{id}/agents/{agent}`
  removes the agent from the roster; no credit cost; the agent itself is not
  deleted). DECIDE flags redundant duplicates and ACT emits `swarm_fire` for
  them. `swarm_delete_agent` (verified live — `DELETE /agents/{id}`) is the
  permanent-delete counterpart.
- Workspace update has NO ABW endpoint (405, verified live) — do not add
  `swarm_update_swarm`. Agent update (`PUT /api/agents/{id}`) remains
  unverified. Workspace delete IS implemented: `swarm_delete_swarm` via the
  team-scoped `DELETE /api/teams/{id}` (verified live 2026-08-02;
  `DELETE /api/workspaces/{id}` is 405) — the counterpart of
  `swarm_create_swarm` for the full lifecycle.
- `swarm_create_agent` hardcodes `provider: "anthropic"` and passes through
  `mcp_tools`/`skills` from the request.

## 15. Local mode (v2 §15) — zed-kask's local substrate

`kask.swarm.mode: local` routes the *operator's choice of tool* to the local
substrate; both tool sets remain registered.

### 15.1 Rejected alternatives (as built)

- 15.1.1 `swarm_hire_local` — rejected: the team is emergent from the call
  pattern; "hire" in local mode means adding a card to the registry.
- 15.1.2 Consent tokens on local tools — rejected: the ledger balance check
  is the gate.
- 15.1.8 Hybrid routing layer — rejected: the operator does the routing by
  choosing the tool.
- 15.1.9 `workspace_id` on local tools — rejected: the workspace is the
  session.

### 15.2 Components

- `LocalAgentRegistry` — reads `<id>/agent_card.json` from
  `agents/local/curated/` (resolved under the hKask data dir; `HKASK_LOCAL_AGENTS_DIR`
  overrides). Reloaded on every list/get so operator-added cards appear
  without a server restart.
- `LocalSwarmRuntime` — `hkask-ledger` (SQLite, operator-funded), `hkask-inference`
  (zed IPC bridge or MediaRouter fallback). Lazily initialized (OnceCell) on
  first local tool call.
- Ledger — operator-funded (`swarm_fund_local`); unfunded delegation returns
  `PaymentRequired`. No auto-replenishment: the corrective signal must be real.
  Transaction history is queryable via `swarm_local_history` (the local-mode
  run/reconciliation surface).
- Cost — 1 credit / 1000 tokens, capped at `credits_authorized`, debited
  *before* the output guard scan (compute was spent even if the output is
  quarantined).

### 15.3 Local agents and tools/skills

`LocalAgentCapabilities` carries `mcp_tools` (qualified `server/tool` names)
and `skills` (skill ids). `swarm_delegate_local` declares the card's
`mcp_tools` to the model and dispatches model tool calls through the zed IPC
bridge's `ToolInvoke` method (governed `McpRuntime` on the zed side, panel
token). Tool calls are allowlisted to the card's declared tools AND the
allowlist is enforced zed-side at the dispatch boundary (the qualified
`server/tool` list travels with every `tool_invoke` request). Declared
`skills` are executed via upstream-Zed body injection (`SkillTool::run` →
`render_skill_envelope`) **before** the LLM call; each skill's output is
injected into the prompt as context. A missing/failed skill is recorded
(`executed_skills` in the response) and the delegation proceeds.

Local cards are removed with `swarm_remove_local` (the local counterpart of
firing — deletes the card directory; a synced card's ABW agent is untouched)
and added via clone or manual file placement (§15.1.1).

Cloned cards' declared `mcp_tools`/`skills` are third-party ABW data. At
clone time (`swarm_clone_to_local`) they are provenance-filtered: entries
must be shape-valid (`server/tool`, charset-safe) and, when
`HKASK_MCP_SERVER_IDS` is set (the parent's governed server set, injected by
the bridge), the `server` must be one of the operator's governed servers — a
cloned ABW card cannot extend the delegated tool surface beyond them.
Dropped entries are logged so the operator sees what was filtered.

### 15.4 Backend toggle

The panel's Backend toggle writes `kask.swarm.mode` to settings.json. The
per-project `ContextServerStore` path re-syncs via the `SettingsStore`
observer (`sync_kask_mcp_servers`). The governed `McpRuntime` path restarts
changed servers via `sync_kask_mcp_runtime_servers` (env diff vs. the launch
baseline; `McpRuntime::stop_server` + `start_server_with_env`).

### 15.5 Steer mode

The Steer `ConversationView` is scoped to the swarm server
(`with_mcp_server_scope("swarm")`), with a system prompt naming both tool
sets, the current mode, the consent gate, the ceiling, and the
`swarm-intelligence` skill. Conversations are not persisted.

### 15.6 Ledger funding (the strongest objection, as built)

The local economy is operator-funded with no auto-replenishment. `debit`
returns `PaymentRequired` on insufficient balance. This is deliberate: a
synthetic ledger breaks the corrective feedback loop.

## 16. Observation & management surfaces (as built, 2026-08-02)

### Observe

- Panel Browse: agent cards (type, executions, cloud/local/synced badge),
  swarm cards (name, agent count, budget/remaining), wallet header (`⛽`),
  local balance header (`■`). Swarm cards have **Details** (roster drill-down)
  and **Run Status** (recent workspace messages) buttons.
- `swarm_get_swarm` — roster + budget (server-sanitized, including roster
  descriptions). `swarm_run_status` — ABW workspace messages. `swarm_balance_local`
  + `swarm_local_history` — local ledger balance and transactions.
- The `swarm-intelligence` skill's SENSE/CHECK re-measure both substrates;
  the algedonic override rides every tool response.

### Manage

- Create: `swarm_create_swarm` (consent-gated hires), `swarm_create_agent`,
  `swarm_create_app`, `swarm_clone_to_local`, `swarm_push_to_cloud`.
- Roster: `swarm_hire` (consent-gated; own agents auto-route through
  `/add`) and `swarm_fire` (verified live — roster removal); local pruning
  via `swarm_remove_local`; permanent ABW deletion via `swarm_delete_agent`;
  workspace teardown via `swarm_delete_swarm` (team-scoped, verified live).
- Spend: `swarm_delegate` / `swarm_execute_agent` (ABW), `swarm_delegate_local`
  (local). Budget: `swarm_fund_local`; per-dispatch ceiling
  (`HKASK_ABW_MAX_CREDITS`); wallet is ABW-side.
- Compose: Xaman Ek (`swarm_xaman`), the `swarm-intelligence` skill, the
  panel's Compose tab.

## 17. ABW lifecycle endpoint ledger (verified / blocked, as of 2026-08-02)

Each lifecycle operation is verified against the live service before a tool
is added; a tool that hits an unverified endpoint is worse than no tool (the
`.rules` "advertised invariants need enforcement points" trap). Endpoints
marked **Disproven** have no ABW endpoint and must not be added:

| Operation | Endpoint shape to verify on ABW | Current state |
|---|---|---|
| Fire / un-hire | `DELETE /api/workspaces/{id}/agents/{agent_id}` — **verified live 2026-08-02** (accepts the agent id or name; 200 `{"message": "Agent removed from workspace"}`) | **Implemented** as `swarm_fire` (no credit cost, no consent token); the skill's DECIDE/ACT emit it for redundant duplicates; local pruning via `swarm_remove_local` |
| Workspace update | `PATCH /api/workspaces/{id}` (name, mission, budget) | **Disproven** — 405 Method Not Allowed on the live service; do NOT implement |
| Workspace delete | `DELETE /api/teams/{id}` — **verified live 2026-08-02** (200 `{"status": "deleted"}`; `DELETE /api/workspaces/{id}` is 405, `POST .../delete`/`archive`/`leave` are 404) | **Implemented** as `swarm_delete_swarm` (team-scoped; irreversible — drops the workspace and its roster); the full lifecycle (create → hire → fire → delete) is verified and covered by a live probe |
| Agent update | `PUT /api/agents/{agent_id}` (system_prompt, model, temperature) | No direct tool; `swarm_push_to_cloud` updates an ABW agent *from a local card*; PUT shape unverified |
| Agent delete | `DELETE /api/agents/{agent_id}` — **verified live 2026-08-02** (200 `{"message": "Agent deleted successfully"}`; catalogue confirms removal) | **Implemented** as `swarm_delete_agent` (resolves uuid-vs-name via the catalogue on 404) |

Additional verified lifecycle facts (2026-08-02): `POST /api/agents` returns
`{agent_id, agent_name, message}` (owned agents carry a uuid in `agent_id`);
`POST /api/teams` returns `{id, slug, ...}` with a default `workspace_budget:
100`; **own agents hire via `POST /api/workspaces/{id}/add`** (400 "Use /add
for your own agents" on `/hire`, `gas_charged: 2` flat add fee), while
third-party catalogue agents hire via `/hire` at a flat **5 cr base**
(verified live on `sensor_advisor`: `gas_charged: 5` with
`dependencies_hired: []`; the third-party `/dependencies` quote already
includes the base — `total = base + required + optional`);
`GET /api/wallet/transactions` verified (the CHECK phase's reconciliation
read: `{balance, transactions[{amount, tx_type, description,
balance_after}], wallet_id}`); agent names are slugs (`[a-z0-9_]`,
3–64 chars) and workspace slugs are capped at 64 chars — both enforced
server-side now.

Verification procedure: inspect ABW's OpenAPI/docs for each shape, add the
tool with the verified path, and pin the response shape with a unit test
(like the existing consent/ceiling tests). Workspace delete is now a tool
(`swarm_delete_swarm`), so leftover verify workspaces are cleaned up by the
same lifecycle probe that creates them (`abw_workspace_lifecycle_cleanup`,
`#[ignore = "requires ABW API key"]`).
