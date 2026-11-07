# Private-inference continuation prompt — the rebuild tail (post-rebuild verification, the §8 legs, the gated studies)

**Handoff target:** any agent with repo access and no conversation history.
**Composed:** 2026-11-06 (afternoon), from the post-rebuild verification: HEAD
`c41334c455`, the binary built 13:11:36 (every commit predates it, including
the decision-model adoption), the editor restarted 16:04:01, and the running
curator child's environment carries
`HKASK_PRIVATE_EMBEDDING_MODEL=ollama/qwen3-embedding:0.6b` — the query-embed
routing's emission verified LIVE at the wiring level.

**Read first:** the worktree-root `.rules` (required), DIVERGENCE.md's D100
row (the full landed state: the query embed, the Phase-2 write side, the
classifier lift, the forgetting mirror — every pin by name),
`kask/docs/plans/private-inference-tool-query-embed-design-2026-11-06.md` (the
implemented design + §8's remaining legs), and
`kask/docs/research/private-inference-engine-compute-determinism-design-2026-11-06.md`
(the pre-spend determinism design).

## Landed and verified (cited — do not re-derive)

- The provenance-routed query embed (`54688d615b`), the Phase-2 write side
  (`89c5fbfe26`), the classifier lift (`79e8964d96`), the settings row + the
  delegated drift-watch ruling (`6416b3b4d3`) — all committed, all suites
  green, the binary current, the env emission live.
- The GUI private-model turn is the ONE unverified live piece: the private DB
  (`~/.local/share/zed-kask/agents/curator/curator-private.db`) was still at
  155,648 bytes, mtime 2026-11-05 21:08:29, at composition.

## The work, in order

1. **Verify the GUI private-model turn (the operator runs it if not yet run).**
   Mark a thread private (the header toggle), pick a local-leg model
   (`ollama/qwen3.5:4b` is deployed locally), and run one turn containing a
   `curator_memory_recall` call. Verification: the private DB's size/mtime
   grows — `stat -c '%s %y' ~/.local/share/zed-kask/agents/curator/curator-private.db`.
   If it has already moved, record the verification (the turn's chunks carry
   local classifier tags + locally-embedded passages — the D100 chain
   end-to-end); if not, the operator runs it and the next session verifies.
2. **The §8 legs (the review's extension, `d71c62d23f`).**
   (a) The research server's rerank leg — rides the already-approved
   provenance-routed mechanism (the §8 filing's recorded direction covers it):
   implement the routed-or-skip rule at the rerank site (a private-provenance
   call reranks through a local model or skips the stage with a surfaced note
   — degrade-not-leak), with the recording-port falsifier pin.
   (b) The corpus server's assisting-inference legs (`corpus_embed`,
   `corpus_tag_chunks`, the OCR vision leg on private-provenance calls) — the
   ruling is OWED: whether the M5 off-axis acceptance covers these tool legs,
   or the intake-time corpus privacy field is scheduled. Present the
   recommended ruling (the same degrade-not-leak rule as every other leg —
   private-provenance calls skip the cloud legs with surfaced reasons); the
   operator decides before any implementation.
3. **The determinism study's spend gate.** The design is filed (the
   ≤70-request, one-session, instrument-gate-first protocol); the operator's
   word gates the run — no spend without it.
4. **The drift-watch (delegated — the runbook's recorded ruling, `6416b3b4d3`).**
   The next llama.cpp release is the triage procedure's first live test — run
   it, append the triage log row (`kask/docs/reference/llama-cpp-drift-watch.md`).
5. **The MTP record §7 item 1 (the older arc's leftover).** The draft-routing
   VRAM arithmetic should reserve the spec-decode verify workspace (unsloth's
   1–2 GB guidance) — a model-brief/manifest field + the guard extension + the
   Lean proof over the reserved term.

## Boundaries

Streaming stays unroutable in v1 (three measured bars; the determinism
question precedes any reopen). No private ggml fork — ever. The store split
stands. The `InferenceProvenance` mark stays the routing signal and the
in-store belt. The no-field rule: no model-facing schema ever exposes a
provenance field. The corpus question is the §8(b) ruling, never a silent
widening. The P1 physics rulings stand (one resident flagship; launch = swap;
D-S2 exact-commit pins).

## Known traps

Beyond the worktree-root `.rules`: the fresh-slot protocol does NOT restore
within-leg determinism at reduced ctx (measure the instrument gate before any
identity claim); `cmp` under `set -e` needs `|| true`; the tree is SHARED and
busy (pathspec commits, numstat verification, disclose co-streamed content);
cross-crate pins run in the CONSUMING crate (run each touched scope's suite);
`./script/clippy` over touched kask crates before any clean claim
(check-plus-tests is not a clean claim).
