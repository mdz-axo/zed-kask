//! The live startup-probe gate (P1 of the llama.cpp consolidation plan
//! REV 2): the canonical build at `placement::engine_root()` must be
//! ACCEPTED by the crate's own startup probe against the shipped
//! manifest — exact-commit plus the full flag vocabulary. This is the
//! machine-gated receipt for "the startup probe accepts" (the capability
//! map's fleet-serving row); it skips visibly, never passes silently,
//! when no canonical build is installed (probe_backend() == None).

use hkask_private_inference::local_leg::manifest::shipped_manifest;
use hkask_private_inference::local_leg::placement::{GpuBackend, probe_backend};
use hkask_private_inference::local_leg::probe::probe_binary;

#[tokio::test]
async fn startup_probe_accepts_the_canonical_build() {
    let manifest = shipped_manifest().expect("the shipped manifest decodes");
    let backend = probe_backend();
    let binary = match backend {
        GpuBackend::Hip { binary } | GpuBackend::Vulkan { binary } => binary,
        GpuBackend::None => {
            eprintln!(
                "SKIP: no canonical llama.cpp build under {} (build via \
                 kask/scripts/build-llamacpp-mainline.sh)",
                hkask_private_inference::local_leg::placement::engine_root().display()
            );
            return;
        }
    };
    let report = probe_binary(&binary, &manifest)
        .await
        .expect("the startup probe accepts the canonical build");
    assert_eq!(report.commit, manifest.version_pin.commit);
    assert_eq!(report.version, manifest.version_pin.version);
}
