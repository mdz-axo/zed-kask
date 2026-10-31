---
title: "zed-kask Documentation"
audience: [developers, architects, agents, operators]
last_updated: 2026-10-28
version: "2.10.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# zed-kask Documentation

> **zed-kask** is a minimal-divergence fork of the [Zed editor](https://zed.dev) with the hKask agent platform compiled in-process. The agent runtime, skills, Regulation nervous system, and sovereign memory run inside the editor as native surfaces; 8 managed MCP servers are launched as child processes over stdio by Zed's `context_server` host (`kask/crates/kask_bridge/src/mcp_servers.rs`).

**Canonical reference:** [`architecture/zed-host-architecture-plan.md`](architecture/zed-host-architecture-plan.md) describes the composition root and current crate inventory. The authoritative integration surface is [`DIVERGENCE.md`](../../DIVERGENCE.md), which lists current and retired numbered seams (retired numbers are never reused).

**Pre-collapse hashes:** commit hashes cited in this corpus predate the 2026-10-27 history collapse (see [`DIVERGENCE.md`](../../DIVERGENCE.md), "History collapse and layered reconstruction") and reference erased pre-collapse history; they do not resolve in this repository. The seam register carries the durable record.

**Per-crate docs:** [`diataxis/INDEX.md`](diataxis/INDEX.md) lists 17 retained artifacts across 9 cross-cutting sets. Eight tutorials were folded on 2026-09-16; on 2026-09-28 the eight per-crate how-to documents were folded into their set references (each reference gained a Procedures section) and the hkask-tool-port explanation was folded into its reference; `diataxis/hkask-mcp-server/tutorial.md` is the one retained tutorial.

**Corpus size (measured 2026-10-30):** 77 Markdown documents and one live YAML inventory in the committed corpus, 82 files total in the working tree (`find kask/docs -type f | wc -l`: 78 committed plus four in-flight drafts from concurrent streams) — under the formal **fewer-than-85** cap; the committed corpus sits three above the **75-file working target** (cap and target history: operator rulings 2026-09-30 and 2026-10-07, [`DOCUMENTATION_STANDARDS.md`](architecture/DOCUMENTATION_STANDARDS.md) §3; the target is condensation direction, not a gate). `principle-constraints.yaml` is a live governance inventory consumed by `kask/scripts/check-principle-constraints.sh`, not a deletion candidate.

## Repair and improvement plans

| Document | Status and purpose |
| --- | --- |
| [`hKask core and MCP repair plan`](plans/hkask-core-mcp-repair-improvement-plan.md) | Active, operator-authorized repair plan; no backward-compatibility requirements. §9 separates verified gallery/packaging slices from unfinished authority, cancellation, containment, attribution and recovery work; P6 separates skill evaluation (algedonic review with operator and Curator) from skill execution; P8 records the experimentation program's execution records — the reference model is promoted to [`research/experimentation-reference-model.md`](research/experimentation-reference-model.md) (2026-10-05) with the locked operator decisions. |
| [`Cybernetic nervous system alignment plan`](plans/cybernetic-nervous-system-alignment.md) | Active — reference model admitted (operator ruling 2026-09-30); S1–S6 recorded; the §8 legacy/orphan cleanup program (CU-1–CU-8) with CU-1–CU-5/CU-7 complete and CU-6 gated on displacement; register row L25 classified (2026-10-01). Carries the measurable target condition, the executed live-log PDCA experiment, the ranked subtractive steps S1–S6, and the whole-file pricing menu for the 60-file docs target (sixth slice: P1 ruled 2026-10-01, P2 executed 2026-10-05, P3 ruled retain 2026-10-05). |
| [`educt speaker identity: research findings and implementation plan`](plans/educt-speaker-identity-plan-2026-10-27.md) | Active — operator decisions D1–D4 recorded 2026-10-27 (D3 resolved voice-as-memory). Phases 1a/1b/1c/2 landed (review flags, gated lexical speaker post-pass, section re-transcribe, durable voice enrollment) with Phase-3 evaluation-contract work in flight on the `educt speaker identity` board. |
| [`educt speaker identity evaluation DesignSpec v2`](plans/educt-speaker-identity-evaluation-design-v2-2026-10-28.md) | Proposed — design only, declaration-blocked: the replacement evaluation contract (overlap-scored, lineage-preserving, comparator-predeclared) for the in-flight Phase 3 matching task. |
| [`EUREKA-2: media and educt improvement-research cycle`](plans/eureka2-media-educt-cycle-plan-2026-10-28.md) | Proposed — decisions D1–D8 TPM-decided and presented for functional veto; execution tracked on the Transcript bench board (S1–S4 done, S5/S6 queued). |
| [`Private-Inference MCP Server: functional requirements (D-2)`](plans/private-inference-mcp-requirements-2026-10-30.md) | Proposed — revised after PI review 2026-10-30 (all three platforms in V1, single-user V1 with group access deferred, privacy boundary sounded out during the build); every requirement is a falsifiable outcome claim tied to a principle (P1–P12) or charter value (V1–V8) with a named verification oracle and falsifier. |
| [`Private-Inference MCP Server: architecture plan (D-3)`](plans/private-inference-mcp-architecture-plan-2026-10-30.md) | Proposed — revised after PI review 2026-10-30; refactor-architecture discipline over the research findings (greenfield-plus over the existing constraint surface), with the kata record (D-5); implementation is a separate engagement — nothing here is built or claimed built. |
| [`Private-inference point-of-view memo (D-4)`](plans/private-inference-point-of-view-memo-2026-10-30.md) | Proposed — delivered for PI review 2026-10-30; the hybrid view (local leg private by default, private-cloud leg private by design, one explicit routing policy the PI owns, an MCP server surface) checked by its falsifier list. |

## Architecture

| Document | Description |
| --- | --- |
| [`zed-host-architecture-plan.md`](architecture/zed-host-architecture-plan.md) | **Canonical architecture** — numbered D-seam integration authority, composition root, crate inventory, deletion history. |
| [`standardized-artifact-storage.md`](architecture/standardized-artifact-storage.md) | **D28** — canonical path layout for persistent Kask artifacts. |
| [`memory-system-specification.md`](architecture/memory-system-specification.md) | **Memory system specification** — schema, ingestion, recall, consolidation, decay, hygiene, sovereignty, and embedded diagrams. |
| [`skills-and-composition.md`](architecture/skills-and-composition.md) | **Agent system** — prompt surfaces, skill body injection, composition principles, and testing. |
| [`functional-interaction-spec.md`](architecture/functional-interaction-spec.md) | **Division of Responsibilities** — operator/product-manager and agent/program-manager working agreement. |
| [`compaction-pipeline-spec.md`](architecture/compaction-pipeline-spec.md) | **Compaction pipeline specification** — ratified 2026-09-29: the three entry points and the always-succeed, deterministic-stage-first pipeline contract. |
| [`core/PRINCIPLES.md`](architecture/core/PRINCIPLES.md) | Architecture principles P1–P12. |
| [`core/magna-carta.md`](architecture/core/magna-carta.md) | The Magna Carta — four sovereignty principles. |
| [`core/magnifica-humanitas.md`](architecture/core/magnifica-humanitas.md) | Magnifica Humanitas — the values charter grounding P1–P12 in human dignity (source: Leo XIV, 2026). |
| [`core/co-research-environment.md`](architecture/core/co-research-environment.md) | The Co-Research Environment — the thesis charter: what zed-kask is (identity, anatomy, lineage, collaboration test); completes the charter triad with the Magna Carta (liberties) and Magnifica Humanitas (values). |
| [`core/MDS.md`](architecture/core/MDS.md) | Minimal Domain Specification — five-category taxonomy, 17 library/composition crates, and 9 MCP servers. |
| [`DOCUMENTATION_STANDARDS.md`](architecture/DOCUMENTATION_STANDARDS.md) | Metadata, lifecycle, Mermaid alignment, citation, and writing standards, including operator-retained Proposed plans. |

## Reference

| Document | Description |
| --- | --- |
| [`reference/testing-protocol.md`](reference/testing-protocol.md) | User expectation contracts, test-layer assignment, bounded proofs, and test-evidence validation. |
| [`reference/verification-ladder.md`](reference/verification-ladder.md) | The five verification rungs (presence, liveness, truth, grounding, binding), their zed-kask homes and clocks, the provenance lattice, and the open rung gaps. |
| [`reference/regulation-spans.md`](reference/regulation-spans.md) | Regulation tracing, persisted records, and actual MCP outcome paths. |
| [`reference/mcp-servers/README.md`](reference/mcp-servers/README.md) | MCP server registry — 8 built-in servers, 238 registered tools fleet-wide (reconciled 2026-10-30 against the `tool_surface_pin!` tests), and the fleet tool surface. |
| [`reference/mcp-servers/corpus.md`](reference/mcp-servers/corpus.md) | Corpus server — gather → process → output pipeline (25 tools). |
| [`reference/mcp-servers/curator.md`](reference/mcp-servers/curator.md) | Curator server — sovereign memory, regulation readback, and the algedonic log (15 tools). |
| [`reference/mcp-servers/experimentation.md`](reference/mcp-servers/experimentation.md) | Experimentation server — experiment registry for the sharded experimentation program (8 tools). |
| [`reference/mcp-servers/kata-kanban.md`](reference/mcp-servers/kata-kanban.md) | Kata-kanban server — Toyota Kata task boards and persistent-until-resolved functional goals (26 tools). |
| [`reference/mcp-servers/media.md`](reference/mcp-servers/media.md) | Media server — research-evidence ingestion, gallery, transcription, educt, jobs, and workflows. |
| [`reference/mcp-servers/prediction-markets.md`](reference/mcp-servers/prediction-markets.md) | Prediction-markets server — Polymarket/Kalshi calibration and economic data. |
| [`reference/mcp-servers/research.md`](reference/mcp-servers/research.md) | Research server — web/RSS retrieval, evidence scoring, run ledger, and paper identity. |
| [`reference/mcp-servers/scenarios.md`](reference/mcp-servers/scenarios.md) | Scenarios server — Schwartz/Tetlock pipeline (22 tools; the provider-free forecast loop moved here from the removed companies server, and `scenario_from_markets_set` bridges prediction-market records into event trees). |
| [`reference/skills/README.md`](reference/skills/README.md) | Registry of 57 skills, 54 template namespaces, and 212 `.j2` resources. |
| [`loop-register.md`](loop-register.md) | Loop audit register — 25 system loops (L24 the experimentation protocol, L25 experimentation-health sensing, added 2026-09-30/10-01) with audit verdicts, three-layer (A/B/C) classifications with per-row sense→report→actuate maps and INV1–INV6 alignment verdicts (pass 3), deferred operator decisions, and the change-log ledger. |
| [`reference/kask-settings.md`](reference/kask-settings.md) | Kask settings and environment reference. |
| [`reference/lisp-eval-dialect.md`](reference/lisp-eval-dialect.md) | Sandboxed `lisp_eval` dialect and `form`/`env` contract. |
| [`reference/ontology-bridge.md`](reference/ontology-bridge.md) | Published-vocabulary bridge, exact term resolver, and `onto_anchor` ladder. |
| [`reference/native-ontologies.md`](reference/native-ontologies.md) | Native-ontologies inventory — every natively-available vocabulary (248 pinned files across 20 source directories), each one's prefixes, version, license and domain, live-probed example resolutions, the 91-ruling derived registry, and the ladder tiers in functional terms; companion to `ontology-bridge.md` (API) and `research/software-ontology-adoption.md` (decision trail). Added 2026-10-13. |
| [`reference/upstream-rebase-process.md`](reference/upstream-rebase-process.md) | Upstream rebase process and removal principles for the numbered D-seams. |

## Diagrams

| Document | Description |
| --- | --- |
| [`DIAGRAMS_INDEX.md`](DIAGRAMS_INDEX.md) | Registry of 83 current `DIAGRAM_ALIGNMENT` records representing 83 unique IDs; 18 records are in the four consolidated diagram files and 65 are inline. |
| [`diagrams/architecture.md`](diagrams/architecture.md) | Cross-cutting architecture diagrams. |
| [`diagrams/kanban.md`](diagrams/kanban.md) | Kata-kanban state machines: task status, move controller, and the goal lifecycle (create → progress → human closure → assessment → current recall). |
| [`diagrams/ui-widgets.md`](diagrams/ui-widgets.md) | Native UI-widget structures. |
| [`diagrams/mcp-dispatch.md`](diagrams/mcp-dispatch.md) | MCP runtime and tool-dispatch flows. |

## Research

| Document | Description |
| --- | --- |
| [`research/chunking-for-rag-research.md`](research/chunking-for-rag-research.md) | Prior-art study of RAG text-chunking patterns and reference models, and the corpus pipeline's alignment and gaps against them. Recommendations only — not implemented. |

| [`research/kanban-board-reference-models.md`](research/kanban-board-reference-models.md) | Reference models for kanban board naming/navigation (Wekan, Planka, Kan, Kanboard), the kata-kanban's alignment and gaps against them, and the implemented shaping/test plan. |
| [`research/syntax-semantic-probabilistic-deterministic-space.md`](research/syntax-semantic-probabilistic-deterministic-space.md) | The syntax–semantic × probabilistic–deterministic 2×2 region space — the `region-routing` skill's recorded reference model: framing, prior art, an empirical micro-test, the navigable map, the C1–C15 claim ledger, and the folded three-party collaboration overlay (§4). |
| [`research/d-seam-audit.md`](research/d-seam-audit.md) | D-seam audit ledger — resumable per-seam worklist over the live `DIVERGENCE.md` surface; identity coverage checked by `kask/scripts/check-d-seam-audit-ledger.sh`. |

| [`research/cybernetic-nervous-system-reference-model.md`](research/cybernetic-nervous-system-reference-model.md) | Reference model (admitted by operator ruling 2026-09-30): the mammal-general afferent/efferent + expectation/prediction-error + reflexive/sensorimotor/deliberative structure, its mapping to loop layers A/B/C with named invariants INV1–INV6, and the surprise-gated logging principle with its information-efficiency argument. All citations resolved (research run 4b9a3b3ed05bf860; 0 resolution failures). Added 2026-09-30. |
| [`research/experimentation-reference-model.md`](research/experimentation-reference-model.md) | The experimentation program's reference model (promoted from the repair plan §P8, 2026-10-05): the vary-locally/select-centrally/retain-durably pattern, the shard map, the generalized experiment protocol, guardrails and meta-metrics with research anchors, the locked operator rulings (Q0–Q7), and the registry's CNS connection map. The execution records remain in the plan §P8. Hosts the discovery-cycle flowchart `DIAG-ARCH-EUREKA-001` — the live `eureka-cycle` skill's ten-step loop contract, relocated here 2026-10-30 from the retired implementation plan. |
| [`research/adc-targeting-carrier-pilot-2026-10-18.md`](research/adc-targeting-carrier-pilot-2026-10-18.md) | Real functional trial of `experimental-design` / `hypothesis-testing`: the operator's ADC targeting-vs-carrier/AUC hypothesis normalized into four rivals; candidate designs gated; the confirmatory factorial bound to a human executor; Hamblett 2004 reanalysed deterministically; outcome `inconclusive`; registry schema mismatch and audit blockage recorded without fake execution. |
| [`research/tree-sitter-adoption-proposal.md`](research/tree-sitter-adoption-proposal.md) | Tree-sitter code-graph capability research and adoption proposal: the partially-confirmed underuse premise (0 structural instruments kask-side vs 131 upstream `tree_sitter::` refs), the 7-capability cited matrix with the parser-not-graph-engine boundary, essentialist-screened use cases (3 survive, 2 rejected, 1 deferred), and the landed structural layer (pinned ast-grep instrument, unwrap ratchet gate over 468 baselined instances, symbol-anchored citation re-anchoring) with measured experiment outcomes — one pre-registered claim falsified as stated, corrected, and reported. Accepted 2026-10-11; the four measured follow-ups landed the same day: widened resolver kinds + impl-qualified find (86% overall / 94% anchorable citation survival), the docs-citation ladder gate (0 rotted — the "671 rotted" reading was false absence; 9 line-drift citations re-pointed across 6 docs), and the `Result<_, String>` structural rule replacing the string-errors gate's line-based matching. |
| [`research/install-build-ci-reference-models.md`](research/install-build-ci-reference-models.md) | Install, build, and CI scripts — reference models from published practice (rustup, Homebrew, nix-installer, Google Shell Style Guide, ShellCheck), the 2026-10-11 pre-fix audit (F1–F7, every one fixed in the same change), the landed corpus-bundle install stage (up-to-date gate, physical resume, atomic assembly), and the conformance gate + selftest that pin the practice. Added 2026-10-11. |
| [`research/software-ontology-adoption.md`](research/software-ontology-adoption.md) | Software ontology adoption — rung-1 and rung-2 research record: the five load-bearing terms (inference, service level agreement, fine-tuning, data retention, audit) resolved through the vendored DPV 2.3 and CSO 3.5 pins, the per-candidate licensing research mapped onto the zk-ref-open allowlist posture, the five rulings with their authorities, and the honest open register (SEON deferred, Skillware pinned but not yet cited). Added 2026-10-13. |

| [`research/media-server-lead-onboarding.md`](research/media-server-lead-onboarding.md) | Media MCP server lead-developer onboarding — tree-grounded orientation for the media server: where everything lives and what to do first, every load-bearing claim cited. |
| [`research/media-model-selection-report.md`](research/media-model-selection-report.md) | Media-model selection record — STT, audio-event annotation, visual-event tagging, and object-redaction models: the measured review with adoptions and outcomes in dated brackets; cited by the inpainting ontology-anchor ruling (`kask/crates/hkask-bridge-ontology/src/derived.rs:2001`). |
| [`research/visual-event-tagging-design.md`](research/visual-event-tagging-design.md) | Visual-event tagging design — the tiered redaction vocabulary `layers_render_redactions` implements (tier 1 cover: blur/pixelate/black/delogo; tier 2 `mask_track` reserved) and its human-gated decision-layer contract. |
| [`research/private-inference-research-syntheses-2026-10-30.md`](research/private-inference-research-syntheses-2026-10-30.md) | Private-inference research syntheses (D-1) — three bounded research runs (R-1 private/secure inference stacks, R-2 provider MCP gateways, R-3 product direction; all completed in the run ledger), every claim dated and verification-stated under window discipline 2026-01-30 through 2026-10-30. |


## Verification gate

- [x] Six-field metadata plus `mds_categories` is present on every active or operator-retained Proposed document (frontmatter gate green over the full working tree, 2026-10-30 — the four private-inference planning drafts gained their seven-field headers when they landed).
- [x] Current-state Mermaid alignment and the Proposed conceptual exception satisfy `DOCUMENTATION_STANDARDS.md`; `DIAG-ARCH-EUREKA-001` re-verified in its new home 2026-10-30 against the live loop contract.
- [x] Internal links target retained documents — this pass swept every row and reference orphaned by the 2026-10-30 stale-docs deletion and by its own six deletions.
- [x] Diagram metadata has unique-ID/location registry parity (83 records, tree-verified 2026-10-30).
- [x] Edited citations use full repository-relative paths; this pass re-anchored the 12 `@sym` citations rotted by `crates/agent/src/thread.rs` line drift (the 2026-10-30 compaction commits) in `skills-and-composition.md` and `lisp-eval-dialect.md` via `kask/scripts/resolve-citation-anchor.sh`, and the citation, anchor, drift and link gates re-ran green over the committed tree.
- [x] Document count is 82 in the working tree (measured 2026-10-30: 78 committed — 77 Markdown plus the live YAML — plus four concurrent-stream in-flight drafts), under the fewer-than-85 cap; the committed corpus sits three above the 75-file working target — condensation direction, not a gate.

## See also

- [`DIVERGENCE.md`](../../DIVERGENCE.md) — authoritative numbered divergence manifest.
- [`diataxis/INDEX.md`](diataxis/INDEX.md) — retained Diataxis sets.
- [`DIAGRAMS_INDEX.md`](DIAGRAMS_INDEX.md) — Mermaid verification registry.
