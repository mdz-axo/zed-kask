//! The installed server is built without the `test-fixture` feature.
//!
//! That feature makes the build accept `ws://` and loopback relay addresses
//! and compiles in the local test relays. An installed server must have
//! neither. Two facts make that so, and each is pinned here:
//!
//! - the feature is not on by default in this crate's manifest;
//! - no `cargo build` in the installer turns a feature on.
//!
//! What this cannot see: a build run by hand, or a build script that is not
//! `kask/scripts/build/install.sh`.

use std::path::{Path, PathBuf};

fn repository_file(relative: &str) -> (PathBuf, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    (path, text)
}

/// The installer's `cargo build` command lines: comment lines and prose
/// that merely mention the words are left out.
fn cargo_build_lines(script: &str) -> Vec<&str> {
    script
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .filter(|line| {
            line.strip_prefix("if ")
                .unwrap_or(line)
                .starts_with("cargo build ")
        })
        .collect()
}

/// expect: no build the installer runs enables a cargo feature, so the
/// installed server cannot contain the test relays or accept loopback relay
/// addresses.
/// pre: `kask/scripts/build/install.sh` as it is in the tree.
/// post: the server build line is found (the control: an empty scan would
/// pass for the wrong reason), and no `cargo build` line carries
/// `--features`, `--all-features` or `-F`.
#[test]
fn installed_build_enables_no_cargo_feature() {
    let (path, script) = repository_file("kask/scripts/build/install.sh");
    let builds = cargo_build_lines(&script);
    let server_builds: Vec<&&str> = builds
        .iter()
        .filter(|line| line.contains("--profile release-mcp"))
        .collect();
    assert_eq!(
        server_builds.len(),
        1,
        "{}: expected exactly one MCP server build line, found {builds:?}",
        path.display()
    );
    for line in &builds {
        let words: Vec<&str> = line.split_whitespace().collect();
        assert!(
            !words.iter().any(|word| {
                *word == "-F" || word.starts_with("--features") || *word == "--all-features"
            }),
            "{}: a build enables a cargo feature: {line}",
            path.display()
        );
    }
}

/// expect: the fixture feature is opt-in.
/// pre: this crate's manifest as it is in the tree.
/// post: the manifest declares `test-fixture`, and no `default` feature list
/// names it (the manifest declares no default features at all).
#[test]
fn installed_build_fixture_feature_is_not_a_default() {
    let (path, manifest) = repository_file("kask/mcp-servers/hkask-mcp-nostr-client/Cargo.toml");
    let features: Vec<&str> = manifest
        .lines()
        .skip_while(|line| line.trim() != "[features]")
        .skip(1)
        .take_while(|line| !line.trim_start().starts_with('['))
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    assert!(
        features.iter().any(|line| line.starts_with("test-fixture")),
        "{}: the test-fixture feature is declared (the control): {features:?}",
        path.display()
    );
    assert!(
        !features.iter().any(|line| line.starts_with("default")),
        "{}: a default feature list exists; it must never name test-fixture: {features:?}",
        path.display()
    );
}

/// expect: this test file itself was not compiled into a fixture build by
/// accident when run as the installed-build check.
/// pre: a default-feature test run.
/// post: the build's address policy is the public-only one.
#[cfg(not(feature = "test-fixture"))]
#[test]
fn installed_build_default_features_refuse_loopback() {
    use hkask_mcp_nostr_client::relays::{AddressPolicy, BUILD_ADDRESS_POLICY};
    assert_eq!(BUILD_ADDRESS_POLICY, AddressPolicy::PublicOnly);
}
