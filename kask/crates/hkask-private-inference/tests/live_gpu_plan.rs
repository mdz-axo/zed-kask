//! Opt-in live compile of the canonical GPU LaunchPlan from a LIVE hardware
//! probe — the acceptance evidence (work order 2026-11-04, phase 4) that a
//! real launch goes through a compiler-emitted plan whose VRAM budget is
//! DETECTED (sysfs), not declared. Absent `HKASK_P0_LIVE_PLAN` the test
//! skips; with it, `HKASK_P0_GGUF_DIR` must name the complete shard set —
//! the guard arithmetic consumes the real tensor accounting, and the
//! strict GGUF parse refuses a truncated set.
//!
//! ```sh
//! HKASK_P0_LIVE_PLAN=1 \
//! HKASK_P0_GGUF_DIR=/home/mdz-axolotl/models/qwen38-flash-next/UD-Q2_K_XL \
//!   cargo test -p hkask-private-inference --test live_gpu_plan -- --ignored --nocapture
//! ```

use hkask_private_inference::local_leg::fleet;
use hkask_private_inference::local_leg::gguf;
use hkask_private_inference::local_leg::placement::{
    PlacementPolicy, VramSource, compile_placement, probe_hardware,
};

#[path = "common/shards.rs"]
mod shards;

const MANIFEST_TOML: &str =
    include_str!("../../../registry/private-inference/llama-cpp-mainline.toml");

#[test]
#[ignore]
fn emits_the_gpu_launch_plan_from_detected_vram() {
    if std::env::var_os("HKASK_P0_LIVE_PLAN").is_none() {
        eprintln!("HKASK_P0_LIVE_PLAN unset; skipping the live GPU plan compile");
        return;
    }
    let Some(paths) = shards::p0_shard_paths() else {
        panic!(
            "HKASK_P0_LIVE_PLAN set but HKASK_P0_GGUF_DIR unset: \
                the plan compile needs the real tensor accounting"
        );
    };

    let hw = probe_hardware().expect("live hardware probe");
    let vram = hw
        .vram
        .as_ref()
        .expect("this machine exposes an amdgpu dGPU");
    let VramSource::Sysfs { card_path, bytes } = vram else {
        panic!("live VRAM must be sysfs-detected for this evidence, got: {vram:?}");
    };
    println!(
        "detected VRAM budget: {bytes} bytes at {} (available RAM {} bytes)",
        card_path.display(),
        hw.available_ram_bytes
    );

    let meta = gguf::parse(&paths).expect("complete shard set parses (strict guard)");
    println!(
        "real meta: expert_bytes={} dense_bytes={} n_expert_used={:?}",
        meta.expert_bytes,
        meta.dense_bytes,
        meta.expert_used_count()
    );

    let manifest = hkask_private_inference::local_leg::manifest::decode(MANIFEST_TOML)
        .expect("pinned manifest decodes");
    let mut policy = PlacementPolicy::default();
    // The measured GPU placement (work order phase 4): dense/attention/KV on
    // the dGPU, routed experts resident in RAM. `cpu_moe` is already true.
    policy.n_gpu_layers = 999;
    // The model brief carries the PLE pin; the policy carries machine-level
    // knobs only. The shipped fleet is the brief home — never a
    // CWD-relative file read.
    let brief = fleet::shipped_model("qwen3-8-flash-next")
        .expect("the shipped fleet carries the Qwen brief")
        .brief;

    // Shard 1 is the metadata-only header shard; llama.cpp discovers the
    // split set from the `-00001-of-00004` naming convention.
    let plan = compile_placement(paths[0].clone(), &meta, &hw, &brief, &policy, &manifest)
        .expect("the real meta + detected VRAM must pass the guards");
    let rendered = plan.to_string();
    println!("LAUNCH_PLAN: {rendered}");
    assert!(
        rendered.contains("-ngl 999"),
        "GPU tier emitted: {rendered}"
    );
    assert!(
        rendered.contains("--cpu-moe"),
        "expert split emitted: {rendered}"
    );
}
