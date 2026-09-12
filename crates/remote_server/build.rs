#![allow(clippy::disallowed_methods, reason = "build scripts are exempt")]
use std::process::Command;

const ZED_MANIFEST: &str = include_str!("../zed/Cargo.toml");
// zed-kask: D53 — D7 unifies the app version at the workspace level
// (`version.workspace = true` in crates/zed/Cargo.toml). The cargo_toml
// Manifest parser rejects inherited values ("inherited workspace value"),
// which broke every build touching this crate. Resolve the version from
// the literal when present (upstream layout), else from the workspace
// root's [workspace.package] (zed-kask layout).
const WORKSPACE_MANIFEST: &str = include_str!("../../Cargo.toml");

fn zed_pkg_version() -> String {
    let zed: toml::Value = toml::from_str(ZED_MANIFEST).expect("failed to parse zed Cargo.toml");
    if let Some(version) = zed
        .get("package")
        .and_then(|package| package.get("version"))
        .and_then(|version| version.as_str())
    {
        return version.to_string();
    }
    let root: toml::Value =
        toml::from_str(WORKSPACE_MANIFEST).expect("failed to parse workspace Cargo.toml");
    root.get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"))
        .and_then(|version| version.as_str())
        .expect("zed version is neither literal nor workspace-inherited")
        .to_string()
}

fn main() {
    println!("cargo:rustc-env=ZED_PKG_VERSION={}", zed_pkg_version());
    println!(
        "cargo:rustc-env=TARGET={}",
        std::env::var("TARGET").unwrap()
    );

    // Populate git sha environment variable if git is available
    println!("cargo:rerun-if-changed=../../.git/logs/HEAD");
    if let Some(output) = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
    {
        let git_sha = String::from_utf8_lossy(&output.stdout);
        let git_sha = git_sha.trim();

        println!("cargo:rustc-env=ZED_COMMIT_SHA={git_sha}");
    }
    if let Some(build_identifier) = option_env!("GITHUB_RUN_NUMBER") {
        println!("cargo:rustc-env=ZED_BUILD_ID={build_identifier}");
    }
}
