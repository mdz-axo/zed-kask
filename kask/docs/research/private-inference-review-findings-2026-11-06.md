# Private-inference review findings — the privacy panel, the M5 `_meta` write, the P2 surface, the private store's lifecycle

> **Context note, 2026-11-11.** A dated review record. Its findings on private-thread routing, the tool perimeter and private memory (the F-numbers the code cites) still describe the tree. Its findings on the kask-launched llama.cpp server, the supervisor and the settings page's endpoint rows describe code removed by operator ruling on this date. The live record is `DIVERGENCE.md` rows D9 and D100.

**Engagement:** the review continuation prompt (composed `73ada7a3fc`,
`kask/docs/plans/private-inference-review-continuation-prompt-2026-11-06.md`).
Review-only: findings and proposals filed, behavior changes reserved to the
operator. Skills by role: `code-review` (item 1), `refactor-architecture`
(the panel axes), `kata-improvement` (item 4's design fork). Machine checks
first, IS/OUGHT labels throughout, every file:line fresh-verified against the
tree during the session.

**Reviewed state.** The handoff was composed at `d6198ec50f` with two streams
live. During this review the tree moved twice: HEAD is now `7a1306ebb5`
(media-panel streams: `83d122809c`, `76b6b99907`, `d2b65de490`,
`7a1306ebb5`), and the P2 batch the handoff marked STAGED **landed** as
`631d4468b2` before this review began — item 2 was reviewed over the
committed state, per the handoff's own instruction. All D100/FR-4/M5/item-3
commits cited in the handoff verified present: `77585247c8`, `dba74fd9ab`,
`2f8a5705a3`, `fe9ab1f696`, `bc1170dcff`, `0952bfbd67`, `532d8b6442`,
`c10f39bb1d`, `2a9acebc03`.

## 0. Machine-check ledger (all re-run this session, all green)

| Scope | Command | Result |
|---|---|---|
| agent (capture + dispatch pins) | `cargo test -p agent --lib -- calling_provenance kask_server_tool_passes` | 2 passed |
| hkask-mcp (full crate; wire pin) | `cargo test -p hkask-mcp`; `cargo test -p hkask-mcp -- call_request_params_serialize` | crate green; wire pin 1 passed |
| hkask-types (the closed-set + key pins) | `cargo test -p hkask-types -- from_private_flag the_meta_key` | 2 passed |
| hkask-mcp-server (child wire pins) | `cargo test -p hkask-mcp-server -- the_call_params_shape absent_or_unknown the_exact_private_string the_scope_reaches from_private_flag` | 4 passed |
| hkask-mcp-curator (call_admits + schema) | `cargo test -p hkask-mcp-curator --test tool_behavior -- public_call_contexts_never private_call_contexts_receive semantic_search_filters no_tool_request_exposes` | 4 passed |
| agent (panel scoping pin) | `cargo test -p agent --lib -- private_thread_model_list_disables` | 1 passed |
| agent (D100 gate/perimeter pins) | `cargo test -p agent --lib -- media_server_tools_are_absent private_mark_round_trips a_marked_thread_never_loads inherit_from_carries curator_thread_marks local_provider_set_extends` | 6 passed |
| settings_content (resolution pin) | `cargo test -p settings_content -- resolved_local_providers` | 1 passed |
| agent_ui (toggle-label pin) | `cargo test -p agent_ui --lib -- private_inference_toggle` | 1 passed (blocked once by a concurrent stream's half-edit, green on retry after the tree settled) |
| hkask-private-inference (P2 pins) | `cargo test -p hkask-private-inference -- the_serveurperso_stream_surface streaming_mode_stays_unroutable` | 2 passed |
| duplication ratchet | `bash kask/scripts/check-duplication-ratchet.sh` | **RED 5613 vs 5599 baseline** — pre-existing, recorded in `631d4468b2`'s message; not attributable to the panel surfaces (below) |

## 1. The privacy agent panel — composition and interaction findings

Surfaces reviewed: the thread-header toggle
(`crates/agent_ui/src/conversation_view/thread_view.rs:6462-6520`), the
model-list scoping (`crates/agent/src/agent.rs:3288-3350`), the picker
(`crates/agent_ui/src/model_selector.rs`, `ui/model_selector_components.rs`),
the tool perimeter (`crates/agent/src/thread.rs:6375-6388`, `:6450-6455`), the
settings page (`crates/settings_ui/src/pages/kask_page/private_inference.rs`),
and the persistence surfaces (`thread.rs:2162-2187`, `:1569-1571`,
`db.rs`). Findings are falsifiable statements; every file:line verified this
session.

### F1 — `cycle_favorite_models` bypasses the disabled check; a refused pick still rewrites the global default model

**Axis:** state coherence + degradation UX. **Severity: medium.**

- `confirm` refuses disabled entries (`model_selector.rs:269-283`,
  `model_info.disabled.is_none()` at `:272`), but
  `cycle_favorite_models` (`model_selector.rs:136-188`) filters favorites
  only (`:150-154`) — no `disabled` filter — and dispatches
  (`:174-176`). The `CycleFavoriteModels` action is reachable from the prompt
  editor (`inline_prompt_editor.rs:181`) and the thread view
  (`thread_view.rs:14117`, idle-guarded there only).
- On a privacy-marked thread, cycling onto a public favorite calls
  `NativeAgentModelSelector::select_model` (`agent.rs:3352-3419`):
  `Thread::set_model` refuses silently (`thread.rs:2513-2524` — log +
  `cx.notify()`, `()`-returning, no error surfaced), yet
  `update_settings_file` runs unconditionally (`agent.rs:3400-3419`) and
  rewrites the **global agent default model** to the refused public model;
  the picker then caches the refused model as selected
  (`model_selector.rs:178`) — the header displays a public model on a
  private thread.
- **Falsifier (proposed):** on a marked thread with a public favorite,
  `CycleFavoriteModels` must leave (a) the thread's model, (b) the picker's
  displayed model, and (c) `settings.agent` unchanged. Today (c) fails.
- **Fix shape (operator call):** a one-line `disabled.is_none()` filter in the
  favorites loop, plus gating the settings write on the gate's admission
  (hoisting an admissibility check before any write).

### F2 — the mark-change-mid-turn interaction is unpinned; the perimeter's residual window and the tooltip's absolute claim

**Axis:** mark-change-mid-turn. **Severity: medium.**

- The toggle is clickable mid-turn (no status guard,
  `thread_view.rs:6477-6489`; `set_private_inference` checks no running
  state, `thread.rs:2562-2605`).
- What the gates see on a mid-turn MARK: `set_private_inference` clears the
  held public model (`:2532-2553`); the request gate re-reads
  `this.model()` per request iteration (`thread.rs:3836-3860`) — the
  in-flight request completes (its content already crossed), the next
  iteration fails typed `PrivateModelUnavailableError` (`:3816-3818`).
  Surfaced, but: **no pin covers any mid-turn flip** (the D100 pin set is
  steady-state only).
- The tool perimeter re-evaluates **per request iteration**, not per turn
  (`refresh_turn_tools` called at `thread.rs:3863-3867`, rebuilding
  `running_turn.tools` via `enabled_tools` at `:6459-6464`) — tighter than
  the D100 row's "re-evaluated per turn" (the register under-claims; both
  true). Residual window: tool calls the model emits in the same iteration
  as the flip dispatch against the pre-flip map — a marked thread's call to
  a perimeter-refused server (e.g., media's cloud-dependent passes) could
  dispatch with private provenance inside that window. A dispatch-time belt
  (`server_disabled_on_private_threads` re-check in `KaskServerTool::run`,
  `context_server_registry.rs:719+`) would close it; defense-in-depth, low
  exposure.
- The marked tooltip's "its content never reaches a public provider"
  (`thread_view.rs:6503-6505`) is false at the transition moment for a
  mid-turn mark (the in-flight request's content already crossed). Wording
  precision or an idle-guard (matching `CycleFavoriteModels`'s
  `thread_view.rs:14117` pattern) — operator's choice.

### F3 — `set_private_inference`'s doc comment cites two phantom pin names

**Axis:** module depth (comment hygiene). **Severity: low.**

`thread.rs:2556-2559` cites
`marking_a_thread_clears_a_held_public_model` and
`unmark_restores_public_model_selection`; neither exists anywhere in the tree
(repo-wide sweep: the only hits are the comment itself). The real pin is
`private_thread_refuses_a_public_model_at_the_storage_gate`
(`thread.rs:15702`, whose unmark leg is "unmark restores" — gate
permissibility, not automatic model restore). An agent grepping the cited
names would wrongly conclude the behavior is unpinned. Same class as the
"stale comments are active misinformation" rule.

### F4 — the settings page carries 3 of 6 content fields; all three missing fields are D100 PI-owned policy

**Axis:** settings completeness. **Severity: medium.**

`KaskPrivateInferenceSettingsContent` has six fields
(`settings_content.rs`, `kask_bridge/src/settings.rs:601-653`): `provider_id`,
`host`, `port` (all three have rows,
`settings_ui/.../private_inference.rs:29-63`), and `local_provider_ids`,
`private_thread_disabled_servers`, `embedding_model` — **no rows for any of
the three**. The handoff flagged `embedding_model` (item 3, set by hand
today); the two `dba74fd9ab` policy fields are the same class. Sharpening for
`embedding_model`: a cloud-prefixed or empty-handled value is refused at
wiring time (`build_rejects_empty_and_cloud_prefixed_models_but_binds_the_local_one`)
and the failure surface is a log line + the keyword-only degrade — the PI
never sees it from the page. A row with entry-time validation (the port row's
`parse`-gate pattern, `private_inference.rs:48-63`) fails at entry instead.
Honest adjudication: the page SHOULD carry `embedding_model` at minimum;
`local_provider_ids`/`private_thread_disabled_servers` are list editors —
worth it if the PI will ever change them (today: defaults serve).

### F5 — the tool perimeter vanishes media tools with no surfaced reason

**Axis:** degradation UX. **Severity: low.**

Models are listed-refused-never-vanished (`disable_non_local_models` carries
the boundary reason, `agent.rs:3298-3308`); the perimeter-refused servers'
tools simply never enter the marked thread's tool map
(`thread.rs:6385-6388`) — the model is not told and neither is the PI. A
private thread's habituated `transcribe`-class call fails as an unknown tool.
Proposal: surface the disabled-server set in the toggle's tooltip (or a
header hint) — the perimeter is a PI-owned policy; its decision text belongs
next to the lock.

### F6 — module depth verdict: the mark's surfaces pass the deletion test (no finding)

**Axis:** module depth. **Positive record.** One mark home
(`KaskThreadState`), readers concentrated (thread.rs 18, kask_thread_state.rs
3, thread_view.rs 2, agent.rs 1), production write path exactly the toggle +
`from_db` restore (`thread.rs:2172-2173`). The restore sanitize covers
`Ready` (`thread.rs:2175-2187`); the `Unresolved` case is covered by the
request gate belt (`:3810-3815`). No wrapper/indirection fails deletion.

### Duplication axis

The ratchet is RED (5613 vs 5599, pre-existing, recorded in `631d4468b2`'s
message — each arc's committed duplication is its own debt). A targeted scan
(`jscpd 5.4.0 --min-tokens 70`) over the six panel files finds **no
cross-file cluster among them** (170 clusters, all intra-file: agent.rs
104, thread.rs 48, thread_view.rs 18 — the fork legs sit inside
upstream-derived files). The panel surfaces are not the ratchet's debt.

### Ranked deepening candidates

1. **The boundary-reason one-home:** the disabled-reason string
   (`agent.rs:3301-3305`) and `PrivateRoutingError`'s text
   (`thread.rs:323-331`) are two spellings of the same boundary that can
   drift; one shared home (the `META_INFERENCE_PROVENANCE` discipline
   applied to the reason text).
2. **The select-path fail-fast ordering:** `select_model` writes three
   surfaces (thread model, thinking/effort/speed, global settings default)
   with no admission check first — F1's root. Admissibility before any write.
3. **The settings rows** (F4's surface) — three policy fields, one page.

## 2. Item 1 — the zed-side `_meta` write, reviewed against M5 plan step 1 + D100

Machine checks: §0, all green. Doc claims adjudicated:

| Claim (M5 §9 step-1 record / D100 write side) | Verdict | Ground |
|---|---|---|
| Captured at BOTH production constructor sites, D68 discipline (host-derived, never from input) | **IS** | `thread.rs:5594` (`run_tool`), `thread.rs:2052` (`replay_tool_call`) — both read `self.kask.is_private_inference()` inside the thread update |
| Un-tied test streams default `PublicProvider` | **IS** | pinned `thread.rs:11178-11249`; pin green |
| `KaskToolSource::invoke` carries it as an explicit host-set parameter | **IS** | trait `context_server_registry.rs:77-81`; read at `:719`, passed at `:734`; dispatch pin (incl. the input-spoof falsifier — the observable is the source-RECORDED provenance, not a struct field) green |
| Routed by `main.rs`'s `ZedKaskToolSource` to the provenance-stamping entry | **IS** (tree), **unpinned** | `main.rs:3667-3698`, the routing line `:3692-3693` — no test covers it (finding F7) |
| `_meta.inference_provenance` under the one-home key `META_INFERENCE_PROVENANCE`; child re-exports it | **IS** | parent `runtime.rs:1935-1982` (pin green); child re-export `call_provenance.rs:25`, parser `:54-68` |
| The plain `ToolPort::invoke` path stays un-stamped | **IS** | `runtime.rs:1501-1515` → `invoke_governed(..., None)`; the wire pin asserts actual `_meta` key ABSENCE (`:1976-1981`) — not a vacuous observable |
| FR-4 falsifier (b) tool legs compose end-to-end at the wire | **IS** | parent wire pin + child wire pin parse the same serialized form; curator `call_admits` pins both sides green (`hkask-mcp-curator` tool_behavior, 4 passed) |
| The `:NNNN@sym` citations re-anchored in skills-and-composition.md + lisp-eval-dialect.md | **IS** (fresh) | every cited line spot-checked at HEAD this session: `thread.rs:4982/5483/7923/8073/8095/3412/8174/8601/6103/2900`, `agent.rs:5501`, `lisp_eval_tool.rs:291/295/326/374/418`, `corpus storage.rs:108/243` — all land exactly |
| M5 §9: "the interim strictest-default period closed: a private-marked thread's curator tool calls now read its private rows" | **IS with caveat** | the wire + child pins compose; caveat: the child's store is the PUBLIC store (§4) — the claim holds for private-marked rows the child can see (the belt rows), and the bridge-side federated recall is the production path for the split store |

**F7 — the source→runtime link is unpinned (the one chain link).** The
thread→stream→source→runtime→wire chain is pinned at every link except
`main.rs:3693` (`ZedKaskToolSource::invoke` → `invoke_with_call_provenance`).
The trait contract is pinned via the fake source; the wire entry is pinned in
hkask-mcp; the production composition-root line is only tree-verified. The
`include_str!` source-order pin precedent exists at `main.rs:5326`
(`kask_tool_source_is_installed_before_initial_refresh`) — the same pattern
pins the routing line. Severity: low; the gap is a pinning gap, not a
behavior gap.

**Falsifier-set verdict:** no vacuous observable found in the item-1 pin set
— the wire pin asserts key absence in serialized JSON; the spoof pin asserts
the recorded dispatch provenance; the capture pin asserts all three mark
states; the schema pin asserts property lists over `schemars` output.

## 3. Item 2 — the P2 streaming surface, reviewed over the committed state (`631d4468b2`)

The handoff's staged batch landed before review; findings cite the committed
bytes at `631d4468b2`, with landings during the review noted inline (the
P2 stream committed its verdict + the compare-script fix mid-review as
`8ca8aa648a` — the F8 addendum). Constraint-by-constraint against the
approved shape (local-leg plan §12 item 2 + the P2 parity plan):

- **Band gated on the fresh-slot protocol: HOLDS.**
  `p2-parity-compare.sh` (committed) runs the instrument gate FIRST
  (`instrument_check` per leg); any within-leg divergence →
  `VERDICT instrument_confounded`, exit 4, no cross-leg comparison. The MTP
  record's honest-recording discipline is implemented, not just recorded.
- **Frozen-prompt extraction: SOUND.**
  `extract-frozen-p2-prompt.sh` extracts the P2 heredoc from the ONE home
  (`p0-bench-llama.sh`), fail-closed (exit 3) on a changed opener/closer —
  never a second copy. Validated live this session: the opener exists
  (1 hit), the extraction returns the verbatim 1814-byte prompt.
- **The scripts measure the band's defined inputs: HOLDS.** The longleg
  (`p2-parity-longleg.sh`) adds only the 1024-token leg — frozen prompt from
  the one home, temperature 0, three reps + discarded warmup, bodies +
  timings + server-log slices + RAM/VRAM samples; the compare records byte
  identity or the envelope (common-prefix depth, differing bytes, lengths,
  ctok both sides) with the divergence-rate arithmetic deferred to the lisp
  receipts (the plan's stated division). The glob set matches the p0 suite's
  actual naming (`p0-bench-llama.sh:288`,
  `${label}_${prompt_id}_${mode}_${rep}_resp.json`).
- **Streaming unroutable in v1: HOLDS and pinned.** The committed
  manifest/placement diffs are TESTS + DATA only: the streaming vocabulary
  never rides `[flags]` (the toml's provenance header records it as
  verified-but-unroutable; `--moe-stream` appears only as constraint
  (`mutually_exclusive` vs `--mlock`) and fixture data);
  `streaming_mode_stays_unroutable_in_v1` (placement) asserts a plan on the
  production manifest never renders a moe-stream spelling — re-run green.
  **The shipped fleet is untouched:** the fleet is a compile-time
  `include_str!` set over the four model briefs
  (the now-deleted local_leg/fleet.rs, lines 48-72 at the reviewed commit;
  ADDENDUM at close: the whole private-inference stack was removed later at
  `a97751ca6a`, when private threads moved to Zed's Ollama and llama.cpp
  providers); the experiment toml is referenced only from
  `manifest.rs`'s test module (`:465`). No `*_enabled` toggle, no
  dispatch-path change, no silent substitution. The supervisor's
  verified-launch contract (CommitMismatch, flag probe, digest/health)
  applies to what it launches — the mainline — unchanged by this commit.
  Both new pins (`the_serveurperso_stream_surface...` — exact-commit pin
  `7174388`/"0.3.0-dev", surface_id, both constraint classes) re-run green.

### F8 — the committed compare script decoded only `content`: a vacuous-identical hazard on this thinking model

**Severity: medium (measurement validity).** The committed
`decode_content` read `.choices[0].message.content // empty` only. On this
thinking model at 128/256/1024-token caps, `finish_reason: "length"` with
empty `content` and the generated surface in `reasoning_content` is a normal
shape — an empty-vs-empty byte comparison then reads vacuously IDENTICAL
(the exact class the vacuous-green rule warns about). **ADDENDUM at close:
this landed while the review was in flight** — the P2 stream committed
`8ca8aa648a` ("Record the P2 streaming verdict: speed miss, band unmeasured
(instrument-confounded)") carrying the fix (content AND reasoning_content
participate; empty surfaces are REFUSED via `EMPTY_REFUSED` — 4 pattern hits
in the landed script). F8 closes as fixed-on-landing; the landed verdict
itself (instrument-confounded, no band claimed) is the honest-recording
discipline applying, and is consistent with this review's concern: the
band was never adjudicated on a vacuous comparison.

### F9 — the landed compare script still has no zero-class guard: "band_measured" over an empty TSV

**Severity: low. Stands after the landing.** If a leg dir is wrong (no bodies
discovered), the gate returns green over zero classes and the band step
emits no rows — the terminal verdict still reads `VERDICT band_measured`
(no zero-class guard in the landed script either). A measured band over
nothing is a broken feedback loop. Guard: refuse when zero classes are
discovered in either dir.

### F10 — the longleg's SYS line comment claims a check the compare step does not implement

**Severity: low.** `p2-parity-longleg.sh` carries a one-line COPY of the p0
suite's SYS prompt with the comment "verified one-line copy — the compare
step refuses legs whose request bodies drift from the shared definition."
The committed compare script verifies no request bodies (it compares
completions only). The SYS copy can drift from the p0 home with no
instrument catching it — the same drift class the extraction script exists
to prevent for the P2 prompt. Either implement the request-body check in
compare, or extract the SYS line from the one home like the prompt.

### Doc note — the plan's §6 script naming drifted from the landed pair

The P2 plan §6 names `p2-parity-llama.sh` (+ a comparison step); the landed
pair is `p2-parity-longleg.sh` + `p2-parity-compare.sh` (the commit message
explains the split; the plan §6 still carries the pre-landing name at
`private-inference-p2-streaming-parity-plan-2026-11-05.md:112` even after
`8ca8aa648a`'s §9 update). Minor plan-record drift; refresh §6's harness
sentence at the next plan edit.

## 4. Item 4 — the private store's lifecycle: the design fork (kata record; NO implementation)

### 4.1 Grasp the current condition (all IS, tree-verified this session)

- **The consolidation pass is DELETION-ONLY — there is no LLM synthesis in
  it.** `MemoryConsolidator::consolidate`
  (`hkask-memory/src/consolidation_service.rs:26-81`) deletes h_mems at or
  below the confidence floor (`low_confidence_h_mems` +
  `delete_h_mem`); no inference call anywhere. `build_curator_consolidation`
  (`kask_bridge/src/memory/curator_stores.rs:320-331`) and
  `fire_curator_consolidation_pass` (`kask_bridge/src/memory.rs:509-534`)
  fire it from the consolidation timer (`:402-409`) and rebuild it at
  curator-store heal (the ingest path, `ingest.rs:215-236` — keyed on the
  PUBLIC store only; `ingest.rs:221-222` records the stated limit verbatim:
  "the private store gets no consolidation pass in v1 — a stated limit").
  Knobs: `kask.memory.consolidation_cadence_secs` + `confidence_floor`
  (`kask_bridge/src/settings.rs:893-899`; regulation defaults
  `hkask-regulation/src/set_points.rs:123-129`).
- **The decay (`memory_life_days`) ALREADY applies to the private store.**
  It is recall-time confidence scoring (the Wozniak curve, applied in every
  query leg, `hkask-memory/src/memory_store.rs:547-596`), never deletion —
  and the private store wires the same `memory_life_days` at its open
  (`kask_bridge/src/memory.rs:329-336`). The V1 stated limit names the
  DELETION passes, not the decay.
- **The prune valve is child-side and public-only.** `curator_memory_prune`
  (`hkask-mcp-curator/src/hkask_mcp_curator.rs:1414+`, its own description:
  "Deterministic, non-LLM. Distinct from confidence-based consolidation")
  runs over the MCP child's store — the public `curator.db` (one
  `CuratorDb` handle; the store-split commit `fe9ab1f696` touched only the
  bridge — the child was never given the private store). The private store
  has NO MCP-side surface at all.
- **Coherence facts for the mirror:** private rows are ordinary rows — same
  insert path, 0.5-start confidence, same decay curve; `delete_h_mem`
  self-cleans embeddings + references (pinned:
  `delete_h_mem_cleans_references_when_entity_emptied`,
  `delete_h_mem_removes_the_deleted_rows_passage_embedding_under_a_live_entity`,
  `delete_h_mem_spares_a_passage_shared_with_a_surviving_sibling`,
  `memory_store.rs:2376/2437/2498`); the item-3 KNN model gate (the LEG's
  model string on private vectors) is unaffected by deletion.

### 4.2 The load-bearing privacy constraint, re-anchored

The handoff's premise — "the consolidation LLM synthesis over private
content MUST route to the private leg, or the mirror leaks" — does not
match the current shape: **the pass has no synthesis leg to route.** All
three forgetting surfaces are inference-free. The constraint is therefore
VACUOUS for the current mirror and BINDING for the future: any
inference-carrying memory pass over private content (an LLM-synthesis
consolidation — the therapy-style contradiction resolution/condensation; a
classifier-assisted recall tagger; distillation) must route its model leg
through the local leg (the item-3 pattern: a dedicated local port + model
setting + the stored-row model-string discipline) or not ship for the
private store. That forward rule belongs in D100 regardless of the fork
decision.

### 4.3 The fork

**(a) The per-store mirror** — mechanical, cloud-free, ~3 units:
1. A private consolidation twin: `build_private_curator_consolidation`
   (second `MemoryConsolidator` over the private store) + fire both in the
   timer tick + the heal-rebuild leg. ~20-30 lines.
2. A bridge-side prune leg for the private store: reuse
   `curator_memory_prune`'s semantics (max-age + spare-recalled-within-days
   + the turn-storage-only default scope), timer-driven over the private
   store. **Bridge-side, not child-side** — extending the curator MCP child
   to the private store would end the public DB's "clean by construction"
   posture and re-open per-tool routing questions at every child surface;
   the child never opens the private DB. ~30-40 lines.
3. The knob decision: shared `kask.memory` cadence/floor/life applying to
   both stores (zero new settings) vs private-specific knobs (new settings
   rows — joins F4).

**(b) The append-only ruling** — the limit as positive: no automated
deletion ever touches private content; the PI alone decides what a private
store forgets. Cost: unbounded growth + recall-noise accumulation (decayed
low-confidence rows never cleaned). Benefit: minimal motion over private
content, explicit control.

### 4.4 Recommendation (the operator decides)

**The mirror (a), bridge-side only, with the forward local-routing rule
recorded in D100.** Grounds: the current surfaces carry no exposure to
route (deletion-only, inference-free); the mirror is mechanical; the cost
of append-only grows monotonically with private-store use; and the feared
constraint converts from an engineering requirement into a recorded rule
that binds the FUTURE inference-carrying pass. The append-only ruling
remains legitimate if the PI values minimal motion over noise control —
it is a values call, not a technical one.

**Operator checkpoint (explicit):** (1) mirror vs append-only; (2) if
mirrored — shared knobs vs private-specific rows; the prune leg's scope
defaults; (3) the D100 record either way: the V1 stated limit replaced by
the mirror record, or the append-only ruling recorded as standing + the
forward local-routing rule. **No implementation in this engagement.**

## 5. The uncommitted state at review close (plain report)

- HEAD at TRUE close is `8e355b0ffa`; the review's own artifact is this
  file (untracked — the only uncommitted change in the tree at close).
- The concurrent streams' in-flight edits named in the mid-session version
  of this section ALL LANDED while the review was in flight: the P2
  verdict + the compare-script fix (`8ca8aa648a`), and the edit_file
  matching rework (`8e355b0ffa`, the D101 seam — its half-edit in
  `streaming_fuzzy_matcher.rs` had blocked `agent`'s compile under
  agent_ui's feature graph for part of the session; the toggle pin was
  blocked on it and is green on retry after the tree settled). The staged
  index is empty. None of those files were touched by this review.
- The media-panel stream landed its review mid-session
  (`7a1306ebb5`), and the P2 experiment's verdict landed at close
  (`8ca8aa648a`: speed miss, band unmeasured — instrument-confounded;
  streaming stays unroutable, the band never adjudicated).

## 6. Acceptance-criteria mapping

1. **Panel review** — §1: F1-F6 with named axes + file:line + falsifiers;
   deepening candidates ranked. 2. **Item 1** — §0 ledger + §2 IS/OUGHT
   table; gaps filed (F7). 3. **Item 2** — §3 over the committed state,
   constraints cited; the stream's in-flight edit reported, its files
   untouched. 4. **Item 4** — §4: the re-anchored premise, the fork, the
   recommendation, the bounded plan, the operator checkpoint; no
   implementation. 5. **Report hygiene** — commit hashes cited throughout;
   every pin re-run with commands + results (§0); every file:line verified
   fresh this session; IS/OUGHT labels throughout; the uncommitted state
   reported (§5).

## 7. Disposition addendum (2026-11-06, the operator's "proceed as proposed")

The operator approved proceeding on the review's proposals; the first
wave landed the five actionable findings as five commits, and the
operator's follow-up approval (2026-11-06) closed item 4 and F5 (each
pathspec-committed, gates re-run, D100 updated in the same change where
the seam changed):

- **F7** — `cafb10b729`: the composition-root routing pin
  (`zed_kask_tool_source_routes_the_provenance_parameter_to_the_stamping_entry`,
  the include_str/D43 discipline).
- **F2+F3** — `d5eb221cc9`: the dispatch-time belt
  (`KaskServerTool::run` re-checks the perimeter for private-provenance
  calls) + the mid-turn pin (`marking_mid_turn_re_scopes_the_running_turns_tool_map`)
  + the phantom pin names fixed + the marked tooltip reworded to the
  transition-true per-request claim. The mid-turn toggle guard was
  DISPOSED as wording-not-idle-guard (the PI can still stop a running
  public turn by marking — an idle guard would prevent exactly that);
  the operator may override.
- **F1** — `de808235c0`: the cycle-favorites disabled filter + the
  `select_model` before-any-write refusal (both picker paths + the
  global-default write closed).
- **F4** — `154ee2cde6`: the three policy rows (the page complete at 6/6;
  embedding_model with entry-time prefix validation).
- **F9/F10 + the plan naming** — `ac40450c3f`: the zero-class guard + the
  SYS one-home extraction + the plan §6 refresh, verified both ways
  live (empty dirs exit 5; a real fixture bands).
- **F8** closed earlier by the P2 stream's own landing
  (`8ca8aa648a`, the reasoning_content fix + the honest
  instrument-confounded verdict).
- **F5** (2026-11-06, the operator's follow-up approval) — `d98663cc18`:
  the marked toggle's tooltip surfaces the disabled-server set
  ("Tools off on this thread: …" naming the policy field), the resolved
  set read extracted to one home (`agent::private_thread_disabled_servers`
  — the same resolution the perimeter refuses by); the pin asserts the
  surfaced set, the empty-set case, and the marked-state-only scoping.
- **Item 4** (2026-11-06, the operator's ruling on the reserved checkpoint:
  the MIRROR) — `d4311a27a4`: the private store's forgetting surfaces
  mirrored bridge-side — the consolidation TWIN (the same
  `build_curator_consolidation` home over the private store, fired on the
  one timer tick, rebuilt after the private store's heal) and the
  age-gated turn-storage forgetting leg (`fire_private_forgetting_pass`,
  the valve's fail-closed default scope, the knob
  `kask.memory.forgetting_days` shared with the child, `0` disables,
  recall-sparing within the window). Both inference-free — the mirror adds
  no cloud-exposure surface; the private store keeps no child-side
  surface; the public store gets no bridge-side prune. The FORWARD
  LOCAL-ROUTING RULE is recorded in D100: any future inference-carrying
  memory pass over private content routes its model leg through the
  local leg (the item-3 pattern) or does not ship for the private store.
  Pins: `private_consolidation_twin_prunes_low_confidence_across_the_split`
  + `private_forgetting_leg_prunes_aged_turn_storage_only`. The
  append-only alternative was not taken; the operator ruled the mirror.
- **The #1 follow-up (2026-11-06, the operator's "proceed with the #1
  follow-up item") — the tool-query-embed family's remaining legs** —
  the research showed the item's CORE (the curator child's query side)
  had already landed under a concurrent stream (`54688d615b` Phase 1,
  `89c5fbfe26` Phase 2, the design doc's status line); what remained was
  the design §8's same-class legs, and the implementation's emission
  census found a second unnamed seam at the research child. Landed as
  `6554ec0b4b`: the research rerank leg (the surfaced skip — no private
  chat model exists) + the semantic-duplication tier (routed via the
  private local model — the admission extended to the research server),
  four pins, both-sides. The corpus legs (design §8.2) were then ruled by
  the operator — shape 2, the perimeter: the corpus server joined the
  default `private_thread_disabled_servers` set (the corpus server is
  not available in private mode), pinned by
  `corpus_server_tools_are_absent_on_private_threads`. The skills' gates,
  computed: CG-2 true (0.975), the cybernetics convergence viable.
