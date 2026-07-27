# zed-kask Diataxis Documentation Pass — Task Breakdown Plan

<!--
DC+BIBO document metadata
Title:        zed-kask Diataxis Documentation Pass — Task Breakdown Plan
Creator:      task-breakdown skill (GLM 5.2 agent)
Date:         2026-07-27
Type:         bibo:Document
Description:  Vertical-slice decomposition of a Diataxis documentation pass for the
              zed-kask project. One slice = one quadrant for one major crate.
              Each slice runs a full PDCA cycle (kata-improvement) and must pass
              diataxis-diagram, pragmatic-semantics, pragmatic-cybernetics,
              essentialist, and grill-me gates before the slice closes.
              Artifacts conform to DOCUMENTATION_STANDARDS.md, MDS.md, and the
              brand-voice rubric (writing excellence).
-->

## Overview

Produce a complete Diataxis documentation set for the zed-kask project, verified
aligned to the codebase at HEAD. "Complete" means one artifact per Diataxis
quadrant per major crate (tutorial, how-to, reference, explanation). "Aligned"
means every documented symbol, path, behavior, and configuration key is backed
by a citation to a concrete file:line in the current tree — no fabricated APIs,
no stale examples.

Artifacts are written under `kask/docs/diataxis/<crate>/<quadrant>.md` (a new
tree inside the existing `kask/docs/` corpus). The existing
`kask/docs/{reference,explanation}/` cross-cutting files remain authoritative
for cross-cutting concerns; the new per-crate set complements them.

## Governing specifications (read before authoring any artifact)

Three specs govern this pass. Two were deleted from the working tree in commit
`a32a7847a4` (2026-07-25) but remain cited as authoritative throughout the
corpus. They are restored as the first task.

1. **`DOCUMENTATION_STANDARDS.md`** (restored to `kask/docs/architecture/`) —
   the documentation standards spec. Mandates:
   - §2: 6-field frontmatter (`title`, `audience`, `last_updated`, `version`,
     `status`, `domain`, `mds_categories`).
   - §4: Mermaid-First Mandate. Every Mermaid block followed by a
     `DIAGRAM_ALIGNMENT` HTML comment with `id: DIAG-<AREA>-<NNN>`,
     `verified_date`, `verified_against` (must cite a code file:line, not
     prose), `status: VERIFIED|STALE|DEPRECATED`.
   - §5: Sourced-Ideas Mandate. Every `##` section introducing a design
     choice needs ≥1 external footnote citation (APA 7th). Pure
     cross-reference tables are exempt.
   - §6: Document location policy. All docs under `kask/docs/`; only
     `README.md` in crate dirs.
   - §8: Relative-path cross-references only; `(path:line)` for code refs.
   - §9 + Appendix A: Writing Excellence. 4-perspective test
     (Hopper/Lovelace/Schriver/Gentle), must pass ≥3 of 4.
     Formal-technical register. Third person for specs, second person for
     operator guides. Never "we". Max 35 words/sentence. Active voice.
     Definite assertions ("must"/"shall"/"does").
   - §10: Verification checklist (publication gate).
   - §11: MDS alignment. Every doc maps to ≥1 of 5 MDS categories.

2. **`MDS.md`** (restored to `kask/docs/architecture/core/`) — the Minimal
   Domain Specification. 5 categories: Domain, Composition, Trust, Lifecycle,
   Curation. Each has a completeness predicate. Capability-driven ("CAN verb
   on resource via interface"). §9.1: documents go in the directory of their
   primary MDS category. Diataxis→MDS mapping for this pass:
   - Tutorial → Lifecycle (learning path, evolution of understanding)
   - How-to → Composition (procedures, interface usage)
   - Reference → Domain (entities, bounded context, what exists)
   - Explanation → Trust + Curation (design rationale, why it is safe/curated)

3. **`docs/.conventions/brand-voice/`** (on disk) — the writing excellence
   rubric. 8-criterion rubric (Technical Grounding, Natural Syntax, Quiet
   Confidence, Developer Respect, Information Priority, Specificity, Voice
   Consistency, Earned Claims), all must score 4+. Taboo phrases: no hype
   words, no em dash chains (max 1 per paragraph), no exclamation points, no
   "it's not X, it's Y", no triple parallel lists, no rhetorical question
   openers, no "we're excited".

## Current condition (Phase 0 inventory)

### Existing docs

- `kask/docs/reference/` — cross-cutting reference (regulation spans, MCP
  server registry, kask-settings, LoRA catalog, skills). Last touched
  2026-07-24; `regulation-spans.md` last touched 2026-07-25 while
  `hkask-regulation/src/` was touched 2026-07-27 → **stale**.
- `kask/docs/explanation/` — cross-cutting explanation (fusion, cognition,
  skills-and-composition, sovereignty, energy, etc.). Last touched 2026-07-24.
- `kask/docs/architecture/` — `zed-host-architecture-plan.md` (D1–D10 seams),
  `PRINCIPLES.md`, `magna-carta.md`, `salience-specification.md`.
- `kask/docs/research/`, `kask/docs/qa/` — historical.
- **No `tutorial/` quadrant exists.** **No `how-to/` quadrant exists.**
- **No per-crate Diataxis sets exist.** All existing docs are cross-cutting.
- 16 crate-level `README.md` files exist under `kask/crates/*/` and
  `kask/mcp-servers/*/`; these are unstructured and not Diataxis-classified.
- `DOCUMENTATION_STANDARDS.md` and `MDS.md` were deleted in commit
  `a32a7847a4` but are cited as authoritative by `DIAGRAMS_INDEX.md`,
  `corpus.yaml`, and the `tdd`/`diagnose` skills. **Restoration is a
  prerequisite** for this pass.

### Crate graph (kask workspace, 25 crates + 10 MCP servers + 2 zed-side)

LOC ranking (src/ only) identifies the major crates deserving their own Diataxis
set. Crates below ~3000 LOC are documented as a group in INDEX.md rather than
getting individual sets.

### Major crates in scope (10 crates × 4 quadrants = 40 slices)

| # | Crate | LOC | Subsystem | Rationale |
|---|-------|-----|-----------|-----------|
| 1 | `hkask-types` | 9316 | Shared traits + domain types | Core; deepest dependency |
| 2 | `hkask-capability` | 1633 | OCAP token + ToolPort | Core; D3/D4 sovereignty |
| 3 | `hkask-storage` | 10043 | SQLCipher + schema + migrations | Core; persistence foundation |
| 4 | `hkask-regulation` | 10177 | Regulation nervous system | Core; P9 spans; largest non-MPC crate |
| 5 | `hkask-inference` | 10971 | Inference port + provider routing | Core; D1/D3/D8 seam |
| 6 | `hkask-templates` | 6489 | Skill manifest registry | Core; D1 skill execution |
| 7 | `hkask-condenser` | 3489 | Thread condensation | Core; D6 memory path |
| 8 | `hkask-mcp-server` | 1836 | MCP server framework | Core; shared by all 10 MCP servers |
| 9 | `kask_bridge` | 4949 | Zed↔kask adapters | Core; D8 composition root |
| 10 | `kask_panel` | 4260 | Zed agent panel UI | Core; D2 curator surface |

### Out of scope (documented as a group in INDEX.md)

- The 10 `hkask-mcp-*` server crates — already documented cross-cuttingly in
  `kask/docs/reference/mcp-servers/`. INDEX.md will link to those.
- Small support crates (`hkask-goal`, `hkask-forecast`, `hkask-email`,
  `hkask-ledger`, `hkask-guard`, `hkask-keystore`, `hkask-memory`,
  `hkask-bridge-dublincore`, `hkask-services-*`, `hkask-mcp`) — grouped entry.
- Upstream zed crates (`crates/agent`, `crates/agent_ui`, etc.) — not zed-kask
  code; only their `// zed-kask:` deviations are in scope, documented under
  `kask_bridge` and `kask_panel`.

### Gap matrix (per major crate, per quadrant)

Legend: ✅ exists & aligned · ⚠️ exists & stale · ❌ missing · ➖ N/A

| Crate | Tutorial | How-to | Reference | Explanation |
|-------|----------|--------|-----------|-------------|
| hkask-types | ❌ | ❌ | ❌ | ❌ |
| hkask-capability | ❌ | ❌ | ❌ | ⚠️ (sovereignty-and-ocap, cross-cutting) |
| hkask-storage | ❌ | ❌ | ❌ | ❌ |
| hkask-regulation | ❌ | ❌ | ⚠️ (cross-cutting only) | ⚠️ (cross-cutting only) |
| hkask-inference | ❌ | ❌ | ❌ | ❌ |
| hkask-templates | ❌ | ❌ | ⚠️ (skills/README) | ⚠️ (skills-and-composition) |
| hkask-condenser | ❌ | ❌ | ⚠️ (condenser.md MCP) | ⚠️ (salience-spec) |
| hkask-mcp-server | ❌ | ❌ | ❌ | ❌ |
| kask_bridge | ❌ | ❌ | ⚠️ (kask-settings) | ⚠️ (architecture-plan) |
| kask_panel | ❌ | ❌ | ❌ | ❌ |

**Gap summary:** 40 slices total. 0 aligned, ~12 stale cross-cutting (to be
superseded by per-crate reference/explanation with file:line citations), 28
fully missing. Tutorial and How-to quadrants are entirely absent for every
major crate.

## Skill composition (ordered, with contracts)

Each slice runs this pipeline. A slice closes only when all gates pass.

1. **kata-improvement** — wraps the slice in PDCA: direction (this plan),
   current condition (gap matrix above), target condition (slice ACs),
   experiment (write artifact, measure alignment).
2. **diataxis-diagram** — generates the quadrant-appropriate Mermaid artifact:
   - Tutorial → step-by-step flowchart (learning path)
   - How-to → procedural flowchart (task-oriented)
   - Reference → ERD or class diagram (information-oriented)
   - Explanation → state or sequence diagram (understanding-oriented)
   Every diagram carries a `DIAGRAM_ALIGNMENT` block per DOCUMENTATION_STANDARDS §4.2.
3. **pragmatic-semantics** — audits every claim: IS vs OUGHT, cited vs
   inference. Rejects uncited Inference claims with confidence ≤ 0.3. OUGHT
   claims only in Explanation.
4. **pragmatic-cybernetics** — verifies the feedback loop closes: documented
   code path exists and behaves as described.
5. **essentialist** — deletion test per section; ≤ 7 top-level sections; no
   organizational comments.
6. **grill-me** — final critic pass: Recall → Mechanism → Rationale → Edge
   Cases → Synthesis. Slice passes only if Mechanism + Rationale survive.
7. **brand-voice rubric** — 8-criterion scorecard, all 4+. Taboo-phrase scan.

## Phased task list

### Phase 0 — Restore governing specs (prerequisite)

**Checkpoint 0** (after T-00): both specs restored, INDEX.md updated to cite
them at their restored paths.

- **T-00** Restore `DOCUMENTATION_STANDARDS.md` and `MDS.md` from git history
  (commit `a32a7847a4~1`). ACs: (a) both files exist at
  `kask/docs/architecture/DOCUMENTATION_STANDARDS.md` and
  `kask/docs/architecture/core/MDS.md`; (b) frontmatter `last_updated` bumped
  to 2026-07-27 with a note that they were restored after accidental deletion;
  (c) `DIAGRAMS_INDEX.md` footnote references resolve. Deps: None. Scope: S.

### Phase 1 — Foundation (deepest crates first, bottom-up)

Slices are ordered so that documentation of foundation crates (types,
capability, storage) informs the documentation of crates that depend on them.

**Checkpoint 1** (after T-01 through T-04): `kask/docs/diataxis/` tree exists,
INDEX.md seeded, 4 foundation-crate reference artifacts pass all gates.

- **T-01** `hkask-types` Reference — class diagram of shared traits
  (`MemoryPort`, `InferencePort`, `ToolPort`, domain types). ACs: (a) artifact
  at `kask/docs/diataxis/hkask-types/reference.md`; (b) every cited
  struct/trait has a `grep` hit; (c) class diagram renders with
  `DIAGRAM_ALIGNMENT` block. Deps: T-00. Scope: M.
- **T-02** `hkask-types` Explanation — sequence diagram of how the port traits
  mediate between zed and kask. ACs: (a) artifact exists; (b) OUGHT claims only
  in design-rationale section; (c) feedback loop closes (each port has a real
  implementor cited). Deps: T-01. Scope: M.
- **T-03** `hkask-capability` Reference — class diagram of `DelegationToken`,
  `ToolPort`, `CapabilityChecker`, verification flow. ACs: (a) artifact exists;
  (b) every symbol cited; (c) diagram renders with `DIAGRAM_ALIGNMENT`. Deps:
  T-01. Scope: M.
- **T-04** `hkask-capability` Explanation — state diagram of token
  verification outcomes (`VerificationOutcome`). ACs: (a) artifact exists;
  (b) OUGHT claims scoped to sovereignty rationale; (c) loop closes. Deps: T-03.
  Scope: M.

### Phase 2 — Core subsystems

**Checkpoint 2** (after T-05 through T-16): all core subsystem reference +
explanation artifacts pass gates.

- **T-05** `hkask-storage` Reference — ERD of SQLCipher schema (tables, FKs,
  migrations). ACs: (a) artifact exists; (b) every table/column cited to
  `src/schema/` or migration file; (c) ERD renders with Crow's Foot +
  `DIAGRAM_ALIGNMENT`. Deps: T-01. Scope: L → split if needed.
- **T-06** `hkask-storage` How-to — procedural flowchart for adding a new
  migration. ACs: (a) artifact exists; (b) each step cites the migration runner
  code path; (c) flowchart renders with `DIAGRAM_ALIGNMENT`. Deps: T-05.
  Scope: M.
- **T-07** `hkask-regulation` Reference — class diagram of `RegulationLedger`,
  `MetacognitionLoop`, `WalletManager`, `Well`, span enums. ACs: (a) artifact
  exists; (b) every symbol cited; (c) supersedes stale cross-cutting
  `regulation-spans.md` with file:line refs. Deps: T-01. Scope: L.
- **T-08** `hkask-regulation` Explanation — state diagram of the Regulation
  homeostatic loop (sense→compare→compute→act→verify). ACs: (a) artifact
  exists; (b) loop closes against `runtime.rs`; (c) OUGHT claims only in
  rationale. Deps: T-07. Scope: M.
- **T-09** `hkask-inference` Reference — class diagram of `InferenceConfig`,
  provider routing, `ProviderId` enum. ACs: (a) artifact exists; (b) every
  config key cited to `config.rs`/`model_constants.rs`; (c) diagram renders
  with `DIAGRAM_ALIGNMENT`. Deps: T-01. Scope: M.
- **T-10** `hkask-inference` How-to — procedural flowchart for configuring a
  new inference provider. ACs: (a) artifact exists; (b) each step cites
  `KaskInferenceProvidersSettings` + `InferenceConfig::from_secrets`; (c)
  flowchart renders with `DIAGRAM_ALIGNMENT`. Deps: T-09. Scope: M.
- **T-11** `hkask-templates` Reference — ERD/class diagram of skill manifest
  schema (`FlowDef`, manifest.yaml fields). ACs: (a) artifact exists; (b)
  every manifest field cited to `src/` or registry template; (c) diagram
  renders with `DIAGRAM_ALIGNMENT`. Deps: T-01. Scope: M.
- **T-12** `hkask-templates` Explanation — sequence diagram of
  `ManifestExecutor` skill invocation path (D1). ACs: (a) artifact exists;
  (b) loop closes against `BridgeManifestExecutor`; (c) OUGHT claims only in
  rationale. Deps: T-11. Scope: M.
- **T-13** `hkask-condenser` Reference — class diagram of condensation
  algorithms + salience. ACs: (a) artifact exists; (b) every algorithm cited
  to `src/`; (c) supersedes stale cross-cutting condenser.md. Deps: T-01.
  Scope: M.
- **T-14** `hkask-condenser` Explanation — state diagram of 2-phase
  condensation. ACs: (a) artifact exists; (b) loop closes; (c) OUGHT claims
  only in rationale. Deps: T-13. Scope: M.
- **T-15** `hkask-mcp-server` Reference — class diagram of the MCP server
  framework (server trait, tool registration, bootstrap). ACs: (a) artifact
  exists; (b) every symbol cited; (c) diagram renders with `DIAGRAM_ALIGNMENT`.
  Deps: T-01. Scope: M.
- **T-16** `hkask-mcp-server` Explanation — sequence diagram of MCP server
  launch (stdio transport, `bootstrap_mcp_server`). ACs: (a) artifact exists;
  (b) loop closes against `bootstrap_mcp_server`; (c) OUGHT claims only.
  Deps: T-15. Scope: M.

### Phase 3 — Zed integration layer

**Checkpoint 3** (after T-17 through T-22): zed-side integration artifacts
pass gates; `// zed-kask:` deviations documented.

- **T-17** `kask_bridge` Reference — class diagram of `KaskSettings`,
  `BridgeToolPort`, `BridgeManifestExecutor`, `FusionLanguageModel`. ACs: (a)
  artifact exists; (b) every settings key cited to `settings.rs`; (c)
  supersedes stale cross-cutting kask-settings.md. Deps: T-01, T-03, T-09.
  Scope: L.
- **T-18** `kask_bridge` Explanation — sequence diagram of the composition
  root (D1–D10 wiring in `main.rs`). ACs: (a) artifact exists; (b) every
  `set_*` hook cited to `main.rs`/`agent.rs`; (c) loop closes. Deps: T-17.
  Scope: M.
- **T-19** `kask_panel` Reference — class diagram of panel view, curator
  variant, agent panel integration. ACs: (a) artifact exists; (b) every symbol
  cited; (c) diagram renders with `DIAGRAM_ALIGNMENT`. Deps: T-17. Scope: M.
- **T-20** `kask_panel` How-to — procedural flowchart for adding a new panel
  action. ACs: (a) artifact exists; (b) each step cites the action registration
  path; (c) flowchart renders with `DIAGRAM_ALIGNMENT`. Deps: T-19. Scope: M.
- **T-21** `kask_bridge` How-to — procedural flowchart for wiring a new kask
  hook (the `set_*` OnceLock pattern). ACs: (a) artifact exists; (b) each step
  cites the deferred-task pattern from `.rules`; (c) flowchart renders with
  `DIAGRAM_ALIGNMENT`. Deps: T-17. Scope: M.
- **T-22** `kask_bridge` Tutorial — step-by-step learning path: "Your first
  kask hook." ACs: (a) artifact exists; (b) each step builds on the prior;
  (c) flowchart renders with `DIAGRAM_ALIGNMENT`. Deps: T-17, T-21. Scope: M.

### Phase 4 — Tutorials and remaining how-tos

**Checkpoint 4** (after T-23 through T-40): all 40 slices complete, INDEX.md
finalized.

- **T-23** `hkask-types` Tutorial — "Understanding the port traits." Deps:
  T-01. Scope: M.
- **T-24** `hkask-types` How-to — "Implementing a new port." Deps: T-01.
  Scope: M.
- **T-25** `hkask-capability` Tutorial — "Your first capability token." Deps:
  T-03. Scope: M.
- **T-26** `hkask-capability` How-to — "Attenuating a token for a sub-task."
  Deps: T-03. Scope: M.
- **T-27** `hkask-storage` Tutorial — "Your first migration." Deps: T-05.
  Scope: M.
- **T-28** `hkask-storage` Explanation — bitemporal hMem model. Deps: T-05.
  Scope: M.
- **T-29** `hkask-regulation` Tutorial — "Reading a Regulation span." Deps:
  T-07. Scope: M.
- **T-30** `hkask-regulation` How-to — "Adding a new span namespace." Deps:
  T-07. Scope: M.
- **T-31** `hkask-inference` Tutorial — "Routing your first inference
  request." Deps: T-09. Scope: M.
- **T-32** `hkask-inference` Explanation — provider selection rationale.
  Deps: T-09. Scope: M.
- **T-33** `hkask-templates` Tutorial — "Your first skill manifest." Deps:
  T-11. Scope: M.
- **T-34** `hkask-templates` How-to — "Adding a PDCA step to a manifest."
  Deps: T-11. Scope: M.
- **T-35** `hkask-condenser` Tutorial — "Condensing your first thread." Deps:
  T-13. Scope: M.
- **T-36** `hkask-condenser` How-to — "Tuning salience weights." Deps: T-13.
  Scope: M.
- **T-37** `hkask-mcp-server` Tutorial — "Your first MCP server." Deps: T-15.
  Scope: M.
- **T-38** `hkask-mcp-server` How-to — "Registering a new tool." Deps: T-15.
  Scope: M.
- **T-39** `kask_panel` Tutorial — "Your first panel action." Deps: T-19.
  Scope: M.
- **T-40** `kask_panel` Explanation — curator variant lifecycle. Deps: T-19.
  Scope: M.

### Phase 5 — INDEX and finalization

**Checkpoint 5** (after T-41): INDEX.md complete, all slices recorded.

- **T-41** Write `kask/docs/diataxis/INDEX.md` listing the full set with
  links, per-crate per-quadrant status, and "N/A — reason" entries for
  out-of-scope crates. Update `kask/docs/README.md` to link to the Diataxis
  set. ACs: (a) INDEX.md exists; (b) every in-scope crate has 4 entries;
  (c) every out-of-scope crate has an "N/A — reason" entry. Deps: T-01..T-40.
  Scope: S.

## Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| Scope: 40 slices is large for one session | High | Phased checkpoints; Phase 1 (T-00..T-04) is the minimum viable deliverable. Surface scope decision to operator after Phase 1. |
| `DOCUMENTATION_STANDARDS.md` and `MDS.md` were deleted; restoring them may conflict with the deletion intent | Medium | Restore verbatim from git history with a `last_updated` bump and a restoration note. If the operator intended deletion, they can re-delete; the corpus citations demand they exist somewhere. |
| Stale cross-cutting docs (`regulation-spans.md`, `kask-settings.md`, `condenser.md`) may conflict with new per-crate docs | Medium | New per-crate docs cite file:line; cross-cutting docs are not deleted (they serve a different audience) but INDEX.md notes the per-crate doc as the canonical reference for that crate. |
| `DIAGRAM_ALIGNMENT` `verified_against` must cite code file:line, adding verification cost per diagram | Medium | Each slice's diataxis-diagram step extracts entities via `grep`/`read_file` first; the file:line is a byproduct of extraction. |
| Brand-voice rubric forbids em dash chains and exclamation points; technical docs often use both | Low | Authoring discipline: max 1 em dash per paragraph, zero exclamation points. The rubric is part of the grill-me pass. |
| Another agent may destroy uncommitted work again | High | Commit after each phase checkpoint. The first destruction event discarded T-00 through T-01 work. |

## Open questions

1. **Were `DOCUMENTATION_STANDARDS.md` and `MDS.md` intentionally deleted?**
   They are cited as authoritative by `DIAGRAMS_INDEX.md`, `corpus.yaml`, and
   the `tdd`/`diagnose` skills. The deletion commit `a32a7847a4` ("Wire
   metacognition loop to curator status tool") appears unrelated. T-00
   restores them; the operator can re-delete if the deletion was intentional.
2. **Should the Diataxis set live under `kask/docs/diataxis/` (inside the
   kask docs tree) or `docs/diataxis/` (the upstream zed docs tree)?** The
   task says `docs/diataxis/` but the kask docs convention puts all kask docs
   under `kask/docs/`. This plan uses `kask/docs/diataxis/` to match the
   existing tree and DOCUMENTATION_STANDARDS §6. The operator can override.
3. **Scope for one session:** 40 slices is ambitious. Phase 1 (T-00..T-04)
   delivers the foundation. Should the pass continue through all 40, or stop
   at a checkpoint for operator review?

## Refinement history

- **Iteration 1 (initial):** Plan drafted with 10 major crates × 4 quadrants
  = 40 slices, phased bottom-up. T-00 (restore specs) added after discovering
  the governing specs were deleted. Brand-voice rubric added as a 7th gate
  after the operator pointed to the writing excellence spec.
- **Iteration 2 (post-destruction):** Plan regenerated after another agent
  discarded all uncommitted work. Added risk row for repeated destruction;
  recommendation to commit after each phase checkpoint.
