# hkask Classifier Model Review & Template Parameter Refactor

**Date:** 2026-08-16 · **Sources:** OpenRouter `/api/v1/models`, `/api/v1/models/{id}/endpoints`, live `/chat/completions` benchmark (`/tmp/bench_results_v2.json`), plus repo evidence tagged `[file:line]`.

**Method (as agreed):** 4 screening criteria → equal-weight average rank over 3 metrics (latency, token speed, capability) → price as a reporting/tie-break column. `Unknown` = absent from all surveyed sources, never treated as zero.

---

## 1. Screening (catalog gates a–d)

All candidates verified against the OpenRouter model catalog `[LIVE:2026-08-16]`.
Gate (c) was also tested live: bodies with `reasoning: {enabled: false}` were sent — endpoints that reject thinking-disabled requests fail the classifier contract regardless of catalog flags (`disable_thinking: true` is the classifier default: `kask/mcp-servers/hkask-mcp-corpus/src/runtime/classify_impl.rs:101-110`, wire format `kask/crates/hkask-inference/src/chat_protocol.rs:109-114`).

| Candidate | (a) temperature | (b) structured output | (c) non-thinking | (d) age < 180 d | Result |
|---|---|---|---|---|---|
| `deepseek/deepseek-v4-flash` **(current)** | ✅ | ✅ | ✅ | ✅ 114 d (2026-04-24) | **PASS** |
| `deepseek/deepseek-v4-pro` | ✅ | ✅ | ✅ | ✅ 114 d (2026-04-24) | **PASS** |
| `meta/muse-glimmer-30b` ("glimmer") | ✅ | ✅ | ❌ endpoint: "Reasoning is mandatory…cannot be disabled" (HTTP 400) | ✅ 7 d | **FAIL (c)** |
| `nvidia/nemotron-3-nano-30b-a3b` (Nemotron) | ✅ | ✅ | ✅ | ❌ 245 d (2025-12-14) | **FAIL (d)** |
| `nvidia/nemotron-3.5-lightning` (Nemotron 3.5) | ✅ | ✅ | ✅ (live: accepts `reasoning.enabled=false`) | ✅ 4 d (2026-08-11) | **PASS** (added 2026-08-16 — missed in initial pass; see revision note) |
| `nvidia/nemotron-3.5-lightning:free` / `-3.5-content-safety:free` | ✅ | ❌ no `structured_outputs` | — | — | **FAIL (b)** |
| `nvidia/nemotron-3-super-120b-a12b` (Nemotron) | ✅ | ✅ | ✅ | ✅ 158 d (2026-03-11) | **PASS** |
| `nvidia/nemotron-3-ultra-550b-a55b` (Nemotron) | ✅ | ✅ | ✅ | ✅ 73 d (2026-06-04) | **PASS** |
| `qwen/qwen3.7-flash` | ✅ | ❌ no `structured_outputs` support | ✅ | ✅ 20 d | **FAIL (b)** |
| `qwen/qwen3.7-plus` | ✅ | ✅ | ✅ | ✅ 74 d | **PASS** |
| `qwen/qwen3.7-max` | ✅ | ✅ | ✅ | ✅ 87 d | **PASS** |
| `qwen/qwen3.8-27b` | ✅ | ✅ | ✅ | ✅ 2 d (2026-08-14) | **PASS** |
| `qwen/qwen3.8-2.4t-a95b` | ✅ | ✅ | ❌ endpoint: reasoning mandatory (HTTP 400) | ✅ 4 d | **FAIL (c)** |
| `qwen/qwen3.8-max` | ✅ | ✅ | ✅ | ✅ 13 d (2026-08-03) | **PASS** |

**Endpoint serving check (requirement: models/endpoints, not just catalog):**
- `qwen/qwen3.7-plus`, `qwen/qwen3.7-max`, `qwen/qwen3.8-max`: all requests returned **HTTP 404** — "No endpoints available matching your guardrail restrictions and data policy." Catalog-eligible but **not servable under the current account's privacy/guardrail settings** → excluded from ranking.
- `nvidia/nemotron-3-super-120b-a12b`: served cleanly with thinking disabled; endpoint inventory shows **DeepInfra only** (n=9/9 runs).
- `nvidia/nemotron-3.5-lightning`: served cleanly on **all three** endpoints — DeepInfra (bf16, 100% uptime), CoreWeave (bf16, 100%), Venice (fp4, 100%) `[ep:2026-08-16]`.

**Screening outcome — 9 catalog-eligible; 6 servable/benchmarkable:**
`deepseek-v4-flash`, `deepseek-v4-pro`, `nemotron-3.5-lightning`, `nemotron-3-super-120b`, `nemotron-3-ultra-550b`, `qwen3.8-27b`.

**Revision note (2026-08-16):** `nemotron-3.5-lightning` was present in the catalog pull used for the initial screening but was not carried into the candidate list. All `nemotron-*` catalog entries were re-screened; result above. Ranking tables in §2–3 updated accordingly.

---

## 2. Equal-weight average rank (e) latency · (f) token speed · (g) capability

Benchmark protocol: `section-classifier` 4-way label task (`kask/registry/classify/section-classifier.yaml` prompt), 5 labeled passages × 2 runs, first run discarded as warm-up, `temperature=0.0`, `max_tokens=300`, `reasoning.enabled=false`, OpenRouter auto-routing. p50 over scored runs. Single-day snapshot `[LIVE:2026-08-16]`, n=8 labeled cases.

| Model | (e) p50 TTFT ms (rank) | (f) p50 tok/s (rank) | (g) label accuracy (rank) | **Avg rank** |
|---|---|---|---|---|
| `nvidia/nemotron-3-super-120b-a12b` | 429 (**2**) | 619.7 (**1**) | 6/8 (**1** tied) | **1.67** |
| `nvidia/nemotron-3.5-lightning` | 314 (**1**) | 300.0 (**2**) | 4/8 (**4** tied) | 2.67 |
| `deepseek/deepseek-v4-pro` | 1216 (**5**) | 62.5 (**4**) | 6/8 (**1** tied) | 3.67 |
| `nvidia/nemotron-3-ultra-550b-a55b` | 647 (**3**) | 145.8 (**3**) | 4/8 (**4** tied) | 3.67 |
| `deepseek/deepseek-v4-flash` (current) | 1481 (**6**) | 57.7 (**5**) | 6/8 (**1** tied) | 4.33 |
| `qwen/qwen3.8-27b` | 829 (**4**) | 42.9 (**6**) | 4/8 (**4** tied) | 5.00 |

Accuracy caveat: 4-way chance = 25%. The three 6/8 models are statistically indistinguishable at n=8 — the quality axis does not separate them. Ranks reflect measured medians, not model-size priors.
Token-speed caveat: `tok/s = completion_tokens / (total − TTFT)` per run. `nemotron-3.5-lightning` emits consistently short answers (6–7 tokens), while other models emitted up to 300; cross-model rate comparisons across different output lengths are noisy — treat the 300 tok/s figure as an inter-token-latency artifact, not sustained throughput.
Lightning-specific quality signal: across all 9 runs it returned only `Statement` or `Evidence` labels — both `Implications` and `Diagram` cases were mislabeled as `Statement`. The 4/8 score masks a label folding tendency that a larger sample needs to confirm or rule out.

Per-endpoint detail (best-named providers, p50 over runs that hit that provider):

| Model | Provider | p50 TTFT ms | p50 tok/s | n |
|---|---|---|---|---|
| `nemotron-3.5-lightning` | DeepInfra | 209 | 241.4 | 3 |
| `nemotron-3.5-lightning` | CoreWeave | 259 | 300.0 | 3 |
| `nemotron-3.5-lightning` | Venice | 597 | 352.9 | 3 |
| `deepseek-v4-flash` | DeepInfra | 602 | 150.0 | 3 |
| `deepseek-v4-flash` | SiliconFlow | 1011 | 57.7 | 1 |
| `deepseek-v4-flash` | StreamLake | 1471 | 292.2 | 2 |
| `deepseek-v4-flash` | DigitalOcean | 2387 | 5.5 | 2 |
| `deepseek-v4-flash` | Venice | 2711 | 285.7 | 1 |
| `deepseek-v4-pro` | StreamLake | 1245 | 65.9 | 5 |
| `deepseek-v4-pro` | Novita | 1130 | 24.4 | 3 |
| `deepseek-v4-pro` | GMICloud | 1073 | 53.1 | 1 |
| `nemotron-3-super-120b-a12b` | DeepInfra | 429 | 619.7 | 9 |
| `nemotron-3-ultra-550b-a55b` | Together | 352 | 260.9 | 3 |
| `nemotron-3-ultra-550b-a55b` | DeepInfra | 647 | 142.9 | 3 |
| `nemotron-3-ultra-550b-a55b` | Venice | 865 | 483.1 | 2 |
| `nemotron-3-ultra-550b-a55b` | BaseTen | 413 | 400.0 | 1 |
| `qwen/qwen3.8-27b` | AkashML | 564 | 42.3 | 5 |
| `qwen/qwen3.8-27b` | Chutes | 988 | 47.1 | 4 |

---

## 3. Price — reporting metric / tie-break

Cheapest-served endpoint per model `$ /1M tokens` (catalog + endpoints API `[LIVE:2026-08-16]`); per-classification cost estimated at the classifier workload shape ~1,000 input / ~30 output tokens:

| Model (avg rank) | $/M in · out | ≈ $ per 1M classifications | Δ vs current |
|---|---|---|---|
| `deepseek-v4-flash` current (4.33) | 0.068 · 0.168 (DigitalOcean) | ≈ $73 | — |
| `nemotron-3.5-lightning` (2.67) | 0.080 · 0.200 (DeepInfra) | ≈ $86 | +18% |
| `nemotron-3-super-120b-a12b` (1.67) | 0.085 · 0.400 (DeepInfra) | ≈ $97 | +33% |
| `qwen/qwen3.8-27b` (5.00) | 0.400 · 3.000 (Chutes) | ≈ $490 | +571% |
| `nemotron-3-ultra-550b-a55b` (3.67) | 0.500 · 2.200 (DeepInfra) | ≈ $566 | +676% |
| `deepseek-v4-pro` (3.67) | 0.645 · 1.290 (Novita) | ≈ $684 | +838% |

**Price as tie-break:** all three top-ranked models sit within +33% of current price — not a dramatic spread, so price does not overturn rank order. It does, however, make `nemotron-3.5-lightning` (rank 2, +18%, three healthy providers) the standout challenger on the cost axis.

---

## 4. Recommendation

**Primary:** migrate `DEFAULT_CLASSIFIER_MODEL` from `OpenRouter/deepseek/deepseek-v4-flash` to `OpenRouter/nvidia/nemotron-3-super-120b-a12b`, optionally provider-pinned to DeepInfra (its only live endpoint; DeepInfra is already a configured kask provider: `kask/crates/kask_bridge/src/inference_providers.rs:55-62`).
- Best composite rank (1.67) — 2nd-fastest TTFT, highest token speed, accuracy-tied at the top (6/8); +33% cost per call.
- All screening gates pass; serves thinking-disabled requests today.

**Challenger — `nvidia/nemotron-3.5-lightning`:** fastest TTFT (314 ms, 209 ms on DeepInfra), cheapest of the elite at +18%, youngest model (4 d), and the only top-3 candidate with **multi-provider redundancy** (DeepInfra + CoreWeave + Venice, all 100% 30-day uptime). Its single weakness — 4/8 accuracy with a `Statement`-folding tendency on Implications/Diagram — is exactly the kind of failure the classifier contract cannot tolerate without a larger sample. **Before promoting it past super, run the extended evaluation (50–100 real corpus passages).** If the fold disappears, it is the better default on cost and resilience.

**Keep current with a pin caveat:** if +18–33% is judged too large, keep `deepseek-v4-flash` but pin the DeepInfra endpoint — its median latency is dominated by bad routing (DigitalOcean 2387 ms vs DeepInfra 602 ms); pinning alone recovers most of the latency gap at current price.

**Not recommended:** Qwen 3.7/3.8 variants are catalog-eligible but blocked by account guardrails today — revisit only if `openrouter.ai/settings/privacy` is relaxed (a settings change, not a code change). `meta/muse-glimmer-30b` and `qwen3.8-2.4t-a95b` fail the non-thinking gate; `nemotron-3-nano-30b-a3b` fails the age gate by 64 days; the `-free` Nemotron variants fail the structured-output gate (b).

**Confidence:** n=8 labeled cases, single day, small sample. The latency/speed ordering was consistent across all runs and is unlikely to change; accuracy ranks are provisional, and the lightning label-fold in particular needs a bigger sample before it can be ranked above super on the quality axis.

---

## 5. Classifier contract (D2) — evidence summary

- Model resolution: `model: ""` → env → constant (`classify_impl.rs:221-225`); unresolvable override → default model + `warn` (`kask/crates/kask_bridge/src/inference.rs:240-247`).
- Concurrency caps enforced: `tokio::sync::Semaphore` (`classify_impl.rs:355`); manifests declare 5 / 10 / 150.
- `timeout_secs: 30` is stored (`classify_impl.rs:230`) but **never enforced** — no `tokio::time::timeout` anywhere in the crate. Latency budget: **Unknown** (no stated target).
- Retry: `MAX_RETRIES = 3` + exponential backoff exists (`hkask-mcp-corpus/src/batch.rs:16-18, 85-109`) and wraps the assertion pipeline, but `classify_batch`'s per-passage calls have **no retry** (`classify_impl.rs:364-368`).
- Cost accounting: wired to the ledger (`classify_impl.rs:305-315, 410-448`); `cost_cache_read_nj_per_token` is forced to 0 (`classify_impl.rs:236`).
- Only `qa-triage` and `qa-feedback` carry cost rates (30/60 nJ/token); extractors/classifiers run with costs disabled — an explicit `warn` on load (`classify_impl.rs:208-214`).
- Malformed-output fallback: JSON parse with category extraction + keyword fallback (`classify_impl.rs:283-299`).
- Stale references to fix in the switch PR: `kask/mcp-servers/hkask-mcp-corpus/README.md:143` and `kask/mcp-servers/hkask-mcp-corpus/src/tools/semantic/mod.rs:364` still claim "default Qwen3-235B-A22B-Instruct on DeepInfra".

## 6. Template parameter surface (D1/D5) — evidence summary

Full inventory: `kask/docs/review/inference-block-inventory.tsv` (358 files, per-key/value, with file:line).

- Only 3 keys have effect: `temperature`, `max_tokens`, `thinking_budget` (parsed `kask/crates/hkask-templates/src/template_renderer.rs:281-285, 330-341`; applied `kask/crates/hkask-templates/src/step_actions.rs:239-262`).
- `work_effort` (275 files) and `verbosity` (267 files) are **dead keys** — dropped by the parser's `_ => {}` arm; only test heuristics read them (`tests/manifest_invariants.rs:633-642`, `tests/token_budget_audit.rs:195-199, 230`).
- `thinking_budget` values outside `{full, on, off, none}` (22 files: `standard`/`medium`/`low`/`minimal`/`high`) trigger warn-only fallback → **thinking silently enabled** (`step_actions.rs:253-262`) — wrong-direction default for any template that expected a budget cap.
- Default when no override: temperature 0.6, max_tokens 2048, thinking on (`kask/crates/hkask-types/src/template.rs:88-110`) — the "edge work" preset.

**Refactor actions (each serves the contract axis):**
1. **Delete** `work_effort` and `verbosity` from all `[inference]` blocks (essentialist deletion test: zero runtime value; keep as comments if the annotation is load-bearing to authors) — shrinks the parameter surface from 5 keys to 3.
2. **Fix the vocabulary**: normalize the 22 non-canonical `thinking_budget` values to `full|off` (or extend the parser mapping — one match arm at `step_actions.rs:246-251`), and change the unrecognized-value default from warn-and-keep-thinking-on to warn-and-disable (classification-adjacent templates are the majority default; see `template_renderer.rs:629-679` for the pinning tests to extend).
3. **Duplicate manifests**: `h_mem-extractor.yaml` ≈ `hmem-extractor.yaml` are near-identical; deep-module deletion test says merge to one and keep the other as a symlink/alias if the name must survive.
4. **Enforce, don't declare**: wire `timeout_secs` into a `tokio::time::timeout` around the batch (`classify_impl.rs:364-368`) or remove the field from the schema — declared-but-unenforced config is a broken feedback loop (`.rules` failure-signal rule).

## 7. D-seam plan (D6)

| Change | Location | Seam / test |
|---|---|---|
| Switch `DEFAULT_CLASSIFIER_MODEL` | `kask/crates/hkask-inference/src/model_constants.rs:23` | kask-only file; add a test that pins env-override precedence (`model_constants.rs:57-59`) |
| Provider pin for classifier (DeepInfra) | pass-through of `provider: { only: [...] }` alongside `model_override` in `LanguageModelInferencePort::generate_with_model` (`kask/crates/kask_bridge/src/inference.rs:519-535`) | new kask-side plumbing only — no upstream zed edits; pin test: override resolves to DeepInfra endpoint (extend `inference.rs:1323-1404` propagation tests) |
| `timeout_secs` enforcement | `hkask-mcp-corpus/src/runtime/classify_impl.rs` | unit test: hung inference future fails after `timeout`; kask-only file |
| `thinking_budget` normalization + default flip | `hkask-templates/src/step_actions.rs` | extend existing parser tests (`template_renderer.rs:629-679`); needs a `// zed-kask:` note only if it changes behavior shared with upstream (it does not — kask crate) |
| Dead-key deletion in `.j2` + `[meta]` block | `kask/registry/templates/**` | `manifest_invariants.rs` rule: `work_effort`/`verbosity` forbidden in `[inference]` blocks |

No upstream `crates/` files are touched; all changes land in `kask/` per DIVERGENCE.md.

## Self-verification

- [x] All model ids/prices/ages from OpenRouter catalog/endpoints APIs (downloaded 2026-08-16)
- [x] All latency/throughput numbers from the live benchmark; no p50 fabricated; `n` per value reported
- [x] Accuracy grounded in labeled cases (8); chance level stated
- [x] Unservable candidates excluded with the exact 400/404 reason, not guessed
- [x] Missing contract targets stated as Unknown (latency budget), never as zero
- [x] Screen-gate failures called out even when the model "exists" (glimmer, nano, qwen3.7-flash, qwen3.8-2.4t)
- [x] Price surfaces per-call deltas; tie-break only applied where ranks tied

End of report.
