# Phase 4 — Security Posture Report

**Date:** 2026-08-04
**Scope:** kask-owned crates (`kask/crates/*`, `kask/mcp-servers/*`, `crates/hkask-*-widget`, `crates/hkask-viz-core`, `crates/kask_extensions_ui`, `crates/swarm_panel`, `crates/marketplace_ui_common`) + D-seam boundary (D1–D20).
**Backward-compat note:** No backward-compatibility constraints apply within kask-owned crates. The D-seam boundary and "do not touch upstream" rule apply in full.
**Mode:** Security audit. No code was modified.

---

## Metacognition record

| | Prediction | Actual | Brier |
|---|---|---|---|
| Blockers | 1–2 (conf 0.5) | 0 | 0.25 |
| High findings | 3–5 (conf 0.5) | 1 (RR-0030) | 0.25 |
| Defense layer gaps | 2–3 (conf 0.5) | 1 (Layer 7 test gap) | 0.25 |

Combined Brier ≈ 0.25. Overestimated blocker count (the GuardedStream and token-expiry issues are known/accepted, not blockers) and high-count (most traps are already enforced). Underestimated the defense layer's test coverage gap.

---

## Skills run

| Skill | Result | Notes |
|---|---|---|
| `kali-audit` | ✅ Ran | Full audit of 20 library crates + 11 MCP servers + 6 widget crates + 3 extension crates + D-seam files |
| `supply-chain-sentinel` | ✅ Ran | Dependency manifest audit (Cargo.toml, deny.toml, Cargo.lock) |
| `runtime-posture-monitor` | ❌ **Cannot run** | Data source `hkask-mcp-regulation` deleted (Phase 3 finding F1). No live telemetry reader. **Gap documented.** |
| `adversarial-red-team` | ❌ **Cannot run** | No live deployed target available. **Gap documented.** |

---

## Pragmatic-semantics classification

| Finding | IS/OUGHT | Epistemic mode | Constraint force | Provenance | Confidence |
|---|---|---|---|---|---|
| RR-0030 enforcement test missing | IS | Declarative | Hard (security) | grep + source read | 1.0 |
| `kask_bridge` missing unsafe-gating | IS | Declarative | Hard (security) | source read | 1.0 |
| MCP error classification gaps | IS | Declarative | Soft (convention) | source grep | 0.95 |
| GuardedStream post-hoc redaction | IS | Declarative | Hard (accepted design) | source read + .rules | 1.0 |
| Token expiry not enforced | IS | Declarative | Hard (accepted design) | source read + .rules | 1.0 |
| Runtime posture monitor cannot run | IS | Declarative | Hard (data source deleted) | Phase 3 finding | 1.0 |

All findings are IS (observed facts). No Inference-tier claims.

---

## Findings

### High

| # | Finding | File(s) | Category | Description | .rules trap | Enforcement |
|---|---------|---------|----------|-------------|-------------|-------------|
| H1 | **RR-0030 enforcement test missing** | `kask/security/regressions/RR-0030.yaml` + `kask/crates/hkask-guard/src/guarded_inference.rs` | OWASP LLM07 / CWE-200 | The `reasoning`/`reasoning_delta` channel IS scanned in the code (`scan_output` on `accumulated_reasoning` at L62, `sanitize_reasoning` at L150). But the enforcement test `canary_in_reasoning_field_is_redacted` does NOT exist anywhere in the codebase. The regression is marked `status: enforced` with `ci_gate: scripts/check-kali-regressions.sh`, but `check-kali-regressions.sh` reports "matched 0 tests" — the test is missing. If someone removes the reasoning scan, nothing catches it. | "Advertised invariants need enforcement points" | **Missing** — code is correct, test is not |
| H2 | **`kask_bridge` missing unsafe-gating attribute** | `kask/crates/kask_bridge/src/kask_bridge.rs:9` | CWE-732 / OWASP LLM06 | The D8 bridge crate (sole bidirectional seam — OCAP dispatch, inference IPC, settings, all port implementations) has no `#![forbid(unsafe_code)]` or `#![deny(unsafe_code)]`. Every other kask crate has one. The `check-unsafe-forbid.sh` script's detection pattern (`crates/*/src/lib.rs` + `mcp-servers/*/src/lib.rs`) doesn't match `kask/crates/kask_bridge/src/kask_bridge.rs` (different path + non-`lib.rs` filename per the `[lib] path` convention). | RR-0020 scope gap | **Missing** — script doesn't scan this path |

### Medium

| # | Finding | File(s) | Category | Description | .rules trap |
|---|---------|---------|----------|-------------|-------------|
| M1 | **MCP error classification — corpus server** | `kask/mcp-servers/hkask-mcp-corpus/src/services/convert.rs:804,820,825,848,878,881,889,892`; `tools/document.rs:76`; `tools/gather/mod.rs:198,216`; `tools/corpus/mod.rs:119,124`; `tools/persona/mod.rs:302,323,372,453,457,482,484` | OWASP LLM06 / CWE-440 | The corpus server has `map_corpus_io_error` (correctly classifies `NotFound`/`PermissionDenied`) but 20+ call sites use blanket `McpToolError::internal(format!(...))` instead. An IO permission error and a serialize bug are indistinguishable to the consumer. | "MCP tool error classification — classify per variant" |
| M2 | **MCP error classification — media, training, swarm servers** | `hkask-mcp-media/src/tools/audio.rs:25,270,276`; `hkask-mcp-training/src/tools/dataset.rs:80,140`; `hkask-mcp-training/src/tools/submit.rs:144`; `hkask-mcp-swarm/src/hkask_mcp_swarm.rs:1927,1934,1937,2025,2027,2079,2096,2177,2189` | OWASP LLM06 / CWE-440 | These servers have proper per-variant error mappers (`map_media_error`, `map_dataset_error`, `SwarmError::into_tool_error`) but some call sites bypass them with blanket `McpToolError::internal` for file I/O errors. | "MCP tool error classification — classify per variant" |
| M3 | **`Result<_, String>` in swarm local-runtime** | `kask/mcp-servers/hkask-mcp-swarm/src/{local_runtime,local_swarms,local_registry,local_knowledge,consent,a2a_http,abw_util}.rs` | CWE-440 | 19 `Result<_, String>` signatures erase error kind at the source; the tool-method boundary blanket-maps everything to `McpToolError::internal`. | "MCP tool error classification" + `check-string-errors.sh` |
| M4 | **`supply-chain-sentinel` stale `convergence-check.j2` reference** | `kask/registry/templates/supply-chain-sentinel/probe.j2:103-104`; `report.j2:133-134` | — | Templates reference a `convergence-check.j2` file that doesn't exist. Convergence is handled by `compute_ref: kata.convergence_check`, not a template. Contradicts the skill's own SKILL.md. | — |
| M5 | **`adversarial-red-team` Layer 4 overstates OCAP gate** | `kask/registry/templates/adversarial-red-team/{select-target,generate-adversarial}.j2` | OWASP LLM06 | "ed25519 DelegationToken" implies signature verification. `.rules` explicitly says: "no signature verification and no unforgeability." The real gate is in-process `(resource, resource_id, action)` match. Misleads red-team attacks toward a forgery target that isn't the real boundary. | "Manifest `ocap:` is declared config, not a security gate" |

### Low (known, accepted)

| # | Finding | File(s) | Description |
|---|---------|---------|-------------|
| L1 | `GuardedStream` is post-hoc redaction | `kask/crates/hkask-guard/src/guarded_inference.rs:50-126` | Leaked text is forwarded in real-time chunks; redaction only sanitizes the stored version. Known, accepted (RR-0023 enforced). |
| L2 | `DelegationToken` has no expiry | `kask/crates/hkask-capability/src/token_types.rs:24-89` | `is_valid_for` checks only `(resource, resource_id, action)` equality. No `expires_at` field. Known, accepted (in-process tokens). |
| L3 | `deny(unsafe_code)` + `#[allow]` overrides | `hkask-storage/src/core/database.rs:55`; `hkask-mcp-codegraph/src/codegraph/graph/store.rs:30` | Weaker than `forbid` — a future developer could add another `#[allow]` without FFI justification. Accepted per RR-0020. |

---

## 8-Layer Defense Coverage Matrix

| Layer | Name | Present | Key file | Enforced by | Test gap |
|-------|------|---------|----------|-------------|----------|
| 1 | Input filtering (guard_input) | ✅ | `hkask-guard/src/guarded_inference.rs:174` | RR-0001 | — |
| 2 | Data/instruction separation | ✅ | `hkask-guard/src/spotlight.rs` | RR-0011 | — |
| 3 | Instruction hierarchy | ✅ | `hkask-templates` (template execution) | RR-0010 | — |
| 4 | Capability gating (OCAP) | ✅ | `hkask-mcp/src/runtime.rs:508-607` (`invoke`) | — | **No regression entry** |
| 5 | Information flow control | ✅ | `hkask-templates/src/executor.rs` (`propagate_taint_for_binding`) | RR-0013, RR-0026, RR-0027, RR-0033, RR-0034 | — |
| 6 | Runtime monitoring | ✅ | `hkask-regulation/src/cybernetics_loop.rs` | RR-0012 | — |
| 7 | Output filtering (guard_output + GuardedStream) | ✅ | `hkask-guard/src/guarded_inference.rs:187` + `GuardedStream` | RR-0023, RR-0024, RR-0030, RR-0035, RR-0039 | **RR-0030 test missing (H1)** |
| 8 | Deception detection (CanaryToken) | ✅ | `hkask-guard/src/pipeline.rs:23-63` | RR-0014 | — |

**Summary:** 8/8 layers present. 1 test gap (Layer 7: RR-0030 enforcement test missing). 1 layer without a regression entry (Layer 4: OCAP capability gating has no RR entry, though the code is correct).

---

## Skill gaps documented

### `runtime-posture-monitor` — cannot run

**Root cause:** The skill's data source (`hkask-mcp-regulation` MCP server) was deleted. The 11 current MCP servers do not include a regulation server. The skill's tools (`regulation_query_spans`, `reg_span_stats`) do not exist anywhere in the codebase.

**Impact:** Runtime security posture monitoring cannot be performed. No tool exists to query span history at runtime. The skill is non-functional.

**Remediation options:**
1. **Re-point** the skill to an existing telemetry surface (e.g., direct `hkask-ledger` read path, or add a `regulation_query` tool to an existing MCP server like `hkask-mcp-curator`)
2. **Document** the gap explicitly in the SKILL.md (per `.rules` "Advertised invariants need enforcement points" — an advertised tool dependency with no enforcement point is theater)
3. **Retire** the skill if runtime posture monitoring is not a current priority

### `adversarial-red-team` — no live target

**Root cause:** Adversarial testing requires a live deployed hKask instance with active inference and MCP tool dispatch. No such instance is available in this local development environment.

**Impact:** Cannot generate adversarial inputs against the deployed defense stack or evaluate resistance rates.

**Remediation:** Deploy a live instance (or use the `run-background-agent-mvp-local` script) and re-run `adversarial-red-team` against it. The skill's methodology is sound (per Phase 3 audit: Stale warning 0.68 — structurally valid, Layer 4 description needs correction but the attack categories and persistence levels are internally consistent).

---

## Supply-chain audit — pass items

| Check | Status | Evidence |
|-------|--------|---------|
| Registry verification | ✅ | `deny.toml:59-60`: `unknown-registry = "deny"`, `unknown-git = "deny"` |
| Git dep pinning | ✅ | All 15 git deps have `rev` pins. No kask-specific crates have git deps. |
| License configuration | ✅ | 16 allowed licenses, `confidence-threshold = 0.8`, `ring` exception |
| Advisory ignores | ⚠️ Watch | 3 ignored advisories with review dates (2026-07-22) and re-review (2026-10-22) |
| Unbounded version specs | ✅ | No `*` or unbounded `>=` in kask crates |
| Orphaned workspace deps | ✅ | All 18 kask-specific workspace deps used by ≥1 crate |
| Model-name constant duplication | ✅ | Single source of truth at `hkask_inference::model_constants` |
| `propagate_taint_for_binding` coverage | ✅ | All `input_mapping` call sites call it before `context.insert` |
| MCP credential scoping | ✅ | Per-server allowlists, fail-closed, tests pin behavior (RR-0038) |
| `AnyJsonValue` in MCP tool inputs | ✅ | No `serde_json::Value` in `#[derive(JsonSchema)]` request structs |
| `parse_tool_response` envelope unwrapping | ✅ | Single seam in `hkask_types::tool_response`, used in 19 swarm_panel call sites |
| `llm-guard` exact pin | ✅ | Pinned to `=0.2.0` (RR-0025, RR-0029) |
| `deny.toml` `[bans]` | ✅ | `libsodium`/`libsodium-sys` banned, `multiple-versions = "warn"` |

---

## Essentialist 3-gate validation

All findings passed:
- **G1 (Exist):** Each finding represents a real gap (missing test, missing attribute, error classification) — complexity doesn't vanish if ignored.
- **G2 (Surface):** Remediations are minimal — add a test, add an attribute, route errors through existing mappers.
- **G3 (Contract):** Each update aligns with `.rules` traps and existing patterns.

---

## Grill-me self-challenge

**Recall:** Is RR-0030 a code bug or a test gap?
**Mechanism:** It's a **test gap**, not a code bug. The code is correct — `GuardedStream` accumulates `reasoning_delta` (L116) and scans it at stream end (L62: `scan_output(&this.accumulated_reasoning)`). The non-streaming path calls `sanitize_reasoning` (L150-156) which scans `result.reasoning`. The mitigation IS implemented. But the test `canary_in_reasoning_field_is_redacted` that's supposed to pin this behavior doesn't exist. If someone removes the reasoning scan in a future refactor, no test catches it — the regression protection is only on paper (`status: enforced` in the YAML), not in code. The fix is to write the test, not to fix the guard.

---

## Pragmatic-cybernetics loop analysis

The `check-kali-regressions.sh` → RR enforcement loop is broken for RR-0030:
- **Polarity:** Corrective (the check is supposed to verify enforcement tests exist)
- **Delay:** Low — `check-kali-regressions.sh` runs and reports "1 cargo-test failure(s)" immediately
- **Gain:** Zero — the check reports the failure but doesn't block CI (it's not in `kask-ci.yml`). The failure is advisory.
- **Closure:** **Broken** — the check surfaces the missing test, but the failure isn't promoted to a CI gate, so the gap persists
- **Fidelity:** High — the check correctly identifies the exact missing test name

**Recommendation:** Promote `check-kali-regressions.sh` to CI (advisory → blocking) to close the loop on RR-0030 and prevent future missing enforcement tests.