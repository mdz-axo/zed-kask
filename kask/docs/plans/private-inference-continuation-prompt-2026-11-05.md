# Continuation prompt — private-inference: the P2 checkpoint, the FR-4 privacy tail, and the cloud-leg gate

**Handoff target:** any agent with repo access and no conversation history.
**Composed:** 2026-11-05, from the fleet-verification + FR-4 storage-leg session (tree
verified clean at HEAD `742e5e4d8d`, running build verified chain-complete: exe
08:05:23 install, process started 17:11:56, all commits predate the install).
**Read first:** the worktree-root `.rules` (required), then the three frozen spec docs —
`kask/docs/plans/private-inference-mcp-requirements-2026-10-30.md` (D-2, §FR-4 carries
the per-surface table and the three falsifiers), `kask/docs/plans/private-inference-mcp-architecture-plan-2026-10-30.md`
(D-3, §4 module verdicts M4–M9, §7 seam placement — Seam A is the NEW D-seam D97), and
`kask/docs/plans/private-inference-local-leg-llama-wrapper-plan-2026-11-04.md` (§11/§12
carry the P2 shape and the operator open items). The architecture ruling stands: private
inference is a **core kask shared service, NOT an MCP server** (dated amendment on the
D-3 plan); the local leg is a `kask/` crate consumed by zed-side seams.

---

## Mission

Close the private-inference privacy boundary end-to-end: land the M6 thread privacy
mark with routing enforcement (Seam A), stamp private-routed content at ingest (FR-4's
marking predicate), and build the per-surface recall/export filters with their three
global falsifier tests (M5) — so that content produced on private threads never flows
to a public provider, into a non-private recall, or through a public export. The P2
streaming experiment is behind an operator checkpoint (phase 0 — present it, never
self-approve). The cloud leg stays sequenced behind this boundary (phase 5 gate).

## Verified ground truth (checked against the tree, 2026-11-05)

**Machine:** Framework 16 — Ryzen AI 9 HX 370 (12C/24T), 96 GB DDR5, RX 7700S-class
dGPU (gfx1102). No passwordless sudo; apt installs are operator-gated.

**Repo:** `/home/mdz-axolotl/Clones/zed-kask` — a SHARED tree, other streams may be
editing concurrently. Pathspec commits only; check `git diff --cached --stat` before
any commit; verify the commit numstat against your measured pre-commit diff; never a
working-tree-wide mutation; one repository, never a second clone.

**What is LIVE and running (do not rebuild or re-derive):**
- Local leg complete through P1+: wrapper crate `kask/crates/hkask-private-inference/`
  (placement compiler, manifests, acquisition, MTP draft surface — commits
  `6a1adb7699`, `332b6a538f`), L4 supervisor + startup probe (`593bfe07e2`),
- Zed-side Serving surface + live launch gate (`e51c8d96f1`):
  `kask/crates/kask_bridge/src/private_inference_supervisor.rs`,
  settings page `crates/settings_ui/src/pages/kask_page/private_inference.rs`.
- M3/M7 provider registration (`kask/crates/kask_bridge/src/private_inference.rs`):
  the fleet serves through a configured `openai_compatible` instance composed from
  the `kask.private_inference` settings section — models appear in the panel selector,
  no restart. No `*_enabled` toggles anywhere in that section (the credential's
  presence IS the toggle).
- Fleet: 4 models sha256-verified against HF LFS oids at
  `~/Documents/zk-data/private-inference/models/` (Qwen3.8-Flash-Next 4 shards, Flint,
  Gemma 4 31B, Muse Glimmer 30B — method + sizes in
  `kask/docs/research/private-inference-local-leg-p0-baseline-2026-11-04.md` §6).
  Engine at `~/.local/share/zed-kask/private-inference/engine/` (not running — the
  launch gate starts it on demand). Pin in-tree:
  `kask/registry/private-inference/llama-cpp-mainline.toml`.
- FR-4 storage leg (`a46beec519`): `hmems.inference_provenance` (TEXT NOT NULL
  DEFAULT 'public_provider', column 13 — the forgetting-migration count pin reads 13),
  `HMem.inference_provenance` + `with_inference_provenance` builder, round-trip pins,
  update carry-through pin, and `InferenceProvenance::parse_stored` on the type in
  `kask/crates/hkask-types/src/visibility.rs` (unknown stored marks read
  `PrivateInference` with a warn — privacy fails closed). **Zero writers exist: no
  row can carry `PrivateInference` until the ingest marking lands. Consumers today:
  hkask-types + hkask-storage only.**

**What is NOT landed (the work):** the M6 thread privacy mark
(`crates/agent/src/kask_thread_state.rs` has no mark), enforcement at the
model-resolution point (`crates/agent/src/thread.rs` `ensure_model`, ~:2245 at
handoff), the ingest marking, every M5 filter, and all three falsifiers.

**P2 checkpoint input (measured, not assumed):**
`kask/docs/research/private-inference-unsloth-mtp-measurement-2026-11-05.md` —
MTP paired measurement, **16.5% P2 median gain, parity unresolved**. The parity band
is exactly the open question the P2 gate exists to resolve.

## The work (phases, each gated)

0. **Operator gate — P2 checkpoint (FIRST, before any code).** Present the decision
   to the operator: approve the P2 streaming experiment shape (pinned three-tier fork —
   ServeurpersoCom `moe-stream-partition`, tested with this exact model — behind the
   same wrapper, plus a parity harness with a measured band; streaming stays
   **unroutable in v1 routing** until the band gates it — the public-retraction lesson:
   streamed-vs-resident output diverges on long generations, semantically close, not
   identical). The MTP record above is the measured input. Record the decision in the
   local-leg plan §12 (approve or defer — both are outcomes). **You cannot self-approve
   this.** If approved, the experiment is its own engagement behind the wrapper —
   never in the v1 routing path.
1. **M6 — thread privacy mark + routing enforcement (Seam A, zed-side).** The mark on
   `KaskThreadState` (`crates/agent/src/kask_thread_state.rs`, the D2/D25 fork-owned
   per-thread home), persisted via the thread metadata (D28 per-message storage,
   `crates/agent/src/db.rs`); enforcement at `Thread::ensure_model` / `set_model`: a
   marked thread's resolved model must be a private-leg model (the policy allowlist is
   the landed `kask.private_inference` routing section — read it, do not invent a new
   policy home); unavailability fails **typed, never falls back**. The unmark surface:
   thread-header toggle in `crates/agent_ui`. **Seam A is the new D-seam D97** — the
   DIVERGENCE.md entry and pinning tests land in the SAME change (every `// zed-kask:`
   comment disabling upstream behavior needs a test pinning the disabled behavior).
   **Curator sessions mark and route identically to native agent threads** (PI ruling
   2026-10-30 — the stated V1 use is the PI through the Curator).
2. **Ingest marking (FR-4's marking predicate).** Content produced by threads routed
   to private inference is stamped `PrivateInference` at ingest: the curator ingest
   seams (`kask_bridge`'s `RealMemoryPort::ingest_turn` and the curator tools) read the
   producing thread's routing state (the M6 mark / the resolved private-leg model) and
   set the field via the storage builder. **Only ingest paths holding the thread's
   routing state stamp the mark — never a model-facing surface** (a mark claim from a
   public thread is the P11 violation). Public-thread content stays `PublicProvider`
   (the default) — pin both directions.
3. **M5 — per-surface filters (FR-4's table, D-2 §FR-4; each surface earns its
   deletion test independently).** The corpus/media/kanban records do NOT carry the
   mark yet — each store gains its own field + migration + filter following the landed
   hmems house pattern (one schema home, idempotent `ALTER TABLE` migration, round-trip
   pins, fail-closed decode consumed from `hkask-types::InferenceProvenance` — one
   parse home, never per-store re-implementations):
   - `hkask-memory` curator recall: `curator_federated_search`,
     `curator_semantic_search`, `curator_memory_recall`, and the bridge's
     curator-scoped context injector (`BridgeCuratorContextInjector` →
     `RealMemoryPort::recall_context_curator` — grep the exact seam, the D-2 FR-4
     oracle names it) exclude private-marked rows from **non-private recall
     contexts**; a private-marked Curator thread recalls its own rows (the PI's
     stated use — the filter must not over-block it).
   - `hkask-mcp-corpus`: `corpus_query` filter; `corpus_export_bundle` and
     `corpus_ingest_qa` outputs refuse private-marked corpora.
   - `hkask-mcp-media`: `educt_search_transcripts` / `layers_search` filters;
     `educt_export` (`corpus_text`) refuses private transcripts.
   - `hkask-mcp-kata-kanban`: the `curator:goal:<id>` recall row and goal index
     exclude private goals from non-private recall; goal history exports refuse them.
   The recall filters key on a **context signal only the M6 mark provides** — the
   dependency is real: do not build the filters against an invented context flag.
4. **The three global falsifier tests (cross-crate, the acceptance core):**
   (a) a marked thread's model resolution can never be public (joins FR-3);
   (b) a public thread **or public Curator thread** recalling a private-marked row
   fails the test, while a private-marked Curator thread recalls its rows;
   (c) an export tool emitting a private-marked row fails the test. Each surface's
   test constructs the store **with the capability under test** (never a
   capability-stripped constructor); degradation asserts the SURFACED reason, never
   `count == 0`.
5. **Cloud leg — GATED, do not start.** Sequenced behind FR-4/M6/M7 completing: the
   cloud leg is "private by design" only because the marking and routing hold the
   boundary (the D-4 memo: single tenancy + TLS + PI-held keys + allowlisted surface —
   no TEE exists at the providers). When this gate opens, the engagement is the D-3
   module set: M1 server (intake → approval → provision → deploy → verify → connect →
   teardown), M2's three provider clients + mock port (mock-port-first — no provider
   keys needed to build), M8 SQLCipher store, M9 model briefs. Do not begin it in this
   continuation unless the operator explicitly re-orders.

## Boundaries

No private ggml fork — ever. Streaming stays P2/unroutable in v1 routing until the
band gates it. Do not rebuild the engine or re-derive the fleet verification (§6 is
the record). Do not re-introduce exact-match `hmems` admission checks in federated
recall (the R5 incident — admission subsets `embeddings` only). Do not touch upstream
zed files outside the D97 seam + its DIVERGENCE.md entry. Do not reinstall or replace
ollama. Do not invent a new settings section or policy home — `kask.private_inference`
is landed; extend it, never duplicate it. Do not invent a goal `source` — if the
operator confirms this arc as a goal, intake through `kanban_goal_create` with THAT
session's real interaction/session IDs.

## Known traps (beyond the worktree-root `.rules`)

- The edit-file tool over escape-heavy regions (regexes, `\"` string literals) lands
  corrupt bytes while reporting success — re-read the landed anchor before chaining
  edits, and byte-verify after any edit over such a region (hit live in the FR-4
  storage slice; caught by compile, fixed by sed).
- Closed-vocabulary MCP string fields: the research server normalizes quoted/padded
  values; other servers may not — check which server you are calling before burning
  retries.
- MCP tool responses are `{"content": <value>}` envelopes — use `unwrap_tool_envelope`.
- Cross-crate pin tests execute in the CONSUMING crate — `cargo test -p <server>`
  never runs the consumer's pins; run each touched scope.
- `cargo test` and `cargo check` in one shell, sequential — parallel cargo invocations
  race on shared target-dir artifacts (phantom compile errors).
- Kanban state moves for the same task race inside one parallel tool batch — one
  row-mutating call per task per batch.
- The `InferenceProvenance` fail direction is fail-CLOSED (unknown → PrivateInference)
  — the deliberate mirror of the extraction `Provenance`'s fail-safe-down; do not
  "fix" either direction.

## Acceptance criteria (each can fail)

1. P2: the operator's checkpoint decision is recorded in local-leg plan §12; if
   approved, the fork experiment runs behind the wrapper with the parity harness and
   streaming remains unroutable in v1 routing; if deferred, zero P2 code landed.
2. M6: a marked thread resolving to a public model fails typed (falsifier (a) green);
   the mark persists across restart (D28 metadata round-trip pinned); the unmark
   surface exists; Curator threads mark and route identically (pinned).
3. Ingest: a private-routed thread's content stores `PrivateInference` (pinned at the
   ingest seam); public-thread content stays `PublicProvider` (pinned); no
   model-facing surface can set the mark (pinned refusal).
4. M5: every surface of the FR-4 table has its filter + export block with its own
   test; the three global falsifiers exist cross-crate and each can genuinely fail.
5. Gates: `cargo test -p` green per touched crate; `./script/clippy -p <touched>` green;
   dependent census `cargo check` green; every zed-side edit carries its DIVERGENCE.md
   seam entry + pinning test in the same change; commits are pathspec-scoped with
   numstat verified against the measured diff.
6. The report states net lines, the gates run (with commands), and any uncommitted
   state plainly — a claim without its oracle re-run is a fabricated attestation.
