---
name: zk-ref-open-sync
description: "Synchronize the zk-ref-open reference-model corpus with codebase citation changes (expansions, edits, and contractions): codebase-to-corpus delta census, operator D10 licensing and commentary gate via zk-ref-license-gate.sh, incremental purge/re-embed/reseal over the calibration run, and Stage 11 bundle export/release verification."
---

# zk-ref-open Sync

Keep the `zk-ref-open` reference-model collection synchronized with the live
codebase as cited reference models expand, change, or contract. This skill is
a thin domain orchestrator: it owns the codebase-to-corpus delta census, D10
licensing and commentary routing, incremental contraction/re-embedding over the
authoring calibration run, and published-bundle verification. It composes
`build-corpus-pipeline` (Stage 1 conversion/OCR and Stage 11 bundle export) and
`kask/scripts/audit/zk-ref-license-gate.sh` without duplicating either.

## When to Use

- A new subsystem, MCP server, or skill adds reference-model citations to the
  codebase (e.g., Nostr NIPs / Blossom BUDs, new research docs under
  `kask/docs/research/*reference-models*.md`).
- An existing reference-model doc in the codebase is corrected or revised
  (citation fix, version bump) so `zk-ref-open` now serves stale text.
- A capability or reference model is retired/removed from the codebase and its
  passages must be contracted from `zk-ref-open` without dropping dual-purpose
  retained vocabularies.

## When NOT to Use

- Building or refreshing an arbitrary corpus from a caller-selected folder —
  use `build-corpus-pipeline`.
- Querying `zk-ref-open` for a research question — use `curator_federated_search`
  or `corpus_query`.
- Auditing `kask/docs` drift without updating `zk-ref-open` — use `doc-update`.

## Ontological anchors

- **Reference model** (`derived:reference_model`, ISO/IEC 7498-1:1994,
  ISO/IEC 10746-2 / RM-ODP, TOGAF, IFLA LRM; operator ruling 2026-11-02):
  defines the load-bearing convention that every capability, skill, doc, and
  D-seam cites a named published model, and anchors `zk-ref-open`'s
  license-compliant provenance form (`Reference model origin` + `Stands in for`
  own-words definitional commentary when the origin work is under publisher
  rights).
- **Collection** (`prov:Collection`, W3C PROV 2013; `bibo:Collection`): models
  `zk-ref-open` as a structured collection whose member documents can be added,
  modified, or subtracted across revisions without losing collection identity.
- **Document** (`crm:E31_Document`, CIDOC CRM 7.1.3; `bibo:Document`): anchors
  each canonical source file and its hash-bound representations.
- **Citation** (`cito:Citation`, CiTO 2026-09-03; `schema:citation`): anchors
  the directional link from citing codebase artifacts (`DIVERGENCE.md`,
  `SKILL.md`, `kask/docs/research/`, `derived.rs`) to cited reference models.
- **License** (`dcterms:license`, DCMI Metadata Terms; `sumo:License`): anchors
  the D10 allowlist enforced fail-closed by `zk-ref-license-gate.sh`.
- **Provenance** (`SEPIO:0000058`, SEPIO 2023-06-13; `dcterms:provenance`):
  anchors the pre-copy snapshot chain, raw/canonical SHA-256 pinning, and
  two-phase seal (`interim -> export -> final re-bind`).
- **Procedure** (`pko:Procedure`, PKO 2.0.0): separates this reusable sync
  procedure from each revision's execution record and measured counts.

## Initial and target condition

- **Initial condition:** the current shipped manifest at
  `kask/registry/zk-ref-open/bundle.json` (recording the active `run_id` and
  `expected_passage_count`), the authoring workspace at
  `~/Documents/zk-data/zk-reference-open/`, the run-record directory at
  `~/Documents/zk-data/corpus-mcp/run-records/zk-ref-open-2026-10-05/`, the
  calibration run directory at
  `~/Documents/zk-data/corpus-mcp/calibration/zk-ref-open-8b-2026-10-07/`, and
  an unmeasured codebase-to-corpus citation delta.
- **Target condition:** the `ZRS-1` convergence gate in `lisp_eval` returns
  `done` — `zk-ref-license-gate.sh` exits 0 on the updated license map and
  census, `corpus_embedding_inventory` reports `missing == 0` on the single
  shipped `fine.db` index (`fine-children.jsonl`) with provider-confirmed model
  identity, the sealed bundle is exported and registered in
  `kask/registry/zk-ref-open/bundle.json` (and `verify-published-bundle.sh`
  exits 0 when release upload is in scope), and live `curator_federated_search`
  probes confirm added/modified models retrieve and contracted models do not.

## Step types (D/P labelling)

| Step | Type | Oracle / critique |
|------|------|-------------------|
| 1 Census & delta scan | D + P | file/grep/hash comparison is D (`sha256sum`, `grep`); classification of `dual_purpose_retain` vs `contract` is P, critiqued by the operator in Step 2 |
| 1.5 Render `delta-plan` | D | `render_template` (`zk-ref-open-sync/delta-plan`) |
| 2 Operator scope & D10 license gate | P + D | operator approves scope and D10 license/commentary rulings (P); `bash kask/scripts/audit/zk-ref-license-gate.sh` enforces allowlist and census parity fail-closed (D) |
| 3 Pre-copy snapshot & source prep | D | `.pre-<revision>-<YYYY-MM-DD>` copies, `corpus_convert` (or `build-corpus-pipeline` Stage 1), `sha256sum`, `cmp` on spec/args accepted arrays |
| 4 Rebuild representations, purge delta & re-embed | D | `corpus_build_chunk_representations`, `corpus_purge_qa`, `purge-orphaned-vectors` (if orphans present), `corpus_embed`, `corpus_embedding_inventory` (`missing == 0` on `fine.db`) |
| 5 Seal, export & register | D | interim bind, `corpus_export_bundle` (`build-corpus-pipeline` Stage 11), final re-bind after export's read-write open, `kask/registry/zk-ref-open/bundle.json` update |
| 6 Verify & live recall probe | D + P | `zk-ref-license-gate.sh`, `corpus_embedding_inventory`, `verify-published-bundle.sh` (D); `curator_federated_search` retrieval relevance check (P, critiqued by exact `run_id` and source identity) |
| 7 Converge (`ZRS-1`) & render `sync-report` | D | `lisp_eval` gate `ZRS-1` (max 2 re-entries) + `render_template` (`zk-ref-open-sync/sync-report`) |

## PDCA Loop

```
Plan:   Step 1 — Census & Delta Scan         → Diff codebase citations vs zk-ref-open census; render delta-plan
Plan:   Step 2 — Operator & D10 License Gate → Operator approves scope & D10 class; zk-ref-license-gate.sh PASS
Do:     Step 3 — Snapshot & Source Prep      → Pre-copy snapshots; convert/pin raw & canonical SHA-256s
Do:     Step 4 — Rebuild, Purge & Re-embed   → Rebuild representations; purge modified/contracted refs; embed delta
Do:     Step 5 — Seal, Export & Register     → Interim bind → corpus_export_bundle → final re-bind → bundle.json & release
Check:  Step 6 — Verify & Live Recall Probe  → License gate, fine.db inventory, verify-published-bundle, live recall probes
Act:    Step 7 — Converge (ZRS-1) & Report   → lisp_eval ZRS-1 routes done / reenter-step-{2,4,5} / stop-and-report (max 2 re-entries)
```

## Instructions

### Step 0 — Prior context (persistence-grounded learning)

Before scanning, call `curator_memory_recall` with `entity: "kask:zkref-open"`
and read `kask/registry/zk-ref-open/bundle.json` to ground the active `run_id`,
`expected_passage_count`, and any deferred refresh items from the previous
revision.

If any MCP tool call fails during execution, call
`curator_report_skill_use_issue` with `skill_name: "zk-ref-open-sync"`,
`tool_name`, `step_ordinal`, `error`, and `failure_origin`, then halt or
re-enter per the gate — never silently bypass a failed tool.

### Step 1 — Census & Delta Scan (Plan)

1. Read the current `zk-ref-open` baseline:
   - `kask/registry/zk-ref-open/bundle.json`
   - `~/Documents/zk-data/corpus-mcp/run-records/zk-ref-open-2026-10-05/census-zkref-open-sha256*.txt` (use the active revision's census file matching `calibration-spec-zkref-open.json`, not an older snapshot)
   - `~/Documents/zk-data/corpus-mcp/run-records/zk-ref-open-2026-10-05/license-map-zkref-open.tsv`
   - `~/Documents/zk-data/corpus-mcp/run-records/zk-ref-open-2026-10-05/calibration-spec-zkref-open.json`
   - `~/Documents/zk-data/zk-reference-open/ACQUISITIONS.tsv`
2. Scan codebase reference-model surfaces for additions, edits, and removals:
   - `kask/docs/research/*reference-models*.md` (compare SHA-256 against
     `~/Documents/zk-data/zk-reference-open/sources/` to catch silent edits)
   - `.agents/skills/*/SKILL.md` (`Reference-models` / `Anchors` sections)
   - `DIVERGENCE.md` (new or retired D-seams naming design-authority docs)
   - `kask/crates/hkask-bridge-ontology/src/derived.rs` (new or updated
     `zk-ref-open` citations)
3. Classify every changed or missing reference model into one of four buckets:
   - `add`: cited in the live codebase, absent from `zk-ref-open`.
   - `modify`: present in `zk-ref-open`, but governing doc SHA-256 or cited
     content has changed.
   - `contract`: capability/model retired from the codebase with no remaining
     live consumer.
   - `dual_purpose_retain`: associated with a retired capability, but still
     consumed independently by a retained subsystem or ontology namespace (do
     NOT contract without explicit operator confirmation).
4. Render the operator approval table via `render_template`:

```json
{
  "template_ref": "zk-ref-open-sync/delta-plan",
  "context": {
    "revision_label": "<short-slug-YYYY-MM-DD>",
    "current_run_id": "<active run_id from bundle.json>",
    "additions": [
      {
        "source": "<filename>",
        "citing_surface": "<repo path or D-seam>",
        "proposed_license": "<D10 allowlist class or commentary_needed>",
        "acquisition_route": "<primary_open | own_words_commentary | local_research_doc>"
      }
    ],
    "modifications": [
      {
        "source": "<filename>",
        "reason": "<what changed>",
        "old_sha256": "<prefix>",
        "new_sha256": "<prefix>"
      }
    ],
    "contractions": [
      {
        "source": "<filename>",
        "reason": "<retired capability / doc removal>"
      }
    ],
    "dual_purpose_retentions": [
      {
        "source": "<filename>",
        "retained_consumer": "<live subsystem or ontology>"
      }
    ],
    "proposed_release_in_scope": true,
    "open_questions": ["<any ambiguous licensing or scope rulings>"]
  }
}
```

### Step 2 — Operator Scope & D10 License Gate (Plan)

1. Present the rendered `delta-plan` to the operator and wait for explicit
   approval of:
   - The exact `add`, `modify`, `contract`, and `dual_purpose_retain` sets.
   - The D10 license class for every new source (`public_domain`, `cc_open`,
     `arxiv_nonexclusive`, `w3c_ietf_open`, `open_project_docs`,
     `vendored_ontology_mirror`, `operator_owned`, `vatican_free_use`). For
     works under publisher rights (including NC/ND), author an
     operator-authorized own-words definitional commentary under
     `~/Documents/zk-data/zk-reference-open/commentary/` carrying the
     `Reference model origin` + `Stands in for` provenance header
     (`operator_owned`).
   - Whether this revision uploads to the `corpus-zk-ref-open` GitHub release
     now or defers upload (`release_in_scope`: true/false).
2. Update `license-map-zkref-open.tsv`, the active revision's census file
   (`census-zkref-open-sha256.<count>-with-manifest-<revision>.txt`), and
   `ACQUISITIONS.tsv` for the approved delta (removing map entries for
   contracted sources so no stale entries remain).
3. Run the fail-closed D10 license gate via `terminal` against the active
   revision's census file (never a stale Oct-18 snapshot):
   ```bash
   bash kask/scripts/audit/zk-ref-license-gate.sh \
     ~/Documents/zk-data/corpus-mcp/run-records/zk-ref-open-2026-10-05/license-map-zkref-open.tsv \
     ~/Documents/zk-data/corpus-mcp/run-records/zk-ref-open-2026-10-05/<active-census-sha256-file>
   ```
   Capture its exit code before any pipe. A non-zero exit blocks Step 3.

### Step 3 — Pre-copy Snapshot & Source Preparation (Do)

1. Before mutating any calibration artifact, create `.pre-<revision>-<YYYY-MM-DD>`
   snapshots in `~/Documents/zk-data/corpus-mcp/calibration/zk-ref-open-8b-2026-10-07/`
   and `~/Documents/zk-data/corpus-mcp/run-records/zk-ref-open-2026-10-05/`:
   - Move `representations/` to `representations.pre-<revision>-<YYYY-MM-DD>/`
     (`corpus_build_chunk_representations` requires `output_dir` not to exist).
   - Copy `queries.jsonl`, `run-identity.json`, `calibration-spec-zkref-open.json`,
     and `build-representations-arguments.json`.
2. For each approved `add` and `modify` source:
   - Place the source file in `~/Documents/zk-data/zk-reference-open/sources/`
     (and `reference-docs/` or `commentary/` per pool convention).
   - Run `corpus_convert` to produce the canonical extracted `.txt` and
     `.report.json` under
     `~/Documents/zk-data/corpus-mcp/extracted/zk-ref-open/`. For PDFs
     requiring OCR triage, delegate to `build-corpus-pipeline` Stage 1 via the
     `skill` tool.
   - Verify the extraction report's recorded input hash matches the pool copy's
     `sha256sum`.
3. Update both `calibration-spec-zkref-open.json` and the revision's
   `build-representations-arguments.<revision>.json` with the exact measured
   raw and canonical SHA-256s for `add`/`modify` entries and removals for
   `contract` entries. Confirm the accepted-sources arrays are byte-identical
   between spec and args (`cmp`) before proceeding.

### Step 4 — Rebuild Representations, Purge Delta & Re-embed (Do)

1. Call `corpus_build_chunk_representations` with the updated accepted-sources
   manifest into the fresh `representations/` directory. Reconcile against the
   `.pre-` snapshot: untouched sources must be byte-identical; record exact
   per-tier row deltas (`reference`, `current`, `fine`, `parents`).
2. For every `modify` and `contract` source, compute the lowercase hex encoding
   of its basename and purge its old rows from `fine.db` via `corpus_purge_qa`
   using the exact trailing-colon entity-ref prefix:
   - `corpus:zk-ref-open::fine:utf8-<hex-basename>:`
   Only `fine.db` is shipped for federated recall (`bundle.json` pins
   `index_name: "fine"`); routine refreshes do NOT maintain legacy calibration
   `reference.db` or `current.db` tiers. When contractions or large replacements
   leave interior orphan `vec0` rows, run
   `cargo run -p hkask-storage --bin purge-orphaned-vectors` — never raw
   `DELETE FROM vec_embeddings` via the `sqlcipher` CLI.
3. For every `add` and `modify` source, run `corpus_embed` over the delta rows
   from `representations/fine-children.jsonl` into `fine.db` and record
   provider-confirmed model identity and zero failures.
4. Run `corpus_embedding_inventory` on `fine.db` against
   `representations/fine-children.jsonl`. Require `missing == 0` and
   provider-confirmed model match (`complete == true`).

### Step 5 — Seal, Export & Register (Do)

1. Execute the two-phase single-index seal and export chain via
   `kask/scripts/audit/seal-single-index-run.sh`:
   - Interim bind of `run-identity.json` over the rebuilt `representations/`
     and updated `fine.db`:
     `bash kask/scripts/audit/seal-single-index-run.sh <run-dir> fine <confirmed-model> <revision-label> interim`
   - Delegate to `build-corpus-pipeline` Stage 11 (`corpus_export_bundle` with
     `source_id: "zk-ref-open"`, `display_name: "zk-reference-open corpus (fine)"`,
     `index_name: "fine"`, `bundle_key: "zkref-open-public-bundle-key-2026-10"`,
     fresh `output_dir`).
   - Final re-bind of `run-identity.json` AFTER `corpus_export_bundle`'s
     read-write open:
     `bash kask/scripts/audit/seal-single-index-run.sh <run-dir> fine <confirmed-model> <revision-label> final`
     Record both interim and final `run_id`s and the measured `expected_passage_count`.
2. Update `kask/registry/zk-ref-open/bundle.json` with the new `run_id`,
   `expected_passage_count`, and the three exported asset SHA-256s
   (`zk-ref-open.db.part-000`, `run-identity.json`,
   `representations-manifest.json`). Commit pathspec-only after checking the
   staged index (`git --no-optional-locks status`) when operator-authorized.
3. If `release_in_scope` is true, upload the exported assets to the
   `corpus-zk-ref-open` GitHub release (retryable last step — never gates local
   state, and never leaves stale parts behind).

### Step 6 — Verify & Live Recall Probe (Check)

Run every applicable oracle and capture its raw exit code before piping:
1. **License gate (D):** `bash kask/scripts/audit/zk-ref-license-gate.sh <license-map> <census>` → `license_rc`.
2. **Inventory gate (D):** `corpus_embedding_inventory` on `fine.db` against
   `representations/fine-children.jsonl` → `fine_complete` (`1` when complete,
   `0` otherwise) and `inventory_missing` (must be `0`).
3. **Published bundle gate (D):** when `release_in_scope` is true, run
   `bash kask/scripts/audit/verify-published-bundle.sh` → `bundle_rc` (`0`,
   `mismatched == 0`, `stale == 0`); when deferred locally, set `bundle_ok` to
   `1` only after verifying local `bundle.json` SHA-256s match the exported
   files and explicitly recording `release_status: "deferred"`.
4. **Live recall probes (P + D):** after clearing warm index / re-materializing
   the updated registration, run `curator_federated_search` queries for at
   least one added/modified reference model (must retrieve with the new
   `run_id` and expected source) and one contracted model if any (must NOT
   retrieve from `zk-ref-open`) → `recall_ok` (`1` or `0`).

### Step 7 — Converge (`ZRS-1`) & Report (Act)

Evaluate convergence deterministically in `lisp_eval` (`ZRS-1`):
- form: `(cond ((and (= license_rc 0) (= fine_complete 1) (= inventory_missing 0) (= bundle_ok 1) (= recall_ok 1)) (quote done)) ((>= reentries 2) (quote stop-and-report)) ((!= license_rc 0) (quote reenter-step-2)) ((or (!= fine_complete 1) (> inventory_missing 0)) (quote reenter-step-4)) (t (quote reenter-step-5)))`
- env: `{ "license_rc": <int>, "fine_complete": <0 or 1>, "inventory_missing": <int>, "bundle_ok": <0 or 1>, "recall_ok": <0 or 1>, "reentries": <0-based count> }`
- receipts:
  ```lisp-receipts
  {"gate": "ZRS-1", "case": "all-clean-done", "env": {"license_rc": 0, "fine_complete": 1, "inventory_missing": 0, "bundle_ok": 1, "recall_ok": 1, "reentries": 0}, "expected": "done"}
  {"gate": "ZRS-1", "case": "license-fail-reenters-step-2", "env": {"license_rc": 1, "fine_complete": 1, "inventory_missing": 0, "bundle_ok": 1, "recall_ok": 1, "reentries": 0}, "expected": "reenter-step-2"}
  {"gate": "ZRS-1", "case": "inventory-missing-reenters-step-4", "env": {"license_rc": 0, "fine_complete": 1, "inventory_missing": 4, "bundle_ok": 1, "recall_ok": 1, "reentries": 0}, "expected": "reenter-step-4"}
  {"gate": "ZRS-1", "case": "bundle-fail-reenters-step-5", "env": {"license_rc": 0, "fine_complete": 1, "inventory_missing": 0, "bundle_ok": 0, "recall_ok": 1, "reentries": 0}, "expected": "reenter-step-5"}
  {"gate": "ZRS-1", "case": "exhausted-stops-and-reports", "env": {"license_rc": 0, "fine_complete": 0, "inventory_missing": 1, "bundle_ok": 0, "recall_ok": 0, "reentries": 2}, "expected": "stop-and-report"}
  ```

Act on the verdict:
- `done`: render `zk-ref-open-sync/sync-report` using the copyable context
  below, save the completion record in the run-records directory, and record
  the revision summary in curator memory via `memory_insert` (`entity: "kask:zkref-open"`,
  `attribute: "<revision_label>_completed"`, `evidence_h_mem_id`: the `h_mem_id`
  recalled in Step 0 or an episodic turn ID from `curator_memory_extract`).
  **Cleanup & retention disposition:** delete any transient staging files or
  temporary `/tmp` logs created during this run; deliberately retain the
  current revision's `.pre-<revision>-<YYYY-MM-DD>` snapshots and exported
  `bundle-<revision>/` directory until the next revision supersedes them
  (rollback & audit provenance — log any deletions under `~/Documents/zk-data/`
  to `INDEX-moves-2026-09-24.log` per `standardized-artifact-storage.md`).
- `reenter-step-2`, `reenter-step-4`, or `reenter-step-5`: repair the named
  failure at that exact step, increment `reentries`, and re-run Step 6.
- `stop-and-report`: render `zk-ref-open-sync/sync-report` with
  `convergence_verdict: "stop-and-report"`, `final_run_id` omitted (if halted
  before Step 5 sealed a run), and `open_failures` listing every open failure,
  then stop for operator decision.

```json
{
  "template_ref": "zk-ref-open-sync/sync-report",
  "context": {
    "revision_label": "<short-slug-YYYY-MM-DD>",
    "prior_run_id": "<prior run_id>",
    "final_run_id": "<new sealed run_id; omit if halted before Step 5>",
    "delta_summary": {
      "added": 0,
      "modified": 0,
      "contracted": 0,
      "retained": 0
    },
    "license_gate_rc": 0,
    "inventory_summary": {
      "fine_complete": true,
      "missing": 0,
      "confirmed_model": "<provider-confirmed embedding model>"
    },
    "bundle_summary": {
      "expected_passage_count": 0,
      "registry_commit": "<commit sha or uncommitted>",
      "release_status": "<published_verified | deferred>"
    },
    "recall_probes": [
      {
        "query": "<probe query>",
        "expectation": "<retrieve | absent>",
        "observed": "<source + run_id or absent>",
        "passed": true
      }
    ],
    "pre_copy_snapshots": ["<path1>"],
    "open_failures": ["<only populated on stop-and-report; omit or [] on done>"],
    "convergence_verdict": "done"
  }
}
```

## Regression case

Execute all five `ZRS-1` cases above via `lisp_eval` (`all-clean-done` →
`done`, `license-fail-reenters-step-2` → `reenter-step-2`,
`inventory-missing-reenters-step-4` → `reenter-step-4`,
`bundle-fail-reenters-step-5` → `reenter-step-5`,
`exhausted-stops-and-reports` → `stop-and-report`). Render both
`zk-ref-open-sync/delta-plan` and `zk-ref-open-sync/sync-report` via
`render_template` and confirm zero contract errors and Jinja-computed totals.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `delta-plan.j2` | Render the codebase-to-corpus reference-model delta plan (additions, modifications, contractions, dual-purpose retentions) and D10 licensing decisions for operator approval before mutating zk-ref-open. Deterministic (no LLM call). |
| `sync-report.j2` | Render the zk-ref-open refresh completion report from measured delta counts, license gate, fine.db embedding inventory, seal run IDs, bundle manifest, and live recall probes. Deterministic (no LLM call). |

To render a template, call `render_template` with the ref:
- `zk-ref-open-sync/delta-plan`: `revision_label`, `current_run_id`, `additions`, `modifications`, `contractions`, `dual_purpose_retentions`, `proposed_release_in_scope` (optional), `open_questions` (optional)
- `zk-ref-open-sync/sync-report`: `revision_label`, `prior_run_id`, `final_run_id` (optional), `delta_summary`, `license_gate_rc`, `inventory_summary`, `bundle_summary`, `recall_probes`, `pre_copy_snapshots`, `open_failures` (optional), `convergence_verdict`

## Constraints

- **Never mutate without operator scope and D10 approval:** Step 2 is a hard
  operator gate; never auto-acquire, auto-contract, or auto-publish.
- **No NC/ND or unallowlisted primary sources:** `zk-ref-license-gate.sh` fails
  closed; publisher-rights works enter only as operator-authorized own-words
  definitional commentary (`Reference model origin` + `Stands in for`).
- **Preserve dual-purpose sources:** never contract a source solely because one
  capability was removed without checking other live consumers.
- **Never touch `vec_embeddings` with raw SQLite deletes:** use
  `corpus_purge_qa` and `purge-orphaned-vectors` (`hkask-storage`).
- **Always re-bind after export:** `corpus_export_bundle` opens `fine.db`
  read-write; the final seal runs after export, never only before.
- **Never duplicate `build-corpus-pipeline`:** delegate Stage 1 OCR/conversion
  and Stage 11 export/install behavior to `build-corpus-pipeline`.
- **Capture exit codes before pipes:** never pipe `zk-ref-license-gate.sh` or
  `verify-published-bundle.sh` directly into `tail`/`head` without saving `$?`.
