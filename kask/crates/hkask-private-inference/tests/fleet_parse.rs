//! Live fleet-parse verification (env-gated `HKASK_P1_LIVE`): every
//! shipped fleet model's brief `[model]` table cross-checked against the
//! real GGUF headers. The brief is declared data (sourced numbers); this
//! is the standing instrument that keeps it honest — a brief that
//! diverges from the weights it describes fails here, before it can
//! misplace a launch.
//!
//! ```sh
//! HKASK_P1_LIVE=1 cargo test -p hkask-private-inference \
//!   --test fleet_parse -- --ignored --nocapture
//! ```

use hkask_private_inference::local_leg::fleet::{self};
use hkask_private_inference::local_leg::gguf;

#[test]
#[ignore]
fn every_fleet_brief_matches_its_real_gguf_parse() {
    if std::env::var("HKASK_P1_LIVE")
        .map(|v| v != "1")
        .unwrap_or(true)
    {
        eprintln!("HKASK_P1_LIVE unset; skipping the live fleet parse");
        return;
    }
    let root = hkask_types::agent_paths::resolve_under_artifacts_dir(std::path::Path::new(
        "private-inference/models",
    ));
    for model in fleet::shipped_models().expect("the fleet decodes") {
        let brief = &model.brief;
        let files = fleet::resolve_model_files(&root, &brief.model_id)
            .unwrap_or_else(|err| panic!("{} resolves: {err}", brief.model_id));
        let meta =
            gguf::parse(&files).unwrap_or_else(|err| panic!("{} parses: {err}", brief.model_id));
        println!(
            "[fleet-parse] {} ({} files): expert_count={:?} expert_used_count={:?} \
             block_count={:?} native_context={:?} expert_bytes={:.2} GiB \
             dense_bytes={:.2} GiB",
            brief.model_id,
            files.len(),
            meta.expert_count(),
            meta.expert_used_count(),
            meta.block_count(),
            meta.context_length(),
            meta.expert_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
            meta.dense_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
        );
        // The brief's declared expert metadata must equal the weights'.
        // Dense models parse `None` for both expert fields and declare 0
        // — the mapping is (None ↔ 0); MoE models must match exactly.
        if brief.model.expert_count > 0 {
            assert_eq!(
                meta.expert_count(),
                Some(brief.model.expert_count),
                "{}: the brief's expert_count must match the GGUF",
                brief.model_id
            );
            assert_eq!(
                meta.expert_used_count(),
                Some(brief.model.expert_used_count),
                "{}: the brief's expert_used_count must match the GGUF",
                brief.model_id
            );
            assert!(meta.expert_bytes > 0 && meta.dense_bytes > 0);
        } else {
            assert_eq!(
                meta.expert_count(),
                None,
                "{}: a dense brief must match a dense parse",
                brief.model_id
            );
            assert_eq!(meta.expert_used_count(), None);
            assert_eq!(
                meta.expert_bytes, 0,
                "{}: a dense parse carries no expert bytes",
                brief.model_id
            );
            assert!(meta.dense_bytes > 0);
        }
    }
}
