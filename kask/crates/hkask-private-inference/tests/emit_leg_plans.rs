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

use hkask_private_inference::local_leg::gguf;
use hkask_private_inference::local_leg::manifest::decode;
use hkask_private_inference::local_leg::placement::{
    PlacementPolicy, compile_placement, probe_hardware,
};

#[path = "common/fleet_brief.rs"]
mod fleet_brief;

#[path = "common/shards.rs"]
mod shards;

const MANIFEST_TOML: &str =
    include_str!("../../../registry/private-inference/llama-cpp-mainline.toml");

#[test]
#[ignore]
fn emits_the_p0_matrix_leg_plans_from_detected_hardware() {
    let Some(paths) = shards::p0_shard_paths() else {
        eprintln!("HKASK_P0_GGUF_DIR unset; skipping plan emission");
        return;
    };
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
    // The discovered fleet is the brief home (seed + discover — the
    // production path), never a CWD-relative file read.
    let brief = fleet_brief::discover_brief("qwen3-8-flash-next");
    for (leg, flash_attn, n_gpu_layers) in [
        ("a-cpu-control [build/bin]", true, 0u32),
        ("b-dgpu-vulkan-fa-on [build-vulkan/bin]", true, 999),
        ("c-dgpu-vulkan-fa-off [build-vulkan/bin]", false, 999),
        ("d-dgpu-hip-fa-on [build-hip/bin]", true, 999),
    ] {
        let mut policy = PlacementPolicy::default();
        policy.flash_attn = flash_attn;
        policy.n_gpu_layers = n_gpu_layers;
        match compile_placement(model.clone(), &meta, &hw, &brief, &policy, &manifest) {
            Ok(plan) => println!("[{leg}] {plan}"),
            Err(err) => println!("[{leg}] REFUSED: {err}"),
        }
    }

    // The draft-vocabulary emission legs: the serveurperso experiment
    // surface (the unsloth prebuilt's manifest was DELETED at the
    // 2026-11-07 audit — a NON-project surface; the serveurperso
    // surface declares the same verified draft spellings). Both legs
    // print from the one surface, differing only by the draft flags
    // (the isolation design). The draft module is the brief's MTP/
    // sibling of the shard dir; a missing module is a printed refusal.
    const DRAFT_VOCABULARY_TOML: &str =
        include_str!("../../../registry/private-inference/llama-cpp-serveurperso-moe-stream.toml");
    let prebuilt = decode(DRAFT_VOCABULARY_TOML).expect("experiment manifest decodes");
    let baseline_policy = PlacementPolicy::default();
    let mut mtp_policy = PlacementPolicy::default();
    mtp_policy.draft = true;
    for (leg, policy) in [
        (
            "e-experiment-baseline [serveurperso moe-stream surface]",
            &baseline_policy,
        ),
        (
            "f-experiment-mtp [serveurperso moe-stream surface]",
            &mtp_policy,
        ),
    ] {
        match compile_placement(model.clone(), &meta, &hw, &brief, policy, &prebuilt) {
            Ok(plan) => println!("[{leg}] {plan}"),
            Err(err) => println!("[{leg}] REFUSED: {err}"),
        }
    }
}
