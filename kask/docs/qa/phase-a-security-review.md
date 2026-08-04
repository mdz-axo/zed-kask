# Phase A — Security Review (Pre-Release, Post-Hardening)

Date: 2026-08-04. Scope: all kask-owned crates. Read-only audit.
Prior pass: Phase 4 (2 High + 5 Medium fixed) and bug hunt (1 Critical + 3 High + 5 Medium fixed).

## A1 — kali-audit probe verdicts

| # | Probe | Verdict | Evidence |
|---|-------|---------|----------|
| 1 | `redact_spans` sort fix | PASS (see F1/F2) | `hkask-guard/src/pipeline.rs:393-424` — sort by `span.start`, clamped, zero-length safe, no Unicode panics |
| 2 | GuardedStream 256KB cap | PASS | `guarded_inference.rs:121-139` — cap after BOTH text+reasoning deltas; `scanned=true` on breach; error chunk well-formed |
| 3 | IPC timeout + guard-nulling | PASS | `inference_ipc_client.rs:74-81,164-188` — timeout wraps entire `read_line`; every error branch nulls cached stream |
| 4 | Memory recall data boundaries | PASS | `context_injector.rs:55-60` — `format_recall_context` used by all four injector paths; framing-only by design |
| 5 | `debit_if_funds` TOCTOU | PASS | `hkask_ledger.rs:212-237` — `BEGIN IMMEDIATE` precedes in-tx balance re-read; ROLLBACK on error |
| 6 | `LocalSwarmError` mapper | PASS | `hkask-mcp-swarm/src/error.rs:138-148` — variants correct; 3 remaining `internal` sites are genuine serde failures |
| 7 | `SkillExecError` | PASS | `inference_port.rs:141-163` — `From<String>` lossless given the D1 seam's `Result<String,String>` |
| 8 | Corpus per-variant mapper | CONCERN | mapper used at most fs sites, but ~17 blanket `internal(format!` sites remain (see RR-0044) |
| 9 | `tool_schema` re-export | PASS | `hkask-mcp-server/src/tool_schema.rs:14` re-exports both items; imports resolve (source inspection) |
| 10 | `ProvisionError` | PASS | `identity.rs:31-40` — preserves all prior info; passphrase never in errors/logs |
| 11 | SQL injection | PASS | all interpolated SQL uses constant fragments only; user values always bound params |
| 12 | Path traversal | PASS | corpus `contain_for_read/write` at all 12 caller-path sites; research/curator expose no caller-path tools; swarm `agent_id` sanitized |
| 13 | Secrets in logs | PASS (see F3) | no credential values logged; one redaction-convention nit |
| 14 | unsafe audit | CONCERN | FFI in storage/codegraph sound but duplicated; **no `check-unsafe-forbid.sh` exists**; several crate roots (hkask-ledger, hkask-mcp-swarm, most MCP servers) lack `forbid(unsafe_code)` — not mechanically pinned |
| 15 | Layer 4 OCAP control flow | PASS | `hkask-mcp/src/runtime.rs:518-540,594-637` — traced: `is_valid_for` OR `verify_capability_domain`, fail-closed on both paths, denial before dispatch. Matches `.rules` (no signatures, no expiry) |

## New findings

### F1 — MEDIUM: overlapping-span skip in `redact_spans` leaks secret suffix
`pipeline.rs:404-422`. The forward pass skips spans where `start < cursor` with only a warn. If llm-guard emits overlapping matches for one secret (generic-prefix pattern at [10,18), full-key pattern at [10,28)), the short span is redacted, the long one skipped, and the suffix bytes are emitted verbatim: `[REDACTED]def456ghi789`. Exploitability depends on llm-guard's pattern registry containing substring-pattern pairs (plausible: `AKIA` prefix vs full AWS key) — not yet confirmed against the pinned llm-guard version.
**Remedy:** merge-on-overlap (`cursor = max(cursor, end)`, redact the union span) instead of skip. Secondary: the warn fires on non-canonical target `hkask.guard.redact` (L412), violating the P9 `reg.*` namespace convention — the runtime-posture `reg.*` log filter would never see it.

### F2 — LOW: `out_of_order_secrets_all_redacted` may not exercise its claim
`pipeline.rs:624-647` — assertion skipped if `result.passed`; out-of-order emission by the scanner is an unverified assumption. Add a direct unit test of `redact_spans` with a hand-constructed out-of-order (and overlapping, post-F1) `Match` vec.

### F3 — LOW: full WebID logged at provision
`kask_bridge/src/identity.rs:259` — `webid={webid:?}` logs the full UUID; convention (`transport.rs:168`) is `redacted_display()`. Low sensitivity but inconsistent.

### F4 — LOW: unsafe-forbid coverage not mechanically enforced
No `check-unsafe-forbid.sh` script exists despite the audit expectation; several kask crate roots lack the attribute. Add the script (or a cargo-test RR entry) and the missing attributes.

## RR-0044 promotion check — FAIL (do not promote)

54 production (non-test) `McpToolError::internal(format!` sites remain under `kask/mcp-servers/`. Clear mis-classifications:
- `hkask-mcp-media/src/tools/processing.rs:198,724` — caller-path `image::open` (NotFound/PermissionDenied class) mapped to `internal`
- `hkask-mcp-training/src/tools/submit.rs:146,285` — fs I/O mapped to `internal`
- `hkask-mcp-curator:420,522,546`; corpus (`persona/mod.rs` 7, `storage.rs` 5, root 5, misc); `hkask-mcp-companies/providers.rs` 4+1; `hkask-mcp-research:574` (raw JoinError — should reuse `map_join_error`); `hkask-mcp-scenarios:60`

Many remaining sites are legitimately internal (serialize/embedding/parse), so the grep gate cannot distinguish them — either migrate the mis-classified sites or refine the regression pattern. **Keep RR-0044 at `status: pending`.**

## A2 — supply-chain-sentinel verdicts

1. Workspace pinning: **clean** — no version-pinned external deps in kask crates.
2. `cargo deny check`: **fails**, but dominated by upstream-Zed noise (GPL-3.0 workspace crates, unpinned git sources). Kask-relevant: 12 advisory hits with no ignore entries (quick-xml ×2, rsa, rustls-webpki, rustls-pemfile ×2, async-std, bincode, cgmath, instant, proc-macro-error, rustybuzz) and 3 ignore entries that appear not to match under cargo-deny 0.20 (paste, proc-macro-error2, ttf-parser). **Action:** refresh `kask/deny.toml` against the current lockfile; consider scoping so kask regressions aren't drowned in upstream noise. All flagged crates are transitive/upstream, none kask-introduced.
3. schemars removal from hkask-mcp-server: **clean** — no dangling references.
4. thiserror in kask_bridge: **used** (`identity.rs:30`). Not orphaned.
5. Orphaned deps (cargo machete): **3 found** — `log` in `hkask-scenarios-widget`, `hkask-portfolio-widget`, `hkask-kanban-widget`. Trivial removals.
6. Git/wildcard/external-path deps: **clean**.

## A3 — runtime-posture-monitor gap check

- Stale `hkask-mcp-regulation` / `reg.regulation` references: **PASS** (only in the intentional fallback note).
- `hkask.*` performative spans in templates: **PASS**.
- Manifest inputs ↔ template vars: **PARTIAL** — `signal_sources`, `signal` (classify-threat.j2) and `convergence_metric` (select-signal.j2) are referenced but not declared in manifest inputs nor wired via `input_mapping`; steps 2–3 depend on implicit step-result propagation.
- Runnability: **RUNNABLE BUT DEGRADED** — no MCP server exposes span querying; the documented fallback (ledger DB / `reg.*` log scrape / caller-supplied telemetry) is honest and no-fabrication-safe but operationally weak. Note F1's secondary issue makes the log-scrape fallback blind to redaction-skip events.

## A4 — adversarial-red-team assessment

- Defense-layer descriptions in templates now match code (Layer 4: no ed25519 claims, explicitly "no signature verification"; Layer 7: post-hoc redaction caveat verbatim). **PASS.**
- No live target available — simulated evaluation only; gap documented.
- Adversarial probe of `redact_spans` produced F1 (overlapping-span residual leak) — the out-of-order sibling fixed in the prior pass; the overlapping case is the unfixed sibling.

## 8-layer defense coverage matrix (post-fix state)

| Layer | Status | Evidence |
|-------|--------|----------|
| 1 Input validation | Enforced | `hkask-mcp-server/src/server/validation.rs`, swarm `sanitize.rs` |
| 2 Path containment | Enforced | corpus `path_safety.rs` at all caller-path sites |
| 3 Taint (FIDES) | Enforced | `propagate_taint_for_binding` convention, RR-0026 |
| 4 OCAP gate | Enforced (traced) | `runtime.rs:518-540`, fail-closed; no sig/expiry by design |
| 5 Gas/rjoule budget | Enforced | `step.gas_cap` + CyberneticsLoop, fail-closed cap charge |
| 6 Output scanning | Enforced | `guard_output` pipeline |
| 7 GuardedStream | Enforced w/ caveats | post-hoc redaction (documented); 256KB cap correct; **F1 weakens redaction pass** |
| 8 Secret redaction | Enforced w/ caveat | out-of-order fixed; **F1 overlapping-span gap open** |

All 8 layers present. F1 is the only open enforcement gap (Layer 7/8 boundary).
