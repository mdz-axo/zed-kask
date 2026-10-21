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

### Anchor registry (state as of 2026-10-14 — re-verify at run start)

Ruled (verify with `onto_anchor` → tier `derived`):

- `scenario planning` — Schwartz, The Art of the Long View (1991) + Chermack (2011); ruling 2026-09-25
- `superforecasting` — Tetlock & Gardner (2015); ruling 2026-09-25
- `Brier score` — Brier (1950); ruling 2026-09-18
- `Bayesian network` — Pearl, Probabilistic Reasoning in Intelligent Systems (1988); ruling 2026-10-02
- `discounted cash flow` — Damodaran, Investment Valuation (2012); origin Williams (1938); ruling 2026-10-02
- `arbitrage` — Hull, Options, Futures and Other Derivatives; law of one price / no-arbitrage; ruling 2026-10-02

Unruled anchors discovered during a server sweep must be surfaced in a batched ask (NEVER assign a private definition while pending — gate the checks that rely on them and mark them `pending-rulings` in the artifact).

Fidelity labels: **functional** = check WHAT the model evaluates (purpose, outcomes); **technical** = check HOW the model works (mechanism ported faithfully); **both** = both check families. Labels are operator decisions (ask-first, batched). Recommended starting labels: `both` for the scenario-planning, superforecasting, and Brier groups; `technical` for the Bayesian-network composition group; `functional` for `scenario_assess`. When the operator is unavailable, proceed on the stated recommendation and flag every default — never decide silently, and never run checks written for a recommendation the operator has flipped.

Utility tools (`scenario_status` and the `*_status` / `ping` family) have no published reference model; proposed anchor class = platform observability convention (the review checks cross-server consistency, not fidelity to a published model) — operator decision point.

## Initial and target condition

- **Initial:** a server name from the operator's sweep order (scenarios → prediction-markets → swarm → kata-kanban → media → remaining; reorder only with a stated connectedness rationale); the source tree at `kask/mcp-servers/<server>/`; the live surface via `list_mcp_tools`; any saved inventory artifact.
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

4. Resolve the server through the canonical build list and runtime registry, then derive membership from `#[tool]` declarations (including wire-name overrides), generated router groups and the selected `ServerHandler`/`tool_handler` router. Trace conditional composition and record the build/configuration used, file:line and source hashes. `execute_tool` wraps execution/response behavior; neither its string arguments nor a bare declaration establish registration. Run each server's existing `tool_names_match_live_router` test through `cargo-test-nonzero.sh` and compare the extractor's exact names against that tested set. A synthetic demonstration of rmcp routing does not test our extractor; use the real server pins rather than maintaining a second fixture server. Generated names and lexical scans remain candidates until reconciled to the selected router; compile-only conditional coverage is limited to the tested build/configuration. Re-verify saved inventories, retain their history and report drift rather than overwriting it.
5. Cross-check the live surface (`list_mcp_tools`, grouped by server id). Record ACTUAL server membership — tool names do not respect server boundaries.
6. Reconcile raw `source_names`, `live_names` and `reviewed_names` with **MTR-1** below; never supply model-counted cardinalities or a claimed missing-name count. Extract names deterministically from the source/live inventories and the exact artifact tool rows (for JSON, `jq -c '[.tools[].name]'`), not by retyping a projection. Preserve server membership; use consistently server-qualified identities for a multi-server run. A same-count rename or duplicate cannot pass. `source_live_match` establishes only inventory agreement; `review_complete` additionally requires exact review-name coverage. A bounded family may have matching inventories yet `review_incomplete`; that is unfinished server coverage, not a tool defect. Source/deployed drift is reported with both identities/builds, not silently fixed by treating a compiled router as the running server.

MTR-1 env (JSON-encode this object for `lisp_eval.env`; substitute actual name arrays):
```json
{"source_names":["server/tool_a"],"live_names":["server/tool_a"],"reviewed_names":["server/tool_a"]}
```
```lisp
(begin
  (define names-ok (lambda (xs seen)
    (if (is_null xs) true
      (and (stringp (car xs)) (> (length (car xs)) 0) (not (member (car xs) seen))
        (names-ok (cdr xs) (cons (car xs) seen))))))
  (define difference (lambda (xs ys)
    (if (is_null xs) (list)
      (if (member (car xs) ys) (difference (cdr xs) ys)
        (cons (car xs) (difference (cdr xs) ys))))))
  (define mtr-reconcile (lambda ()
    (if (not (and (listp source_names) (listp live_names) (listp reviewed_names)
      (> (length source_names) 0) (names-ok source_names (list))
      (names-ok live_names (list)) (names-ok reviewed_names (list))))
      (list (list "input_valid" false) (list "source_live_match" false)
        (list "review_complete" false) (list "result" "invalid_names"))
      (let ((source_only (difference source_names live_names))
            (live_only (difference live_names source_names))
            (unreviewed (difference source_names reviewed_names))
            (unknown (difference reviewed_names source_names)))
        (let ((inventory_ok (and (is_null source_only) (is_null live_only))))
          (let ((complete (and inventory_ok (is_null unreviewed) (is_null unknown))))
            (list (list "input_valid" true) (list "source_count" (length source_names))
              (list "live_count" (length live_names)) (list "reviewed_count" (length reviewed_names))
              (list "source_live_match" inventory_ok) (list "review_complete" complete)
              (list "source_only_names" source_only) (list "live_only_names" live_only)
              (list "not_reviewed_names" unreviewed) (list "unknown_reviewed_names" unknown)
              (list "result" (cond ((not inventory_ok) "source_live_drift")
                (complete "pass") (true "review_incomplete"))))))))))
  (mtr-reconcile))
```
An invalid result or a missing-binding/type/budget error blocks a pass. Name-set agreement does not prove semantic/reference review occurred. Append instrument/build provenance without changing the computed object; the findings template serializes it directly.

### Phase C — Metric space [D; oracle: declared before measuring]

7. Declare the efficiency metric space BEFORE measuring anything: axes, measured-vs-judged-vs-blocked, instrument per axis. Measured: call success/failure classification (live probes; harness named), schema strictness (static read), dead surface (scoped walk), D/P character (static). Judged: description honesty (verification channel: file:line trace to the enforcement line). Blocked: billed tokens and latency whenever the harness exposes no usage/timing metadata — record the harness gap as a finding; never assert a number.

### Phase D — Per-tool review passes [mixed; oracle per axis]

One pass per tool per cycle. For each tool:

8. **Syntax** [D]: schema strictness — arbitrary-JSON inputs must be `AnyJsonValue` (bare `serde_json::Value` schemars-renders as `true`, breaking strict providers); unsupported JSON Schema keywords must yield `UnsupportedSchema`, never a pass.
9. **Semantics** [P; verification channel: file:line trace]: description honesty — every advertised invariant points at its enforcement line; per-variant error classification (never blanket `internal`) under the canonical contract on the `McpToolError` constructors (operator ruling 2026-10-05): missing caller-named resources → `not_found` naming the thing, malformed input against a closed set → `invalid_argument` naming the valid values, absent state → `failed_precondition` naming the remedy, missing credentials → `permission_denied` naming the env var, server faults → `internal`; degradation surfaced (a note/status naming the reason), never empty-equals-success.
10. **Cybernetics**: every finding routes to an actuator (`curator_directive` `evolve_mcp_tool_schema` for input-schema evolution; kanban / algedonic review for code changes; `skill-maintenance` for process). Ashby check: the review's variety must cover the failure modes that matter — the `.rules` traps are the prior distribution. Sense inputs never `unwrap_or(0)` — a broken sense is a finding.
11. **Loops**: one pass per tool per cycle; cycle budget declared before the run; progress signal emitted per server.
12. **Use patterns** [D]: dead-surface walk — fixed-string grep over `rs`/`md`/`j2`/`yml`, excluding the server crate; STATE THE SCOPE; distinguish "no callers found" (a scope-limited search result) from "no callers" (a claim). Corroborate with curator skill-use records, research-run ledgers, kanban histories.
13. **Observability**: per-tool findings with severity; per-server summary; total-surface manifest; artifacts durable (kanban, research-run ledger, or `~/Documents/zk-data/skills/mcp-tool-review/{YYYY-MM-DD}-{server}/` via terminal). Retain historical report paths as provenance, not as the destination for new runs.
14. **Traceability**: every finding cites tool + file:line + verbatim excerpt; count reconciliation at every seam (verify stage N's output shape is what stage N+1 consumes before running the chain).
15. **Integration** [D]: credential resolution (`resolve_db_passphrase`, env injection, `nudge_mcp_servers`), response envelopes (`unwrap_tool_envelope`), settings defaults location, model-name constants. Integration defects are findings.
16. **Divergence** [D]: name every DIVERGENCE.md seam the review touches; propose removal or minimization where the user-visible purpose no longer needs divergence; prefer `kask/`-side fixes; any unavoidable zed-side edit carries its D-seam entry + test in the same change. The review's own proposals create NO new divergences — a proposal requiring a zed-side edit without a seam entry + test is a defect in the proposal.

### Phase E — Probes [D calls + P classification; verification: static/live reconciliation]

17. Live-probe charter: read-only and pure-computation tools only; tools that durably mutate state (journal/persistence writes) are excluded from probing and reviewed statically — record which and why. Ephemeral in-memory mutation is allowed with probe-prefixed identifiers and disclosed in the artifact. Every probe result is reconciled against the static source read (second channel); disagreement is a finding. A failing probe is classified failure evidence, not an abort; report tool defects via `curator_report_skill_use_issue` and continue.

### Phase F — Findings assembly and verification [P + D gates]

18. Assemble the completed semantic records, then render `mcp-tool-review/findings-artifact` with the full context below. The template is a deterministic JSON formatter: save/parse its exact output, never send it through a model to reconstruct counts, lists or a wrapper object. Pass MTR-1's computed reconciliation unchanged. Gated checks remain `pending-rulings`; use explicit not-run/blocked evidence metadata where checks have not run, not fabricated success. After the citation check, re-render with its actual receipt and retain the prior artifact as history.
19. Run `bash kask/scripts/check-mcp-review-citations.sh <artifact.json>` — every citation must exist (file present, line in range, excerpt within ±3 lines). Fix and re-run at most twice; a third failure stops the run with the failures listed.
20. Run the routing invariant in `lisp_eval`: `(and (= unclassified 0) (= ungated 0) (= bad_region 0) (>= operator_roles 1) (>= other_roles 1))` over the run's step classification, and verify citation-check exit and actuator-routing completeness via `MTR-2`: `(and (= citation_script_exit 0) (= unrouted_findings 0) (= unanchored_tools 0))`.
21. Critic pass [P; decoupled by rubric]: re-read each finding against its citation — would a reviewer checking the cited line see the claimed behavior? Drop or amend what fails; record the pass in the artifact.

### Phase G — Fix, pin, and route [D + P]

22. Close the loop on every finding: an in-scope code defect is FIXED in the same pass — the change, its pinning test, and the touched crates' suites green — before the run reports. A finding left unfixed is an unfinished review. Only findings that change a tool's functional contract (behavior a consumer depends on) go to the operator as decisions, and platform-level redesigns go to kanban/algedonic with the evidence.
23. Verify live where possible: after a fix that changes wire behavior, re-probe the live tool (server rebuild+restart when the running binary predates the fix) and record the before/after. Where live verification is gated on a restart, name the exact remaining step and mark the receipt pending-restart — never claim it.
24. Report the verdict, the functional outcome, the fixes landed with their test receipts, the acceptance-check execution, the batched ask (when pending), and learnings.

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
| F18 Artifact assembly | D | direct JSON serialization of supplied records and MTR-1 output; semantic records were judged in D9/F21 |
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

Render `mcp-tool-review/findings-artifact` for the JSON shape. Load-bearing fields: `reconciliation` (counts + instrument + result), `tools[]` (name, file, line, anchor, fidelity_label, dp_character, axes, probe, status), `findings[]` (id, tool, axis, severity, claim, citations[{file, line, excerpt}], evidence, proposal, actuator, status), `gated_pending_rulings[]`, `citation_check`, `ask_first`. Save new runs under `~/Documents/zk-data/skills/mcp-tool-review/{YYYY-MM-DD}-{server}/` via terminal — for payloads exceeding comfortable single-call size (large slices, 60+ tools), write in chunks (each chunk independently valid JSON, merged with jq, validated, then the citation script run): same artifact, same checks, no truncation risk.

## Acceptance checks (executed, not proposed)

1. MTR-1 returns `review_complete: true` on raw exact-name sets, per server and total — executed and shown. Partial cohorts retain the full parent denominator and an explicit incomplete state, not a full-server pass.
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
- Run-level: the pilot records evidence-grounded worker progress; native human closure and optional prospective forecast resolution remain separate.

## Constraints

- Ask-first is batched: ONE decision request per run, never one interruption per tool.
- Never assign a private definition to a pending anchor; never claim fidelity the process does not exercise.
- Never probe a durable-mutation tool; never leave a probe's ephemeral state undisclosed.
- Findings are fixed in-tree with pinning tests in the same pass (uncommitted — the shared-tree commit discipline stays with the operator); functional-contract changes and platform redesigns go to the operator / kanban, never implemented unilaterally. Never commit unless the operator explicitly asks.
- No second clone or worktree for any measurement (absolute prohibition) — measurements run in-tree.
- A tool's missing anchor, label, or D/P character is a finding, never a load-time failure.

## Registry Templates

| Template | Purpose |
|---|---|
| `findings-artifact.j2` | The findings artifact contract: renders the JSON skeleton (reconciliation, per-tool records, findings with citations, gated-pending list, citation-check receipt, ask-first record) that the review fills and the citation script verifies. |

Copyable `mcp-tool-review/findings-artifact` context (JSON-encode as the outer context string; replace these initial/not-run values with actual records and receipts):
```json
{
  "server":"<server id>", "run_date":"<YYYY-MM-DD>",
  "tools":[], "findings":[], "reconciliation":{}, "gated_pending":[], "notes":[],
  "efficiency_metric_space":{"harness":"<named instrument>","axes":[]},
  "anchor_registry":{"ruled":[],"pending":[]},
  "citation_check":{"script":"kask/scripts/check-mcp-review-citations.sh","result":"not_run","checked":null},
  "ask_first":{"batched_ask_presented":false,"defaults_applied":[]}
}
```
The generator does not decide fidelity labels, fabricate receipts or grade missing records. Empty arrays/objects above are intake shapes, not passing evidence. A final artifact needs actual tool dispositions and the executed MTR-1 result.

## Regression case

- Run `bash kask/scripts/cargo-test-nonzero.sh -p hkask-lisp --bin check-skill-lisp-forms mcp_inventory_reconciliation_uses_exact_raw_names`: the authored MTR-1 must pass reordered equal sets, reject same-count renames/duplicates/invalid names, and distinguish partial review from source/live drift. Supplied scalar counts cannot override its raw-list result.
- Run each real server's name pin and reconcile its exact set to the extracted inventory. Registry rendering must preserve supplied records, including incomplete/blocked metadata; structural checks do not establish semantic effectiveness.
- `check-mcp-review-citations.sh` over an artifact with one good citation and one citing a nonexistent file → exit 1 naming the bad citation.
- Routing invariant over `{unclassified: 0, ungated: 0, bad_region: 0, operator_roles: 2, other_roles: 3}` → `true`; `ungated: 1` → `false`.

Historical citation/routing pilot receipts remain historical. Current discovery/raw-name/formatter regressions are the mandatory checks above; passing them does not establish the semantic quality of every tool review.