---
title: "Plan: Remove the companies MCP server, FMP/EODHD, and FIBO"
audience: [architects, developers, agents]
last_updated: 2026-10-18
version: "1.0.0"
status: "Proposed"
domain: "Cross-cutting"
mds_categories: [domain, composition, lifecycle]
---

# Plan: Remove the companies MCP server, FMP/EODHD, and FIBO

Status: approved design record, not yet executed.
Composed: 2026-10-17. Measured against HEAD `3b3688a8ae`.

## 1. Functional target

kask no longer ships a companies MCP server, no longer calls FMP or EODHD,
and no longer carries FIBO anywhere in the ontology ladder. Nothing that
survives is left half-wired: no skill references a deleted tool, no setting
feeds a dead server, no credential is requested for an API we no longer
call, no doc describes a removed capability, no test pins deleted behavior.
Capabilities worth keeping live in a surviving server and are verified by
being called there.

## 2. Evidence base (measured, re-verify before editing)

| Fact | Measurement | Site |
|---|---|---|
| Companies tool surface | 32 tools, 8 modules | `tool_surface_is_exactly_32_registered_tools` (companies `hkask_mcp_companies.rs` — deleted 2026-10-18; the pin line is the measurement record) |
| Companies size | 19,703 LOC total / 15,750 production | `find ... -name '*.rs'` |
| `hkask-forecast` ownership | SHARED crate — scenarios, prediction-markets, kata-kanban, hkask-graph-widget | `Cargo.toml` deps |
| FIBO consumers outside companies | prediction-markets 29 refs, corpus 8 refs | `semantic_mapping.rs`, `economic_object.rs`, `hkask_mcp_corpus.rs`, `ontology_io.rs` |
| FIBO ladder position | first rung of `PUBLISHED_DOMAIN` | `term_resolution.rs:115` |
| FIBO payload | `fibo.rs` 180 L, `fibo-release-modules.tsv` 168 L, `sources/fibo/` 6.9M / 158 RDF | bridge crate |
| `derived` rung FIBO coupling | **zero** — clean separation | `grep -c fibo derived.rs` → 0 |

### Provider coupling per tool module

| Module | Provider touches | Verdict |
|---|---|---|
| `expectations.rs` | 24 | dies with data |
| `valuation.rs` | 18 | split — forecast loop survives |
| `financial_data.rs` | 9 | dies with data |
| `analysis.rs` | 4 | split — research search survives |
| `transcript.rs` | 2 | split — corpus mode survives |
| `economic_profit.rs` | 2 | dies with data |
| `analytics.rs` | 2 | dies with data |
| `notes.rs` | 0 | portable, but zero consumers |

### Skill consumption (the repair surface)

`company-research-flash` 17 hits · `company-research-deep` 9 ·
`superforecasting` 4 · `grounding-verify` 1 · `gradient-hunter` 1 ·
`create-skill` 1. Plus 15 templates under
`kask/registry/templates/company-research/`.

Fourteen tools have **zero** skill consumers: `working_capital_cycle`,
`symbol_search`, `result_feedback`, `note_add`/`note_list`/`note_delete`,
`monte_carlo_dcf`, `moat_check`, `management_scorecard`,
`historical_price`, `forecast_get`, `equity_duration`, `ep_valuation`,
`calibrate_forecast`.

## 3. Port triage

The discriminator is **data dependency**, not usefulness. A tool whose
inputs arrive from the caller survives; a tool that reads FMP/EODHD does
not.

### Tier 1 — port, has live consumers

**P1. Forecast persistence + Brier loop → scenarios server.**
`forecast_persist`, `forecast_get`, `forecast_list`, `forecast_record`,
`result_feedback`.
Verified portable: `forecast_record` takes `forecast_price_change` and
`actual_price_change` as caller parameters (companies `valuation.rs`);
`resolve_current_price` is a pure function over already-fetched JSON
(companies `forecast.rs`). The Brier math already lives in the shared
`hkask-forecast` crate. Consumers: `superforecasting` (4 hits),
`company-research-flash`.
Design note: scenarios owns a file-backed journal+snapshot `ForecastStore`
(`superforecast/store.rs`); companies uses SQLite `screen_store`. Unify on
the scenarios store; migrate the `StoredForecast` snapshot shape
(companies `forecast.rs` snapshot shape) into it. Drop the DCF-model decomposition fields that
have no non-provider source — `forecast_record` must degrade visibly
(a named note), never silently.

**P2. Multi-provider research search → research server.**
`company_research_search` over `research.rs` (644 L, Exa/Tavily/Brave).
The research server already wires all three providers. This is a thin
re-expression over existing `web_search`, **not** a lift-and-shift: port
the claim-classification layer (`ResearchClaimClassifier` — category
tagging, numeric extraction, ticker detection) and delete the duplicate
provider plumbing. Expect a large net deletion here.

### Tier 2 — port one mode, delete the other

**P3. `company_transcript` corpus mode → media server.**
Corpus mode is SerpAPI YouTube with a channel allowlist
(`tools/transcript.rs:63-80`); media already has `youtube_search` and holds
`HKASK_SERPAPI_API_KEY`. Earnings mode is FMP `/earning-call-transcript`
and dies. Preserve the `entity_ref_prefix` convention
(`company:{symbol}:...`) — the corpus pipeline consumes it.
Consumers: `company-research-deep`, `company-research-flash`.

### Tier 3 — do not port

All 9 `financial_data` tools; `moat_check`, `management_scorecard`,
`working_capital_cycle`; `dcf_valuation`, `reverse_dcf`, `ep_valuation`;
`expectations_gap`; `comparable_analysis`, `sensitivity_analysis`,
`monte_carlo_dcf`, `equity_duration`; `calibrate_forecast`.
Each reads provider statements. No data, no tool.

`scenario_impact_valuation` is the scenarios↔companies join (3 skill hits).
It consumes a scenario tree and emits per-node DCF impacts — it dies with
the DCF engine and leaves a hole in `scenario-planning`. **Operator
decision required** (§5).

`note_add`/`note_list`/`note_delete` are provider-free and portable, but
have zero consumers. Recommend delete. **Operator decision** (§5).

## 4. Execution phases

Ordering is a correctness constraint. Never leave the tree build-broken
between passes.

**Phase A — Port the survivors (before deleting anything).**
P1 → scenarios, P2 → research, P3 → media. Each port is verified by
*calling the tool in its new home and getting a correct result*, not by
`cargo check`. Gate: all three ported tools answer correctly.

**Phase B — Repair skills and templates.**
Re-point `superforecasting`, `company-research-flash`,
`company-research-deep`, `grounding-verify`, `gradient-hunter`,
`create-skill`, and the 15 `company-research/` templates at the new tool
homes; delete references to Tier-3 tools. Done before deletion so no skill
ever points at nothing. Gate: zero references to deleted tool names across
`.agents/skills` and `kask/registry`.

**Phase C — Delete the companies server and its wiring.**
Crate `kask/mcp-servers/hkask-mcp-companies/`; workspace members
(`Cargo.toml:310`, `:940`); registry entry (`mcp_servers.rs:49-77`) and its
pin tests (`:1009-1021`, `:1030-1046`, `:1140`); `KaskCompaniesSettings`
(`settings.rs:336,348,762,951,1057,1431`), `emit_companies_env`
(`mcp_env.rs:117-134`), `settings_content.rs:1671`,
`settings_ui/.../companies.rs`; FMP/EODHD credentials
(`inference_providers.rs:201-208`, `kask_page.rs:583`); `agent.rs:10679`;
`reliability.rs:923`; `kask/docs/reference/mcp-servers/companies.md` and
`kask-settings.md:139-140,534-535`.
Gate: `./script/clippy` green; `cargo check -p zed` green.

**Phase D — Decide and land FIBO replacement anchors.**
Must pass before Phase E. Live-verified substitution for corpus:
`onto_anchor("corporation")` currently returns `fibo-be-le-cb:Corporation`
with `sumo:Corporation` and `schema:Corporation` as alternatives — removing
FIBO resolves it to `sumo:Corporation` with a published definition. Apply
to `hkask_mcp_corpus.rs:1040,1253,1263` and `ontology_io.rs:78`.
Prediction-markets needs six anchors re-homed
(`REFERENCE_INTEREST_RATE`, `INTEREST_RATE_BENCHMARK`,
`CONSUMER_PRICE_INDEX`, `ECONOMIC_INDICATOR`, `REFERENCE_INDEX`,
`GROSS_DOMESTIC_PRODUCT`). Recommendation: RDF Data Cube (`qb:`), the next
`PUBLISHED_DOMAIN` rung and the published statistical-data vocabulary, with
SUMO fallback. **Operator decision** (§5) — a changed anchor changes what
terms resolve to.

**Phase E — Remove FIBO from the ladder.**
`"FIBO"` from `PUBLISHED_DOMAIN` (`term_resolution.rs:116`); `src/fibo.rs`;
`fibo-release-modules.tsv`; `sources/fibo/` (158 files, 6.9M);
`index_fibo` codegen (`build.rs:145-210`); `fibo` from
`ONTOLOGY_TERM_PATTERN` and the `"fibo:Corporation"` allowlist entry
(`ontology_literal_guard.rs:42,74`); FIBO ladder assertions
(`term_resolution.rs:286-346,660-667`); the `fibo_anchored` fixture
(`check_skill_lisp_forms.rs:663-666`).
Gate: ladder resolves every previously-FIBO term to a real anchor with a
published definition; guard test green; `onto_anchor` never returns a
dangling `fibo:` concept.

**Phase F — Residue sweep and closure.**
Full-repo sweep over `.rs`, `.md`, `.j2`, `.toml`, `.json`, `.yml` for
every removed identifier — code *and* docs. Known doc surface:
`MDS.md`, `PRINCIPLES.md`, `skills-and-composition.md`,
`diagrams/architecture.md`, `diagrams/ui-widgets.md`, `loop-register.md`,
`reference/mcp-servers/README.md`, `reference/native-ontologies.md`,
`reference/ontology-bridge.md`, `research/ontology-anchoring-review.md`,
and the `fibo` mentions in `onto-anchor`, `pragmatic-semantics`, and
`sankey-flow` SKILL.md files. Update `DIVERGENCE.md` in the same pass.
Count matches with `grep -c` before consuming; never report clean from
truncated output.

## 5. Decisions reserved for the operator

1. **`scenario_impact_valuation`'s hole.** Scenario trees lose their
   valuation bridge. Narrow `scenario-planning` to non-valuation scenarios,
   or keep a caller-supplied-inputs variant?
2. **Prediction-markets FIBO anchors.** Confirm `qb:`-with-SUMO-fallback,
   or choose different anchors for the six economic concepts.
3. **Notes tools.** Delete (recommended — zero consumers) or port?
4. **`company-research-flash` and `company-research-deep`.** At 17 and 9
   hits with their valuation spine removed, these may need narrowing to
   qualitative research or retirement. Not repairable by substitution
   alone.
5. **Sealed-corpus content.** If `zk-ref-open` carries FIBO-dependent
   passages, whether to re-seal or accept the dangling reference.

## 6. Risks

| Risk | Severity | Mitigation |
|---|---|---|
| Another stream's ast-grep rules (`tool-surface-pin.yml`, `builtin-mcp-server-entry.yml`) gate exactly what this work removes | High | Coordinate before Phase C; never edit their in-flight files |
| FIBO removed before replacement anchors land → terms resolve to `core` coarse anchors | High | Phase D gates Phase E |
| `hkask-forecast` mistaken for companies-owned and deleted | High | Pinned in §2; four other consumers |
| Shared dirty tree; pathspec-less commit sweeps foreign work | High | Pathspec every commit; verify numstat |
| Skill repair lands after deletion → skills point at nothing | Medium | Phase B precedes Phase C |

## 7. Verification ladder

- **Ported tool:** called in its new server, correct result shown. Not
  `cargo check`.
- **Repaired skill:** runs end-to-end without reaching a deleted tool.
- **Ladder:** every previously-FIBO term resolves to a published anchor.
- **Removal:** `grep -c` over the full repo returns zero for every removed
  identifier, code and docs.
- **P5.5:** net lines reported per commit. Expected strongly negative
  (~20k lines + 158 files + 6.9M removed against ~1k added).
