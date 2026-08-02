# Agent Bestiary World (ABW) Swarm Intelligence — Design & Current State

> Supersedes the original 2026-08-01 integration plan (removed 2026-08-01 as
> stale in `8f3ebd5a00`). This document is **current-state**: every claim below
> is grounded in the code at the cited paths, not in a design aspiration. When
> this document disagrees with the code, the code wins — file an issue.

## 0. Substrate: ABW swarm semantics (verified live 2026-08-01)

- A swarm is an ABW **workspace** with hired agents (not the deprecated
  "ensemble" model).
- Compound agents declare `dependencies { required, optional }` and auto-hire
  their team.
- Gas: hire 5 cr, @mention 1 cr + tokens, delegation 1 cr + tokens.
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
through the global `ToolInvoker` hook → `McpRuntime` (governed, OCAP + gas),
never ad-hoc HTTP from the UI.

### 3.3 Tool surface (23 tools)

ABW tools: `swarm_list_agents`, `swarm_get_swarm`, `swarm_get_agent`,
`swarm_list_apps`, `swarm_ontology_templates`, `swarm_execute_agent`,
`swarm_hire_cost`, `swarm_request_consent`, `swarm_hire`, `swarm_delegate`,
`swarm_run_status`, `swarm_generate_prompt`, `swarm_generate_ontology`,
`swarm_create_agent`, `swarm_create_swarm`, `swarm_xaman`, `swarm_create_app`.
Local tools (v2 §15): `swarm_fund_local`, `swarm_delegate_local`,
`swarm_list_local_agents`, `swarm_balance_local`, `swarm_clone_to_local`,
`swarm_push_to_cloud`. Both tool sets are **always available in either mode** —
the operator chooses the tool explicitly; there is no `Hybrid` routing layer
(§15.1.8). `SwarmConfig.mode` only selects the startup warning; no server tool
branches on it. The panel pins the tool-name contract in
`panel_tool_names_match_server`.

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

Registry-first cascade (`kask/registry/manifests/swarm-intelligence.yaml` +
`kask/registry/templates/swarm-intelligence/*.j2`): SENSE → ORIENT → DECIDE →
ACT → CHECK → CONVERGE (Cauchy criterion on the swarm-state distance `d`,
algedonic override on 402 / un-acknowledged curator dispatch).

Mode-aware (v2 §15): SENSE/ACT/CHECK branch on `{{ mode }}` ("abw" | "local").
The mode + `swarm_id` reach the cascade through the `skill` tool's `context`
argument (`SkillToolInput.context`, merged into the cascade context before
`task`). Missing `mode` defaults to `"abw"` (manifest input_mapping
`{{ mode | default('abw') }}`). The Steer system prompt carries the current
mode and instructs the curator to pass it.

Known limits (as of 2026-08-02):
- No `fire` tool — the ABW fire endpoint is not implemented. DECIDE flags
  redundant duplicates (`flag_redundant_duplicate`) for manual pruning;
  ACT aborts a `fire` move with `no_fire_tool`. Do not reintroduce `fire` or
  `swarm_update_swarm` in the templates until a server tool exists.
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
  (zed IPC bridge or MediaRouter fallback), `hkask-guard` (mandatory scanners).
  Lazily initialized (OnceCell) on first local tool call.
- Ledger — operator-funded (`swarm_fund_local`); unfunded delegation returns
  `PaymentRequired`. No auto-replenishment: the corrective signal must be real.
- Cost — 1 credit / 1000 tokens, capped at `credits_authorized`, debited
  *before* the output guard scan (compute was spent even if the output is
  quarantined).

### 15.3 Local agents and tools/skills

`LocalAgentCapabilities` carries `mcp_tools` (qualified `server/tool` names)
and `skills` (skill ids). `swarm_delegate_local` declares the card's
`mcp_tools` to the model and dispatches model tool calls through the zed IPC
bridge's `ToolInvoke` method (governed `McpRuntime` on the zed side, panel
token). Tool calls are allowlisted to the card's declared tools. Declared
`skills` are carried on the card and in create/clone/push, but are **not yet
executed** in local mode — the skill-exec IPC method is the follow-up.

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
