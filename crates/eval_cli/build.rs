// zed-kask: D56 — D7 unifies the app version at the workspace level
// (`version.workspace = true` in crates/zed/Cargo.toml, enforced by
// `kask/scripts/check-version-sync.sh`). Resolve the version from the
// workspace root's [workspace.package].version only: the former
// literal-in-zed-manifest scan (upstream-layout tolerance) fired only in
// a tree where that sync gate already fails, and when it fired it
// reported a different version than the app — the split-brain D7 exists
// to prevent — so it is removed rather than kept as dead tolerance.
fn zed_pkg_version() -> String {
    let root =
        std::fs::read_to_string("../../Cargo.toml").expect("Failed to read workspace Cargo.toml");
    let mut in_package_section = false;
    for line in root.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package_section = line == "[workspace.package]";
        } else if in_package_section && line.starts_with("version = ") {
            return line
                .split('=')
                .nth(1)
                .expect("Invalid version format")
                .trim()
                .trim_matches('"')
                .to_string();
        }
    }
    panic!("workspace [workspace.package].version not found — D7 requires it");
}

fn main() {
    println!("cargo:rerun-if-changed=../../Cargo.toml");
    println!("cargo:rustc-env=ZED_PKG_VERSION={}", zed_pkg_version());
}
