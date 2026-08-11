# Security Review — Kask↔Zed Seam (kali-audit)

> Standards: OWASP LLM Top-10 (2025), MITRE ATLAS v5.1, NIST SSDF SP 800-218A.
> Anchors: Microsoft Research FIDES, OpenAI Instruction Hierarchy, ANSSI Secure Rust, RustSec, Trail of Bits.
> Every finding cites `file:line`. `deferred` items state a reason.

## Verdict: **Conditional**

No critical/high findings. 7 of 8 defense layers are covered and
enforced. Layer 7 (FIDES taint / IFC) is structurally present but
**operationally inert** (KS-01 + KS-02). The primary security membrane — the
OCAP capability-match gate + the gas gate inside `McpRuntime::invoke`
(Layers 1 + 3) — is fully functional and fail-closed, so the inert IFC layer
is defense-in-depth degradation, not a primary-membrane breach.

## Defense-layer coverage

| # | Layer | Status | Enforcement point |
|---|-------|--------|-------------------|
| 1 | McpRuntime invoke gate (OCAP capability match) | covered | `kask/crates/hkask-mcp/src/runtime.rs:426-435` (`is_valid_for` + `verify_capability_domain`; fail-closed when governance absent, L489-500). Token expiry NOT enforced — confirmed: no `expires_at`/`is_expired`/`is_valid_for_at`/`new_with_expiry` in `kask/**/*.rs` (matches `.rules`). |
| 2 | Per-agent `mcp_tools` allowlist | covered | `kask/crates/kask_bridge/src/inference_ipc_server.rs:734-757` (delegated-tool allowlist enforced at dispatch boundary before token mint; fail-closed on missing/empty). KS-04 notes the allowlist is child-self-declared — a documented same-uid trust tradeoff, not a gap. |
| 3 | Gas gate | covered | `kask/crates/hkask-mcp/src/runtime.rs:443-468` (`can_proceed` + `charge_call` both fail-closed; charge failure denies the call to preserve the cap; fail-closed for agents without a registered budget). |
| 4 | Credential scoping per MCP server | covered | `kask/crates/kask_bridge/src/mcp_servers.rs:407-461` (all 13 `BuiltinMcpServer` entries use `Some(&[])` minimum; `filter_credentials_for_server`/`filter_config_env_for_server` fail-closed for unknown IDs; content-alignment tests present). |
| 5 | Schema validation (`AnyJsonValue` / `find_boolean_schema_positions`) | covered | `kask/crates/hkask-types/src/tool_schema.rs:116` + `tests/schema_compliance.rs` in all 13 mcp-servers. Servers accepting arbitrary JSON use `AnyJsonValue`, not `serde_json::Value`. |
| 6 | Signing / expiry (marketplace) | covered | `crates/collab/src/api/kask_skills.rs:86-127` (Ed25519 `verify_strict` rejects malleable sigs; fail-closed `SignatureMismatch`; 120-day over-cap). D-seam supporting file — in scope. |
| 7 | Taint propagation / IFC (FIDES) | **MISSING (inert)** | `kask/crates/hkask-templates/src/step_actions.rs:705` + `kask/crates/hkask-mcp/src/runtime.rs:370`. Gate structurally present but operationally dead — see KS-01 + KS-02. |
| 8 | Error classification (per-variant) | covered | `kask/crates/hkask-mcp-server/src/server/validation.rs:83-162` + `kask/security/regressions/RR-0044.yaml` (grep regression `^(?!.*rr0044-ok).*McpToolError::internal\(`). Per-variant classifiers: `map_io_error`, `map_join_error`, `map_memory_store_error`, `map_media_error`, `map_portfolio_error`. |

## Findings

### KS-01 — FIDES taint gate dead at read side: `__taint__` markers never written
- **Surface**: code | **Severity**: medium | **Force**: blocking
- **file:line**: `kask/crates/hkask-templates/src/step_actions.rs:710`
- **Standard**: OWASP-LLM-2025:LLM06 | ATLAS:AML.TA0006 | CWE-20
- **Evidence**: `check_untrusted_input` reads taint from the legacy string
  map via `format!("__taint__{key}")` then `context.get(&marker)...is_some_and(|t| t == 1)`.
  A whole-project grep for `__taint__` finds exactly ONE match — this read site.
  `StepContext::store_result`/`store_named` (`step_context.rs:90-128`) insert
  only the value, never a `__taint__{key}` marker. The typed `taint` is stored
  on `StepResult.taint` but the gate reads the separate `legacy` map which is
  never populated with taint markers. Result: `has_untrusted_input` is always
  `false`, so `DefaultPolicy::check` Rule 2 (`tool_taint == Sink &&
  has_untrusted_input` → Block`, `runtime_policy.rs:86-91`) never fires.
- **Remediation**: rewrite `check_untrusted_input` to read the typed
  `StepResult.taint` via `StepContext::taint_of(step_id)` instead of legacy
  `__taint__` markers; add a cargo-test that stores a `Source`-tainted result,
  then runs `execute_tool_invoke` with a `Sink`-tainted tool and asserts a
  `Block`. Push into `kask/crates/hkask-templates/` (no D-seam).

### KS-02 — All MCP tools hardcoded `ToolTaint::Pure` — FIDES lattice labels defeat the gate
- **Surface**: mcp | **Severity**: medium | **Force**: blocking
- **file:line**: `kask/crates/hkask-mcp/src/runtime.rs:370`
- **Standard**: OWASP-LLM-2025:LLM06 | CWE-20 | `.rules:Advertised-invariants-need-enforcement-points`
- **Evidence**: `McpRuntime::get_tool_info` constructs `ToolInfo` with
  `taint: hkask_capability::tool_taint::ToolTaint::Pure` hardcoded for every
  MCP tool. `invoke_tool` (`step_actions.rs:846`) returns
  `Ok((result, tool_info.taint))` — always `Pure`. `DefaultPolicy::check` Rule 2
  requires `Sink`; Rule 4 requires `Source`. With all labels `Pure`, neither
  fires. The FIDES IFC layer is advertised as enforced in
  `kask/docs/architecture/guard-taint-pipeline.md:130-138` but the tool-label
  input to that gate is a constant `Pure`.
- **Remediation**: add a per-tool `taint` field on `McpTool` populated from
  server-declared metadata; have `get_tool_info` read it. Or, if per-tool
  taint is not yet modeled, downgrade the doc claims to "not yet enforced —
  all tools labeled Pure". Push into `kask/crates/hkask-mcp/` (no D-seam).

### KS-03 — Stale `.rules` + docs reference removed `propagate_taint_for_binding` (phantom prior)
- **Surface**: config | **Severity**: low | **Force**: directing
- **file:line**: `kask/docs/architecture/guard-taint-pipeline.md:85`
- **Standard**: CWE-1078 | `.rules:Convention-priors-must-be-verified`
- **Evidence**: The `.rules` trap and `guard-taint-pipeline.md:85-92`,
  `kask/docs/diataxis/hkask-templates/reference.md:25` cite
  `propagate_taint_for_binding` at `executor.rs:282` with call sites at
  `executor.rs:789/1245/1376/1423`. A whole-project grep finds ZERO matches
  in any `.rs` source — only a historical comment (`step_context.rs:16`),
  `.rules`, `GEMINI.md`, and `kask/registry/templates/kali-audit/select-surface.j2:97`.
  The executor was refactored into `step_machine.rs`/`step_actions.rs`/
  `step_context.rs`; taint is now a field on `StepResult` (`step_context.rs:38`).
  The kali-audit template instructs the auditor to look for the removed
  function — always a false negative.
- **Remediation**: update `.rules` to describe the new `StepResult.taint` +
  `check_untrusted_input` mechanism; update `guard-taint-pipeline.md` +
  `reference.md` source-citation tables to `step_actions.rs:705` /
  `step_context.rs:38`; update `kali-audit/select-surface.j2:97-101` to check
  for `StepResult.taint` field presence. Per `.rules` hygiene, this is a
  **Suggested .rules addition** for review — not an inline edit.

### KS-04 — Panel token grant-all for tool name; delegated-tool allowlist is child-self-declared
- **Surface**: mcp | **Severity**: info | **Force**: enabling
- **file:line**: `kask/crates/kask_bridge/src/inference_ipc_server.rs:759`
- **Standard**: OWASP-LLM-2025:LLM06 | `.rules:Manifest-ocap-is-declared-config-not-security-gate`
- **Evidence**: `dispatch` for `ToolInvoke` mints
  `panel_default_token(DelegationResource::Tool, tool.clone(), Execute, webid, webid)`
  where `resource_id = tool` (name only, not `server/tool`). The delegated-tool
  allowlist is read from `params.tool_allowlist` — declared by the child MCP
  server per-request. The allowlist IS enforced before minting (L735-757,
  fail-closed), but a child can broaden it. The code comment (L683-696)
  documents this as an accepted same-uid trust tradeoff. Not a vuln within
  the stated threat model.
- **Remediation**: no code change if same-uid trust holds. If tightening is
  desired: scope `resource_id` as `{server}/{tool}`, or validate the
  child-declared allowlist against the governed `HKASK_MCP_SERVER_IDS`
  config. All within `kask/crates/kask_bridge/` + `kask/crates/hkask-capability/`.

## Surfaces verified clean (no finding)
- `unwrap_or(0)` on regulation-loop sense inputs — none in `hkask-regulation`/
  `hkask-memory`/consolidation (`.rules`-cited sites already fixed).
- `AnyJsonValue` schema guard — all 13 servers.
- Credential scoping — all `Some(&[])`, fail-closed.
- Marketplace signing — Ed25519 strict + 120-day cap, fail-closed.
- MCP error classification — RR-0044 enforced.