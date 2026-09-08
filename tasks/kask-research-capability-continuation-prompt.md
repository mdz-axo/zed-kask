# Continuation: Kask research-server capability adoption (Feynman transfers)

## Operator instruction and immediate objective

You are receiving an authorized, fully specified implementation that has
not yet started. This is not an audit and not a re-planning task.

Provenance, in the operator's own words: the operator asked for a
deep-module + code-review examination of Feynman
(`advaitpaliwal/feynman` v0.3.49) compared against its website, with a
metacognition reflection on value to the research MCP server; then asked
for a plan adopting the high- and medium-value capabilities "implemented
in a zed-kask idiomatic rust and idiomatic lisp and pragmatic semantics
and pragmatic cybernetics skill style tested and gated by grill-me and
essentialist skills"; then said **"update the plan to reflect that there
is no backward compatibility requirement"**; then said **"please proceed
with plan implementation"** and asked what needed deciding on the
embedding tier; then asked how the plan aligns with the reliability plan;
then said **"please compose a continuation prompt"**.

The authoritative spec is
`kask/docs/architecture/research-server-capability-plan.md` (committed in
`a0282ad98c`, status **OUGHT** — none of it is implemented). Everything in
it — IS baseline, transfers table, feed-substrate/corpus sections, module
designs (C1/C2/C3), duplication audit, migration order, verification
gates, essentialist/grill-me results, cybernetics analysis, lisp_eval
gates, Decisions 1–7 — was produced through that skill process and the
operator's ratifications. Do not re-derive it, re-scope it, or "recover"
deferred scope (see Decision defaults below).

Ratified and binding:

- **No backward-compatibility requirement** (operator directive, recorded
  in the plan). Tool outputs are redesigned freely; renames land in the
  same commit that changes their meaning; tests are rewritten, not
  preserved. One exclusion: user data (Magna Carta P1) — the DB-filename
  rename carries a one-time migration note, never a silent orphan.
- **Implementation is authorized.** The migration is commit-sliced (one
  domain per commit).

Still open: Decision 6 (embedding tier) — a default is specified below.

## Read first

1. `kask/docs/architecture/research-server-capability-plan.md` — THE spec.
2. Project `.rules`; load the `program-manager` and `tdd` skills. The
   plan's own gates: essentialist (3-gate), grill-me (challenge pass),
   idiomatic-rust / idiomatic-lisp (module shape), pragmatic-semantics
   (IS/OUGHT labeling), pragmatic-cybernetics (loop reasoning).
3. `kask/docs/architecture/core/magna-carta.md` — IS/OUGHT discipline
   (never present intended design as live code; grep-verify enforcement
   claims) and Principle 1 (user-data sovereignty), which governs the
   DB-filename migration note.
4. `kask/docs/reference/mcp-servers/research.md` — current server
   reference; the docs commit updates it.
5. `tasks/plan.md` and `tasks/todo.md` — the reliability program's state.
   Checkpoint A operator review is pending; T04–T08 are open. Their write
   sets (curator memory, swarm session, regulation) are disjoint from
   yours, but only one editing process may edit the tree at a time.
6. Before your first edit, name one architecture doc and its constraining
   invariant (project rule).

## Worktree baseline and preservation

Root: `/home/mdz-axolotl/Clones/zed-kask`.

HEAD at handoff: `2aef59d09b` — "Document T18 and mark Phase D tasks
complete". The tree was clean before this handoff. Run
`git --no-pager --no-optional-locks status --short` and compare before
editing. Expected uncommitted artifacts of the handoff itself: the plan
doc (one factual correction to the Commit 6 wiring — see below) and this
continuation prompt.

The reliability program landed between the plan's authoring and now
(13 commits, `2475305420..2aef59d09b`), including research-server
surfaces:

- `raw_fetch.rs` +416, `feed.rs` +136: T02 transport hardening — per-hop
  redirect re-validation, validating DNS resolver, surfaced proxy
  bypass, plus a strict `discover_client` used by `rss_discover_feeds`.
  The permissive `rss_client` is DELIBERATE for user-curated feeds
  (they may live on local networks by ratified policy); do not "unify"
  the two clients — see the field's doc comment in
  `hkask_mcp_research.rs`.
- `hkask_mcp_research.rs` +21: the `discover_client` field and `run()`
  wiring only. Scoring semantics, the `require_rss_db` macro, the DDL,
  and the evidence tools are unchanged.
- `tests/tool_behavior.rs` +128; `kask_bridge/mcp_env.rs` +30 and
  `mcp_servers.rs` +20 (memory-decay emission — NOT the research rows).

Consequence: the plan's IS-baseline line citations were verified
2026-09-07 pre-landing and may be off by tens of lines. Anchor by symbol
and text (`require_rss_db`, `score_evidence_set`, `emit_research_env`,
`managed_database_layout`), and re-verify each citation before relying on
it.

Preservation: do not reset, clean, stash, or overwrite. Only one agent
process may edit at a time, including disjoint file scopes; read-only
reviews may run concurrently. If the reliability agent is actively
working T04–T08, do not start — coordinate with the operator.

## Implementation status — nothing started

Zero capability code exists. No RED or GREEN has been observed for any
capability commit. The plan document is the only artifact, and it is
OUGHT — keep every unimplemented item labeled that way, and update the
plan/reference docs under the IS/OUGHT discipline as commits land.

## Task sequence — one domain per commit, TDD

The plan's "Migration" section is authoritative. Summary plus the
non-obvious invariants:

1. **Commit 1 — C1 evidence module** (`src/research/evidence.rs`):
   component enum + const weight table (the model IS the table),
   per-component signals with basis strings, profile-substitution
   sensitivity (`NotEvaluable` surfaced, never fabricated stability),
   Tier-1 shingle syndication (4-word Jaccard ≥ 0.5, the corpus
   QA-grounding precedent), `content_clusters` + `duplication_mode` in
   the output. The flat booleans `has_published_date`/`has_content` are
   DELETED (signals subsume them). The five existing scoring tests are
   REWRITTEN — behaviors carry over, assertions target the new shape.
   Default weights retained by design choice, not compatibility.
2. **Commit 2 — C2 schema + rename sweep + begin/get**:
   `RSS_SCHEMA_DDL` → `RESEARCH_SCHEMA_DDL` with BOTH consumers updated —
   the server open in `run()` and the `hkask-storage` rotation test that
   string-splits on the const's name; a stale name there must fail
   loudly. Interface renames: `HKASK_RSS_DB` → `HKASK_RESEARCH_DB`,
   settings field `rss_db` → `research_db` (+ `Content` struct + `From`
   impl + regenerated JSON schema), `emit_research_env`, the
   `managed_database_layout` entry (single-site — passphrase rotation
   AND the maintenance inventory DERIVE from it: verify the propagated
   surfaces, do not hand-edit them), the `config_env` allowlist entry +
   `research_allowlist_matches_actual_reads` pin, `require_rss_db` →
   `require_research_db` (message names both env vars), the server
   field, the composition-test fixture, doc rows, and the default
   filename `mcp/research/rss.db` → `mcp/research/research.db`. Run
   tables (`research_runs`, `run_sources` with `excerpt` + `corpus_ref`,
   `idx_run_sources_url`). Tools `begin_research_run` +
   `get_research_run`; `run_id` parameter on `web_search`/`web_extract`
   with server-side append; a ledger-write failure surfaces in the tool
   output as a note (the T01 reliability pattern: public response
   surfaces persistence failure).
3. **Commit 3 — C2 validation gate**: `annotate_research_run` +
   `validate_research_run`. `verified` requires a server-recorded row
   (fail-closed) and a basis. Annotations are upsert-idempotent
   (PRIMARY KEY `(run_id, url)` — re-annotating after a crash is safe).
4. **Commit 4 — C3 paper identity** (`src/research/paper_id.rs`): typed
   `PaperId` enum, one parse function, canonical URL; every rejection is
   a typed error naming what was expected — no `Option`-silence.
5. **Commit 5 — C3 OpenAlex provider** (`providers/openalex.rs`): the
   provider-API client pattern (NOT the strict raw-fetch client —
   OpenAlex is a provider API, like Semantic Scholar/arXiv); registers
   unconditionally (free provider); `resolve_paper` records via `run_id`.
6. **Commit 6 — C1 embedding tier** (Decision 6 default): parameter-
   gated (`duplication: "semantic"` request parameter). Cosine ≥ 0.85
   (the `corpus_deduplicate` threshold) via the existing
   `InferencePort::embed` (`hkask-types/src/ports/inference_port.rs:296`
   — the port the server already holds for rerank; the IPC client side
   is `InferenceIpcClient::embed`,
   `hkask-inference/src/inference_ipc_client.rs:572-576`). Model
   resolution: `hkask_inference::model_constants::embedding_model()`
   (reads `HKASK_EMBEDDING_MODEL`, returns `Option<String>` — there is
   NO `DEFAULT_EMBEDDING_MODEL` constant function; unset is a legitimate
   degraded mode because the deterministic floor exists). Wiring follows
   the corpus emission precedent (`emit_corpus_embedding_env` emits
   UNCONDITIONALLY because consuming servers have no fallback for unset
   env, `kask_bridge/src/mcp_env.rs:183-194`): extend the emission to the
   research server, add the `config_env` allowlist entry, extend
   `research_allowlist_matches_actual_reads`. Degradation surfaced as
   `duplication_mode: "shingles"` with the reason — never silent.
7. **Docs commit**: `kask/docs/reference/mcp-servers/research.md`, the
   server README, `kask/docs/reference/kask-settings.md`, the
   `kask_bridge` reference rows — diffed against the plan, never
   regenerated from code (doc-update discipline).

## Execution protocol and validation

- TDD: one failing test per behavior before implementation; observe and
  record RED honestly — test code preceding the fix is not RED.
- Per commit: `cargo test --offline --locked -p hkask-mcp-research`
  (plus `-p kask_bridge` and `-p hkask-storage` where touched); then
  `./script/clippy` from the repository root. The clippy wrapper changed
  in `5fd4bac424` (development-profile default now); inspect
  `kask/scripts/build/check-build-profile.sh` and the wrapper before
  choosing runtime/jobs. Also run
  `bash kask/scripts/check-mcp-tool-tests.sh` (the MCP test-presence
  ratchet — necessary, never sufficient).
- Rename-sweep completeness gate (the plan's ra-verify): zero remaining
  hits for `HKASK_RSS_DB`, `RSS_SCHEMA_DDL`, `require_rss_db`, or
  `.rss_db` across `kask/` code, tests, allowlists, and comments (the
  kata-kanban comment mention included).
- Degradation surfacing everywhere: `permission_denied` naming env vars;
  `NotEvaluable` with reason; ledger failures as surfaced notes; embed
  unavailable as a mode string. Capability-stripped-constructor trap:
  run-tool tests must construct the server WITH the database — a
  stripped constructor may only pin degradation, never function.
- Rust discipline (`.rules`): no `unwrap()`, no `let _ =` on fallible
  ops, no panicking indexing, full variable names, `?` propagation;
  tool-input types stay schemars-compatible; tool responses follow the
  existing `execute_tool` envelope pattern.

## Operator decision defaults — act only as written

- **Decision 6 (embedding tier): implement Commit 6 parameter-gated.**
  This is the reversible superset: always-on is a default flip; defer
  means never passing the parameter (and dropping the wiring). If the
  operator answers before Commit 6, their answer supersedes.
- **Decisions 1–3 (deferrals): do NOT implement** calibration profiles,
  citation-graph expansion, or full-text resolution. They are
  consciously deferred with named re-entry triggers recorded in the
  plan; do not "recover" them as forgotten scope.
- **Decision 4 (verified gate): strict** — annotations about sources the
  server never served are `invalid_argument`.
- **Decision 5 (filename rename): ships with the one-time migration
  note** (move the file before first launch of the renamed build). The
  maintenance inventory flagging the old path afterward is correct
  behavior, not a failure.

## Honest-status requirements

Record exact validation evidence per commit (commands and outcomes). No
unsupported completion claims. Label reconstructions. Blocked
verification stays explicit. Never represent the presence ratchet as
behavior coverage. If you cannot finish a slice, leave the tree building
— gate, branch, or revert; never a half-edit.