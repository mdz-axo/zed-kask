//! Opt-in live check of the acquisition module against the real
//! HuggingFace hub (anonymous; the unsloth repo is public). Absent
//! `HKASK_LIVE_HF` the test skips: no fabricated evidence.
//!
//! ```sh
//! HKASK_LIVE_HF=1 \
//!   cargo test -p hkask-private-inference --test live_acquisition -- --ignored --nocapture
//! ```

use hkask_private_inference::acquisition::{self, FetchOutcome};

const REPO: &str = "unsloth/Qwen3.8-Flash-Next-GGUF";
const SUBPATH: &str = "UD-Q2_K_XL";
const SHARD1_SIZE: u64 = 10_946_618;

#[tokio::test]
#[ignore]
async fn fetches_resumes_and_verifies_a_real_hf_shard() {
    if std::env::var_os("HKASK_LIVE_HF").is_none() {
        eprintln!("HKASK_LIVE_HF unset; skipping the live acquisition fetch");
        return;
    }

    let files = acquisition::list_repo_files(REPO, SUBPATH, None)
        .await
        .expect("anonymous tree listing works for a public repo");
    assert_eq!(files.len(), 4, "the UD-Q2_K_XL subpath carries four shards");
    let shard1 = files
        .iter()
        .find(|file| file.path.ends_with("00001-of-00004.gguf"))
        .expect("shard 1 is listed");
    assert_eq!(shard1.size, SHARD1_SIZE);
    assert!(
        shard1.lfs_sha256.is_some(),
        "GGUF shards are LFS-backed; the content sha is what fetch verifies"
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let dest = dir.path().join("shard1.gguf");

    // Fresh fetch: size + (inside fetch_file) full-file sha256 verification.
    let outcome = acquisition::fetch_file(REPO, shard1, &dest, None)
        .await
        .expect("fresh fetch completes and verifies");
    assert!(matches!(
        outcome,
        FetchOutcome::Completed { bytes } if bytes == SHARD1_SIZE
    ));

    // Resume leg: truncate to half and re-fetch — the Range/206 path must
    // resume (not restart), and the whole-file sha256 must still verify.
    let half = shard1.size / 2;
    let handle = std::fs::OpenOptions::new()
        .write(true)
        .open(&dest)
        .expect("open the partial for truncation");
    handle.set_len(half).expect("truncate to half");
    let outcome = acquisition::fetch_file(REPO, shard1, &dest, None)
        .await
        .expect("resumed fetch completes and re-verifies");
    assert!(matches!(
        outcome,
        FetchOutcome::Resumed { resumed_from, bytes }
            if resumed_from == half && bytes == SHARD1_SIZE
    ));
}
