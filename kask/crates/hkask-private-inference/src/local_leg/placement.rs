//! L3 — the placement compiler: (model metadata, hardware, policy, engine
//! surface manifest) → a guard-verified LaunchPlan.
//!
//! All guard arithmetic is deterministic and unit-tested; the drift lives
//! in the manifest (L1) and its constraint forms (L2), not here. Any guard
//! refusal is a typed error naming the violated constraint and the numbers
//! that tripped it — fail-closed, never a silent fallback (FR-3 spirit).

use std::path::{Path, PathBuf};

use crate::local_leg::gguf::GgufMeta;
use crate::local_leg::launch::LaunchPlan;
use crate::local_leg::manifest::{
    ConstraintEnv, EngineSurfaceManifest, Evaluation, constraint_name, evaluate_constraint,
};
use crate::local_leg::model_brief::{DraftSpec, ModelBrief};

#[derive(Debug, thiserror::Error)]
pub enum PlacementError {
    #[error("hardware probe failed: {0}")]
    Probe(String),
    #[error("guard `{constraint}` refused the launch: {reason}")]
    GuardDenied { constraint: String, reason: String },
    #[error("constraint evaluation failed: {0}")]
    Constraint(#[from] crate::local_leg::manifest::ManifestError),
    #[error(
        "quantized kv_cache_type `{0}` requires flash_attn to be enabled (engine constraint D1, observed live 2026-11-04)"
    )]
    QuantizedKvRequiresFlashAttn(String),
    #[error(
        "draft emission asked but the model brief `{0}` declares no [draft] spec — the draft is per-model data, never invented by the compiler"
    )]
    DraftBriefMissing(String),
    #[error(
        "draft emission asked but the engine surface `{0}` spells no speculative-draft vocabulary — the draft flags were never verified against this pin"
    )]
    DraftVocabularyMissing(String),
    #[error(
        "draft module `{path}` not found — a draft plan requires the brief's module file on disk (fail-closed at emit, the truncated-shard discipline)"
    )]
    DraftModuleMissing { path: String },
    #[error(
        "partial GPU offload asked but the model carries no usable block_count in its GGUF metadata — the verified layer split cannot be computed (fail-closed, never guessed)"
    )]
    PartialOffloadNeedsBlockCount,
    #[error(
        "no-mmap asked but the engine surface `{0}` spells no load-mode vocabulary — the spelling was never verified against this pin"
    )]
    NoMmapVocabularyMissing(String),
}

/// Hardware inventory. `available_ram_bytes` is the number the guards use
/// (plan hypothesis H4): MemAvailable from /proc/meminfo already accounts
/// for reclaimable cache and co-resident services — total RAM is carried
/// for telemetry, never for the guard.
#[derive(Debug, Clone)]
pub struct HardwareInventory {
    pub total_ram_bytes: u64,
    pub available_ram_bytes: u64,
    /// VRAM with provenance: sysfs-detected from the amdgpu driver when
    /// the dGPU exposes it, caller-declared otherwise. An absent budget
    /// fails closed wherever it matters (`VramDenseFit`).
    pub vram: Option<VramSource>,
    pub provenance: &'static str,
}

/// Where the VRAM figure came from — provenance is carried on the
/// number, never flattened: a `Sysfs` figure was read off the running
/// amdgpu driver, a `Declared` figure is a caller-supplied estimate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VramSource {
    /// Read from an amdgpu sysfs node (`mem_info_vram_total`).
    Sysfs { card_path: PathBuf, bytes: u64 },
    /// Supplied by the caller (pins, pre-probed configs) — an estimate.
    Declared { bytes: u64 },
}

impl VramSource {
    pub fn bytes(&self) -> u64 {
        match self {
            Self::Sysfs { bytes, .. } | Self::Declared { bytes } => *bytes,
        }
    }
}

/// The GPU backend the placement compiler can target on this machine.
/// Auto-detected by `probe_backend()`: the first backend whose build
/// exists on disk wins. The compiler emits the binary path from this
/// enum, never a hardcoded HIP/Vulkan — the measured HIP > Vulkan > CPU
/// order (baseline record §4/§5, 2026-11-04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuBackend {
    /// ROCm/HIP backend (gfx1102-class AMD dGPU).
    Hip { binary: PathBuf },
    /// Vulkan backend (any GPU with a Vulkan 1.2 driver).
    Vulkan { binary: PathBuf },
    /// No GPU backend: CPU-only.
    None,
}

impl GpuBackend {
    /// The llama-server binary this backend launches.
    pub fn binary(&self) -> Option<&Path> {
        match self {
            Self::Hip { binary } | Self::Vulkan { binary } => Some(binary),
            Self::None => None,
        }
    }
}

/// The llama.cpp engine root — the canonical engine tree under the
/// artifacts root (`{artifacts}/private-inference/engines/llama-cpp-mainline/`,
/// the one-path artifacts location the consolidation plan pins; the
/// build script `kask/scripts/build-llamacpp-mainline.sh` materializes it).
/// Resolved through `agent_paths` — the HKASK_ARTIFACTS_DIR → XDG documents
/// → platform-default chain, the same root class the bridge uses for the
/// models root — never a literal home-dir string. The engine tree is a
/// build artifact (multi-GiB, user-inspectable), so it lives under the
/// visible artifacts root, not the hidden internal-data root.
pub fn engine_root() -> PathBuf {
    hkask_types::agent_paths::resolve_under_artifacts_dir(Path::new(
        "private-inference/engines/llama-cpp-mainline",
    ))
}

/// Probe which GPU backend this machine can serve from. Walks the
/// engine root's build tree looking for `llama-server` in `build-hip/`
/// (ROCm) then `build-vulkan/` (Vulkan) — first hit wins, the measured
/// preference order (HIP primary, Vulkan fallback; baseline record §5).
/// The probe answers "which build exists on disk?" — the same question
/// the P0 hand-launcher answered by prefixing the binary, now against
/// the canonical root.
///
/// On a machine without either build, the answer is `GpuBackend::None`
/// and the compiler emits a CPU-only plan.
pub fn probe_backend() -> GpuBackend {
    probe_backend_at(&engine_root())
}

/// The engine-build probe against an explicit root — the test seam: the
/// HIP → Vulkan → None order is pinned over a stubbed root, no env or
/// home-dir involved.
pub fn probe_backend_at(root: &Path) -> GpuBackend {
    let hip = root.join("build-hip/bin/llama-server");
    if hip.exists() {
        return GpuBackend::Hip { binary: hip };
    }

    let vulkan = root.join("build-vulkan/bin/llama-server");
    if vulkan.exists() {
        return GpuBackend::Vulkan { binary: vulkan };
    }

    GpuBackend::None
}

/// The live sysfs root the VRAM probe walks.
pub const SYSFS_DRM_ROOT: &str = "/sys/class/drm";

/// Probe every amdgpu card under `drm_root` and return the largest VRAM
/// as the dGPU candidate. Only amdgpu devices expose
/// `device/mem_info_vram_total`, so node presence is the amdgpu marker;
/// connector (`cardN-DP-1`) and render (`renderD128`) nodes are skipped
/// by name. Largest-wins is the dGPU rule — verified against the 890M
/// iGPU's BIOS carve-out (512 MiB) vs the 7700S (~8 GiB), never assumed.
/// An unreadable root or a malformed VRAM node fails loud; a machine
/// with no amdgpu card is `Ok(None)` — the absence the `VramDenseFit`
/// guard fails closed on.
pub fn probe_vram_at(drm_root: &Path) -> Result<Option<VramSource>, PlacementError> {
    let cards = std::fs::read_dir(drm_root)
        .map_err(|e| PlacementError::Probe(format!("{}: {e}", drm_root.display())))?;
    let mut best: Option<VramSource> = None;
    for card in cards {
        let Ok(card) = card else {
            continue;
        };
        let file_name = card.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if !is_drm_card_node(name) {
            continue;
        }
        let device_dir = card.path().join("device");
        let mem_info = device_dir.join("mem_info_vram_total");
        let text = match std::fs::read_to_string(&mem_info) {
            Ok(text) => text,
            // Not an amdgpu card: no VRAM node is that card's expected shape.
            Err(_) => continue,
        };
        let bytes = text.trim().parse::<u64>().map_err(|_| {
            PlacementError::Probe(format!(
                "malformed amdgpu VRAM node {}: {text:?}",
                mem_info.display()
            ))
        })?;
        if bytes == 0 {
            continue;
        }
        let candidate = VramSource::Sysfs {
            card_path: device_dir,
            bytes,
        };
        if best.as_ref().is_none_or(|b| candidate.bytes() > b.bytes()) {
            best = Some(candidate);
        }
    }
    Ok(best)
}

/// `cardN` and nothing else — connectors and writeback nodes carry the
/// `cardN-` prefix, render nodes the `renderD` prefix.
fn is_drm_card_node(name: &str) -> bool {
    let rest = name.strip_prefix("card").unwrap_or("");
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

/// Live VRAM probe over the machine's drm sysfs tree.
/// Multi-vendor VRAM probe. Tries the amdgpu sysfs path first (this
/// machine’s primary), then the NVIDIA sysfs path
/// (`/proc/driver/nvidia/gpus/*/information`). Neither finding a
/// device is `Ok(None)` — CPU-only, fail-closed.
///
/// Apple Silicon (unified memory) has no discrete VRAM: the CPU
/// fallback is correct there (the KV cache is guarded as anonymous
/// RAM, same as CPU-only).
pub fn probe_vram() -> Result<Option<VramSource>, PlacementError> {
    // amdgpu first: this is the primary path on this machine (gfx1102).
    if let Some(vram) = probe_vram_at(Path::new(SYSFS_DRM_ROOT))? {
        return Ok(Some(vram));
    }
    // NVIDIA fallback: read the first GPU’s total VRAM from the sysfs
    // tree. The format is `MemTotal: 8192 MiB` (no subprocess needed).
    let nvidia_info = Path::new("/proc/driver/nvidia/gpus/0/information");
    if let Ok(text) = std::fs::read_to_string(nvidia_info) {
        for line in text.lines() {
            if let Some(rest) = line.trim().strip_prefix("Memory Total:") {
                let trimmed = rest.trim().trim_end_matches(" MiB");
                if let Ok(bytes_mib) = trimmed.parse::<u64>() {
                    return Ok(Some(VramSource::Declared {
                        bytes: bytes_mib.saturating_mul(1024 * 1024),
                    }));
                }
            }
        }
    }
    Ok(None)
}

/// Read the machine's RAM inventory from /proc/meminfo and its dGPU VRAM
/// from the amdgpu sysfs nodes.
pub fn probe_hardware() -> Result<HardwareInventory, PlacementError> {
    let text = std::fs::read_to_string("/proc/meminfo")
        .map_err(|e| PlacementError::Probe(format!("/proc/meminfo: {e}")))?;
    let mut total = None;
    let mut available = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total = meminfo_kb(rest);
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            available = meminfo_kb(rest);
        }
    }
    let vram = probe_vram()?;
    match (total, available) {
        (Some(total_ram_bytes), Some(available_ram_bytes)) => Ok(HardwareInventory {
            total_ram_bytes,
            available_ram_bytes,
            vram,
            provenance: "/proc/meminfo; /sys/class/drm mem_info_vram_total",
        }),
        _ => Err(PlacementError::Probe(
            "MemTotal/MemAvailable missing from /proc/meminfo".into(),
        )),
    }
}

fn meminfo_kb(rest: &str) -> Option<u64> {
    rest.split_whitespace()
        .next()
        .and_then(|kb| kb.parse::<u64>().ok())
        .map(|kb| kb.saturating_mul(1024))
}

/// PI-owned policy knobs. Defaults are declared estimates with named
/// provenance — the arithmetic over them is deterministic (plan §7). The
/// estimates can only make the guard over-refuse, never under-refuse.
#[derive(Debug, Clone)]
pub struct PlacementPolicy {
    pub context_length: u32,
    /// Declared estimate: hybrid-attention KV bytes per token.
    pub kv_bytes_per_token: u64,
    /// Declared estimate: llama-server compute buffer allocation.
    pub compute_buffer_bytes: u64,
    pub kv_cache_type: String,
    pub flash_attn: bool,
    /// Keep routed experts in RAM while dense layers go to the GPU. Only
    /// emitted when a GPU tier exists (`n_gpu_layers > 0`) — inert and
    /// omitted on a GPU-less build.
    pub cpu_moe: bool,
    pub n_gpu_layers: u32,
    /// Ask for the model's speculative-draft module (the brief's
    /// `[draft]` table): the compiler resolves the module file, routes
    /// it (GPU when the VRAM arithmetic leaves room, else CPU `-ngld 0`)
    /// and emits the spec flags. Absent (false) on every plan that does
    /// not ask — no behavior change without the ask.
    pub draft: bool,
    /// Partial GPU offload (the DME route capability, consolidation P4):
    /// when the full dense set will not fit the declared VRAM, compute
    /// the VERIFIED layer count that fits and emit that `-ngl` — the
    /// non-offloaded layers ride the host by llama.cpp design — instead
    /// of the whole-offload refusal. The split is computed from the
    /// parse's own per-tensor byte accounting (offset-gap spans include
    /// alignment padding — conservative: over-refusing at the boundary,
    /// never under-reserving), and the guard evaluates the OFFLOADED
    /// share through the pinned-cpu channel. Default false: no behavior
    /// change for any existing caller; an ask where nothing fits still
    /// leaves the full ask to the guard's numeric refusal.
    pub partial_gpu_offload: bool,
    /// Load the model WITHOUT memory mapping (the 2026-11-07 dGPU
    /// link-loss investigation: mmap'd weights under the HIP backend drive
    /// the KFD userptr/SVM churn that preceded every PCIe link loss). The
    /// surface spells the argv fragment; the guard then counts weights as
    /// resident RAM (`model_mmap=false`). Default false: no behavior
    /// change for any existing caller — the ask is always explicit.
    pub no_mmap: bool,
    pub threads: u32,
    pub host: String,
    pub port: u16,
}

impl Default for PlacementPolicy {
    fn default() -> Self {
        Self {
            context_length: 65_536,
            kv_bytes_per_token: 8_192,
            compute_buffer_bytes: 4 * 1024 * 1024 * 1024,
            kv_cache_type: "q8_0".into(),
            flash_attn: true,
            cpu_moe: true,
            n_gpu_layers: 999,
            draft: false,
            partial_gpu_offload: false,
            no_mmap: false,
            threads: 12,
            host: "127.0.0.1".into(),
            port: 8010,
        }
    }
}

/// Compile a launch plan. Every manifest constraint is evaluated against
/// the plan's own env. Streaming-mode launches are unroutable in v1 — the
/// slot fields stay `None` (plan §6, the variety-deficit attenuation).
///
/// The model brief supplies the model-level policy (PLE pin pattern,
/// expert counts); the hardware probe supplies the machine-level budget;
/// the engine manifest supplies the flag spellings. The compiler
/// composes all three.
pub fn compile_placement(
    model_path: PathBuf,
    meta: &GgufMeta,
    hw: &HardwareInventory,
    brief: &ModelBrief,
    policy: &PlacementPolicy,
    manifest: &EngineSurfaceManifest,
) -> Result<LaunchPlan, PlacementError> {
    // D1 (engine constraint, observed live 2026-11-04): a quantized V
    // cache requires flash_attn to be enabled. Refuse at emit time with
    // a typed error rather than letting the engine refuse at load.
    let quantized_kv = !matches!(policy.kv_cache_type.as_str(), "f16" | "f32");
    if quantized_kv && !policy.flash_attn {
        return Err(PlacementError::QuantizedKvRequiresFlashAttn(
            policy.kv_cache_type.clone(),
        ));
    }

    let kv_bytes_estimate = policy
        .kv_bytes_per_token
        .saturating_mul(u64::from(policy.context_length));

    // Dense bytes the pin keeps on CPU — excluded from the VRAM budget.
    // The pattern comes from the model brief (M9), not the policy: the
    // brief carries per-model tensor names (Qwen3.8 has a PLE table,
    // Gemma 4 doesn't), the policy carries machine-level knobs.
    let mut pinned_cpu_bytes = if brief.placement.ple_pin_pattern.is_empty() {
        0
    } else {
        meta.tensors
            .iter()
            .filter(|t| !t.is_expert && t.name.contains(&brief.placement.ple_pin_pattern))
            .map(|t| t.approx_bytes)
            .fold(0u64, |acc, bytes| acc.saturating_add(bytes))
    };

    // Partial GPU offload (policy-declared, the DME route capability):
    // when the full dense set will not fit the declared VRAM, compute
    // the VERIFIED layer split that fits and emit that -ngl — the
    // non-offloaded layers ride the host by llama.cpp design. Byte
    // accounting is exact-per-tensor over the parse's own offset-gap
    // spans (padding included — conservative: over-refusing at the
    // boundary, never under-reserving); the non-block dense share
    // (embeddings, norms, output) is conservatively counted on the GPU
    // side. The guard then evaluates the OFFLOADED share through the
    // pinned-cpu channel. When nothing fits, the full ask stands and
    // the guard denies with its own numbers — never a silent CPU-only
    // substitution.
    let mut n_gpu_layers = policy.n_gpu_layers;
    if policy.partial_gpu_offload && n_gpu_layers > 0 {
        if let Some(vram) = hw.vram.as_ref() {
            let offloadable_dense = meta.dense_bytes.saturating_sub(pinned_cpu_bytes);
            let full_resident = offloadable_dense
                .saturating_add(kv_bytes_estimate)
                .saturating_add(policy.compute_buffer_bytes);
            if full_resident > vram.bytes() {
                let n_layers = u64::from(
                    meta.block_count()
                        .ok_or(PlacementError::PartialOffloadNeedsBlockCount)?,
                );
                if n_layers == 0 {
                    return Err(PlacementError::PartialOffloadNeedsBlockCount);
                }
                // Per-layer dense byte shares from the parse's accounting;
                // experts never enter the VRAM channel (--cpu-moe).
                let mut layer_bytes = vec![0u64; n_layers as usize];
                let mut non_block_dense = 0u64;
                for t in &meta.tensors {
                    if t.is_expert {
                        continue;
                    }
                    if let Some(rest) = t.name.strip_prefix("blk.") {
                        if let Some((idx, _)) = rest.split_once('.') {
                            if let Ok(i) = idx.parse::<usize>()
                                && i < n_layers as usize
                            {
                                layer_bytes[i] = layer_bytes[i].saturating_add(t.approx_bytes);
                                continue;
                            }
                        }
                    }
                    non_block_dense = non_block_dense.saturating_add(t.approx_bytes);
                }
                let fixed = non_block_dense
                    .saturating_add(kv_bytes_estimate)
                    .saturating_add(policy.compute_buffer_bytes);
                if fixed < vram.bytes() {
                    let budget = vram.bytes() - fixed;
                    let mut acc = 0u64;
                    let mut fit = 0usize;
                    for (i, lb) in layer_bytes.iter().enumerate() {
                        if acc.saturating_add(*lb) > budget {
                            break;
                        }
                        acc = acc.saturating_add(*lb);
                        fit = i + 1;
                    }
                    if fit > 0 {
                        n_gpu_layers = (fit as u32).min(policy.n_gpu_layers);
                        for lb in layer_bytes.iter().skip(fit) {
                            pinned_cpu_bytes = pinned_cpu_bytes.saturating_add(*lb);
                        }
                    }
                }
            }
        }
    }

    let vocabulary = &manifest.flags;

    // The speculative draft (the unsloth transfer, T4): the brief supplies
    // the per-model data (module file, spec type, draft depth, bytes),
    // the policy supplies the ask, the surface supplies the verified
    // spellings. The module file must exist at emit — the truncated-shard
    // discipline: a draft plan only emits on a complete on-disk set.
    struct DraftPlan<'a> {
        spec: &'a DraftSpec,
        path: PathBuf,
        on_gpu: bool,
        model_draft: String,
        spec_type: String,
        draft_n_max: String,
        draft_ngl: String,
    }
    let draft = if !policy.draft {
        None
    } else {
        let spec = brief
            .draft
            .as_ref()
            .ok_or_else(|| PlacementError::DraftBriefMissing(brief.model_id.clone()))?;
        // The surface must spell every draft flag — a partial vocabulary
        // is a typed refusal, never a silently omitted draft.
        let (Some(model_draft), Some(spec_type), Some(draft_n_max), Some(draft_ngl)) = (
            vocabulary.model_draft.as_deref(),
            vocabulary.spec_type.as_deref(),
            vocabulary.spec_draft_n_max.as_deref(),
            vocabulary.spec_draft_ngl.as_deref(),
        ) else {
            return Err(PlacementError::DraftVocabularyMissing(
                manifest.surface_id.clone(),
            ));
        };
        let path = model_path
            .parent()
            .ok_or_else(|| PlacementError::DraftModuleMissing {
                path: model_path.display().to_string(),
            })?
            .join(&spec.module_file);
        if !path.is_file() {
            return Err(PlacementError::DraftModuleMissing {
                path: path.display().to_string(),
            });
        }
        // Draft placement: the GPU only when the VRAM arithmetic leaves
        // room after the offloadable dense set, the KV estimate and the
        // compute buffer (the estimate class — it can only over-refuse).
        // Else the draft rides the CPU (-ngld 0) — the placement the
        // measured MTP pair ran (2026-11-05: the card leaves ~1.2 GB
        // free against a 2.79 GB draft). KNOWN GAP: this arithmetic
        // reserves the draft module's bytes but NOT the spec-verify
        // workspace (unsloth's guidance: 1-2 GB extra headroom) — the
        // measured pair required ctx 32768 on the 8 GB card for exactly
        // that reason; the named follow-up adds the reserved term.
        let on_gpu = match (hw.vram.as_ref(), policy.n_gpu_layers > 0) {
            (Some(vram), true) => {
                let offloadable = meta.dense_bytes.saturating_sub(pinned_cpu_bytes);
                let used = offloadable
                    .saturating_add(kv_bytes_estimate)
                    .saturating_add(policy.compute_buffer_bytes)
                    // MTP record §7 item 1 (the D-B finding): the spec-decode
                    // verify workspace is GPU-side whenever a draft is asked —
                    // it enters the used set BEFORE the headroom test, so a
                    // budget that would fit the draft without it routes the
                    // draft to CPU instead of under-reserving the verify path
                    // that OOM'd ctx 65536 on the 8 GB card.
                    .saturating_add(spec.verify_workspace_bytes);
                // The exact routing predicate: the budget covers the used
                // set AND the headroom covers the draft. The ordering
                // makes the subtraction underflow-free, and the degenerate
                // zero-byte draft never rides the GPU on a saturated
                // headroom (the corner the T5 proof pins closed).
                vram.bytes() >= used && vram.bytes() - used >= spec.draft_bytes
            }
            _ => false,
        };
        Some(DraftPlan {
            spec,
            path,
            on_gpu,
            model_draft: model_draft.into(),
            spec_type: spec_type.into(),
            draft_n_max: draft_n_max.into(),
            draft_ngl: draft_ngl.into(),
        })
    };

    let mut args: Vec<String> = vec![vocabulary.model.clone(), model_path.display().to_string()];
    args.push(vocabulary.ctx_size.clone());
    args.push(policy.context_length.to_string());
    for spelling in &vocabulary.kv_cache_type {
        args.push(spelling.clone());
        args.push(policy.kv_cache_type.clone());
    }
    // Always explicit: the -fa on/off A/B on gfx1102 is load-bearing
    // (plan §4), so the plan states the setting instead of relying on
    // engine defaults.
    args.push(vocabulary.flash_attn.clone());
    args.push(if policy.flash_attn { "on" } else { "off" }.into());
    // The no-mmap intent: the surface spells the whole argv fragment; a
    // vocabulary-less surface is a typed refusal, never a silently mmap'd
    // launch (the draft-vocabulary discipline).
    if policy.no_mmap {
        let fragment = vocabulary
            .no_mmap
            .as_ref()
            .ok_or_else(|| PlacementError::NoMmapVocabularyMissing(manifest.surface_id.clone()))?;
        args.extend(fragment.iter().cloned());
    }
    if n_gpu_layers > 0 {
        args.push(vocabulary.n_gpu_layers.clone());
        args.push(n_gpu_layers.to_string());
        if policy.cpu_moe {
            args.push(vocabulary.cpu_moe.clone());
        }
        if !brief.placement.ple_pin_pattern.is_empty() {
            args.push(vocabulary.override_tensor.clone());
            args.push(format!("{}=CPU", brief.placement.ple_pin_pattern));
        }
    }
    if let Some(draft) = &draft {
        args.push(draft.model_draft.clone());
        args.push(draft.path.display().to_string());
        args.push(draft.spec_type.clone());
        args.push(draft.spec.spec_type.clone());
        args.push(draft.draft_n_max.clone());
        args.push(draft.spec.draft_n_max.to_string());
        args.push(draft.draft_ngl.clone());
        // The draft's own offload tier: the GPU tier's layer count when
        // the arithmetic placed it there, else explicitly CPU-pinned.
        args.push(if draft.on_gpu {
            policy.n_gpu_layers.to_string()
        } else {
            "0".to_string()
        });
    }
    args.push(vocabulary.threads.clone());
    args.push(policy.threads.to_string());
    args.push(vocabulary.host.clone());
    args.push(policy.host.clone());
    args.push(vocabulary.port.clone());
    args.push(policy.port.to_string());

    let env = ConstraintEnv {
        available_ram_bytes: hw.available_ram_bytes,
        total_ram_bytes: hw.total_ram_bytes,
        // The guard consumes the budget figure however sourced; the
        // Sysfs/Declared provenance stays here at the probe layer.
        vram_bytes_declared: hw.vram.as_ref().map(VramSource::bytes),
        gpu_offload: n_gpu_layers > 0,
        // v1 never plans `--mlock`: file-backed (mmap'd) weights are
        // evictable, so only the anonymous class is guarded — EXCEPT on a
        // no-mmap plan, where weights are read into anonymous RAM and
        // count as resident (manifest.rs semantics).
        model_mmap: !policy.no_mmap,
        expert_bytes: meta.expert_bytes,
        dense_bytes: meta.dense_bytes,
        pinned_cpu_bytes,
        kv_bytes_estimate,
        compute_bytes_estimate: policy.compute_buffer_bytes,
        context_length: policy.context_length,
        slot_count: None,
        n_expert_used: meta.expert_used_count(),
        draft_bytes: draft.as_ref().map_or(0, |d| d.spec.draft_bytes),
        draft_on_gpu: draft.as_ref().is_some_and(|d| d.on_gpu),
        verify_workspace_bytes: draft.as_ref().map_or(0, |d| d.spec.verify_workspace_bytes),
        planned_flags: args.clone(),
    };

    for constraint in &manifest.constraints {
        if let Evaluation::Deny(reason) = evaluate_constraint(constraint, &env)? {
            return Err(PlacementError::GuardDenied {
                constraint: constraint_name(constraint).to_string(),
                reason,
            });
        }
    }

    Ok(LaunchPlan {
        surface_id: manifest.surface_id.clone(),
        binary: vocabulary.binary.clone(),
        host: policy.host.clone(),
        port: policy.port,
        args,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_leg::manifest::decode;

    #[test]
    fn backend_probe_walks_the_engine_root_in_the_measured_order() {
        // The probe is root-driven (the canonical engine install), never a
        // literal home path: empty root → CPU-only; Vulkan alone → Vulkan;
        // both builds → HIP (the measured preference, baseline record §5).
        let root = tempfile::tempdir().expect("tempdir");
        assert_eq!(probe_backend_at(root.path()), GpuBackend::None);

        std::fs::create_dir_all(root.path().join("build-vulkan/bin")).expect("mkdir");
        std::fs::write(root.path().join("build-vulkan/bin/llama-server"), b"").expect("write");
        match probe_backend_at(root.path()) {
            GpuBackend::Vulkan { binary } => {
                assert_eq!(binary, root.path().join("build-vulkan/bin/llama-server"));
            }
            other => panic!("Vulkan must win alone: {other:?}"),
        }

        std::fs::create_dir_all(root.path().join("build-hip/bin")).expect("mkdir");
        std::fs::write(root.path().join("build-hip/bin/llama-server"), b"").expect("write");
        match probe_backend_at(root.path()) {
            GpuBackend::Hip { binary } => {
                assert_eq!(binary, root.path().join("build-hip/bin/llama-server"));
            }
            other => panic!("HIP must win when both builds exist: {other:?}"),
        }
    }

    #[test]
    fn engine_root_resolves_the_canonical_artifacts_root() {
        // The shape pin: the engine tree lives under the private-inference
        // subtree of the ARTIFACTS root — the resolution chain
        // (HKASK_ARTIFACTS_DIR → XDG documents → platform default) is
        // `agent_paths`' own, tested there; this pins that the
        // private-inference call site names the canonical subpath, never a
        // `~` literal (the retired probe bug).
        let resolved = engine_root();
        assert!(resolved.ends_with("private-inference/engines/llama-cpp-mainline"));
        assert!(!resolved.to_string_lossy().contains('~'));
    }

    const MANIFEST_TOML: &str =
        include_str!("../../../../registry/private-inference/llama-cpp-mainline.toml");
    const BRIEF_TOML: &str =
        include_str!("../../../../registry/model-briefs/qwen3-8-flash-next.toml");

    fn test_brief() -> ModelBrief {
        ModelBrief::decode(BRIEF_TOML).expect("Qwen brief decodes")
    }

    #[test]
    fn quantized_kv_cache_with_fa_off_is_refused_at_emit_time() {
        let manifest = decode(MANIFEST_TOML).expect("manifest");
        let mut policy = PlacementPolicy::default();
        policy.flash_attn = false;
        // kv_cache_type defaults to q8_0 — quantized.
        let err = compile_placement(
            PathBuf::from("/models/test.gguf"),
            &GgufMeta::default(),
            &HardwareInventory {
                total_ram_bytes: 96_000_000_000,
                available_ram_bytes: 72_000_000_000,
                vram: None,
                provenance: "test",
            },
            &test_brief(),
            &policy,
            &manifest,
        )
        .expect_err("D1: quantized KV requires fa-on");
        assert!(
            matches!(err, PlacementError::QuantizedKvRequiresFlashAttn(ref kv) if kv == "q8_0"),
            "refusal must name the quantized kv_cache_type, got: {err}"
        );
    }

    #[test]
    fn f16_kv_cache_with_fa_off_is_allowed() {
        let manifest = decode(MANIFEST_TOML).expect("manifest");
        let mut policy = PlacementPolicy::default();
        policy.flash_attn = false;
        policy.kv_cache_type = "f16".into();
        policy.n_gpu_layers = 0; // CPU shape: D1 checks the fa/KV pair, not VRAM
        let plan = compile_placement(
            PathBuf::from("/models/test.gguf"),
            &GgufMeta::default(),
            &HardwareInventory {
                total_ram_bytes: 96_000_000_000,
                available_ram_bytes: 72_000_000_000,
                vram: None,
                provenance: "test",
            },
            &test_brief(),
            &policy,
            &manifest,
        )
        .expect("f16 KV with fa-off is a legal plan");
        assert!(plan.to_string().contains("--flash-attn off"));
    }

    fn test_meta() -> GgufMeta {
        let mut meta = GgufMeta::default();
        meta.expert_bytes = 72_000_000_000;
        meta.dense_bytes = 6_900_000_000;
        meta
    }

    fn test_hw() -> HardwareInventory {
        HardwareInventory {
            total_ram_bytes: 96_000_000_000,
            available_ram_bytes: 72_000_000_000,
            vram: None,
            provenance: "test fixture",
        }
    }

    #[test]
    fn probe_reads_meminfo() {
        let hw = probe_hardware().expect("live machine has /proc/meminfo");
        assert!(hw.total_ram_bytes > 0);
        assert!(hw.available_ram_bytes > 0);
        assert!(hw.available_ram_bytes <= hw.total_ram_bytes);
    }

    #[test]
    fn cpu_plan_passes_guards_and_renders_the_command_line() {
        let manifest = decode(MANIFEST_TOML).expect("manifest");
        // The CPU control is an explicit policy since the default became
        // the measured GPU tier (2026-11-04 matrix, baseline record §5).
        let mut policy = PlacementPolicy::default();
        policy.n_gpu_layers = 0;
        let plan = compile_placement(
            PathBuf::from("/models/test.gguf"),
            &test_meta(),
            &test_hw(),
            &test_brief(),
            &policy,
            &manifest,
        )
        .expect("healthy CPU plan compiles");
        let rendered = plan.to_string();
        assert!(rendered.starts_with("llama-server --model"));
        // No GPU tier on the control policy: no -ngl, no --cpu-moe.
        assert!(!rendered.contains("-ngl"));
        assert!(!rendered.contains("--cpu-moe"));
        assert!(rendered.contains("--ctx-size 65536"));
        // The split KV cache-type intent emits every spelling with the
        // policy value (v0.9.0-dev drift specimen, pinned).
        assert!(rendered.contains("--cache-type-k q8_0 --cache-type-v q8_0"));
        assert_eq!(plan.endpoint(), "http://127.0.0.1:8010/v1");
    }

    #[test]
    fn gpu_plan_emits_the_expert_split_flags() {
        let manifest = decode(MANIFEST_TOML).expect("manifest");
        let mut hw = test_hw();
        hw.vram = Some(VramSource::Declared {
            bytes: 8_589_934_592,
        });
        let mut policy = PlacementPolicy::default();
        policy.n_gpu_layers = 999;
        let plan = compile_placement(
            PathBuf::from("/models/test.gguf"),
            &test_meta(),
            &hw,
            &test_brief(),
            &policy,
            &manifest,
        )
        .expect("GPU plan with declared VRAM compiles");
        let rendered = plan.to_string();
        assert!(rendered.contains("-ngl 999"));
        assert!(rendered.contains("--cpu-moe"));
    }

    #[test]
    fn no_mmap_plan_emits_the_verified_load_mode_fragment() {
        let manifest = decode(MANIFEST_TOML).expect("manifest");
        let mut policy = PlacementPolicy::default();
        policy.n_gpu_layers = 0; // CPU shape: the fragment is tier-independent
        policy.no_mmap = true;
        let plan = compile_placement(
            PathBuf::from("/models/test.gguf"),
            // Weightless meta: this test pins the fragment's emission, not
            // the resident-RAM accounting (the guard test below owns that).
            &GgufMeta::default(),
            &test_hw(),
            &test_brief(),
            &policy,
            &manifest,
        )
        .expect("no-mmap plan compiles against the mainline vocabulary");
        let rendered = plan.to_string();
        // The verified mainline spelling (2026-11-07, build-hip --help):
        // the pin's load-mode vocabulary, `none` = no mmap/mlock/dio.
        assert!(rendered.contains("--load-mode none"));
    }

    #[test]
    fn no_mmap_counts_weights_resident_for_the_ram_guard() {
        let manifest = decode(MANIFEST_TOML).expect("manifest");
        // The same tight-RAM shape under both knobs: mmap'd weights are
        // evictable (the guard passes on the anonymous class alone);
        // no-mmap weights are resident RAM and bust the headroom — the
        // only difference between the two plans is the knob.
        let mut mmap_policy = PlacementPolicy::default();
        mmap_policy.n_gpu_layers = 0;
        compile_placement(
            PathBuf::from("/models/test.gguf"),
            &test_meta(),
            &test_hw(),
            &test_brief(),
            &mmap_policy,
            &manifest,
        )
        .expect("mmap plan: weights are evictable, anonymous fits");

        let mut resident_policy = mmap_policy;
        resident_policy.no_mmap = true;
        let err = compile_placement(
            PathBuf::from("/models/test.gguf"),
            &test_meta(),
            &test_hw(),
            &test_brief(),
            &resident_policy,
            &manifest,
        )
        .expect_err("no-mmap plan: weights count as resident and bust the headroom");
        assert!(
            matches!(err, PlacementError::GuardDenied { ref constraint, .. } if constraint == "available_ram_headroom"),
            "refusal must name the headroom guard, got: {err}"
        );
    }

    #[test]
    fn no_mmap_ask_against_a_vocabulary_less_surface_is_refused() {
        let mut manifest = decode(MANIFEST_TOML).expect("manifest");
        manifest.flags.no_mmap = None;
        let mut policy = PlacementPolicy::default();
        policy.no_mmap = true;
        let err = compile_placement(
            PathBuf::from("/models/test.gguf"),
            &test_meta(),
            &test_hw(),
            &test_brief(),
            &policy,
            &manifest,
        )
        .expect_err("a surface that spells no fragment must refuse the ask");
        assert!(
            matches!(err, PlacementError::NoMmapVocabularyMissing(ref s) if s == "llama-cpp-mainline-cpu-p0"),
            "refusal must name the surface, got: {err}"
        );
    }

    #[test]
    fn partial_gpu_offload_computes_the_verified_layer_split() {
        // The DME S-leg shape: a dense model bigger than the declared
        // VRAM. 4 blocks × 2 GiB + 1 GiB non-block dense = 9 GiB on an
        // 8 GiB budget. The full ask (ngl 999) refuses at vram_dense_fit
        // (9 GiB + 0.5 GiB KV > 8 GiB); the partial ask computes the
        // verified split — fixed share = 1 GiB non-block + 0.5 GiB KV +
        // 1 GiB compute (declared in the policy), budget 5.5 GiB → 2
        // blocks fit — and the guard then evaluates the OFFLOADED share
        // (5 GiB + 0.5 GiB ≤ 8 GiB) through the pinned-cpu channel.
        use crate::local_leg::gguf::{GgufTensor, GgufValue};
        const GIB: u64 = 1 << 30;
        let manifest = decode(MANIFEST_TOML).expect("manifest");
        let mut meta = GgufMeta::default();
        meta.dense_bytes = 9 * GIB;
        meta.kv
            .insert("qwen4test.block_count".into(), GgufValue::U32(4));
        for i in 0..4u64 {
            meta.tensors.push(GgufTensor {
                name: format!("blk.{i}.mlp.weight"),
                ggml_type: 0,
                dims: vec![1],
                offset: i,
                approx_bytes: 2 * GIB,
                is_expert: false,
            });
        }
        meta.tensors.push(GgufTensor {
            name: "token_embd.weight".into(),
            ggml_type: 0,
            dims: vec![1],
            offset: 4,
            approx_bytes: GIB,
            is_expert: false,
        });
        let mut hw = test_hw();
        hw.vram = Some(VramSource::Declared { bytes: 8 * GIB });
        let mut policy = PlacementPolicy::default();
        policy.n_gpu_layers = 999;
        policy.compute_buffer_bytes = GIB; // declared estimate for this test
        // Without the partial ask: the full ask refuses, naming the guard.
        let err = compile_placement(
            PathBuf::from("/models/test.gguf"),
            &meta,
            &hw,
            &test_brief(),
            &policy,
            &manifest,
        )
        .expect_err("the full 9 GiB offload on an 8 GiB budget refuses");
        assert!(
            matches!(err, PlacementError::GuardDenied { ref constraint, .. } if constraint == "vram_dense_fit"),
            "refusal must name the violated guard, got: {err}"
        );
        // With the partial ask: the verified split compiles and emits the
        // computed layer count — never a silent substitution.
        policy.partial_gpu_offload = true;
        let plan = compile_placement(
            PathBuf::from("/models/test.gguf"),
            &meta,
            &hw,
            &test_brief(),
            &policy,
            &manifest,
        )
        .expect("the verified partial split compiles");
        assert!(plan.to_string().contains("-ngl 2"), "got {}", plan);
    }

    #[test]
    fn flash_attn_off_renders_the_explicit_off_spelling() {
        let manifest = decode(MANIFEST_TOML).expect("manifest");
        let mut policy = PlacementPolicy::default();
        policy.n_gpu_layers = 0; // CPU control shape: the off spelling, not the GPU tier
        policy.flash_attn = false;
        policy.kv_cache_type = "f16".into(); // D1: quantized KV would be refused at fa-off
        let plan = compile_placement(
            PathBuf::from("/models/test.gguf"),
            &test_meta(),
            &test_hw(),
            &test_brief(),
            &policy,
            &manifest,
        )
        .expect("CPU plan with flash-attn off compiles");
        assert!(plan.to_string().contains("--flash-attn off"));
    }

    #[test]
    fn guard_denies_gpu_offload_without_a_vram_budget() {
        let manifest = decode(MANIFEST_TOML).expect("manifest");
        let mut policy = PlacementPolicy::default();
        policy.n_gpu_layers = 999;
        let err = compile_placement(
            PathBuf::from("/models/test.gguf"),
            &test_meta(),
            &test_hw(), // vram: None
            &test_brief(),
            &policy,
            &manifest,
        )
        .expect_err("undeclared VRAM must fail closed");
        assert!(
            matches!(err, PlacementError::GuardDenied { ref constraint, .. } if constraint == "vram_dense_fit"),
            "refusal must name the violated guard, got: {err}"
        );
    }

    #[test]
    fn guard_denies_oversized_anonymous_bytes() {
        let manifest = decode(MANIFEST_TOML).expect("manifest");
        let mut policy = PlacementPolicy::default();
        policy.n_gpu_layers = 0; // the anonymous-class guard on a CPU plan
        // 65_536 tokens × 1 MiB/token ≈ 64 GiB of KV — far beyond the
        // 72 GB × 0.85 budget: the H4 anonymous class trips.
        policy.kv_bytes_per_token = 1 << 20;
        let err = compile_placement(
            PathBuf::from("/models/test.gguf"),
            &test_meta(),
            &test_hw(),
            &test_brief(),
            &policy,
            &manifest,
        )
        .expect_err("oversized anonymous bytes must fail closed");
        assert!(
            matches!(err, PlacementError::GuardDenied { ref constraint, .. } if constraint == "available_ram_headroom"),
            "refusal must name the violated guard, got: {err}"
        );
    }

    #[test]
    fn gpu_plan_compiles_with_sysfs_detected_vram() {
        let manifest = decode(MANIFEST_TOML).expect("manifest");
        let mut hw = test_hw();
        hw.vram = Some(VramSource::Sysfs {
            card_path: PathBuf::from("/sys/class/drm/card1/device"),
            bytes: 8_573_157_376,
        });
        let mut policy = PlacementPolicy::default();
        policy.n_gpu_layers = 999;
        let plan = compile_placement(
            PathBuf::from("/models/test.gguf"),
            &test_meta(),
            &hw,
            &test_brief(),
            &policy,
            &manifest,
        )
        .expect("GPU plan with detected VRAM compiles");
        let rendered = plan.to_string();
        assert!(rendered.contains("-ngl 999"));
        assert!(rendered.contains("--cpu-moe"));
    }

    #[test]
    fn vram_probe_picks_the_largest_amdgpu_card() {
        let root = tempfile::tempdir().expect("tempdir");
        for (card, bytes) in [("card1", "8573157376"), ("card2", "536870912")] {
            let device = root.path().join(card).join("device");
            std::fs::create_dir_all(&device).expect("create card device dir");
            std::fs::write(device.join("mem_info_vram_total"), bytes)
                .expect("write mem_info_vram_total");
        }
        // Connectors and render nodes must not be mistaken for cards.
        std::fs::create_dir_all(root.path().join("card1-DP-1").join("device"))
            .expect("connector dir");
        std::fs::create_dir_all(root.path().join("renderD128").join("device")).expect("render dir");
        let vram = probe_vram_at(root.path())
            .expect("probe over the fixture")
            .expect("two amdgpu cards present");
        let VramSource::Sysfs { card_path, bytes } = &vram else {
            panic!("detection must carry sysfs provenance, got: {vram:?}");
        };
        assert_eq!(*bytes, 8_573_157_376);
        assert!(
            card_path.ends_with("card1/device"),
            "largest card wins: {card_path:?}"
        );
    }

    #[test]
    fn vram_probe_reports_absence_without_amdgpu_cards() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(root.path().join("card0").join("device"))
            .expect("non-amdgpu card dir");
        let probed = probe_vram_at(root.path()).expect("probe over the fixture");
        assert!(
            probed.is_none(),
            "no amdgpu VRAM node means no budget, got {probed:?}"
        );
    }

    #[test]
    fn vram_probe_fails_loud_on_malformed_vram_bytes() {
        let root = tempfile::tempdir().expect("tempdir");
        let device = root.path().join("card0").join("device");
        std::fs::create_dir_all(&device).expect("card dir");
        std::fs::write(device.join("mem_info_vram_total"), "8 GiB\n")
            .expect("malformed mem_info_vram_total");
        let err = probe_vram_at(root.path())
            .expect_err("malformed driver output must fail loud, not silently skip");
        assert!(
            matches!(err, PlacementError::Probe(_)),
            "malformed node must surface as a typed probe error, got: {err}"
        );
    }

    // The draft-vocabulary fixture: the serveurperso experiment surface
    // (the unsloth prebuilt's manifest was DELETED at the 2026-11-07
    // audit — it documented a NON-project surface; the serveurperso
    // surface declares the same verified draft spellings, and its
    // fixtures gate the constraint set).
    const DRAFT_VOCABULARY_TOML: &str = include_str!(
        "../../../../registry/private-inference/llama-cpp-serveurperso-moe-stream.toml"
    );
    const GEMMA_BRIEF_TOML: &str =
        include_str!("../../../../registry/model-briefs/gemma-4-31b.toml");

    fn test_gpu_hw(vram_bytes: u64) -> HardwareInventory {
        HardwareInventory {
            total_ram_bytes: 96_000_000_000,
            available_ram_bytes: 72_000_000_000,
            vram: Some(VramSource::Declared { bytes: vram_bytes }),
            provenance: "test fixture",
        }
    }

    fn draft_dir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let mtp = dir.path().join("MTP");
        std::fs::create_dir_all(&mtp).expect("draft dir");
        std::fs::write(
            mtp.join("mtp-Qwen3.8-Flash-Next-shared-Q8_0.gguf"),
            b"draft-module-bytes",
        )
        .expect("draft file");
        let model = dir.path().join("model.gguf");
        (dir, model)
    }

    fn draft_policy() -> PlacementPolicy {
        let mut policy = PlacementPolicy::default();
        policy.draft = true;
        policy
    }

    #[test]
    fn draft_plan_emits_spec_flags_and_pins_to_cpu_when_vram_is_tight() {
        // The live-machine shape: dense 6.9 GB + kv 0.5 GB + the verify
        // workspace 2 GiB (the MTP §7 item-1 term — GPU-side even with a
        // CPU-routed draft) leaves 9.547 resident against a 10 GB card,
        // while the routing's used set (6.9 + 0.5 + 4.29 compute + the
        // workspace = 13.837 GB) leaves no headroom for the 2.79 GB draft
        // — the estimate over-refuses (by design) and the draft rides CPU.
        // The pre-§7 card (8.57 GB) genuinely cannot host this draft ask
        // anymore — the workspace is the D-B lesson.
        let manifest = decode(DRAFT_VOCABULARY_TOML).expect("prebuilt manifest");
        let (_dir, model_path) = draft_dir();
        let plan = compile_placement(
            model_path,
            &test_meta(),
            &test_gpu_hw(10_000_000_000),
            &test_brief(),
            &draft_policy(),
            &manifest,
        )
        .expect("tight-VRAM draft plan compiles with a CPU-pinned draft");
        let rendered = plan.to_string();
        assert!(rendered.contains("--spec-draft-model"));
        assert!(rendered.contains("MTP/mtp-Qwen3.8-Flash-Next-shared-Q8_0.gguf"));
        assert!(rendered.contains("--spec-type draft-mtp"));
        assert!(rendered.contains("--spec-draft-n-max 2"));
        assert!(rendered.contains("--spec-draft-ngl 0"));
    }

    #[test]
    fn draft_rides_the_gpu_when_vram_headroom_allows() {
        let manifest = decode(DRAFT_VOCABULARY_TOML).expect("prebuilt manifest");
        let (_dir, model_path) = draft_dir();
        let plan = compile_placement(
            model_path,
            &test_meta(),
            &test_gpu_hw(20_000_000_000),
            &test_brief(),
            &draft_policy(),
            &manifest,
        )
        .expect("roomy-VRAM draft plan compiles with a GPU draft");
        let rendered = plan.to_string();
        assert!(rendered.contains("--spec-draft-ngl 999"));
        // The GPU-placed draft's bytes enter the VRAM channel: the guard
        // re-derives the same sum (offloadable + kv + draft ≤ vram).
        assert!(rendered.contains("--spec-draft-model"));
    }

    #[test]
    fn missing_draft_module_is_a_typed_refusal() {
        let manifest = decode(DRAFT_VOCABULARY_TOML).expect("prebuilt manifest");
        let dir = tempfile::tempdir().expect("tempdir");
        let model_path = dir.path().join("model.gguf");
        let err = compile_placement(
            model_path,
            &test_meta(),
            &test_gpu_hw(8_573_157_376),
            &test_brief(),
            &draft_policy(),
            &manifest,
        )
        .expect_err("a draft plan without the module on disk must fail closed");
        assert!(
            matches!(err, PlacementError::DraftModuleMissing { .. }),
            "refusal must be the typed missing-module error, got: {err}"
        );
    }

    #[test]
    fn draft_on_a_surface_without_vocabulary_is_refused() {
        // The mainline surface spells no draft vocabulary — its pinned
        // binary is not on disk to verify against, so the honest state
        // is absence, and a draft ask is a typed refusal.
        let manifest = decode(MANIFEST_TOML).expect("mainline manifest");
        let (_dir, model_path) = draft_dir();
        let err = compile_placement(
            model_path,
            &test_meta(),
            &test_gpu_hw(8_573_157_376),
            &test_brief(),
            &draft_policy(),
            &manifest,
        )
        .expect_err("the mainline surface must refuse the draft ask");
        assert!(
            matches!(err, PlacementError::DraftVocabularyMissing(ref s) if s == "llama-cpp-mainline-cpu-p0"),
            "refusal must name the vocabulary-less surface, got: {err}"
        );
    }

    #[test]
    fn streaming_mode_stays_unroutable_in_v1() {
        // The P2 boundary (compile_placement's doc + the P2 streaming-parity
        // plan §7): the compiler has no streaming intent in v1 — the slot
        // fields stay `None` and the rendered command never carries a
        // moe-stream spelling. Streaming earns routability only through
        // the measured parity band (the operator's routing decision reads
        // the band); never by default, never by accident. The pinned
        // experiment surface can decode and fixture-gate as an EXPERIMENT
        // manifest while the production surface stays the mainline pin.
        let manifest = decode(MANIFEST_TOML).expect("manifest");
        let mut hw = test_hw();
        hw.vram = Some(VramSource::Declared {
            bytes: 8_589_934_592,
        });
        let plan = compile_placement(
            PathBuf::from("/models/test.gguf"),
            &test_meta(),
            &hw,
            &test_brief(),
            &PlacementPolicy::default(),
            &manifest,
        )
        .expect("the GPU plan compiles on the production surface");
        let rendered = plan.to_string();
        assert!(
            !rendered.contains("moe-stream"),
            "v1 routing never emits a streaming spelling — the P2 experiment engine sits behind the band gate, got: {rendered}"
        );
        assert_eq!(
            plan.surface_id, "llama-cpp-mainline-cpu-p0",
            "the production surface is the mainline pin, never an experiment surface"
        );
    }

    #[test]
    fn draft_asked_without_a_brief_spec_is_refused() {
        // The Gemma brief has no [draft] table — the compiler never
        // invents a draft for a model that ships none.
        let manifest = decode(DRAFT_VOCABULARY_TOML).expect("prebuilt manifest");
        let brief = ModelBrief::decode(GEMMA_BRIEF_TOML).expect("Gemma brief decodes");
        let (_dir, model_path) = draft_dir();
        let err = compile_placement(
            model_path,
            &test_meta(),
            &test_gpu_hw(8_573_157_376),
            &brief,
            &draft_policy(),
            &manifest,
        )
        .expect_err("a draft ask against a brief without [draft] must fail closed");
        assert!(
            matches!(err, PlacementError::DraftBriefMissing(ref id) if id == "gemma-4-31b"),
            "refusal must name the brief without a draft spec, got: {err}"
        );
    }

    const ZERO_BYTE_DRAFT_BRIEF_TOML: &str = r#"
model_id = "qwen3-8-flash-next"
display_name = "Qwen3.8 Flash Next"

[model]
total_params = 125_000_000_000
active_params = 6_000_000_000
expert_count = 512
expert_used_count = 10

[placement]
ple_pin_pattern = "per_layer_token_embd"

[recommendations]
kv_cache_type = "q8_0"
flash_attn = true
context_length = 65_536

[draft]
module_file = "MTP/mtp-Qwen3.8-Flash-Next-shared-Q8_0.gguf"
spec_type = "draft-mtp"
draft_n_max = 2
draft_bytes = 0
"#;

    #[test]
    fn zero_byte_draft_never_rides_a_saturated_headroom() {
        // The degenerate corner the exact routing predicate closes: a
        // zero-byte draft with an over-budget used set (offloadable 6.9 +
        // kv 0.5 + compute 4.29 GiB > the 7.5 GB card) — the headroom is
        // saturated, so the draft routes to CPU even though draft_bytes
        // is zero; the base plan still fits and emits with -ngld 0.
        let manifest = decode(DRAFT_VOCABULARY_TOML).expect("prebuilt manifest");
        let brief =
            ModelBrief::decode(ZERO_BYTE_DRAFT_BRIEF_TOML).expect("zero-byte-draft brief decodes");
        let (_dir, model_path) = draft_dir();
        let plan = compile_placement(
            model_path,
            &test_meta(),
            &test_gpu_hw(7_500_000_000),
            &brief,
            &draft_policy(),
            &manifest,
        )
        .expect("the base plan fits; the draft rides CPU");
        let rendered = plan.to_string();
        assert!(rendered.contains("--spec-draft-ngl 0"));
        assert!(!rendered.contains("--spec-draft-ngl 999"));
    }

    const WORKSPACE_DRAFT_BRIEF_TOML: &str = r#"
model_id = "qwen3-8-flash-next"
display_name = "Qwen3.8 Flash Next"

[model]
total_params = 125_000_000_000
active_params = 6_000_000_000
expert_count = 512
expert_used_count = 10

[placement]
ple_pin_pattern = "per_layer_token_embd"

[recommendations]
kv_cache_type = "q8_0"
flash_attn = true
context_length = 65_536

[draft]
module_file = "MTP/mtp-Qwen3.8-Flash-Next-shared-Q8_0.gguf"
spec_type = "draft-mtp"
draft_n_max = 2
draft_bytes = 2_000_000_000
verify_workspace_bytes = 2_147_483_648
"#;

    const WORKSPACE_FREE_DRAFT_BRIEF_TOML: &str = r#"
model_id = "qwen3-8-flash-next"
display_name = "Qwen3.8 Flash Next"

[model]
total_params = 125_000_000_000
active_params = 6_000_000_000
expert_count = 512
expert_used_count = 10

[placement]
ple_pin_pattern = "per_layer_token_embd"

[recommendations]
kv_cache_type = "q8_0"
flash_attn = true
context_length = 65_536

[draft]
module_file = "MTP/mtp-Qwen3.8-Flash-Next-shared-Q8_0.gguf"
spec_type = "draft-mtp"
draft_n_max = 2
draft_bytes = 2_000_000_000
"#;

    #[test]
    fn the_verify_workspace_consumes_the_headroom_before_the_draft() {
        // MTP record §7 item 1 (the D-B finding): the verify workspace
        // enters the used set BEFORE the headroom test — a card whose
        // headroom would fit the draft without it routes the draft to CPU
        // instead of under-reserving the GPU verify path. test_meta() pins
        // offloadable 6.9G + kv 0.5G + compute 4.29G = 11.69G used; the 15G
        // card leaves 3.31G headroom — the 2.0G draft fits WITHOUT the
        // workspace, and does NOT with the 2.147G one (13.837G used →
        // 1.163G headroom).
        let manifest = decode(DRAFT_VOCABULARY_TOML).expect("prebuilt manifest");
        let (_dir, model_path) = draft_dir();
        let brief =
            ModelBrief::decode(WORKSPACE_DRAFT_BRIEF_TOML).expect("workspace brief decodes");
        let plan = compile_placement(
            model_path.clone(),
            &test_meta(),
            &test_gpu_hw(15_000_000_000),
            &brief,
            &draft_policy(),
            &manifest,
        )
        .expect("the base plan fits");
        assert!(
            plan.to_string().contains("--spec-draft-ngl 0"),
            "the workspace consumed the headroom — the draft routes CPU"
        );

        // The positive control: the same card, the same draft, NO workspace
        // reserve — the draft rides the GPU (the pre-§7 arithmetic), so
        // the routing change is the workspace's alone.
        let brief = ModelBrief::decode(WORKSPACE_FREE_DRAFT_BRIEF_TOML)
            .expect("workspace-free brief decodes");
        let plan = compile_placement(
            model_path,
            &test_meta(),
            &test_gpu_hw(15_000_000_000),
            &brief,
            &draft_policy(),
            &manifest,
        )
        .expect("the plan fits");
        assert!(
            plan.to_string().contains("--spec-draft-ngl 999"),
            "without the workspace reserve the draft rides the GPU"
        );
    }
}
