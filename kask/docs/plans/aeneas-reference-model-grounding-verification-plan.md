---
title: "Aeneas Reference-Model Grounding and Rust Verification Plan"
audience: [developers, architects, agents, operators]
last_updated: 2026-09-24
version: "0.1.0"
status: "Proposed"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust]
---

# Ground reference models, then verify Rust with Aeneas

## Target, authority, and present limits

**Plan only; no production refactor or Aeneas adoption authorized by this document.** For each selected behavior, establish two independently checkable claims: (1) a reference model faithfully states an operator-approved requirement or sourced external method; (2) the *production* Rust implementation satisfies that model, subject to named tool and external-library assumptions. Aeneas may help with claim 2; it cannot approve claim 1, establish natural-language entailment, or certify Rust-to-Lean translation by a Lean exit alone.

The first candidate is corpus citation admission. `ground_row` matches nonempty quotations to identified canonical chunks and records a byte offset (`kask/mcp-servers/hkask-mcp-corpus/src/services/qa_grounding.rs:309-412`). An answer not exact within its own verified evidence remains `model_inference` (`:414-483`). The ingestion gate re-hashes and re-executes rows before permitting output/DB access (`:728-859`; `kask/mcp-servers/hkask-mcp-corpus/src/tools/corpus.rs:240-247,339`). Existing public-tool tests cover source mismatch, missing evidence, a valid-first/invalid-later batch, and model-mediated answers (`kask/mcp-servers/hkask-mcp-corpus/src/tools/corpus/ingest_tests.rs:651-790`). These facts are not a semantic-support certificate.

**Tool status at research time:** pinned Aeneas `557f7a15f3b75e18ddae224fc65db0807b70acdf`, Charon `62585970fc75f61d83c7898ff8dfdd7edaa3c073`, Lean 4.31.0. Charon extracted the original `ground_row`; Aeneas failed at Rust's `core::str::pattern::Pattern::Searcher<'a>`. A minimal `str.find(&str)` reproduced it on [open Aeneas issue #838](https://github.com/AeneasVerif/aeneas/issues/838#issuecomment-5809419040); no upstream remedy had appeared at the last check. A scratch byte-search alternative translated but was *not* production code and required a `str::as_bytes` model. Forecast `marginalize` failed on a float constant; `find_balanced_json` failed on loop early returns. Only generated `TaskStatus.next` passed a Lean ordering theorem, with `[propext, Quot.sound]` and no `sorryAx`; that finite example is not a proof of corpus admission. Experiment logs, hashes, and observed results: [`RESULTS.md`](/home/mdz-axolotl/.cache/zed-kask-aeneas.2HExCd/RESULTS.md). The separate [go/no-go record](/home/mdz-axolotl/.cache/zed-kask-aeneas.2HExCd/ADOPTION_GATE.md) is research evidence, not an adopted CI gate. Tool use follows the [pinned Aeneas README](https://raw.githubusercontent.com/AeneasVerif/aeneas/557f7a15f3b75e18ddae224fc65db0807b70acdf/README.md).

## Phase 1 — Ground the reference model before translation

Place the first expectation contract in the existing corpus domain specification; do **not** add a general manifest or copy the Rust algorithm into Lean. Use the contract fields already required by `kask/docs/reference/testing-protocol.md:33-59`: context/outcome, functional role, variables, allowed variation, falsifier/oracle, and authority. The operator approves functional meaning; an independent reviewer checks source citations and whether the proposed model overclaims them.

The corpus dossier must retain: (a) versioned original sources and their exact cited passages, source identity and digest; (b) which assertions are direct quotations, derived rules, or product decisions; (c) assumptions and non-goals; (d) reviewed counterexamples. Ground `str.find`'s first-match **byte** index and no-match behavior in [Rust's standard-library documentation](https://doc.rust-lang.org/std/primitive.str.html#method.find), pinning the Rust/toolchain version used for the implemented path. Ground the *product* decision in `kask/docs/reference/mcp-servers/corpus.md:291-325`: every cited quote must be source-identified and byte-exact, at least one citation is required, a paraphrased answer may ingest only as model-mediated, and semantic support remains a separate Stage 8/operator review. The original source's words are not automatically true, and exact quotation does not establish answer entailment. The `grounding-verify` skill distinguishes mechanical matches from completeness and reasoning quality (`.agents/skills/grounding-verify/SKILL.md`, “When NOT to Use”).

**Gate G1:** reviewers can falsify the contract using an empty quote, absent quote, wrong source, duplicate chunk reference, Unicode byte offset, forged row, paraphrase, and a valid row preceding an invalid row. Any disputed semantic or admission policy returns to the operator; do not silently redefine the requirement to make a proof easier.

## Phase 2 — Specify one independent Lean reference

Write the model against typed canonical chunks, evidence quotes, and byte sequences, *before* inspecting generated Lean. Proposed obligation: on admission, each cited quote is nonempty; its chunk reference identifies a unique classified source; its cited source agrees with that chunk; and its recorded offset denotes exact bytes inside the chunk. State first-occurrence behavior only if it is an approved part of the interface. Specify answer provenance separately: exact text within the row's own verified evidence may receive mechanical provenance; other answer text stays `model_inference`. Model missing/invalid data as explicit rejection, not an empty-success fallback.

Include a reference truth table and negative controls. Show that an alternative algorithm preserving the approved behavior can still satisfy the model; a line-by-line transcription of Rust is not independent grounding. No theorem here establishes whether prose is semantically supported. If multiple implementations later use the model, prove each against the same statement and its representation relation; do not assert pairwise equivalence where the specification allows several outputs.

**Gate G2:** a reviewer can explain each hypothesis and falsifier without reading `ground_row`; the model does not claim to parse all JSON, establish external-source truth, or authorize training/publication by itself.

## Phase 3 — Re-test Aeneas fit on unchanged production Rust

When upstream offers a supported fix or reviewed model, pin compatible Aeneas, Charon, Rust nightly, Lean and external-model revisions. In an isolated checkout, run Charon with the documented Aeneas preset rooted at the *actual* `ground_row`, then Aeneas's Lean backend. Reconcile translated function counts against the expected target: an exit-0 run that emits zero definitions is not success. Preserve the source commit and hashes of Rust source, `.llbc`, generated Lean, reference specification, and models. Inspect every generated external template and unsupported operation. A function represented by an `axiom`, `sorry`, or partial placeholder is not verified. Do not treat an uninterpreted `str.find` model as verification of substring matching; `--sysroot default` and the translation itself remain explicit trust boundaries.

**Gate G3:** nonempty generated Lean for unchanged production `ground_row`, no translation error or `sorry`, and no opaque assumption for the matching behavior under examination. Otherwise record **blocked** and retain existing Rust verification; do not refactor source just for translator compatibility.

## Phase 4 — Prove the model and close the runtime seam

In the pinned Lake environment, prove the exact generated function satisfies the independent contract under explicit chunk-uniqueness, source-identity, byte/UTF-8 and external-model assumptions. Cover successful, rejected, and model-mediated branches and any relevant termination/failure obligations. Record `lake build`/Lean command, diagnostics, `#print axioms` for the named theorem, and an expected-failing negative control. A proof about a pure translated function does **not** prove Charon/Aeneas correspondence, data authenticity, DB atomicity, or caller preconditions.

Retain public-tool tests to establish the integration path: `verify_grounding_gate` re-executes `ground_row` and `corpus_ingest_qa` calls the gate before writing. A controlled harmful change must be caught by the property/proof appropriate to it **and** the relevant tool-seam test. Recheck the complete current source after any edit or tool/model update rather than inheriting old proof status.

**Gate G4:** checked statement, approved axioms and external models, explicit negative controls, and observed production caller tests. Report each as separate evidence; a high grounding score or successful Lean exit cannot replace a missing stage.

## Refactor-architecture route and adoption decision

`ra-explore`/`ra-candidates`: retain the current grounding gate as the leading boundary. Inlining it would return hashing, identity reconciliation, re-execution and fail-closed admission to its callers (`qa_grounding.rs:728-859`); an “Aeneas adapter” that only forwards a filename would be a shallow wrapper. `ra-deepen` is **conditional on G3**: change an interface only if a production-used core genuinely makes callers simpler while hiding more verified invariant complexity. No proof-only twin, manual substring scanner solely for translatability, or unrequested change in paraphrase policy. If G3 or G4 fails, `ra-route` is **no Aeneas adoption for this boundary**; keep Rust examples, proptest and file-backed tool-seam tests as independent evidence (`kask/docs/reference/testing-protocol.md:61-130`).

After one useful pilot, compare demonstrated fault detection and maintenance with the existing tests before proposing a repeatable CI check. Only then consider other candidates: `TaskStatus.next` is a successful but low-leverage finite proof (`kask/crates/hkask-types/src/kanban_status.rs:79-88`); forecast marginalization requires an operator-approved independence assumption and faithful `f64` semantics (`kask/crates/hkask-forecast/src/hkask_forecast.rs:185-221`); JSON scanning currently lacks a complete Aeneas translation (`kask/crates/hkask-types/src/json_extract.rs:55-97`). Missing tool support, unreviewed library models, source/spec drift, excessive proof upkeep, or a model that assumes its target property are stop conditions, not reasons to weaken the reference.

**Ownership and closure:** the operator ratifies each model's functional authority; engineering owns the toolchain, proof, caller integration and regression evidence; an independent reviewer challenges citations/model assumptions. Keep open findings as blocked with owner and falsifier. A code/model/tool version change makes the associated correspondence unverified until rerun. No code, tests, build or proof was run while saving this plan; historical experiments above are explicitly separate.
