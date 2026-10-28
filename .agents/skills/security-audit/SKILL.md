---
name: security-audit
description: "Target-scoped deep security audit of zed-kask code — a change, crate, MCP server, or named surface — against the OWASP LLM Top-10 (2025), MITRE ATLAS v5.1 and NIST SSDF SP 800-218A, with deterministic evidence sweeps first, falsifier-backed findings citing file:line with verbatim evidence, defense-layer coverage verdicts, coverage honesty, and a hardening loop that proposes ratchet entries (.rules traps + selftest-pinned check scripts + regression tests) for operator ratification."
---

# Security Audit

Deep security review of a named target — the tier above `code-review`'s inline
security basics and the delegate `code-review` invokes for security-critical
changes. The skill is **read-only**: it finds, adjudicates, and proposes
hardening; fixes flow through separately authorized work.

## Reference models

- **OWASP Top 10 for LLM Applications (2025)** — the risk classes: LLM01:2025
  Prompt Injection, LLM02:2025 Sensitive Information Disclosure, LLM03:2025
  Supply Chain, LLM04:2025 Data and Model Poisoning, LLM05:2025 Improper
  Output Handling, LLM06:2025 Excessive Agency, LLM07:2025 System Prompt
  Leakage, LLM08:2025 Vector and Embedding Weaknesses, LLM09:2025
  Misinformation, LLM10:2025 Unbounded Consumption.
- **MITRE ATLAS v5.1** — adversarial techniques, including AML.T0051 prompt
  injection (direct and **indirect**), AML.T0099 AI agent tool data poisoning,
  AML.T0100 AI agent clickbait, AML.T0101 data destruction via AI agent tool
  invocation.
- **NIST SSDF SP 800-218A** — the secure-development process frame (the PW
  group owns threat modeling and security review; the RV group owns the
  respond-to-vulnerabilities loop this skill's hardening proposals feed).
- **OWASP Code Review Guide v2** — review methodology, inherited from
  `code-review`.
- **Ontology disclosure**: "security review" and "defense in depth" anchor
  coarsely on the 5W1H core rung (operator ruling required to improve them);
  the models above are skill-declared reference models, not ladder-anchored
  terms. **Never cite a CWE identifier** — the local corpus does not ship the
  MITRE CWE enumeration, so any CWE citation is fabricated.

## Priority-surface catalog

The threat model for THIS system (evidence recorded 2026-10-24; the
map-surface phase walks the rows that intersect the target):

| ID | Surface | OWASP LLM 2025 | ATLAS | Current defense state (evidence) |
|----|---------|---------------|-------|----------------------------------|
| S1 | Indirect prompt injection — untrusted content (fetched pages, RSS/synthetic feeds, transcripts, memory recall, tool outputs) flowing into agent context | LLM01:2025 | AML.T0051 (indirect) | No deterministic mediation: tool results stored verbatim (`crates/agent/src/agent.rs:3466`); content scanning deleted as inert (RR-0001/RR-0030) |
| S2 | Credential resolution and keychain boundary | LLM02, LLM06 | — | Live: two-tier resolution with `permission_denied` contract (`kask/crates/hkask-mcp-server/src/server/credentials.rs:76`, `kask/crates/hkask-keystore/src/keychain.rs:425`) |
| S3 | Arbitrary-JSON MCP tool inputs | LLM01, LLM06 | AML.T0051 | Live: `AnyJsonValue` contract, schema validation, closed vocabularies, typed `McpToolError` boundary |
| S4 | Path containment and traversal (file tools, template rendering, server path params) | LLM06 | — | Live: `tool_permissions` canonicalize + symlink-escape detection (`crates/agent/src/tools/tool_permissions.rs`); `render_template` canonicalize+starts_with; symlink alias rejection |
| S5 | Subprocess execution (terminal, OCR binaries, yt-dlp) | LLM06 | AML.T0101 | Partial: MCP child env isolation (`PASSTHROUGH_ENV_VARS`, `kask/crates/hkask-mcp/src/runtime.rs`); fixed binaries for OCR; terminal gated by approval prompts |
| S6 | Supply chain (cargo dependencies, downloaded media) | LLM03 | AML.T0099 | Configured: `cargo-deny`/`cargo-audit` installed; `kask/deny.toml` (advisories, unmaintained bans — kask-workspace scoped, scheduled re-review 2026-11-04); gate red at 2026-10-24 (stale advisory ignore + sources check — remediation follow-up); yt-dlp downloads remote media |
| S7 | Excessive agency / destructive tools (delete, git operations, destructive gallery mode) | LLM06 | AML.T0101 | Partial: approval prompts, sensitive-settings detection, sandbox write grants |
| S8 | Secret exposure via logs and responses | LLM02 | — | Contract-enforced (`log::warn!` rules, no `.ok()?` collapse) — review-level, not CI |
| S9 | System prompt and context leakage to inference providers | LLM07 | — | Provider-side; disclosed data-sharing boundary |
| S10 | Unbounded consumption (inference spend, runaway loops) | LLM10 | — | Partial: Regulation budgets, skill iteration bounds |

## Defense-layer catalog (evidence-evolved)

A layer is **live only with a live enforcement point** — a layer whose
machinery is inert is de-advertised, never reported as present.

Live:

1. **Capability separation** — allowlists (`BuiltinMcpServer`
   credentials/config_env), closed vocabularies, path containment, env
   passthrough isolation.
2. **Runtime monitoring** — Regulation sensor loop (`tool_reliability`,
   `error_kinds`, algedonic alerts).
3. **Provider-side safety** — external by design; disclosed, never claimed as
   ours.

Recorded absences — never report these as present or as missing machinery;
they are de-advertised with reasons:

- **Information-flow control** — deleted as inert (RR-0053; both gate inputs
  were constants). Evolution path if the indirect-injection threat
  materializes: FIDES-style variable-mediated IFC (Costa, Köpf et al.,
  arXiv:2505.23643) — tool results enter variables carrying security labels,
  the conversation history taints only when a variable is inspected, and
  tool-call arguments that must be trusted (recipients, paths, commands) are
  gated on the context label.
- **Consent gating** — removed with the ABW backend (2026-10-22); local
  delegation has no consent gate by design.
- **Deception detection** — no production enforcement point (2026-10-24 tree
  sweep: test fixtures only).
- **Content scanning, spotlighting, canary tokens** — `hkask-guard` deleted as
  inert (RR-0001/RR-0030).

New layers are added as research advances; every addition names its live
enforcement point in the same change.

## Initial and target condition

- **Initial condition**: a named target (change / crate / MCP server /
  surface), the current ratchet state (`.rules` traps, `kask/scripts/check-*.sh`
  gates, pinned tests), and any prior security findings recalled from curator
  memory. Measured by: the sweep ledger the probe phase records.
- **Target condition**: a report where every priority surface intersecting the
  target carries `checked` or `not_checked` with a reason, every finding cites
  file:line with verbatim evidence that survives spot re-verification, every
  defense layer carries a verdict naming an enforcement point or an explicit
  recorded absence, `coverage_honesty.residual_risk` states what the audit did
  not cover, confirmed findings carry ratchet proposals, and the SA-1 gate
  reads `done`. Zero unverified claims.

## D/P labelling

| Step | Type | Oracle / critique |
|------|------|-------------------|
| 1 map-surface | P | the SA-1 gate; the operator |
| 2 probe (sweeps) | D | `grep`/`terminal` exit counts; known-positive fixtures |
| 2 probe (spot re-verification) | D | `read_file`/`grep` against the cited file:line |
| 3 adjudicate (interpretation, severity) | P | falsifiers; constraint force; the spot re-verification record |
| 4 convergence gate | D | `lisp_eval` SA-1 over the finding/surface/layer records |
| 5 report | P | coverage honesty; the operator ratifies ratchet proposals |

## When to Use

- Deep security review of a change, crate, MCP server, or named surface,
  beyond `code-review`'s inline basics.
- As `code-review`'s security delegate for security-critical changes —
  findings flow back to the caller as raw material; this skill never
  re-invokes `code-review`.
- Periodic surface audits, and supply-chain checks (`cargo-deny`,
  `cargo-audit`).

## When NOT to Use

- Change-correctness review with basic security — `code-review` owns that.
- Seam-scoped full-surface audits — `kask-seam-audit` owns that.
- Dynamic red-teaming or prompt-injection attack execution — this skill is
  static exposure analysis only; no attack traffic is generated.
- Fixing code — read-only; hardening proposals flow through separately
  authorized work.

## Instructions

### Step 1 — Map the surface (Plan)

1. Call `curator_memory_recall` with entity `"<target_surface>"` and
   `curator_semantic_search` with query `"security finding <target_surface>"`;
   merge relevant prior findings.
2. Collect the enforced ratchet: the `.rules` traps and
   `kask/scripts/check-*.sh` gates that already apply to the target —
   already-enforced checks are skipped, not re-found.
3. Call `render_template`:
   - template: `security-audit/map-surface`
   - context (every contract field):
     ```
     {
       "task": "<the audit task as requested>",                          // required
       "target_surface": "<crate path / MCP server / diff / surface>",    // required
       "prior_findings": [<recalled finding objects>],                    // optional, default []
       "enforced_ratchet": [<{"kind": "rules"|"check_script"|"test", "entry": "..."}>]  // optional, default []
     }
     ```
4. Follow the rendered prompt and produce the `surface_map` object.

### Step 2 — Probe (Do: the deterministic harness floor, then supplementary sweeps)

1. Run the deterministic probe harness FIRST — the evidence floor, not optional:
   `bash kask/scripts/security-audit-probe-suite.sh --target <target> --surfaces <the intersecting S2,S3,S4,S5,S7,S8 classes>`
   The harness self-validates every probe against planted fixtures (a broken instrument fails the run — never a silent pass) and emits JSONL: a `run` header (version-pinned), one `probe` row per surface class (file:line matches with the production/test-scoped split done mechanically), and a `summary` footer. Every production match must be adjudicated in step 3; a surface class claimed `checked` MUST have validated harness rows — SA-2 enforces this.
2. Call `render_template`:
   - template: `security-audit/probe`
   - context: `{ "task": "<task>", "surface_map": <step 1 output> }`
   The harness output is the primary sweep ledger; agent-chosen sweeps run on top and MUST be prefixed `supplementary:` in their `sweep_command`.
3. Execute any prescribed supplementary sweeps via `terminal`/`grep` with the known-positive rule as before; fold their results into the ledger.
4. For supply-chain surfaces run `cargo deny --config kask/deny.toml check` and `cargo audit` when available; record skipped checks with the named reason, never silently.
5. Spot re-verify **every** recorded finding's `file_line` with `read_file` before adjudication; mark `verified` accordingly.

### Step 3 — Adjudicate (Check: interpretation and severity)

1. Call `render_template`:
   - template: `security-audit/adjudicate`
   - context: `{ "task": "<task>", "surface_map": <step 1 output>, "raw_findings": <step 2 output> }`
2. Follow the rendered prompt: classify each verified finding against the
   OWASP LLM 2025 / ATLAS anchors, attach a falsifier (the probe that would
   disprove it), derive severity from constraint force, and produce the
   `adjudicated` object including per-surface status and defense-layer
   verdicts.

### Step 4 — Convergence gate (Check, D)

Call `lisp_eval` (SA-1):
- form:
  `(begin (define count-uncited (lambda (fs) (if (is_null fs) 0 (+ (if (or (is_null (assoc "file_line" (car fs))) (string= "" (assoc "file_line" (car fs)))) 1 0) (count-uncited (cdr fs)))))) (define count-unverified (lambda (fs) (if (is_null fs) 0 (+ (if (assoc "verified" (car fs)) 0 1) (count-unverified (cdr fs)))))) (define count-open-surface (lambda (ss) (if (is_null ss) 0 (+ (if (or (is_null (assoc "status" (car ss))) (string= "" (assoc "status" (car ss)))) 1 0) (count-open-surface (cdr ss)))))) (define count-open-layer (lambda (ls) (if (is_null ls) 0 (+ (if (or (is_null (assoc "verdict" (car ls))) (string= "" (assoc "verdict" (car ls)))) 1 0) (count-open-layer (cdr ls)))))) (let ((u (count-uncited findings)) (v (count-unverified findings)) (s (count-open-surface surfaces)) (l (count-open-layer layers))) (list u v s l (if (= (+ u (+ v (+ s l))) 0) (quote done) (if (>= iteration 3) (quote stop-and-report) (quote iterate))))))
- env: `{ "findings": <adjudicated.findings>, "surfaces": <adjudicated.surfaces>, "layers": <adjudicated.defense_layer_coverage.layers>, "iteration": <1-based> }`

`done` → step 5. `iterate` → re-enter the step owning the open count (sweeps,
verification, or adjudication); bound: **2 re-loops** (iteration 3 reads
`stop-and-report`). `stop-and-report` → step 5 with open items listed.

Then SA-2 (the harness floor gate, D): call `lisp_eval`:
- form: `(and (string= script_version "1.0") (= unvalidated_probes 0) (= unchecked_surfaces_with_harness_rows 0))`
- env: `{ "script_version": <the harness run row's version>, "unvalidated_probes": <count of harness probe rows with instrument_validated false>, "unchecked_surfaces_with_harness_rows": <count of adjudicated surfaces marked checked, within the harness's S-classes, that have NO validated harness row> }`

SA-2 false → the run is INVALID: re-run the harness for the missing surface classes or correct the adjudication. A superficial or theatrical run cannot pass SA-2 — the evidence floor is machine-stamped, not agent-attested.

### Step 5 — Report and hardening proposals (Act)

1. Call `render_template`:
   - template: `security-audit/report`
   - context: `{ "task": "<task>", "surface_map": <step 1 output>, "adjudicated": <step 3 output> }`
2. Produce the report: verdict driven by blocker presence, findings grouped by
   severity, coverage honesty (`checked` / `not_checked` / `residual_risk` —
   never a bare clean bill), defense-layer coverage verdicts, and one ratchet
   proposal per confirmed finding at severity should-fix or above — each
   proposal names the `.rules` trap, the selftest-pinned check script, and the
   test pin. Proposals are pending operator ratification in the algedonic
   review.
3. If invoked as `code-review`'s delegate, return the findings as raw
   material to the caller.

### Failure surfacing

If any tool call fails, call `curator_report_skill_use_issue` with
`skill_name: "security-audit"`, the failed `tool_name`, the error, and the
classified `failure_origin`. Continue with the best available information —
do not abort; record the degradation in `coverage_honesty`.

## PDCA loop

```
Plan: map-surface — target, assets, entry points, priority surfaces, enforced-ratchet skip list
Do:   probe — deterministic sweeps (known-positive validated) + spot re-verification of every finding
Check: adjudicate — falsifier, constraint-force severity, layer verdicts; SA-1 gate
       (signal: tool-produced open counts; threshold: 0; bound: 2 re-loops)
Act:   report — coverage honesty + ratchet proposals; re-enter the step owning the open count
```

## Registry Templates

| Template | Purpose |
|----------|---------|
| `map-surface.j2` | PLAN — map the audit target to assets, entry points, priority surfaces with OWASP LLM and ATLAS anchors, and the enforced ratchet entries to skip, producing the surface_map object the probe phase walks. |
| `probe.j2` | DO — prescribe deterministic evidence sweeps for each priority surface in the surface_map, requiring known-positive instrument validation before any zero-match sweep result is trusted, and shape the raw_findings ledger the adjudicate phase consumes. |
| `adjudicate.j2` | CHECK — adjudicate raw sweep findings into evidence-backed verdicts carrying a falsifier, constraint-force severity, spot re-verification status, and defense-layer coverage verdicts, leaving count arithmetic to lisp_eval. |
| `report.j2` | ACT — compose the audit report with coverage honesty (checked / not_checked / residual_risk), a verdict driven by blocker presence, and ratchet proposals for operator ratification, never claiming a comprehensive security audit or certification. |

## Regression case

Run SA-1 through `lisp_eval` in both directions: a clean env (all findings
cited and verified, all surfaces statused, all layers verdicted, iteration 1)
→ `done` with zero counts; one uncited finding → `iterate` with `[1 0 0 0]`;
iteration 3 with any open count → `stop-and-report`. Verify each of the four
open-count paths independently (uncited, unverified, open surface, open layer).
SA-2 receipts: a harness run whose version differs from the pin, an
unvalidated probe row, or a checked surface without harness rows → false; a
complete validated set → true. `bash
kask/scripts/security-audit-probe-suite.sh --selftest` must exit 0 (all
probes detect their planted fixtures).

## Constraints

- Read-only: never edit code, `.rules`, or git state; fixes flow through
  separately authorized work.
- Never claim a comprehensive security audit, certification, or absence of
  vulnerabilities — a clean pass is valid but `residual_risk` always states
  what was not covered.
- Never cite a CWE identifier (no MITRE enumeration in the local corpus — any
  CWE citation is fabricated). Classify by OWASP LLM 2025 and ATLAS only.
- Every finding cites file:line with a verbatim evidence snippet and survives
  spot re-verification; uncited or uncorroborated observations are dropped,
  not recorded.
- Every zero-match-capable sweep validates its instrument against a
  known-positive first; a zero from an unvalidated instrument is not evidence.
  The probe harness (`security-audit-probe-suite.sh`, pinned version 1.0)
  IS that validation for its battery: run it for every target audit; a surface
  claimed `checked` without validated harness rows fails SA-2. The agent's
  judgment layers (interpretation, severity, falsifiers) operate ON the harness
  output, not instead of it.
- Defense layers: report live enforcement points or recorded absences — never
  machinery that is not there, and never re-advertise a de-advertised layer
  without naming its new live enforcement point.
- Ratchet proposals are proposals: the operator ratifies; the skill never
  writes `.rules`, check scripts, or tests.
- Registry is authoritative — when this SKILL.md disagrees with the registry
  templates, the registry wins.