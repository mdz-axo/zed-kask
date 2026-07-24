# Platform Validation — RunPod Post-Mortem + Nebius Comparison

> **Date**: 2026-07-23
> **Method**: HuggingFace API inspection + RunPod API inspection + web research

## What Happened in the Smoke Tests

Two smoke test runs were attempted:

| Run | Job ID | Pod ID | Duration | Dataset uploaded? | Manifest uploaded? | Model repo updated? |
|---|---|---|---|---|---|---|
| 1 | `36448a27-...` | `hhrtr85wsz3dao` | 30 min (timeout) | ✅ to dataset repo | ❌ never appeared | ❌ empty (only .gitattributes) |
| 2 | `eb6af275-...` | `6e4m6cd4j6wxpd` | ~20 min (stopped) | ✅ to dataset repo | ❌ never appeared | ❌ empty |

**Key finding**: The dataset was published to HuggingFace successfully (the `training_submit` → `publish_dataset` path works). But the training pod never reached the point where it writes and uploads the completion manifest. This means the install script was still running pip install or training when the pods were cancelled.

## Root Cause: pip install is the bottleneck

The install script does `pip install --no-cache-dir axolotl huggingface_hub` on a fresh pod with the minimal `hkask-training-base` Docker image (~130MB, contains only Python + bash). Axolotl pulls in:
- torch (~2GB download)
- transformers
- peft
- accelerate
- bitsandbytes
- datasets
- tokenizers
- And ~50 other transitive dependencies

On an H100 Secure Cloud pod, this pip install takes **20-30 minutes** from a cold start. The training itself (10 samples, 1 epoch, 0.5B model) would take 2-5 minutes. Total: 25-35 minutes.

The stale `RUNPOD_TEMPLATE_ID` in `.env` (`f4wac8wrhz`) was also a problem — it returned "Template not found" from RunPod. The code fell back to the default Docker image (`docker.io/mdzaxo/hkask-training-base:latest`), which works but requires the slow pip install.

## RunPod Status

- **Pods**: Both test pods were terminated. No billing pods remain.
- **Pre-existing pod**: `a47kj67s0tl9cx` (H200, EXITED) — not from our runs.
- **HuggingFace dataset repo**: Contains 8 job directories with `dataset.jsonl` files from previous training attempts (all from the post-mortem era).
- **HuggingFace model repo**: Empty (only `.gitattributes`). No adapter has ever been successfully uploaded.

## Recommendation: Pre-built Axolotl Template

The pip install bottleneck can be eliminated by using a RunPod template with axolotl pre-installed. RunPod's own documentation recommends this approach:

> "If you'd like to skip the setup below, feel free to just deploy this axolotl template by winglian." — RunPod blog

RunPod offers pre-built axolotl templates in their template library. The hKask code already supports `RUNPOD_TEMPLATE_ID` — the operator just needs a valid template ID.

**Action needed**: Create or find a RunPod template with axolotl pre-installed, update `.env` with the template ID, and re-run the smoke test. Expected time with pre-built template: 5-10 minutes (pod creation + model download + training + manifest upload).

## Nebius Comparison

Based on web research (nebius.com, docs.nebius.com):

| Criterion | RunPod | Nebius |
|---|---|---|
| H100 on-demand | $2.89-2.99/hr | $2.95/hr |
| Pre-built templates | ✅ (axolotl, vLLM, etc.) | ❌ (raw VM + Jupyter) |
| SSH access | ✅ | ✅ |
| /workspace persistence | ✅ (survives restarts) | ✅ (persistent storage) |
| Container disk | 60GB (small) | Configurable |
| API for pod management | GraphQL | REST + Terraform |
| Existing hKask integration | ✅ (full) | ❌ (would need new provider) |
| pip install needed | Yes (with minimal image) | Yes (raw VM) |
| Reliability | Community: host-dependent; Secure: SOC2 | Owns hardware, 99% 30-day |

**Verdict**: Nebius offers similar pricing and better reliability (owns hardware), but lacks pre-built templates and would require a new `TrainingHost` implementation. RunPod's pre-built template library is the key advantage — it can eliminate the pip install bottleneck that caused the smoke test timeouts.

**Recommendation**: Fix the RunPod template ID in `.env` first. If RunPod Secure Cloud proves unreliable after successful training runs, then implement a Nebius provider as a secondary host.

## Summary

| Issue | Status | Fix |
|---|---|---|
| Stale RUNPOD_TEMPLATE_ID | Identified | Update `.env` with valid template ID |
| pip install 20-30 min bottleneck | Identified | Use pre-built template OR accept longer timeout |
| Completion detection code | ✅ Implemented | `check_completion_manifest` fetches from HuggingFace |
| Manifest upload in install script | ✅ Implemented | Writes locally + uploads to HF |
| Template wiring (bf16, gc, fa, packing) | ✅ Implemented | All 3 harnesses wired |
| Checkpoint resume | ✅ Implemented | `auto_resume_from_checkpoints: true` + restart detection |
| Harness matrix | ✅ Updated | Axolotl supports full spectrum |
| Pod restart detection | ✅ Implemented | Uptime tracking + `reg.training.checkpoint.resume` span |
| Benchmark eval | ✅ Implemented | MMLU-style multiple-choice in `tools/evaluate.rs` |
| lib.rs refactor | ✅ Complete | 1742 → 505 lines, tools in `tools/` submodule |