# ABW Swarm Integration — Kali Security Audit

**Date:** 2026-08-01
**Auditor:** kali-audit skill (inline run)
**Scope:** `hkask-mcp-swarm` MCP server, `swarm_panel` UI, `kask_bridge` settings/registry, `swarm-intelligence.yaml` manifest
**Prior audits:** §12 (security, 10 fixes), bug-hunt (quality, 12 fixes) — both remediated

## Verdict: **Conditional**

No critical or high findings. 4 medium findings, 3 low findings. 5 of 8 defense layers present, 2 partial, 1 missing. The §12 and bug-hunt remediations closed the highest-risk gaps (prompt-injection → unauthorized-spend, unsanitized curator output, cost re-verification, consent refund). The remaining findings are defense-layer coverage gaps and one advertised invariant without an enforcement point.

## Defense-Layer Coverage Map

| Layer | Name | Status | Evidence |
|-------|------|--------|----------|
| 1 | Input filtering | **Present** | `require_auth()` on 16/17 handlers; `url_encode_segment()` on all path params; empty-string validation on spend paths; `swarm_hire_cost`/`swarm_hire` reject missing `total_hire_cost` (§12.4 + BH-01 fix) |
| 2 | Data/instruction separation | **Partial** | `sanitize_abw_response()` wraps `swarm_xaman` and `swarm_execute_agent` output in `{content, source, trust}` container with injection-prefix stripping. **Gap:** `swarm_generate_prompt`, `swarm_generate_ontology`, `swarm_run_status`, `swarm_create_agent`, `swarm_list_agents` return ABW/LLM content unsanitized (KA-01) |
| 3 | Instruction hierarchy | **Partial** | `steer_system_prompt` establishes the panel's instruction frame. **Gap:** references `swarm_update_swarm` and `GateDecision::Proceed` consent gate that do not exist in the MCP server (KA-04) — the advertised instruction hierarchy has no enforcement point |
| 4 | Capability gating (OCAP) | **Present** | `ConsentStore` single-use tokens, action+target scoped, `require_auth()` on minter, cost re-verification in `swarm_hire`/`swarm_create_swarm`, refund on failure (BH-04 fix). Credential allowlist `Some(&["HKASK_ABW_API_KEY"])` — never `None` |
| 5 | Information flow control | **Missing** | No taint labels on ABW-sourced content. `sanitize_abw_response` is the only IFC-adjacent mechanism, and it's pattern-based, not label-based. FIDES-style Source→Sink blocking not implemented. Out of scope for this audit (workspace-level concern) but noted for completeness |
| 6 | Runtime monitoring | **Present** | `with_wallet()` attaches balance to every spend response; `tracing::warn!` on stale signals (wallet, cost, consent); `detect_embedded_error` catches ABW-wrapped LLM failures; `reg.swarm` target spans |
| 7 | Output filtering | **Partial** | `sanitize_abw_response` strips 5 injection prefixes. **Gap:** only 2 of 7 LLM-output-returning handlers use it (KA-01). No secret redaction (API keys, tokens) in output — though none are logged either |
| 8 | Deception detection | **Missing** | No canary tokens, no decoy tools, no ABW-response canary detection. Out of scope for a single MCP server but noted for completeness |

## Findings

### KA-01 — LLM output returned unsanitized on 5 handlers (MEDIUM)

**Severity:** MEDIUM
**CWE:** CWE-20 (Improper Input Validation), CWE-74 (Injection)
**OWASP LLM 2025:** LLM02:2025 (Sensitive Information Disclosure), LLM01:2025 (Prompt Injection)
**ATLAS:** AML.T0043 (Payload Manipulation)
**NIST SSDF:** SP 800-218A PS.1 (Protect from Injection)
**Missing defense layer:** 2 (Data/instruction separation), 7 (Output filtering)
**Confidence:** 0.90 (Direct measurement)

**Location:** `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs`
- `swarm_generate_prompt` L1283 — returns `data` raw
- `swarm_generate_ontology` L1308 — returns `data` raw
- `swarm_run_status` L1255 — returns `messages: data` raw
- `swarm_create_agent` L1353 — returns `with_wallet(data)` raw
- `swarm_list_agents` L755 — returns `description: a.get("description")` raw

**Evidence:**
```rust
// swarm_generate_prompt (L1283) — no sanitize_abw_response
let data = self.client.post("/agents/generate-prompt", ...).await...?;
Ok(data)  // ← raw LLM output, no sanitization

// swarm_run_status (L1255) — chat messages returned raw
"messages": data,  // ← contains LLM agent output, unsanitized
```

**IS:** Five handlers return ABW/LLM-generated content to the caller without `sanitize_abw_response()`. Only `swarm_xaman` and `swarm_execute_agent` sanitize. ABW agent descriptions, generated prompts, generated ontologies, and workspace chat messages can carry prompt-injection payloads or sensitive data from upstream LLM failures.

**OUGHT:** Every handler that returns LLM-generated text should route through `sanitize_abw_response()` (or a batch variant for arrays). The `swarm_run_status` messages array is the highest-risk surface — it returns full agent chat history, which is the primary vector for an ABW agent to inject instructions into the calling agent's context.

**Remediation:** Apply `sanitize_abw_response()` to the `response`/`description`/`message` fields in all 5 handlers. For `swarm_run_status`, map over the messages array and sanitize each message's content. Add a test pinning that `swarm_run_status` output is wrapped in the `{content, source, trust}` container.

---

### KA-02 — `swarm_list_agents` skips `require_auth()` (MEDIUM)

**Severity:** MEDIUM
**CWE:** CWE-306 (Missing Authentication for Critical Function), CWE-200 (Information Exposure)
**OWASP LLM 2025:** LLM06:2025 (Excessive Agency — unauthenticated enumeration)
**ATLAS:** AML.T0040 (Information Gathering)
**NIST SSDF:** SP 800-218A AC.1 (Access Control)
**Missing defense layer:** 4 (Capability gating — partial)
**Confidence:** 0.85 (Direct measurement)

**Location:** `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:707-712`

**Evidence:**
```rust
pub async fn swarm_list_agents(&self, parameters: Parameters<ListAgentsRequest>) -> String {
    execute_tool_semantic(self, "swarm_list_agents", Some("dublin-core"), async {
        let req = parameters.0;
        let data = self.client.get("/agents").await...?;  // ← no require_auth()
```

**IS:** `swarm_list_agents` is the only handler that does not call `require_auth()`. An unauthenticated MCP client (no `HKASK_ABW_API_KEY`) can enumerate the full ABW agent catalogue — agent names, types, descriptions, models, dependencies, execution stats. Every other read handler (`swarm_get_agent`, `swarm_get_swarm`, `swarm_list_apps`) requires auth.

**OUGHT:** If catalogue browsing in catalogue-only mode is intentional (the `is_authenticated` field in the response suggests so), the handler should still gate access: either (a) require auth like every other handler, or (b) return an empty/redacted list when unauthenticated, not the full catalogue. The current behavior is an information-exposure surface: an attacker who gains MCP access without an API key can map the operator's available agent surface.

**Remediation:** Add `require_auth()` to `swarm_list_agents`, or filter the response to redact `description`/`dependencies`/`execution_stats` when `!is_authenticated()`. Add a test pinning that unauthenticated calls return an error or a redacted list, not the full catalogue.

---

### KA-03 — `swarm_create_swarm` slug generation panics on pre-epoch clock (MEDIUM)

**Severity:** MEDIUM
**CWE:** CWE-754 (Improper Check for Unusual or Exceptional Conditions), CWE-248 (Uncaught Exception → DoS)
**OWASP LLM 2025:** LLM06:2025 (Excessive Agency — unhandled panic)
**ATLAS:** AML.T0050 (Denial of Service)
**NIST SSDF:** SP 800-218A RV.1 (Vulnerability Handling)
**Missing defense layer:** 1 (Input filtering — runtime input is the clock)
**Confidence:** 0.95 (Direct measurement — reproduced by code inspection)

**Location:** `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:1394-1399`

**Evidence:**
```rust
let slug = format!(
    "{}_{}",
    slug_base.trim_matches('_'),
    &std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_default()[..4]  // ← panics on empty string
);
```

**IS:** When `SystemTime::now()` is before `UNIX_EPOCH` (pre-epoch, mocked clock, or system clock corruption), `duration_since` returns `Err`, `unwrap_or_default()` produces an empty `String`, and `&string[..4]` panics with "byte index 4 is out of bounds of string". This is an unhandled panic in a tool handler — a DoS vector. The bug hunt flagged this as BH-05; it remains unfixed.

**OUGHT:** Use `.chars().take(4).collect::<String>()` or `get(..4).unwrap_or("0")` instead of indexing. A pre-epoch clock is a runtime condition, not a programmer error — the handler should degrade gracefully (e.g. slug = `{base}_0`), not panic.

**Remediation:** Replace `&string[..4]` with `string.get(..4).unwrap_or("0")`. Add a test pinning that slug generation with a pre-epoch clock produces a valid slug, not a panic.

---

### KA-04 — Advertised consent gate (`swarm_update_swarm`/`DispatchIntent`) does not exist (MEDIUM)

**Severity:** MEDIUM
**CWE:** CWE-1037 (Processor Protection Failure — advertised invariant not enforced)
**OWASP LLM 2025:** LLM06:2025 (Excessive Agency — consent gate advertised but absent)
**ATLAS:** AML.T0051 (Trust Boundary Violation)
**NIST SSDF:** SP 800-218A PW.1 (Architecture — advertised control not implemented)
**Missing defense layer:** 3 (Instruction hierarchy), 4 (Capability gating — partial)
**Confidence:** 0.95 (Direct measurement — grep confirmed absence)

**Location:**
- `crates/swarm_panel/src/swarm_panel.rs:96-110` (steer prompt advertises `swarm_update_swarm`, `GateDecision::Proceed`, `DispatchIntent`)
- `kask/registry/manifests/swarm-intelligence.yaml:51,177,195,199` (skill manifest references same)
- `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs` — **0 matches** for `swarm_update_swarm`, `GateDecision`, `DispatchIntent`

**Evidence:**
```rust
// steer_system_prompt (panel) — advertises a gate that doesn't exist:
"acts via gated swarm_update_swarm/swarm_delegate calls with a DispatchIntent \
 consent gate ... Do not hire or delegate without the skill's consent gate \
 producing a GateDecision::Proceed."
```
```yaml
# swarm-intelligence.yaml (manifest) — same:
- mandatory: no swarm_update_swarm or swarm_delegate call is emitted
```
```rust
// MCP server — 0 implementations:
grep -c 'swarm_update_swarm\|GateDecision\|DispatchIntent' hkask_mcp_swarm.rs  // → 0
```

**IS:** The panel's steer prompt and the `swarm-intelligence` skill manifest both advertise a `DispatchIntent` consent gate enforced via `swarm_update_swarm` that must produce `GateDecision::Proceed` before any hire/delegate. The MCP server does not implement `swarm_update_swarm`, `DispatchIntent`, or `GateDecision`. The advertised consent gate has no enforcement point — the `.rules` trap "Advertised invariants need enforcement points" applies directly. An agent following the steer prompt would wait for a `GateDecision::Proceed` that never comes, or (worse) interpret the absence as "no gate needed" and proceed.

**OUGHT:** Either (a) implement `swarm_update_swarm` with a `DispatchIntent` consent gate in the MCP server, or (b) remove the references from the steer prompt and skill manifest and document that the `ConsentStore` (which *is* enforced) is the actual consent gate. Option (b) is the surgical fix; option (a) is the full plan §4 implementation (slice 6 work).

**Remediation:** If not implementing `swarm_update_swarm` now, update the steer prompt and manifest to reference the actual `ConsentStore`-backed `swarm_request_consent`/`swarm_hire`/`swarm_delegate` flow instead of the nonexistent `DispatchIntent`/`GateDecision` machinery. Add a test pinning that the steer prompt references only tools that exist in the MCP server.

---

### KA-05 — `swarm_create_agent` hardcodes model name `claude-haiku-4-5-20251001` (LOW)

**Severity:** LOW
**CWE:** CWE-1047 (Hardcoded Sensitive Information — model name, not secret)
**OWASP LLM 2025:** N/A (operational hygiene, not a security risk)
**Missing defense layer:** N/A
**Confidence:** 0.90 (Direct measurement)

**Location:** `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:1336`

**Evidence:**
```rust
"model": req.model.unwrap_or_else(|| "claude-haiku-4-5-20251001".to_string()),
```

**IS:** The default model for new ABW agents is hardcoded as `claude-haiku-4-5-20251001`. This is the code-level analog of the `.rules` trap "Manifests must not hardcode model names in the `fusion` block" — model names go stale. When Anthropic renames/deprecates this model, every new agent created with the default will fail or use a stale name.

**OUGHT:** The default model should come from `SwarmConfig` (operator-configurable via `HKASK_ABW_DEFAULT_MODEL` env var), not a code literal. The manifest correctly omits hardcoded model names; the code should follow the same pattern.

**Remediation:** Add `default_model: String` to `SwarmConfig` with an env-var override, and use `req.model.unwrap_or_else(|| self.client.config().default_model.clone())`. Low priority — operational hygiene, not a security vulnerability.

---

### KA-06 — `swarm_delegate` task interpolated raw into @mention (LOW)

**Severity:** LOW
**CWE:** CWE-74 (Injection — semantic, not syntactic)
**OWASP LLM 2025:** LLM01:2025 (Prompt Injection — at the ABW chat layer)
**ATLAS:** AML.T0043 (Payload Manipulation)
**Missing defense layer:** 1 (Input filtering — semantic layer)
**Confidence:** 0.70 (Inference — ABW chat semantics)

**Location:** `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:1209`

**Evidence:**
```rust
&serde_json::json!({ "content": format!("@{} {}", req.agent_name, req.task) }),
```

**IS:** The `task` string is interpolated raw into the ABW chat message content. If `task` contains `@other_agent do something else`, it would mention another agent in the workspace, potentially triggering unintended delegation. This is a semantic injection at the ABW chat layer, not a JSON injection (serde handles the encoding).

**OUGHT:** This is a low-severity finding because the operator authorizes the task via the consent gate before it's sent. The ABW chat layer is the trust boundary. A future hardening could strip leading `@` characters from `task` or warn when `task` contains `@`, but this is defense-in-depth, not a primary control.

**Remediation:** Optional — strip leading `@` from `req.task` or warn on `@` presence. Not blocking.

---

### KA-07 — `swarm_generate_prompt` missing input validation (LOW)

**Severity:** LOW
**CWE:** CWE-20 (Improper Input Validation)
**OWASP LLM 2025:** LLM01:2025 (Prompt Injection — unvalidated input to LLM)
**Missing defense layer:** 1 (Input filtering)
**Confidence:** 0.80 (Direct measurement)

**Location:** `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:1270-1273`

**Evidence:**
```rust
pub async fn swarm_generate_prompt(...) -> String {
    ...
    let req = parameters.0;
    // ← no empty-string check on req.description or req.agent_name
    let data = self.client.post("/agents/generate-prompt", ...).await...?;
```

**IS:** `swarm_generate_prompt` does not validate that `description` and `agent_name` are non-empty, unlike `swarm_create_agent` (L1326), `swarm_hire` (L1051), `swarm_xaman` (L1557), and `swarm_create_swarm` (L1378). An empty description would send a degenerate request to ABW's LLM.

**OUGHT:** Add the same `trim().is_empty()` guard the other handlers use. Low severity because ABW likely rejects the empty input server-side, but the client should fail fast.

**Remediation:** Add `if req.description.trim().is_empty() || req.agent_name.trim().is_empty() { return Err(...) }` after `let req = parameters.0;`.

## Proposed Regression Entries

### RR-0040 — LLM output sanitization coverage

```yaml
id: RR-0040
title: "All LLM-output-returning swarm handlers must sanitize via sanitize_abw_response"
kind: cargo-test
surface: mcp
severity: medium
owasp: [LLM02:2025, LLM01:2025]
cwe: [CWE-20, CWE-74]
atlas: [AML.T0043]
source: kask/docs/audits/abw-swarm-kali-audit.md KA-01
detection:
  - grep -n 'Ok(data)' kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs
  - grep -n 'messages.*data' kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs
test: |
  # Every handler returning LLM output must route through sanitize_abw_response.
  # Pin: swarm_run_status messages are wrapped in {content, source, trust}.
  fn swarm_run_status_output_is_sanitized() { ... }
```

### RR-0041 — `swarm_list_agents` auth gate

```yaml
id: RR-0041
title: "swarm_list_agents must require auth or redact when unauthenticated"
kind: cargo-test
surface: mcp
severity: medium
owasp: [LLM06:2025]
cwe: [CWE-306, CWE-200]
atlas: [AML.T0040]
source: kask/docs/audits/abw-swarm-kali-audit.md KA-02
detection:
  - grep -n 'fn swarm_list_agents' ... | check require_auth in body
test: |
  fn swarm_list_agents_rejects_unauthenticated() { ... }
```

### RR-0042 — `swarm_create_swarm` slug panic

```yaml
id: RR-0042
title: "swarm_create_swarm slug generation must not panic on pre-epoch clock"
kind: cargo-test
surface: mcp
severity: medium
cwe: [CWE-754, CWE-248]
atlas: [AML.T0050]
source: kask/docs/audits/abw-swarm-kali-audit.md KA-03
detection:
  - grep -n '\[\.\.4\]' kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs
test: |
  fn slug_generation_handles_pre_epoch_clock() { ... }
```

### RR-0043 — Steer prompt references only existing tools

```yaml
id: RR-0043
title: "steer_system_prompt must not reference nonexistent MCP tools"
kind: cargo-test
surface: panel
severity: medium
cwe: [CWE-1037]
owasp: [LLM06:2025]
source: kask/docs/audits/abw-swarm-kali-audit.md KA-04
detection:
  - grep -o 'swarm_\w*' crates/swarm_panel/src/swarm_panel.rs steer prompt
  - diff against grep -o 'pub async fn swarm_\w*' hkask_mcp_swarm.rs
test: |
  fn steer_prompt_references_only_existing_tools() { ... }
```

## Top 3 Highest-Priority Fixes

1. **KA-01 (MEDIUM):** Apply `sanitize_abw_response()` to the 5 unsanitized LLM-output handlers. `swarm_run_status` is the highest-risk surface — it returns full agent chat history, the primary injection vector. This is the largest defense-layer gap (layers 2 and 7).

2. **KA-04 (MEDIUM):** Resolve the phantom consent gate. Either implement `swarm_update_swarm`/`DispatchIntent` (slice 6) or update the steer prompt and skill manifest to reference the actual `ConsentStore`-backed flow. The current state is an advertised invariant with no enforcement point — the `.rules` trap.

3. **KA-02 (MEDIUM):** Add `require_auth()` to `swarm_list_agents` or redact the response when unauthenticated. An unauthenticated MCP client can currently enumerate the full agent catalogue.

## What §12 and bug-hunt already fixed (not re-reported)

- Prompt-injection → unauthorized-spend chain (§12.1): `require_auth()` on `swarm_request_consent`
- Curator consent gate (§12.2): `curator_consent_default` setting + `swarm_xaman` gate
- Cost re-verification (§12.3 + BH-01 + BH-02): `swarm_hire` and `swarm_create_swarm` re-fetch deps
- `unwrap_or(0)` on cost (§12.4 + BH-01): explicit error + warn
- Error mapping 402/429 (§12.10): preserved unchanged
- Reqwest timeouts (§12.11): `connect_timeout(10s)` + `timeout(60s)`
- URL encoding (§12.12): `url_encode_segment()` on all path params
- Consent refund on failure (BH-04): `ConsentStore::refund()` on error paths
- Curate target stability (BH-09): fixed `"xaman"` target
- Panel `ask_xaman` consent (BH-03): mints curate token before call
- Panel `create_swarm` cost fetch (BH-02): fetches real cost before minting consent
- Panel `create_swarm` error surfacing (BH-07): surfaces `hire_errors` and consent failures
- `SerializableItem` (§12.6): implemented
- `within_budget` fail-closed (§12 fix): default `false`
- Manifest no hardcoded model names (verified clean)
- Credential allowlist `Some(&[...])` (verified clean)
- `#![forbid(unsafe_code)]` (verified present)
