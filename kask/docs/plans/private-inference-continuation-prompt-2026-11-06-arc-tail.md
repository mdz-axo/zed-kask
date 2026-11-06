# Private-inference continuation prompt — the arc tail (post-P2, post-embedding-lift)

**Handoff target:** any agent with repo access and no conversation history.
**Composed:** 2026-11-06, from the P2 close-out + the tree's current state (HEAD
`af611efd9b` at composition; the running editor binary verified current — the
operator rebuilt and restarted at 08:45/08:50, postdating every commit below).
The private-inference arc's major legs have all landed; this prompt carries the
remaining seams, not the settled work.

**Read first:** the worktree-root `.rules` (required), then DIVERGENCE.md's
**D100** row (the full arc: the routing gates, the store split, the embedding
lift, the M5 read+write sides, the review's F1/F2 legs — every pin by name),
`kask/docs/research/private-inference-p2-streaming-parity-measurement-2026-11-05.md`
(the P2 verdict), and
`kask/docs/research/private-inference-unsloth-mtp-measurement-2026-11-05.md`
(the MTP record, whose follow-ups §7 names).

## Landed and verified (cited — do not re-derive)

- **M5 write side (complete, end-to-end at the wire):** the stream capture +
  the `KaskToolSource` parameter + the runtime `_meta` stamp + the child read
  side — committed `9c0374f6c6` + `0952bfbd67`, with the later
  composition-root pin `zed_kask_tool_source_routes_the_provenance_parameter_to_the_stamping_entry`.
- **The store split + FR-4 chain:** private store, ingest stamping, injector
  federation, per-surface filters (D100 row).
- **The embedding half of the degrade — LIFTED (a concurrent stream, recorded
  in D100):** `build_private_embedding_port` (the local ollama descriptor,
  loopback `/v1/embeddings`, no credentials), the setting
  `kask.private_inference.embedding_model` (default `ollama/qwen3-embedding:0.6b`
  — **deployed locally, 639 MB, verified present at composition**), the
  stored-vector model-string gate, the no-overlap-query falsifier, and the
  degrade-persists-unwired pins. Private chunks and private-context queries
  embed locally; the cloud embedder never sees them.
- **The review's F1/F2 legs (2026-11-06):** the picker's dispatch-leg refusals
  (favorites cycling + select-before-write), the mid-turn re-scope, and the
  kask-tool dispatch-time belt.
- **P2 streaming — adjudicated, CLOSED:** the pinned ServeurpersoCom fork
  (tip `7174388`) is a speed miss (9.02 vs 13.72 tok/s) with the band
  unmeasured (instrument-confounded); streaming stays unroutable on three
  independent bars (the research record above). The fork surface stays pinned
  as the experiment record; the harness scripts live in `kask/scripts/`.

## The work, in order

1. **The curator tool-side query embed for private contexts — the one
   remaining privacy seam (D100's named follow-up).**
   `curator_semantic_search` / `curator_consult` / `curator_federated_search`
   embed every query via the inference IPC (the configured cloud embedder)
   regardless of call provenance — a private context's TOOL query still
   crosses to the cloud. The M5 `_meta` already carries the calling context
   to the child; the design is the child-side mirror of the item-3 lift: route
   the query embed by the call's provenance (private → the local embedding
   port, public → the configured cloud). **Operator checkpoint, named in
   D100:** closing this changes the M5 positive-control shape — approve the
   design before implementing (`private_call_contexts_receive_their_private_rows`
   and its siblings re-shape when the query embed is provenance-routed).
2. **The local classifier leg (the classifier half of the degrade).** Private
   chunks ingest with structural tags only — the configured classifier is a
   cloud model. The same local-port pattern (a small classify model on the
   local provider) lifts it; the model choice is an operator checkpoint.
3. **The private store's lifecycle (the V1 stated limit).** No
   consolidation/forgetting pass runs over the private store today. Either
   mirror the public path per-store (`fire_curator_consolidation_pass` over
   the private store; the forgetting valve scoped per-store) or record the
   explicit append-only ruling in the D100 row.
4. **The two P2-era `.rules` proposals (adjudicate).** Filed in the MTP
   record §7, never landed: the shipped-lib CWD-relative-path trap (a probe
   resolving a lib relative to CWD rather than the binary — the P1 build's
   lesson) and the cross-instrument comparison trap (comparing instruments
   that observe different observables — the D-E/D-F lessons). Apply the
   three admission gates; land accepted ones in a rules-only commit and
   re-verify the `.rules:NN` cites (currently `.rules:107`/`.rules:171`).
5. **The bench repairs (post-experiment, small).** The `usage_of` parse for
   the fork engines' TSV token columns (MTP D-E) and the first-content-line
   TTFT timer (D-F) in `kask/scripts/p0-bench-llama.sh` — the frozen
   instrument may now be patched WITH the deviation recorded (the
   mid-experiment freeze is over).
6. **The engine-compute determinism question (research, no spend without a
   design).** The P2 record's follow-up: why do 256-token completions flip
   knife-edge tokens between temp-0 reps at reduced ctx while 1024-token
   completions on the same engine can be byte-stable? This precedes ANY
   future identity-instrument trust (both the MTP and P2 records were
   instrument-confounded on it).

## Boundaries

Streaming stays **unroutable in v1** — three measured bars; only a new
measured band on hardware that can host a meaningful expert cache (the M4
Max class) reopens it, and only after the determinism question is answered.
No private ggml fork — ever. The store split stands — do not un-split. The
`InferenceProvenance` mark stays the routing signal and the in-store belt.
The no-field rule: no model-facing schema ever exposes a provenance field.
The classifier degrade persists (surfaced, structural tags only) until item
2 lands — degrade-not-leak, never silent widening to the cloud. The
corpus/media/goal off-axis rulings stand (M5 plan §4). The P1 physics
rulings stand (one resident flagship; launch = swap; D-S2 exact-commit pins).

## Known traps (beyond the worktree-root `.rules`)

- The P2 harness lessons, now pinned in the harness itself: a quoted glob
  silently matches nothing (the class-discovery bug); a thinking model emits
  into `reasoning_content` first — an instrument that observes only
  `content` compares empty-vs-empty and reports vacuous identity; refuse
  empty-side comparisons.
- The fresh-slot protocol does NOT restore within-leg determinism at reduced
  ctx on these engines — never assume the MTP record's D-G fix transfers;
  measure the instrument gate before any identity claim.
- `cmp`'s exit code under `set -e`: guard with `|| true` and read the output.
- The tree is SHARED and busy (multiple streams; observed: work swept into
  other streams' commits mid-flight — pathspec commits, verify numstats,
  commit finished work immediately, disclose co-streamed content).
- Cross-crate pin tests execute in the CONSUMING crate — run each touched
  scope's suite; `./script/clippy` over touched kask crates before any
  clean claim (check-plus-tests is not a clean claim).

## Operator checkpoints

1. **The GUI turn + the live `_meta` verification (may already be done).**
   The rebuilt binary carries the M5 write side; one private-marked Curator
   thread running `curator_memory_recall` now carries its private rows
   end-to-end. The restart's native-first telemetry showed one substantive
   turn — confirm whether that WAS the private-model turn; if not, run it
   (Settings > Kask > Private Inference → launch → pick the private model in
   the panel's picker → run one turn).
2. **The tool-side query embed design approval** (item 1 — it changes the
   M5 positive-control shape).
3. **The drift-watch ownership ruling** (wrapper plan §12 item 3:
   operator-triaged per release, or delegated with typed escalation — the
   runbook at `kask/docs/reference/llama-cpp-drift-watch.md` serves either).
4. **The classifier model choice** (item 2, if pursued).

## Acceptance criteria (each can fail)

1. Item 1: a private-marked context's `curator_semantic_search` query embeds
   via the local port ONLY (zero cloud embed calls, pinned); a public
   context's unchanged (pinned); the reshaped M5 positive control passes
   under the operator-approved design.
2. Item 2: private chunks ingest with classifier tags from the local model
   (pinned); the classifier degrade warns are gone (pinned).
3. Item 3: the lifecycle policy implemented per-store AND pinned, or the
   append-only ruling recorded in the D100 row — one of the two, explicitly.
4. Item 4: each proposal adjudicated with the three gates named; accepted
   rules landed rules-only with the `.rules:NN` cites re-verified.
5. Items 5–6: the bench repairs pinned (the frozen suite's re-freeze
   recorded); the determinism question has a design before any spend.
6. The report cites commit hashes, net lines, the gates run (with
   commands), and any uncommitted state plainly.
