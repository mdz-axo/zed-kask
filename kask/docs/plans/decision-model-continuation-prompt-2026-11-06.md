# Decision-model adoption — continuation prompt (2026-11-06)

For the next session picking up the decision-model / provenance-routing
seam family. Written at the operator's close-out request; the thread's
review arc (the 2026-11-06 template/private-mode/decision-model review) is
closed — this prompt carries the state, the pickup actions, and the traps.

**Parallel prompt:** the private-inference thread's
`private-inference-continuation-prompt-2026-11-06-rebuild-tail.md` (removed 2026-11-11; commit
`b34e3ec4fa`) covers the same seam family from its side — the GUI
private-model turn verification and the same design-doc §8 rulings. The
§8 checkpoint below is THE one checkpoint (not two); this prompt adds the
decision-model thread's state and the decision-point candidate routing.

## Verified starting facts (all probed live this session)

- The decision-model role is adopted and LIVE: `kask.models.decision_model`
  (default `DEFAULT_DECISION_MODEL` = `OpenRouter/typesafe/jev-1.14` —
  TypeSafe's Jev, a System One decision model), UI row + save arm pinned,
  `HKASK_DECISION_MODEL` emitted to the prediction-markets child only
  (consumer-scoping pinned), fail-visible resolution (unset is a typed
  error naming the setting, never the chat model).
- The first consumer landed: eqm's `market_score_rationale` — 12 typed
  Score questions over `InferenceMethod::Decision` (the rerank IPC
  pattern: zed-side OpenRouter key via the credential channel, the child
  never sees it; `hkask-inference/src/decisions.rs`), argmax level as the
  integer score, a private-provenance refusal at the scoring site.
- Live probe (post-restart): `model="typesafe/jev-1.14-20261014"`, all 12
  markers answered, usage reported (1789/202), scores discriminate sanely
  on a mixed rationale (the confirmatory framing flagged red). Receipt on
  card `45e4f8fd`; commits `b92124aebb` (the adoption) + `c41334c455`
  (the citation re-anchor sweep).
- The three review cards (`aff20ded`, `67824bf2`, `45e4f8fd`) are Done;
  the S15 decision-point check is live (skill-maintenance, SM-2 at 20).

## First actions (each operator-gated; nothing is in flight)

1. **The design-doc §8 rulings** — the operator's next checkpoint on
   `kask/docs/plans/private-inference-tool-query-embed-design-2026-11-06.md`
   §8: (a) the research server's rerank leg (route private-provenance
   rerank to a local model or surfaced skip — the query-embed §2 rule over
   the read side every kask server already carries), and (b) the corpus
   assisting-inference stance (extend the off-axis acceptance or schedule
   the intake-mark design). Implementation follows the landed query-embed
   precedent (`54688d615b`/`89c5fbfe26`).
2. **Decision-point candidates** — the remaining censused points (mcda
   weight/score semantic halves, the scenario/task-breakdown quality
   gates, CR-1 severity) proceed one candidate at a time through
   skill-maintenance-optimize's measured comparison when the operator
   routes one; S15 now names the setting as the routing target.
3. **Drift baseline** — the citation-drift gate is green (0 @sym
   mismatches) but carries 535 shifted-region warnings from this day's
   commits (three streams); the next drift-audit cadence pass absorbs
   them into `citation-drift-audit-hash.txt`.

## Known traps

- Jev is NOT a chat model: it answers only through OpenRouter's Decisions
  API (`POST /api/alpha/decisions` — the model page states chat SDKs will
  not work with it). Only the `score` primitive is wired; choice/noul/box
  join when a consumer needs them (`decisions.rs` refuses them typed).
- The version is PINNED (`typesafe/jev-1.14`); `~typesafe/jev-latest`
  tracks newer versions — re-verify the catalog before any bump, and
  note Jev's own docs: probabilities vary slightly between calls — set
  thresholds as bands, never exact values.
- D100's forward local-routing rule covers the decision leg: any future
  inference-carrying pass over private content routes through a local
  model or refuses surfaced — no cloud decision model on private content.
- The unpushed set at close: `c41334c455`, `b92124aebb` (this arc) +
  `6416b3b4d3`, `89c5fbfe26` (the parallel stream's) — push is the
  operator's.

## Where the durable records live

- DIVERGENCE.md: D9 (the decision-model role + pins), D100 (the private
  boundary + the landed provenance-routing legs), D102 (the skills
  version-control seam).
- The review: `kask/docs/research/template-inference-private-mode-
  decision-models-review-2026-11-06.md` (with the same-day addendum).
- The algedonic board: the ruling + the probe receipt recorded verbatim on
  card `45e4f8fd`; the doc-only and install cards closed with evidence.
