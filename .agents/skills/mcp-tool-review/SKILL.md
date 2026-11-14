---
name: mcp-tool-review
description: Anchor a built-in MCP server's tools to published reference models and audit their functionality and quality — inventory reconciliation, fidelity-labeled checks, measured probes, bounded passes, and a findings artifact whose file:line citations are script-verified and routed to actuators.
audience: platform
---

# mcp-tool-review

## Reference models

The review's own methodology models, each with its fidelity label:

| Model | Label | Consequence |
|---|---|---|
| MCP tool/schema contracts (rmcp + `hkask-mcp-server` wire behavior) | technical | checks verify actual contract mechanics: envelopes, strict schemas, error variants |
| Fagan inspection (Fagan 1976; derived `fagan_inspection`) | functional | findings recorded as assertions and re-read by the decoupled critic pass (F21); correction lands in the same pass (step 22) — the process does not separate detection from correction in time, and the row says so (the curator run's fixes landed before findings.json existed) |
| ISA 500 assertion-orientation (IAASB) | functional | findings are assertions; evidence graded by source reliability |
| Ousterhout deep modules (2018; derived `deep_module`) | functional | design-quality axis: interface minimalism, dead surface |
| Deming PDCA / Toyota kata (derived entries) | functional | the process skeleton below |
| Ashby requisite variety (VSM) | functional | review variety must cover the known failure-mode distribution — the project `.rules` traps are the prior |
| Criterion benchmark discipline | functional | efficiency numbers only from measured runs with the harness named |

Per-tool anchors live in the **anchor registry** — every tool's anchor is a published reference model or a platform-convention class, never a private definition. A labeled-but-unfollowed model is a false provenance claim: never claim fidelity the process does not exercise.

### Anchor registry (state as of 2026-10-14 — re-verify at run start)

Ruled (verify with `onto_anchor` → tier `derived`, or a `domain_supplement` primary with the derived entry in `alternatives` — see step 3):

- `scenario planning` — Schwartz, The Art of the Long View (1991) + Chermack (2011); ruling 2026-09-25
- `superforecasting` — Tetlock & Gardner (2015); ruling 2026-09-25
- `Brier score` — Brier (1950); ruling 2026-09-18
- `Bayesian network` — Pearl, Probabilistic Reasoning in Intelligent Systems (1988); ruling 2026-10-02 — resolves at tier `domain_supplement` (`cso:bayesian_network`, CSO 3.5) with the derived Pearl (1988) entry in `alternatives` (state as of 2026-11-01)
- `discounted cash flow` — Damodaran, Investment Valuation (2012); origin Williams (1938); ruling 2026-10-02
- `arbitrage` — Hull, Options, Futures and Other Derivatives; law of one price / no-arbitrage; ruling 2026-10-02

Unruled anchors discovered during a server sweep must be surfaced in a batched ask (NEVER assign a private definition while pending — gate the checks that rely on them and mark them `pending-rulings` in the artifact).

Fidelity labels: **functional** = check WHAT the model evaluates (purpose, outcomes); **technical** = check HOW the model works (mechanism ported faithfully); **both** = both check families. Labels are operator decisions (ask-first, batched). Recommended starting labels: `both` for the scenario-planning, superforecasting, and Brier groups; `technical` for the Bayesian-network composition group; `functional` for `scenario_assess`. When the operator is unavailable, proceed on the stated recommendation and flag every default — never decide silently, and never run checks written for a recommendation the operator has flipped.

Utility tools (`scenario_status` and the `*_status` / `ping` family) have no published reference model; proposed anchor class = platform observability convention (the review checks cross-server consistency, not fidelity to a published model) — operator decision point.

## Initial and target condition

- **Initial:** a server name from the operator's sweep order (scenarios → prediction-markets → kata-kanban → media → remaining; reorder only with a stated connectedness rationale); the source tree at `kask/mcp-servers/<server>/`; the live surface via `list_mcp_tools`; any saved inventory artifact.
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
2. Batch EVERY unclear anchor, fidelity label, utility class, divergence scope, and mapping question into ONE decision request, presented before the checks that rely on them run. A2 means shown: the ask is in a message to the operator before any check that relies on it — a file is a record of the ask, not the ask (the experimentation run wrote the batched ask to a file and continued on its defaults; the operator saw it two hours later). When the operator is unavailable: proceed on the stated recommendation and flag every default in the artifact. Checks depending on already-ruled anchors run first; gated work is marked `pending-rulings` — the run never deadlocks on pending rulings.
3. Verify registry state: `onto_anchor` each ruled term and each pending term. Three outcomes: tier `derived` — the ruling is confirmed; a coarse (core-rung) tier — the term is pending; a `domain_supplement` primary with the derived entry in `alternatives` — ruled-confirmed, not pending (the `Bayesian network` row is the current instance). State as of 2026-11-01.

### Phase B — Inventory [D; oracle: grep walk + `list_mcp_tools` + `lisp_eval`]

4. Resolve the server through the canonical build list and runtime registry, then derive membership from `#[tool]` declarations (including wire-name overrides), generated router groups and the selected `ServerHandler`/`tool_handler` router. Trace conditional composition and record the build/configuration used, file:line and source hashes. `execute_tool` wraps execution/response behavior; neither its string arguments nor a bare declaration establish registration. Run each server's existing `tool_names_match_live_router` test through `cargo-test-nonzero.sh` and compare the extractor's exact names against that tested set. A synthetic demonstration of rmcp routing does not test our extractor; use the real server pins rather than maintaining a second fixture server. Generated names and lexical scans remain candidates until reconciled to the selected router; compile-only conditional coverage is limited to the tested build/configuration. Re-verify saved inventories, retain their history and report drift rather than overwriting it.
5. Cross-check the live surface (`list_mcp_tools`, grouped by server id). Record ACTUAL server membership — tool names do not respect server boundaries.
6. Reconcile raw `source_names`, `live_names` and `reviewed_names` with **MTR-1** below; never supply model-counted cardinalities or a claimed missing-name count. Extract names deterministically from the source/live inventories and the exact artifact tool rows (for JSON, `jq -c '[.tools[].name]'`), not by retyping a projection. A declared cross-cutting (axis-first) pass — one or more axes checked across every tool in place of the per-tool pass — is allowed when declared as such in the run: its tool records are marked `status: partial` (the corpus session-1 shape: `"partial: cross-cutting axes only; …"`) and excluded from `reviewed_names` by a jq filter on status (`jq -c '[.tools[] | select(((.status // "") | test("^partial")) | not) | .name]'`), so MTR-1 reports `review_incomplete` honestly for the pass instead of counting partial records as full passes. Preserve server membership; use consistently server-qualified identities for a multi-server run. A same-count rename or duplicate cannot pass. `source_live_match` establishes only inventory agreement; `review_complete` additionally requires exact review-name coverage. A bounded family may have matching inventories yet `review_incomplete`; that is unfinished server coverage, not a tool defect. Source/deployed drift is reported with both identities/builds, not silently fixed by treating a compiled router as the running server.

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

7. Declare the efficiency metric space BEFORE measuring anything: axes, measured-vs-judged-vs-blocked, instrument per axis. Measured: call success/failure classification (live probes; harness named), schema strictness (static read), dead surface (scoped walk), D/P character (static), and what a model receives — the editor forwards a description only to its first sentence within 200 bytes (crates/agent/src/thread.rs `build_request_tools`) and the schema whole; measure description reach with `instruments/measure-surface.sh` and count a promise that sits past the forwarded cut as NOT advertised. Judged: description honesty (verification channel: file:line trace to the enforcement line). Blocked: billed tokens and latency whenever the harness exposes no usage/timing metadata — record the harness gap as a finding; never assert a number. A measurement axis found mid-run has a named amendment path — the choice is never between skipping the axis and measuring it undeclared: a dated amendment in the run directory records the file and its timestamp, which samples precede it (marked exploratory), the axis, and predictions with falsifiers declared before the next evidence (the curator run's `efficiency.amendment-*.json` shape: written_at, why_an_amendment, status_of_existing_samples, predictions_declared_before_further_evidence, if_P1_is_false).

### Phase D — Per-tool review passes [mixed; oracle per axis]

One pass per tool per cycle. For each tool:

8. **Syntax** [D]: schema strictness — arbitrary-JSON inputs must be `AnyJsonValue` (bare `serde_json::Value` schemars-renders as `true`, breaking strict providers); unsupported JSON Schema keywords must yield `UnsupportedSchema`, never a pass.
9. **Semantics** [P; verification channel: file:line trace]: description honesty — every advertised invariant points at its enforcement line; per-variant error classification (never blanket `internal`) under the canonical contract on the `McpToolError` constructors (operator ruling 2026-10-05): missing caller-named resources → `not_found` naming the thing, malformed input against a closed set → `invalid_argument` naming the valid values, absent state → `failed_precondition` naming the remedy, missing credentials → `permission_denied` naming the env var, server faults → `internal`; degradation surfaced (a note/status naming the reason), never empty-equals-success. The FORWARDED sentence, not the description, is what is judged for honesty: run `instruments/measure-surface.sh` on the tools/list capture before and after any description edit — a review that corrects a description for honesty lengthens it, and the correction can silently remove the evidence requirement, a pooling rule or a destructive-tool warning from what a model receives (156/158/184 forwarded characters before the curator run's description edits, 45/64/41 after); a destructive tool's forwarded sentence says it destroys. When a promised invariant is check-then-write across statements (a race — e.g. "disjoint runs", "the seal's baseline evidence"), write the concurrent test FIRST and record its failure rate: both invariants here passed every sequential probe and failed under concurrency — 6 of 40 and 253 of 300 runs on a file-backed pool; the number is the finding's evidence, and a race promised in a description gets its concurrent test before the fix lands. A citation of an author, paper or numbered principle in a description or response is a reference-model claim: editing or removing a source citation is an anchor-registry action (the run's Axis C), gated on the term's ruling — when the term it depends on is pending, record the observation and leave the text (SC-20: commit 3b155590a7 rewrote or removed three source citations on the reviewer's own reading, each hanging on a term with no operator ruling; reverted).
10. **Cybernetics**: every finding routes to an actuator (`curator_directive` `evolve_mcp_tool_schema` for input-schema evolution; kanban / algedonic review for code changes; `skill-maintenance` for process). Ashby check: the review's variety must cover the failure modes that matter — the `.rules` traps are the prior distribution. Sense inputs never `unwrap_or(0)` — a broken sense is a finding.
11. **Loops**: one pass per tool per cycle; cycle budget declared before the run; progress signal emitted per server.
12. **Use patterns** [D]: dead-surface walk — fixed-string grep over `rs`/`md`/`j2`/`yml`, excluding the server crate; STATE THE SCOPE; distinguish "no callers found" (a scope-limited search result) from "no callers" (a claim). Corroborate with curator skill-use records, research-run ledgers, kanban histories. After any request-type change, read every calling skill and template END TO END and compare each call's argument names and closed-set values against tools/list on the build — a closed-schema fix that makes tools refuse unnamed arguments turns any stale argument name in a skill into a live failure (SC-08), and a search for the removed spellings could not have shown the absence the end-to-end read did.
13. **Observability**: per-tool findings with severity; per-server summary; total-surface manifest; artifacts durable (kanban, research-run ledger, or `~/Documents/zk-data/skills/mcp-tool-review/{YYYY-MM-DD}-{server}/` via terminal). Retain historical report paths as provenance, not as the destination for new runs.
14. **Traceability**: every finding cites tool + file:line + verbatim excerpt; count reconciliation at every seam (verify stage N's output shape is what stage N+1 consumes before running the chain). A count cited from a live call is either recomputed from a saved copy of that response or stated as not counted — a live MCP response is not on disk (the curator run typed "22 of 58 events" and "3 of 76 passes" from session responses while the script's own count was 13 of 104; caught in the critic pass).
15. **Integration** [D]: credential resolution (`resolve_db_passphrase`, env injection, `nudge_mcp_servers`), response envelopes (`unwrap_tool_envelope`), settings defaults location, model-name constants. For each column a tool writes, grep the other writers and readers of that table — other processes included — and record what each does at the extremes of the value's range (the curator run's per-tool read stayed inside the server crate; the h_mem deletion trigger sat in `kask_bridge`'s consolidation pass). Integration defects are findings.
16. **Divergence** [D]: name every DIVERGENCE.md seam the review touches; propose removal or minimization where the user-visible purpose no longer needs divergence; prefer `kask/`-side fixes; any unavoidable zed-side edit carries its D-seam entry + test in the same change. The review's own proposals create NO new divergences — a proposal requiring a zed-side edit without a seam entry + test is a defect in the proposal.

### Phase E — Probes [D calls + P classification; verification: static/live reconciliation]

17. Live-probe charter:
    - Configuration, before the first probe: read the running server's environment variable NAMES (never values) and name the code paths the isolated configuration cannot reach (the research run probed keyless for two rounds while the operator's server held five provider keys; its three highest-severity defects were reachable only with keys — reported by the operator as process error P-01).
    - Scope: the write prohibition protects the operator's running server and data — a durable-mutation tool is never probed against them. In an isolated temp directory every tool is probed, writes included: excluding write probes hid the scenarios run's two worst defects — a store file that did not read back was treated as an empty store and the next write replaced it (SC-14, critical), and `scenario_status` lost its cached tree at every restart while the reference doc said it survived (SC-13) — both need a write followed by a read or a restart. For a file- or database-backed server the round includes (a) a restart between write and read and (b) a store file that does not read back, followed by one write. Ephemeral in-memory mutation is allowed with probe-prefixed identifiers and disclosed in the artifact.
    - Isolation: every probe server runs with a cleared environment and every kask path on a temp dir, AND sets `XDG_RUNTIME_DIR` to its temp dir — `InferenceIpcClient::from_env` falls back to `{XDG_RUNTIME_DIR or /run/user/<uid>}/kask/inference-socket-path`, so a cleared environment alone still finds the operator's editor bridge (kask/crates/hkask-inference/src/inference_ipc_client.rs, `read_socket_path_from_file` / `runtime_dir_with`); a probe server started without it has sent calls to the live editor bridge. Isolation covers cargo and every executable under `target/`, not only probe servers: every cargo test and every `target/` binary runs through one isolating wrapper that clears all `HKASK_`/`KASK_` variables and puts every kask path on a temp dir (`instruments/iso.sh`); a helper that runs a test binary takes the path cargo prints for `unittests src/<lib>.rs` and refuses to run it unless `--list` names the test (a glob-picked binary started a working-tree server 601 times in the terminal's environment, each start resolving the keychain passphrase and opening the operator's real registry); a loop whose every iteration fails is stopped and read, not re-run. The run records a check that no probe output holds a bridge-side response; that check reports the matched lines (or counts lines naming a socket connection), never a bare word count — provider warnings contain the word "bridge" with no bridge present (`grep -ci bridge` reported 9, then 3, such lines in sessions with no bridge). `instruments/probe-lib.sh` is the canonical isolated stdio session helper (start/call/stop over a temp HOME, config, runtime, data and artifacts dir; add server-specific env in `start`).
    - Operator-state guard: sha256 of each operator store the server can reach, taken at run start and compared before every commit and at the end; the per-round size-and-mtime check stays as the fast one — it reported UNCHANGED for every round while a registry grew 4096 bytes between two rounds, and only the content hash sees that.
    - Instruments: read an instrument's full usage — defaults included — before running it, and pass an explicit label to any script that writes evidence directories (the scenarios run's `probe-b.sh` defaulted its stores-dir to `installed`, and an unlabelled run overwrote the previous pass's `stores.installed` evidence).
    - Network rounds: a declared per-host request budget is written to the run directory BEFORE the first request, and any command that starts the server binary — including one used only to validate a fix — is a probe round under the same budget discipline (the research run sent one undeclared keyless web_ping inside a validation command and exceeded a declared budget: OpenAlex 7 against 5, arXiv 4 against 3, example.com 5 against 3).
    - Writes that need inference: a server whose writes embed (or otherwise call the inference bridge) before committing cannot be write-probed over stdio in isolation — an isolated server has no bridge, so no row can be created. Name the case when it holds and substitute wire-decoded requests in the crate harness (JSON arguments against the handler); the artifact records which results are live probes and which are harness pins (the curator run: every curator write embeds before it commits, so its write probes were handler-level tests with JSON arguments).
    - Reconciliation: every probe result is reconciled against the static source read (second channel); disagreement is a finding. For a numeric tool, compute the expected value independently BEFORE reading the tool's answer — a right answer and a wrong one both look plausible on their own (`scenario_update` came back 0.99, the clamp's ceiling — wrong, SC-10 — while `scenario_recompute_posteriors`' 0.8 was right; the independently computed number is what told them apart). A failing probe is classified failure evidence, not an abort; report tool defects via `curator_report_skill_use_issue` and continue.

### Phase F — Findings assembly and verification [P + D gates]

18. Assemble the completed semantic records, then serialize the artifact from the full context below with `instruments/serialize-artifact.jq` — the template's body as jq: same keys, same order, values passed through. `render_template` returns its output to the model and has no file sink, so it cannot produce the artifact bytes; render it once for the shape's contract only. The check is key order against the `.j2` plus `jq ==` per value against the rendered shape (an `artifact-compare` receipt); never send the artifact through a model to reconstruct counts, lists or a wrapper object. Assembly filters that append are not idempotent: count the assembled output against the expected counts after EVERY filter application and keep the single-application intermediate as the recovery point (the scenarios third-pass amend filter, applied twice, doubled its findings — 27 against the expected 25 — and later duplicated citations; both doublings were caught by the count and restored from the single-application file). Call tallies about the run's own calls are computed by jq from `live-reads.tsv` / `live-writes.tsv` at build time, never typed into a finding — records about the run's own calls go stale when a correction is itself a call (the curator run's CU-H1/CU-P1 were amended three times because each correction — a `memory_update`, a search for an id — changed the tally they report). Pass MTR-1's computed reconciliation unchanged. Gated checks remain `pending-rulings`; use explicit not-run/blocked evidence metadata where checks have not run, not fabricated success. After the citation check, re-serialize with its actual receipt and retain the prior artifact as history.
19. Run `bash kask/scripts/check-mcp-review-citations.sh <artifact.json>` — every citation must exist (file present, line in range, excerpt within ±3 lines). Fix and re-run at most twice; a third failure stops the run with the failures listed.
20. Run the routing invariant in `lisp_eval`: `(and (= unclassified 0) (= ungated 0) (= bad_region 0) (>= operator_roles 1) (>= other_roles 1))` over the run's step classification, and verify citation-check exit and actuator-routing completeness via `MTR-2`: `(and (= citation_script_exit 0) (= unrouted_findings 0) (= unanchored_tools 0))`. Both envs are produced by `jq` over the run's saved records, never typed — the routing counts from the run's `steps.json` (one row per step: `step`, `type` D|P, `gate`, `role` operator|critic|instrument|agent|curator):
    ```jq
    { unclassified: ([.[] | select((.type // "") == "")] | length), ungated: ([.[] | select((.gate // "") == "")] | length), bad_region: ([.[] | select(.type == "P" and ((.gate // "") | test("script|exit code|probe|lisp_eval")) and ((.gate // "") | test("operator|decision|re-read|critic") | not))] | length), operator_roles: ([.[] | select(.role == "operator")] | length), other_roles: ([.[] | select(.role == "critic" or .role == "instrument" or .role == "agent" or .role == "curator")] | length) }
    ```
    and the MTR-2 counts from the saved `artifact.json` (a tool is unanchored when any of `anchor`, `fidelity_label`, `dp_character` is blank, or its declared pending marker `anchor_status` reads `pending`; a finding is unrouted when `actuator` is blank; a missing `citation_check.exit_code` reads −1, which fails the gate):
    ```jq
    { citation_script_exit: (.citation_check.exit_code // -1), unrouted_findings: ([.findings[] | select((.actuator // "") == "")] | length), unanchored_tools: ([.tools[] | select((.anchor // "") == "" or (.fidelity_label // "") == "" or (.dp_character // "") == "" or (.anchor_status // "") == "pending")] | length) }
    ```
    Save both filter outputs beside the artifact (`routing-inputs.json`, `gate-inputs.json`) as the receipts the gates were run on.
21. Critic pass [P; decoupled by rubric]: re-read each finding against its citation — would a reviewer checking the cited line see the claimed behavior? Drop or amend what fails; record the pass in the artifact. The critic pass runs before any commit whose message states a count or a universal, and recounts any number carried from before a surface change (the research run's critic found a web_ping request count of nine where the registrations make seven — already in a doc and a commit message — and a second defect, ledger writes without a transaction, at a line cited for a different finding).

### Phase G — Fix, pin, and route [D + P]

22. Close the loop on every finding: an in-scope code defect is FIXED in the same pass — the change, its pinning test, and the touched crates' suites green — before the run reports; after each fix commit the gate set also runs: `check-docs-citations.sh`, `check-docs-citation-drift.sh`, `check-code-docs-citations.sh`, `check-rustfmt.sh`, plus `check-mcp-readme-counts.sh` and `check-mcp-registration.sh` when a tool was added or removed, and the ontology literal guard (`cargo test -p hkask-bridge-ontology --test ontology_literal_guard` — tool descriptions are string literals in its scanned trees, so every description edit a review makes is in its scope). A fix that only moves lines can still break a docs gate. Each gate's exit is taken from the gate itself, never from a compound command whose earlier member may have short-circuited it — in `gate1 && gate2; echo $?` a failing gate1 short-circuits the chain and `$?` is gate1's exit, not gate2's. Scripted multi-line substitutions have corrupted lines outside their target (a source file on 2026-11-11; a crate's `forbid(unsafe_code)` attribute on 2026-11-12, through an unanchored pattern): after any scripted edit, read the complete diff of each touched file; prefer exact-text edits for anything that is not a whole-token rename; a scripted replacement counts its matches and refuses to write on anything but exactly one (`instruments/subst-once.sh`). Run the docs-citation gates after the LAST source edit, not after each intermediate one: re-anchor citations from the tree once, then read each re-anchored endpoint back — a further edit moves lines again (the corpus run re-anchored twice through this and the drift was caught only on read-back). A sentence added to a reference page in the same pass is a claim: probe it before committing it (the corpus run committed a doc sentence an hour before a probe showed it false for read-only inputs). After any change to a request type or a description, capture tools/list before and after and diff the two per tool with jq — every changed byte is either named in the commit message or is a defect (the curator run's CU-15 fix added `schemars(schema_with)` to two Option fields; its pin asserted the enum values and passed, while the same change moved both fields into the schema's `required` list, which nothing read until the installed binary's listing was compared a day later); then run `instruments/measure-surface.sh` (forwarded text) and `instruments/required-nullable.jq` (optional properties listed as required) on the after capture. When a fix needs a shared file that another stream has dirty (the research run: `Cargo.lock` for a new dependency edge), do not commit from a hand-built index: save the patch, its commit message and numstat in the run directory, record the condition for landing it, and land it when the condition holds — the research run's patch waited for Cargo.lock clean at HEAD and landed then. A finding left unfixed is an unfinished review. Only findings that change a tool's functional contract (behavior a consumer depends on) go to the operator as decisions, and platform-level redesigns go to kanban/algedonic with the evidence.
23. Verify live where possible: after a fix that changes wire behavior, re-probe the live tool (server rebuild+restart when the running binary predates the fix) and record the before/after. "Predates the fix" is measured, never copied from a handoff note: before writing a pending-restart receipt, hash the running process's executable (`sha256sum /proc/<pid>/exe`), read its start time (`ps -o lstart= -p <pid>`), and compare both with the fix's build (the built binary's hash and mtime); record all three in the artifact's reconciliation. Where live verification is gated on a restart, name the exact remaining step and mark the receipt pending-restart — never claim it.
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

Render `mcp-tool-review/findings-artifact` for the JSON shape. Load-bearing fields: `reconciliation` (counts + instrument + result), `tools[]` (name, file, line, anchor, fidelity_label, dp_character, anchor_status, axes, probe, status), `findings[]` (id, tool, axis, severity, claim, citations[{file, line, excerpt}], evidence, proposal, actuator, status), `gated_pending_rulings[]`, `citation_check`, `ask_first`. `tools[].anchor_status` is the declared pending marker — `pending` when the anchor waits on an operator ruling — recorded there and never in prose; MTR-2 counts it unanchored (the corpus run encoded the state in fidelity-label text and its artifact passed with five unresolved anchors). `findings[].status` uses a closed deployment vocabulary — `open` (not fixed), `fixed (in-tree)` (fix committed, the running binary predates it), `fixed (deployed)` (in the running binary and reproducing on isolated probes), `fixed (live-verified)` (re-probed live, before/after recorded), `pending-restart` (live verification gated on a restart, with step 23's three measurements recorded) — and the actuator names what was and was not re-probed; runs do not invent their own deployment words (finished runs had invented `fixed (pending-restart)`, `fixed (live-verified)`, `deployed (live-verified …)`). Save new runs under `~/Documents/zk-data/skills/mcp-tool-review/{YYYY-MM-DD}-{server}/` via terminal — for payloads exceeding comfortable single-call size (large slices, 60+ tools), write in chunks (each chunk independently valid JSON, merged with jq, validated, then the citation script run): same artifact, same checks, no truncation risk.

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
- Never probe a durable-mutation tool against the operator's running server or data — in an isolated temp directory every tool is probed, writes included; never leave a probe's ephemeral state undisclosed.
- Bash-only syntax (`<(`, `<<<`, `$'`, `PIPESTATUS`) is never typed into the /bin/sh terminal: the command is written to a script file in the run directory and run with `bash` (stating the rule has not prevented the error — five runs hit it: four skill-use reports and one artifact note).
- Findings are fixed in-tree with pinning tests in the same pass; functional-contract changes and platform redesigns go to the operator / kanban, never implemented unilaterally. Never commit unless the operator explicitly asks; when the ask delegates commits (a batch that names them), commit verified work with a pathspec.
- No second clone or worktree for any measurement (absolute prohibition) — measurements run in-tree.
- A tool's missing anchor, label, or D/P character is a finding, never a load-time failure.

## Registry Templates

| Template | Purpose |
|---|---|
| `findings-artifact.j2` | The findings artifact contract: the JSON shape (reconciliation, per-tool records, findings with citations, gated-pending list, citation-check receipt, ask-first record). `render_template` supplies the shape's contract only — the artifact bytes are the same context serialized by `instruments/serialize-artifact.jq`, which the citation script verifies. |

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
- Routing invariant and `MTR-2` producers (2026-11-13; re-evaluated by `~/Documents/zk-data/skills/mcp-tool-review/2026-11-13-mtr2-producers/receipts.sh`): the step-20 jq filters over the saved `steps.json` of the 2026-11-12 scenarios (third pass) and research runs reproduce those runs' recorded `routing-inputs.json` / `gate-inputs.routing.json` byte for byte; over a synthetic two-step file with one blank type, one blank gate and one P step gated only by `lisp_eval` they return `{unclassified:1, ungated:1, bad_region:1, operator_roles:1, other_roles:1}` → invariant `false`; over a synthetic artifact with one blank actuator and one tool lacking its anchor, fidelity label or D/P character, `MTR-2`'s env reads `unrouted_findings 1`, `unanchored_tools 1` → `false`; a tool whose declared pending marker `anchor_status` reads `pending` counts the same way even with all three text fields filled → `false`; an artifact without a `citation_check.exit_code` reads −1 → `false`.

Historical citation/routing pilot receipts remain historical. Current discovery/raw-name/formatter regressions are the mandatory checks above; passing them does not establish the semantic quality of every tool review.
