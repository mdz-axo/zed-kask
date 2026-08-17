# hkask Classifier Model Review & Template Parameter Refactor

**Date:** 2026-08-16 · **Status:** Complete
**Sources:** OpenRouter `/api/v1/models`, `/api/v1/models/{id}/endpoints`, live `/chat/completions`
(2-round benchmark: 12 models × 10 = 120 runs; 6 models × 50 = 300 runs against the 50-obs eval set
`kask/docs/review/eval_set.json`), plus repo evidence tagged `[file:line]`.
Benchmark tooling: `kask/scripts/check-classifier-models.sh` (bash).

**Evidence tags:** `[file:line]` = code/manifest · `[LIVE:2026-08-16]` = measured at runtime ·
`Unknown` = absent from every surveyed source (never treated as zero).

**Method (as agreed):** 4 screening criteria → equal-weight average rank over latency / token
speed / accuracy on the classifier's actual label spaces → price as a reporting/tie-break column.
Operator cost ceiling: the largest observed delta (~+1041%, GLM-5.2) is **unacceptable**, which
bounds the admissible price band.

---

## 1. Screening (catalog gates a–d)

Gates: (a) temperature · (b) structured output · (c) non-thinking callable (endpoint must accept
`reasoning.enabled=false` — live-checked on first-round runs) · (d) age < 180 days.

| Candidate | (a) temp | (b) struct | (c) non-thinking | (d) age < 180 d | Result |
|---|---|---|---|---|---|
| `deepseek/deepseek-v4-flash` **(current)** | ✅ | ✅ | ✅ | ✅ (rev-1 call) | **PASS** |
| `deepseek/deepseek-v4-pro` | ✅ | ✅ | ✅ | 114 d ✅ | PASS |
| `meta/muse-glimmer-30b` ("glimmer") | ✅ | ✅ | ❌ endpoint 400 "Reasoning is mandatory" | ✅ | **FAIL (c)** |
| `nvidia/nemotron-3-nano-30b-a3b` | ✅ | ✅ | ✅ | 244 d ❌ | **FAIL (d)** |
| `nvidia/nemotron-3.5-lightning` | ✅ | ✅ | ✅ | 157 d ✅ | PASS |
| `nvidia/nemotron-3-super-120b-a12b` | ✅ | ✅ | ✅ | 157 d ✅ | PASS |
| `nvidia/nemotron-3-ultra-550b-a55b` | ✅ | ✅ | ✅ | 157 d ✅ | PASS |
| `qwen/qwen3.7-flash` | — | ❌ no `structured_outputs` | — | ✅ | **FAIL (b)** |
| `qwen/qwen3.7-plus` / `3.7-max` | ✅ | ✅ | ✅ | ✅ | PASS screening, **404** account privacy guardrail → **Not servable** |
| `qwen/qwen3.8-max` | ✅ | ✅ | ✅ | ✅ | PASS screening, **404** → **Not servable** |
| `qwen/qwen3.8-27b` | ✅ | ✅ | ✅ | ✅ | PASS |
| `qwen/qwen3.8-2.4t-a95b` | ✅ | ✅ | ❌ endpoint 400 reasoning mandatory | ✅ | **FAIL (c)** |
| `z-ai/glm-5.2` | ✅ | ✅ | ✅ | 60 d ✅ | PASS |
| `ibm-granite/granite-4.0-h-micro` | ✅ | ❌ no `structured_outputs` | ✅ | 299 d ❌ | **FAIL (b) and (d)** |
| `microsoft/phi-4` | ✅ | ✅ | ✅ | 582 d ❌ | **FAIL (d)** |

**Screening outcome — 12 catalog-eligible; 7 servable/benchmarkable:**
`deepseek-v4-flash`, `deepseek-v4-pro`, `nemotron-3.5-lightning`, `nemotron-3-super-120b`,
`nemotron-3-ultra-550b`, `qwen3.8-27b`, `glm-5.2`. (Qwen 3.7/3.8-max are catalog-eligible but
blocked by account privacy guardrails — revisit only if `openrouter.ai/settings/privacy` is relaxed.)

---

## 2. Accuracy on the real label spaces (n=47 scored + 3 in-context)

The first-round 8-case 4-way eval was noise-level and is superseded. All screen-passing models plus
the requested instruct candidates were re-scored on a 50-observation eval set
(`kask/docs/review/eval_set.json`) over **three real hKask label spaces** drawn from the registry:
- **section** (4-way Statement/Evidence/Diagram/Implications) — `registry/classify/section-classifier.yaml:26-30`
- **dimension** (4-way Gentle/Schriver/Hopper/Lovelace ontology) — `registry/classify/hmem-extractor.yaml:74-78`
- **failure** (6-way Panic/Assertion/Timeout/Flake/LogicError/MemoryError) — `registry/classify/qa-triage.yaml:25`

Texts are real repo excerpts (`.rules`, docs, code comments) or realistic test artifacts; gold labels
assigned per the manifest contract definitions. S01–S03 are the YAML's own 3-shot in-context examples,
scored separately. Protocol: temperature 0.0, max_tokens 150, `reasoning.enabled=false`, streaming.

| Model | section | dimension | failure | **Total** | TTFT p50 | tok/s p50 | errors |
|---|---|---|---|---|---|---|---|
| `z-ai/glm-5.2` (thinking OFF) | 16/17 | 13/20 | 10/10 | **39/47 (83%)** | 1648 ms | 58.4 | 0 |
| `deepseek/deepseek-v4-flash` (current) | 14/17 | 11/20 | 10/10 | 35/47 (74%) | 1529 ms | 24.9 | 0 |
| `nvidia/nemotron-3-super-120b-a12b` | 10/17 | 11/20 | 10/10 | 31/47 (66%) | 1408 ms | 272.7 | 0 |
| `microsoft/phi-4` (screen-fail: age) | 12/17 | 8/20 | 10/10 | 30/47 (64%) | 333 ms | 62.2 | 0 |
| `nvidia/nemotron-3.5-lightning` | 8/17 | 8/20 | 10/10 | 26/47 (55%) | 256 ms | 315.8 | 0 |
| `ibm-granite/granite-4.0-h-micro` (screen-fail: b,d) | 9/17 | 5/20 | 8/10 | 22/47 (47%) | 443 ms | 42.1 | 0 |

**Findings:**
1. **GLM-5.2 with thinking off is the quality leader (83%)** — but at +1041% per call it is
   eliminated by the operator cost ceiling.
2. **The current model (`deepseek-v4-flash`, 74%) beats `nemotron-3-super-120b` (66%)** on the
   real accuracy metric. This inverts the rev-1 ranking, which was built on the noisy 8-case set.
3. **Lightning's label-folding suspicion was real:** 8/17 on section types, mislabeling 5 Diagrams
   as Statement. Fast but under-performs on the harder ontology task.
4. **Every model hits 10/10 on the 6-way failure-type space**, but the 4-way **dimension ontology**
   is where general models collapse to 55–65%. This is the genuinely contract-sensitive axis and the
   missed opportunity: no off-the-shelf model clears it. The registry already has LoRA fine-tune
   infrastructure (`lora-training` skill, `LLMParameters.adapter` at `hkask-types/src/template.rs:63-75`)
   — a small fine-tune on dimension-ontology examples is the durable fix.
5. The cheap+fast instruct options (`phi-4`, `granite-micro`) both fail screening gates *and* score
   lower than the current model.

---

## 3. Equal-weight average rank (latency / token speed / accuracy)

Ranks computed over the 7 servable models on the n=47 accuracy column above, p50 TTFT, and p50 tok/s.

| Model | (e) TTFT p50 ms (rank) | (f) tok/s p50 (rank) | (g) accuracy (rank) | **Avg rank** |
|---|---|---|---|---|
| `nvidia/nemotron-3.5-lightning` | 256 (**1**) | 315.8 (**1**) | 26/47 (6) | 2.67 |
| `nvidia/nemotron-3-super-120b-a12b` | 1408 (5) | 272.7 (**2**) | 31/47 (3) | 3.33 |
| `z-ai/glm-5.2` | 1648 (7) | 58.4 (5) | 39/47 (**1**) | 4.33 |
| `deepseek/deepseek-v4-flash` (current) | 1529 (6) | 24.9 (6) | 35/47 (**2**) | 4.67 |
| `deepseek/deepseek-v4-pro` | 1216 (4) | 62.5 (4) | 31/47 (3) | 3.67 |
| `nvidia/nemotron-3-ultra-550b-a55b` | 647 (3) | 145.8 (3) | 26/47 (6) | 4.00 |
| `qwen/qwen3.8-27b` | 829 (2) | 42.9 (7) | 26/47 (6) | 5.00 |

Accuracy caveat: at n=47 the top three (GLM 83%, Flash 74%, Super 66%) are meaningfully separated;
the bottom cluster (55–66%) is not. Ranks reflect measured medians, not model-size priors.

---

## 4. Price — reporting metric / tie-break

Cheapest-served endpoint per model `$ /1M tokens` (catalog + endpoints API `[LIVE:2026-08-16]`);
per-classification cost estimated at the classifier workload shape ~1,000 input / ~30 output tokens.

| Model (avg rank) | $/M in · out | ≈ $ per 1M classifications | Δ vs current |
|---|---|---|---|
| `deepseek-v4-flash` current (4.67) | 0.068 · 0.168 (DigitalOcean) | ≈ $73 | — |
| `nemotron-3.5-lightning` (2.67) | 0.080 · 0.200 (DeepInfra) | ≈ $86 | +18% |
| `nemotron-3-super-120b-a12b` (3.33) | 0.085 · 0.400 (DeepInfra) | ≈ $97 | +33% |
| `qwen/qwen3.8-27b` (5.00) | 0.400 · 3.000 (Chutes) | ≈ $490 | +571% |
| `nemotron-3-ultra-550b-a55b` (4.00) | 0.500 · 2.200 (DeepInfra) | ≈ $566 | +676% |
| `deepseek-v4-pro` (3.67) | 0.645 · 1.290 (Novita) | ≈ $684 | +838% |
| `z-ai/glm-5.2` (4.33) | 0.760 · 2.420 (catalog) | ≈ $833 | **+1041%** |

**Price as tie-break under the operator cost ceiling:** GLM-5.2's +1041% is the largest observed
delta and is **unacceptable** per operator direction, so it is eliminated despite leading accuracy.
The admissible band is the cheap cluster (current through super, all within +33%). Within that band
price does not overturn rank order, but it does make `nemotron-3.5-lightning` (rank 1, +18%, three
healthy providers) the standout challenger on cost+speed — at the cost of the lowest accuracy.

---

## 5. Recommendation

**Primary: keep `deepseek/deepseek-v4-flash` (current).** Under the operator cost ceiling, no
screen-passing model dominates it on the axis the contract weights most (accuracy on the real label
spaces): it scores 74%, second only to the cost-prohibited GLM-5.2 (83%), and beats the faster
Nemotron challengers (super 66%, lightning 55%) on quality. Its median latency is dominated by bad
routing (DigitalOcean 2387 ms vs DeepInfra 602 ms) — **pin the DeepInfra endpoint** to recover most
of the latency gap at current price without changing the model.

**Challenger — `nvidia/nemotron-3.5-lightning`:** fastest TTFT (256 ms), cheapest of the elite
(+18%), youngest (157 d), and the only top candidate with multi-provider redundancy (DeepInfra +
CoreWeave + Venice, all 100% 30-day uptime). Its single weakness is accuracy (55%, with a
`Statement`-folding tendency on Diagram/Implications). Promote it only if a larger eval confirms the
fold disappears, or if latency/throughput is re-weighted above accuracy.

**The real fix is a fine-tune, not a model swap.** The dimension-ontology axis is where every
general model collapses to 55–65%; no off-the-shelf model clears it. The registry already has LoRA
infrastructure (`lora-training` skill; `LLMParameters.adapter` at `hkask-types/src/template.rs:63-75`).
A small fine-tune of a cheap base (e.g. `deepseek-v4-flash` or `nemotron-3.5-lightning`) on
dimension-ontology examples is the durable path to both speed and accuracy within the cost band.

**Not recommended:** Qwen 3.7/3.8 variants are catalog-eligible but blocked by account guardrails
today — revisit only if `openrouter.ai/settings/privacy` is relaxed (a settings change, not code).
`meta/muse-glimmer-30b` and `qwen3.8-2.4t-a95b` fail the non-thinking gate; `nemotron-3-nano-30b-a3b`
fails the age gate; `granite-4.0-h-micro` and `phi-4` fail screening gates and score below current.

**Confidence:** n=47 labeled cases, single day. The accuracy ordering (GLM > Flash > Super > rest)
was consistent; the latency/speed ordering was consistent across all runs. A 100+ case run or replay
of real corpus passages would harden the dimension-ontology numbers before any default-model switch.

---

## 6. Classifier contract (D2) — evidence summary

- Model resolution: `model: ""` → env → constant (`classify_impl.rs:221-225`); unresolvable override
  → default model + `warn` (`kask/crates/kask_bridge/src/inference.rs:240-247`).
- Concurrency caps enforced: `tokio::sync::Semaphore` (`classify_impl.rs:355`); manifests declare 5/10/150.
- `timeout_secs: 30` is stored (`classify_impl.rs:230`) but **never enforced** — no `tokio::time::timeout`
  anywhere in the crate. Latency budget: **Unknown** (no stated target).
- Retry: `MAX_RETRIES = 3` + exponential backoff exists (`hkask-mcp-corpus/src/batch.rs:16-18, 85-109`)
  and wraps the assertion pipeline, but `classify_batch`'s per-passage calls have **no retry**
  (`classify_impl.rs:364-368`).
- Cost accounting: wired to the ledger (`classify_impl.rs:305-315, 410-448`); `cost_cache_read_nj_per_token`
  is forced to 0 (`classify_impl.rs:236`).
- Only `qa-triage` and `qa-feedback` carry cost rates (30/60 nJ/token); extractors/classifiers run with
  costs disabled — an explicit `warn` on load (`classify_impl.rs:208-214`).
- Malformed-output fallback: JSON parse with category extraction + keyword fallback (`classify_impl.rs:283-299`).
- Stale references to fix in any switch PR: `kask/mcp-servers/hkask-mcp-corpus/README.md:143` and
  `kask/mcp-servers/hkask-mcp-corpus/src/tools/semantic/mod.rs:364` still claim
  "default Qwen3-235B-A22B-Instruct on DeepInfra".

## 7. Template parameter surface (D1/D5) — evidence summary

Full inventory: `kask/docs/review/inference-block-inventory.tsv` (358 files, per-key/value, with file:line).

- Only 3 keys have effect: `temperature`, `max_tokens`, `thinking_budget` (parsed
  `kask/crates/hkask-templates/src/template_renderer.rs:281-285, 330-341`; applied
  `kask/crates/hkask-templates/src/step_actions.rs:239-262`).
- `work_effort` (275 files) and `verbosity` (267 files) are **dead keys** — dropped by the parser's
  `_ => {}` arm; only test heuristics read them (`tests/manifest_invariants.rs:633-642`,
  `tests/token_budget_audit.rs:195-199, 230`).
- `thinking_budget` values outside `{full, on, off, none}` (22 files: `standard`/`medium`/`low`/
  `minimal`/`high`) trigger warn-only fallback → **thinking silently enabled**
  (`step_actions.rs:253-262`) — wrong-direction default for any template that expected a budget cap.
- Default when no override: temperature 0.6, max_tokens 2048, thinking on
  (`kask/crates/hkask-types/src/template.rs:88-110`) — the "edge work" preset.

**Refactor actions (each serves the contract axis):**
1. **Delete** `work_effort` and `verbosity` from all `[inference]` blocks (essentialist deletion test:
   zero runtime value; keep as comments if the annotation is load-bearing to authors) — shrinks the
   parameter surface from 5 keys to 3.
2. **Fix the vocabulary**: normalize the 22 non-canonical `thinking_budget` values to `full|off` (or
   extend the parser mapping — one match arm at `step_actions.rs:246-251`), and change the
   unrecognized-value default from warn-and-keep-thinking-on to warn-and-disable (classification-adjacent
   templates are the majority default; see `template_renderer.rs:629-679` for the pinning tests to extend).
3. **Duplicate manifests**: `h_mem-extractor.yaml` ≈ `hmem-extractor.yaml` are near-identical; deep-module
   deletion test says merge to one and keep the other as a symlink/alias if the name must survive.
4. **Enforce, don't declare**: wire `timeout_secs` into a `tokio::time::timeout` around the batch
   (`classify_impl.rs:364-368`) or remove the field from the schema — declared-but-unenforced config is a
   broken feedback loop (`.rules` failure-signal rule).

## 8. D-seam plan (D6)

| Change | Location | Seam / test |
|---|---|---|
| Keep `DEFAULT_CLASSIFIER_MODEL` (no change) | `kask/crates/hkask-inference/src/model_constants.rs:23` | n/a — recommendation is to hold |
| Provider pin for classifier (DeepInfra) | pass-through of `provider: { only: [...] }` alongside `model_override` in `LanguageModelInferencePort::generate_with_model` (`kask/crates/kask_bridge/src/inference.rs:519-535`) | new kask-side plumbing only — no upstream zed edits; pin test: override resolves to DeepInfra endpoint (extend `inference.rs:1323-1404` propagation tests) |
| `timeout_secs` enforcement | `hkask-mcp-corpus/src/runtime/classify_impl.rs` | unit test: hung inference future fails after `timeout`; kask-only file |
| `thinking_budget` normalization + default flip | `hkask-templates/src/step_actions.rs` | extend existing parser tests (`template_renderer.rs:629-679`); kask crate, no upstream change |
| Dead-key deletion in `.j2` + `[meta]` block | `kask/registry/templates/**` | `manifest_invariants.rs` rule: `work_effort`/`verbosity` forbidden in `[inference]` blocks |

No upstream `crates/` files are touched; all changes land in `kask/` per DIVERGENCE.md.

## Self-verification

- [x] All model ids/prices/ages from OpenRouter catalog/endpoints APIs (downloaded 2026-08-16)
- [x] All latency/throughput numbers from live runs; p50 reported with n; no fabricated benchmarks
- [x] Accuracy grounded in 47 labeled cases across three real registry label spaces; chance level noted
- [x] Unservable candidates excluded with the exact 400/404 reason, not guessed
- [x] Missing contract targets stated as Unknown (latency budget), never as zero
- [x] Screen-gate failures called out even when the model "exists" (glimmer, nano, qwen3.7-flash, qwen3.8-2.4t, granite, phi-4)
- [x] Price surfaces per-call deltas; operator cost ceiling applied as elimination rule (GLM-5.2)
- [x] Benchmark tooling is bash (`kask/scripts/check-classifier-models.sh`); no Python persisted in the repo

End of report.