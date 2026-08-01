# ABW Swarm Integration — Bug Hunt Report

**Date:** 2026-08-01
**Scope:** ABW (Agent Bestiary World) swarm integration — three surfaces only
**Methodology:** bug-hunt (Charter → Probe → Oracle → Taxonomize → Report)
**Prior audit:** §12 security audit (10 findings, all fixed and committed)
**Baseline tests:** 30 passing (hkask-mcp-swarm: 20, kask_bridge swarm: 4, swarm_panel: 6)

---

## Charter

### Charter statement

> Explore the ABW swarm integration (MCP server + panel + settings/registry)
> using Bach's HTSM Product-Elements and Quality-Criteria strategies, with
> Beizer focus on **integration**, **interface**, **timing**, and **data**
> categories, to discover quality threats that the §12 security audit did not
> cover — specifically: error paths on weird ABW API shapes, consent-token
> lifecycle edge cases, panel state mutations under debounce, network failures
> mid-hire, panic-prone code on non-startup paths, and missing tests for
> critical paths.

### Target area

| Surface | Path | LOC |
|---|---|---|
| MCP server | `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs` | 1817 |
| Panel | `crates/swarm_panel/src/swarm_panel.rs` | 2050 |
| Settings | `kask/crates/kask_bridge/src/settings.rs` (swarm section) | ~80 |
| Registry | `kask/crates/kask_bridge/src/mcp_servers.rs` (swarm entry) | ~20 |
| Content | `crates/settings_content/src/settings_content.rs` (swarm struct) | ~10 |

### Strategy

Bach's HTSM **Product Elements** (data, interfaces, code, integration) +
**Quality Criteria** (correctness on edge cases, error handling, data
integrity). The §12 audit was security-focused (attack chains, consent gates,
credential scoping); this hunt targets the *operational* quality surface:
what happens when ABW returns weird shapes, when the network drops mid-spend,
when the debounce timer races, when the clock is wrong.

### Beizer focus

1. **Integration** — ABW HTTP error mapping, JSON shape assumptions, network
   failure mid-hire (the dominant failure class for an HTTP-dependent MCP server).
2. **Interface** — consent-token lifecycle (mint/consume/replay/refund), panel
   ↔ server contract on tool names and response shapes.
3. **Timing** — debounce timer races, consent consume before network call,
   double-fire on rapid clicks.
4. **Data** — `unwrap_or(0)` on cost signals, integer casts, slug generation
   panics, missing `with_wallet` on browse tools.

### Crate model

**Architecture:** The MCP server (`hkask-mcp-swarm`) is a thin reqwest wrapper
over ABW's REST API, exposing 16 tools via rmcp. Spend tools are gated by an
in-memory `ConsentStore` (single-use, action+target-scoped tokens). The panel
(`swarm_panel`) is a GPUI center-pane `Item` with four modes (Browse/Author/
Compose/Steer) that calls the MCP server via the governed `ToolInvoker` hook.

**Data flow:**
```
Panel → ToolInvoker → MCP runtime (OCAP/gas/spans) → hkask-mcp-swarm
  → reqwest → ABW API → response → sanitize → with_wallet → MCP client → panel
```

**Critical paths:**
- `ConsentStore::consume` — the spend gate enforcement point
- `SwarmClient::send` — HTTP error mapping (status + body-embedded errors)
- `swarm_hire` — consent consume → re-verify cost → POST hire (3-step, fail-mid-way)
- `swarm_create_swarm` — create workspace → per-agent consent consume → POST hire (loop)
- `SwarmPanel::confirm_hire` — mint consent → invoke hire (2-step panel-side)
- `SwarmPanel::fetch_all` — parallel agents + swarms fetch, race-safe retain pattern
- `SwarmPanel::refresh_search` — 250ms debounce timer

**Dependency surface:** reqwest (HTTP), serde_json (parsing), rmcp (MCP
protocol), gpui (UI), kask_panel (ToolInvoker hook), kask_bridge (settings).

**Observed characteristics:**
- **async:** heavy — every tool is `async fn`, `reqwest` I/O, `with_wallet` awaits
- **trait_objects:** `ToolInvoker` is `Arc<dyn ToolInvoker + Send + Sync>`
- **concurrency:** `ConsentStore` uses `std::sync::Mutex`; panel uses GPUI
  foreground spawns (single-threaded)
- **unsafe:** none (`#![forbid(unsafe_code)]`)
- **ffi:** none
- **macros:** `mcp_server!`, `tool_router`, `#[tool]`, `actions!`
- **proc_macros:** none

---

## Findings

Findings are ordered by severity. Each cites `file:line` with a verbatim
evidence snippet. No fabricated findings — every finding was discovered by
reading the source or grepping for a pattern.

---

### BH-01 — `swarm_hire` re-verify uses `unwrap_or(0)` on cost, bypassing the §12.4 fix

**Severity:** HIGH
**Beizer category:** data
**Verdict:** Tier 1 BUG (confidence 0.92, reproducible)
**Epistemic mode:** Declarative
**Provenance:** Direct measurement (source read + pattern grep)

**Location:** `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:1063-1066`

**Evidence:**
```rust
let actual_cost = deps
    .get("total_hire_cost")
    .and_then(|c| c.as_u64())
    .unwrap_or(0);
if actual_cost > u64::from(req.credits_authorized) {
```

**IS:** The `swarm_hire` re-verify path (added by the §12.3 fix) reads
`total_hire_cost` from ABW's `/agents/{id}/dependencies` response with
`unwrap_or(0)`. If ABW omits the field, changes its name, or returns a
non-integer, `actual_cost = 0`, and `0 > credits_authorized` is false — the
gate passes and the hire POST proceeds with an unknown cost.

**OUGHT:** The §12.4 fix established the pattern in the *same file*: a missing
`total_hire_cost` must return an error (`McpToolError::internal("hire cost
unknown")`) with a `tracing::warn!`, not fabricate `0`. The re-verify path
must follow the same pattern — a missing cost signal is "unknown", not "zero".
The `.rules` trap "`unwrap_or(0)` on regulation-loop sense inputs is a broken
feedback loop" applies directly: the re-verify is a sense input to the spend
gate, and `unwrap_or(0)` makes a failed measurement indistinguishable from a
free hire.

**Pattern signature:** `\.get\("total_hire_cost"\).*\.unwrap_or\(0\)` in
`swarm_hire` (distinct from the fixed `swarm_hire_cost` which uses `match`).

**Fix suggestion:** Replace the `unwrap_or(0)` with the same `match` pattern
used in `swarm_hire_cost` (lines 933-951): on `None`, emit `tracing::warn!` and
return `McpToolError::internal("hire cost unknown — re-verify failed")`. Add a
test pinning that `swarm_hire` rejects when ABW's dependencies response omits
`total_hire_cost`.

---

### BH-02 — `swarm_create_swarm` consumes consent with hardcoded `cost: 5`, no cost re-verification

**Severity:** HIGH
**Beizer category:** integration
**Verdict:** Tier 1 BUG (confidence 0.90, reproducible)
**Epistemic mode:** Declarative
**Provenance:** Direct measurement (source read)

**Location:** `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:1367` (consume) + `crates/swarm_panel/src/swarm_panel.rs:970` (mint)

**Evidence (server):**
```rust
// Consume the consent token for this specific hire.
if let Err(e) = self.consent.consume(token, "hire", agent, 5) {
```

**Evidence (panel):**
```rust
"swarm_request_consent",
json!({ "action": "hire", "target": agent, "credits_authorized": 5 }),
```

**IS:** The panel's `create_swarm` mints a consent token per agent with
`credits_authorized: 5` (hardcoded). The server's `swarm_create_swarm` consumes
each token with `cost: 5` (hardcoded). The actual hire cost is never fetched or
compared. If an agent costs 20 credits, the gate passes (`5 <= 5`) and the hire
POST proceeds, spending 20 credits with only 5 authorized.

**OUGHT:** The §12.3 fix added cost re-verification to `swarm_hire` (fetch
`/agents/{id}/dependencies`, compare `actual_cost <= credits_authorized`).
`swarm_create_swarm` has no such re-verification — it trusts the hardcoded `5`.
The panel should call `swarm_hire_cost` per agent (as `begin_hire` does) and
pass the real cost; the server should re-verify per hire (as `swarm_hire` does).
The hardcoded `5` is a magic number that defeats the cost/consent gate's purpose.

**Pattern signature:** `consume\(token, "hire", agent, 5\)` — hardcoded cost
literal in a consent consume call.

**Fix suggestion:** (1) Panel: call `swarm_hire_cost` per agent before minting
consent, pass the real `total_hire_cost` as `credits_authorized`. (2) Server:
in the `swarm_create_swarm` hire loop, re-fetch `/agents/{id}/dependencies` and
verify `actual_cost <= token.credits_authorized` before each POST, mirroring
`swarm_hire`. (3) Add a test pinning that `swarm_create_swarm` rejects when the
actual cost exceeds the authorized amount.

---

### BH-03 — Panel's `ask_xaman` does not pass a `consent_token`; broken by default

**Severity:** HIGH
**Beizer category:** interface
**Verdict:** Tier 1 BUG (confidence 0.93, reproducible)
**Epistemic mode:** Declarative
**Provenance:** Direct measurement (source read)

**Location:** `crates/swarm_panel/src/swarm_panel.rs:1043-1050` (panel call) +
`kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:1432-1440` (server gate)

**Evidence (panel):**
```rust
"swarm_xaman",
json!({
    "message": message.trim(),
    "session_type": "composition_design",
    "session_id": session_id,
}),
```

**Evidence (server):**
```rust
if !self.client.config().curator_consent_default {
    let target = req.session_id.as_deref().unwrap_or("xaman");
    let Some(token) = req.consent_token.as_deref() else {
        return Err(SwarmError::ConsentDenied(
            "Xaman Ek curator call requires a consent token (action 'curate') — \
             set kask.swarm.curator_consent_default true to opt in globally"
```

**IS:** The panel's `ask_xaman` (the "Ask Xaman Ek" button in Compose mode)
calls `swarm_xaman` without a `consent_token` field. The server's consent gate
requires a token when `curator_consent_default` is `false` — which is the
default (`KaskSwarmSettings::default().curator_consent_default == false`,
pinned by test). So with default settings, every "Ask Xaman Ek" click is
rejected with `ConsentDenied`, and the panel displays "Xaman Ek unavailable:
ABW spend refused: Xaman Ek curator call requires a consent token...".

**OUGHT:** The panel must mint a curate consent token (via
`swarm_request_consent` with `action: "curate"`, `target: "xaman"`,
`credits_authorized: 0`) before calling `swarm_xaman`, and pass it as
`consent_token`. Alternatively, the panel should detect the `ConsentDenied`
error and guide the operator to set `kask.swarm.curator_consent_default: true`.
The current behavior is a broken-by-default feature — the operator clicks a
button and gets an opaque error with no remediation guidance in the UI.

**Pattern signature:** `swarm_xaman` call in panel without `consent_token`
field; `curator_consent_default: false` (default) blocks it.

**Fix suggestion:** In `ask_xaman`, before the `swarm_xaman` call, mint a
curate consent token:
```rust
let consent = invoker.invoke_tool(SWARM_SERVER, "swarm_request_consent",
    json!({ "action": "curate", "target": "xaman", "credits_authorized": 0 })).await;
// extract token, pass as consent_token to swarm_xaman
```
Add a test (panel-side or integration) pinning that `ask_xaman` mints a curate
token before the xaman call. Note the target-mismatch trap: on session
continuation, the server uses `target = session_id`, but the token was minted
for `target = "xaman"` — either mint per-call with the current session_id, or
always use `"xaman"` as the target on the server side.

---

### BH-04 — Consent token consumed before network call; no refund on mid-hire failure

**Severity:** HIGH
**Beizer category:** integration
**Verdict:** Tier 1 BUG (confidence 0.88, reproducible)
**Epistemic mode:** Subjunctive (counterfactual: network drops after consume)
**Provenance:** Inference (control-flow trace)

**Location:** `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:1040-1086`

**Evidence:**
```rust
// The consent gate is the enforcement point: consume the token
// (single-use) and verify it authorizes this exact hire.
self.consent
    .consume(&req.consent_token, "hire", &req.agent_name, req.credits_authorized)
    .map_err(SwarmError::into_tool_error)?;

// Re-verify the hire cost against ABW immediately before spending.
let deps = self.client.get(...).await.map_err(...)?;  // ← can fail here
...
let data = self.client.post(...).await.map_err(...)?;  // ← or here
```

**IS:** `swarm_hire` consumes the consent token (line 1040, removing it from the
store) BEFORE the re-verify GET (line 1057) and the hire POST (line 1080). If
either network call fails — connection drop, ABW 500, timeout — the token is
already consumed. The operator's consent is gone, no spend completed, and there
is no refund/re-mint mechanism. The same pattern exists in `swarm_delegate`
(line 1124) and `swarm_create_swarm` (line 1367).

**OUGHT:** The consent gate's purpose is to authorize a *successful* spend. If
the spend fails, the consent should be restorable so the operator can retry
without re-confirming. The fix is either: (a) consume the token only AFTER the
hire POST succeeds (move `consume` to the end), or (b) on POST failure,
re-insert the grant into the store (refund). Option (a) is simpler but opens a
replay window if the POST succeeds server-side but the response is lost
(network drop after ABW commits) — the operator could retry and double-spend.
Option (b) is safer: consume upfront, refund on failure. The plan's §3.6
"single-use" invariant is preserved either way (a refunded token is still
single-use per *successful* spend).

**Pattern signature:** `consume(...)` followed by `.await.map_err(...)?` on a
network call, with no refund on the error path.

**Fix suggestion:** Add a `ConsentStore::refund(grant)` method that re-inserts
the grant, and call it on every error path after `consume`. Alternatively,
restructure to consume-after-success. Add a test: consume succeeds, POST fails,
token is refundable (re-consumable). Document the chosen invariant in the
`ConsentStore` doc comment.

---

### BH-05 — `swarm_create_swarm` slug generation panics on pre-epoch or mocked clock

**Severity:** MEDIUM
**Beizer category:** coding
**Verdict:** Tier 1 BUG (confidence 0.85, reproducible)
**Epistemic mode:** Declarative
**Provenance:** Direct measurement (source read + Rust slicing semantics)

**Location:** `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:1329-1333`

**Evidence:**
```rust
let slug = format!(
    "{}_{}",
    slug_base.trim_matches('_'),
    &std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_default()[..4]
);
```

**IS:** `duration_since(UNIX_EPOCH)` returns `Err(SystemTimeError)` when the
system clock is before UNIX_EPOCH (possible in CI sandboxes, mocked clocks, or
clock-skew scenarios). `.map(|d| d.as_millis().to_string()).unwrap_or_default()`
then yields `String::default()` = `""` (empty string). The slice `&""[..4]`
panics with "byte index 4 is out of bounds of string". This is a non-startup
path (the tool runs on operator request), so the panic crashes the MCP tool
call, not the server process — but it's still a panic on a recoverable error.

**OUGHT:** A clock error should degrade gracefully (e.g., use a fixed suffix
or a random nonce), not panic. The `.rules` trap "Avoid using functions that
panic like `unwrap()`, instead use mechanisms like `?` to propagate errors"
applies to indexing that can panic too.

**Pattern signature:** `&<expr>.to_string()[..N]` — string slicing with a fixed
index on a dynamically-sized string.

**Fix suggestion:** Use `chars().take(4).collect::<String>()` instead of
`[..4]`, or guard the slice: `let ts = ...; let suffix = if ts.len() >= 4 { &ts[..4] } else { &ts };`. Better: use the full millisecond timestamp (no
slicing) — the 4-digit truncation also creates slug collisions within the same
1000ms window. Add a test with a pre-epoch `SystemTime` (not directly testable
without injection, but the slicing logic can be extracted and tested).

---

### BH-06 — Wallet balance not attached to `swarm_list_agents` or `swarm_get_swarm`; algedonic channel dead on browse

**Severity:** MEDIUM
**Beizer category:** integration
**Verdict:** Tier 1 BUG (confidence 0.90, reproducible)
**Epistemic mode:** Declarative
**Provenance:** Direct measurement (source read + grep for `with_wallet`)

**Location:** `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:743-748` (`swarm_list_agents`) and `:766-778` (`swarm_get_swarm`)

**Evidence (`swarm_list_agents`):**
```rust
Ok(serde_json::json!({
    "count": filtered.len(),
    "authenticated": self.client.is_authenticated(),
    "agents": filtered,
}))
```

**Evidence (`swarm_get_swarm`):**
```rust
match req.workspace_id {
    Some(id) => { let data = ...; Ok(data) }
    None => { let data = ...; Ok(data) }
}
```

**Evidence (panel extraction):**
```rust
// fetch_all, agents spawn:
if let Ok(balance) = &result
    && let Some(b) = extract_wallet_balance(balance)
{ this.wallet_balance = Some(b); }
// fetch_all, swarms spawn:
if let Some(b) = extract_wallet_balance(&output) { this.wallet_balance = Some(b); }
```

**IS:** The panel's `fetch_all` calls `swarm_list_agents` and `swarm_get_swarm`
(the two tools invoked on panel load) and extracts the wallet balance from
each via `extract_wallet_balance`. But neither tool calls `with_wallet` —
`swarm_list_agents` returns a raw JSON object without `wallet`, and
`swarm_get_swarm` returns ABW's response verbatim. So `extract_wallet_balance`
returns `None` for both, and `this.wallet_balance` stays `None` on panel load.
The algedonic channel (the operator's credit balance display in the panel
header) is dead until the operator takes a spend action (hire/create/ask_xaman)
that calls a tool which *does* invoke `with_wallet`.

**OUGHT:** The plan's §4.1 algedonic feedback loop requires the wallet balance
to be "always visible when known." The panel header renders the balance only
when `wallet_balance: Some(_)` — which is never on browse. Either
`swarm_list_agents` and `swarm_get_swarm` should call `with_wallet` (consistent
with the 12 other tools that do), or the panel should issue a separate
`/wallet` fetch on load. The inconsistency is: 12 tools attach the wallet, 2
don't, and those 2 are the ones the panel loads first.

**Pattern signature:** Tool returns `Ok(json!({...}))` without
`self.client.with_wallet(...).await` — grep for `Ok(serde_json::json!` not
preceded by `with_wallet`.

**Fix suggestion:** Add `self.client.with_wallet(...).await` to both
`swarm_list_agents` and `swarm_get_swarm` return paths. Add a test pinning that
`swarm_list_agents` output contains a `wallet` key when authenticated (mirror
the `wallet_envelope_absent_when_unauthenticated` test).

---

### BH-07 — Panel's `create_swarm` silently swallows consent-minting failures and hides `hire_errors`

**Severity:** MEDIUM
**Beizer category:** interface
**Verdict:** Tier 1 BUG (confidence 0.87, reproducible)
**Epistemic mode:** Declarative
**Provenance:** Direct measurement (source read)

**Location:** `crates/swarm_panel/src/swarm_panel.rs:968-972` (swallow) + `:1003-1009` (hide)

**Evidence (swallow):**
```rust
match invoker.invoke_tool(SWARM_SERVER, "swarm_request_consent",
    json!({ "action": "hire", "target": agent, "credits_authorized": 5 })).await
{
    Ok(output) => { ... if let Some(t) = token { consent_tokens.push(t); } }
    Err(_) => {}  // ← silently swallowed
}
```

**Evidence (hide):**
```rust
match result {
    Ok(output) => {
        ...
        this.compose.status = Some(format!("Swarm '{}' created.", name.trim()).into());
        this.fetch_all(cx);
    }
```

**IS:** The panel's `create_swarm` mints a consent token per agent. If minting
fails (network error, auth error, ABW down), the error is silently swallowed
(`Err(_) => {}`), and no token is pushed. The server then receives fewer
tokens than agents and reports "no consent token provided" in `hire_errors`.
But the panel's success handler displays "Swarm 'X' created." without parsing
or showing `hire_errors` from the response. So the operator sees a success
message while all hires silently failed — the swarm is created empty.

**OUGHT:** A partial failure (swarm created, hires failed) must be surfaced to
the operator, not hidden behind a success message. The `.rules` trap "Never
silently discard errors with `let _ =` on fallible operations" applies —
`Err(_) => {}` is the same pattern. The panel should: (1) surface consent
minting failures, (2) parse `hire_errors` from the `swarm_create_swarm`
response and display them in `compose.status`.

**Pattern signature:** `Err(_) => {}` on a fallible `invoke_tool` call;
`Ok(output)` handler that doesn't parse error fields from the response.

**Fix suggestion:** (1) Replace `Err(_) => {}` with `Err(e) => { errors.push(format!("consent for {agent}: {e}")); }` and surface `errors` in the status.
(2) Parse `hire_errors` from the `swarm_create_swarm` response and append to
`compose.status` when non-empty. Add a test pinning that `create_swarm` surfaces
hire failures.

---

### BH-08 — `confirm_hire` casts `u64` total_hire_cost to `u32` without overflow check

**Severity:** LOW
**Beizer category:** data
**Verdict:** Tier 2 POTENTIAL_BUG (confidence 0.65, reproducible)
**Epistemic mode:** Declarative
**Provenance:** Inference (Rust cast semantics)

**Location:** `crates/swarm_panel/src/swarm_panel.rs:719`

**Evidence:**
```rust
let agent_name = pending.agent_name.clone();
let credits = pending.total_hire_cost as u32;
```

**IS:** `pending.total_hire_cost` is `u64` (from `content.get("total_hire_cost").and_then(|c| c.as_u64())`). The cast `as u32` silently truncates if the value
exceeds `u32::MAX` (~4.29 billion). The truncated value is then passed as
`credits_authorized` to `swarm_request_consent` and `swarm_hire`. If ABW ever
returns a cost > 4.29e9 (absurd for credits, but the type allows it), the
panel authorizes fewer credits than the cost, and the server's re-verify
rejects the hire with a confusing "actual cost X exceeds authorized Y" where
Y is the truncated value.

**OUGHT:** Use `u32::try_from(pending.total_hire_cost).unwrap_or(u32::MAX)` or
reject costs exceeding `u32::MAX` with an error. The cast is a silent
precision loss on a safety-critical value (the authorized credit ceiling).

**Pattern signature:** `as u32` cast on a `u64` cost field.

**Fix suggestion:** Replace `pending.total_hire_cost as u32` with
`u32::try_from(pending.total_hire_cost).unwrap_or(u32::MAX)` and add a guard
that rejects costs > `u32::MAX` with a user-facing error. Low priority — the
value is bounded by ABW's credit economy, which is well under 4 billion.

---

### BH-09 — `swarm_xaman` consent target mismatches on session continuation

**Severity:** LOW
**Beizer category:** interface
**Verdict:** Tier 2 POTENTIAL_BUG (confidence 0.70, reproducible)
**Epistemic mode:** Subjunctive
**Provenance:** Inference (control-flow trace)

**Location:** `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:1430-1440`

**Evidence:**
```rust
if !self.client.config().curator_consent_default {
    let target = req.session_id.as_deref().unwrap_or("xaman");
    let Some(token) = req.consent_token.as_deref() else { ... };
    self.consent.consume(token, "curate", target, 0).map_err(...)?;
}
```

**IS:** On the first `swarm_xaman` call (no `session_id`), the consent target
is `"xaman"`. The server creates a session and returns a real `session_id`. On
the second call (continuing, with the real `session_id`), the consent target is
the real `session_id`. If the operator minted a consent token for target
`"xaman"` (first call) and tries to reuse it on the second call, `consume`
fails with scope mismatch ("token is for curate on 'xaman', not curate on
'<session_id>'"). The token is single-use, so this is moot for the *same* token
— but it means the operator must mint a new token per session continuation,
and the target changes unpredictably.

**OUGHT:** The consent target for curate should be stable across a session.
Either always use `"xaman"` as the target (ignore `session_id` for consent
scoping), or document that each continuation requires a fresh token. The
current behavior is a UX trap: the operator minted a token, the first call
succeeded, the second call fails with a scope-mismatch error that doesn't
explain why.

**Pattern signature:** `consume(token, "curate", req.session_id.unwrap_or("xaman"), 0)` — target depends on an optional field that changes across calls.

**Fix suggestion:** Use a fixed target `"xaman"` for all curate consent
consumes (the session_id is an ABW detail, not a consent-scope dimension).
Add a test pinning that a curate token minted for `"xaman"` is consumable
across session continuations.

---

### BH-10 — `swarm_hire` re-verify and `swarm_create_swarm` consume have no tests

**Severity:** MEDIUM
**Beizer category:** requirements
**Verdict:** Tier 2 POTENTIAL_BUG (confidence 0.80, reproducible)
**Epistemic mode:** Declarative
**Provenance:** Direct measurement (test inventory)

**Location:** `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs` (tests module L1583-1817)

**Evidence:**
```
$ cargo test -p hkask-mcp-swarm --no-run  # 20 tests
# Tests cover: detect_embedded_error, consent_consume (6 cases), extract_quoted,
# config_defaults, wallet_envelope, client_url, sanitize_abw_response (3),
# url_encode_segment, config_curator_consent_default.
# NO tests for: swarm_hire re-verify path, swarm_create_swarm consume loop,
# swarm_xaman consent gate, swarm_delegate, swarm_hire cost-rejection path.
```

**IS:** The test suite has 20 tests covering the consent store primitives,
error detection, sanitization, URL encoding, and config defaults. But the
critical spend paths — `swarm_hire`'s 3-step flow (consume → re-verify → POST),
`swarm_create_swarm`'s per-agent consume loop, `swarm_xaman`'s consent gate —
have no tests. The §12.3 re-verify logic (the `actual_cost > credits_authorized`
check) is untested. The §12.4 fix (missing `total_hire_cost` → error) is tested
in `swarm_hire_cost` but not in `swarm_hire`'s re-verify (which uses
`unwrap_or(0)`, BH-01).

**OUGHT:** Weinberg's quality definition: absent tests for critical paths =
quality threat. The spend paths are the highest-risk surface (they spend real
credits). Each should have at least: (1) a happy-path test, (2) a
cost-exceeds-authorized rejection test, (3) a missing-cost-field test. The
panel's `confirm_hire`, `create_swarm`, and `ask_xaman` flows also have no
tests (the 6 panel tests cover only `extract_wallet_balance`,
`steer_system_prompt`, and tool-name pinning).

**Pattern signature:** `#[test]` or `#[tokio::test]` functions matching
`swarm_hire|swarm_create_swarm|swarm_xaman|confirm_hire|create_swarm|ask_xaman`
— zero matches.

**Fix suggestion:** Add integration-level tests (using a mock HTTP layer or by
extracting the logic into testable pure functions) for: (1) `swarm_hire`
rejects when re-verify cost > authorized, (2) `swarm_hire` rejects when ABW
omits `total_hire_cost` in re-verify, (3) `swarm_create_swarm` reports
`hire_errors` when a token is missing, (4) `swarm_xaman` rejects without a
consent token when `curator_consent_default: false`.

---

### BH-11 — `extract_agent_mentions` heuristic produces false positives

**Severity:** LOW
**Beizer category:** data
**Verdict:** Tier 3 OBSERVATION (confidence 0.55, reproducible)
**Epistemic mode:** Probabilistic
**Provenance:** Assessment (heuristic analysis)

**Location:** `crates/swarm_panel/src/swarm_panel.rs:289-301`

**Evidence:**
```rust
for token in text.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
    if token.len() > 3
        && token.contains('_')
        && token.chars().all(|c| c.is_lowercase() || c.is_numeric() || c == '_')
    {
        found.push(token.to_string());
    }
}
```

**IS:** The fallback heuristic in `extract_agent_mentions` matches any token
>3 chars containing `_` with all lowercase/digits/underscores. This matches
non-agent tokens like "the_best", "is_a_test", "step_1", "no_way", etc. The
operator reviews before applying (the "Use team" button pre-fills the agents
field), so the cost is a noisy suggestion list, not a wrong hire. But it
degrades the feature's signal-to-noise ratio.

**OUGHT:** The heuristic is documented as "heuristic by design — the operator
reviews before applying," so this is an observation, not a bug. A tighter
heuristic would cross-reference against the loaded agent catalogue (the panel
already has `entries: Vec<SwarmEntry::Agent>`).

**Pattern signature:** `token.contains('_') && token.chars().all(|c| c.is_lowercase() || ...)` — broad regex-equivalent match.

**Fix suggestion:** Optional: cross-reference extracted mentions against
`self.entries` (the loaded agent list) and filter to known agents. Low
priority — the operator reviews the list.

---

### BH-12 — `swarm_create_swarm` slug truncation creates collisions within 1000ms

**Severity:** LOW
**Beizer category:** data
**Verdict:** Tier 3 OBSERVATION (confidence 0.60, reproducible)
**Epistemic mode:** Declarative
**Provenance:** Inference

**Location:** `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:1329-1333`

**Evidence:**
```rust
&std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_millis().to_string())
    .unwrap_or_default()[..4]
```

**IS:** The slug suffix is the first 4 characters of the millisecond timestamp
string. Current time `as_millis()` ≈ 1.7e12 → "1700000000000" → `[..4]` =
"1700". Two swarms created within the same 1000ms window with the same name
get the same slug. ABW may reject the second as a duplicate, or create two
workspaces with colliding slugs (depending on ABW's uniqueness constraint).

**OUGHT:** Use the full timestamp, or add a random component, to avoid
collisions. The 4-digit truncation serves no purpose — it doesn't make the slug
shorter (the slug is `name_timestamp`, and the name dominates the length).

**Pattern signature:** `[..4]` on a timestamp string.

**Fix suggestion:** Drop the `[..4]` slice and use the full millisecond
timestamp, or use `as_nanos()` for finer granularity. This also fixes BH-05
(the panic on pre-epoch clock).

---

## Taxonomy summary

### By Beizer category

| Category | Count | Finding IDs |
|---|---|---|
| integration | 4 | BH-02, BH-04, BH-06, BH-10 |
| interface | 3 | BH-03, BH-07, BH-09 |
| data | 4 | BH-01, BH-08, BH-11, BH-12 |
| coding | 1 | BH-05 |
| requirements | 1 | BH-10 (also) |

### By severity

| Severity | Count | Finding IDs |
|---|---|---|
| HIGH | 4 | BH-01, BH-02, BH-03, BH-04 |
| MEDIUM | 4 | BH-05, BH-06, BH-07, BH-10 |
| LOW | 4 | BH-08, BH-09, BH-11, BH-12 |

### By verdict

| Verdict | Count |
|---|---|
| Tier 1 BUG (≥0.80) | 8 |
| Tier 2 POTENTIAL_BUG (0.60–0.79) | 3 |
| Tier 3 OBSERVATION (<0.60) | 2 |

---

## Lessons learned

1. **A fix in one path does not propagate to parallel paths.** The §12.4 fix
   replaced `unwrap_or(0)` with an explicit error in `swarm_hire_cost`, but
   the *same* `unwrap_or(0)` pattern survived in `swarm_hire`'s re-verify
   path (BH-01) and the §12.3 re-verify logic was never added to
   `swarm_create_swarm` (BH-02). When fixing a pattern in one call site, grep
   for all call sites of the same pattern in the same file.

2. **A consent gate that consumes before the network call is a one-way
   valve.** The "consume upfront, no refund" pattern (BH-04) means any
   network failure after consume loses the operator's consent with no spend.
   The fix (refund on failure, or consume-after-success) must be applied to
   all three spend tools (`swarm_hire`, `swarm_delegate`, `swarm_create_swarm`).

3. **A feature gated by a default-off setting is broken-by-default if the
   caller doesn't satisfy the gate.** The `curator_consent_default: false`
   gate (BH-03) is correct security policy, but the panel's `ask_xaman` doesn't
   mint the required token — so the feature is broken for every operator who
   hasn't changed the default. The panel must either satisfy the gate or guide
   the operator to change the setting.

4. **The algedonic channel is only as good as its attachment points.** The
   `with_wallet` pattern is applied to 12 tools but not the 2 the panel loads
   first (BH-06). A signal that isn't attached to the tools the UI actually
   calls is a dead signal, regardless of how carefully the extraction is
   written.

5. **String slicing on a dynamically-sized string is a latent panic.** The
   `&string[..4]` pattern (BH-05) panics when the string is shorter than 4
   chars. Use `chars().take(N).collect()` or guard the length. The
   `.unwrap_or_default()` on the `Result` doesn't help — it produces an empty
   string, which still panics on the slice.

6. **Silent error swallowing on fallible operations hides partial failures
   from the operator.** The `Err(_) => {}` pattern (BH-07) is the same class
   as the `.rules` trap "`let _ =` on fallible operations" — it discards an
   error that the operator needs to see. Partial success (swarm created, hires
   failed) must be surfaced, not hidden behind a success message.

---

## Pattern signatures (for next expedition)

These are grep-able patterns derived from actual findings. The next bug-hunt
expedition can seed its Probe phase with these.

| Signature | Finding | Grep pattern |
|---|---|---|
| `unwrap_or(0)` on cost field | BH-01 | `\.get\("total_hire_cost"\).*\.unwrap_or\(0\)` |
| Hardcoded cost in consent consume | BH-02 | `consume\([^)]*,\s*\d+\)` (literal cost) |
| Missing `consent_token` in tool call | BH-03 | `swarm_xaman` call without `consent_token` field |
| Consume-before-network-call | BH-04 | `consume\(` followed by `\.await\.map_err` |
| String slice with fixed index | BH-05 | `\[\.\.\d+\]` on a `.to_string()` result |
| Tool without `with_wallet` | BH-06 | `Ok\(serde_json::json!` not preceded by `with_wallet` |
| `Err(_) => {}` swallow | BH-07 | `Err\(_\)\s*=>\s*\{\s*\}` |
| `as u32` cast on u64 cost | BH-08 | `as u32` on a `total_hire_cost` or `*_cost` field |
| Consent target from optional field | BH-09 | `consume\(.*req\.\w+\.unwrap_or` |
| Missing test for spend path | BH-10 | no `#[test]` matching `swarm_hire|create_swarm|xaman` |
| Broad heuristic token match | BH-11 | `token.contains('_') && token.chars().all` |
| Timestamp truncation slice | BH-12 | `as_millis\(\)\.to_string\(\).*\[\.\.\d+\]` |

---

## Coverage estimate

| Surface | Public functions | Functions with tests | Coverage |
|---|---|---|---|
| MCP server — consent primitives | `mint`, `consume`, `fnv1a` | `mint` (indirect), `consume` (6 cases) | ~80% |
| MCP server — error detection | `detect_embedded_error`, `extract_quoted` | both tested | 100% |
| MCP server — sanitization | `sanitize_abw_response`, `url_encode_segment` | both tested | 100% |
| MCP server — config | `SwarmConfig::from_env`, `default` | `default` tested | 50% |
| MCP server — HTTP client | `send`, `get`, `post`, `wallet_balance`, `with_wallet` | `wallet_balance`, `with_wallet` (1 test) | ~30% |
| MCP server — tools (16) | `swarm_list_agents` ... `swarm_create_app` | 0 tested directly | 0% |
| Panel — extraction | `extract_wallet_balance`, `extract_agent_mentions` | `extract_wallet_balance` (3 tests) | 50% |
| Panel — flows | `fetch_all`, `begin_hire`, `confirm_hire`, `cancel_hire`, `create_agent`, `create_swarm`, `ask_xaman` | 0 tested | 0% |
| Panel — render | `render`, `render_card`, `render_consent_banner`, etc. | 0 tested | 0% |
| Panel — wiring | `init`, `steer_system_prompt` | `steer_system_prompt` (2 tests) | 50% |
| Settings — swarm | `KaskSwarmSettings::default`, `From`, `mcp_env` | `default`, `From` (2 tests) | ~80% |
| Registry — swarm | `BuiltinMcpServer` entry, `filter_*` | 2 tests | 100% |

**Overall estimate:** ~35% of critical paths have test coverage. The consent
primitives and error detection are well-tested; the spend flows and panel
flows are untested. The highest-risk surface (tools that spend real credits)
has 0% direct test coverage.

---

## What §12 covered (not re-reported)

The following §12 findings are confirmed fixed and are NOT re-reported here:
- §12.1: `swarm_request_consent` now calls `require_auth()` ✓
- §12.1: `swarm_curate`/`swarm_xaman` output is sanitized via `sanitize_abw_response` ✓
- §12.2: `swarm_xaman` has a consent gate (when `curator_consent_default: false`) ✓
- §12.3: `swarm_hire` re-verifies cost against ABW ✓ (but see BH-01 for the `unwrap_or(0)` gap)
- §12.4: `swarm_hire_cost` returns error on missing `total_hire_cost` ✓ (but see BH-01 for the re-verify gap)
- §12.5: `KaskSwarmSettings` has `curator_consent_default` field ✓
- §12.6: `SwarmPanel` implements `SerializableItem` ✓
- §12.7: swarm-specific credential-filtering tests exist ✓
- §12.11: reqwest client has connect_timeout + timeout ✓
- §12.12: URL-encoding via `url_encode_segment` ✓
- §12.13: `within_budget` defaults to `false` (fail-closed) ✓
- §12.14: `max_credits` fallback documented as mirroring `Default` ✓

---

*Report generated by the bug-hunt skill (Charter → Probe → Oracle → Taxonomize → Report).*
*All findings cite `file:line` with verbatim evidence. No fabricated findings.*
