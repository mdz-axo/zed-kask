# Phase 3 — Skill Maintenance Report

**Date:** 2026-08-03
**Scope:** `kali-audit`, `supply-chain-sentinel`, `runtime-posture-monitor`, `adversarial-red-team`
**Backward-compat note:** No backward-compatibility constraints apply within kask-owned skill artifacts. Manifests, templates, and SKILL.md companions may be updated freely. The D-seam boundary and "do not touch upstream" rule apply in full.
**Mode:** Audit + maintenance. No updates were applied in this phase — findings and recommended updates only.

---

## Metacognition record

| | Prediction | Actual | Brier |
|---|---|---|---|
| Skills with stale references | 1–2 (conf 0.55) | 3 (including 1 Critical) | 0.22 |
| Coverage gaps | 1 (conf 0.55) | 2 | 0.20 |
| Quality drift severity | "minimal" (conf 0.5) | 1 Critical + 1 Stale | 0.36 |

Combined Brier ≈ 0.26. Direction correct; severity underestimated — predicted "stale references" but found a deleted MCP server dependency and an OCAP security-gate misrepresentation.

---

## Summary verdicts

| Skill | Verdict | Score | Top finding |
|-------|---------|-------|-------------|
| `kali-audit` | **Active** | 0.83 | Coverage gap: `kask_bridge` (D8) not in discovery path; regression library doesn't encode `.rules` traps |
| `supply-chain-sentinel` | **Active** | 0.90 | Stale `convergence-check.j2` reference; manifest inputs don't match template contracts |
| `runtime-posture-monitor` | **Critical** | 0.35 | **Data source `hkask-mcp-regulation` deleted**; `reg.regulation` namespace not registered/emitted |
| `adversarial-red-team` | **Stale warning** | 0.68 | Layer 4 "ed25519 DelegationToken" implies signature verification that `.rules` says doesn't exist |

**All 4 skills pass structural validation** (R1–R12, E1–E11, X1–X4). No references to deleted crates (`kask_panel`, `hkask-acp`, `hkask-agents`, `hkask-services-*`, `hkask-goal`) or abolished OCAP features (`OcapConfig`, `required_capabilities`, `CapabilityAwareValidator`, `expires_at`, `ocap:` manifest block). The staleness is in **coverage and content**, not structure.

---

## Pragmatic-semantics classification

| Finding | IS/OUGHT | Epistemic mode | Constraint force | Provenance | Confidence |
|---|---|---|---|---|---|
| `hkask-mcp-regulation` deleted | IS | Declarative | Hard | `kask/docs` + grep | 1.0 |
| `reg.regulation` not registered/emitted | IS | Declarative | Hard | `event.rs` grep + source grep | 1.0 |
| `kask_bridge` not in kali-audit discovery | IS | Declarative | Soft (coverage gap) | Template grep | 0.95 |
| "ed25519 DelegationToken" overstates gate | IS | Declarative | Hard (security claim) | `.rules` + source grep | 0.95 |
| Regression library lacks `.rules` traps | IS | Declarative | Soft | grep | 1.0 |
| `convergence-check.j2` stale ref | IS | Declarative | Soft | File system | 1.0 |

All findings are IS (observed facts). Recommended updates are OUGHT (normative). No Inference-tier claims (all confidence ≥ 0.95).

---

## Detailed findings

### `runtime-posture-monitor` — Critical (0.35)

**F1 [Critical] — Documented MCP server `hkask-mcp-regulation` does not exist.**
- `SKILL.md:88-96` states the skill reads runtime telemetry via the `hkask-mcp-regulation` MCP server using tools `regulation_query_spans` and `reg_span_stats`.
- This server is in the deleted list (`kask/docs/architecture/zed-host-architecture-plan.md:117`).
- The 11 current MCP servers are: codegraph, companies, condenser, corpus, curator, kata-kanban, media, research, scenarios, swarm, training — **no regulation server**.
- A project-wide grep for `regulation_query_spans` / `reg_span_stats` returns exactly one hit: the SKILL.md itself.
- **The skill's primary data path is severed.** It has no functioning telemetry reader.

**F2 [High] — `reg.regulation` is not a registered namespace and is never emitted.**
- `SKILL.md:70-74` claims `reg.regulation` is "registered in `CANONICAL_NAMESPACES`" — it is **not** (`event.rs` has no `reg.regulation` entry).
- `SKILL.md:204` instructs "Emit `reg.regulation` event (feeds CyberneticsLoop)".
- `emit-regulation.j2:43-50` instructs literal emission with `target: "reg.regulation"`.
- Verified: `hkask-regulation/src/cybernetics_loop.rs` emits to `reg.outcome`, `reg.cybernetics.*` — **nothing emits `reg.regulation`**.
- The correct namespace is `reg.outcome` (and `reg.cybernetics.*`).

**F3 [Low] — `hkask.*` "performative spans" framing is loose.**
- `select-signal.j2:1,7` references `hkask.*` performative spans as a signal class, but `CANONICAL_NAMESPACES` contains no `hkask.*` entries.

**Input contract drift [Low]:** manifest `inputs:` declares only `telemetry_stream`, `workspace_context`, but templates reference `target_signal`, `discovered_signals`, `existing_regressions`, `convergence_metric`, `userpod_host` — none in the manifest inputs.

**Recommended updates:**
1. Replace the `hkask-mcp-regulation` tool-dependency block with the actual current span-observation surface, or explicitly document that no runtime telemetry query tool exists (per `.rules` "Advertised invariants need enforcement points" trap).
2. Replace every literal `reg.regulation` emit/count target with `reg.outcome` (and `reg.cybernetics.*` where applicable). Update `SKILL.md:70-74,204,243,348-349` and `emit-regulation.j2:43-50`.
3. Add missing inputs to manifest `inputs:` block.
4. Re-verify the `hkask.*` "performative spans" claim — register them or stop advertising.

### `adversarial-red-team` — Stale warning (0.68)

**F4 [High] — Layer 4 described as `ed25519 DelegationToken` implies signature verification that does not exist.**
- `select-target.j2:63`: "Capability gating (ed25519 DelegationToken per tool call)"
- `generate-adversarial.j2:64`: "Capability gating (ed25519 DelegationToken)"
- `test-against-target.j2:88`: "Layer 4 — Capability gating | Attack induces tool call without valid capability token"
- `.rules` ("Manifest `ocap:` is declared config, not a security gate") states: "Tokens are minted and consumed in-process — there is **no signature verification and no unforgeability**; do not describe the system as providing either."
- `ed25519-dalek` backs **skill-manifest signing** (`kask_extensions_ui`/`collab`), not `DelegationToken`. The real gate is the in-process `(resource, resource_id, action)` equality match + per-agent `mcp_tools` allowlist.
- **The "ed25519" descriptor overstates the gate's strength** and misleads red-team attacks toward a forgery target that isn't the real boundary.

**F5 [Medium] — "Capability token forgery" framing overstates the threat model.**
- `test-against-target.j2:102,133`: "capability_token_forgery" as a behavioral-compromise indicator.
- Per `.rules`, tokens are in-process and not forgeable by an LLM I/O attack — the LLM never holds token material. Probing unauthorized tool-call *attempts* is legitimate, but "forgery" implies a forgeable artifact.

**F6 [Low] — Layer 7 output filtering framed as blocking, but `GuardedStream` is post-hoc redaction.**
- `.rules` ("`GuardedStream` is post-hoc redaction, not real-time blocking") states leaked text has already been forwarded before redaction. A "Layer 7 held" verdict shouldn't claim real-time blocking.

**Recommended updates:**
1. Replace "ed25519 DelegationToken" with: "in-process `DelegationToken` `(resource, resource_id, action)` match at `McpRuntime::invoke` + per-agent `mcp_tools` allowlist at `ToolDispatchPort` (no signature verification, no unforgeability — per `.rules` OCAP rule)".
2. Reframe "capability token forgery" as "unauthorized tool-call / capability-boundary probe".
3. Add a one-line caveat to Layer 7 noting `GuardedStream` is post-hoc redaction.

### `kali-audit` — Active (0.83)

**F7 [High] — Coverage gap: `kask_bridge` (D8) not in discovery path.**
- `select-surface.j2` uses `crates/hkask-*/src/` which matches the 19 `hkask-*` library crates but **not** `kask_bridge` (it doesn't match the `hkask-*` glob).
- `kask_bridge` is the single most security-critical kask crate (OCAP dispatch, inference IPC, settings, all port implementations).

**F8 [High] — Coverage gap: widget crates, `kask_extensions_ui`, `swarm_panel`, D-seam files not in discovery.**
- Widget crates (`crates/hkask-*-widget`, `crates/hkask-viz-core`) — D18, render GPUI elements
- `crates/kask_extensions_ui` — marketplace UI (signing, install verification, credential attachment)
- `crates/swarm_panel` — hosts `tool_invoker.rs`
- D-seam boundary files (D1–D20) in upstream `crates/` — where most `.rules` traps live

**F9 [Medium] — Regression library doesn't encode `.rules` traps.**
- Grepped the regression library (`kask/security/regressions/RR-*.yaml`) for key `.rules` trap concepts: `GuardedStream`, `content` envelope, `AnyJsonValue`, `propagate_taint`, `LazyToolRouter`, `background_spawn`, `tokio`, `credential.*allowlist`, `model_override`, `self-events`, `ordinal-keyed`, `input_mapping`.
- **Result: zero matches.** The forward-adaptable design (consuming the regression library at runtime) is sound but the library itself hasn't been populated with the `.rules` traps — so the design's promise is unfulfilled.

**Recommended updates:**
1. Add `kask/crates/kask_bridge/` to `select-surface.j2` discovery (explicit path or broaden to `crates/*/src/`).
2. Add widget crates, `kask_extensions_ui`, `swarm_panel`, `marketplace_ui_common` to discovery.
3. Add D-seam boundary files to discovery (or add a D-seam surface category).
4. Populate the regression library with `.rules`-derived entries (GuardedStream post-hoc redaction, MCP `content` envelope unwrapping, `AnyJsonValue` schema positions, credential allowlist scoping, tokio/background_spawn, etc.).

### `supply-chain-sentinel` — Active (0.90)

**F10 [Medium] — Stale `convergence-check.j2` reference.**
- `probe.j2:103-104` and `report.j2:133-134` reference `convergence-check.j2` as a separate template.
- No `convergence-check.j2` file exists. Convergence is handled by the `compute` step (`compute_ref: kata.convergence_check`), not a template.
- The reference is factually wrong and contradicts the SKILL.md itself (L256-258: "Convergence is computed deterministically by the executor").

**F11 [Medium] — Manifest `inputs` don't match template contracts.**
- Manifest `inputs` declares `manifest_path` (singular), but templates consume `manifest_paths` (plural), `surface`, `existing_regressions`, `userpod_host`.
- Steps 1–3 have no `input_mapping` blocks (unlike `kali-audit` which maps every step).
- The declared `manifest_path` is never consumed by any template.

**F12 [Low] — Duplicate `go.sum` in `select-surface.j2`.**
- `select-surface.j2:82` lists `go.sum` twice in the file-discovery list.

**Recommended updates:**
1. Fix `probe.j2:103-104` and `report.j2:133-134`: replace "that is `convergence-check.j2` responsibility" with "that is the `compute` step's responsibility (`compute_ref: kata.convergence_check`)".
2. Align manifest `inputs` with template contracts: declare `target_surface`, `manifest_paths`, `existing_regressions`, `userpod_host` (or add `input_mapping` blocks to steps 1–3).
3. Remove the duplicate `go.sum` in `select-surface.j2:82`.

---

## Essentialist 3-gate validation

All 12 findings passed the 3-gate:

- **G1 (Exist):** Each finding represents real staleness or coverage that would cause incorrect results if left unfixed. Complexity doesn't vanish — it reappears as wrong outputs.
- **G2 (Surface):** All recommended updates are minimal (one-line path additions, namespace corrections, text replacements). No new abstractions proposed.
- **G3 (Contract):** Each update aligns the skill with the actual codebase state. The `runtime-posture-monitor` Critical finding (F1) is the strongest — a deleted data source makes the skill non-functional.

---

## Grill-me self-challenge

**Recall:** Which finding makes a skill non-functional vs. merely imprecise?
**Mechanism:** F1 (deleted `hkask-mcp-regulation` server) makes `runtime-posture-monitor` **non-functional** — the skill's primary data path is severed; it has no telemetry to read. F4 (ed25519 DelegationToken) makes `adversarial-red-team` **imprecise** — the skill runs but mischaracterizes the defense layer, directing attacks at a forgery target that doesn't exist. The distinction is: F1 removes the input signal (skill produces nothing); F4 distorts the threat model (skill produces misleading results). F1 is the only finding that blocks the skill from running at all.

---

## Pragmatic-cybernetics loop analysis

The skill-maintenance → skill-update loop is broken for `runtime-posture-monitor`:
- **Polarity:** Corrective (maintenance audit is supposed to surface staleness)
- **Delay:** **Very high** — the `hkask-mcp-regulation` server was deleted (per `zed-host-architecture-plan.md:117`) but the skill still references it. The deletion event didn't trigger a skill update.
- **Gain:** Zero — no CI check validates skill templates against the current MCP server list
- **Closure:** **Broken** — the `check-mcp-servers.sh` CI check verifies `mcp-servers.txt` matches `BUILT_IN_MCP_SERVERS`, but no check verifies skill templates reference only existing servers
- **Fidelity:** Low — the audit found it, but there's no gate preventing recurrence

**Recommendation:** Add a CI check that greps skill `.j2` templates and SKILL.md files for `hkask-mcp-*` references and verifies each matches an entry in `mcp-servers.txt`. This closes the loop.

---

## Updates applied in this session

**None.** Phase 3 is audit-only per the task specification. All findings are documented for the user to prioritize.

## Cross-cutting observation

The regression library (`kask/security/regressions/`) is the designed feedback mechanism for all 4 security skills — skills consume it at runtime to check known regressions. **Zero of the 17+ `.rules` traps are encoded as regression entries.** This is the single highest-leverage update: populating the regression library with `.rules` traps would simultaneously improve all 4 security skills' coverage. The `.rules` traps are already validated patterns (they've been hit multiple times per their own documentation) — they're ready to encode.