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

use hkask_private_inference::local_leg::fleet;
use hkask_private_inference::local_leg::gguf;
use hkask_private_inference::local_leg::manifest::decode;
use hkask_private_inference::local_leg::placement::{
    PlacementPolicy, compile_placement, probe_hardware,
};

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
    // The shipped fleet is the brief home (compile-time embed, the
    // manifest-shipping precedent) — never a CWD-relative file read.
    let brief = fleet::shipped_model("qwen3-8-flash-next")
        .expect("the shipped fleet carries the Qwen brief")
        .brief;
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

    // The MTP paired experiment legs (unsloth transfer plan, T4): both on
    // the unsloth prebuilt engine (release b11160-mix-a6922cc, commit
    // a3c12db9 — the binary at ~/.unsloth/llama.cpp/build/bin), differing
    // only by the draft flags (the isolation design). The draft module is
    // the brief's MTP/ sibling of the shard dir (downloaded 2026-11-04,
    // sha256 5ff54097406a905cf3a724c709124ceb0e3e10235ee862298969e91c96fa96e6
    // — matches the HF LFS oid); a missing module is a printed refusal.
    const UNSLOTH_PREBUILT_TOML: &str =
        include_str!("../../../registry/private-inference/llama-cpp-unsloth-prebuilt.toml");
    let prebuilt = decode(UNSLOTH_PREBUILT_TOML).expect("prebuilt manifest decodes");
    let baseline_policy = PlacementPolicy::default();
    let mut mtp_policy = PlacementPolicy::default();
    mtp_policy.draft = true;
    for (leg, policy) in [
        (
            "e-prebuilt-baseline [~/.unsloth/llama.cpp/build/bin]",
            &baseline_policy,
        ),
        (
            "f-prebuilt-mtp [~/.unsloth/llama.cpp/build/bin]",
            &mtp_policy,
        ),
    ] {
        match compile_placement(model.clone(), &meta, &hw, &brief, policy, &prebuilt) {
            Ok(plan) => println!("[{leg}] {plan}"),
            Err(err) => println!("[{leg}] REFUSED: {err}"),
        }
    }
}
