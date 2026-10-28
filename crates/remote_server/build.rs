#![allow(clippy::disallowed_methods, reason = "build scripts are exempt")]
use std::process::Command;

// zed-kask: D56 — D7 unifies the app version at the workspace level
// (`version.workspace = true` in crates/zed/Cargo.toml, enforced by
// `kask/scripts/check-version-sync.sh`). Resolve the version from the
// workspace root's [workspace.package] only. The former literal-in-zed-
// manifest branch (upstream-layout tolerance) fired only in a tree where
// that sync gate already fails — and when it fired it silently reported
// upstream's literal here while the app reported the workspace version,
// the split-brain D7 exists to prevent — so it is removed rather than
// kept as dead tolerance.
const WORKSPACE_MANIFEST: &str = include_str!("../../Cargo.toml");

fn zed_pkg_version() -> String {
    let root: toml::Value =
        toml::from_str(WORKSPACE_MANIFEST).expect("failed to parse workspace Cargo.toml");
    root.get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"))
        .and_then(|version| version.as_str())
        .expect("workspace [workspace.package].version is missing — D7 requires it")
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
