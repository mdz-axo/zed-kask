# Rust-Native LoRA Training on RunPod — Research Report

> **Question**: Can we use a pure Rust Docker container on RunPod to train LoRA
> adapters, eliminating the Python/axolotl dependency entirely?
>
> **Date**: 2026-07-20
> **Author**: agent (research session, revised after deep OxiCUDA review)

---

## TL;DR

**Yes.** The OxiCUDA stack (v0.5.0, 73 crates, 1.3M SLoC, 38,622 tests) provides
every primitive needed for Rust-native LoRA training:

- **PEFT** (`oxicuda-peft`): LoRA, QLoRA, AdaLoRA, DoRA, PiSSA — 790 tests, adapter save/load (OXPA format)
- **Training** (`oxicuda-train`): GpuAdamW, GpuLion, GpuCAME, GpuMuon, LR schedulers, gradient clipping, AMP, ZeRO — 105 tests
- **DNN** (`oxicuda-dnn`): FlashAttention, RMSNorm, MoE, RoPE — 1,262 tests
- **BLAS** (`oxicuda-blas`): GEMM (F16/BF16/F32/F64/FP8), batched GEMM — 965 tests
- **LM** (`oxicuda-lm`): BPE tokenizer, LLaMA/GPT-2 model implementations — 182 tests
- **Inference** (`oxicuda-infer`): KV-cache, paged attention, sampling — 138 tests

The only runtime dependency is `libcuda.so` (the NVIDIA driver, already on RunPod
pods). No Python, no CUDA SDK, no nvcc, no pip install.

**Image size**: ~110MB (debian-slim + static Rust binary).

---

## The OxiCUDA Stack

OxiCUDA is a pure Rust CUDA replacement built by COOLJAPAN OU. It replaces the
entire NVIDIA CUDA Toolkit software stack with type-safe, memory-safe Rust code.
The only runtime dependency is the NVIDIA driver (`libcuda.so`).

### Key statistics (v0.5.0, 2026-07-14)

| Metric | Value |
|---|---|
| Crates | 73 (workspace members) |
| SLoC | ~1,300,000 |
| Tests | 38,622 passing (`--all-features`) |
| Clippy warnings | 0 |
| `unwrap()` in library code | 0 (zero-unwrap policy) |
| External dependencies | 5 (libloading, thiserror, num-complex, half, serde) |
| GPU architectures | SM 7.5 (Turing) through SM 10.0 (Blackwell) |
| Production audit | 109 findings, 106 confirmed, 95 fixed |

### Crates relevant to LoRA training

| Crate | Vol | SLoC | Tests | Purpose |
|---|---|---|---|---|
| `oxicuda-peft` | 42 | 23,516 | 790 | LoRA, QLoRA, AdaLoRA, DoRA, PiSSA, IA³, Prefix-Tuning, model merging |
| `oxicuda-train` | 8 | 11,760 | 364 | GpuAdamW, GpuLion, GpuCAME, GpuMuon, LR schedulers, grad clip, AMP, ZeRO |
| `oxicuda-dnn` | 4 | 47,562 | 1,262 | FlashAttention, RMSNorm, MoE, RoPE, conv, pooling, quantization |
| `oxicuda-blas` | 3 | 33,597 | 965 | GEMM (F16/BF16/F32/F64/FP8), batched GEMM, elementwise, reductions |
| `oxicuda-lm` | 13 | 7,025 | 275 | BPE tokenizer, LLaMA/GPT-2 models, KV-cache, weight loading |
| `oxicuda-infer` | 11 | 10,687 | 405 | PagedAttention, speculative decoding, sampling |
| `oxicuda-quant` | 10 | 8,811 | 288 | INT8/INT4/NF4/FP8 quantization (QLoRA support) |
| `oxicuda-gen` | 17 | 16,532 | 596 | LoRA adapter for diffusion models, VAE, schedulers |

### PEFT method coverage (`oxicuda-peft`)

The PEFT crate covers the full lora-training skill's method catalog:

| Method | OxiCUDA module | lora-training gate |
|---|---|---|
| LoRA | `lora::lora::LoraLinear` | G4=default |
| QLoRA | `lora::qlora::QloraLinear` (NF4 quantization) | G2=memory-bound |
| AdaLoRA | `lora::adalora::AdaloraLinear` (SVD-parameterized) | — |
| DoRA | `lora::dora::DoraLinear` (magnitude+direction) | G4=cost-sensitive |
| PiSSA | `lora::pissa::PissaLinear` (SVD of base weight) | G4=fast convergence |
| LoRA-FA | `lora::lora_fa::LoraFaLinear` (frozen A) | — |
| LoRA+ | `lora::lora_plus::LoraPlusLinear` (different LR) | — |
| VeRA | `lora::vera::VeraLinear` (shared random projection) | — |
| LoHa | `lora::loha::LohaLinear` (Hadamard product) | — |
| LoKr | `lora::lokr::LokrLinear` (Kronecker product) | — |
| MoLoRA | `lora::molora::MoLoRA` (mixture of LoRA) | — |
| OLoRA | `lora::olora::OloraLinear` (orthogonal init) | — |
| QA-LoRA | `lora::qa_lora::QaLoraLinear` (quantization-aware) | — |
| AWQ | `lora::awq::AwqQuantizer` (activation-aware) | — |
| GPTQ | `lora::gptq::GptqQuantizer` (Hessian-based) | — |
| HQQ | `lora::hqq::HqqQuantizer` (half-quantized) | — |

### Adapter serialization (`oxicuda-peft::io`)

The `io` module provides:
- `AdapterPayload` — named collection of `f32` tensors representing adapter state
- `OXPA` format — self-describing binary container with FNV-1a checksum
- `AdapterRegistry` — hub convention for multiple task adapters keyed by `base_model` + `task` + `name`
- `save_to_file()` / `load_from_file()` — pure Rust, no serde/bincode

### Training engine (`oxicuda-train`)

The training crate provides a full GPU-accelerated training stack:

- **GPU optimizers**: GpuAdamW, GpuAdam, GpuLion, GpuCAME, GpuMuon, GpuRAdam, GpuRMSProp, GpuAdaGrad
- **Gradient clipping**: global norm clip, per-layer clip, value clip
- **Gradient accumulation**: micro-batch accumulation with configurable step count
- **Gradient checkpointing**: activation recomputation (uniform, selective, offload policies)
- **LR schedulers**: 11 variants (constant, step, multi-step, exponential, cosine, warmup+cosine, polynomial, 1cycle, cyclic, reduce-on-plateau)
- **ZeRO**: Stage 1/2/3 optimizer state sharding for distributed training
- **AMP**: GradScaler with dynamic loss scaling
- **EMA**: Exponential Moving Average of model parameters

The training API is clean and simple:

```rust
use oxicuda_train::gpu_optimizer::{GpuOptimizer, ParamTensor};
use oxicuda_train::gpu_optimizer::adamw::GpuAdamW;
use oxicuda_train::grad_clip::clip_grad_norm;
use oxicuda_train::lr_scheduler::{LrScheduler, WarmupCosine};

// Build model parameters
let mut params = vec![ParamTensor::new(vec![0.5f32; 1024], "lora_A"), ...];

// Create AdamW optimizer
let mut opt = GpuAdamW::new(3e-4).with_weight_decay(0.01);

// Create LR scheduler
let mut sched = WarmupCosine::new(3e-4, 500, 10_000);

// Training loop
for step in 0..10_000u64 {
    // ... compute gradients into params[i].grad ...
    clip_grad_norm(&mut params, 1.0)?;
    let lr = sched.step();
    opt.set_lr(lr);
    opt.step(&mut params)?;
    opt.zero_grad(&mut params);
}
```

### Model loading (`oxicuda-lm`)

The LM crate has:
- `WeightTensor { data, shape }` — named tensor with shape validation
- `ModelWeights` — HashMap-backed weight store with `get_checked()`
- `load_llama_block()` — HuggingFace key convention loader for LLaMA models
- `load_gpt2_block()` — HuggingFace key convention loader for GPT-2 models
- `BpeTokenizer` — byte-level BPE tokenizer (HuggingFace-compatible)
- `LlamaModel` / `Gpt2Model` — full model implementations with `next_token()` greedy decode

**Gap**: The LM crate has LLaMA and GPT-2 loaders but not Qwen3. We would need
to write a Qwen3 weight loader (or use the `hf-hub` crate to download safetensors
and parse them into `WeightTensor` objects). This is ~100 lines of code.

---

## Architecture: Rust LoRA Training Binary

```
┌──────────────────────────────────────────────────────────┐
│              hkask-lora-trainer (Rust binary)             │
│                                                          │
│  ┌──────────────┐  ┌───────────────┐  ┌──────────────┐  │
│  │ hf-hub       │  │ oxicuda-lm    │  │ oxicuda-train│  │
│  │ (download    │  │ (BPE tokenizer│  │ (GpuAdamW,   │  │
│  │  model +     │  │  weight load) │  │  LR sched,   │  │
│  │  dataset)    │  │               │  │  grad clip)  │  │
│  └──────┬───────┘  └───────┬───────┘  └──────┬───────┘  │
│         │                  │                 │          │
│         ▼                  ▼                 ▼          │
│  ┌──────────────────────────────────────────────────┐   │
│  │     oxicuda-peft (LoRA wrapper on q/k/v/o)       │   │
│  │     oxicuda-dnn (FlashAttention, RMSNorm)        │   │
│  │     oxicuda-blas (GEMM)                          │   │
│  └──────────────────────┬───────────────────────────┘   │
│                         │                               │
│                         ▼                               │
│  ┌──────────────────────────────────────────────────┐   │
│  │           oxicuda-driver (CUDA via libcuda.so)   │   │
│  │           oxicuda-memory (DeviceBuffer)          │   │
│  │           oxicuda-launch (kernel launch)         │   │
│  └──────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────┘
```

**Dependencies** (all pure Rust, git dependency on OxiCUDA):
- `oxicuda` (umbrella) — or individual crates: `oxicuda-peft`, `oxicuda-train`, `oxicuda-dnn`, `oxicuda-blas`, `oxicuda-lm`
- `hf-hub` v1.0.0 — HuggingFace model/dataset download (crates.io)
- `tokenizers` v0.22.2 — HuggingFace tokenizer (crates.io, used by transformers)
- `serde` + `serde_json` — config parsing
- `clap` — CLI args

**Docker image**:
```dockerfile
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    libcuda1  # NVIDIA driver userspace (provided by RunPod host)
COPY hkask-lora-trainer /usr/local/bin/
ENTRYPOINT ["/usr/local/bin/hkask-lora-trainer"]
```

**Image size**: ~80MB (debian-slim) + ~30MB (static Rust binary) = ~110MB.

---

## What We'd Need to Build

### 1. The training binary (~500-800 lines of Rust)

The binary orchestrates the OxiCUDA crates:

1. **Download base model** from HuggingFace (`hf-hub` crate)
2. **Load model weights** into `oxicuda-lm::ModelWeights` (need Qwen3 loader — ~100 lines)
3. **Wrap with LoRA** using `oxicuda-peft::LoraLinear` on q_proj, k_proj, v_proj, o_proj
4. **Download + tokenize dataset** (`hf-hub` + `tokenizers` crate)
5. **Training loop** using `oxicuda-train::GpuAdamW` + `WarmupCosine` scheduler
6. **Save adapter** using `oxicuda-peft::io::AdapterPayload::save_to_file()`
7. **Upload to HuggingFace** (`hf-hub` crate's upload API)

### 2. Qwen3 weight loader (~100 lines)

The `oxicuda-lm` crate has `load_llama_block()` and `load_gpt2_block()` but not
Qwen3. Qwen3 is architecturally similar to LLaMA (decoder-only, RMSNorm, SwiGLU,
RoPE) so the loader is a straightforward adaptation of `load_llama_block()`.

### 3. The Docker image (~10 lines Dockerfile)

Static Rust binary + debian-slim. No Python, no pip, no CUDA SDK.

---

## Comparison: Python/axolotl vs Rust/OxiCUDA

| Aspect | Python/axolotl | Rust/OxiCUDA |
|---|---|---|
| Docker image size | 10GB+ (pre-built) or 129MB+pip (fails) | ~110MB (static binary) |
| pip install at startup | Required (causes restart loops) | Not needed |
| Python dependency | Yes (policy violation) | No |
| CUDA SDK | Required (nvcc, cudart) | Not needed (libcuda.so only) |
| Model loading | transformers (mature) | oxicuda-lm (has LLaMA/GPT-2, need Qwen3) |
| LoRA training | axolotl/PEFT (battle-tested) | oxicuda-peft (790 tests, CPU+PTX) |
| Optimizer | AdamW via torch | GpuAdamW via oxicuda-train (PTX kernels) |
| Gradient checkpointing | Built-in | oxicuda-train::CheckpointManager |
| Flash attention | Built-in | oxicuda-dnn::FlashAttention |
| EVA/PiSSA init | PEFT supports it | oxicuda-peft has PiSSA module |
| Adapter save/load | PEFT format (safetensors) | OXPA format (pure Rust) |
| Production maturity | High | Medium (v0.5.0, 38K tests, audited) |

---

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| OxiCUDA not on crates.io (git dep) | Low | Low | Add as git dependency in Cargo.toml |
| Qwen3 loader doesn't exist | High | Low | Write it (~100 lines, based on LLaMA loader) |
| OXPA format incompatible with PEFT | Medium | Medium | Write a safetensors exporter (~50 lines) |
| GPU PTX kernels not tested on H100 | Medium | High | PoC with a simple GEMM on H100 first |
| Training slower than axolotl | Medium | Low | Acceptable — we trade speed for simplicity |
| No sample packing | Low | Low | Not needed for initial version |

---

## Recommended Next Step

**Build a proof-of-concept** (~2-4 hours, ~$1 of GPU time):

1. Create a new Rust binary crate `hkask-lora-trainer`
2. Add OxiCUDA as a git dependency
3. Load a small model (Qwen3-0.5B) on GPU via oxicuda-driver
4. Run a simple GEMM via oxicuda-blas to verify CUDA works
5. Wrap a Linear layer with `oxicuda-peft::LoraLinear`
6. Run 1 training step using `oxicuda-train::GpuAdamW`
7. Save the adapter via `oxicuda-peft::io::AdapterPayload`

If the PoC works, we build the full training binary and the image problem is
solved permanently — no Python, no pip install, no restart loops, ~110MB image.

---

## Key References

- [OxiCUDA GitHub](https://github.com/cool-japan/oxicuda) — pure Rust CUDA replacement
- [OxiCUDA TODO.md](https://github.com/cool-japan/oxicuda/blob/master/TODO.md) — full roadmap with 38,622 tests
- [oxicuda-peft README](https://github.com/cool-japan/oxicuda/tree/master/crates/oxicuda-peft) — PEFT primitives
- [oxicuda-train lib.rs](https://github.com/cool-japan/oxicuda/blob/master/crates/oxicuda-train/src/lib.rs) — training engine API
- [oxicuda-lm](https://github.com/cool-japan/oxicuda/tree/master/crates/oxicuda-lm) — BPE tokenizer + LLaMA/GPT-2 models
- [hf-hub](https://crates.io/crates/hf-hub) — HuggingFace API client for Rust
- [Candle](https://github.com/huggingface/candle) — alternative Rust ML framework (HuggingFace)
- [candle-lora](https://github.com/EricLBuehler/candle-lora) — LoRA for Candle (mistral.rs author)
