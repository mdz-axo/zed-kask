# Private-inference review continuation prompt — the privacy panel's composition/interaction patterns, and items 1, 2, 4

**Handoff target:** any agent with repo access and no conversation history.
**Composed:** 2026-11-05 (late evening), from the item-3 close-out session. HEAD
at composition was `d6198ec50f`; re-verify with `git log --oneline -5` before
working — three streams landed work tonight and two remain live. The tree
carries their in-flight state: the media-panel stream (transcript-bundle S6/S7)
and the P2 streaming stream, whose batch (the parity plan, the three
`p2-parity-*`/`extract-frozen-p2-prompt` scripts, the
`llama-cpp-serveurperso-moe-stream.toml` registry entry, and the
`hkask-private-inference` `manifest.rs`/`placement.rs` edits) is **STAGED in
the shared index** — check `git diff --cached --stat` before ANY commit.
Pushing is the operator's.

**Read first:** the worktree-root `.rules` (required), then:
`kask/docs/plans/private-inference-mcp-requirements-2026-10-30.md` (D-2, with
the 2026-11-05 store-separation amendment and the item-3 lift record),
`kask/docs/plans/private-inference-mcp-architecture-plan-2026-10-30.md` (D-3,
with the M5/item-3 amendments),
`kask/docs/plans/private-inference-m5-recall-export-filters-plan-2026-11-05.md`
(§9's implementation record — the child read side AND the landed zed write
side), `kask/docs/plans/private-inference-local-leg-llama-wrapper-plan-2026-11-04.md`
(§11/§12 — the APPROVED P2 checkpoint and its constraints), and DIVERGENCE.md's
**D100** row (the enforcement home: the gates, the panel scoping, the tool
perimeter, the store split, the M5 read+write sides, the item-3 embedding lift,
and every pin by name).

## Landed and verified (cited — do not re-derive)

- **D100 (the thread privacy mark + FR-3 routing + the panel scoping + the
  tool perimeter):** `77585247c8` + `dba74fd9ab` — the mark on
  `KaskThreadState`/`DbThread`; the storage gates; the turn-entry typed
  refusal; the request gate; the model-list scoping
  (`NativeAgentModelSelector::list_models` → `disable_non_local_models`);
  the tool perimeter (`enabled_tools` skipping `private_thread_disabled_servers`,
  re-evaluated per turn); the settings home
  (`KaskPrivateInferenceSettingsContent`, one resolution home per value).
- **FR-4 ingest marking:** `2f8a5705a3`. **The store split:** `fe9ab1f696`.
  **The M5 child read side:** `bc1170dcff`.
- **Item 1 (the zed-side `_meta` write — landed by a concurrent stream):**
  `0952bfbd67`, plus the scaffolding dedups `532d8b6442` and `c10f39bb1d` —
  `ToolCallEventStream.calling_provenance` captured at both production
  constructor sites from the calling thread's D100 mark (the D68 capture
  discipline); `KaskToolSource::invoke` carries it as an explicit host-set
  parameter (`crates/agent/src/tools/context_server_registry.rs`);
  `McpRuntime::invoke_with_call_provenance` (`kask/crates/hkask-mcp`) stamps
  `_meta.inference_provenance` under the one-home key
  `hkask_types::META_INFERENCE_PROVENANCE` (the child re-exports it); the
  plain `ToolPort::invoke` path stays un-stamped, so child→parent IPC and
  other non-thread-routed callers read public, fail-closed. The input-spoof
  falsifier is pinned (a public context whose tool input carries
  `inference_provenance` still dispatches public).
- **Item 3 (the local ollama embedding lift — landed this session):**
  `2a9acebc03` — the private pair on `RealMemoryPort`
  (`private_embedding_port` + `private_embedding_model`), the
  `build_private_embedding_port` builder (bound to the `INFERENCE_PROVIDERS`
  ollama descriptor, no credentials; refuses the empty opt-out and
  cloud-prefixed strings at wiring time), both degrade sites lifted
  (private chunks at ingest, private-context queries at recall — zero cloud
  calls), the LEG's model string on stored vectors (the KNN model gate joins
  on it), the setting `kask.private_inference.embedding_model` (default
  `ollama/qwen3-embedding:0.6b`, `DEFAULT_PRIVATE_EMBEDDING_MODEL`), the
  degrade-persist variants pinned, and the docs updated (D100 + the D-2/D-3
  amendments).
- **Item 2 (P2 streaming — APPROVED, operator-called, RUNNING in a
  concurrent stream):** the parity plan, the parity scripts, the registry
  toml, and the `hkask-private-inference` manifest/placement edits are
  in-flight (staged). **READ-ONLY over all of them.**
- **Item 4 (the private store's lifecycle):** not started; the design fork
  is open (work item 4 below).
- **A discovered, filed gap (context for the panel review):** the curator MCP
  child's tool-side query embed (`curator_semantic_search`/`consult`/
  `curator_federated_search`) is provenance-ungated — a private context's
  TOOL query still crosses to the cloud embedder via the inference IPC.
  Filed as the operator checkpoint in the D100 row; closing it changes the
  M5 positive-control shape. The panel review may surface its UX dimension
  (what a private thread's semantic-search degradation should LOOK like),
  but the routing fix is not this engagement's.

## The skills and their roles

- **`code-review`** — item 1: a landed change reviewed against its stated
  spec (the M5 plan step 1 + the D100 row). Machine checks first (re-run the
  adjudicating pins), IS/OUGHT adjudication of every doc claim, falsifiers
  for claimed invariants, file:line no-fiction.
- **`refactor-architecture`** — the privacy panel composition/interaction
  audit: friction discovery, deepening candidates, the duplication census
  over the ratchet baseline.
- **`kask-seam-audit`** — available tracks for the panel review: the security
  track over the privacy boundary (the D100 seam), and the measured GPUI
  layout track if any panel-layout finding needs measurement. Pull it where
  its tracks fit; the panel review owns the framing.
- **`kata-improvement`** — item 4: grasp the current condition, establish the
  target condition, the one experiment; the design decision, not
  implementation.

**Engagement discipline:** this is a REVIEW. The deliverables are findings and
proposals — a findings artifact at
`kask/docs/research/private-inference-review-findings-2026-11-06.md` (new file),
plus kanban tasks where a finding is actionable. Behavior changes require an
operator call; findings are filed, not fixed in this engagement.

## The work, in order

### 1. The privacy agent panel — composition and interaction patterns

The surfaces (symbols named; grep for exact paths):

- The thread-header privacy toggle (`Lock`/`LockOff` + decision text,
  `crates/agent_ui/src/conversation_view/thread_view.rs`) — the PI's only
  mark/unmark surface (D-3 §4 rules out an agent-callable unmark).
- The model-list scoping (`NativeAgentModelSelector::list_models` →
  `disable_non_local_models`) — every non-local model listed-but-disabled
  with the boundary as the reason.
- The tool perimeter (`enabled_tools` skipping
  `private_thread_disabled_servers`, re-evaluated per turn —
  `crates/agent/src/thread.rs` + `kask_thread_state.rs`).
- The settings UI (`crates/settings_ui/src/pages/kask_page.rs` +
  `kask_page/private_inference.rs` — provider/host/port, Serving, Register).
  NOTE: item 3 added `kask.private_inference.embedding_model` with NO
  settings-UI row — a composition gap candidate (the operator sets it by
  hand); adjudicate honestly whether the page should carry it.
- The mark's persistence surfaces (the D28 metadata blob, the `from_db`
  restore sanitize, the `SharedThread` round-trip).

Review axes — name the axis in every finding:

1. **State coherence:** per-turn re-evaluation vs cached lists. What goes
   stale when the PI toggles the mark mid-session (the model list, the
   enabled-tools set, the summarization slot)? Does every surface
   re-derive from the mark, or does any cache it?
2. **The mark-change-mid-turn interaction:** a turn already dispatched when
   the PI marks/unmarks — what do the perimeter and the request gate see,
   and is that interaction pinned?
3. **Degradation UX:** does every refusal name the boundary (the listed-
   refused-never-vanished contract; the typed errors' text)?
4. **Module depth:** the mark's read surface — one resolution home or
   scattered `is_private_inference()` readers? Apply the deletion test to
   any wrapper/indirection layer the seam grew.
5. **Duplication:** the panel surfaces vs the ratchet baseline
   (`kask/scripts/` jscpd conventions — mind the report-name suffix rule).
6. **Settings-page completeness vs the content struct:** every field the
   operator can set has a row? (`embedding_model` is the known candidate.)

### 2. Item 1 review (`code-review` against the M5 plan step 1 + D100)

Machine checks FIRST — re-run the adjudicating pins and cite commands +
results (cross-crate pins execute in the CONSUMING crate):

- `cargo test -p agent --lib -- calling_provenance kask_server_tool_passes`
  (the capture + dispatch pins, incl. the input-spoof falsifier)
- `cargo test -p hkask-mcp` (the wire pin
  `call_request_params_serialize_the_calling_provenance_meta` and the
  un-stamped-ToolPort default)
- `cargo test -p hkask-types -- from_private_flag the_meta_key`

Then adjudicate the doc claims against the tree — label every claim IS
(tree-verified, cite file:line) or OUGHT (spec-claim):

- The M5 plan §9's step-1 record vs the landed shape.
- D100's write-side sentences (the capture discipline, the one-home key, the
  un-stamped default, the end-to-end (b) composition claim).
- The D-3 amendment's landed list (it was updated the same night; verify its
  citations are still line-accurate — the tree shifted under three streams).
- The falsifier set: does any pin assert a vacuous observable (a field that
  exists by construction)? Is the thread→stream→source→runtime→wire chain
  pinned at EVERY link?

### 3. Item 2 review (READ-ONLY over the P2 stream's in-flight files)

Against the approved shape (local-leg plan §12): the ServeurpersoCom
`moe-stream-partition` fork, tested with the exact model, behind the same
wrapper, the parity harness with its measured band, streaming UNROUTABLE in
v1 routing until the band gates it, and the fresh-slot parity-instrument
protocol preceding any band adjudication. The measured input stands as
recorded (`kask/docs/research/private-inference-unsloth-mtp-measurement-2026-11-05.md`).

Review the in-flight artifacts against that contract:

- The parity plan's discipline: is band adjudication gated on the fresh-slot
  protocol? Is the frozen-prompt extraction sound (does
  `extract-frozen-p2-prompt.sh` match the plan's stated protocol)? Do the
  `p2-parity-*.sh` scripts measure what the band is defined over?
- The `manifest.rs`/`placement.rs` edits' interaction with the supervisor's
  verified-launch contract: the exact-commit version pin (a
  `CommitMismatch` refusal, never a silent launch), the flag probe
  (`MissingFlags` names the exact spellings), the launch record's
  digest/health verification, and the swap semantics — does a new manifest
  entry (the `moe-stream` toml) alter the shipped fleet the probe pins?
- No `*_enabled` toggles, no silent substitution at dispatch (the M6 routing
  hook's concern stands), and streaming stays unroutable in v1 — flag any
  in-flight code that would make it routable before the band gates it.

Findings filed; never edit their files while the stream is live. If the
stream has landed its work by the time you review, review the committed
state and say so.

### 4. Item 4 design review (`kata-improvement`; a design decision, not an implementation)

- **Grasp the current condition:** the consolidation pass and the forgetting
  surfaces over the PUBLIC store — `build_curator_consolidation` /
  `fire_curator_consolidation_pass` (the confidence-floor cleanup), the
  `memory_life_days` decay, and the MCP-side `curator_memory_prune` valve.
  Read them end to end; the private store currently gets none of them
  (the V1 stated limit, recorded in D100).
- **The fork:** (a) mirror the public path per-store — with THE
  LOAD-BEARING PRIVACY CONSTRAINT: the consolidation LLM synthesis over
  private content MUST route to the private leg (the local chat models),
  or the mirror leaks — assess what that requires (the synthesis/classifier
  model resolution on the private path; whether the private store's rows —
  young, embeddings only in the item-3 ollama model space — stay coherent
  under consolidation); or (b) the append-only ruling (the limit as
  positive — record it in D100; the operator decides, not the reviewer).
- **Deliverable:** the recommendation, the bounded implementation plan for
  the mirror if recommended (with the synthesis-routing leg sized), the
  explicit operator checkpoint. NO implementation.

## Boundaries

- READ-ONLY over the two live streams' in-flight files (the media-panel
  files; the P2 batch listed above). A finding over their files is filed,
  not fixed. If a stream lands its work mid-review, review the committed
  state and re-anchor file:line.
- The shared index carries the P2 stream's STAGED batch: check
  `git diff --cached --stat` before ANY commit; pathspec commits only;
  never a working-tree-wide mutation.
- This is a review engagement: findings and proposals filed (the findings
  artifact + kanban tasks); behavior changes require an operator call.
- The standing seams: no agent-callable unmark surface; the no-field rule
  (no provenance field in any model-facing schema, parent side or child
  side); streaming unroutable in v1 until the band gates it; degrade-not-leak
  wherever a leg is unwired; the store split stands.

## Known traps (beyond the worktree-root `.rules`)

- Three streams landed work tonight; line numbers shifted across commits —
  verify every file:line fresh against the tree before citing (the
  citation-rot obligation, diff-driven).
- Cross-crate pin tests execute in the CONSUMING crate — run each touched
  scope's suite, not the defining crate's alone.
- Parallel cargo invocations race on shared target-dir artifacts — run
  scopes sequentially; re-run a red scope alone before diagnosing.
- The jscpd ratchet's JSON reporter emits path SUFFIXES — resolve report
  names by unique-suffix match, never repo-root.
- edit-file corruption traps: a failing edit rolls back ALL its edits;
  never chain byte-assumed `old_text`; re-read landed anchors after
  escape-heavy regions.
- Truncated sweep output silently hides results — count matches first,
  consume the full list.

## Acceptance criteria (each can fail)

1. **Panel review:** concrete findings with file:line + the named axis
   (coherence / mark-change-mid-turn / degradation-UX / module-depth /
   duplication / settings-completeness), each falsifiable, plus ranked
   deepening candidates.
2. **Item 1:** the adjudicating pins re-run (commands + results cited);
   every doc claim adjudicated IS vs OUGHT against the tree; gaps filed
   as findings.
3. **Item 2:** read-only over the in-flight files; findings cite the
   approved shape's constraints (the band, the fresh-slot protocol, the
   unroutable-in-v1 line, the supervisor's verified-launch contract).
4. **Item 4:** the design fork's recommendation with the
   synthesis-routing constraint assessed, the bounded plan, and the
   explicit operator checkpoint — no implementation.
5. **The report:** commit hashes cited; every pin/gate re-run cited with
   commands + results; every finding's file:line fresh-verified; the
   uncommitted state (the live streams' files) reported plainly; IS vs
   OUGHT labeled throughout.
