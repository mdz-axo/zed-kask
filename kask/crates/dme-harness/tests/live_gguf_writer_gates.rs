//! The live GGUF reader/writer gates.
//!
//! Machine-gated receipts over the REAL DME artifacts. The directory
//! holding them is named by `DME_HARNESS_ARTIFACT_DIR`; the test skips
//! visibly (never passes silently) when the variable is unset or the
//! files are absent:
//!   1. byte-identity on re-parse — parse determinism, plus the writer's
//!      no-edit round trip leaves the file byte-identical (sha256), plus
//!      the alpha=0 strided edit on the real tensor bytes is byte-identical;
//!   2. param_count parity 752,393,024 on the BF16 target;
//!   3. requant floor exactly 0.0 — requant(dequant(bytes)) == bytes on the
//!      real Q8_0 tensor (ggml bit-compatibility).
//!
//! ```sh
//! DME_HARNESS_ARTIFACT_DIR=/path/to/qwen35-0.8b \
//!   cargo test -p dme-harness --test live_gguf_writer_gates -- --nocapture
//! ```

use std::collections::BTreeSet;
use std::path::PathBuf;

use dme_harness::{gguf, gguf_writer};

const ARTIFACT_DIR_ENV: &str = "DME_HARNESS_ARTIFACT_DIR";

/// A DME artifact under the directory `DME_HARNESS_ARTIFACT_DIR` names.
fn dme_artifact(name: &str) -> Option<PathBuf> {
    let dir = std::env::var_os(ARTIFACT_DIR_ENV)?;
    let path = PathBuf::from(dir).join(name);
    path.is_file().then_some(path)
}

/// A scratch dir removed on drop (std-only — the harness carries no
/// external dev-dependency).
struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("dme-live-gates-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the scratch dir");
        Self(dir)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const PARAM_COUNT_TARGET: u64 = 752_393_024;

#[test]
fn gguf_component_gates_on_the_real_dme_artifacts() {
    let Some(bf16) = dme_artifact("Qwen3.5-0.8B-BF16.gguf") else {
        eprintln!(
            "SKIP: the DME edit target Qwen3.5-0.8B-BF16.gguf was not found \
             (set {ARTIFACT_DIR_ENV} to the directory holding it)"
        );
        return;
    };
    let Some(q8) =
        dme_artifact("Qwen3.5-0.8B-Q8_0.gguf").or_else(|| dme_artifact("Qwen3.5-0.8B-q8_0.gguf"))
    else {
        eprintln!("SKIP: the DME smoke artifact Qwen3.5-0.8B-Q8_0.gguf is not on disk");
        return;
    };

    // --- gate 1a: parse determinism (byte-identity of the parse) ---
    let a = gguf::parse_file(&bf16).expect("BF16 target parses");
    let b = gguf::parse_file(&bf16).expect("BF16 target re-parses");
    assert_eq!(format!("{a:?}"), format!("{b:?}"), "parse is deterministic");

    // --- gate 2: param_count parity on the BF16 target ---
    assert_eq!(a.param_count, PARAM_COUNT_TARGET, "param_count parity");
    assert_eq!(a.architecture(), "qwen35");
    assert_eq!(a.dominant_type(), gguf::GGML_BF16);

    // --- gate 3: requant floor exactly 0.0 on the real Q8_0 tensor ---
    let qmeta = gguf::parse_file(&q8).expect("Q8_0 smoke parses");
    assert_eq!(qmeta.param_count, PARAM_COUNT_TARGET);
    let t = qmeta
        .tensor("blk.15.ffn_down.weight")
        .expect("the anchor tensor is present");
    let raw = gguf_writer::read_tensor_bytes(&q8, &qmeta, t).expect("tensor bytes read");
    let de = gguf_writer::dequant_q8_0(&raw, t.nelements() as usize);
    let rq = gguf_writer::requant_q8_0(&de);
    assert_eq!(
        rq, raw,
        "requant floor must be exactly 0.0 (ggml bit-compatibility)"
    );

    // --- gate 1b: the alpha=0 strided edit is byte-identical on the
    //     real tensor (the full edit path, zero edit) ---
    let ne0 = t.dims[0] as usize;
    let ne1 = t.dims[1] as usize;
    assert_eq!(
        ne1, 1024,
        "the anchor's direction space is the 1024-dim residual"
    );
    let chans: BTreeSet<usize> = gguf_writer::spread_channels(ne0, 64).into_iter().collect();
    let dir = vec![1e-3f32; ne1];
    let (patched, stats) = gguf_writer::subtract_strided_columns(
        &raw,
        t.ggml_type,
        t.nelements() as usize,
        ne0,
        ne1,
        &chans,
        0.0,
        &dir,
    )
    .expect("alpha=0 strided edit runs");
    assert_eq!(patched, raw, "alpha=0 edit must be byte-identical");
    assert_eq!(stats.unedited_elements_changed, 0);

    // --- gate 1c: the writer's no-edit write round-trip leaves the file
    //     byte-identical (a copy of the real artifact, one tensor written
    //     back unchanged) ---
    let scratch = ScratchDir::new();
    let copy = scratch.0.join("roundtrip.gguf");
    std::fs::copy(&q8, &copy).expect("copy the smoke artifact");
    let before = gguf_writer::sha256_file(&copy).expect("sha256 before");
    let bytes = gguf_writer::read_tensor_bytes(&copy, &qmeta, t).expect("read from copy");
    gguf_writer::write_tensor_bytes(&copy, &qmeta, t, &bytes).expect("write back unchanged");
    let after = gguf_writer::sha256_file(&copy).expect("sha256 after");
    assert_eq!(before, after, "no-edit write round-trip is byte-identical");

    // --- gate 1d (the dense-trigger extension): an all-zero dense trigger
    //     is byte-identical on the real tensor, and the channel mask is
    //     the indicator special case of the dense edit (equal patched
    //     bytes at a real alpha) ---
    let kz = vec![0f32; ne0];
    let dvec = vec![1e-3f32; ne1];
    let (pz, sz) = gguf_writer::subtract_dense_columns(
        &raw,
        t.ggml_type,
        t.nelements() as usize,
        ne0,
        ne1,
        &kz,
        1.0,
        &dvec,
    )
    .expect("zero-trigger dense edit runs");
    assert_eq!(pz, raw, "zero-trigger dense edit must be byte-identical");
    assert_eq!(sz.unedited_elements_changed, 0);
    let mut kind = vec![0f32; ne0];
    for j in gguf_writer::spread_channels(ne0, 64) {
        kind[j] = 1.0;
    }
    let (pd, _) = gguf_writer::subtract_dense_columns(
        &raw,
        t.ggml_type,
        t.nelements() as usize,
        ne0,
        ne1,
        &kind,
        1.0,
        &dvec,
    )
    .expect("indicator dense edit runs");
    let (ps, _) = gguf_writer::subtract_strided_columns(
        &raw,
        t.ggml_type,
        t.nelements() as usize,
        ne0,
        ne1,
        &chans,
        1.0,
        &dvec,
    )
    .expect("strided edit runs");
    assert_eq!(
        pd, ps,
        "the channel mask is the dense indicator special case (real alpha)"
    );

    eprintln!(
        "GATES: byte-identity (parse + zero-edit + write round-trip + dense zero-trigger) OK; \
         param_count {} == {PARAM_COUNT_TARGET}; requant floor 0.0 OK; \
         dense indicator == strided channels OK",
        a.param_count
    );
}
