---
title: "zed-kask Documentation"
audience: [developers, architects, agents, operators]
last_updated: 2026-10-25
version: "2.8.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# zed-kask Documentation

> **zed-kask** is a minimal-divergence fork of the [Zed editor](https://zed.dev) with the hKask agent platform compiled in-process. The agent runtime, skills, Regulation nervous system, and sovereign memory run inside the editor as native surfaces; 9 managed MCP servers are launched as child processes over stdio by Zed's `context_server` host (`kask/crates/kask_bridge/src/mcp_servers.rs` — the companies server was removed 2026-10-18, commit `3439db5e00`).

**Canonical reference:** [`architecture/zed-host-architecture-plan.md`](architecture/zed-host-architecture-plan.md) describes the composition root and current crate inventory. The authoritative integration surface is [`DIVERGENCE.md`](../../DIVERGENCE.md), which lists current and retired numbered seams (retired numbers are never reused).

**Pre-collapse hashes:** commit hashes cited in this corpus's pass ledgers and tombstones predate the 2026-10-27 history collapse (see [`DIVERGENCE.md`](../../DIVERGENCE.md), "History collapse and layered reconstruction") and reference erased pre-collapse history; they do not resolve in this repository. The seam register and these documents carry the durable record.

**Per-crate docs:** [`diataxis/INDEX.md`](diataxis/INDEX.md) lists 19 retained artifacts across 10 cross-cutting sets. Eight tutorials were folded on 2026-09-16; on 2026-09-28 the eight per-crate how-to documents were folded into their set references (each reference gained a Procedures section) and the hkask-tool-port explanation was folded into its reference; `diataxis/hkask-mcp-server/tutorial.md` is the one retained tutorial.

**Corpus size (measured 2026-10-27, after the speaker-identity plan pass):** 82 Markdown documents and one live YAML inventory, 83 files total (`find kask/docs -type f | wc -l`) — **under** the formal **fewer-than-85** count gate. The 2026-10-19 realignment pass deleted the executed companies-removal plan (`plans/companies-fmp-eodhd-fibo-removal.md`, −1, tombstone below) — its five operator decisions are all executed (commits `4f13f108da`, `3439db5e00`, `3adc75c9e1`, `6af8ab2e18`) and the ported forecast surface lives in [`reference/mcp-servers/scenarios.md`](reference/mcp-servers/scenarios.md) §§ Forecast persistence, and reconciled the companies-wave drift across the README, MDS, the architecture plan, the diataxis index, the skills registry, and the MCP registry (9 servers, 343 tools); the 2026-10-18 vector-store pass had added one scope/decision record ([`research/vector-store-deduplication-scope.md`](research/vector-store-deduplication-scope.md), +1), which sizes the remaining 1.03 GiB vector duplication and requests a decision; the same day's experimental-design pass added one real functional-trial record ([`research/adc-targeting-carrier-pilot-2026-10-18.md`](research/adc-targeting-carrier-pilot-2026-10-18.md), +1); the 2026-10-17 removal pass deleted the training-server reference and the LoRA training catalog (−2) when the training MCP capability was removed as out of scope for the co-researcher mandate; the same day's charter pass added the co-research-environment thesis charter ([`architecture/core/co-research-environment.md`](architecture/core/co-research-environment.md), +1 — the platform identity statement completing the charter triad with the Magna Carta and Magnifica Humanitas); the 2026-10-16 pass deleted the implemented agentic-scientist integration proposal and the superseded deterministic-vs-probabilistic routing study (−2, tombstones below); the earlier passes consolidated the media-server redesign set into [`reference/mcp-servers/media.md`](reference/mcp-servers/media.md) § Redesign record (−4) and deleted the point-in-time curiosity study (−1, tombstone below), while the concurrent ontology program added three operator-carded records ([`reference/native-ontologies.md`](reference/native-ontologies.md), [`research/ontology-anchoring-review.md`](research/ontology-anchoring-review.md), [`research/ontology-open-loops-resolution.md`](research/ontology-open-loops-resolution.md)) — net 86 → 78, three above the working **75-file target** (raised 60 → 75 by operator ruling 2026-10-07). The 2026-10-22 removal wave added the ABW and Reduct removal plan artifacts (+2, 78 → 80); the 2026-10-23 capability pass added the eureka-cycle implementation plan ([`plans/eureka-cycle-implementation-plan.md`](plans/eureka-cycle-implementation-plan.md), +1, 80 → 81 — point-in-time plan, deleted when executed per the house lifecycle) and reconciled the diagram registry (four stale rows removed with the 2026-10-18→22 waves' deleted diagrams; `DIAG-ARCH-EUREKA-001` registered; the parity gate green); the 2026-10-23/24 affordance pass added one experiment record ([`research/instrument-affordance-experiment.md`](research/instrument-affordance-experiment.md), +1, 81 → 82); the 2026-10-24 denominator pass added the per-task adjudication record ([`research/native-first-denominator-sample.md`](research/native-first-denominator-sample.md), +1, 82 → 83); the 2026-10-24 ADC-allocation framing pass added the meta-study design record (+1, 83 → 84 — fully removed on operator direction 2026-10-25: artifacts deleted, git history purged; see the ledger tombstone below); the 2026-10-25 transcript-bench plan pass added the transcript-bench implementation plan (`plans/transcript-bench-implementation-plan-2026-10-25.md`, +1) and deleted the two executed 2026-10-22 removal plan artifacts (−2, tombstones below), 84 → 83; the 2026-10-26 transcript-bench execution pass delivered S4-3's label editor and highlight rename (`29f6399133`, suite 55/55), reconciled the media-panel transcript-surface citations, and deleted the executed transcript-bench plan (−1, tombstone below) — reconciling that tail arithmetic to the measured 81 files total. The 2026-10-27 speaker-identity plan pass added the ratified educt speaker-identity plan ([`plans/educt-speaker-identity-plan-2026-10-27.md`](plans/educt-speaker-identity-plan-2026-10-27.md), +1 — point-in-time plan, deleted when executed per the house lifecycle) and its bounded research-design assessment ([`plans/educt-research-design-assessment-2026-10-27.md`](plans/educt-research-design-assessment-2026-10-27.md), +1, 81 → 83), and registered the plan's §5 flow diagram (`DIAG-ARCH-SPEAKER-IDENT-001`) with the diagram parity gate green — reconciling the tail arithmetic to the measured 83 files total. Cap history: raised 79 → 85 by operator ruling 2026-10-07, justified by the experimentation and spreadsheet MCP servers' reference documentation and the Magnifica Humanitas values charter ([`core/magnifica-humanitas.md`](architecture/core/magnifica-humanitas.md)) grounding P1–P12; first raised 75 → 79 the same day; cap and target set by operator ruling 2026-09-30, [`DOCUMENTATION_STANDARDS.md`](architecture/DOCUMENTATION_STANDARDS.md) §3. [`principle-constraints.yaml`](architecture/principle-constraints.yaml) is a live governance inventory consumed by `kask/scripts/check-principle-constraints.sh`, not a deletion candidate. The ADC meta-study was fully removed on operator direction (2026-10-25): the record left the tree in the relocation pass (−1, 83 → 82), its data-directory artifacts were deleted, and its study files were purged from the git history in the same pass. No successor copy exists.

## Repair and improvement plans

| Document | Status and purpose |
| --- | --- |
| [`hKask core and MCP repair plan`](plans/hkask-core-mcp-repair-improvement-plan.md) | Active, operator-authorized repair plan; no backward-compatibility requirements. §9 separates verified gallery/packaging slices from unfinished authority, cancellation, containment, attribution and recovery work; P6 separates skill evaluation (algedonic review with operator and Curator) from skill execution; P8 records the experimentation program's execution records — the reference model is promoted to [`research/experimentation-reference-model.md`](research/experimentation-reference-model.md) (2026-10-05) with the locked operator decisions. |
| [`Cybernetic nervous system alignment plan`](plans/cybernetic-nervous-system-alignment.md) | Active — reference model admitted (operator ruling 2026-09-30); S1–S6 recorded; the §8 legacy/orphan cleanup program (CU-1–CU-8) with CU-1–CU-5/CU-7 complete and CU-6 gated on displacement; register row L25 classified (2026-10-01). Carries the measurable target condition, the executed live-log PDCA experiment, the ranked subtractive steps S1–S6, and the whole-file pricing menu for the 60-file docs target (sixth slice: P1 ruled 2026-10-01, P2 executed 2026-10-05, P3 ruled retain 2026-10-05). |
| [`Eureka-cycle implementation plan`](plans/eureka-cycle-implementation-plan.md) | Active — operator-authorized 2026-10-23. T0–T3 executed (goal `9c5af814`: the board, the rehearsal probe corroborated, the skill + templates authored with gates green; EUREKA-1 resolved corroborated with the pre-push wiring fix and its EUREKA-2/EUREKA-3 follow-ups closed; the memory-candidate machinery was removed 2026-10-24 — lesson capture is the memory architecture's automatic thread distillation, per the plan's §3.1 correction). The standing human-chaired discovery loop: an Open Questions kanban board plus a composing skill routing each funded question to `hypothesis-testing` (scientific), `self-improvement` (artifact) or a research run, with mandatory prior-experiment recall, a PI funding gate, sealed pre-registered experiments, and a grounding-verified study report. Load-bearing hypotheses H1–H5 with discriminating tests; produced through coding-guidelines, refactor-architecture, grill-me, essentialist, pragmatic-cybernetics, pragmatic-semantics, hypothesis-framer and falsifiability. |

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
| [`reference/mcp-servers/README.md`](reference/mcp-servers/README.md) | MCP server registry — 9 built-in servers, 343 registered tools fleet-wide, and the fleet tool surface. |
| [`reference/mcp-servers/corpus.md`](reference/mcp-servers/corpus.md) | Corpus server — gather → process → output pipeline (25 tools). |
| [`reference/mcp-servers/curator.md`](reference/mcp-servers/curator.md) | Curator server — sovereign memory, regulation readback, and the algedonic log (15 tools). |
| [`reference/mcp-servers/experimentation.md`](reference/mcp-servers/experimentation.md) | Experimentation server — experiment registry for the sharded experimentation program (8 tools). |
| [`reference/mcp-servers/kata-kanban.md`](reference/mcp-servers/kata-kanban.md) | Kata-kanban server — Toyota Kata task boards and persistent-until-resolved functional goals (27 tools). |
| [`reference/mcp-servers/media.md`](reference/mcp-servers/media.md) | Media server — research-evidence ingestion, gallery, transcription, educt, jobs, and workflows. |

| [`reference/mcp-servers/prediction-markets.md`](reference/mcp-servers/prediction-markets.md) | Prediction-markets server — Polymarket/Kalshi calibration and economic data. |
| [`reference/mcp-servers/research.md`](reference/mcp-servers/research.md) | Research server — web/RSS retrieval, evidence scoring, run ledger, and paper identity. |
| [`reference/mcp-servers/scenarios.md`](reference/mcp-servers/scenarios.md) | Scenarios server — Schwartz/Tetlock pipeline (21 tools; the provider-free forecast loop moved here from the removed companies server). |
| [`reference/mcp-servers/swarm.md`](reference/mcp-servers/swarm.md) | Swarm server — local swarm agents and local swarms (37 tools). |
| [`reference/skills/README.md`](reference/skills/README.md) | Registry of 59 skills, 55 template namespaces, and 222 `.j2` resources. |
| [`loop-register.md`](loop-register.md) | Loop audit register — 25 system loops (L24 the experimentation protocol, L25 experimentation-health sensing, added 2026-09-30/10-01) with audit verdicts, three-layer (A/B/C) classifications with per-row sense→report→actuate maps and INV1–INV6 alignment verdicts (pass 3), deferred operator decisions, and the change-log ledger. |
| [`reference/kask-settings.md`](reference/kask-settings.md) | Kask settings and environment reference. |
| [`reference/lisp-eval-dialect.md`](reference/lisp-eval-dialect.md) | Sandboxed `lisp_eval` dialect and `form`/`env` contract. |
| [`reference/ontology-bridge.md`](reference/ontology-bridge.md) | Published-vocabulary bridge, exact term resolver, and `onto_anchor` ladder. |
| [`reference/native-ontologies.md`](reference/native-ontologies.md) | Native-ontologies inventory — every natively-available vocabulary (248 pinned files across 20 source directories), each one's prefixes, version, license and domain, live-probed example resolutions, the 91-ruling derived registry, and the ladder tiers in functional terms; companion to `ontology-bridge.md` (API) and `research/software-ontology-adoption.md` (decision trail). Added 2026-10-13. |
| [`reference/upstream-rebase-process.md`](reference/upstream-rebase-process.md) | Upstream rebase process and removal principles for the numbered D-seams. |

## Diagrams

| Document | Description |
| --- | --- |
| [`DIAGRAMS_INDEX.md`](DIAGRAMS_INDEX.md) | Registry of 95 current `DIAGRAM_ALIGNMENT` records representing 95 unique IDs; 25 records are in the five consolidated diagram files and 70 are inline. |
| [`diagrams/architecture.md`](diagrams/architecture.md) | Cross-cutting architecture diagrams. |
| [`diagrams/kanban.md`](diagrams/kanban.md) | Kata-kanban state machines: task status, move controller, and the goal lifecycle (create → progress → human closure → assessment → current recall). |
| [`diagrams/swarm.md`](diagrams/swarm.md) | Swarm architecture and control loops. |
| [`diagrams/ui-widgets.md`](diagrams/ui-widgets.md) | Native UI-widget structures. |
| [`diagrams/mcp-dispatch.md`](diagrams/mcp-dispatch.md) | MCP runtime and tool-dispatch flows. |

## Research

| Document | Description |
| --- | --- |
| [`research/chunking-for-rag-research.md`](research/chunking-for-rag-research.md) | Prior-art study of RAG text-chunking patterns and reference models, and the corpus pipeline's alignment and gaps against them. Recommendations only — not implemented. |
| [`research/vector-store-deduplication-scope.md`](research/vector-store-deduplication-scope.md) | Scope and decision record for removing the duplicated vector (`embeddings.vector` vs vec0's `embedding`, 1.03 GiB on the fine index). Falsifies the documented auxiliary-column "escape hatch" against the sqlite-vec 0.1.9 source, names the change boundary including the admission column contract and a format break, and recommends a two-phase migration. Scoped, not implemented — decision requested. Added 2026-10-18. |
| [`research/kanban-board-reference-models.md`](research/kanban-board-reference-models.md) | Reference models for kanban board naming/navigation (Wekan, Planka, Kan, Kanboard), the kata-kanban's alignment and gaps against them, and the implemented shaping/test plan. |

| [`research/syntax-semantic-probabilistic-deterministic-space.md`](research/syntax-semantic-probabilistic-deterministic-space.md) | The syntax–semantic × probabilistic–deterministic 2×2 region space — the `region-routing` skill's recorded reference model: framing, prior art, an empirical micro-test, the navigable map, the C1–C15 claim ledger, and the folded three-party collaboration overlay (§4). |
| [`research/d-seam-audit.md`](research/d-seam-audit.md) | D-seam audit ledger — resumable per-seam worklist over the live `DIVERGENCE.md` surface; identity coverage checked by `kask/scripts/check-d-seam-audit-ledger.sh`. |
| [`research/two-symptom-latency-findings.md`](research/two-symptom-latency-findings.md) | Two-symptom latency investigation (thread-load latency; input-tracking freeze) — findings report, hypothesis register, ranked plan, and rounds 2–3 (binary provenance, parity rebuild, hang-evidence durability via D89); the P7g superseding record. Added 2026-10-01. |
| [`research/cybernetic-nervous-system-reference-model.md`](research/cybernetic-nervous-system-reference-model.md) | Reference model (admitted by operator ruling 2026-09-30): the mammal-general afferent/efferent + expectation/prediction-error + reflexive/sensorimotor/deliberative structure, its mapping to loop layers A/B/C with named invariants INV1–INV6, and the surprise-gated logging principle with its information-efficiency argument. All citations resolved (research run 4b9a3b3ed05bf860; 0 resolution failures). Added 2026-09-30. |
| [`research/experimentation-reference-model.md`](research/experimentation-reference-model.md) | The experimentation program's reference model (promoted from the repair plan §P8, 2026-10-05): the vary-locally/select-centrally/retain-durably pattern, the shard map, the generalized experiment protocol, guardrails and meta-metrics with research anchors, the locked operator rulings (Q0–Q7), and the registry's CNS connection map. The execution records remain in the plan §P8. |
| [`research/adc-targeting-carrier-pilot-2026-10-18.md`](research/adc-targeting-carrier-pilot-2026-10-18.md) | Real functional trial of `experimental-design` / `hypothesis-testing`: the operator's ADC targeting-vs-carrier/AUC hypothesis normalized into four rivals; candidate designs gated; the confirmatory factorial bound to a human executor; Hamblett 2004 reanalysed deterministically; outcome `inconclusive`; registry schema mismatch and audit blockage recorded without fake execution. |
| [`research/tree-sitter-adoption-proposal.md`](research/tree-sitter-adoption-proposal.md) | Tree-sitter code-graph capability research and adoption proposal: the partially-confirmed underuse premise (0 structural instruments kask-side vs 131 upstream `tree_sitter::` refs), the 7-capability cited matrix with the parser-not-graph-engine boundary, essentialist-screened use cases (3 survive, 2 rejected, 1 deferred), and the landed structural layer (pinned ast-grep instrument, unwrap ratchet gate over 468 baselined instances, symbol-anchored citation re-anchoring) with measured experiment outcomes — one pre-registered claim falsified as stated, corrected, and reported. Accepted 2026-10-11; the four measured follow-ups landed the same day: widened resolver kinds + impl-qualified find (86% overall / 94% anchorable citation survival), the docs-citation ladder gate (0 rotted — the "671 rotted" reading was false absence; 9 line-drift citations re-pointed across 6 docs), and the `Result<_, String>` structural rule replacing the string-errors gate's line-based matching. |
| [`research/install-build-ci-reference-models.md`](research/install-build-ci-reference-models.md) | Install, build, and CI scripts — reference models from published practice (rustup, Homebrew, nix-installer, Google Shell Style Guide, ShellCheck), the 2026-10-11 pre-fix audit (F1–F7, every one fixed in the same change), the landed corpus-bundle install stage (up-to-date gate, physical resume, atomic assembly), and the conformance gate + selftest that pin the practice. Added 2026-10-11. |
| [`research/software-ontology-adoption.md`](research/software-ontology-adoption.md) | Software ontology adoption — rung-1 and rung-2 research record: the five load-bearing terms (inference, service level agreement, fine-tuning, data retention, audit) resolved through the vendored DPV 2.3 and CSO 3.5 pins, the per-candidate licensing research mapped onto the zk-ref-open allowlist posture, the five rulings with their authorities, and the honest open register (SEON deferred, Skillware pinned but not yet cited). Added 2026-10-13. |
| [`research/ontology-anchoring-review.md`](research/ontology-anchoring-review.md) | Ontological anchoring review (task `49ed2320`) — the anchoring hierarchy and multi-domain chunk-linking methodology reviewed: ladder-tier coverage, the derived-registry discipline, and the review's findings over the pinned vocabularies. Added 2026-10-13. |
| [`research/ontology-open-loops-resolution.md`](research/ontology-open-loops-resolution.md) | Ontology open loops — resolution paths (decision-ready proposal): the seven open loops from the anchoring review, each with proposed resolution path, effort, and the operator decision it awaits. Added 2026-10-13. |
| [`research/media-server-lead-onboarding.md`](research/media-server-lead-onboarding.md) | Media MCP server lead-developer onboarding — tree-grounded orientation for the media server: where everything lives and what to do first, every load-bearing claim cited. |

## Document lifecycle ledger

Git history is the archive of record. Every removed document names its active successor here. The dated Corpus size measurement above, not this lifecycle ledger, applies the document-count gate (all files, per [`DOCUMENTATION_STANDARDS.md`](architecture/DOCUMENTATION_STANDARDS.md) §3) and identifies the live YAML inventory.

### Deleted 2026-10-26 (transcript-bench implementation plan — executed to termination, house lifecycle)

| Artifact | Successor |
| --- | --- |
| `plans/transcript-bench-implementation-plan-2026-10-25.md` | The executed bench it specified is the live record: the media panel's Transcript tab is the direct transcript workspace — recordings browse (with a back-to-browser affordance), synced reading with word-click seek and follow-along, two-click select → label editor (typed or pasted labels) → researcher-highlight create/rename/delete, provenance-keyed reel compose+render (`reference/mcp-servers/media.md` § Media panel — transcript surface, function-cited). Delivered under goal `4841c444-5c11-4e6d-a0be-a8785d561316` on the "Transcript bench" board: S1–S7 across commits `18ad4e3f37` (S1 Detail), `d6a53e1213` + `cb88358e4f` (S2 seek + fixture), `3d1e88d32e` (invoker lock + CI gate), `57230722b0` + `da32b38557` + `36505c90f7` (S3 workspace), `38086ac772` (S6 browser), `adfba1d9e9` + `6d1d65f7ac` + `baf651f456` + `29f6399133` (S4 highlight CRUD + label editor), `7412afcdba` (wrap fix), `ab75671fe1` + `cfa4679064` (S5 reels), and `6fb45d938d` + `6c889f4296` (S7 docs + tombstone), with four live-verified media-server repairs under the plan's §1 operator-authorized amendment (`6c59e731c4` pass-model default, `9796279f80` highlight index map, `2897646c90` STT preconditioning, `14c6b1547c` paragraph windows); the 2026-10-26 review pass closed its findings across `1af61160fe` (server), `1d755630a0` (widget), and `ddcdb6b87a` (panel). An earlier revision of this entry cited hashes that never existed as git objects — corrected 2026-10-26 against `git cat-file` (the fabricated-hash trap). The pre-collapse history named above was erased by the 2026-10-27 history collapse (see the corpus-header disclosure); this entry and the seam register carry the durable record. |

### Deleted 2026-10-25 (executed point-in-time removal plans — house lifecycle)

| Artifact | Successor |
| --- | --- |
| `plans/reduct-removal-plan-2026-10-22.md` | The executed removal it specified is the live record: the 20 `reduct_*` cloud tools, `tools/reduct.rs`, the `REDUCT_API_KEY` descriptor and the provider profile are gone; `kask_bridge::purge_removed_reduct_api_key` purges the dormant keychain slot at startup (DIVERGENCE.md D5; `reference/mcp-servers/media.md` § Redesign record's removed-surface note). Git history holds the full plan and its decision log. |
| `plans/abw-removal-plan-2026-10-22.md` | The executed removal it specified is the live record: the ABW cloud backend and its credential chain are gone, the swarm server's local mode is the surviving surface (`reference/mcp-servers/swarm.md`), and `kask_bridge::purge_removed_abw_api_key` purges the dormant keychain slot at startup (DIVERGENCE.md D5, Decision A). Git history holds the full plan. |

### Deleted 2026-10-19 (companies-removal docs reconciliation — executed plan, successor recorded)

| Artifact | Successor |
| --- | --- |
| `plans/companies-fmp-eodhd-fibo-removal.md` | The landed surface it specified — the ported forecast tools and their behavior tests in `kask/mcp-servers/hkask-mcp-scenarios/src/equity_forecast.rs` and `tests/equity_forecast_port.rs` (port commit `4f13f108da`; removal commits `3439db5e00` server+FMP/EODHD, `3adc75c9e1` FIBO rung, `6af8ab2e18` company-research skills); the executed five operator decisions (§5) are visible as: N2 the `scenario_risk_measure` wrapper over caller-supplied returns (documented in [`reference/mcp-servers/scenarios.md`](reference/mcp-servers/scenarios.md) § Design model), and the FIBO-anchor replacement resolving through the `qb:`/SUMO fallbacks in [`reference/ontology-bridge.md`](reference/ontology-bridge.md). Git history holds the full plan and its §5 decision log. |

### Deleted 2026-10-17 (training capability removal — out of scope for the co-researcher mandate)

| Artifact | Successor |
| --- | --- |
| `reference/mcp-servers/training.md` | [`reference/mcp-servers/README.md`](reference/mcp-servers/README.md) — the server registry (the training row is removed with the server). |
| `reference/lora-training-catalog.md` | No successor — the capability it catalogued (the training MCP server, the `lora-training` skill and its templates, the Nebius/HuggingFace data services, and the LoRA-adapter inference path) was removed end-to-end by operator ruling: model training is outside the co-researcher mandate. Git history holds the catalog, its nine-tool surface and the G-M1..G-Q5 gate definitions. |

### Deleted 2026-10-16 (docs-consolidation pass — implemented design and superseded prior-art study)

| Artifact | Successor |
| --- | --- |
| `research/agentic-scientist-v3-integration.md` | The landed surface it specified: the experimentation server's rubric block, variant roles, `experiment_seal`, and `final_evaluation_record`, plus the functionally named `hypothesis-testing` lifecycle and `experimental-design` deep module; the former `co-scientist` name/direct-design path was retired 2026-10-18. The D1–D4 operator rulings live in the derived ontology registry (commit `cfe8ecb654`); git history holds the full proposal and its §6 decision log. |
| `research/deterministic-vs-probabilistic-compute-routing.md` | [`research/syntax-semantic-probabilistic-deterministic-space.md`](research/syntax-semantic-probabilistic-deterministic-space.md) — the region-routing skill's recorded reference model (its [R1] prior-art citation repointed); the ratified routing claim lives on as P8.4 and in the derived-ontology rulings; git history holds the hypotheses study. |

### Deleted 2026-10-15 (spreadsheet capability removal — Tranche S)

| Artifact | Successor |
| --- | --- |
| `plans/logisheets-spreadsheet-capability-plan.md` | No successor — the capability (server, engine, widget, fenced language, both presentation options) was removed end-to-end by operator ruling; git history holds the plan and its SP-01..SP-12 execution record. |
| `reference/mcp-servers/spreadsheet.md` | [`reference/mcp-servers/README.md`](reference/mcp-servers/README.md) — the server registry (the spreadsheet row is removed with the server). |

### Deleted 2026-10-13 (doc-update condensation — the point-in-time curiosity study; no capability, no consumer)

| Artifact | Successor |
| --- | --- |
| `research/artificial-curiosity-capability-space.md` | Git history (the study, the seven-source follow-up, and the renderer-only pilot — the 2026-09-23 fold's full content — remain at `aa7f6ce4eb` and prior). The study's own verdict stood: **no distinct curiosity skill** — the analyzed skill set was consolidated out 2026-09-24, no curiosity-selection capability exists in the tree (verified by search 2026-09-28), and zero docs, skills, or scripts linked the record. The one standing decision survives here: promote a curiosity-probe capability only after it wins a repeatable, uniquely valuable input/output contract in a cost-matched test. |

### Consolidated 2026-10-13 (media-server redesign set — doc-update condensation, the scenarios/spreadsheet precedent)

| Artifact | Successor |
| --- | --- |
| `research/media-server-redesign-review.md` | [`reference/mcp-servers/media.md`](reference/mcp-servers/media.md) § Redesign record (method summary — the four verified reference models, the FINER-gated phase verdicts, the 39-primitive/8-family model reconciled to the 102-tool pin); git history (`1a8c500c12`..`42f885b0e3`) holds the full review. |
| `research/media-server-redesign-findings.md` | [`reference/mcp-servers/media.md`](reference/mcp-servers/media.md) § Redesign record (the five target conditions MF-1…MF-5 with closure commits, the depth-pass closure MF-9…MF-15, the Phase-4 two-oracle verdicts and the Lean-deferred trigger); the per-tranche line-by-line audit narrative remains in git history. |
| `research/media-server-redesign-proposals.md` | The landed proposals are the code and its pins (PR-M1 the `media_limits` imports, PR-M2 `collage_publishes_through_canonical_storage`, PR-S1 the skill line `fb22b0b4d5`); the standing decisions and receipts are recorded in the Redesign record. |
| `research/media-server-redesign-improvement-plan.md` | [`reference/mcp-servers/media.md`](reference/mcp-servers/media.md) § Redesign record (the execution status — all slices DONE/CLOSED — the commit ledger, and the live receipts verified in the current tree); git history holds the slice-by-slice narrative. |

### Folded 2026-10-13 (gallery docs recomposition — C1 follow-up; the gallery store code moved to the media server in commit `c85b1e4896`)

| Artifact | Successor |
| --- | --- |
| `diataxis/hkask-storage/{explanation,reference}.md` § Gallery sections | [`reference/mcp-servers/media.md`](reference/mcp-servers/media.md) § Gallery store identity and reconciliation + § Gallery store entities (the store's sole-consumer doc; diagrams re-registered `DIAG-STOR-004`→`DIAG-RF-007`, `DIAG-STOR-006`→`DIAG-RF-008`); the storage set describes only what `hkask-storage` ships. |

### Deleted 2026-10-11 (test-suite refresh — the working plan, executed to termination)

| Artifact | Successor |
| --- | --- |
| `plans/test-suite-refresh-plan.md` | The saved final report (`report_load` kind `report`, name `test-suite-refresh-final-report`) + the `test-suite-refresh` kanban board (33/33 verdicts with per-batch findings comments) + git history (`aee548085a`..`4791c5b913` holds the plan and its execution log; `2886a2daee` the D90 ledger registration the mini-batch surfaced). Review outcome: zero stale pins across 246 kask-side test files and 48 zed-side pin files; zero test modifications. |

### Consolidated 2026-10-07 (spreadsheet-capability redesign set — operator instruction)

| Artifact | Successor |
| --- | --- |
| `research/spreadsheet-capability-redesign-review.md` | `reference/mcp-servers/spreadsheet.md` § Redesign record (the method summary — FINER-gated PDCA phases, the 54-row coverage, the 16 cited findings, the Lean spec pin and structural checks); the live state is the tree. |
| `research/spreadsheet-capability-redesign-proposals.md` | The landed proposals are the code and its pins (the SP-01..SP-12 commit ledger in the Redesign record); the standing operator rulings (SP-05 complete, SP-06 enforce, SP-09 the Dublin Core/SUMO/PKO anchors) are recorded there. |
| `research/spreadsheet-capability-redesign-improvement-plan.md` | `reference/mcp-servers/spreadsheet.md` § Redesign record (the execution record — Remaining: none — and the live receipts); git history (`b252d88b3d`..`87b8673285`) holds the full execution narrative. |

### Consolidated 2026-10-07 (scenario redesign plan set — operator instruction)

| Artifact | Successor |
| --- | --- |
| `research/scenario-server-redesign-review.md` | `reference/mcp-servers/scenarios.md` § Redesign record (method summary, coverage receipts, the git-history commit range); the live coverage is the 19-tool surface itself. |
| `research/scenario-server-redesign-findings.md` | `reference/mcp-servers/scenarios.md` § Design model (the twelve primitives at their current state) and § Redesign record (the accepted-limitation rulings); `kask/lean/event_tree_marginalization.lean` (the formal pin, cited from the reference's Formal specification section). |
| `research/scenario-server-redesign-proposals.md` | The landed proposals are the code and its pins (commit ledger in the Redesign record); the two standing decisions (PR-13 remove — executed; PR-15 deferred) are recorded in the Redesign record. |
| `research/scenario-server-redesign-improvement-plan.md` | `reference/mcp-servers/scenarios.md` § Redesign record (decisions, commit ledger, live receipts, the goal-closure batch-record note); git history (`8692a23520`..`e677993804`) holds the full execution narrative. |

### Deleted 2026-10-05 (docs deep-alignment — the closed guardrails plan; the P2 media-onboarding deletion was reversed by operator ruling 2026-10-06 and the record restored)

| Artifact | Successor |
| --- | --- |
| `plans/agent-loop-guardrails-plan.md` | `DIVERGENCE.md`'s tool-retry row (the shipped C2 stop-loss contract, its design revision, and every pin name) + `kask/lean/repetition_guard_spec.lean` (the machine-checked contract) + the `lisp_eval` teaching contract (pinned in hkask-lisp, documented in `.rules`); git history holds the full kata record. Closed — executed and audited 2026-10-02; every §13 open question resolved. **Do not re-land C1a** (the removed reasoning-runaway watchdog) without a content-level discriminator (looping vs. progressing), a non-fatal recovery path, and a settings knob — the post-mortem is in git history. |

### Folded 2026-10-01 (P1 ruling — region-space research consolidation)

| Artifact | Successor |
| --- | --- |
| `research/navigating-the-region-space-collaboration.md` | `research/syntax-semantic-probabilistic-deterministic-space.md` §4 (the three-party collaboration overlay subsection — the durable model); the reification record lives in `.agents/skills/region-routing/SKILL.md` and the skills registry; git history is the session-record archive. |

### Deleted 2026-09-28 (doc-update realignment — reference absorbs how-to)

| Artifact | Successor |
| --- | --- |
| `diataxis/swarm_system/how-to.md` | `diataxis/swarm_system/reference.md` (Procedures section). |
| `diataxis/hkask-types/how-to.md` | `diataxis/hkask-types/reference.md` (Procedures section). |
| `diataxis/hkask-storage/how-to.md` | `diataxis/hkask-storage/reference.md` (Procedures section). |
| `diataxis/hkask-regulation/how-to.md` | `diataxis/hkask-regulation/reference.md` (Procedures section). |
| `diataxis/hkask-inference/how-to.md` | `diataxis/hkask-inference/reference.md` (Procedures section). |
| `diataxis/hkask-condenser/how-to.md` | `diataxis/hkask-condenser/reference.md` (Procedures section). |
| `diataxis/kask_bridge/how-to.md` | `diataxis/kask_bridge/reference.md` (Procedures section). |
| `diataxis/hkask-mcp-server/how-to.md` | `diataxis/hkask-mcp-server/reference.md` (Procedures section). |
| `diataxis/hkask-tool-port/explanation.md` | `diataxis/hkask-tool-port/reference.md` (capability-separation rationale and the invoke-pipeline/boundary diagrams folded in). |
| `diataxis/hkask-bridge-ontology/` (empty directory) | Residue of the 2026-09-09 how-to fold; no files. |
| `reports/` (empty directory) | Residue of the 2026-09-17/19 report deletions; no files. |

### Folded 2026-09-23 (research and pilot evidence)

| Artifact | Successor |
| --- | --- |
| `research/artificial-curiosity-open-questions-followup.md` | `research/artificial-curiosity-capability-space.md` §§6, 8: distinct seven-source follow-up, limits and four-arm experiment design; full original remains in git at `aa7f6ce4eb`. |
| `research/artificial-curiosity-probe-pilot.md` | `research/artificial-curiosity-capability-space.md` §9: four positive/negative renderer controls, measured boundary and unresolved experiment; full original remains in git at `aa7f6ce4eb`. |

### Deleted 2026-08-28 (condensation — no formal role / stale / duplicative)

| Artifact | Successor |
| --- | --- |
| `plans/` (10 files) | Point-in-time plans and implemented designs: swarm plans → `diataxis/swarm_system/`; fact-checking design → `grounding-verify`/`falsifiability`/`hypothesis-framer` skills; memory plans → `architecture/memory-system-specification.md`; other completed or abandoned work → git history. |
| `explanation/` (13 files) | Durable content folded into `architecture/skills-and-composition.md`, `architecture/memory-system-specification.md`, `reference/mcp-servers/README.md`, and surviving Diataxis/reference coverage. |
| `architecture/AGENT_SYSTEM_PROMPT.md` | `architecture/skills-and-composition.md` Part I. |
| `architecture/memory-system-and-therapy.md` | `architecture/memory-system-specification.md` §§9–11. |
| `architecture/salience-specification.md` | Implemented `word_frequencies` surface in `diataxis/hkask-condenser/reference.md`. |
| `architecture/hkask-types-core-domain-split.md` | No accepted successor architecture; `hkask-types` remains one crate, with current coverage in `diataxis/hkask-types/`. |
| `architecture/core/scenarios-companies-bridge.md` | `reference/mcp-servers/scenarios.md` (the companies server was removed 2026-10-18, commit `3439db5e00`; its provider-free forecast loop lives in the scenarios server). |
| `diagrams/` (25 of 28 files) | Five consolidated files under `diagrams/`; three memory diagrams moved into `architecture/memory-system-specification.md`. |
| `REFRESH_TRIAGE.md` | This lifecycle ledger. |

### Deleted 2026-09-09 (doc-update realignment)

| Artifact | Successor |
| --- | --- |
| `architecture/research-server-capability-plan.md` | Implemented record in `reference/mcp-servers/research.md` under Capability adoption record. |
| `reference/README.md` | This portal's Reference table. |
| `reference/upstream-removal-principles.md` | `reference/upstream-rebase-process.md` §9. |
| `diataxis/hkask-bridge-ontology/how-to.md` | `reference/ontology-bridge.md` under How to use the bridge and term resolver. |

### Deleted 2026-09-16 (tutorial folds and implemented inference plan)

The eight tutorial removals landed in `5881f1f406fafe12f4fc65a549dbf1eb0a62ca84`; each successor below is the retained task-oriented document for that set. The implemented inference plan was removed in `5322725bdbaa067580943c2bf33634c3640d8341`.

| Deleted artifact | Named successor |
| --- | --- |
| `diataxis/hkask-condenser/tutorial.md` | `diataxis/hkask-condenser/how-to.md` |
| `diataxis/hkask-inference/tutorial.md` | `diataxis/hkask-inference/how-to.md` |
| `diataxis/hkask-regulation/tutorial.md` | `diataxis/hkask-regulation/how-to.md` |
| `diataxis/hkask-storage/tutorial.md` | `diataxis/hkask-storage/how-to.md` |
| `diataxis/hkask-tool-port/tutorial.md` | `diataxis/hkask-tool-port/reference.md` |
| `diataxis/hkask-types/tutorial.md` | `diataxis/hkask-types/how-to.md` |
| `diataxis/kask_bridge/tutorial.md` | `diataxis/kask_bridge/how-to.md` |
| `diataxis/swarm_system/tutorial.md` | `diataxis/swarm_system/how-to.md` |
| `plans/inference-regulation-loop-completion-plan.md` | Implemented inference-resilience behavior in `kask/crates/kask_bridge/src/inference_resilience.rs` and current operational explanation in `kask/docs/diataxis/hkask-regulation/explanation.md`. |

### Deleted 2026-09-17 (point-in-time prediction-market reports)

| Deleted artifact | Named successor |
| --- | --- |
| `reports/prediction-markets/` (5 files) | Current implementation and tool surface in `reference/mcp-servers/prediction-markets.md`; git history remains the research archive. |
| `reports/gemba-loop-specification.md` | Active procedure in `.agents/skills/algedonic-review/SKILL.md` (its gemba walk phase; `gemba-walk` merged into it 2026-09-24); git history remains the design archive. |

### Deleted 2026-09-19 (doc-update realignment)

| Deleted artifact | Named successor |
| --- | --- |
| `reports/bug-hunt-report.json` | The bug-hunt skill's trace-filesystem expedition report (`.agents/skills/bug-hunt/SKILL.md:95` writes reports to the run trace dir); git history (added `ed9be534cc`, updated `8c92966ca6`, converged `be80e66916`) remains the durable archive of the 2026-09-18/19 expeditions. The docs-tree copy violated the `DOCUMENTATION_STANDARDS.md` §6.2 location policy (no `reports/` class) and pushed the tree to the 70-file cap. |
| `plans/goedel-gap-closure-plan.md` | Completed plan consolidated out: R1 (evaluation correctness), R2 (bounded proofs — predicate core verified, harness set removed `5b4799bcad`), R3 (Rust inventory checker), R4 (advisory goal-traceability), and the R5 approved acceptance cycle are delivered in code and tests; the acceptance protocol and Kani budget conventions live in [`reference/testing-protocol.md`](reference/testing-protocol.md); the remaining authority, activation, and recovery work is carried by [`plans/hkask-core-mcp-repair-improvement-plan.md`](plans/hkask-core-mcp-repair-improvement-plan.md) (P2/P3); the full plan and execution record remain in git history. |

### Verification gate

- [x] Six-field metadata plus `mds_categories` is present on every active or operator-retained Proposed document.
- [x] Current-state Mermaid alignment and the Proposed conceptual exception satisfy `DOCUMENTATION_STANDARDS.md`.
- [x] Internal links target retained documents; the eight folded how-to documents' links were repointed to their set references (2026-09-28).
- [x] Diagram metadata has unique-ID/location registry parity (95 records, tree-verified 2026-10-19).
- [x] Edited citations use full repository-relative paths; 525 sampled citations resolved (2026-09-28), and the 2026-10-05 deep-alignment run re-verified every citation it touched against live code (the compaction entry points and pin sample, the d-seam DIVERGENCE rows, kanban R1, the 13 live tool-surface pin sites, `SignalMetric` at `signals.rs:14`) with the tree-wide relative-link sweep resolving.
- [x] Document count is 83, under the fewer-than-85 cap (measured 2026-10-27, `find kask/docs -type f | wc -l`): the 2026-10-27 speaker-identity plan pass added the ratified educt speaker-identity plan and its research-design assessment (+2, 81 → 83) with the corpus-size line repaired in the same change and the plan's §5 diagram registered (`DIAG-ARCH-SPEAKER-IDENT-001`); the 2026-10-26 transcript-bench execution pass delivered the label editor + rename (`29f6399133`), reconciled the media-panel transcript-surface citations, and deleted the executed transcript-bench plan (−1, tombstone below), reconciling the prior pass's tail arithmetic to the measured count; the 2026-10-25 relocation pass removed the ADC meta-study record from the tree (−1, 83 → 82), and the same day's operator-directed pass deleted its data-directory artifacts and purged its study files from the git history entirely; the 2026-10-22 removal wave added the ABW and Reduct removal plans (+2, 78 → 80); the 2026-10-23 capability pass added the eureka-cycle implementation plan (+1, 80 → 81) and reconciled the diagram registry; the 2026-10-23/24 affordance pass added the instrument-affordance experiment record (+1, 81 → 82), with the four header-less docs repaired to the §2 seven-field standard in the same pass; the 2026-10-24 denominator pass added the per-task adjudication record (+1, 82 → 83), its own header and the README counts repaired in the same change; the 2026-10-24 ADC-allocation framing pass added the meta-study design record (+1, 83 → 84 — fully removed on operator direction 2026-10-25: artifacts deleted, git history purged); the 2026-10-25 transcript-bench plan pass added the transcript-bench implementation plan (+1) and deleted the two executed 2026-10-22 removal plan artifacts (−2, tombstones below), 84 → 83, with the corpus-size line repaired in the same change and this checklist entry repaired in the follow-up gate fix. The 75-file working target is exceeded by eight — condensation direction, not a gate.

## See also

- [`DIVERGENCE.md`](../../DIVERGENCE.md) — authoritative numbered divergence manifest.
- [`diataxis/INDEX.md`](diataxis/INDEX.md) — retained Diataxis sets.
- [`DIAGRAMS_INDEX.md`](DIAGRAMS_INDEX.md) — Mermaid verification registry.
