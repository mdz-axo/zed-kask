# Private-inference continuation prompt — the zed-side `_meta` leg, the approved P2 engagement, the local-embedding prerequisite

**Handoff target:** any agent with repo access and no conversation history.
**Composed:** 2026-11-05 (evening), from the store-split close-out session. The
tree was clean at `fe9ab1f696` at composition; the running build was verified
chain-complete (every commit predates the exe install and the process start),
and the private curator store was verified LIVE on disk
(`~/.local/share/zed-kask/agents/curator/curator-private.db`, constructed on
restart and receiving private-marked turns). Four commits were unpushed at
composition (`bc1170dcff`, `dba74fd9ab`, `c2bdaf0c5a`, `fe9ab1f696`) — pushing
is the operator's.

**Read first:** the worktree-root `.rules` (required), then:
`kask/docs/plans/private-inference-mcp-requirements-2026-10-30.md` (D-2, with
the 2026-11-05 store-separation amendment),
`kask/docs/plans/private-inference-mcp-architecture-plan-2026-10-30.md` (D-3,
with the 2026-11-05 M5 amendment),
`kask/docs/plans/private-inference-m5-recall-export-filters-plan-2026-11-05.md`
(the M5 plan — §3's M5-A design, §6's step table, §9's implementation record),
`kask/docs/plans/private-inference-local-leg-llama-wrapper-plan-2026-11-04.md`
(§11/§12 — the P2 checkpoint is **APPROVED 2026-11-05**), and DIVERGENCE.md's
**D100** row (the enforcement home: the gates, the panel scoping, the tool
perimeter, the store split, and every pin by name).

## Landed and verified (cited — do not re-derive)

- **D100 (the thread privacy mark + FR-3 routing):** `77585247c8` +
  `dba74fd9ab` — the mark on `KaskThreadState`/`DbThread`; the storage gates
  (`set_model`, `set_summarization_model`, the `from_db` sanitize); the
  turn-entry typed refusal; the request gate; the panel scoping (non-local
  models disabled with the boundary reason); the tool perimeter (the media
  server disabled on marked threads at `enabled_tools`); the local-leg set
  (`kask.private_inference.local_provider_ids`, default ollama +
  `private_thread_disabled_servers`, default media; one resolution home in
  `settings_content`).
- **FR-4 ingest marking:** `2f8a5705a3` — the mark rides `TurnRecord` from
  the producing thread's routing state; `memory_insert` derives from the
  cited evidence; distillation taints private batches.
- **The recall seam + the store split:** `8d33ff249f` + `fe9ab1f696` — the
  injector carries the thread's mark; `curator-private.db` receives private
  turns (write routing by provenance); private contexts federate BOTH
  stores, public contexts never touch the private one; per-store
  connectedness; **degrade-not-leak both directions** — private chunks and
  private queries never cross to the configured cloud
  embedder/classifier (keyword-only, surfaced).
- **The M5 child side:** `bc1170dcff` —
  `kask/crates/hkask-mcp-server/src/server/call_provenance.rs` (the
  per-call provenance task-local + `ProvenanceScopedService`, wired once in
  `run_stdio_server`; ONLY the exact closed-set private string claims
  private; absent/unparsable reads PUBLIC) + the curator tools'
  `call_admits` filters with surfaced exclusion counts. Verified live in
  tree and pinned.
- **The P2 approval:** `c2bdaf0c5a` — recorded in the local-leg plan §12.

## The work, in order

1. **The zed-side `_meta` ride (M5 plan step 1 — the next leg).** The
   child side reads the per-call provenance from
   `CallToolRequestParams._meta.inference_provenance` (SEP-1319) and
   currently reads PUBLIC for every call — the strictest interim. The
   missing piece: the zed-side dispatch stamps `_meta` from the CALLING
   THREAD's D100 mark before curator-server calls. Exploration notes from
   the close-out session: `Thread::run_tool` holds
   `self.kask.is_private_inference()`; the rmcp request construction lives
   in `crates/context_server` (the MCP client); MCP tools are
   registry-shared across threads with different marks, so the mark must
   ride the specific call — candidate mechanisms: a call-scoped task-local
   around the dispatch, or a per-turn wrapper in `Thread::enabled_tools`
   (already D100-aware — the tool perimeter's site; a wrapper there could
   carry the mark into the tool handle). Upstream surface discipline: any
   `crates/context_server` edit is a NEW D-seam — the DIVERGENCE.md entry
   and pins land in the same change; prefer a kask-side wrapper if the
   seam allows. The child-side pins already exist
   (`the_call_params_shape_parses_from_wire_json`,
   `absent_or_unknown_request_marks_read_public`,
   `no_tool_request_exposes_a_provenance_field` — the no-field rule);
   the new work pins the parent-side write: a request fixture asserts the
   `_meta` value, and the end-to-end legs — a private-marked thread's
   tool call returns its private rows (the child-side positive control
   `private_call_contexts_receive_their_private_rows` becomes
   live-routable), a public thread's call never does. FR-4 falsifier (b)'s
   tool leg then passes both directions end-to-end.
2. **The P2 streaming experiment (APPROVED — its own engagement,
   operator-called; do not start without the call).** The approved shape
   (local-leg plan §12): the ServeurpersoCom `moe-stream-partition` fork,
   tested with this exact model, behind the same wrapper, with the parity
   harness and its measured band. Streaming stays UNROUTABLE in v1 routing
   until the band gates it. The parity-instrument follow-up (a fresh-slot
   protocol) precedes any band adjudication. The measured input stands as
   recorded (`kask/docs/research/private-inference-unsloth-mtp-measurement-2026-11-05.md`).
3. **The local embedding/classification model for the private leg (the
   prerequisite that lifts the degrade).** A fleet/registry engagement: an
   embedding model brief (llama.cpp serves embeddings), the wrapper's
   embedding surface, and the bridge's private-path embedding routed to
   the local model. The degrade sites name their lift points — grep
   "a local embedding model lifts the degrade" in
   `kask/crates/kask_bridge/src/memory.rs` and `memory/ingest.rs`.
   Acceptance: private turns embed locally (zero cloud embed calls,
   semantic recall live for private content) and the degrade pins flip
   to positive (`private_chunks_never_reach_the_configured_embedder`,
   `private_context_recall_never_embeds_the_query` become local-embed
   variants).
4. **The private store's consolidation/forgetting pass (the V1 stated
   limit).** The private store currently has no consolidation or
   forgetting. Mirror the public path per-store
   (`fire_curator_consolidation_pass` over the private store; the
   forgetting valve scoped per-store) or record the ruling to keep
   private memory append-only.

## Boundaries

No private ggml fork — ever. Streaming stays unroutable in v1 routing
until the parity band gates it. Do not re-derive the fleet verification
(the baseline record §6 and the MTP record are the evidence). The
no-field rule: no model-facing request schema ever exposes a provenance
field (pinned at the SCHEMA surface — keep it true for any new surface).
Degrade-not-leak until item 3 lands: private content never crosses to the
configured cloud embedder/classifier, surfaced, never silent. The store
split stands — do not un-split; the `InferenceProvenance` mark remains the
routing signal and the in-store belt. The media server stays disabled on
private threads until its inference can run locally (the operator's
`private_thread_disabled_servers` set is PI-owned). The corpus/media/goal
off-axis rulings stand (M5 plan §4, PI-approved stated limits).

## Known traps (beyond the worktree-root `.rules`)

- The edit-file tool over escape-heavy regions lands corrupt bytes while
  reporting success — re-read the landed anchor and byte-verify after any
  such edit (hit twice in this arc: a dropped `///` prefix block, a quote
  typo; both caught at compile/verify).
- One `edit_file` call with a failing edit rolls back ALL its edits —
  resend the whole call; and a successful call can land byte arrangements
  that differ from `new_text` — never chain byte-assumed `old_text`.
- Parallel cargo invocations race on shared target-dir artifacts
  (phantom compile errors) — run scopes sequentially, and re-run a scope
  alone before diagnosing a compile error seen only under parallel cargo.
- Cross-crate pin tests execute in the CONSUMING crate — run each touched
  scope's suite.
- The tree is SHARED: pathspec commits only, check
  `git diff --cached --stat` first, verify the commit numstat against the
  measured pre-commit diff, never a working-tree-wide mutation.
- The duplication-ratchet gate is red on main (5613 vs the 5599 baseline at
  handoff): every arc's committed duplication is its own debt — additions
  that survive as accepted duplication bump
  `kask/scripts/duplication-ratchet-baseline.txt` in the SAME commit with
  the justification (the gate's doctrine); dedupable scaffolding
  consolidates instead (the adjudicated precedents: `532d8b6442` and
  `c10f39bb1d`, nine clusters, zero baseline change). The gate has a ±2-4
  same-tree noise floor (calibration task `30796b8c` on the Ongoing Work
  board) — re-run before believing a ±1 breach.
- MCP tool responses are `{"content": <value>}` envelopes — use
  `unwrap_tool_envelope`.

## Acceptance criteria (each can fail)

1. Item 1: the `_meta` stamp reaches curator-server calls parent-side
   only (the request fixture pins it); the end-to-end falsifier (b) tool
   legs pass both directions; every zed-side edit carries its
   DIVERGENCE.md seam entry + pinning test in the same change; `cargo
   test -p` green per touched crate; `./script/clippy` green; the commit
   numstats verified.
2. Item 2 (if called): the experiment runs behind the wrapper only;
   streaming remains unroutable in v1 routing; the parity harness
   adjudicates against the measured band with the fresh-slot protocol
   first.
3. Item 3: private turns embed via the local model only (pinned);
   semantic recall for private content live (pinned); the degrade warns
   are gone (pinned).
4. Item 4: the private store's lifecycle policy is implemented per-store
   or explicitly ruled append-only in the D100 register row.
5. The report cites commit hashes, net lines, the gates run (with
   commands), and any uncommitted state plainly.
