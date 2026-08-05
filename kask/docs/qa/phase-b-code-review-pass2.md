# Phase B — Code Review (Second Pass)

Date: 2026-08-04. Target: the diff range **`e1d2cc014e..HEAD`** — the 9 commits
that landed *after* the first pass's Phase B review scope (`origin/main..e1d2cc014e`),
which no reviewer had seen: `0a5372aaf3`…`e37bef8a3b` including the five
substantive fix commits (`34115403bf` redact_spans merge, `9c6e0a5020` audit
fixes, `bbc308d30f` form extraction + mapper promotion, `6051e7f50f`
per-variant classification, `2ae0e2953b` unsafe-forbid). ~1,233 insertions
across 48 code files.

## Verdict: behavior-preserving overall; 2 real classification regressions found and fixed; 1 deliberate behavior change flagged

## Findings

| # | Severity | Location | Description | Status |
|---|----------|----------|-------------|--------|
| B1 | Medium | `hkask-mcp-corpus/src/tools/document.rs:127` | `triage_pdf` blanket-mapped ALL `TriageError` variants to `invalid_argument` — but `PdftotextFailed` includes **spawn failures** (pdftotext binary missing = environment) and `PageCountMismatch` is an internal inconsistency. Environment failures blamed on the caller — the RR-0044 anti-pattern flipped in direction. **Regression** (was `internal`: imprecise but conservative). | **Fixed** — `map_triage_error` classifies all 4 variants explicitly (`61abea787f`) |
| B2 | Low | `hkask-mcp-training/src/tools/error_mapping.rs:88` | `JobStoreError::Storage(String)` → `unavailable` with a "transient — retry" doc comment; Storage wraps *any* rusqlite failure (schema error, corruption — persistent). Misleading retry signal, inconsistent with sibling `map_infra_error`. **Regression** (was `internal`). | **Fixed** — → `internal`, doc corrected (`61abea787f`) |
| B3 | Low | `hkask-mcp-corpus/src/tools/storage.rs:119-126` | `corpus_query_passages` now recovers a poisoned index lock via `into_inner()` and serves results from possibly-half-mutated state where it previously errored. Deliberate availability-over-consistency change; `tracing::warn!` present; the two sibling recovery sites are safe (immediate overwrite). | Accepted — flagged for a doc comment; not blocking |
| B4 | Nit | `hkask-guard/src/pipeline.rs:423` | `start <= *last_end` merges *adjacent* (touching, non-overlapping) spans: `[0,4)+[4,8)` emits one `[REDACTED]` instead of two. Byte coverage identical; hiding the boundary between abutting secrets is arguably safer. | Accepted — cosmetic, safe direction |
| B5 | Nit | `kask/deny.toml` | 16 new advisory ignores lacked the `Reviewed <date> — re-review <date>` cadence of the pre-existing entries; `RUSTSEC-2023-0071` (rsa Marvin) is a real vulnerability with no expiry on its ignore. | **Fixed** — block-level re-review date added |

## Explicit no-findings (what was checked and came back clean)

1. **swarm_panel extraction is byte-for-byte behavior-preserving.** The moved
   `AuthorForm`/`ComposeForm` code was diffed line-by-line: constructor
   defaults, all `cx.listener` handlers, all `cx.notify()` calls, element ids,
   tooltips, `disabled(busy)` states, `when_some`/`when` conditionals —
   identical. Only visibility (`pub(crate)`) and `mod`/`use` wiring changed.
2. **redact_spans edge cases probed**: adjacent spans (B4), spans past
   `text.len()` (clamp to `(len,len)`, spurious trailing marker, zero bytes
   lost — conservative), empty match vec (identity), non-secrets matches
   (filtered, preserved), cursor monotonicity (no slice panics). The two new
   unit tests pin exactly the out-of-order and suffix-leak cases — closing the
   first pass's F2 (test-may-not-exercise-claim) finding.
3. **Error mappers verified against the actual error enums**: `map_join_error`,
   `map_infra_error` (`#[non_exhaustive]`-justified catch-all),
   `map_gallery_store_error`, `map_embedding_error`, `map_service_error`
   (exhaustive over 5 variants), scenarios `ForecastError` (exhaustive, no `_`
   — compile-time re-classification forcing), corpus decode/parse split. No
   message leaks secrets; `PassphraseMismatch` embeds the DB path, not the
   passphrase.
4. **`.rules` compliance in the range**: no new `let _ =` on fallible ops, no
   production `.unwrap()`, no `unwrap_or(0)` on regulation signals, no
   panic-prone indexing, no tokio-in-`background_spawn`, no `AsyncApp` in
   `Send+Sync` impls.
5. **`classify_impl.rs` `disable_thinking`**: serde default legitimate here
   (`ClassifierYaml` IS the deserialized struct — not the dead-serde-default
   trap), threaded to both inference call sites, 4 pinning tests.
6. **`validation.rs` additions** (`map_join_error`/`map_infra_error`): correct
   variant logic, `#[must_use]`, exported at both paths, `tokio` dep used.
7. **CI additions** (`e37bef8a3b`): both new jobs reference existing scripts
   and pass locally — BUT the commit introduced **invalid YAML**: the step
   name `Enforce security regression library (status: enforced entries)` has
   an unquoted `status:` colon inside a block-mapping scalar, which a
   spec-compliant parser rejects. GitHub would have refused the entire
   workflow file, silently disabling ALL kask CI jobs (fmt, clippy, test,
   build, all gate jobs) from that commit onward. **Fixed this pass** (name
   quoted; file re-validated with a YAML parser). This pass also removed the
   dead trace/mutation steps that referenced the deleted `kask/scripts/test`
   and added the missing toolchain/cache to the `kali-regressions` job.
8. **`.env.example` churn** (`2e7458607b` added `KAGGLE_API_KEY`, `e953a9744f`
   removed it 4s later): deliberate cleanup, final state consistent.

## Test-quality verification

- `redact_spans_out_of_order_matches_all_redacted` and
  `redact_spans_overlapping_matches_merged_no_suffix_leak` construct `Match`
  vecs directly — deterministic, no dependence on scanner emission order.
  First-pass F2 is genuinely closed.
- `panel_tool_names_match_server` pins the `ledger_router` rename across the
  panel/server seam.
- Validation run this pass: 730 lib tests green across the 8 touched crates
  (guard 37, companies 164, corpus 176+1i, media 57 [via agent], server 25,
  swarm 103, curator 2, training 165+2i), plus `cargo check` on all touched
  crates, `cargo fmt --check` on annotated files, `cargo deny advisories` ok,
  and all three regression gates green.

## Process note

Both fix sub-agents committed their work directly (`c23f6f9661`,
`77eeaf70aa`) without an explicit commit request; `77eeaf70aa` initially swept
in unrelated user WIP (`tasks/plan.md`, prediction-markets report) and was
amended to `61abea787f` with the WIP restored to the working tree. Verify
`git log` before pushing.
