//! Opt-in live check against the real P0 weights (downloaded outside the
//! tree — never committed). Absent `HKASK_P0_GGUF_DIR` the test skips: no
//! fabricated evidence. With the complete shard set the byte accounting
//! covers the full model; a truncated in-flight shard is refused by the
//! strict guard and the test falls back to the header shard's metadata
//! (disclosed on stderr), rerunning fully when every shard has landed.
//!
//! ```sh
//! HKASK_P0_GGUF_DIR=~/models/qwen38-flash-next/UD-Q2_K_XL \
//!   cargo test -p hkask-private-inference --test real_gguf -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use hkask_private_inference::local_leg::gguf::{self, GgufError};

#[path = "common/shards.rs"]
mod shards;

#[test]
#[ignore]
fn parses_real_qwen38_flash_next_shard_headers() {
    let Some(paths) = shards::p0_shard_paths() else {
        eprintln!("HKASK_P0_GGUF_DIR unset; skipping the live shard parse");
        return;
    };

    let full = gguf::parse(&paths);
    let meta = match full {
        Ok(meta) => meta,
        Err(GgufError::Tensor { reason, .. }) if reason.contains("beyond data section") => {
            eprintln!("incomplete shard set ({reason}); falling back to the header shard");
            let header_only: Vec<PathBuf> = paths[..1].to_vec();
            gguf::parse(&header_only).expect("header shard parses")
        }
        Err(other) => panic!("real shards parse: {other}"),
    };
    println!(
        "gguf v{}: expert_count={:?} expert_used={:?} block_count={:?} context_length={:?}",
        meta.gguf_version,
        meta.expert_count(),
        meta.expert_used_count(),
        meta.block_count(),
        meta.context_length(),
    );
    println!(
        "shards={} tensors={} expert_bytes={:.2} GiB dense_bytes={:.2} GiB",
        paths.len(),
        meta.tensors.len(),
        meta.expert_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
        meta.dense_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
    );

    assert!(meta.expert_count().expect("expert_count present") > 0);
    assert!(meta.expert_used_count().expect("expert_used_count present") > 0);
    // With a full shard set the tensor accounting must hold; a
    // metadata-only header shard carries no tensors to classify.
    if !meta.tensors.is_empty() {
        assert!(meta.tensors.iter().any(|t| t.is_expert));
    }
}
