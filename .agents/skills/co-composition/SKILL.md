---
name: co-composition
description: Human-in-the-loop co-composition for narrative fiction and expository prose, anchored in GOLEM v1.1 post-draft story/document state, Flower & Hayes recursive writing processes, Mann & Thompson RST, Genette narratology, Phelan character dimensions, Williams cohesion, and Leech & Short stylistics. Preserves authorial agency with human-preserved Markdown state lines, deterministic lisp_eval epistemic-leak and line-preservation gates, and optional delegation to writing-style.
---

# Co-Composition

Human-in-the-loop co-composition and iterative revision across narrative
fiction and expository/rhetorical writing. Rather than autonomous one-shot
generation or rigid pre-drafting ontology slots, this skill separates
voice-first human drafting from **(1) non-constraining structural planning**,
**(2) post-draft GOLEM v1.1 Markdown/YAML state extraction** (preserving all
human-authored lines verbatim), and **(3) decoupled craft review** with
deterministic continuity checks via `lisp_eval`.

## Reference models and ontological anchors

Grounded in primary sources resolved under research run `94248e2666131fad`
and `hkask-bridge-ontology` derived rulings (2026-10-13 and 2026-11-18):

- **GOLEM Ontology for Narrative and Fiction v1.1** (Pianzola et al., 2024,
  `doi:10.5281/zenodo.14911396`; *Humanities* 14(10):193, 2025,
  `doi:10.3390/h14100193`): `derived:narrative`, `gc:G1_Character`,
  `gc:G4_Social_Relationship`, `gc:G7_Narrative_Sequence` (fabula vs.
  syuzhet), `gc:G9_Narrative_Unit`, `gc:G12_Setting`,
  `gc:G13_Narrative_Location`, `gc:G16_Object`. Used **post-draft** for
  story/document state and continuity checks, never as a drafting slot-filler.
- **Derived domain anchors (`derived.rs`, operator ruling 2026-11-18):**
  `literary_composition` (disambiguated from MovieLabs `omc:Composition`),
  `narratology`, `storytelling`, `rhetoric`, `prose`, `plot`.
- **Composition & Rhetoric:** Flower & Hayes (1981,
  `doi:10.58680/ccc198115885`) recursive Planning / Translating / Reviewing;
  Mann & Thompson (1988, `doi:10.1515/text.1.1988.8.3.243`) Rhetorical
  Structure Theory; Swales (1990) Genre Analysis.
- **Prose Craft:** Leech & Short (2007/2013,
  `doi:10.4324/9781315835525-17`) *Style in Fiction* (mind style, speech/thought
  presentation); Williams (1990) *Style: Toward Clarity and Grace*
  (topic/stress cohesion).
- **Character & Narratology:** Phelan (1987, summarized in Jannidis, 2012)
  mimetic/thematic/synthetic triad and Phelan (1989,
  `doi:10.5860/choice.27-0782`) character progression; Genette (1980,
  `doi:10.2307/1770890`) *Narrative Discourse* (Order, Duration, Frequency,
  Mood/Focalization, Voice); Greimas Actantial Model.
- **Empirical Co-Writing Guardrails:** Mirowski et al. (CHI 2023,
  `doi:10.1145/3544548.3581225`) on top-down formulaic drift; Agarwal, Naaman,
  & Vashistha (CHI 2025, `doi:10.1145/3706598.3713564`) on Western stylistic
  homogenization; Chakrabarty, Laban, & Wu (CHI 2025,
  `doi:10.1145/3706598.3713559`) on mitigating AI idiosyncrasies through
  iterative human-directed edits; Motif (CodeByAshton, 2026) post-chapter
  human-line-preserving Markdown sync pattern.

## Initial and target condition

- **Initial condition:** A human-authored brief, outline, or draft
  chapter/section, its composition `mode` (`fiction` or `expository`), and an
  optional Markdown/YAML codex or state folder.
- **Target condition:** Zero deterministic epistemic-leak findings (`CC-1`),
  100% verbatim preservation of human-authored codex lines (`CC-2`), and
  explicit human acceptance of any local craft edits (`CC-3` converges or
  stops cleanly at `iteration = 3`).

## When to Use

- Co-composing multi-chapter fiction or multi-section essays/reports where
  long-range entity, timeline, or argumentative continuity matters.
- Extracting post-draft GOLEM-aligned state (characters, locations,
  relationships, epistemic knowledge) into human-editable Markdown without
  overwriting human notes.
- Running a decoupled craft review grounded in published composition,
  stylistics, character, and narratology models.

## When NOT to Use

- Pure curated author-voice transfer ("rewrite this in Hemingway") — use
  `writing-style` directly (this skill delegates there when a curated voice is
  requested).
- Hard sci-fi/historical VeriForge latent knowledge-gap research or
  fiction-specific ellipsis audits — use `story-craft`.
- Autonomous one-shot book or essay generation without human drafting and
  review.

## Step types (D/P labelling)

| Step | Type | Oracle / critique |
|---|---|---|
| Stage 0 — Anchor & mode setup | D | `onto_anchor` (verifies `literary_composition` / `derived:narrative` / `gc:` URIs) |
| Stage 1 — Plan structure (`plan-structure.j2`) | D render feeding P | `render_template` (D); structural plan critiqued by the human author (P) |
| Stage 2 — Human-led drafting / optional `writing-style` delegation | P (+ D centroid if delegated) | Human author owns prose; `writing-style` centroid distance when invoked |
| Stage 3 — Post-draft GOLEM state extraction (`extract-state.j2`) | D render feeding P | `render_template` (D); gated by `CC-1` and `CC-2` in `lisp_eval` (D) |
| Stage 4 — Deterministic continuity & preservation gates (`CC-1`, `CC-2`) | D | `lisp_eval` (epistemic-leak check + verbatim human-line preservation) |
| Stage 5 — Decoupled craft review (`craft-review.j2`) | D render feeding P | `render_template` (D); local edits accepted or rejected by human author (P) |
| Convergence gate (`CC-3`) | D | `lisp_eval` over `open_findings` and `iteration` (max 3 iterations) |

## Instructions

Follow Flower & Hayes's recursive model: any stage may be entered directly
when the author already holds earlier artifacts, but never mix drafting
(Stage 2) with post-draft state extraction (Stage 3) or critique (Stage 5) in
a single unseparated pass.

### Stage 0 — Anchor & Prior Context

1. Confirm `mode` (`fiction` or `expository`) and check prior project context
   with `curator_memory_recall` (if an entity key exists) or read the author's
   existing Markdown codex notes via `read_file`.
2. Resolve active domain terms with `onto_anchor` (e.g., `literary_composition`,
   `narrative`, `gc:G1_Character`, `gc:G7_Narrative_Sequence`) when anchoring
   structural claims. Never conflate `omc:Composition` with written
   composition.

### Stage 1 — Non-Constraining Structural Plan (Optional / Recursive)

1. Call `render_template` with `template_ref: "co-composition/plan-structure"`
   and this complete context shape:
   ```json
   {
     "mode": "fiction",
     "author_brief": "<author's premise, scene goal, or thesis>",
     "existing_state_summary": "<compact summary of relevant prior chapters/sections, or 'none'>",
     "active_constraints": ["<genre, audience, POV, or length constraint>"]
   }
   ```
2. Present the `structural_plan`, `open_questions`, and
   `divergent_alternatives` to the author for selection before drafting.

### Stage 2 — Human-Led Drafting (Voice-First)

1. Keep drafting freeform and author-led. Do **not** inject full ontology
   schemas or exhaustive codex dumps into drafting prompts—supply only the
   active scene's mentioned entities and current-chapter state slice.
2. When the author explicitly requests a curated literary voice (`hemingway`,
   `woolf`, `agatha-eliot`, `jane-wilde`, `ulysses-s-twain`), delegate via the
   `skill` tool to `writing-style`.

### Stage 3 — Post-Draft GOLEM State Extraction

1. After a chapter or section draft is written, collect any human-authored
   lines from the existing codex note into `human_authored_lines`.
2. Call `render_template` with `template_ref: "co-composition/extract-state"`
   and this complete context shape:
   ```json
   {
     "chapter_or_section_id": "ch-02",
     "draft_text": "<finished chapter or section text>",
     "existing_codex_notes": "<current Markdown codex content, or ''>",
     "human_authored_lines": ["<exact human-typed line to preserve verbatim>"]
   }
   ```
3. For larger multi-chapter corpora stored in SQLCipher, optionally run
   `corpus_extract_assertions` (which emits GOLEM predicates for narrative
   chunks) to cross-check long-range assertions.

### Stage 4 — Deterministic Continuity & Preservation Gates (`CC-1`, `CC-2`)

1. **Gate `CC-1` (D — Epistemic-leak check):** Verify via `lisp_eval` that
   every `(character, fact, chapter_index)` in `epistemic_references` was
   already present in that character's `knows` timeline at some
   `acquired_chapter <= chapter_index`.
   - form:
     ```lisp
     (begin
       (define is-array (lambda (x) (or (eq x (list)) (and (listp x) (not (is_null x))))))
       (if (or (not (is-array epistemic_references)) (not (is-array knowledge_timeline)))
         (list (list "open_findings" 1) (list "leaks" (list "invalid_inputs")))
         (begin
           (define known-by (lambda (ch f ch_idx ks)
             (cond ((is_null ks) false)
                   ((and (listp (car ks))
                         (stringp (assoc "character" (car ks)))
                         (stringp (assoc "fact" (car ks)))
                         (numberp (assoc "acquired_chapter" (car ks)))
                         (string= ch (assoc "character" (car ks)))
                         (string= f (assoc "fact" (car ks)))
                         (<= (assoc "acquired_chapter" (car ks)) ch_idx)) true)
                   (t (known-by ch f ch_idx (cdr ks))))))
           (define find-leaks (lambda (refs ks)
             (cond ((is_null refs) (list))
                   ((and (listp (car refs))
                         (stringp (assoc "character" (car refs)))
                         (stringp (assoc "fact" (car refs)))
                         (numberp (assoc "chapter_index" (car refs)))
                         (known-by (assoc "character" (car refs))
                                   (assoc "fact" (car refs))
                                   (assoc "chapter_index" (car refs)) ks))
                    (find-leaks (cdr refs) ks))
                   (t (cons (car refs) (find-leaks (cdr refs) ks))))))
           (let ((leaks (find-leaks epistemic_references knowledge_timeline)))
             (list (list "open_findings" (length leaks))
                   (list "leaks" leaks))))))
     ```
   - receipts:
     ```lisp-receipts
{"gate":"CC-1","case":"no-leak","env":{"epistemic_references":[{"character":"Mara","fact":"ledger_pawned","chapter_index":3}],"knowledge_timeline":[{"character":"Mara","fact":"ledger_pawned","acquired_chapter":2}]},"expected":{"open_findings":0,"leaks":[]}}
{"gate":"CC-1","case":"epistemic-leak-detected","env":{"epistemic_references":[{"character":"Elias","fact":"ledger_pawned","chapter_index":2}],"knowledge_timeline":[{"character":"Mara","fact":"ledger_pawned","acquired_chapter":1}]},"expected":{"open_findings":1,"leaks":[{"character":"Elias","fact":"ledger_pawned","chapter_index":2}]}}
{"gate":"CC-1","case":"malformed-ref-entry","env":{"epistemic_references":["bad",{"character":"Mara","fact":"ledger_pawned","chapter_index":3}],"knowledge_timeline":[{"character":"Mara","fact":"ledger_pawned","acquired_chapter":2}]},"expected":{"open_findings":1,"leaks":["bad"]}}
{"gate":"CC-1","case":"null-inputs","env":{"epistemic_references":null,"knowledge_timeline":[]},"expected":{"open_findings":1,"leaks":["invalid_inputs"]}}
     ```

2. **Gate `CC-2` (D — Human-line preservation guard):** Verify via `lisp_eval`
   that every line in `human_authored_lines` appears verbatim in
   `updated_codex_markdown` before writing any codex update:
   - form:
     ```lisp
     (begin
       (define is-array (lambda (x) (or (eq x (list)) (and (listp x) (not (is_null x))))))
       (define all-preserved (lambda (lines doc)
         (if (is_null lines) true
             (and (stringp (car lines))
                  (<= (length (car lines)) (length doc))
                  (string-contains (car lines) doc)
                  (all-preserved (cdr lines) doc)))))
       (and (is-array human_authored_lines)
            (stringp updated_codex_markdown)
            (all-preserved human_authored_lines updated_codex_markdown)))
     ```
   - receipts:
     ```lisp-receipts
{"gate":"CC-2","case":"human-line-preserved","env":{"human_authored_lines":["- Mara never trusts harbor officials."],"updated_codex_markdown":"# Mara\n- Mara never trusts harbor officials.\n- ch 2 | location: Harbor"},"expected":true}
{"gate":"CC-2","case":"human-line-dropped","env":{"human_authored_lines":["- Mara never trusts harbor officials."],"updated_codex_markdown":"# Mara\n- ch 1 | status: harbor-bound\n- ch 2 | location: Harbor"},"expected":false}
{"gate":"CC-2","case":"null-human-lines","env":{"human_authored_lines":null,"updated_codex_markdown":"# Mara"},"expected":false}
{"gate":"CC-2","case":"empty-human-lines","env":{"human_authored_lines":[],"updated_codex_markdown":"# Mara"},"expected":true}
{"gate":"CC-2","case":"short-ascii-codex","env":{"human_authored_lines":["- Mara never trusts harbor officials."],"updated_codex_markdown":"short"},"expected":false}
{"gate":"CC-2","case":"non-string-codex","env":{"human_authored_lines":["- Mara never trusts harbor officials."],"updated_codex_markdown":null},"expected":false}
     ```
   Known `hkask-lisp` boundary: `length` counts Unicode characters while
   `string-contains` compares UTF-8 byte lengths, so the length pre-check
   prevents short-codex crashes for ASCII lines while a multi-byte UTF-8 line
   longer in bytes than `updated_codex_markdown` still surfaces a runtime
   error (blocking the write visibly, never failing open). If `CC-2` returns
   `false`, **refuse the codex write**, restore the missing human lines, and
   re-run `CC-2`.

### Stage 5 — Decoupled Craft Review

1. Call `render_template` with `template_ref: "co-composition/craft-review"`
   and this complete context shape:
   ```json
   {
     "mode": "fiction",
     "draft_text": "<finished chapter or section text>",
     "continuity_findings": [],
     "author_voice_goals": "<author's stated register, rhythm, or voice goals>"
   }
   ```
2. Present `craft_observations`, `cohesion_diagnostics`,
   `ai_idiosyncrasy_flags`, and bounded `proposed_local_edits` to the human
   author. Apply only edits the author explicitly accepts.

## Convergence

After Stage 4 and Stage 5, run Gate `CC-3` in `lisp_eval` using the
`open_findings` count returned by `CC-1` (plus any unresolved `CC-2`
preservation failure counted as `1`) and the 1-based `iteration` count:

- form:
  ```lisp
  (if (or (not (numberp open_findings)) (< open_findings 0) (not (numberp iteration)) (< iteration 1))
    (quote invalid_inputs)
    (if (= open_findings 0) (quote done) (if (>= iteration 3) (quote stop-and-report) (quote iterate))))
  ```
- receipts:
  ```lisp-receipts
{"gate":"CC-3","case":"clean-done","env":{"open_findings":0,"iteration":1},"expected":"done"}
{"gate":"CC-3","case":"iterate-on-findings","env":{"open_findings":1,"iteration":1},"expected":"iterate"}
{"gate":"CC-3","case":"stop-at-bound","env":{"open_findings":1,"iteration":3},"expected":"stop-and-report"}
{"gate":"CC-3","case":"negative-open-findings","env":{"open_findings":-1,"iteration":1},"expected":"invalid_inputs"}
  ```

- `done`: deliver the updated codex and accepted local edits.
- `iterate`: return to Stage 2/3 with the exact `CC-1` leak list and human
  revision choices.
- `stop-and-report`: stop at iteration 3 and report remaining open findings for
  authorial decision.

## Regression case

Renders (declared contexts): `co-composition/plan-structure` with
`mode: "fiction"`, `author_brief`, `existing_state_summary`, and
`active_constraints`; `co-composition/extract-state` with
`chapter_or_section_id`, `draft_text`, `existing_codex_notes`, and
`human_authored_lines`; `co-composition/craft-review` with `mode`,
`draft_text`, `continuity_findings`, and `author_voice_goals`.
Gates: `CC-1`, `CC-2`, and `CC-3` receipts above executed live via
`lisp_eval`.

## Registry Templates

| Template | Purpose |
|---|---|
| `plan-structure.j2` | Build a non-constraining structural or rhetorical plan: for fiction use Genette Order/Duration/Focalization and Greimas actantial tension; for expository/persuasive writing use Mann & Thompson RST nucleus-satellite relations and Swales genre moves. |
| `extract-state.j2` | Extract post-draft GOLEM v1.1 story/document state updates (characters, locations, objects, relationships, timeline status, and epistemic knows facts) as append/replace diffs that leave all human-authored lines verbatim. |
| `craft-review.j2` | Run a decoupled craft critique checking Phelan mimetic/thematic/synthetic balance, Williams topic/stress cohesion, Leech & Short speech/thought presentation, anti-homogenization alternatives, and AI-prose idiosyncrasies without rewriting the draft. |

## Constraints

- Human author holds Final Say over all prose and codex edits; never silently
  overwrite whole chapters or human-authored codex lines (`CC-2`).
- Never inject raw RDF/OWL ontology schemas into live drafting prompts; use
  GOLEM v1.1 post-draft only.
- Never conflate MovieLabs `omc:Composition` with literary/rhetorical
  composition (`derived:literary_composition`).
- Keep commercial/folk plotting frameworks outside the peer-reviewed craft
  model tier unless explicitly requested by the author.
- If any MCP tool or `render_template`/`lisp_eval` call fails, report it via
  `curator_report_skill_use_issue` with `skill_name: "co-composition"` and
  surface the failure visibly rather than silently skipping a gate.
