// zed-kask: D56 — D7 unifies the app version at the workspace level
// (`version.workspace = true` in crates/zed/Cargo.toml); the literal
// `version = "` line this scan expected no longer exists. Fall back to the
// workspace root's [workspace.package].version.
fn zed_pkg_version() -> String {
    let cargo_toml =
        std::fs::read_to_string("../zed/Cargo.toml").expect("Failed to read crates/zed/Cargo.toml");
    if let Some(version) = cargo_toml
        .lines()
        .find(|line| line.starts_with("version = "))
        .map(|line| {
            line.split('=')
                .nth(1)
                .expect("Invalid version format")
                .trim()
                .trim_matches('"')
                .to_string()
        })
    {
        return version;
    }
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
    panic!("Version not found in crates/zed/Cargo.toml or workspace Cargo.toml");
}

fn main() {
    println!("cargo:rerun-if-changed=../zed/Cargo.toml");
    println!("cargo:rerun-if-changed=../../Cargo.toml");
    println!("cargo:rustc-env=ZED_PKG_VERSION={}", zed_pkg_version());
}
