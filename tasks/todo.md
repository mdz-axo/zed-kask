# zed-kask Diataxis Documentation Pass — Checklist

## Phase 0 — Restore governing specs (prerequisite)

- [x] **T-00: Restore DOCUMENTATION_STANDARDS.md and MDS.md from git history**
  - Restored both files from commit `a32a7847a4~1` to their original paths
  - Bumped `last_updated` to 2026-07-27 with a restoration note (v0.31.1)
  - `DIAGRAMS_INDEX.md` footnote references resolve

## Phase 1 — Foundation (deepest crates first)

- [x] **T-01: hkask-types Reference** — class diagram of shared traits
  - Artifact at `kask/docs/diataxis/hkask-types/reference.md`
  - Every cited struct/trait has a `grep` hit (13/13 verified)
  - Class diagram renders with `DIAGRAM_ALIGNMENT` block
  - Gates passed: pragmatic-semantics (no OUGHT), pragmatic-cybernetics (9/9 implementors exist), essentialist (6 sections), brand-voice (0 taboo), grill-me Mechanism+Rationale
- [ ] **T-02: hkask-types Explanation** — sequence diagram of port mediation
  - Artifact exists; OUGHT claims only in design-rationale section
  - Feedback loop closes (each port has a real implementor cited)
- [ ] **T-03: hkask-capability Reference** — class diagram of OCAP tokens
  - Artifact exists; every symbol cited; diagram renders with `DIAGRAM_ALIGNMENT`
- [ ] **T-04: hkask-capability Explanation** — state diagram of verification outcomes
  - Artifact exists; OUGHT claims scoped to sovereignty rationale; loop closes

**Checkpoint 1:** `kask/docs/diataxis/` tree exists, INDEX.md seeded, 4 foundation artifacts pass all gates.

## Phase 2 — Core subsystems

- [ ] **T-05: hkask-storage Reference** — ERD of SQLCipher schema
- [ ] **T-06: hkask-storage How-to** — adding a new migration
- [ ] **T-07: hkask-regulation Reference** — class diagram of Regulation ledger/loop/wallet
- [ ] **T-08: hkask-regulation Explanation** — state diagram of homeostatic loop
- [ ] **T-09: hkask-inference Reference** — class diagram of config + provider routing
- [ ] **T-10: hkask-inference How-to** — configuring a new provider
- [ ] **T-11: hkask-templates Reference** — ERD/class of skill manifest schema
- [ ] **T-12: hkask-templates Explanation** — sequence of ManifestExecutor invocation
- [ ] **T-13: hkask-condenser Reference** — class diagram of condensation algorithms
- [ ] **T-14: hkask-condenser Explanation** — state diagram of 2-phase condensation
- [ ] **T-15: hkask-mcp-server Reference** — class diagram of MCP server framework
- [ ] **T-16: hkask-mcp-server Explanation** — sequence of MCP server launch

**Checkpoint 2:** all core subsystem reference + explanation artifacts pass gates.

## Phase 3 — Zed integration layer

- [ ] **T-17: kask_bridge Reference** — class diagram of KaskSettings + bridges
- [ ] **T-18: kask_bridge Explanation** — sequence of composition root (D1–D10)
- [ ] **T-19: kask_panel Reference** — class diagram of panel view + curator variant
- [ ] **T-20: kask_panel How-to** — adding a new panel action
- [ ] **T-21: kask_bridge How-to** — wiring a new kask hook (set_* OnceLock pattern)
- [ ] **T-22: kask_bridge Tutorial** — "Your first kask hook"

**Checkpoint 3:** zed-side integration artifacts pass gates; `// zed-kask:` deviations documented.

## Phase 4 — Tutorials and remaining how-tos

- [ ] **T-23: hkask-types Tutorial** — "Understanding the port traits"
- [ ] **T-24: hkask-types How-to** — "Implementing a new port"
- [ ] **T-25: hkask-capability Tutorial** — "Your first capability token"
- [ ] **T-26: hkask-capability How-to** — "Attenuating a token for a sub-task"
- [ ] **T-27: hkask-storage Tutorial** — "Your first migration"
- [ ] **T-28: hkask-storage Explanation** — bitemporal hMem model
- [ ] **T-29: hkask-regulation Tutorial** — "Reading a Regulation span"
- [ ] **T-30: hkask-regulation How-to** — "Adding a new span namespace"
- [ ] **T-31: hkask-inference Tutorial** — "Routing your first inference request"
- [ ] **T-32: hkask-inference Explanation** — provider selection rationale
- [ ] **T-33: hkask-templates Tutorial** — "Your first skill manifest"
- [ ] **T-34: hkask-templates How-to** — "Adding a PDCA step to a manifest"
- [ ] **T-35: hkask-condenser Tutorial** — "Condensing your first thread"
- [ ] **T-36: hkask-condenser How-to** — "Tuning salience weights"
- [ ] **T-37: hkask-mcp-server Tutorial** — "Your first MCP server"
- [ ] **T-38: hkask-mcp-server How-to** — "Registering a new tool"
- [ ] **T-39: kask_panel Tutorial** — "Your first panel action"
- [ ] **T-40: kask_panel Explanation** — curator variant lifecycle

**Checkpoint 4:** all 40 slices complete.

## Phase 5 — INDEX and finalization

- [ ] **T-41: Write INDEX.md and update README**
  - `kask/docs/diataxis/INDEX.md` lists the full set with links
  - Every in-scope crate has 4 entries; every out-of-scope crate has "N/A — reason"
  - `kask/docs/README.md` links to the Diataxis set

**Checkpoint 5:** INDEX.md complete, all slices recorded.

## Per-slice gate checklist (applies to every slice T-01..T-40)

- [ ] kata-improvement PDCA cycle complete (direction → current → target → experiment)
- [ ] diataxis-diagram quality gate passes (≤ 0.15 weighted total)
- [ ] Diagram carries `DIAGRAM_ALIGNMENT` block with `verified_against` citing code file:line
- [ ] pragmatic-semantics: no uncited Inference claims (confidence ≤ 0.3 rejected)
- [ ] pragmatic-semantics: OUGHT claims only in Explanation quadrant
- [ ] pragmatic-cybernetics: feedback loop closes (documented code path exists + behaves as described)
- [ ] essentialist: deletion test passed, ≤ 7 top-level sections, no organizational comments
- [ ] grill-me: Mechanism round survived
- [ ] grill-me: Rationale round survived
- [ ] brand-voice rubric: all 8 criteria score 4+
- [ ] brand-voice: zero taboo phrases (no hype, no em dash chains, no exclamation points)
- [ ] DOCUMENTATION_STANDARDS §2: 6-field frontmatter present
- [ ] DOCUMENTATION_STANDARDS §5: every design-choice `##` section has ≥1 external footnote citation
- [ ] DOCUMENTATION_STANDARDS §11: `mds_categories` field maps to ≥1 MDS category
- [ ] Artifact links to ≥1 source file and ≥1 sibling artifact in the same crate's Diataxis set
