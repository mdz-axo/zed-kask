---
name: writing-style
description: "Compose or rewrite prose in a curated literary style using the live style registry — Hemingway, Woolf, Agatha Eliot, Jane Wilde, Ulysses S. Twain, plus Gentle Lovelace and Dunning technical corpora. Three modes: conversational turns, fresh composition, and recomposition of existing documents. Every generation is centroid-validated against measured style signatures, not vibes."
---

# Writing Style

Compose and rewrite prose in a curated author's voice using the live style
registry in `kask/registry/styles/`. The pipeline is mechanical: prompt →
embed → KNN exemplar retrieval from the author's actual corpus → Jinja2
system prompt from the author's cognition config → inference → centroid
validation. "In the style of Hemingway" means the output is measured against
Hemingway's embedded prose — not merely prompted toward him.

## When to Use

- A request names a style and a text: "write this in Hemingway's style",
  "explain the deployment pipeline in Agatha Eliot's voice".
- A session asks for a conversational style mode: "talk to me in Hemingway"
  (like adhd-mode — a session-scoped output style with a pre-send gate).
- An existing document should be recomposed: "take this README and
  recompose it in agatha-eliot" — read, rewrite section by section,
  validate each.
- A style choice needs a recommendation: which voice suits a release
  note, an incident post-mortem, a tutorial.

## When NOT to Use

- No style is named and no stylistic outcome is wanted — normal
  composition needs no style infrastructure.
- Dimension-specific *documentation-quality* rewrites only
  (accessibility, precision, findability, agent-correctness) — call
  `corpus_rewrite` directly with the dimension and skip the author
  configs; the quality dimensions are not literary voices.
- Style corpus maintenance (adding authors, re-embedding) — that is
  build-corpus-pipeline territory, not composition.

## The style catalog

Five literary voices (live cognition configs — the `jinja2_template`
system prompts consumed by `corpus_compose`/`corpus_rewrite` via
`config_path`) and two technical corpora (centroids and tagging only —
no cognition configs; see "Degraded modes" below).

| Style | Path (under `kask/registry/styles/`) | Voice | Best for |
|---|---|---|---|
| `hemingway` | `hemingway/hemingway-style-synthesizer.yaml` | Short declarative sentences, parataxis, subtext under plain facts | Terse clarity — release notes, status updates, anything that must cut noise |
| `woolf` | `woolf/woolf-style-synthesizer.yaml` | Interiority, stream of consciousness, flowing clauses | Reflective narrative — post-mortems, retrospectives, "what it felt like" writing |
| `agatha-eliot` | `agatha-eliot/agatha-eliot-mashup.yaml` | Eliot's moral interiority × Christie's structural precision; every fact lands as both clue and consequence | Narrative documentation — making dense material readable through story structure |
| `jane-wilde` | `jane-wilde/jane-wilde-mashup.yaml` | Austen observation × Wilde epigram; drawing-room irony with precision | Persuasive prose — announcements, critiques, anything that benefits from wit |
| `ulysses-s-twain` | `ulysses-s-twain/ulysses-s-twain-mashup.yaml` | Grant's factual precision × Twain's dry raised eyebrow | Explanations of events or history — "what actually happened and why it's funny" |
| `gentle-lovelace` | `gentle-lovelace/corpus.yaml` (no cognition config) | Four dimensions of technical documentation excellence (Gentle, Schriver, Hopper, Lovelace) | *Evaluation* of documentation against the quality composite; not a generation voice |
| `david-dunning` | `david-dunning/corpus.yaml` (no cognition config) | Academic-corpus centroid | Embedding-space analysis; not a generation voice |

**Recommendation heuristic:** terse → hemingway; reflective → woolf;
narrative documentation → agatha-eliot; wit → jane-wilde; factual
history with dry humor → ulysses-s-twain. When two fit, prefer the one
whose corpus shape matches the *document type* (prose vs. narrative vs.
report), not just the tone.

## Instructions

### Mode 1 — Conversational style turn

1. The operator names a style for the session ("talk to me in
   Hemingway"). Load the style's catalog row: its mechanics live in the
   cognition config's `jinja2_template` (`read_file` the YAML).
2. Read the config's mechanics — the syntactic rules, dialogue ratios,
   signal thresholds (e.g. hemingway's adjective_density_max 5.0,
   parataxis_ratio_min 0.6). These are the measurable signature.
3. Adopt the voice for the turn: compose the reply following the
   mechanics. A one-shot `corpus_compose` call is NOT required for every
   conversational turn — the config's rules are the prompt. But run a
   validation pass on the composed draft (Mode 3 step 3) when the turn
   is long or the operator asks for authenticity.
4. Style off: the operator says so, or the session ends. Like adhd-mode,
   the style applies to output shape, never to technical content —
   facts, code, file paths stay verbatim under any voice.

### Mode 2 — Compose a document in a style

1. Resolve the style to its cognition config path from the catalog
   table above.
2. Call `corpus_compose` with `config_path` set to the config, `author`
   set to the style name, and the `prompt` carrying the document brief.
   `db_path` and `passphrase` connect to the corpus DB for exemplar
   retrieval — use the standard corpus DB resolution (the corpus
   server's own tools do this; for a direct call pass the corpus DB
   path and passphrase from the session's credential context).
3. The tool runs the full pipeline: KNN exemplar retrieval → the
   config's system prompt → inference → centroid validation. Read the
   result: it carries the composed prose and the validation distance.
4. If the centroid distance exceeds the config's
   `centroid_distance_max`, the tool reports it — a style miss, not a
   silent pass. Surface that honestly; either accept (the operator
   decides) or re-compose.

### Mode 3 — Recompose an existing document

1. Read the source document in full (`read_file`).
2. Choose the recomposition granularity: whole-document if short
   (under ~2000 words); section-by-section otherwise — each section
   becomes one rewrite so validation is per-section and no voice drift
   accumulates across a long document.
3. For each unit call `corpus_rewrite` with `config_path` (the style's
   cognition config), `author`, the unit text as `content`, and the intended
   quality `dimension` (default `composite`). Rewrite always validates against
   `style:{author}:{dimension}:centroid`: a literary config supplies prompts
   and thresholds but does not override that target. Rewrite has no
   `no_validate` parameter. Confirm the target exists; `centroid_missing=true`
   with null distance/pass is unvalidated, not a successful check
   (`kask/mcp-servers/hkask-mcp-corpus/src/tools/compose_tools.rs:303–366`).
4. Reassemble the recomposed units, preserving the document's structure
   (headings, code blocks, tables stay verbatim — a style voice never
   rewrites code or identifiers).
5. Verify with `lisp_eval` that every heading, code fence, and table
   from the source survived: a simple count comparison of structural
   markers between source and recomposition.

### Degraded modes — surface, never fake

- **Requested centroid missing**: compose/rewrite reports
  `centroid_missing=true`, `centroid_distance=null`, `style_passed=null`.
  Report *unvalidated* prose, not an author-verified result. DB-open/read
  failures are errors, not missing-centroid fallbacks. In cognition YAML,
  `centroid_entity_ref` belongs inside `embedding`; rewrite selects its
  dimension target regardless of that configured value.
- **Technical corpora without cognition configs** (gentle-lovelace,
  david-dunning): they cannot be composed in — `corpus_compose` with
  their paths would find no `jinja2_template`. They are for evaluation
  and analysis against their centroids. Do not improvise a "style
  config" for them.

## Convergence

The skill converges per composition: centroid distance ≤ the config's
`centroid_distance_max` (or the operator explicitly accepts a miss), and
for recomposition, structural parity between source and recomposition.
There is no iteration loop — a miss either re-composes once or surfaces.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `style-recommend.j2` | Recommend a style for a document brief: weigh the catalog's voices against the brief's document type, audience, and tone constraints, with the corpus-shape heuristic. |
| `conversational-turn.j2` | Compose a single conversational turn in an adopted style: apply the style's measured mechanics (from the cognition config) to the draft reply, keeping technical content verbatim. |

To render a template, call the `render_template` tool with the template ref (e.g., `writing-style/style-recommend`) and a context object with the required variables.

Template context variables (from each template's `[inference] contract):
- `style-recommend.j2`: `document_brief`, `available_styles`
- `conversational-turn.j2`: `style_name`, `style_mechanics`, `draft_reply`, `technical_content`

## Constraints

- The pipeline is the guarantee: a style claim is validated against the
  author's embedded corpus (centroid distance), never asserted from the
  prompt alone. Unvalidated output is labeled unvalidated.
- Code blocks, identifiers, file paths, and commands are never
  restyled — they are technical content, verbatim under any voice.
- The catalog's recommendation heuristic is advisory; the operator's
  named style always wins.
- `config_path` values are exact registry paths from the catalog table —
  no improvised configs, no path guessing.
- Every `corpus_compose`/`corpus_rewrite` failure surfaces its error
  class (DB unavailable, validation failed, inference failure) — never
  collapse to a silent fallback.
- Conversational style mode shapes prose only — it never changes the
  agent's technical behavior, tool selection, or fact content.
