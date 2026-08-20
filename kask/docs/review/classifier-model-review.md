# hkask Classifier Model Review

> **⚠️ Partially deprecated 2026-08-20.** The classifier model selection
> (default switched to `OpenRouter/z-ai/glm-5.2`) and the corpus-server
> evidence remain current. However, §7 and §8 cite deleted files
> (`template_renderer.rs`, `step_actions.rs`) as locations for proposed
> changes — those files are gone. The `thinking_budget` normalization and
> dead-key deletion actions that targeted them are void; the template
> parameter parsing surface moved with the crate's deletion (the
> `render_template` agent tool renders Jinja2 templates but does not parse
> `[inference]` blocks the way the deleted `template_renderer.rs` did). The
> `work_effort`/`verbosity` dead-key findings are historical.

**Date:** 2026-08-17 · **Status:** Complete — default switched to `OpenRouter/z-ai/glm-5.2`
**Sources:** OpenRouter `/api/v1/models` + `/api/v1/models/{id}/endpoints` (catalog, gate
screening, price); live `/chat/completions` eval (12 models × 50 cases, 2026-08-17); repo
evidence tagged `[file:line]`. (Benchmark scripts and the eval-set fixture used during
the review were deleted afterward per operator direction; this report is self-contained.)

**Evidence tags:** `[file:line]` = code/manifest · `[LIVE:2026-08-17]` = measured at runtime ·
`Unknown` = absent from every surveyed source (never treated as zero).

**Method:** 4 screening gates → equal-weight average rank over latency (time to first token) /
accuracy on the classifier's real label spaces → price as a reporting column only (operator
direction: cost is the operator's concern, not a ranking axis). Generation speed (tok/s) was
measured but is unreliable and excluded from the rank (see §5).

---

## 1. Screening

Gates: (a) accepts `temperature` · (b) supports `structured_outputs` · (c) non-thinking callable
— passes if EITHER non-reasoning (no thinking mode to disable) OR reasoning-capable and accepts
`reasoning.enabled=false` with a non-thinking mode (live-checked, not inferred from catalog) · (d) **updated within 90 days** (revision date, not creation date — the
`created`/`canonical_slug` date is the model's last revision; threshold 2026-05-19).

Candidate pool derived from screening (no operator anchoring for derivation), then the operator
selected the final 12 to evaluate (two data-point additions outside the 90-day window:
`z-ai/glm-4.7-flash` and `mistralai/mistral-small-2603`). All 12 passed gate (c) live
(each served `{"category":"Statement"}` with `reasoning.enabled=false`).

**Not in the pool:** DeepSeek R1 — fails gate (c) (`reasoning.mandatory = true`; no non-thinking
mode) and gate (d) (latest revision `deepseek-r1-0528` = 2025-05-28, >90 days). R1 is a
reasoning-mandatory model and cannot serve as a non-thinking classifier under the wire protocol
the classifier uses (`reasoning.enabled=false`; `hkask-inference/src/chat_protocol.rs:109-114`).

---

## 2. Eval results — 12 models × 50 cases (2026-08-17)

Eval set: 50 observations across the three real hKask classifier label spaces:
- **section** (4-way Statement/Evidence/Diagram/Implications) — `registry/classify/section-classifier.yaml`
- **dimension** (4-way Gentle/Schriver/Hopper/Lovelace) — `registry/classify/hmem-extractor.yaml`
- **failure** (6-way Panic/Assertion/Timeout/Flake/LogicError/MemoryError) — `registry/classify/qa-triage.yaml`

S01–S03 are the section YAML's 3-shot in-context examples, reported separately (`inctx/3`) and
excluded from the accuracy total. Protocol: temperature 0.0, `max_tokens` 150,
`reasoning.enabled=false`, streaming. `CONCURRENCY=8` (per-case files, deterministic ordering).

Table sorted by **average rank** over accuracy + time-to-first-token (equal weight, lower = better).
Generation speed is shown but **not ranked** (§5). Price = cheapest provider via the endpoints API
(reporting only).

| Avg rank | Model | Accuracy | Section /17 | Dimension /20 | Failure /10 | Time to first token (ms) | Generation speed (tok/s)¹ | Price $/M in·out² |
|--:|---|--:|--:|--:|--:|--:|--:|--:|
| 3.5 | `bytedance-seed/seed-2-1-turbo` | 40/47 | 17/17 | 13/20 | 10/10 | 1229 | 1400 | 0.50 / 2.50 |
| 3.5 | `thinkingmachines/inkling-small` | 36/47 | 13/17 | 13/20 | 10/10 | 537 | 2200 | 0.45 / 1.20 |
| 5.0 | `z-ai/glm-5.2` **(chosen)** | 39/47 | 17/17 | 12/20 | 10/10 | 1257 | 1550 | 0.50 / 2.20 |
| 6.0 | `nvidia/nemotron-3-ultra-550b-a55b` | 33/47 | 13/17 | 10/20 | 10/10 | 782 | 1675 | 0.50 / 2.20 |
| 6.25 | `anthropic/claude-opus-5` | 38/47 | 14/17 | 14/20 | 10/10 | 1422 | 3250 | 5.00 / 25.00 |
| 6.5 | `moonshotai/kimi-k3` | 35/47 | 15/17 | 10/20 | 10/10 | 1246 | 3000 | 2.60 / 13.00 |
| 6.5 | `mistralai/mistral-small-2603` | 31/47 | 12/17 | 9/20 | 10/10 | 704 | 1633 | 0.15 / 0.60 |
| 6.5 | `z-ai/glm-4.7-flash` | 28/47 | 11/17 | 7/20 | 10/10 | 521 | 1500 | 0.06 / 0.40 |
| 7.25 | `tencent/hy3` | 38/47 | 15/17 | 13/20 | 10/10 | 1930 | 1708 | 0.13 / 0.52 |
| 8.0 | `deepseek/deepseek-v4-flash-0731` | 29/47 | 12/17 | 7/20 | 10/10 | 884 | 1775 | 0.06 / 0.12 |
| 8.5 | `minimax/minimax-m3` | 34/47 | 13/17 | 11/20 | 10/10 | 1668 | 1675 | 0.23 / 0.96 |
| 10.5 | `deepseek/deepseek-v4-pro-0813` | 32/47 | 14/17 | 8/20 | 10/10 | 3099 | 1500 | 1.06 / 2.44 |

**Findings:**
1. **`seed-2-1-turbo` (40/47, 85%) is the raw accuracy leader** and perfect on section (17/17),
   but its time-to-first-token (1229 ms) is mid-pack, tying it with `inkling-small` (36/47, 537 ms)
   at average rank 3.5.
2. **`glm-5.2` (39/47, 83%) is the chosen model** — 2nd on accuracy, perfect on section (17/17),
   and the operator knows the GLM family. Its average rank (5.0) is 3rd, behind the two
   accuracy+latency leaders, because its TTFT (1257 ms) is mid-pack. The decision incorporates
   factors beyond the rank (see §3).
3. **The prior default's latest revision (`deepseek-v4-flash-0731`) scored 29/47 (62%)** — well
   below the leaders and below its own earlier runs (31–35; see §5 variance). The prior default is
   not the quality leader.
4. **The dimension axis is where every model collapses** (7–14 of 20); only `claude-opus-5` (14)
   and `seed-2-1-turbo` / `inkling-small` / `hy3` (13) reach decent. No off-the-shelf model clears
   it — the durable fix remains a small fine-tune on dimension-ontology examples (§8).
5. **`glm-4.7-flash` (28/47)**, added as a data point, is not competitive.

---

## 3. Decision — `OpenRouter/z-ai/glm-5.2`

The classifier default is switched to `OpenRouter/z-ai/glm-5.2`
(`kask/crates/hkask-inference/src/model_constants.rs:23`).

Rationale:
- **Accuracy leader-tier:** 39/47 (2nd of 12), perfect on the section axis (17/17), 12/20 on the
  hard dimension axis — ahead of the prior default's latest revision (29/47) and ahead of or tied
  with every model except `seed-2-1-turbo`.
- **Operator familiarity with the GLM family** and its behavior under hKask's wire protocol
  (GLM-5.2's thinking-mode / null-content handling is already proven in
  `hkask-inference/src/chat_protocol.rs:148-158, 345-349` and `hkask-types/src/json_extract.rs:58`).
- **Forward price trajectory:** GLM 5.3 is expected to drop price (~75–80%, operator-provided).
  This is a **revisit trigger, not current fact** — it does not enter the ranking or the price
  column, which reflect GLM-5.2's current published prices.
- **Throughput is optimizable over time** via provider pinning (GLM-5.2 has 33 providers on
  OpenRouter; pinning a fast one cuts time-to-first-token without changing the
  model). This is a follow-up, not part of the switch.

The rank leader (`seed-2-1-turbo`) was not chosen; the operator weighed GLM-family familiarity and
the 5.3 price trajectory above the rank delta. Cost was the operator's call, not a ranking axis.

---

## 4. Centralization — every reference reads from the settings config variable

The classifier model has one source of truth and one configuration surface. The flow:

```
Settings >> Kask >> Models  (classifier_model field)
   └─ kask_bridge::KaskModelsSettings::classifier_model            [kask_bridge/src/settings.rs:704]
      └─ KaskSettings::mcp_env() injects HKASK_CLASSIFIER_MODEL    [kask_bridge/src/settings.rs:1122-1128]
         (only when the field is non-empty)
         └─ hkask_inference::model_constants::classifier_model()   [model_constants.rs:57-60]
            (env HKASK_CLASSIFIER_MODEL  →  DEFAULT_CLASSIFIER_MODEL constant)
            └─ ClassifierConfig::from_def resolves empty `model:`  [classify_impl.rs:221-223]
               └─ injected into context via the skill body
```

- **Single default constant:** `DEFAULT_CLASSIFIER_MODEL = "OpenRouter/z-ai/glm-5.2"`
  (`model_constants.rs:23`). No other crate re-declares the default id — `HkaskSettings` resolves
  it via `default_classifier_model()` → the constant (`hkask-services-core/src/settings.rs:70-74`),
  per the repo rule that defaults live in `Default` impls only.
- **Registry YAMLs defer:** every `registry/classify/*.yaml` carries `model: ""`, deferring to the
  constant (verified: section-classifier, hmem-extractor, h_mem-extractor, hmem-extractor-literary,
  qa-triage, qa-feedback, convergence-evaluator).
- **Settings UI:** the placeholder shown in Settings >> Kask >> Models is
  `OpenRouter/z-ai/glm-5.2` (`crates/settings_ui/src/pages/kask_page/models.rs:41`); the live value
  comes from `kask_bridge::KaskModelsSettings` so the UI shows the same default the runtime uses.
- **Stale references fixed in the switch:** the corpus assertion tool description
  (`hkask-mcp-corpus/src/tools/semantic/mod.rs:365`) and the `ClassifierDef` doc example
  (`classify_impl.rs:70`) were corrected off the prior Qwen/DeepSeek claims; doc references in
  `kask/docs/diataxis/hkask-inference/reference.md`, `kask/docs/reference/kask-settings.md`, and
  `kask/docs/plans/evolving-test-harness.md` were updated.

---

## 5. Measurement caveats and confidence

- **Generation speed (tok/s) is excluded from the ranking.** The measured values (1400–3250) are
  `completion_tokens / (total_ms − time_to_first_token)` — a ratio of two small noisy numbers that
  reads ~5–10× too high for these models. OpenRouter's published throughput/latency fields
  (`latency_last_30m`, `throughput_last_30m`) are present in the endpoints API but **null for every
  provider of all 12 models** — the website renders them client-side from an internal source, so
  they could not be substituted. A re-run with throughput measured over the first-token-to-last-
  token window would make the tok/s axis usable; until then the rank rests on accuracy + latency.
- **n=47, single run.** A 3-model validation pass showed run-to-run accuracy variance of ±4
  (`deepseek-v4-flash` scored 29–35 across runs) due to OpenRouter provider/replica routing at
  temperature 0 — not a concurrency artifact (concurrency was validated against a stubbed curl:
  ordering, per-case timing, and the running counter are correct). Point accuracies carry no
  variance band; a 100+ case or multi-run pass would harden the dimension-axis numbers.
- **Time to first token** is a direct measurement (timestamp of first content chunk − request
  start), not a ratio — structurally sound, with normal network/routing variance.

---

## 6. Classifier contract (D2) — evidence summary

- Model resolution: `model: ""` → env → constant (`classify_impl.rs:221-223`); unresolvable
  override → default model + `warn` (`kask/crates/kask_bridge/src/inference.rs:240-247`).
- Concurrency caps enforced: `tokio::sync::Semaphore` (`classify_impl.rs:355`); manifests declare 5/10/150.
- `timeout_secs: 30` is stored (`classify_impl.rs:230`) but **never enforced** — no
  `tokio::time::timeout` in the crate. Latency budget: **Unknown** (no stated target).
- Retry: `MAX_RETRIES = 3` + exponential backoff exists (`hkask-mcp-corpus/src/batch.rs:16-18,
  85-109`) and wraps the assertion pipeline, but `classify_batch`'s per-passage calls have **no
  retry** (`classify_impl.rs:364-368`).
- Cost accounting: wired to the ledger (`classify_impl.rs:305-315, 410-448`);
  `cost_cache_read_nj_per_token` is forced to 0 (`classify_impl.rs:236`).
- Only `qa-triage` and `qa-feedback` carry cost rates (30/60 nJ/token); extractors/classifiers run
  with costs disabled — an explicit `warn` on load (`classify_impl.rs:208-214`).
- Malformed-output fallback: JSON parse with category extraction + keyword fallback
  (`classify_impl.rs:283-299`).

## 7. Template parameter surface (D1/D5) — evidence summary

- Only 3 keys have effect: `temperature`, `max_tokens`, `thinking_budget`.
- `work_effort` (275 files) and `verbosity` (267 files) are **dead keys** — dropped by the parser's
  `_ => {}` arm; only test heuristics read them.
- `thinking_budget` values outside `{full, on, off, none}` (22 files: `standard`/`medium`/`low`/
  `minimal`/`high`) trigger warn-only fallback → **thinking silently enabled**
  — wrong-direction default for templates expecting a budget cap.
- Default when no override: temperature 0.6, max_tokens 2048, thinking on
  (`hkask-types/src/template.rs:88-110`).

**Refactor actions (each serves the contract axis):**
1. Delete `work_effort` and `verbosity` from all `[inference]` blocks (essentialist deletion test:
   zero runtime value) — shrinks the surface from 5 keys to 3.
2. Normalize the 22 non-canonical `thinking_budget` values to `full|off`, and flip the
   unrecognized-value default from warn-and-keep-thinking-on to warn-and-disable
   (classification-adjacent templates are the majority default).
3. Merge the duplicate `h_mem-extractor.yaml` ≈ `hmem-extractor.yaml` (deep-module deletion test).
4. Wire `timeout_secs` into a `tokio::time::timeout` around the batch (`classify_impl.rs:364-368`)
   or remove the field — declared-but-unenforced config is a broken feedback loop.

## 8. D-seam plan (D6)

| Change | Location | Seam / test |
|---|---|---|
| **Done:** switch `DEFAULT_CLASSIFIER_MODEL` → `OpenRouter/z-ai/glm-5.2` | `kask/crates/hkask-inference/src/model_constants.rs:23` | n/a — applied |
| Settings UI placeholder → `OpenRouter/z-ai/glm-5.2` | `crates/settings_ui/src/pages/kask_page/models.rs:41` | n/a — applied |
| Stale tool-description + doc-example fixes | `hkask-mcp-corpus/src/tools/semantic/mod.rs:365`, `classify_impl.rs:70` | n/a — applied |
| Provider pin for classifier (throughput optimization) | `LanguageModelInferencePort::generate_with_model` provider pass-through (`kask/crates/kask_bridge/src/inference.rs:519-535`) | new kask-side plumbing only; pin test: override resolves to pinned endpoint |
| `timeout_secs` enforcement | `hkask-mcp-corpus/src/runtime/classify_impl.rs` | unit test: hung inference future fails after `timeout`; kask-only file |
| `thinking_budget` normalization + default flip | (deleted crate) | extend parser tests; kask crate |
| Dead-key deletion in `.j2` + `[meta]` blocks | `kask/registry/templates/**` | `manifest_invariants.rs` rule: `work_effort`/`verbosity` forbidden in `[inference]` blocks |

No upstream `crates/` files are touched by the remaining actions; all land in `kask/` per
DIVERGENCE.md. (The settings UI placeholder edit touches `crates/settings_ui/src/pages/kask_page/`,
which is kask-wiring living in the upstream tree.)

## Self-verification

- [x] All model ids, gate statuses, and prices from OpenRouter catalog/endpoints APIs (2026-08-17).
- [x] Gate (c) live-confirmed for all 12 evaluated models (one probe call each).
- [x] Accuracy grounded in 47 labeled cases across three real registry label spaces; chance level
      noted (dimension axis is the hard one).
- [x] Latency (time to first token) from live runs, p50 reported with n=50.
- [x] tok/s excluded from the ranking with the reason stated; no fabricated throughput.
- [x] OpenRouter published latency/throughput unavailable (null in API) — stated, not papered over.
- [x] Missing contract targets stated as Unknown (latency budget), never as zero.
- [x] Centralization verified: one default constant; all YAMLs defer; runtime reads env → constant;
      settings config variable flows via `mcp_env()` env injection.
- [x] Stale references fixed; no remaining classifier-default literals outside the constant and the
      UI placeholder.
- [x] Decision rationale distinguishes measured rank (GLM-5.2 = 3rd on accuracy+latency) from
      operator judgment (familiarity, 5.3 price trajectory) — the rank is not misrepresented.

End of report.