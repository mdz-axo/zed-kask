//! Build-profile seam — the parse half of the structural contract on the
//! root `Cargo.toml` `[profile.release]` tables (DIVERGENCE.md D46).
//!
//! `kask/scripts/build/check-build-profile.sh` owns the seam greps — the
//! `[profile.release-mcp]` block, the install.sh split build, the jobs cap,
//! the sccache wiring — but bash cannot parse TOML, and hand-rolled TOML
//! shape logic in bash is worse than the disease. This test owns what bash
//! cannot do: `[profile.release]` external dependencies must not inherit
//! single-unit release codegen, and zed's explicit codegen setting and
//! thin LTO must stay preserved. Replaces the gate's former `tomllib`
//! heredoc (landed 2026-09-08 under the 2026-09-09 ruling; migrated by the
//! zero-Python operator ruling 2026-10-13).
//!
//! `BUILD_PROFILE_MANIFEST` overrides the manifest path (the selftest
//! convention, e.g. provider_profiles.rs's `PROFILES_DIR`) so a fixture
//! tree can point this test elsewhere.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct Manifest {
    profile: Profiles,
}

#[derive(Deserialize)]
struct Profiles {
    release: ReleaseProfile,
}

#[derive(Deserialize)]
struct ReleaseProfile {
    lto: String,
    package: std::collections::BTreeMap<String, PackageCodegen>,
}

#[derive(Deserialize)]
struct PackageCodegen {
    #[serde(rename = "codegen-units")]
    codegen_units: i64,
}

fn manifest_path() -> PathBuf {
    std::env::var_os("BUILD_PROFILE_MANIFEST")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Cargo.toml"))
}

#[test]
fn release_profile_preserves_the_d46_codegen_seam() {
    let path = manifest_path();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let manifest: Manifest =
        toml::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let release = &manifest.profile.release;
    assert_eq!(
        release.lto, "thin",
        "profile.release.lto must stay thin (D46) — zed's explicit LTO setting"
    );
    let dependencies = release.package.get("*").unwrap_or_else(|| {
        panic!(
            "{}: profile.release.package.* missing — external dependencies would inherit \
             single-unit release codegen (D46)",
            path.display()
        )
    });
    assert_eq!(
        dependencies.codegen_units, 16,
        "profile.release.package.*.codegen-units must be 16 (D46)"
    );
    let zed = release.package.get("zed").unwrap_or_else(|| {
        panic!(
            "{}: profile.release.package.zed missing — zed's explicit codegen setting must \
             stay preserved (D46)",
            path.display()
        )
    });
    assert_eq!(
        zed.codegen_units, 16,
        "profile.release.package.zed.codegen-units must stay 16 (D46)"
    );
}
