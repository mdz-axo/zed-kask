//! Emits the P0 matrix leg launch plans from the REAL weights and the
//! LIVE hardware probe — the acceptance path: full shard-set metadata +
//! sysfs-detected VRAM (never declared) → compile_placement → rendered
//! command lines. Strict by construction: the gguf parse refuses a
//! truncated shard set, so plans only emit on complete weights. P0
//! launches by hand from the printed plans (plan §11) — the leg's
//! build-dir binary is prefixed at launch time (the plan's `binary`
//! stays the manifest's `llama-server`).
//!
//! ```sh
//! HKASK_P0_GGUF_DIR=~/models/qwen38-flash-next/UD-Q2_K_XL \
//!   cargo test -p hkask-private-inference --test emit_leg_plans -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use hkask_private_inference::local_leg::gguf;
use hkask_private_inference::local_leg::manifest::decode;
use hkask_private_inference::local_leg::placement::{
    PlacementPolicy, compile_placement, probe_hardware,
};

const MANIFEST_TOML: &str =
    include_str!("../../../registry/private-inference/llama-cpp-mainline.toml");

#[test]
#[ignore]
fn emits_the_p0_matrix_leg_plans_from_detected_hardware() {
    let Some(dir) = std::env::var_os("HKASK_P0_GGUF_DIR") else {
        eprintln!("HKASK_P0_GGUF_DIR unset; skipping plan emission");
        return;
    };
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("read HKASK_P0_GGUF_DIR")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "gguf"))
        .collect();
    assert!(!paths.is_empty(), "no .gguf shards in {dir:?}");
    paths.sort();
    let meta = gguf::parse(&paths).expect("complete shard set parses — strict byte guard");
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    println!(
        "meta: expert_bytes={:.2} GiB dense_bytes={:.2} GiB ({} tensors)",
        meta.expert_bytes as f64 / GIB,
        meta.dense_bytes as f64 / GIB,
        meta.tensors.len(),
    );

    let manifest = decode(MANIFEST_TOML).expect("manifest decodes");
    let hw = probe_hardware().expect("live hardware probe");
    println!(
        "hardware: available RAM {:.1} GiB; VRAM source {:?}",
        hw.available_ram_bytes as f64 / GIB,
        hw.vram,
    );

    // Legs a/b/c/d of the matrix (§4 of the baseline record). A guard
    // refusal is a printed finding for the record, not a test failure —
    // the matrix adapts with a recorded deviation, never silently.
    let model = paths[0].clone();
    // Model-aware PLE pin: set only when the parsed model actually carries
    // per-layer token embedding tensors. An unmatched pattern would emit a
    // dead `--override-tensor` on models without them (observed live on the
    // Flint emission 2026-11-04 — its dense class is 1.86 GiB with no PLE
    // tensors). When the model carries the class, pinning keeps the
    // offloadable dense + KV inside the 8 GB dGPU budget — the un-pinned
    // plan is correctly refused by `vram_dense_fit` (observed live).
    let ple_pin: Option<String> = if meta
        .tensors
        .iter()
        .any(|t| !t.is_expert && t.name.contains("per_layer_token_embd"))
    {
        Some("per_layer_token_embd".into())
    } else {
        None
    };
    for (leg, flash_attn, n_gpu_layers) in [
        ("a-cpu-control [build/bin]", true, 0u32),
        ("b-dgpu-vulkan-fa-on [build-vulkan/bin]", true, 999),
        ("c-dgpu-vulkan-fa-off [build-vulkan/bin]", false, 999),
        ("d-dgpu-hip-fa-on [build-hip/bin]", true, 999),
    ] {
        let mut policy = PlacementPolicy::default();
        policy.flash_attn = flash_attn;
        policy.n_gpu_layers = n_gpu_layers;
        if n_gpu_layers > 0 {
            policy.pin_to_cpu = ple_pin.clone();
        }
        match compile_placement(model.clone(), &meta, &hw, &policy, &manifest) {
            Ok(plan) => println!("[{leg}] {plan}"),
            Err(err) => println!("[{leg}] REFUSED: {err}"),
        }
    }
}
