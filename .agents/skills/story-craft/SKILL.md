---
name: story-craft
description: Human-led fiction co-composition and editorial critique anchored in GOLEM v1.1, Flower & Hayes cognitive writing processes, Genette narrative discourse, Phelan character dimensions, Reza et al. agency-preserving co-writing (S1/S2/S4), and VeriForge mixed-initiative domain discovery. Keeps the human author in control of sentence-level prose, separates planning, domain research, optional voice contrast, and decoupled line-cited critique, and enforces deterministic prose-budget, citation-grounding, and convergence gates via lisp_eval.
---

# Story Craft

Human-led fiction co-composition, verisimilitude research, and decoupled
developmental critique. The author holds 100% ownership over sentence-level
Translation and Final Say; the agent provides non-constraining planning
briefs, VeriForge-style domain Knowledge Cards, optional side-by-side
voice proposals, deterministic script/`lisp_eval` checks, and line-cited
editorial observations.

## Reference models and ontological anchors

Every phase of this skill is grounded in published primary sources
(resolved under research run `0e8e030e223f8ce9`) and `zed-kask`'s
embedded ontology ladder (`onto_anchor` verified 2026-11-18):

- **GOLEM Ontology for Narrative and Fiction v1.1** (Pianzola et al.,
  2024, `doi:10.5281/zenodo.14911396`): `derived:narrative` (operator
  ruling 2026-10-13), `gc:G1_Character` (citing Jannidis, 2019),
  `gc:G4_Social_Relationship`, `gc:G7_Narrative_Sequence` (fabula vs.
  syuzhet), `gc:G9_Narrative_Unit`, `gc:G12_Setting`,
  `gc:G14_Narrative-Stoff`, `gc:G18_Textual_Feature`. Used **post-draft**
  for continuity and story-bible extraction (`corpus_extract_assertions`),
  never as a live drafting slot-filler.
- **SUMO / CSO anchors:** `sumo:Literature` (literary merit of
  `FictionalText` vs. mechanical plot), `cso:ontology`.
- **Pending bare-term rulings:** bare English `narratology`,
  `focalization`, `fabula`, `plot`, and `setting` currently resolve to
  coarse `5w1h_core` in `onto_anchor`; this skill addresses GOLEM via
  prefixed `gc:` URIs and `derived:narrative` until operator rulings are
  added to `hkask-bridge-ontology/src/derived.rs`.
- **Flower & Hayes (1981)** (`doi:10.58680/ccc198115885`): recursive
  *Planning*, *Translating*, *Reviewing*, and *Monitoring* — never mix
  Translating with Reviewing in one pass.
- **Genette (1980)** *Narrative Discourse* (Order, Duration/Ellipsis,
  Mood/Focalization, Voice) and **Mann & Thompson (1988)** RST
  (`doi:10.1515/text.1.1988.8.3.243`).
- **Phelan (1989)** *Reading People, Reading Plots* (Mimetic, Thematic,
  and Synthetic dimensions of character — operationalizes "melt, don't
  map").
- **Reza et al. (2025)** (`arxiv.org/html/2504.12488v1`, PRISMA review
  of 109 HCI papers) & **Carrera et al. (CHI 2026)**
  (`doi:10.1145/3772318.3791952`): form-focused literary writers protect
  sentence-level Translation; default to **S4 Critical Feedback**, **S2
  Guided Exploration**, and **S1 Structured Guidance** with Suggestions
  (never silent overwrites), Final Say, Global AI Toggle (flow state),
  and Local AI Toggle (intentional rule-breaking).
- **Sun et al. (UIST 2026)** *VeriForge*
  (`programs.sigchi.org/uist/2026/program/content/262814`): AI takes
  initiative on latent domain-knowledge discovery (source-anchored
  Knowledge Cards); the author retains full initiative on narrative
  synthesis.
- **Disconfirming baseline (H-Null guardrails):** Doshi & Hauser
  (*Science Advances*, 2024, `doi:10.1126/sciadv.adn5290`) on voice
  homogenization and Chakrabarty et al. (CHI 2024,
  `doi:10.1145/3613904.3642731`) on LLM cliché/subtext weakness.

## When to Use

- Planning an episode or scene from a living author ledger (`FRAME.md` /
  `OUTLINE.md`) while preserving open story questions and melting
  exemplar resonances.
- Running a VeriForge-style **latent knowledge-gap** check for hard
  sci-fi, historical, or domain-grounded fiction without letting the
  model rewrite prose.
- Comparing two contrasting narrative registers or character voices
  side-by-side across distinct models/prompts as non-overwriting
  proposals.
- Running a **decoupled developmental critique** (Ellipsis, Melt-not-Map,
  Register/Focalization, Phelan Mimetic/Thematic balance, and
  anti-slop/rhythm) backed by deterministic scripts and `lisp_eval`
  gates.
- Running post-draft GOLEM continuity extraction across finished
  episodes.

## When NOT to Use

- Recomposition of technical documentation or single-voice centroid
  style transfer — use `writing-style`.
- Autonomous one-shot story generation without a human author in the
  chair — this skill refuses silent overwrites and preserves authorial
  agency.
- General factual claim verification for research reports — use
  `grounding-verify`.

## Initial and target condition

- **Initial condition:** a manuscript repository with an author decision
  ledger (e.g., `FRAME.md` / `OUTLINE.md`), an episode goal or drafted
  episode file, and optional deterministic check scripts (e.g.,
  `scripts/check_ledger.sh`, `scripts/slop_scan.sh`).
- **Target condition:** the requested phase completes with (a) zero
  unrequested overwrites of author prose, (b) deterministic `lisp_eval`
  gates (`SC-1` prose budget, `SC-2` citation grounding, `SC-3`
  convergence) executed and passed or explicitly overridden by the
  author's Local Rule-Breaking Toggle, and (c) open story questions
  preserved untouched.

## Step types (D/P labelling)

| Step | Type | Oracle / critique |
|---|---|---|
| Phase 1 — Plan (`episode-brief.j2`) | D render feeding P | `render_template` (D); author reviews brief and confirms open questions remain open (P) |
| Phase 2 — Domain Discovery (`knowledge-gaps.j2`) | P + D retrieval | `corpus_query` / `web_search` / `resolve_paper` primary sources (D); author chooses what enters prose (P) |
| Phase 3 — Voice Contrast (`voice-contrast.j2`, optional) | D render feeding P | `render_template` (D); author accepts, rejects, or discards proposals (P); optional `writing-style` composition |
| Phase 4a — Deterministic Prose & Ledger Checks | D | `check_ledger.sh`, `slop_scan.sh --json`, and `lisp_eval` gate `SC-1` |
| Phase 4b — Decoupled Editorial Critique (`ellipsis-audit.j2`) | D render feeding P + D gate | `render_template` (D); cross-family model critique (P) verified by `lisp_eval` citation gate `SC-2` (D) and author veto |
| Phase 5 — Converge & Continuity | D + P | `lisp_eval` convergence gate `SC-3` (D, max 2 review passes); post-draft `corpus_extract_assertions` (GOLEM v1.1) |

## PDCA Loop

```
Plan:  Phase 1 (episode-brief.j2) + Phase 2 (knowledge-gaps.j2, on demand)
Do:    Human drafts (Global AI Toggle = off) OR Phase 3 (voice-contrast.j2, proposals only)
Check: Phase 4a (check_ledger.sh + slop_scan.sh + SC-1) → Phase 4b (ellipsis-audit.j2 + SC-2)
Act:   Phase 5 (SC-3 convergence, max 2 review cycles; author Final Say & Local Rule-Breaking Toggle; optional GOLEM continuity + memory_insert)
```

## Instructions

### Phase 0 — Prior context & mode selection

1. Call `curator_memory_recall` (or `curator_consult`) for prior craft
   rulings on the active manuscript so resolved authorial rules are
   respected.
2. Read the project's governing ledger (`FRAME.md`, `OUTLINE.md`, or
   equivalent) and confirm which phase the author wants: **Plan** (Phase
   1), **Knowledge Gaps** (Phase 2), **Voice Contrast** (Phase 3),
   **Editorial Critique** (Phase 4), or **Post-Draft GOLEM Continuity**
   (Phase 5). Never auto-draft prose unless explicitly asked for a
   proposal.

### Phase 1 — Plan (Flower & Hayes Planning + Genette Posture)

1. Render `story-craft/episode-brief` with `render_template` using this
   complete context shape:

```json
{
  "episode_goal": "<string: episode number/title and core dramatic beat>",
  "frame_ledger": "<string: governing craft rules and resolved decisions from FRAME.md>",
  "discourse_posture": {
    "register": "notebook | communal | frame | braided | open",
    "pov": "old_man | young_woman | shared | open",
    "focalization": "internal | external | zero | open",
    "order": "chronological | analepsis | prolepsis | non_linear"
  },
  "exemplar_resonances": ["<string: exemplar vignette or text to melt, never map 1:1>"],
  "open_questions": ["<string: open story questions that MUST remain open>"]
}
```

2. Present the resulting brief (`scene_seed`, `sensory_anchors`,
   `melt_reminders`, `preserved_open_questions`) to the author as a
   starting sketch—never as a rigid checklist.

### Phase 2 — Domain Discovery (VeriForge Pattern, on demand)

1. When an outline or draft touches hard science, engineering, or
   historical specifics, first retrieve primary sources via
   `corpus_query`, `web_search`, or `resolve_paper`.
2. Render `story-craft/knowledge-gaps` with `render_template`:

```json
{
  "draft_excerpt": "<string: rough outline or drafted passage>",
  "genre_domain": "<string: e.g., hard science fiction / orbital engineering / 1970s-2040s>",
  "world_constraints": "<string: settled physical or historical rules from worldbuilding/>",
  "retrieved_sources": [
    {"title": "<string>", "source": "<string: URL, DOI, or local path>", "excerpt": "<string>"}
  ]
}
```

3. Deliver `knowledge_gaps`, source-anchored `knowledge_cards`, and
   `unverified_claims`. Do **not** rewrite the author's prose to insert
   the facts; the author decides what to weave in and what to leave in
   ellipsis.

### Phase 3 — Voice Contrast (S2 Guided Exploration, optional)

1. Only run when the author explicitly asks to compare two voices or
   registers (for example, `notebook` vs. `communal`, or two distinct
   local models such as `Arete-27B` vs. `Muse-Glimmer-30B` /
   `Gemma-4-26B-A4B`). If centroid-validated curated style transfer is
   requested, delegate to `skill` (`name: "writing-style"`).
2. Render `story-craft/voice-contrast` with `render_template`:

```json
{
  "beat_prompt": "<string: short scene beat or dialogue exchange to test>",
  "register_a": "<string: voice A posture, cadence, and lexical boundaries>",
  "register_b": "<string: voice B posture, cadence, and lexical boundaries>",
  "craft_constraints": ["<string: e.g., preserve ellipsis, no infodumps, no melodrama>"]
}
```

3. Present `proposal_a`, `proposal_b`, and `contrast_notes` side-by-side
   as **non-overwriting proposals**. Never write proposals directly into
   the manuscript file without explicit author instruction.

### Phase 4 — Check (Deterministic Oracles First, Then S4 Critical Feedback)

#### Step 4a — Deterministic Prose & Ledger Oracles (D)

1. Run the repository's deterministic scripts via `terminal` when
   present:
   - `bash scripts/check_ledger.sh --json`
   - `bash scripts/slop_scan.sh <episode_path> --json`
2. Run **`lisp_eval` gate `SC-1` (Prose Budget Gate)** over the
   `slop_scan.sh` JSON metrics (adjusting `min_words`/`max_words` to the
   project's target band):
   - form: `(let ((len-ok (and (>= words min_words) (<= words max_words))) (over-ok (= over_explanation_hits 0)) (filter-ok (<= filter_per_1k max_filter_per_1k)) (mono-ok (<= monotony_runs max_monotony_runs))) (list (list "pass" (and len-ok over-ok filter-ok mono-ok)) (list "len_ok" len-ok) (list "over_explanation_ok" over-ok) (list "filter_ok" filter-ok) (list "monotony_ok" mono-ok)))`
   - env: `{ "words": <int>, "min_words": 900, "max_words": 1500, "over_explanation_hits": <int>, "filter_per_1k": <float>, "max_filter_per_1k": 12.0, "monotony_runs": <int>, "max_monotony_runs": 1 }`
   - Every failure is an **advisory signal** subject to the author's Local
     Rule-Breaking Toggle (deliberate stylistic choices override the
     linter).

#### Step 4b — Decoupled Editorial Critique (P + D Citation Gate)

1. Render `story-craft/ellipsis-audit` with `render_template` (prefer a
   **different model family** than any model used in Phase 3 to avoid
   shared blind spots):

```json
{
  "episode_text": "<string: full drafted episode text with line numbers L1..Ln>",
  "frontmatter_meta": {
    "episode": 1,
    "register": "notebook",
    "pov": "old_man",
    "year": 1976,
    "narrator_age": 4
  },
  "frame_rules": "<string: governing craft rules from FRAME.md>",
  "slop_scan_json": {
    "words": 0,
    "slop_hits": [],
    "filter_words": {"per_1k": 0.0},
    "sentences": {"monotony_runs": []}
  }
}
```

2. Produce the JSON critique (`one_line_pulse`, `ellipsis_cuts`,
   `melt_not_map_flags`, `register_and_character_notes`,
   `slop_and_rhythm_notes`). Every item in the four arrays MUST carry
   `line` (e.g., `"L14"`), `quote` (exact verbatim substring from the
   draft), and `observation` (cut-question or note — never an
   unrequested rewrite).
3. Run **`lisp_eval` gate `SC-2` (Citation Grounding Gate)** to verify
   every finding quotes a real span from `episode_text` and carries a
   non-blank `L<n>` line marker:
   - form: `(begin (define valid-item (lambda (item text) (let ((ln (assoc "line" item)) (qt (assoc "quote" item))) (and (stringp ln) (starts-with "L" ln) (stringp qt) (not (string= "" qt)) (string-contains qt text))))) (define all-valid (lambda (xs text) (if (is_null xs) true (and (valid-item (car xs) text) (all-valid (cdr xs) text))))) (all-valid findings episode_text))`
   - env: `{ "findings": <concatenated list of all critique items>, "episode_text": <raw episode body string> }`
   - If `SC-2` returns `false`, drop or repair the ungrounded finding
     before showing the critique to the author.

### Phase 5 — Converge, Continuity & Learning (Act)

1. Run **`lisp_eval` gate `SC-3` (PDCA Convergence Gate)**:
   - form: `(cond ((= open_findings 0) (quote done)) ((>= iteration 2) (quote stop-and-report)) (t (quote iterate)))`
   - env: `{ "open_findings": <count of unresolved findings not waived by author>, "iteration": <1-based review pass> }`
   - `done`: deliver the clean report. `iterate`: return to the author
     for their manual revision, then re-run Phase 4 once.
     `stop-and-report`: stop at pass 2 and list remaining items for the
     author's Final Say.
2. **Post-draft GOLEM continuity (optional, on finished episodes):**
   When the author approves a finished episode, run `corpus_chunk` →
   `corpus_tag_chunks` → `corpus_extract_assertions` to index
   `gc:G1_Character`, `gc:G4_Social_Relationship`, `gc:G7_Narrative_Sequence`,
   and `gc:G12_Setting` triples across episodes so accidental timeline
   contradictions are caught while intentional register gaps stay
   visible.
3. **Failure surfacing:** If any MCP tool call fails, call
   `curator_report_skill_use_issue` with `skill_name: "story-craft"`,
   the failing tool name, error, and `failure_origin`, then continue
   with available local scripts.

## Regression case

Receipts executed live via `lisp_eval`:
- `SC-1` dirty draft (`words: 24, min_words: 900, max_words: 1500, over_explanation_hits: 1, filter_per_1k: 0.0, max_filter_per_1k: 12.0, monotony_runs: 0, max_monotony_runs: 1`) → `pass: false` (`len_ok: false`, `over_explanation_ok: false`).
- `SC-1` clean draft (`words: 1150, min_words: 900, max_words: 1500, over_explanation_hits: 0, filter_per_1k: 6.2, max_filter_per_1k: 12.0, monotony_runs: 0, max_monotony_runs: 1`) → `pass: true`.
- `SC-2` grounded finding (`quote: "which meant that"` present in `episode_text`, `line: "L5"`) → `true`; fabricated quote (`quote: "shadows danced"`) → `false`.
- `SC-3` convergence: `{open_findings: 0, iteration: 1}` → `done`; `{open_findings: 3, iteration: 1}` → `iterate`; `{open_findings: 2, iteration: 2}` → `stop-and-report`.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `episode-brief.j2` | Prepare a non-constraining episode brief from the project ledger, Genette discourse posture, and exemplar resonances to melt (never a 1:1 grid), keeping open story questions open. |
| `knowledge-gaps.j2` | Identify latent domain-knowledge gaps in a draft or outline (VeriForge pattern) and propose source-anchored Knowledge Cards for domain discovery without rewriting the author's prose. |
| `voice-contrast.j2` | Generate side-by-side beat proposals across two contrasting narrative registers or character voices (S2 Guided Exploration) as non-overwriting options for the author to accept, reject, or discard. |
| `ellipsis-audit.j2` | Run a decoupled S4 critical-feedback pass over a drafted episode across Ellipsis, Melt-not-Map, Register/Focalization, Phelan Mimetic/Thematic balance, and Rhythm, returning line-cited observations and cut-questions only (never unrequested rewrites). |

To render a template, call `render_template` with the ref (e.g., `story-craft/ellipsis-audit`):
- `episode-brief.j2`: `episode_goal`, `frame_ledger`, `discourse_posture`, `exemplar_resonances`, `open_questions`
- `knowledge-gaps.j2`: `draft_excerpt`, `genre_domain`, `world_constraints`, `retrieved_sources`
- `voice-contrast.j2`: `beat_prompt`, `register_a`, `register_b`, `craft_constraints`
- `ellipsis-audit.j2`: `episode_text`, `frontmatter_meta`, `frame_rules`, `slop_scan_json`

## Constraints

- **Human agency & Final Say (Reza et al., 2025):** Never silently
  overwrite an author's episode file. All generated text is a proposal;
  all editorial findings are observations or cut-questions.
- **Global & Local AI Toggles:** Respect flow-state requests (no
  unsolicited suggestions during drafting) and intentional rule-breaking
  (author overrides of `slop_scan.sh` or `SC-1` are recorded as waived,
  never re-flagged as failures).
- **Melt, don't map:** Exemplars and GOLEM classes inform post-draft
  structure and loose resonance—never enforce 1:1 symbolic grids or
  inject formal ontology schemas into prose drafting prompts.
- **No ungrounded critique:** Every editorial finding must pass `SC-2`
  (`L<n>` + verbatim `string-contains` match in `episode_text`).
- **Bounded iteration:** Maximum 2 review passes per episode (`SC-3`);
  the model never scores its own prose to declare convergence.
