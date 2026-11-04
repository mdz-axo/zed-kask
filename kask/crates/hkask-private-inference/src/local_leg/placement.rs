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

#[derive(Debug, thiserror::Error)]
pub enum PlacementError {
    #[error("hardware probe failed: {0}")]
    Probe(String),
    #[error("guard `{constraint}` refused the launch: {reason}")]
    GuardDenied { constraint: String, reason: String },
    #[error("constraint evaluation failed: {0}")]
    Constraint(#[from] crate::local_leg::manifest::ManifestError),
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
pub fn probe_vram() -> Result<Option<VramSource>, PlacementError> {
    probe_vram_at(Path::new(SYSFS_DRM_ROOT))
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
    /// Tensor-name substring pinned to CPU while layers offload to the GPU
    /// (the PLE n-gram table: `per_layer_token_embd`). Emitted as
    /// `--override-tensor <pattern>=CPU`; the pinned bytes are excluded
    /// from the VRAM budget the `vram_dense_fit` guard checks.
    pub pin_to_cpu: Option<String>,
    pub n_gpu_layers: u32,
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
            pin_to_cpu: None,
            // v1 P0 runs the CPU-only build; the GPU tier lands with
            // Vulkan/HIP (plan §11).
            n_gpu_layers: 0,
            threads: 12,
            host: "127.0.0.1".into(),
            port: 8010,
        }
    }
}

/// Compile a launch plan. Every manifest constraint is evaluated against
/// the plan's own env. Streaming-mode launches are unroutable in v1 — the
/// slot fields stay `None` (plan §6, the variety-deficit attenuation).
pub fn compile_placement(
    model_path: PathBuf,
    meta: &GgufMeta,
    hw: &HardwareInventory,
    policy: &PlacementPolicy,
    manifest: &EngineSurfaceManifest,
) -> Result<LaunchPlan, PlacementError> {
    let kv_bytes_estimate = policy
        .kv_bytes_per_token
        .saturating_mul(u64::from(policy.context_length));

    // Dense bytes the pin keeps on CPU — excluded from the VRAM budget.
    let pinned_cpu_bytes = policy
        .pin_to_cpu
        .as_deref()
        .map(|pattern| {
            meta.tensors
                .iter()
                .filter(|t| !t.is_expert && t.name.contains(pattern))
                .map(|t| t.approx_bytes)
                .fold(0u64, |acc, bytes| acc.saturating_add(bytes))
        })
        .unwrap_or(0);

    let vocabulary = &manifest.flags;
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
    if policy.n_gpu_layers > 0 {
        args.push(vocabulary.n_gpu_layers.clone());
        args.push(policy.n_gpu_layers.to_string());
        if policy.cpu_moe {
            args.push(vocabulary.cpu_moe.clone());
        }
        if let Some(pattern) = &policy.pin_to_cpu {
            args.push(vocabulary.override_tensor.clone());
            args.push(format!("{pattern}=CPU"));
        }
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
        gpu_offload: policy.n_gpu_layers > 0,
        // v1 never plans `--mlock`: file-backed weights are evictable, so
        // only the anonymous class is guarded (manifest.rs semantics).
        model_mmap: true,
        expert_bytes: meta.expert_bytes,
        dense_bytes: meta.dense_bytes,
        pinned_cpu_bytes,
        kv_bytes_estimate,
        compute_bytes_estimate: policy.compute_buffer_bytes,
        context_length: policy.context_length,
        slot_count: None,
        n_expert_used: meta.expert_used_count(),
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

    const MANIFEST_TOML: &str =
        include_str!("../../../../registry/private-inference/llama-cpp-mainline.toml");

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
        let plan = compile_placement(
            PathBuf::from("/models/test.gguf"),
            &test_meta(),
            &test_hw(),
            &PlacementPolicy::default(),
            &manifest,
        )
        .expect("healthy CPU plan compiles");
        let rendered = plan.to_string();
        assert!(rendered.starts_with("llama-server --model"));
        // No GPU tier on the P0 policy: no -ngl, no --cpu-moe.
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
            &policy,
            &manifest,
        )
        .expect("GPU plan with declared VRAM compiles");
        let rendered = plan.to_string();
        assert!(rendered.contains("-ngl 999"));
        assert!(rendered.contains("--cpu-moe"));
    }

    #[test]
    fn flash_attn_off_renders_the_explicit_off_spelling() {
        let manifest = decode(MANIFEST_TOML).expect("manifest");
        let mut policy = PlacementPolicy::default();
        policy.flash_attn = false;
        let plan = compile_placement(
            PathBuf::from("/models/test.gguf"),
            &test_meta(),
            &test_hw(),
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
        // 65_536 tokens × 1 MiB/token ≈ 64 GiB of KV — far beyond the
        // 72 GB × 0.85 budget: the H4 anonymous class trips.
        policy.kv_bytes_per_token = 1 << 20;
        let err = compile_placement(
            PathBuf::from("/models/test.gguf"),
            &test_meta(),
            &test_hw(),
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
}
