---
title: "hKask MCP Server QA Strategy"
audience: [QA engineers, security engineers, agents]
last_updated: 2026-08-01
version: "0.3.1"
status: "Active"
domain: "trust"
mds_categories: [trust, composition, lifecycle]
---

# hKask MCP Server QA Strategy

A per-tool QA routine for every tool exposed by every hKask MCP server, with
explicit pass/fail criteria, skill assignments, and observability hooks.

**Scope**: 11 MCP servers, 206 tools (counted from source via `grep -rn 'Parameters<' kask/mcp-servers/hkask-mcp-*/src/` on 2026-08-01 — never fabricated).

**Status of this document**: Phase 1 (inventory) is grounded in source reads.
Phases 2-5 are the contract. The runnable routine lives alongside this doc
at `kask/scripts/qa-mcp-servers.sh` and the per-tool contracts at
`kask/docs/qa/per-tool-contracts.md`.

---

## Phase 1 — Inventory (from source)

### Server-level summary

| # | Server crate | Binary | Transport | Lib root | Entry | Credentials (env vars) |
|---|---|---|---|---|---|---|
| 1 | `hkask-mcp-codegraph` | `hkask-mcp-codegraph` | stdio | `src/hkask_mcp_codegraph.rs` | `pub async fn run()` L529 | (none declared; `DEEPINFRA_API_KEY`/`OPENROUTER_API_KEY` read inline by `codegraph_index_embeddings`) |
| 2 | `hkask-mcp-companies` | `hkask-mcp-companies` | stdio | `src/hkask_mcp_companies.rs` | `run()` | **required**: `HKASK_FMP_API_KEY`, `HKASK_EODHD_API_KEY`; **optional**: `HKASK_EXA_API_KEY`, `HKASK_TAVILY_API_KEY`, `HKASK_BRAVE_API_KEY` |
| 3 | `hkask-mcp-condenser` | `hkask-mcp-condenser` | stdio | `src/hkask_mcp_condenser.rs` | `run()` | (none; uses `InferencePort` injected from `HKASK_INFERENCE_URL`) |
| 4 | `hkask-mcp-corpus` | `hkask-mcp-corpus` | stdio | `src/hkask_mcp_corpus.rs` | `run()` | **optional**: `HKASK_OCR_MODEL`, `HKASK_EMBEDDING_MODEL`, `HKASK_DEFAULT_MODEL`; inline `FALAI_API_KEY` for docres |
| 5 | `hkask-mcp-curator` | `hkask-mcp-curator` | stdio | `src/hkask_mcp_curator.rs` | `run()` | **optional**: `HKASK_CURATOR_DB`, `HKASK_DB_PASSPHRASE` |
| 6 | `hkask-mcp-kata-kanban` | `hkask-mcp-kata-kanban` | stdio | `src/hkask_mcp_kata_kanban.rs` | `run()` | **optional**: `HKASK_KANBAN_DB`, `HKASK_DB_PASSPHRASE` |
| 7 | `hkask-mcp-media` | `hkask-mcp-media` | stdio | `src/hkask_mcp_media.rs` | `run()` | **optional**: `DEEPINFRA_API_KEY`, `FALAI_API_KEY` (vision via IPC bridge to zed) |
| 8 | `hkask-mcp-research` | `hkask-mcp-research` | stdio | `src/hkask_mcp_research.rs` | `run()` | **optional**: `HKASK_BRAVE_API_KEY`, `HKASK_FIRECRAWL_API_KEY`, `HKASK_TAVILY_API_KEY`, `HKASK_SERPAPI_API_KEY`, `HKASK_EXA_API_KEY`, `HKASK_BROWSERBASE_API_KEY` |
| 9 | `hkask-mcp-scenarios` | `hkask-mcp-scenarios` | stdio | `src/hkask_mcp_scenarios.rs` | `run()` | (none; uses `reqwest::Client` for upstream research/companies calls) |
| 10 | `hkask-mcp-training` | `hkask-mcp-training` | stdio | `src/hkask_mcp_training.rs` | `run()` | **optional**: `RUNPOD_API_KEY`, `DEEPINFRA_API_KEY`, `NEBIUS_PROJECT_ID`, `NEBIUS_SUBNET_ID`, `HKASK_TRAINING_HOST`, `RUNPOD_TEMPLATE_ID`, `RUNPOD_GPU_TYPE_ID`, `RUNPOD_CONTAINER_DISK_GB`, `RUNPOD_DOCKER_IMAGE`, `HKASK_TRAINING_DB`, `HKASK_DB_PASSPHRASE` |
| 11 | `hkask-mcp-swarm` | `hkask-mcp-swarm` | stdio | `src/hkask_mcp_swarm.rs` | `run()` L2551 | **optional**: `HKASK_SWARM_DB`, `HKASK_DB_PASSPHRASE` |

All servers use `hkask_mcp_server::run_server` → `run_stdio_server` (stdio
transport). No SSE/HTTP server exists in the fleet.

### Tool inventory (206 tools)

Tool counts (verified 2026-08-01 via `grep -rn 'Parameters<' kask/mcp-servers/hkask-mcp-*/src/`): codegraph 8, companies 40, condenser 6, corpus 26, curator 11, kata-kanban 18, media 37, research 15, scenarios 18, training 8, swarm 19.

The full per-tool table (with input schema struct, output shape, LLM I/O
boundary, external deps, McpToolError kinds used) is in
`kask/docs/qa/per-tool-contracts.md`. The summary table here is the index.

| Server | Tools | LLM I/O boundary? | External deps hit |
|---|---|---|---|
| codegraph | 8 | yes — `codegraph_index_embeddings` calls embedding API; `codegraph_context` returns LLM-bound text | sqlite (bundled), tree-sitter, reqwest (DeepInfra/OpenRouter embeddings) |
| companies | 40 | yes — `research_search` returns raw LLM/web claims; `company_screener` parses NL prompts | FMP, EODHD, Exa, Tavily, Brave (reqwest) |
| condenser | 4 | yes — `condenser_thread_summary`, `condenser_score_saliency` call `InferencePort` | `InferencePort` (HKASK_INFERENCE_URL), episodic/semantic memory |
| corpus | 26 | yes — `corpus_compose`, `corpus_mashup`, `corpus_generate_qa`, `corpus_extract_triples`, `corpus_tag_chunks`, `corpus_ocr` all return LLM output | inference port, FAL docres, sqlite FTS5 |
| curator | 11 | no — reads Regulation ledger, does not call LLM | sqlite (SQLCipher) |
| kata-kanban | 18 | no — pure state machine over sqlite | sqlite (SQLCipher) |
| media | 37 | yes — `generate_image`, `generate_video`, `voice_design`, `generate_speech`, `transcribe`, `describe_image`, `gallery_analyze`, `video_caption` all return model output | DeepInfra, fal.ai, Together AI, ElevenLabs (reqwest) |
| research | 15 | yes — `web_search`, `web_extract`, `web_browse`, `web_find_similar` return fetched/scraped content (indirect LLM boundary via tool output) | Brave, Firecrawl, Tavily, SerpAPI, Exa, Browserbase, arXiv, Semantic Scholar (reqwest) |
| scenarios | 18 | yes — `scenario_brainstorm`, `scenario_synthesize`, `scenario_assess` call inference; `scenario_research` calls research server | inference port, research server, companies server |
| training | 8 | yes — `training_validate_config`, `training_evaluate` call LLM; `training_submit` provisions GPU pods | RunPod, DeepInfra, Nebius, HuggingFace, OpenAI (reqwest) |
| swarm | 19 | yes — `swarm_generate_prompt`, `swarm_generate_ontology` call LLM; `swarm_execute_agent` dispatches to local agents | sqlite (SQLCipher), local agent runtime |

### Existing test coverage (from source)

| Server | `#[cfg(test)]` modules in `src/` | `tests/` dir | Verdict |
|---|---|---|---|
| codegraph | 10 | yes (1 file: `qa_contract.rs`) | moderate inline coverage |
| companies | 20 | no | moderate inline coverage |
| condenser | **0** | yes (1 file: `qa_contract.rs`) | **gap — no inline tests**; qa_contract.rs covers the 8 tools |
| corpus | 29 | yes (4 files) | moderate inline coverage |
| curator | 1 | yes (1 file: `qa_contract.rs`) | **gap — minimal inline tests**; qa_contract.rs covers the 11 tools |
| kata-kanban | 5 | yes (2 files: `qa_contract.rs`, `service_integration.rs`) | qa_contract.rs covers all 18 tools |
| media | 5 | no | low inline coverage |
| research | 4 | yes (1 file: `research_contract.rs`) | low |
| scenarios | 1 | yes (1 file: `scenarios_contract.rs`) | **gap — minimal inline tests** |
| training | 15 | yes (1 file: `live_adapter.rs`) | moderate inline, no tool-behavior |

The existing CI gate `scripts/check-mcp-tool-tests.sh` passes because its
grep keys on `Parameters(` in `src/` (where every tool's signature lives),
not on actual test invocations. This is a known false-positive in the gate
that this QA strategy must close.

### capability / `reg.guard.*` reality check (material finding)

The Phase 2.3 contract assumes tools emit `reg.guard.*` on capability denial.
Source inspection shows this is **not wired**:

- `codegraph`'s `ensure_indexed()` carries the comment
  *"capability-governed file access (#5): future integration point. When the daemon
  provides capability verification, filter paths here via capability tokens
  before passing to index_directory. For now: index entire workspace
  (standalone mode)."*
- `reg.guard` appears only as a `tracing::warn!` target in
  `hkask-mcp-corpus/src/tools/semantic/mod.rs` for output-scan violations —
  not for capability denial.
- `kata-kanban` maps `KanbanError::PermissionDenied` →
  `McpToolError::permission_denied`, but the underlying `KanbanError` is
  produced by state-machine logic, not by a capability-tier check at the tool
  entry point.
- `CapabilityTier` is stored on `CodeGraphServer` and `CondenserServer` but
  is only **read** to report `mode` in `condenser_ping` — never **gated**.

**Implication for the QA routine**: Phase 2.3 (capability/auth denial) cannot
assert `reg.guard.*` emission because no tool emits it. The routine must
instead assert the *negative* — that calling a tool without a required
credential produces a structured `permission_denied` or
`failed_precondition` error (not a panic) — and flag the missing
`reg.guard.*` emission as a `proposed` RR entry. See Gap Report §4.

### `reg.qa.*` namespace reality check

`CANONICAL_NAMESPACES` (in `kask/crates/hkask-types/src/event.rs`) registers
the following `reg.qa.*` namespaces:

- `reg.qa.mutant_survived`
- `reg.qa.repair_attempted`
- `reg.qa.repair_exhausted`
- `reg.qa.repair_verified`
- `reg.qa.bolero_failure`
- `reg.qa.run` (QA routine pass — emitted by `scripts/qa-mcp-servers.sh`)
- `reg.qa.run.pass`
- `reg.qa.run.fail`
- `reg.qa.run.skipped`

The `reg.qa.run*` namespaces were added since the original Phase 1
inventory (Gap A is now closed). The runnable routine in Phase 4 can emit
`reg.qa.run.pass` / `reg.qa.run.fail` / `reg.qa.run.skipped` per
(tool, category) cell.

---

## Phase 2 — Per-Tool Test Contract

Every tool must cover all seven categories. If a category does not apply,
state why. The per-tool contracts (with the exact input struct, expected
output shape, and the seven-category assertions) live in
`kask/docs/qa/per-tool-contracts.md`. The category definitions are fixed
here:

1. **Happy path** — well-formed input, expected output shape, expected
   `reg.tool` span emitted with `outcome=ok` and well-formed fields
   (`tool`, `outcome`, `duration_ms`, `error_kind=""`, `caller=<webid>`,
   `ontology=<concept-or-empty>`). The span is emitted by `ToolSpanGuard`
   via `tracing::info!(target: "reg.tool", ...)` in
   `hkask-mcp-server/src/server/tool_span.rs`.
2. **Schema violation** — (a) missing required field, (b) wrong type,
   (c) extra unknown field. Assert the error reaches the caller as a
   structured `McpToolError` JSON `{"error": "...", "kind": "..."}` —
   never silently swallowed (project rule against `let _ =`). For rmcp
   `Parameters<T>` deserialization failures, rmcp itself returns a
   `-32602 Invalid params` JSON-RPC error before the tool body runs; the
   routine asserts this surfaces as a non-panic error response.
3. **capability / auth denial** — call without the required capability/token.
   Assert: (a) no panic, (b) structured error returned, (c) denial is
   observable. **Because no tool currently emits `reg.guard.*` on denial
   (see Phase 1 reality check), the routine asserts the structured-error
   contract and records the missing `reg.guard.*` emission as a
   `proposed` RR entry — it does NOT assert a span that the code does not
   emit.**
4. **Partial / empty result** — empty corpus, missing entity, no match.
   Assert no panic and a typed empty result (e.g. `[]`, `{"error":
   "symbol not found: X"}`, `{"symbols_embedded": 0, "note": "..."}`).
5. **Error propagation** — underlying dependency fails (DB, HTTP upstream,
   LLM inference). Assert the error reaches the caller as a structured
   `McpToolError` with a meaningful `kind` (mapped via
   `classify_http_error` for HTTP; `db_err` for sqlite). The routine
   simulates failure by pointing the tool at an unreachable upstream
   (e.g. `HKASK_INFERENCE_URL=http://127.0.0.1:1`) or a closed sqlite
   handle — never by mocking inside the tool.
6. **Resource bounds** — large input, long-running call. Assert: (a)
   context-budget enforcement where the tool declares a budget (e.g.
   `codegraph_context`'s `ContextBudget`), (b) the routine's own
   `--timeout` fires before the call hangs. Tools without an explicit
   budget are still bounded by the routine's outer timeout.
7. **Adversarial input** (LLM-I/O tools only) — injection, hijack,
   exfiltration, tool-misuse attempts per `adversarial-red-team`
   categories. Applies to every tool marked "LLM I/O boundary? yes" in
   the Phase 1 table. Tools with "no" are skipped with reason
   "not an LLM I/O boundary".

---

## Phase 3 — Skill Assignment

Each assignment states the tools covered, the template/phase used, and the
evidence produced.

### 3.1 LLM I/O boundary tools → `adversarial-red-team` + `kali-audit`

- **Tools covered**: every tool marked "LLM I/O boundary? yes" in Phase 1
  (codegraph: `codegraph_index_embeddings`, `codegraph_context`;
  companies: `research_search`, `company_screener`; condenser:
  `condenser_thread_summary`,
  `condenser_score_saliency`; corpus: `corpus_compose`,
  `corpus_mashup`, `corpus_generate_qa`, `corpus_generate_qa_batch`,
  `corpus_extract_triples`, `corpus_tag_chunks`, `corpus_ocr`,
  `corpus_rewrite`, `corpus_build_persona`, `corpus_compare`,
  `corpus_embed`; media:
  `generate_image`, `generate_video`, `voice_design`, `generate_speech`,
  `transcribe`, `transcribe_bundle`, `record_and_transcribe`,
  `describe_image`, `gallery_analyze`, `extract_object`,
  `transform_image`, `upscale_image`, `execute_workflow`,
  `image_remove_background`, `image_apply_style`, `image_to_video`,
  `video_remix`, `video_from_images`, `video_caption`, `video_meme`; research: `web_search`, `web_extract`, `web_browse`,
  `web_find_similar`; scenarios: `scenario_brainstorm`,
  `scenario_synthesize`, `scenario_assess`, `scenario_research`;
  training: `training_validate_config`, `training_evaluate`).
- **Template/phase**: `adversarial-red-team` single-shot category pass
  (injection, hijack, exfiltration, tool-misuse) at persistence level 1;
  `kali-audit` LLM I/O surface (OWASP LLM Top 10 2025, MITRE ATLAS v5.1).
- **Evidence**: `reg.runtime.*` spans from `runtime-posture-monitor`
  observing the live calls; `proposed` RR-NNNN entries for any bypass
  found; oracle reports in `kask/docs/qa/oracle/<server>__<tool>.md`.

### 3.2 Rust code path → `bug-hunt` + `kali-audit`

- **Tools covered**: all 206 tools (the implementation path of every tool).
- **Template/phase**: `bug-hunt` Charter → Probe → Oracle → Taxonomize →
  Report → Convergence, with Beizer taxonomy and the missing-tests
  detection sub-phase (which will surface the condenser/curator
  zero-`#[cfg(test)]` gap from Phase 1); `kali-audit` code surface (CWE).
- **Evidence**: `reg.bughunt.*` spans; bug-hunt pattern signatures; oracle
  reports with `file:line` citations; `proposed` RR entries for any
  finding not already covered by RR-0001..RR-0021.

### 3.3 Dependency manifest → `supply-chain-sentinel`

- **Tools covered**: all 11 servers (Cargo.toml scope, not per-tool).
- **Template/phase**: `supply-chain-sentinel` 4-phase pipeline
  (select → probe → report → convergence) over each server's `Cargo.toml`
  and the workspace `deny.toml`.
- **Evidence**: `reg.supply_chain.*` spans; `proposed` RR entries with
  `surface: supply-chain` (extending RR-0021's quick-xml precedent).

### 3.4 Runtime behavior → `runtime-posture-monitor`

- **Tools covered**: all 206 tools, observed live during the QA routine.
- **Template/phase**: `runtime-posture-monitor` 4-phase pipeline
  (select → classify → regulate → convergence) consuming `hkask.*`
  performative spans and `reg.tool` spans emitted by the routine itself.
- **Evidence**: `reg.runtime.*` spans; `proposed` RR entries with
  `surface: runtime` for endpoint abuse / LLM usage anomalies observed
  during the pass.

### 3.5 Code-graph / call-site coverage → `graph-audit`

- **Tools covered**: all 206 tools (verification that the QA routine
  actually reaches each tool).
- **Template/phase**: `graph-audit` code mode via the
  `hkask-mcp-codegraph` MCP server — query the symbol graph for each
  tool fn and confirm a test call site exists.
- **Evidence**: `reg.bughunt.*` coverage sub-metric; a coverage report
  at `kask/docs/qa/coverage.md` listing reached vs. unreached tools.

### 3.6 Test scaffolding → `task-breakdown` + `tdd`

- **Tools covered**: the QA work itself (not the MCP tools).
- **Template/phase**: `task-breakdown` to slice the QA work into
  per-server vertical slices with acceptance criteria;
  `tdd` red-green-refactor against the per-tool contracts in
  `per-tool-contracts.md`, anchored to the seven categories with
  `// REQ: <category>` tags.
- **Evidence**: `kask/docs/qa/plan.md` + `kask/docs/qa/todo.md` from
  `task-breakdown`; `reg.skill.task-breakdown.*` and
  `reg.skill.tdd.*` spans.

### 3.7 Skill composition → `skill-bundler`

When multiple skills apply to the same tool (e.g. an LLM I/O tool needs
3.1 + 3.2 + 3.4), `skill-bundler` produces a manifest with ordering and
conflict resolution. See Phase 4 deliverable 3.

---

## Phase 4 — Deliverables

### Deliverable 1 — Coverage matrix

The coverage matrix is the convergence artifact. Each cell has a status
of `pass | fail | skipped-with-reason`. The matrix is generated by the
runnable routine (Deliverable 2) and written to
`kask/docs/qa/coverage-matrix.md`. Schema:

```
| server | tool | category | skill | status | evidence |
```

- `category` ∈ {happy, schema-violation, ocap-denial, empty-result,
  error-propagation, resource-bounds, adversarial}
- `skill` ∈ {bug-hunt, kali-audit, adversarial-red-team,
  supply-chain-sentinel, runtime-posture-monitor, graph-audit, tdd, n/a}
- `status` ∈ {pass, fail, skipped-with-reason}
- `evidence` — path to the oracle report or the `reg.*` span target line

The matrix starts empty (all `pending`) and is filled by the routine.
Convergence is reached when no cell is `pending`.

### Deliverable 2 — Runnable QA routine

The runnable routine is `kask/scripts/qa-mcp-servers.sh` (a bash driver)
plus a per-tool Rust contract test file per server under
`kask/mcp-servers/<server>/tests/qa_contract.rs`. The driver:

- builds the server binary (`cargo build --bin <binary>`)
- spawns it over stdio with a controlled env (no real API keys →
  triggers Phase 2.3 and 2.5 paths; a fake upstream on `127.0.0.1:1` →
  triggers Phase 2.5)
- for each tool, invokes the contract test via `cargo test --package
  <server> qa_contract::`
- emits one `reg.qa.run` span per tool (namespace is registered — see
  Gap Report §4 Gap A, now closed)
- writes the row to `coverage-matrix.md`
- has an explicit `--max-iterations` (default 1 pass per tool) and
  `--timeout` (default 30s per tool call, 10m per server)

Each `qa_contract.rs` file is independently callable (`cargo test
--package <server> qa_contract`) and idempotent (no shared state between
tools; each test sets up and tears down its own DB where needed).

### Deliverable 3 — Skill-bundle manifest

Recommended bundle for a full QA pass against one server:

```yaml
# kask/docs/qa/skill-bundle.yaml
bundle: mcp-server-qa
order:
  - task-breakdown        # slice the work, write plan.md + todo.md
  - tdd                   # red-green against per-tool-contracts.md
  - bug-hunt              # exploratory charters over the Rust code path
  - kali-audit            # CWE / OWASP LLM / ATLAS over code + LLM I/O
  - supply-chain-sentinel # Cargo.toml / deny.toml
  - runtime-posture-monitor # observe live tool calls during the routine
  - graph-audit           # confirm every tool is reached by the routine
conflict_resolution:
  - {between: [bug-hunt, kali-audit], rule: "bug-hunt finds, kali-audit classifies; do not double-report"}
  - {between: [kali-audit, supply-chain-sentinel], rule: "supply-chain-sentinel owns Cargo.toml; kali-audit owns src/"}
  - {between: [adversarial-red-team, kali-audit], rule: "adversarial-red-team probes the live LLM boundary; kali-audit reads the code; both propose RR entries but kali-audit's are surface: mcp, red-team's are surface: runtime"}
  - {between: [runtime-posture-monitor, adversarial-red-team], rule: "runtime-posture-monitor observes real traffic; adversarial-red-team generates synthetic traffic; do not let red-team traffic posture-monitor"}
evidence:
  spans: [reg.bughunt.*, reg.runtime.*, reg.supply_chain.*, reg.skill.*, reg.qa.run]
  reports: kask/docs/qa/oracle/<server>__<tool>.md
  regressions: kask/security/regressions/RR-NNNN.yaml (status: proposed)
```

### Deliverable 4 — Gap report

#### Gap A — No `reg.qa.run` namespace for QA routine passes (CLOSED)

`CANONICAL_NAMESPACES` now registers `reg.qa.run`, `reg.qa.run.pass`,
`reg.qa.run.fail`, `reg.qa.run.skipped` (in
`kask/crates/hkask-types/src/event.rs`). The runnable routine can emit
these per (tool, category) cell. This gap is closed; the original
proposal below is retained for the audit trail.

**Original proposal (now implemented):**

```text
// in kask/crates/hkask-types/src/event.rs CANONICAL_NAMESPACES
"reg.qa.run",            // a QA routine pass executed a tool
"reg.qa.run.pass",       // the tool call passed its contract
"reg.qa.run.fail",       // the tool call failed its contract
"reg.qa.run.skipped",    // the tool call was skipped with a reason
```

#### Gap B — No `reg.guard.*` emission on capability denial

No MCP server emits `reg.guard.*` when a tool is called without the
required capability. The Phase 2.3 contract is therefore weakened to
"structured error, no panic" until capability-match span emission is wired. **Filed as RR-0022**
(`kask/security/backlog/RR-0022.yaml`, status: `proposed` — moved out of the
enforced regressions directory in the 2026-08-01 prune):

```yaml
id: RR-0022
title: "MCP tools do not emit reg.guard.* on capability denial"
surface: mcp
cwe: CWE-862
owasp_llm_2025: LLM06
atlas_tactic: AML.TA0006
discovered_in: kask/mcp-servers/hkask-mcp-codegraph/src/hkask_mcp_codegraph.rs (ensure_indexed comment), kask/mcp-servers/hkask-mcp-condenser/src/hkask_mcp_condenser.rs (capability_tier read-only)
severity: medium
detection:
  kind: grep
  pattern: "capability_tier"
  include: "kask/mcp-servers/**/src/**/*.rs"
mitigation: "Gate tool entry on capability_tier checks; emit reg.guard.violation on denial; return McpToolError::permission_denied"
ci_gate: scripts/check-kali-regressions.sh
status: proposed
```

The RR entry is filed; the QA routine references it rather than
re-proposing it.

#### Gap C — condenser and curator have zero `#[cfg(test)]` modules

`hkask-mcp-condenser` and `hkask-mcp-curator` have no inline test modules
in `src/`. The existing `check-mcp-tool-tests.sh` gate passes because it
greps for `Parameters(` in `src/` (where every tool signature lives),
which is a false positive. **No new RR entry** — this is a coverage gap,
not a security regression — but `bug-hunt`'s missing-tests sub-phase will
flag it and `tdd` should drive the red-green to close it.

**Update (2026-07-29):** both servers now have `tests/qa_contract.rs`
files covering all their tools (condenser 8, curator 11). The inline
`#[cfg(test)]` gap in `src/` remains, but the per-tool contract is covered
by the integration test files.

#### Gap D — No existing skill covers "MCP stdio transport-level fuzzing"

The seven categories cover tool *semantics* but not stdio transport
robustness (malformed JSON-RPC framing, partial messages, oversized
messages). No installed skill covers this. **Recommendation**: invoke
`skill-discovery` with the gap signal "MCP stdio transport fuzzing" to
find a candidate skill; if none exists, write a small `cargo test` harness
in `hkask-mcp-server` that feeds malformed frames to `run_stdio_server`.

#### Gap E — `reg.qa.bolero_failure` is registered but no bolero harness exists in MCP servers

The namespace exists for property-test failures, but no MCP server
crate has a `[dev-dependencies] bolero` entry. This is not blocking —
the QA routine does not require property tests — but `bug-hunt`'s Probe
phase should note it as a missing-test type.

### Deliverable 5 — One complex prompt (the full QA pass)

Paste-able into a follow-up agent thread. Bounded tool loops, explicit
stop conditions, self-contained.

```text
Run the hKask MCP server QA pass against the server named in $1
(one of: hkask-mcp-codegraph, hkask-mcp-companies, hkask-mcp-condenser,
hkask-mcp-corpus, hkask-mcp-curator, hkask-mcp-kata-kanban, hkask-mcp-media,
hkask-mcp-research, hkask-mcp-scenarios, hkask-mcp-training).

Read these files first (they are the contract — do not re-derive):
- kask/docs/qa/mcp-server-qa-strategy.md        (this strategy)
- kask/docs/qa/per-tool-contracts.md            (per-tool 7-category contracts)
- kask/docs/qa/skill-bundle.yaml                (skill composition)
- kask/crates/hkask-mcp-server/src/server/tool_span.rs  (reg.tool span format)
- kask/crates/hkask-types/src/event.rs         (CANONICAL_NAMESPACES)

For the target server, execute every tool's seven categories in this order:
1. happy        — call with the contract's well-formed input; assert the
                  reg.tool span (target: "reg.tool", outcome: "ok") is
                  emitted and the output matches the contract's shape.
2. schema-violation — three sub-calls: (a) omit a required field, (b) wrong
                  type, (c) extra unknown field. Assert each returns a
                  structured error, never a panic, never silently swallowed.
3. ocap-denial  — call with the required credential env var unset. Assert a
                  structured error (permission_denied or failed_precondition)
                  and no panic. DO NOT assert reg.guard.* — it is not wired
                  (see Gap B). Record the missing emission as a proposed RR.
4. empty-result — call against an empty store / missing entity. Assert a
                  typed empty result and no panic.
5. error-propagation — set the upstream to 127.0.0.1:1 (unreachable). Assert
                  the error reaches the caller as a structured McpToolError
                  with a meaningful kind (unavailable, timeout, internal).
6. resource-bounds — call with the largest legal input and a 30s timeout.
                  Assert the tool returns or the timeout fires.
7. adversarial — ONLY for tools marked "LLM I/O boundary? yes" in
                  per-tool-contracts.md. Run the four adversarial-red-team
                  categories (injection, hijack, exfiltration, tool-misuse)
                  at persistence level 1. Skip otherwise with reason
                  "not an LLM I/O boundary".

Bounds:
- Max 1 iteration per (tool, category). Max 7 categories per tool.
- 30s timeout per tool call. 10m timeout per server.
- If a tool needs a live dependency you cannot satisfy (real LLM key,
  real RunPod pod), run the dry-run variant: assert everything except the
  live call, mark the cell skipped-with-reason, and name the dependency.

Stop conditions:
- STOP when every (tool, category) cell for the target server has a status
  of pass | fail | skipped-with-reason. "Looks reasonable" is not stop.
- ESCALATE to the user if the same gap persists across 3 tools (materiality
  guard) — do not re-run.

Output:
- Append rows to kask/docs/qa/coverage-matrix.md
- Write oracle reports to kask/docs/qa/oracle/<server>__<tool>.md for any
  fail or adversarial finding
- Write proposed RR entries to kask/security/regressions/RR-NNNN.yaml with
  status: proposed (do not flip any existing RR to enforced)
- Emit reg.tool spans during the run; emit reg.qa.run.pass / .fail / .skipped
  per (tool, category) cell (Gap A is closed — see Phase 1 §reg.qa.* reality check)

Do not edit .rules. If you discover a non-obvious pattern, propose it as a
"Suggested .rules additions" heading in your final message, not inline.
```

---

## Phase 5 — Convergence Criteria

- **Convergence** = every cell in `coverage-matrix.md` has a status of
  `pass | fail | skipped-with-reason`. "Looks reasonable" is not
  convergence.
- **Materiality guard**: if the same gap (e.g. "no `reg.guard.*`
  emission") persists across 3 tools, escalate to the user rather than
  re-running. Do not loop on a structural gap that the code cannot fix.
- **Dry-run variant**: if a tool cannot be tested without a live
  dependency (DB, LLM key, network, GPU pod), the routine states the
  dependency and runs the dry-run variant — every category except the
  live call, with the live-call cell marked `skipped-with-reason:
  needs <dependency>`.
- **No fabrication**: every tool name, schema field, span target, and
  error kind in the matrix must trace to a source line read during
  Phase 1. The routine does not invent tool names or spans.
- **No silent error swallowing**: the routine itself must not use
  `let _ =` on fallible operations (project rule). Failures are
  propagated to the coverage matrix as `fail` cells with evidence.

---

## Appendix — Source provenance

Every claim in Phase 1 traces to a source read:

- Server list: `kask/crates/kask_bridge/src/mcp_servers.rs`
  `BUILT_IN_MCP_SERVERS` (10 entries) + `kask/mcp-servers/` directory listing.
- Tool names: `rg '#\[tool\((?P<desc>.*?)\)\]\s*...pub async fn (\w+)'`
  over every `kask/mcp-servers/*/src/**/*.rs` (195 matches).
- Credentials: `rg 'CredentialRequirement::(required|optional)\(...'`
  over every server's `run()` factory.
- `reg.tool` span format: `kask/crates/hkask-mcp-server/src/server/tool_span.rs`
  L193-202 (`emit_tool_span`).
- `CANONICAL_NAMESPACES`: `kask/crates/hkask-types/src/event.rs` L111-421.
- `reg.guard.*` absence: `rg 'reg\.guard|PermissionDenied|capability_tier'`
  over `kask/mcp-servers/**/src/**/*.rs` (8 matches, none are denial gates).
- Existing RR entries: `kask/security/regressions/RR-0001..RR-0021.yaml`.
- Existing CI gates: `kask/scripts/check-mcp-servers.sh`,
  `kask/scripts/check-mcp-tool-tests.sh`.
