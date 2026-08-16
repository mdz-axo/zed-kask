# Constraints Review — Proposed L2 Rule + Existing Set Review (v1)

> **Date:** 2026-08-16
> **Skill:** `constraints-review` v0.34.0
> **Reference:** `kask/docs/architecture/review-reference-models.md` v1
> **Subject:** Proposed L2 crate-graph rule, reviewed against the existing constraint set and the reference models. Plus a review of the existing rule set for additional recommendations.

## The proposed rule (after essentialist reduction)

> *MCP servers must not depend on other MCP servers. Flag god-crates (fan-in > 10) for review.*

### Force classification

| Clause | Force | Enforcement | Maturity |
|---|---|---|---|
| "MCP servers must not depend on other MCP servers" | **Prohibition** | Review-time (no mechanical enforcement yet). Could be wired as a CI script that parses `kask/mcp-servers/*/Cargo.toml` for `hkask-mcp-` deps. | **unenforced** — currently a Hypothesis dressed as a Prohibition. Recommend wiring a CI check. |
| "Flag god-crates (fan-in > 10) for review" | **Guardrail** | Review-time. The flag is advisory ("for review"), not a hard deny. | met (it's explicitly a flag, not a deny). |

### Floor/ceiling/maturity gate (Ashby)

| Check | Verdict | Detail |
|---|---|---|
| **Floor** (does it fill the L2 below-floor gap?) | **met** | The drift baseline found L2 has zero constraints. This rule adds 2, covering surface-to-surface deps and god-crates. The remaining L2 failure modes (missing leaf crate, dead crate) are still uncovered, but the floor is now partially met. |
| **Ceiling** (does it over-constrain?) | **met** | The rule prohibits one specific layering violation and flags another. It doesn't prevent legitimate cross-tier deps (e.g. MCP server → MCP base → domain). The fan-in threshold (10) is a flag, not a deny, so it doesn't block legitimate high-fan-in crates like `hkask-types` (fan-in 27) or `hkask-mcp-server` (fan-in 14). |
| **Maturity** | **partial** | The Prohibition clause is unenforced. Per the project rule ("advertised invariants must point to the enforcement line"), this should either be wired as a CI check or reclassified as a Guardrail until wired. |

### Drift score (against reference models)

| Reference | Score | Rationale |
|---|---|---|
| Simon near-decomposability | **0** (aligned) | The rule directly enforces near-decomposability at the MCP-server tier: servers are surfaces, not domain layers; surface-to-surface deps violate the weak-inter-tier-links property. |
| Courtois | **0** (aligned) | The rule enforces the formal near-decomposability condition for the crate graph. |
| Ashby requisite variety | **0** (aligned) | The rule adds variety to the L2 constraint set (which was at zero). The two clauses cover two distinct failure modes. |
| ATAM (quality attributes) | **0** (aligned) | The rule maps to the modifiability + maintainability quality attributes at L2. |
| 4+1 (multiple views) | **0** (aligned) | The rule operates at L2 (development view), not collapsing levels. |
| ATAM (intended vs evaluated) | **0** (aligned) | The rule has a clear intended model (servers are leaves) to compare the evaluated graph against. |

**Drift score: 0.** The proposed rule is fully aligned with the reference models.

### Constraints-review verdict on the proposed rule

**PASS.** The rule fills a below-floor gap, doesn't over-constrain, and aligns with all six reference models. One caveat: the Prohibition clause is currently unenforced — it should be wired as a CI check or reclassified as a Guardrail until wired.

### Recommended action

1. **Add the rule to `.rules`** (in the "zed-kask integration traps" section, as an L2 constraint).
2. **Wire a CI check** that greps `kask/mcp-servers/*/Cargo.toml` for `hkask-mcp-` workspace deps and fails if any are found (excluding `hkask-mcp-server` the base). This moves the Prohibition from unenforced to enforced.
3. The god-crate flag (fan-in > 10) stays as a Guardrail — review-time, not mechanically denied.

## Existing rule set — additional recommendations

Running the constraints-review logic across the full existing set (from the drift baseline), the high-value recommendations to adopt:

### 1. Reclassify `.rules` L96 and L97 as Guardrails (not Prohibitions)

**Current (L96):** "Advertised invariants (doc comments claiming security gates, audit surfaces, migrations) must point to the enforcement line. If no enforcement point exists, the doc must say 'not yet enforced.'"

**Current (L97):** "Manifest `ocap:` is declared config, not a security gate. The real gates are `McpRuntime::invoke` (token + gas) and per-agent `mcp_tools` allowlist. Token expiry is NOT enforced. Don't re-add `ocap:` blocks or `OcapConfig` without wiring into the runtime gate."

**Issue:** Both are worded as Prohibitions ("must", "don't") but are review-enforced, not mechanically enforced. Per Ashby, a Prohibition with no mechanical enforcement is a Hypothesis dressed as a Prohibition — it claims variety it doesn't have.

**Recommendation:** Reword to make the force explicit. Either wire mechanical enforcement (a clippy lint or CI check) or reclassify as Guardrails. The reclassification is the lower-effort fix; the wiring is the higher-value fix.

### 2. Resolve D17 and D19 numbering gaps

**Issue:** DIVERGENCE.md has D1–D28 but D17 and D19 are absent. The drift baseline flagged these as score-3 (ATAM IS/OUGHT: intended model incomplete).

**Recommendation:** Add a note to DIVERGENCE.md stating whether D17 and D19 were retired or are missing. If retired, a one-line note ("D17 retired — folded into D5") closes the gap. If missing, restore the seam entries.

### 3. Verify `// zed-kask:` comments map to D-seams

**Issue:** 38 `// zed-kask:` comments in `crates/` — not verified to map to D-seam entries. The drift baseline flagged this as score-3.

**Recommendation:** Add a CI check (or run a one-time audit) that greps `// zed-kask:` comments and verifies each references a D-seam in DIVERGENCE.md. This is the L1 equivalent of the L2 CI check proposed above.

### 4. Rename or split `hkask-test-harness`

**Issue:** `hkask-test-harness` is a runtime dependency in 13 places but named as test-only. The drift baseline flagged this as score-3 (Simon: naming lies about layering).

**Recommendation:** Either rename to `hkask-harness` (if it's runtime utility) or split into `hkask-test-harness` (dev-dep only) + `hkask-runtime-utils` (runtime). The rename is lower-effort; the split is cleaner.

### 5. Decide on `kask_bridge` breadth

**Issue:** `kask_bridge` has 16 source files doing unrelated jobs. The drift baseline flagged this as score-3 (Simon: integration root doing logic beyond dispatch).

**Recommendation:** Either document the exception (kask_bridge is intentionally a logic home — add to the calibration doc's "Where we deviate") or split the unrelated jobs into thin crates behind the bridge. This is the lowest-priority recommendation — the bridge earns its keep; the question is whether it should be thinner.

## Priority order for adoption

1. **Add the L2 rule to `.rules`** (highest leverage — fills the below-floor gap).
2. **Wire the L2 CI check** (moves the Prohibition from unenforced to enforced).
3. **Reclassify L96/L97 as Guardrails** (honest force classification).
4. **Resolve D17/D19** (closes the L1 gap).
5. **Wire the L1 comment→seam CI check** (closes the other L1 gap).
6. **Rename/split `hkask-test-harness`** (naming honesty).
7. **Decide on `kask_bridge`** (lowest priority).

## What I'm adopting now

Per the instruction to adopt the high-value recommendations: I'll add the L2 rule to `.rules` (item 1) and reclassify L96/L97 (item 3). The CI checks (items 2, 5) and the D17/D19 resolution (item 4) require code changes or DIVERGENCE.md edits that should go through a PR; I'll note them as proposed in the PR description per the rules-hygiene rule ("Don't edit `.rules` inline during feature work — propose additions in PR descriptions"). The `hkask-test-harness` rename (item 6) and `kask_bridge` decision (item 7) are larger refactors that warrant their own PRs.

Actually — re-reading the rules-hygiene rule: "Don't edit `.rules` inline during feature work — propose additions in PR descriptions." This means I should **not** edit `.rules` directly here. I'll propose the additions in this report and leave the `.rules` edit for the PR.
