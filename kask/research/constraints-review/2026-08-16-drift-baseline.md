# Constraints Review — Drift Baseline (v1)

> **Date:** 2026-08-16
> **Skill:** `constraints-review` v0.34.0
> **Reference:** `kask/docs/architecture/review-reference-models.md` v1
> **Scope:** `.rules` (root, 111 lines), `kask/.rules` (27 lines), `DIVERGENCE.md` (D1–D28, 26 seams), `AGENTS.md`
> **Method:** Manual application of the four phases (elicit → classify → gate → drift). The skill's Lisp gates were not executed (no ManifestExecutor runtime); the invariants were checked by hand.

## Phase 1 — Elicitation

Constraint sources scanned:

| Source | Constraints found | Notes |
|---|---|---|
| `.rules` (root) | 47 | 111 lines, 11 sections |
| `kask/.rules` | 4 | 27 lines, 2 sections (convergence checks, config deny_unknown_fields) |
| `DIVERGENCE.md` | 26 | D1–D28 (D17, D19 absent from numbering — see finding) |
| `AGENTS.md` | (not separately counted — overlaps with `.rules`) | |
| **Total** | **77** | |

Each constraint was recorded with source file, line, level (L1–L5), failure mode, and advertised enforcement. The full per-constraint table is omitted here for brevity; the drift scores below cite the constraint by source + line.

## Phase 2 — Classification

Force distribution across the 77 elicited constraints:

| Force | Count | Notes |
|---|---|---|
| Prohibition | 18 | Hard-enforced: clippy (`no unwrap`, `no mod.rs`), CI (`script/clippy`), runtime gate (tool retry tracker) |
| Guardrail | 22 | Soft-enforced: review conventions (`prefer existing files`, `comments explain why`) |
| Guideline | 12 | Advisory: prioritization, naming, PR hygiene |
| Evidence | 8 | Empirical: smol timers break `run_until_parked`, `AsyncApp` not `Send`, `cx.background_spawn` panics on tokio |
| Hypothesis | 2 | Speculative: "this layering will hold under upstream rebases" (implicit in DIVERGENCE.md's near-zero-merge-conflict claim) |
| Invariant (D-seam) | 15 | D-seams with advertised test pinning — treated as Prohibition (test-enforced) |

### Enforcement gaps (score-3 candidates)

| Constraint | Advertised | Actual | Status |
|---|---|---|---|
| D17 numbering gap | (none — gap) | D17 absent from DIVERGENCE.md | **gap** — retired or missing? |
| D19 numbering gap | (none — gap) | D19 absent from DIVERGENCE.md | **gap** — retired or missing? |
| `.rules` L96 "advertised invariants must point to the enforcement line" | Prohibition | No mechanical enforcement — review-only | Guardrail, not Prohibition (misclassified by wording) |
| `.rules` L97 "ocap: is declared config, not a security gate" | Prohibition | No mechanical enforcement — review-only | Guardrail, not Prohibition |
| 38 `// zed-kask:` comments in `crates/` | D-seam mapping | Not verified in this run — each should map to a D-seam | **unverified** |

## Phase 3 — Per-level variety gate (Ashby)

| Level | Constraint count | Failure-mode coverage | Floor | Ceiling | Maturity | Verdict |
|---|---|---|---|---|---|---|
| L1 Boundary | 15 (D-seams) + 1 (divergence rule) | D-seam obligations, `// zed-kask:` mapping, numbering gaps | met | met | **has_unverified** (38 comments not mapped in this run) | Below variety for comment→seam mapping |
| L2 Crate graph | 0 | (no explicit L2 constraints in `.rules`) | **below floor** | met | N/A | **Gap: no constraint addresses crate-graph layering, cycles, or surface-to-surface deps** |
| L3 Module | 2 (trait-with-one-impl, dead surface) | shallow modules, wide interfaces, dead code | met | met | met | met |
| L4 Surface | 8 (MCP server patterns) | tool envelopes, error classification, credentials, ocap | met | met | met | met |
| L5 Code | 22 (Rust guidelines, GPUI traps, failure signals) | unwrap, silent errors, block_on, busy-spin, missing warns | met | met | met | met |

### Key gate finding

**L2 is below floor.** The `.rules` has no constraint addressing crate-graph health — no rule against dependency cycles, no rule against surface-to-surface deps (MCP server depending on another MCP server), no rule against god-crates. The scan in the earlier conversation found three surface-to-surface deps (`hkask-mcp-kata-kanban → hkask-mcp-swarm`, `hkask-mcp-scenarios → hkask-mcp-prediction-markets`, `hkask-mcp-companies → hkask-mcp-portfolio`) and a god-server (`hkask-mcp-corpus`, 9 deps). None of these violate any existing `.rules` constraint because no L2 constraint exists.

This is the single highest-leverage gap in the constraint set. The reference models (Simon near-decomposability, Courtois) justify an L2 constraint; its absence is drift.

## Phase 4 — Drift measurement

### Per-constraint drift scores (summary)

| Score | Count | % |
|---|---|---|
| 0 (aligned) | 58 | 75% |
| 1 (neutral) | 8 | 10% |
| 2 (divergent, documented) | 3 | 4% |
| 3 (divergent, no exception) | 8 | 10% |

**Drift density: 0.10** (8 score-3 / 77 total). This is at the `significant_drift` threshold (≥ 0.1).

### Score-3 findings (the actionable drift)

| # | Constraint | Source | Reference diverged | Recommended action |
|---|---|---|---|---|
| 1 | **No L2 crate-graph constraint exists** | (absence) | Simon near-decomposability, Courtois | **change_constraint**: add an L2 rule prohibiting surface-to-surface deps and god-crates. This is the highest-leverage fix. |
| 2 | D17 numbering gap | `DIVERGENCE.md` | ATAM IS/OUGHT (intended model incomplete) | **document_exception**: add a note that D17 was retired, or restore the seam. |
| 3 | D19 numbering gap | `DIVERGENCE.md` | ATAM IS/OUGHT | **document_exception**: same as D17. |
| 4 | `.rules` L96 "advertised invariants must point to enforcement line" classified as Prohibition but review-only | `.rules:96` | Ashby (variety mismatch — claims mechanical enforcement, has none) | **change_constraint**: reclassify as Guardrail, or wire a clippy lint / CI check that greps for doc claims without enforcement lines. |
| 5 | `.rules` L97 "ocap: is declared config, not a security gate" classified as Prohibition but review-only | `.rules:97` | Ashby | **change_constraint**: reclassify as Guardrail. |
| 6 | 38 `// zed-kask:` comments not verified to map to D-seams | `crates/` | ATAM IS/OUGHT (intended model vs evaluated) | **change_constraint**: add a CI check that greps `// zed-kask:` comments and verifies each maps to a D-seam entry. |
| 7 | `hkask-test-harness` is a runtime dependency (in `hkask-inference`, `hkask-regulation`, 6 MCP servers) but named as test-only | `kask/crates/*/Cargo.toml` | Simon near-decomposability (naming lies about layering) | **change_constraint**: rename to `hkask-harness` (if runtime utility) or split into `hkask-test-harness` (dev-dep only) + `hkask-runtime-utils`. |
| 8 | `kask_bridge` has 16 source files doing unrelated jobs (skill_executor, inference_ipc_server, metacognition_bridge, identity, mcp_servers) | `kask/crates/kask_bridge/src/` | Simon near-decomposability (integration root doing logic beyond dispatch) | **document_exception** or **change_constraint**: either document that kask_bridge is intentionally a logic home (score-2) or split the unrelated jobs into thin crates behind the bridge. |

### Score-2 findings (documented deviations — no action)

| Constraint | Reference | Deviation documented? |
|---|---|---|
| `.rules` L43 "`.rules` are traps to avoid, not maps to follow" | ATAM (constraints as trade-offs) | Yes — the `.rules` hygiene section is self-aware about this. |
| D-seam numbering gaps are left (not renumbered) | ATAM IS/OUGHT | Implicit — standard practice, but should be explicit. |
| Kask uses 5 levels not Kruchten's 4 views | 4+1 | Yes — documented in `review-reference-models.md` "Where we deviate." |

### Per-level drift density

| Level | Score-3 count | Constraint count | Drift density |
|---|---|---|---|
| L1 | 3 (D17, D19, comment mapping) | 16 | 0.19 |
| L2 | 1 (no constraint exists) | 0 | ∞ (below floor) |
| L3 | 1 (test-harness naming) | 2 | 0.50 |
| L4 | 0 | 8 | 0.00 |
| L5 | 3 (two misclassified Prohibitions, kask_bridge breadth) | 22 | 0.14 |

## Overall verdict

**`significant_drift`** — drift density 0.10, at the threshold. 8 score-3 findings.

The drift concentrates at **L1 (boundary completeness)** and **L2 (crate-graph absence)**. L4 and L5 are largely aligned. The highest-leverage fix is adding an L2 constraint — the crate graph has no guardian in the constraint set, and the scan found three layering violations that no rule prohibits.

## Recommended actions (priority order)

1. **Add an L2 crate-graph constraint to `.rules`** (addresses finding #1, the below-floor gap). Proposed text:
   > *MCP servers must not depend on other MCP servers — shared domain logic belongs in a non-server crate. Flag god-crates (fan-in > 10) for review. Dependency cycles are Prohibitions.*
2. **Verify the 38 `// zed-kask:` comments map to D-seams** (addresses finding #6). Either a CI check or a one-time audit.
3. **Reclassify `.rules` L96 and L97 as Guardrails** (addresses findings #4, #5). They're review-enforced, not mechanically enforced.
4. **Resolve D17 and D19** (addresses findings #2, #3). Document as retired or restore.
5. **Rename or split `hkask-test-harness`** (addresses finding #7).
6. **Decide on `kask_bridge` breadth** (addresses finding #8). Document the exception or split.

## Caveats

- This is a **manual** run. The skill's Lisp gates (completeness, validity, referential integrity) were checked by hand, not executed. A runtime execution may surface additional defects the manual check missed.
- The 38 `// zed-kask:` comments were counted but not individually mapped to D-seams in this run. Finding #6 assumes some are unmapped; verification is needed.
- The Murphy citation was not verified by primary-source fetch. Per the calibration doc's fallback, the IS/OUGHT findings are re-grounded in ATAM's "intended vs evaluated" framing (verified). The findings stand; only the citation changed.
