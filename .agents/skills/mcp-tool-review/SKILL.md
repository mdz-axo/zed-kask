---
name: mcp-tool-review
description: Anchor a built-in MCP server's tools to published reference models and audit their functionality and quality — inventory reconciliation, fidelity-labeled checks, measured probes, bounded passes, and a findings artifact whose file:line citations are script-verified and routed to actuators.
---

# mcp-tool-review

## Reference models

The review's own methodology models, each with its fidelity label:

| Model | Label | Consequence |
|---|---|---|
| MCP tool/schema contracts (rmcp + `hkask-mcp-server` wire behavior) | technical | checks verify actual contract mechanics: envelopes, strict schemas, error variants |
| Fagan inspection (Fagan 1976; derived `fagan_inspection`) | functional | detection separated from correction; findings verified in follow-up |
| ISA 500 assertion-orientation (IAASB) | functional | findings are assertions; evidence graded by source reliability |
| Ousterhout deep modules (2018; derived `deep_module`) | functional | design-quality axis: interface minimalism, dead surface |
| Deming PDCA / Toyota kata (derived entries) | functional | the process skeleton below |
| Ashby requisite variety (VSM) | functional | review variety must cover the known failure-mode distribution — the project `.rules` traps are the prior |
| Criterion benchmark discipline | functional | efficiency numbers only from measured runs with the harness named |

Per-tool anchors live in the **anchor registry** — every tool's anchor is a published reference model or a platform-convention class, never a private definition. A labeled-but-unfollowed model is a false provenance claim: never claim fidelity the process does not exercise.

### Anchor registry (state as of 2026-10-02 — re-verify at run start)

Ruled (verify with `onto_anchor` → tier `derived`):

- `scenario planning` — Schwartz, The Art of the Long View (1991) + Chermack (2011); ruling 2026-09-25
- `superforecasting` — Tetlock & Gardner (2015); ruling 2026-09-25
- `Brier score` — Brier (1950); ruling 2026-09-18

Pending operator rulings (batched ask; NEVER assign a private definition while pending — gate the checks that rely on them and mark them `pending-rulings` in the artifact):

- `Bayesian network` — proposed: Pearl, Probabilistic Reasoning in Intelligent Systems (1988)
- `discounted cash flow` — proposed: Damodaran, Investment Valuation (2012); origin Williams (1938)
- `constant-maturity prediction` — proposed: Federal Reserve H.15 CMT methodology + the platform's `cmp-term-structure` reification
- `arbitrage` — proposed: law of one price / no-arbitrage (e.g., Hull, Options, Futures and Other Derivatives)

Fidelity labels: **functional** = check WHAT the model evaluates (purpose, outcomes); **technical** = check HOW the model works (mechanism ported faithfully); **both** = both check families. Labels are operator decisions (ask-first, batched). Recommended starting labels: `both` for the scenario-planning, superforecasting, and Brier groups; `technical` for the Bayesian-network composition group; `functional` for `contract_price_coherence` and `scenario_assess`. When the operator is unavailable, proceed on the stated recommendation and flag every default — never decide silently, and never run checks written for a recommendation the operator has flipped.

Utility tools (`scenario_status` and the `*_status` / `ping` family) have no published reference model; proposed anchor class = platform observability convention (the review checks cross-server consistency, not fidelity to a published model) — operator decision point.

## Initial and target condition

- **Initial:** a server name from the operator's sweep order (scenarios → prediction-markets → companies → swarm → kata-kanban → media → remaining; reorder only with a stated connectedness rationale); the source tree at `kask/mcp-servers/<server>/`; the live surface via `list_mcp_tools`; any saved inventory artifact.
- **Target:** a findings artifact for the server where enumerated tool count == reviewed count (per server and total, `lisp_eval`-reconciled), every finding cites tool + file:line verified by `kask/scripts/check-mcp-review-citations.sh`, efficiency numbers come from measured invocations with the harness named (a blocked axis is recorded as a harness-gap finding, never an asserted number), every tool carries an anchor + fidelity label (or a flagged default), and every finding routes to a named actuator.

## When to Use

- Reviewing a built-in MCP server's tools against published reference models, per the operator's sweep order.
- Re-verifying a saved inventory artifact against the current tree and live surface (drift is a finding, not a rebuild).

## When NOT to Use

- Reviewing skills or templates — `skill-maintenance`; the zed-Kask D-seam surface — `kask-seam-audit` (seam findings this review surfaces route there).
- A one-off code change against its spec — `code-review`.

## Instructions

### Phase A — Direction [P; critic: the operator]

1. Load the operator rulings in force: sweep order, ask-first, divergence goal, key priority (anchor tools to reference models; audit and improve functionality and quality).
2. Batch EVERY unclear anchor, fidelity label, utility class, divergence scope, and mapping question into ONE decision request, presented before the checks that rely on them run. When the operator is unavailable: proceed on the stated recommendation and flag every default in the artifact. Checks depending on already-ruled anchors run first; gated work is marked `pending-rulings` — the run never deadlocks on pending rulings.
3. Verify registry state: `onto_anchor` each ruled term (expect tier `derived`) and each pending term (a coarse tier confirms it is pending).

### Phase B — Inventory [D; oracle: grep walk + `list_mcp_tools` + `lisp_eval`]

4. Walk `kask/mcp-servers/<server>/src/` for `execute_tool(` registrations (grep; record file:line per tool). The pattern must tolerate line-wrapped call layouts — rustfmt wraps long registrations so the tool name sits on its own line (observed 2026-10-02: 4 of 32 prediction-markets registrations are multi-line; a single-line pattern undercounts and produced the skeleton's stale 28). A saved inventory artifact is re-verified against the current tree and live surface, never rebuilt — drift is a finding.
5. Cross-check the live surface (`list_mcp_tools`, grouped by server id). Record ACTUAL server membership — tool names do not respect server boundaries.
6. Reconcile in `lisp_eval`: source count == live count == reviewed count AND every live-registered name present in the source list. A server that fails enumeration is a finding, not a gap.

### Phase C — Metric space [D; oracle: declared before measuring]

7. Declare the efficiency metric space BEFORE measuring anything: axes, measured-vs-judged-vs-blocked, instrument per axis. Measured: call success/failure classification (live probes; harness named), schema strictness (static read), dead surface (scoped walk), D/P character (static). Judged: description honesty (verification channel: file:line trace to the enforcement line). Blocked: billed tokens and latency whenever the harness exposes no usage/timing metadata — record the harness gap as a finding; never assert a number.

### Phase D — Per-tool review passes [mixed; oracle per axis]

One pass per tool per cycle. For each tool:

8. **Syntax** [D]: schema strictness — arbitrary-JSON inputs must be `AnyJsonValue` (bare `serde_json::Value` schemars-renders as `true`, breaking strict providers); unsupported JSON Schema keywords must yield `UnsupportedSchema`, never a pass.
9. **Semantics** [P; verification channel: file:line trace]: description honesty — every advertised invariant points at its enforcement line; per-variant error classification (never blanket `internal`); degradation surfaced (a note/status naming the reason), never empty-equals-success; missing credentials → `permission_denied` naming the env var.
10. **Cybernetics**: every finding routes to an actuator (`curator_directive` `evolve_mcp_tool_schema` for input-schema evolution; kanban / algedonic review for code changes; `skill-maintenance` for process). Ashby check: the review's variety must cover the failure modes that matter — the `.rules` traps are the prior distribution. Sense inputs never `unwrap_or(0)` — a broken sense is a finding.
11. **Loops**: one pass per tool per cycle; cycle budget declared before the run; progress signal emitted per server.
12. **Use patterns** [D]: dead-surface walk — fixed-string grep over `rs`/`md`/`j2`/`yml`, excluding the server crate; STATE THE SCOPE; distinguish "no callers found" (a scope-limited search result) from "no callers" (a claim). Corroborate with curator skill-use records, research-run ledgers, kanban histories.
13. **Observability**: per-tool findings with severity; per-server summary; total-surface manifest; artifacts durable (`report_save`, kanban, or research-run ledger for live probes).
14. **Traceability**: every finding cites tool + file:line + verbatim excerpt; count reconciliation at every seam (verify stage N's output shape is what stage N+1 consumes before running the chain).
15. **Integration** [D]: credential resolution (`resolve_db_passphrase`, env injection, `nudge_mcp_servers`), response envelopes (`unwrap_tool_envelope`), settings defaults location, model-name constants. Integration defects are findings.
16. **Divergence** [D]: name every DIVERGENCE.md seam the review touches; propose removal or minimization where the user-visible purpose no longer needs divergence; prefer `kask/`-side fixes; any unavoidable zed-side edit carries its D-seam entry + test in the same change. The review's own proposals create NO new divergences — a proposal requiring a zed-side edit without a seam entry + test is a defect in the proposal.

### Phase E — Probes [D calls + P classification; verification: static/live reconciliation]

17. Live-probe charter: read-only and pure-computation tools only; tools that durably mutate state (journal/persistence writes) are excluded from probing and reviewed statically — record which and why. Ephemeral in-memory mutation is allowed with probe-prefixed identifiers and disclosed in the artifact. Every probe result is reconciled against the static source read (second channel); disagreement is a finding. A failing probe is classified failure evidence, not an abort; report tool defects via `curator_report_skill_use_issue` and continue.

### Phase F — Findings assembly and verification [P + D gates]

18. Assemble the artifact per the contract — render `mcp-tool-review/findings-artifact` (or read `kask/registry/templates/mcp-tool-review/findings-artifact.j2`). Gated checks appear as `pending-rulings` entries.
19. Run `bash kask/scripts/check-mcp-review-citations.sh <artifact.json>` — every citation must exist (file present, line in range, excerpt within ±3 lines). Fix and re-run at most twice; a third failure stops the run with the failures listed.
20. Run the routing invariant in `lisp_eval`: `(and (= unclassified 0) (= ungated 0) (= bad_region 0) (>= operator_roles 1) (>= other_roles 1))` over the run's step classification.
21. Critic pass [P; decoupled by rubric]: re-read each finding against its citation — would a reviewer checking the cited line see the claimed behavior? Drop or amend what fails; record the pass in the artifact.

### Phase G — Routing and report [D]

22. Route findings to their actuators; file proposals, not commits. Report the verdict, the functional outcome, the findings table, the acceptance-check execution, the batched ask (when pending), and learnings.

## D/P labelling

| Step | Type | Oracle / critique |
|---|---|---|
| A1 Load rulings | D | the recorded rulings and operator grant text |
| A2 Batched ask | P | the operator (asked, never assumed); flagged defaults recorded in the artifact |
| A3 Registry verify | D | `onto_anchor` tier check (`derived` for ruled, coarse confirms pending) |
| B4–B5 Inventory walk + live cross-check | D | grep receipts + `list_mcp_tools` |
| B6 Reconciliation | D | `lisp_eval` count + name-set form |
| C7 Metric-space declaration | D | declared-before-measuring; blocked axes recorded as findings |
| D8 Syntax | D | source read (`AnyJsonValue` vs bare `Value`; unsupported-keyword handling) |
| D9 Semantics (description honesty) | P | file:line trace to the enforcement line + the F21 critic pass |
| D10–D11 Cybernetics / loops | D | actuator routing receipts; declared cycle budget |
| D12 Use patterns | D | scoped fixed-string walk (scope stated in every dead-surface verdict) |
| D13–D14 Observability / traceability | D | artifact fields + the citation script |
| D15 Integration | D | source read (passphrase helper, envelope, settings defaults, model constants) |
| D16 Divergence | D | DIVERGENCE.md seam entries |
| E17 Probes | D calls + P classification | static/live reconciliation (second channel); `curator_report_skill_use_issue` on tool defects |
| F18 Artifact assembly | P (the render itself is D) | the `findings-artifact` contract render |
| F19 Citation check | D | `check-mcp-review-citations.sh` exit code |
| F20 Routing invariant | D | `lisp_eval` over the step classification |
| F21 Critic pass | P | decoupled-by-rubric re-read of each finding against its citation |
| G22 Routing / report | D | actuator receipts (kanban / algedonic / directive) |

## Bounded passes and termination

- One review pass per tool per cycle. Cycle budget declared before the run (default: 1 review cycle + at most 2 fix-verify cycles per server).
- Termination is structural: the inventory is finite (walk count N), passes are bounded (N per cycle), cycles are bounded (declared budget), and every `lisp_eval` call carries a `max_steps` budget. Formal obligation (stated): the review loop is a fold over a finite `List Tool`; structural recursion terminates. `lean_check` on the obligation requires operator approval per its tool contract — run when granted; until then the `lisp_eval` budget is the executed guard.
- Progress signal per server: enumerated / reviewed / findings counts emitted in the artifact.

## Self-reference safety

- The inventory derives from the source tree and registry walk, never from a single live MCP call. Live calls are evidence, reconciled against static schema reads.
- Findings about tools the process itself uses (e.g., `lisp_eval`, `list_mcp_tools`) get decoupled verification: a second channel (source read vs live call) must agree.

## Findings artifact contract

Render `mcp-tool-review/findings-artifact` for the JSON shape. Load-bearing fields: `reconciliation` (counts + instrument + result), `tools[]` (name, file, line, anchor, fidelity_label, dp_character, axes, probe, status), `findings[]` (id, tool, axis, severity, claim, citations[{file, line, excerpt}], evidence, proposal, actuator, status), `gated_pending_rulings[]`, `citation_check`, `ask_first`. Save durably via `report_save` (kind `report`).

## Acceptance checks (executed, not proposed)

1. Enumerated tool count == reviewed count, per server and total — `lisp_eval` reconciliation executed and shown.
2. Every finding cites tool + file:line; `kask/scripts/check-mcp-review-citations.sh` verifies each citation exists — executed.
3. Efficiency numbers come from measured invocations with the harness named — blocked axes recorded as harness-gap findings, never asserted numbers.
4. Every reference model carries a functional/technical/both fidelity label with its consequence; unclear labels were asked (batched) or proceeded with the flagged default.
5. Every step carries a D/P label and a named oracle; every probabilistic step carries a verification channel.
6. Pass structure, budget, and termination criteria stated; the run completes within them.
7. The run produces the server's findings artifact.
8. Mapping ambiguities resolved with the operator or explicitly defaulted and flagged.
9. Zero silently-decided anchors; once confirmed, rulings are recorded in the derived registry (verify: `onto_anchor` returns the derived tier for each ruled term).
10. Findings name their seam; proposals remove or minimize divergence and create none; any zed-side edit carries its D-seam entry + test in the same change.

## Calibration

- Deterministic: count reconciliation, the citation script, and the routing invariant — receipts recorded in the artifact.
- Probabilistic: description-honesty judgments carry the file:line trace channel; the critic pass is recorded.
- Run-level: the pilot's intake prediction is Brier-scored via the goal loop (`kanban_goal_score`).

## Constraints

- Ask-first is batched: ONE decision request per run, never one interruption per tool.
- Never assign a private definition to a pending anchor; never claim fidelity the process does not exercise.
- Never probe a durable-mutation tool; never leave a probe's ephemeral state undisclosed.
- Findings are proposals; code changes go to kanban / algedonic review, not commits.
- No second clone or worktree for any measurement (absolute prohibition) — measurements run in-tree.
- A tool's missing anchor, label, or D/P character is a finding, never a load-time failure.

## Registry Templates

| Template | Purpose |
|---|---|
| `findings-artifact.j2` | The findings artifact contract: renders the JSON skeleton (reconciliation, per-tool records, findings with citations, gated-pending list, citation-check receipt, ask-first record) that the review fills and the citation script verifies. |

Template context variables (from the template's `[inference]` contract): `server`, `run_date`, `tools`, `findings`, `reconciliation`, `gated_pending`, `notes`.

## Regression case

- Reconciliation form over `{src: 19, live: 19, reviewed: 19}` with matching name lists → `true`; a drifted count (`live: 18`) → `false`.
- `check-mcp-review-citations.sh` over an artifact with one good citation and one citing a nonexistent file → exit 1 naming the bad citation.
- Routing invariant over `{unclassified: 0, ungated: 0, bad_region: 0, operator_roles: 2, other_roles: 3}` → `true`; `ungated: 1` → `false`.

All receipts executed live through `lisp_eval` / the script at pilot time.