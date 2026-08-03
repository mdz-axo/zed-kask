//! GitHub-backed release feed for zed-kask auto-updates (D17).
//!
//! Resolves the latest zed-kask release binary from GitHub Releases for the
//! current platform. The upstream `auto_update` crate polls zed.dev cloud
//! (`/releases/{channel}/{version}/asset`); this module provides an
//! alternative feed that queries the GitHub Releases API instead.
//!
//! The caller (`auto_update::AutoUpdater::update`) passes the resolved asset
//! back into the existing `download_release` / `install_release` /
//! `check_if_fetched_version_is_newer` pipeline — this module only resolves
//! *which* release to download, not how to download or install it.

use anyhow::{Context as _, Result, bail};
use http_client::HttpClient;
use http_client::github::{GithubRelease, GithubReleaseAsset, latest_github_release};
use std::sync::Arc;

/// Default GitHub repository that publishes zed-kask release binaries.
/// Overridable via the `HKASK_UPDATE_GITHUB_REPO` env var.
const DEFAULT_GITHUB_REPO: &str = "mdz-axo/zed-kask";

/// A resolved zed-kask release asset from GitHub.
///
/// Has the same shape as `auto_update::ReleaseAsset` (`version` + `url`) but
/// lives in kask_bridge so the GitHub-specific resolution logic stays on the
/// kask side of the D8 seam. The `auto_update` crate converts this into its own
/// `ReleaseAsset` before passing it to `download_release`.
#[derive(Clone, Debug)]
pub struct ZedKaskReleaseAsset {
    /// Semantic version string (e.g. `"0.1.0"` — the `v` prefix from the git
    /// tag is stripped before this field is populated).
    pub version: String,
    /// Direct download URL for the release asset (the GitHub
    /// `browser_download_url`).
    pub url: String,
}

/// Resolves the latest zed-kask release asset for the given platform from
/// GitHub Releases.
///
/// `pre_release` controls whether to look for a prerelease (`true`) or a
/// stable release (`false`). The caller (`auto_update`) determines this from
/// the `ReleaseChannel`.
///
/// Returns `Ok(None)` when no release or no matching asset is found — this is
/// the expected state when no GitHub releases have been published yet (or the
/// current platform has no matching asset). The caller treats `None` as "up to
/// date" (`AutoUpdateStatus::Idle`), not as an error. Network failures, API
/// errors, and deserialization errors still propagate as `Err`.
pub async fn get_zed_kask_release_asset(
    http: Arc<dyn HttpClient>,
    os: &str,
    arch: &str,
    pre_release: bool,
) -> Result<Option<ZedKaskReleaseAsset>> {
    let repo = std::env::var("HKASK_UPDATE_GITHUB_REPO")
        .unwrap_or_else(|_| DEFAULT_GITHUB_REPO.to_string());

    // `latest_github_release` returns an error when no release matches the
    // prerelease filter. The context message is "finding a prerelease" (from
    // the upstream `.context("finding a prerelease")?` call). This is "no
    // release published", not a network or API error — treat it as `None`.
    //
    // The string match is on an upstream context message; if upstream changes
    // it, this falls through to `Err` (safe degradation: the user sees an
    // error instead of "up to date", but no incorrect update occurs).
    let release = match latest_github_release(&repo, true, pre_release, http).await {
        Ok(release) => release,
        Err(e) if e.chain().any(|c| c.to_string().contains("finding a")) => {
            return Ok(None);
        }
        Err(e) => return Err(e),
    };

    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name)
        .to_string();

    // `match_asset` returns an error when no asset matches the current
    // platform's OS/ARCH/extension. We control this error message, so the
    // string match is stable.
    let asset = match match_asset(&release, os, arch) {
        Ok(asset) => asset,
        Err(e) if e.to_string().contains("no GitHub release asset found") => {
            return Ok(None);
        }
        Err(e) => return Err(e),
    };

    Ok(Some(ZedKaskReleaseAsset {
        version,
        url: asset.browser_download_url.clone(),
    }))
}

/// Selects the release asset matching the current platform from a GitHub
/// release's asset list.
///
/// Matching is by file extension (platform-specific: `.dmg` / `.tar.gz` /
/// `.exe`) and by the OS and ARCH strings appearing in the asset name
/// (case-insensitive). This accommodates common naming conventions such as
/// `zed-kask-0.1.0-linux-x86_64.tar.gz` or `Zed-Kask-macos-aarch64.dmg`.
fn match_asset<'a>(
    release: &'a GithubRelease,
    os: &str,
    arch: &str,
) -> Result<&'a GithubReleaseAsset> {
    let extension = match os {
        "macos" => ".dmg",
        "linux" => ".tar.gz",
        "windows" => ".exe",
        other => bail!("unsupported OS for GitHub release asset matching: {other}"),
    };

    let os_lower = os.to_lowercase();
    let arch_lower = arch.to_lowercase();

    release
        .assets
        .iter()
        .find(|asset| {
            let name = asset.name.to_lowercase();
            name.contains(&os_lower) && name.contains(&arch_lower) && name.ends_with(extension)
        })
        .with_context(|| {
            format!(
                "no GitHub release asset found for os={os}, arch={arch}, extension={extension}. \
                 Available assets: {:?}",
                release.assets.iter().map(|a| &a.name).collect::<Vec<_>>()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_client::github::GithubRelease;

    fn make_release(tag: &str, asset_names: &[&str]) -> GithubRelease {
        GithubRelease {
            tag_name: tag.to_string(),
            pre_release: false,
            assets: asset_names
                .iter()
                .map(|name| GithubReleaseAsset {
                    name: name.to_string(),
                    browser_download_url: format!(
                        "https://github.com/test/repo/releases/download/{tag}/{name}"
                    ),
                    digest: None,
                })
                .collect(),
            tarball_url: String::new(),
            zipball_url: String::new(),
        }
    }

    #[test]
    fn test_match_asset_linux_x86_64() {
        let release = make_release(
            "v0.1.0",
            &[
                "zed-kask-0.1.0-linux-aarch64.tar.gz",
                "zed-kask-0.1.0-linux-x86_64.tar.gz",
                "zed-kask-0.1.0-macos-x86_64.dmg",
            ],
        );
        let asset =
            match_asset(&release, "linux", "x86_64").expect("should find linux x86_64 asset");
        assert_eq!(asset.name, "zed-kask-0.1.0-linux-x86_64.tar.gz");
    }

    #[test]
    fn test_match_asset_macos_aarch64() {
        let release = make_release(
            "v0.1.0",
            &[
                "zed-kask-0.1.0-macos-x86_64.dmg",
                "Zed-Kask-macos-aarch64.dmg",
            ],
        );
        let asset =
            match_asset(&release, "macos", "aarch64").expect("should find macos aarch64 asset");
        assert_eq!(asset.name, "Zed-Kask-macos-aarch64.dmg");
    }

    #[test]
    fn test_match_asset_windows_x86_64() {
        let release = make_release(
            "v0.1.0",
            &[
                "zed-kask-0.1.0-windows-x86_64.exe",
                "zed-kask-0.1.0-linux-x86_64.tar.gz",
            ],
        );
        let asset =
            match_asset(&release, "windows", "x86_64").expect("should find windows x86_64 asset");
        assert_eq!(asset.name, "zed-kask-0.1.0-windows-x86_64.exe");
    }

    #[test]
    fn test_match_asset_no_match_returns_error() {
        let release = make_release("v0.1.0", &["zed-kask-0.1.0-linux-x86_64.tar.gz"]);
        let result = match_asset(&release, "macos", "aarch64");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no GitHub release asset found"),
            "error should mention no asset found"
        );
    }

    #[test]
    fn test_match_asset_skips_wrong_extension() {
        let release = make_release("v0.1.0", &["zed-kask-0.1.0-linux-x86_64.tar.gz"]);
        // linux asset exists but we're looking for a .dmg (macos)
        let result = match_asset(&release, "macos", "x86_64");
        assert!(
            result.is_err(),
            "should not match a .tar.gz when looking for .dmg"
        );
    }

    #[test]
    fn test_default_github_repo_is_kask() {
        // Pin the default repo so an upstream merge cannot silently revert it.
        assert_eq!(DEFAULT_GITHUB_REPO, "mdz-axo/zed-kask");
    }
}
