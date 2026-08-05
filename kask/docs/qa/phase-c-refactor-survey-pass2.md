# Phase C — Refactor-Architecture Survey (Second Pass)

Date: 2026-08-04. Scope: the structural artifacts introduced by the
post-first-pass commits (`e1d2cc014e..HEAD`) — the follow-up refactors that no
survey had covered: swarm_panel `author.rs`/`compose.rs` extraction, corpus
`helpers.rs` growth, shared error mappers, `ledger_tools.rs` rename.

## Verdict table

| Artifact | Verdict | Rationale |
|---|---|---|
| `swarm_panel/src/author.rs` (226) | Shallow-but-justified | Cohesive single surface; an impl-split (fields `pub(crate)`, actions stay in panel), not a module boundary — accepted GPUI renderer pattern, honestly documented |
| `swarm_panel/src/compose.rs` (281) | Shallow-but-justified | Same pattern |
| `swarm_panel/src/swarm_panel.rs` | **Shallow-flag (unchanged)** | Still 4,112 lines / 74 fns / 8 `render_*` fns after extraction (~13% removed). The first pass's M1 "remainder" claim of completion was optimistic — the god-file end-state was not reached |
| `swarm_panel/src/parse.rs` (657) | Deep | DTOs + view models + parse fns are one concern; watch growth |
| corpus `helpers.rs` | Shallow-flag (mild) | Now a 4-concern grab-bag: error mapping, JSONL IO, vector math, text chunking. Each cluster cohesive; next addition should trigger an `error.rs` split for parity with media/swarm/training |
| corpus `classify_impl.rs` (+61) | Deep | Exemplary change-shape: one config field threaded end-to-end, 4 pinning tests |
| `hkask-mcp-server` shared mappers | Deep | `map_join_error` (3 crates/7 sites), `map_infra_error` (4 crates/9 sites), and now `map_semantic_memory_error` (2 crates) — all clear the 2-consumer bar |
| media `error.rs` | Deep | Genuinely domain-specific; delegates infra arm to shared mapper |
| swarm `ledger_tools.rs` rename | Deep/complete | No stale code refs; router renamed at definition + composition; historical QA docs correctly untouched |

## Findings (this pass) — both fixed

**M1 — `map_semantic_memory_error` duplicated verbatim in two servers**
(corpus `helpers.rs` / training `error_mapping.rs`; both added in
`6051e7f50f`). The same commit range *promoted* shared mappers and *violated*
the promotion discipline — the pattern was applied inconsistently within one
commit. **Fixed** (`61abea787f`): promoted to
`hkask-mcp-server/src/server/validation.rs` (context-param version); corpus
aliases it (mirroring `map_corpus_io_error`), training imports it.

**M2 — training `map_fs_error` re-implemented the just-promoted shared
`map_io_error`** (logic-identical, swapped arg order), while corpus did the
right thing with a `pub(crate) use` alias. **Fixed** (`61abea787f`): deleted;
3 call sites use the shared fn.

## Notes (not blocking)

- **N1**: two import paths for shared mappers (crate root vs `server::`) —
  cosmetic; pick one for greppability.
- **N2**: `SemanticMemoryError::Embedding(_)` → internal in the semantic-memory
  mapper vs `DimensionMismatch` → invalid_argument in `map_embedding_error` —
  the same underlying error classifies differently depending on the wrapping
  type. Defensible (the wrap context differs) but worth a doc line if it
  confuses.
- **L1 (carried)**: `swarm_panel.rs` remains a 4,100-line aggregation;
  `render_swarm_detail` (~200 lines) and `render_card` (~300 lines) are the
  next extraction candidates if the effort continues. Post-release.
- Interface widths all within Ousterhout bounds: author.rs 3 items,
  compose.rs 3, media error.rs 5, training error_mapping.rs 7→5 after M1/M2,
  parse.rs DTOs exempt (crate-internal data types).

## Overall

The follow-up refactors were honest file-splits plus one inconsistently-applied
mapper campaign; with M1/M2 fixed, the error-mapping layer is now fully
convergent: shared infra mappers in `hkask-mcp-server`, genuinely
domain-specific mappers per server, and an enforced (post-repair, see Phase A
pass 2) RR-0044 gate with an explicit annotation vocabulary (`rr0044-ok`) for
sanctioned internal sites. No new friction introduced by this pass's fixes:
the promoted mapper has 2 real consumers, no new traits, no new files beyond
the regression-entry edits.
