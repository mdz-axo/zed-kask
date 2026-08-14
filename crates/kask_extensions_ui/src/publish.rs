//! Publish and install pipelines for kask skills.
//!
//! Mirrors the extension marketplace pattern:
//! - **Publish:** package the skill directory into `archive.tar.gz` +
//!   `manifest.json`, upload both directly to S3 at
//!   `kask-skills/{source_user}/{skill_name}/{version}/`. The collab
//!   server's periodic poll picks it up and inserts it into Postgres.
//! - **Install:** `GET /api/kask-skills/:id/download` → S3 presigned
//!   redirect → download tarball → verify SHA256 → extract into
//!   `{global_skills_dir}/_marketplace/{source_user}/{skill_name}/`.
//!
//! Per the `.rules` trap "Cross-thread GPUI communication uses channels,
//! not `AsyncApp` handles", these pipelines run on a background executor
//! and do not capture `AsyncApp`. They return results via the GPUI
//! foreground task that spawned them.

use chrono::{Datelike, Timelike};
use std::path::{Path, PathBuf};

use agent_skills::Skill;
use anyhow::{Context as _, Result, anyhow, bail};
use async_compression::futures::bufread::{GzipDecoder, GzipEncoder};
use async_tar::Builder;
use bytes::Bytes;
use cloud_api_types::{KaskSkillManifest, KaskSkillMetadata, KaskSkillRef};
use fs::{Fs, read_dir_items};
use hkask_keystore::{
    KEY_MAX_AGE_DAYS, derive_public_key, generate_signing_keypair, load_signing_key, sign,
    store_signing_key,
};
use http_client::{AsyncBody, HttpClient, HttpClientWithUrl};
use sha2::{Digest, Sha256};
/// The S3 key prefix for kask skills. Mirrors `extensions/` for extensions.
pub const KASK_SKILLS_S3_PREFIX: &str = "kask-skills";

/// zed-kask: the content hash of a kask skill package.
///
/// A kask skill package is the triple `(SKILL.md, manifest.yaml, *.j2
/// templates)`. "Modified" = this hash differs from the shipped hash. The
/// hash is order-independent (files are sorted by name) and length-prefixed
/// per file so adjacent file boundaries can't collide. Used by the kask
/// extensions panel to badge bundled skills the user has changed; the same
/// function will identify package revisions on the extension-toml publish
/// path.
pub fn kask_skill_package_hash(files: &[(&str, &[u8])]) -> String {
    let mut sorted: Vec<(&str, &[u8])> = files.iter().copied().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    let mut hasher = Sha256::new();
    for (name, bytes) in &sorted {
        hasher.update(name.as_bytes());
        hasher.update(b"\n");
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod package_hash_tests {
    use super::kask_skill_package_hash;

    #[test]
    fn order_independent_and_stable() {
        let a = kask_skill_package_hash(&[("SKILL.md", b"body"), ("manifest.yaml", b"m")]);
        let b = kask_skill_package_hash(&[("manifest.yaml", b"m"), ("SKILL.md", b"body")]);
        assert_eq!(a, b, "hash must be order-independent");
    }

    #[test]
    fn content_change_detected() {
        let a = kask_skill_package_hash(&[("SKILL.md", b"body")]);
        let b = kask_skill_package_hash(&[("SKILL.md", b"body2")]);
        assert_ne!(a, b);
    }

    #[test]
    fn empty_vs_nonempty_differ() {
        let empty = kask_skill_package_hash(&[]);
        let one = kask_skill_package_hash(&[("SKILL.md", b"x")]);
        assert_ne!(empty, one);
        assert_eq!(empty, kask_skill_package_hash(&[]));
    }

    #[test]
    fn boundary_safety_no_collision() {
        // `f` + "abc" must not collide with `f1` + "a" and `f2` + "bc".
        let a = kask_skill_package_hash(&[("f", b"abc")]);
        let b = kask_skill_package_hash(&[("f1", b"a"), ("f2", b"bc")]);
        assert_ne!(a, b);
    }
}

/// zed-kask: Gather the shipped (embedded) package files for a skill.
///
/// A kask skill package is `(SKILL.md, manifest.yaml, *.j2 templates)` plus
/// the process manifest (`registry/manifests/<name>.yaml`) and any template
/// YAML sub-manifests. "A change is a change in any of those" — the panel
/// badges a bundled skill "Modified" when [`kask_skill_package_hash`] over
/// these files differs from the on-disk copy ([`gather_disk_skill_package`]).
///
/// `skill_md` is the shipped SKILL.md content (from `shipped_skill_seed`); the registry files come from the
/// `hkask-templates` embedded seed. Canonical names (`SKILL.md`,
/// `process.yaml`, `manifest.yaml`, `templates/<file>`) match
/// [`gather_disk_skill_package`] so the two hashes are comparable. Missing
/// files are skipped — an unmodified install has every file embedded.
pub fn gather_shipped_skill_package(name: &str, skill_md: Option<&str>) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    if let Some(content) = skill_md {
        files.push(("SKILL.md".to_string(), content.as_bytes().to_vec()));
    }
    // Process manifest (registry/manifests/<name>.yaml).
    if let Some((_, yaml)) = hkask_templates::process_manifest_seed()
        .iter()
        .find(|(n, _)| *n == name)
    {
        files.push(("process.yaml".to_string(), yaml.as_bytes().to_vec()));
    }
    // Template manifest (registry/templates/<name>/manifest.yaml).
    if let Some((_, yaml)) = hkask_templates::template_manifest_seed()
        .iter()
        .find(|(n, _)| *n == name)
    {
        files.push(("manifest.yaml".to_string(), yaml.as_bytes().to_vec()));
    }
    let prefix = format!("{name}/");
    // Jinja2 templates (registry/templates/<name>/*.j2).
    for (key, content) in hkask_templates::template_file_seed() {
        if let Some(basename) = key.strip_prefix(&prefix) {
            files.push((format!("templates/{basename}"), content.as_bytes().to_vec()));
        }
    }
    // Template YAML sub-manifests (registry/templates/<name>/*.yaml, excl manifest.yaml).
    for (key, content) in hkask_templates::template_yaml_file_seed() {
        if let Some(basename) = key.strip_prefix(&prefix) {
            files.push((format!("templates/{basename}"), content.as_bytes().to_vec()));
        }
    }
    files
}

/// zed-kask: Gather the on-disk package files for a skill. Mirrors
/// [`gather_shipped_skill_package`]'s canonical names so the two hashes are
/// comparable. SKILL.md is read from the global skills directory; the
/// registry files are read from `registry_root` (dev: `kask/registry/`,
/// prod: `{kask_data_dir}/skills/registry/`). Missing files are skipped — an
/// unmodified install has every file seeded to disk, so a missing file is
/// itself a modification signal (or not-yet-seeded, which resolves after
/// startup; the panel fetches on user toggle, post-startup).
pub async fn gather_disk_skill_package(
    fs: &dyn Fs,
    name: &str,
    registry_root: &Path,
) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let skill_md = agent_skills::global_skills_dir()
        .join(name)
        .join("SKILL.md");
    if fs.is_file(&skill_md).await {
        if let Ok(content) = fs.load(&skill_md).await {
            files.push(("SKILL.md".to_string(), content.into_bytes()));
        }
    }
    let process_yaml = registry_root.join("manifests").join(format!("{name}.yaml"));
    if fs.is_file(&process_yaml).await {
        if let Ok(content) = fs.load(&process_yaml).await {
            files.push(("process.yaml".to_string(), content.into_bytes()));
        }
    }
    let template_manifest = registry_root
        .join("templates")
        .join(name)
        .join("manifest.yaml");
    if fs.is_file(&template_manifest).await {
        if let Ok(content) = fs.load(&template_manifest).await {
            files.push(("manifest.yaml".to_string(), content.into_bytes()));
        }
    }
    // Template dir: *.j2 and *.yaml (excl manifest.yaml).
    let template_dir = registry_root.join("templates").join(name);
    if fs.is_dir(&template_dir).await {
        if let Ok(entries) = read_dir_items(fs, &template_dir).await {
            for (path, is_dir) in entries {
                if is_dir {
                    continue;
                }
                let Some(file_name) = path.file_name().and_then(|f| f.to_str()) else {
                    continue;
                };
                if file_name == "manifest.yaml" {
                    continue;
                }
                let is_skill_artifact = matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("j2") | Some("yaml")
                );
                if is_skill_artifact {
                    if let Ok(content) = fs.load(&path).await {
                        files.push((format!("templates/{file_name}"), content.into_bytes()));
                    }
                }
            }
        }
    }
    files
}

/// zed-kask: Decide the on-disk registry root for bundled-skill package
/// hashing. Dev (source tree present): `kask/registry/`. Prod (seeded):
/// `{kask_data_dir}/skills/registry/` — a subdirectory of the skills class,
/// containing execution manifests and templates for all skills. Sibling of
/// the per-skill dirs under `skills/`.
/// Pure decision behind the async `fs.is_dir` check in `fetch_bundled_skills`,
/// extracted so the dev/prod branch + the no-parent fallback are testable
/// without a GPUI executor. Mirrors the resolution in `main.rs`.
pub fn resolve_registry_root(dev_manifests_exist: bool, globals_dir: &Path) -> PathBuf {
    if dev_manifests_exist {
        return PathBuf::from("kask/registry");
    }
    // D28 — registry is a child of the skills dir, not a sibling.
    globals_dir.join("registry")
}

#[cfg(test)]
mod resolve_registry_root_tests {
    use super::resolve_registry_root;
    use std::path::Path;

    #[test]
    fn dev_source_tree_present_uses_kask_registry() {
        let root = resolve_registry_root(true, Path::new("/data/skills"));
        assert_eq!(root, Path::new("kask/registry"));
    }

    // D28 — registry is now a child of the skills dir, not a sibling.
    #[test]
    fn prod_uses_seeded_registry_child_of_skills_dir() {
        let root = resolve_registry_root(false, Path::new("/data/skills"));
        assert_eq!(root, Path::new("/data/skills/registry"));
    }
}

#[cfg(test)]
mod gather_package_tests {
    use super::{gather_shipped_skill_package, kask_skill_package_hash};

    /// A known shipped skill (metacognition) must produce a package that
    /// includes the SKILL.md, the process manifest, and the template
    /// manifest — i.e. the full triple, not just SKILL.md. Pins that the
    /// embedded registry content is reachable from this crate.
    #[test]
    fn shipped_package_covers_full_triple() {
        let skill_md = agent_skills::shipped_skill_seed()
            .iter()
            .find(|(name, _)| *name == "metacognition")
            .map(|(_, content)| *content)
            .expect("metacognition is a shipped skill");
        let pkg = gather_shipped_skill_package("metacognition", Some(skill_md));
        let names: Vec<&str> = pkg.iter().map(|(n, _)| n.as_str()).collect();
        assert!(
            names.contains(&"SKILL.md"),
            "package must include SKILL.md: {names:?}"
        );
        assert!(
            names.contains(&"process.yaml"),
            "package must include the process manifest: {names:?}"
        );
        assert!(
            names.contains(&"manifest.yaml"),
            "package must include the template manifest: {names:?}"
        );
        // And at least one Jinja2 template under templates/.
        assert!(
            names.iter().any(|n| n.starts_with("templates/")),
            "package must include at least one template: {names:?}"
        );
    }

    /// A nonexistent skill yields an empty package (no SKILL.md, no registry
    /// files) — the panel skips empty packages rather than badging them.
    #[test]
    fn unknown_skill_yields_empty_package() {
        let pkg = gather_shipped_skill_package("does-not-exist", None);
        assert!(
            pkg.is_empty(),
            "unknown skill must produce an empty package"
        );
    }

    /// The core correctness property of the "Modified" badge: an unmodified
    /// skill's on-disk source must hash-identically to the shipped (embedded)
    /// package. If this ever breaks, every bundled skill would falsely badge
    /// "Modified". Reads the real source files (the same files `build.rs`
    /// embeds via `include_str!`) with `std::fs` and compares against the
    /// embedded gather, pinning the canonical-name alignment between the two
    /// gather paths.
    #[test]
    fn unmodified_skill_on_disk_source_matches_shipped_package() {
        let name = "metacognition";
        let skill_md = agent_skills::shipped_skill_seed()
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, content)| *content)
            .expect("metacognition is a shipped skill");
        let shipped = gather_shipped_skill_package(name, Some(skill_md));

        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("repo root from crate manifest dir");
        let mut disk: Vec<(String, Vec<u8>)> = Vec::new();
        disk.push((
            "SKILL.md".to_string(),
            std::fs::read(repo_root.join(".agents/skills").join(name).join("SKILL.md"))
                .expect("read source SKILL.md"),
        ));
        let process_yaml = repo_root
            .join("kask/registry/manifests")
            .join(format!("{name}.yaml"));
        if process_yaml.is_file() {
            disk.push((
                "process.yaml".to_string(),
                std::fs::read(&process_yaml).unwrap(),
            ));
        }
        let template_manifest = repo_root
            .join("kask/registry/templates")
            .join(name)
            .join("manifest.yaml");
        if template_manifest.is_file() {
            disk.push((
                "manifest.yaml".to_string(),
                std::fs::read(&template_manifest).unwrap(),
            ));
        }
        let template_dir = repo_root.join("kask/registry/templates").join(name);
        for entry in std::fs::read_dir(&template_dir).expect("read template dir") {
            let path = entry.unwrap().path();
            if path.is_dir() {
                continue;
            }
            let file_name = path.file_name().unwrap().to_str().unwrap();
            if file_name == "manifest.yaml" {
                continue;
            }
            let is_skill_artifact = matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("j2") | Some("yaml")
            );
            if is_skill_artifact {
                disk.push((
                    format!("templates/{file_name}"),
                    std::fs::read(&path).unwrap(),
                ));
            }
        }

        let shipped_files: Vec<(&str, &[u8])> = shipped
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect();
        let disk_files: Vec<(&str, &[u8])> = disk
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect();
        let shipped_hash = kask_skill_package_hash(&shipped_files);
        let disk_hash = kask_skill_package_hash(&disk_files);
        assert_eq!(
            shipped_hash, disk_hash,
            "an unmodified skill's on-disk source must hash-match the shipped package \
             or every bundled skill would falsely badge Modified"
        );
    }
}

/// zed-kask: Scan a block of text — a multiplayer channel message, a
/// notification body, a contact-share note — for `kask-skill://` references
/// and return the parsed refs. This is the multiplayer→skill bridge: the
/// discreet-piggyback design carries skill refs as ordinary message text,
/// so discovering a shared skill = scanning the message body for the URI.
/// Trailing punctuation is trimmed so a ref at end-of-sentence still parses.
/// Deduplicates while preserving first-seen order.
pub fn scan_for_skill_refs(text: &str) -> Vec<KaskSkillRef> {
    let needle = format!("{}://", KaskSkillRef::SCHEME);
    let mut refs = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(needle.as_str()) {
        let after_scheme = start + needle.len();
        let tail = &rest[after_scheme..];
        let end = tail.find(|c: char| c.is_whitespace()).unwrap_or(tail.len());
        let raw = &rest[start..after_scheme + end];
        let trimmed = raw.trim_end_matches(|c: char| {
            matches!(
                c,
                '.' | ',' | ';' | '!' | '?' | ')' | ']' | '`' | '\'' | '"'
            )
        });
        if let Some(reff) = KaskSkillRef::parse(trimmed) {
            if !refs.contains(&reff) {
                refs.push(reff);
            }
        }
        rest = &rest[after_scheme + end..];
    }
    refs
}

#[cfg(test)]
mod skill_ref_scan_tests {
    use super::scan_for_skill_refs;

    #[test]
    fn finds_bare_ref_in_message() {
        let msg = "check out kask-skill://alice/essentialist/2026-08-02.1 — it's great";
        let refs = scan_for_skill_refs(msg);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].source_user, "alice");
        assert_eq!(refs[0].skill_name, "essentialist");
        assert_eq!(refs[0].version, "2026-08-02.1");
    }

    #[test]
    fn trims_trailing_punctuation() {
        let msg = "I use kask-skill://alice/essentialist/1.0.";
        let refs = scan_for_skill_refs(msg);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].version, "1.0");
    }

    #[test]
    fn deduplicates_and_preserves_order() {
        let msg = "kask-skill://alice/a/1 kask-skill://bob/b/2 kask-skill://alice/a/1";
        let refs = scan_for_skill_refs(msg);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].skill_name, "a");
        assert_eq!(refs[1].skill_name, "b");
    }

    #[test]
    fn ignores_non_skill_uris() {
        let msg = "see https://example.com/foo and http://alice/essentialist/1";
        assert!(scan_for_skill_refs(msg).is_empty());
    }

    #[test]
    fn no_refs_in_plain_text() {
        assert!(scan_for_skill_refs("just a normal channel message").is_empty());
    }

    #[test]
    fn ref_in_markdown_link_target_is_found() {
        // A ref may appear as a markdown link target: [label](kask-skill://...).
        // The scanner finds the scheme prefix wherever it occurs.
        let msg = "shared [essentialist](kask-skill://alice/essentialist/1) in the channel";
        let refs = scan_for_skill_refs(msg);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].skill_name, "essentialist");
    }
}
///
/// Decoupled from `server_url` (which points at Zed's cloud for login,
/// telemetry, collab) so the marketplace can target a kask-aware collab
/// server without breaking Zed account auth.
///
/// Resolution order:
/// 1. `HKASK_MARKETPLACE_URL` env var — operator/dev override for the
///    split-auth case (Zed account on zed.dev, skill traffic elsewhere).
/// 2. The client's own `server_url` (`http_client.base_url()`) — the normal
///    self-hosted case: the kask collab server already serves
///    `/api/kask-skills`, so no second URL is needed.
/// 3. `http://localhost:3000` — dev fallback when the client has no URL
///    (e.g. not logged in).
///
/// Returns the base URL with no trailing slash.
/// Pure decision behind `kask_marketplace_base_url`, extracted for
/// testability (env-var tests are racy under parallel test runners).
fn resolve_marketplace_base(env_override: Option<String>, server_url: String) -> String {
    match env_override {
        Some(val) if !val.trim().is_empty() => val.trim_end_matches('/').to_string(),
        _ => {
            if server_url.trim().is_empty() {
                log::warn!(
                    "HKASK_MARKETPLACE_URL not set and the client has no server_url — \
                     falling back to localhost:3000. Marketplace operations \
                     (publish/install/vote) will fail unless a marketplace server \
                     is running locally. Set HKASK_MARKETPLACE_URL to point to a \
                     production marketplace, or log in so server_url is populated."
                );
                "http://localhost:3000".to_string()
            } else {
                server_url.trim_end_matches('/').to_string()
            }
        }
    }
}

fn kask_marketplace_base_url(http_client: &HttpClientWithUrl) -> String {
    resolve_marketplace_base(
        std::env::var("HKASK_MARKETPLACE_URL").ok(),
        http_client.base_url(),
    )
}

/// Whether the Zed account `Authorization` header may be attached to a
/// marketplace request. The credentials are issued by `server_url`'s host;
/// sending them to a different host leaks the account token, so the header
/// is only attached when the resolved marketplace URL is same-host.
fn credentials_allowed_for_url(http_client: &HttpClientWithUrl, marketplace_url: &str) -> bool {
    let base = http_client.base_url();
    if base.trim().is_empty() {
        return false;
    }
    let host_of = |url: &str| {
        url.trim_end_matches('/')
            .split_once("://")
            .map(|(_, rest)| rest.split('/').next().unwrap_or("").to_string())
            .unwrap_or_default()
    };
    let allowed = host_of(marketplace_url) == host_of(&base);
    if !allowed {
        log::warn!(
            "kask-extensions: withholding Zed credentials from marketplace host — \
             the resolved marketplace URL '{marketplace_url}' is not same-host with \
             the credential issuer '{base}'. The operation will likely fail with 401. \
             Remediation: point HKASK_MARKETPLACE_URL at the same host as server_url, \
             or obtain credentials issued by the marketplace host."
        );
    }
    allowed
}

/// zed-kask: Build a full marketplace URL string by joining `path` to the
/// marketplace base URL. `path` should start with `/` (e.g. `/api/kask-skills`).
/// `query` is a slice of `(key, value)` pairs; pass `&[]` for none.
///
/// Unlike `build_zed_api_url`, this does NOT remap `zed.dev` → `api.zed.dev`
/// or `localhost:3000` → `localhost:8080`. The marketplace URL is used as-is.
/// Callers pass the result to `http_client.get(url_str, ...)` or
/// `Request::post(url_str, ...)`.
pub fn kask_marketplace_url(
    http_client: &HttpClientWithUrl,
    path: &str,
    query: &[(&str, &str)],
) -> Result<String> {
    let base = kask_marketplace_base_url(http_client);
    let mut url_str = format!("{}{}", base, path);
    if !query.is_empty() {
        let query_string = query
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        url_str.push('?');
        url_str.push_str(&query_string);
    }
    Ok(url_str)
}

/// Build a request with the Zed account `Authorization` header attached
/// only when `credentials_allowed_for_url` permits it. See that function
/// for the same-host rationale.
fn authed_request(
    method: http_client::http::method::Method,
    url: &str,
    http_client: &HttpClientWithUrl,
    credentials: &client::Credentials,
) -> http_client::http::request::Builder {
    let mut request = http_client::http::Request::builder()
        .method(method)
        .uri(url);
    if credentials_allowed_for_url(http_client, url) {
        request = request.header("Authorization", credentials.authorization_header());
    }
    request
}

/// Package a skill directory into a tar.gz archive and compute its SHA256.
///
/// Reads `SKILL.md` + `manifest.yaml` + all `*.j2` templates from the skill
/// directory. Returns the tarball bytes, the SHA256 hex digest, and the
/// manifest JSON.
pub async fn package_skill_for_publish(
    fs: &dyn Fs,
    skill: &Skill,
    source_user: &str,
    version: &str,
) -> Result<(Vec<u8>, String, String)> {
    let skill_dir = &skill.directory_path;

    // Collect all files in the skill directory. `read_dir_items` returns
    // `Vec<(PathBuf, is_dir)>` — simpler than streaming the raw `read_dir`.
    let entries = read_dir_items(fs, skill_dir)
        .await
        .with_context(|| format!("failed to read skill directory at {}", skill_dir.display()))?;

    let mut tar_builder = Builder::new(Vec::new());
    let mut manifest_dependencies: Vec<String> = Vec::new();

    for (path, is_dir) in entries {
        // Skip directories for v1 — skills are flat.
        if is_dir {
            continue;
        }

        let file_name = path
            .file_name()
            .with_context(|| format!("path has no file name: {}", path.display()))?
            .to_string_lossy()
            .to_string();
        let relative_path = Path::new(&file_name);

        let content = fs
            .load(&path)
            .await
            .with_context(|| format!("failed to read skill file {}", path.display()))?;

        // Add to tarball.
        let mut header = async_tar::Header::new_gnu();
        header.set_path(relative_path)?;
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar_builder.append(&header, content.as_bytes()).await?;

        // Parse manifest.yaml for dependencies if this is the manifest.
        if file_name == "manifest.yaml" || file_name == "manifest.yml" {
            manifest_dependencies = parse_manifest_dependencies(&content);
        }
    }

    let tar_bytes = tar_builder.into_inner().await?;

    // Gzip the tar first, then compute SHA256 on the gzipped bytes.
    // The install path downloads the .tar.gz and verifies SHA256 on the
    // downloaded bytes — so the hash must be on the compressed data.
    let mut gzip_encoder = GzipEncoder::new(tar_bytes.as_slice());
    let mut gz_bytes = Vec::new();
    smol::io::AsyncReadExt::read_to_end(&mut gzip_encoder, &mut gz_bytes).await?;

    let sha256 = hex::encode(Sha256::digest(&gz_bytes));

    // Load the publisher's signing key, or generate + store a fresh one.
    // The key lives in the OS keychain under `signing-keys/{source_user}`
    // (hkask-keystore). Keychain failures are non-fatal for the publish
    // itself (the in-memory key signs this manifest), but are warned so an
    // operator can distinguish "key stored" from "key regenerated every
    // publish" (`.rules` startup-failure-signal trap).
    let signing_key = match load_signing_key(source_user) {
        Some(key) => key,
        None => {
            let key = generate_signing_keypair();
            if let Err(error) = store_signing_key(source_user, &key) {
                log::warn!(
                    "kask-extensions: failed to store the signing key for publisher \
                     '{source_user}' in the OS keychain: {error}. The key will be \
                     regenerated on the next publish. Remediation: check keychain \
                     availability (Linux: DBus Secret Service) or set \
                     kask.collab.enabled = false if the marketplace is unused."
                );
            }
            key
        }
    };
    let public_key = derive_public_key(&signing_key);

    // Create the manifest JSON. `expires_at` is set at signing time to
    // `now + KEY_MAX_AGE_DAYS` (the server cap) — the signature commits to
    // it, so a tampered `expires_at` invalidates the signature (plan D2).
    let expires_at =
        (chrono::Utc::now() + chrono::Duration::days(KEY_MAX_AGE_DAYS as i64)).to_rfc3339();
    let mut manifest = KaskSkillManifest {
        source_user: source_user.to_string(),
        skill_name: skill.name.clone(),
        version: version.to_string(),
        description: skill.description.clone(),
        dependencies: manifest_dependencies,
        tarball_sha256: sha256.clone(),
        public_key: public_key.to_string(),
        signature: String::new(),
        expires_at,
    };
    let canonical_bytes = manifest.canonical_signing_bytes()?;
    manifest.signature = sign(&canonical_bytes, &signing_key).to_string();
    let manifest_json = serde_json::to_string(&manifest)?;

    Ok((gz_bytes, sha256, manifest_json))
}

/// Parse the `dependencies` field from a kask skill manifest.yaml.
///
/// The manifest is YAML; we do a simple line-based parse for the
/// `dependencies:` list to avoid pulling in a full YAML parser. The
/// dependencies are skill IDs (`{source_user}/{skill_name}`).
fn parse_manifest_dependencies(manifest_content: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut in_deps = false;
    for line in manifest_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("dependencies:") {
            in_deps = true;
            continue;
        }
        if in_deps {
            // A dependency list item starts with `- `.
            if let Some(dep) = trimmed.strip_prefix("- ") {
                deps.push(dep.trim().trim_matches('"').to_string());
            } else if !trimmed.is_empty() && !trimmed.starts_with('-') {
                // We've left the dependencies list.
                in_deps = false;
            }
        }
    }
    deps
}

/// Publish a skill to the marketplace by uploading to S3.
///
/// This is called by the `SkillVisibilityQueue` drain task when a skill is
/// toggled to `Public`. It packages the skill and uploads both
/// `archive.tar.gz` and `manifest.json` to S3 at
/// `kask-skills/{source_user}/{skill_name}/{version}/`.
///
/// On failure: `log::warn!` with skill ID, failure reason, remediation.
/// The local `visibility` flag is NOT rolled back (plan §2.6).
pub async fn publish_skill(
    fs: &dyn Fs,
    http_client: &HttpClientWithUrl,
    credentials: &client::Credentials,
    skill: &Skill,
    source_user: &str,
    version: &str,
) -> Result<()> {
    log::info!(
        "kask-extensions: publishing skill '{}/{}' version {}",
        source_user,
        skill.name,
        version
    );

    let (tarball_bytes, _sha256, manifest_json) =
        package_skill_for_publish(fs, skill, source_user, version)
            .await
            .with_context(|| {
                format!(
                    "failed to package skill '{}/{}' for publish",
                    source_user, skill.name
                )
            })?;

    // Upload to S3 via the collab server's upload endpoint.
    // The collab server proxies the upload to S3 (mirrors how extension
    // uploads work in production — the client doesn't have direct S3
    // credentials).
    let s3_key = format!(
        "{}/{}/{}/{}/archive.tar.gz",
        KASK_SKILLS_S3_PREFIX, source_user, skill.name, version
    );
    let manifest_key = format!(
        "{}/{}/{}/{}/manifest.json",
        KASK_SKILLS_S3_PREFIX, source_user, skill.name, version
    );

    // Upload tarball.
    let upload_url = crate::publish::kask_marketplace_url(
        http_client,
        "/api/kask-skills/upload",
        &[("key", &s3_key)],
    )?;
    http_client
        .send(
            authed_request(
                http_client::http::method::Method::POST,
                &upload_url,
                http_client,
                credentials,
            )
            .header("Content-Type", "application/octet-stream")
            .body(AsyncBody::from_bytes(Bytes::from(tarball_bytes)))?,
        )
        .await
        .context("uploading kask skill tarball")?;

    // Upload manifest.
    let manifest_upload_url = crate::publish::kask_marketplace_url(
        http_client,
        "/api/kask-skills/upload",
        &[("key", &manifest_key)],
    )?;
    http_client
        .send(
            authed_request(
                http_client::http::method::Method::POST,
                &manifest_upload_url,
                http_client,
                credentials,
            )
            .header("Content-Type", "application/json")
            .body(AsyncBody::from_bytes(Bytes::from(
                manifest_json.into_bytes(),
            )))?,
        )
        .await
        .context("uploading kask skill manifest")?;

    log::info!(
        "kask-extensions: published skill '{}/{}' version {}",
        source_user,
        skill.name,
        version
    );

    Ok(())
}

/// Unpublish a skill by deleting it from S3.
///
/// This is called by the `SkillVisibilityQueue` drain task when a skill is
/// toggled back to `Private`. The local skill stays on disk; only the
/// marketplace listing is removed.
pub async fn unpublish_skill(
    http_client: &HttpClientWithUrl,
    credentials: &client::Credentials,
    source_user: &str,
    skill_name: &str,
) -> Result<()> {
    log::info!(
        "kask-extensions: unpublishing skill '{}/{}'",
        source_user,
        skill_name
    );

    // zed-kask: URL-encode the skill ID (alice/bug-hunt → alice%2Fbug-hunt)
    // so it's a single path segment. The server decodes it back.
    let skill_id_str = format!("{}/{}", source_user, skill_name);
    let encoded_id = urlencoding::encode(&skill_id_str);
    let delete_url = crate::publish::kask_marketplace_url(
        http_client,
        &format!("/api/kask-skills/{}", encoded_id),
        &[],
    )?;
    http_client
        .send(
            authed_request(
                http_client::http::method::Method::DELETE,
                &delete_url,
                http_client,
                credentials,
            )
            .body(AsyncBody::empty())?,
        )
        .await
        .context("unpublishing kask skill")?;

    log::info!(
        "kask-extensions: unpublished skill '{}/{}'",
        source_user,
        skill_name
    );

    Ok(())
}

/// Verify a kask skill manifest's signature and expiry on the install path
/// (plan Phase 4 / D3).
///
/// The manifest comes from the **catalog** (`KaskSkillMetadata` flattens it),
/// not from a downloaded artifact — the server verified it at upload/poll
/// time, so the client trusts the server-indexed `public_key`. This check
/// re-verifies over the same canonical bytes (plan D4) before any tarball
/// bytes are touched, and rejects a manifest whose `expires_at` has passed
/// even if the server's sweep hasn't run yet.
fn verify_install_manifest(manifest: &KaskSkillManifest) -> Result<()> {
    let public_key: hkask_types::Ed25519PublicKey = manifest
        .public_key
        .parse()
        .map_err(|_| anyhow::anyhow!("kask skill manifest has an invalid public key"))?;
    let signature: hkask_types::Ed25519Signature = manifest
        .signature
        .parse()
        .map_err(|_| anyhow::anyhow!("kask skill manifest has an invalid signature"))?;

    let canonical = manifest.canonical_signing_bytes()?;
    if !hkask_keystore::signing::verify(&canonical, &signature, &public_key) {
        bail!(
            "kask skill signature does not verify against the catalog public key; \
             the manifest or catalog entry may be tampered with. Do not install."
        );
    }

    let now = chrono::Utc::now();
    let expires_at = chrono::DateTime::parse_from_rfc3339(&manifest.expires_at)
        .with_context(|| "kask skill manifest has an invalid expires_at")?;
    if expires_at.with_timezone(&chrono::Utc) <= now {
        bail!(
            "kask skill manifest expired at {}; re-publish the skill to continue using it.",
            manifest.expires_at
        );
    }

    Ok(())
}

/// The install directory and download URL derive from `skill_id`, while
/// verification and the SHA256 bound use `manifest` — a mismatched pair
/// would extract a verified skill into the wrong directory. Enforce the
/// coupling (`.rules` "advertised invariants need enforcement points"): the
/// manifest must describe the id being installed. Pure check, extracted for
/// testability without fs/http types.
fn ensure_manifest_matches_skill_id(skill_id: &str, manifest: &KaskSkillManifest) -> Result<()> {
    if skill_id != format!("{}/{}", manifest.source_user, manifest.skill_name) {
        bail!(
            "manifest {}/{} does not match skill id {skill_id}; refusing to install",
            manifest.source_user,
            manifest.skill_name
        );
    }
    Ok(())
}

/// zed-kask: Pure version-promise check for install-from-ref. The
/// `kask-skill://` URI commits to a specific version; the catalog serves the
/// latest. Refuse if they differ — a stale ref must error loudly rather than
/// silently install a different version. Extracted for testability without
/// fs/http types (mirrors `ensure_manifest_matches_skill_id`).
fn verify_ref_version(reff: &KaskSkillRef, manifest: &KaskSkillManifest) -> Result<()> {
    if manifest.version != reff.version {
        bail!(
            "kask skill '{}' reference is for version {}, but the catalog's latest is {}; \
             the publisher may have released a newer version. Ask the sharer for an updated \
             `kask-skill://` reference.",
            reff.id(),
            reff.version,
            manifest.version
        );
    }
    Ok(())
}

/// Install a kask skill from the marketplace.
///
/// Downloads the tarball via `GET /api/kask-skills/:id/download` (which
/// redirects to an S3 presigned URL), verifies the manifest's Ed25519
/// signature against the catalog's `public_key` (plan Phase 4 / D3) and the
/// `expires_at` window, verifies the tarball SHA256, then extracts into the
/// marketplace dir under the global skills directory.
///
/// The catalog metadata (`manifest`) is the trust anchor: the signature is
/// verified over `canonical_signing_bytes()` reconstructed from the catalog
/// fields (not from a downloaded manifest — plan D4). No manifest download
/// is needed; the metadata already carries every field.
pub async fn install_skill(
    fs: &dyn Fs,
    http_client: &HttpClientWithUrl,
    skill_id: &str,
    manifest: &KaskSkillManifest,
    marketplace_dir: &Path,
) -> Result<PathBuf> {
    let (source_user, skill_name) = skill_id
        .split_once('/')
        .context("kask skill id must be \"{source_user}/{skill_name}\"")?;

    // zed-kask: the manifest must describe the id being installed (see
    // `ensure_manifest_matches_skill_id` — enforcement point).
    ensure_manifest_matches_skill_id(skill_id, manifest)?;

    // zed-kask: verify the manifest signature against the catalog's public
    // key **before** downloading anything (plan Phase 4 / D3). The catalog
    // key is server-verified; a signature that does not verify means the
    // catalog row or the manifest was tampered with. Fail closed — a skill
    // we cannot authenticate must not be installed.
    verify_install_manifest(manifest)?;

    log::info!(
        "kask-extensions: installing skill '{}/{}'",
        source_user,
        skill_name
    );

    // Download the tarball.
    let encoded_id = urlencoding::encode(skill_id);
    let download_url = crate::publish::kask_marketplace_url(
        http_client,
        &format!("/api/kask-skills/{}/download", encoded_id),
        &[],
    )?;
    let mut response = http_client
        .get(&download_url, AsyncBody::empty(), true)
        .await
        .context("downloading kask skill")?;

    let mut tar_gz_bytes = Vec::new();
    futures::AsyncReadExt::read_to_end(response.body_mut(), &mut tar_gz_bytes)
        .await
        .context("reading kask skill tarball")?;

    // Verify SHA256 (integrity of the tarball bytes; the signature already
    // authenticated the manifest, which binds this hash — plan D1).
    let actual_sha256 = hex::encode(Sha256::digest(&tar_gz_bytes));
    if actual_sha256 != manifest.tarball_sha256 {
        bail!(
            "kask skill tarball SHA256 mismatch: expected {}, got {}",
            manifest.tarball_sha256,
            actual_sha256
        );
    }

    // Extract into the marketplace directory.
    let install_dir = marketplace_dir.join(source_user).join(skill_name);

    // Remove any existing install.
    fs.remove_dir(
        &install_dir,
        fs::RemoveOptions {
            recursive: true,
            ignore_if_not_exists: true,
        },
    )
    .await?;

    // Create the install directory and all parent directories.
    // First install: the marketplace dir doesn't exist yet.
    let marketplace_parent = marketplace_dir;
    if !fs.is_dir(marketplace_parent).await {
        fs.create_dir(marketplace_parent).await?;
    }
    let user_dir = marketplace_dir.join(source_user);
    if !fs.is_dir(&user_dir).await {
        fs.create_dir(&user_dir).await?;
    }
    fs.create_dir(&install_dir).await?;

    // Decompress and extract.
    let decompressed = GzipDecoder::new(tar_gz_bytes.as_slice());
    let archive = async_tar::Archive::new(decompressed);
    archive.unpack(&install_dir).await?;

    log::info!(
        "kask-extensions: installed skill '{}/{}' to {}",
        source_user,
        skill_name,
        install_dir.display()
    );

    Ok(install_dir)
}

/// zed-kask: Fetch a single skill's catalog metadata by id via
/// `GET /api/kask-skills/:id`. The metadata carries the signed manifest
/// (`public_key`, `signature`, `expires_at`, `tarball_sha256`) — the trust
/// anchor for install. Used by the piggyback install-from-reference path: a
/// `kask-skill://` ref resolves to its id, this fetches the metadata, and
/// [`install_skill`] verifies + downloads + extracts.
pub async fn fetch_skill_metadata(
    http_client: &HttpClientWithUrl,
    skill_id: &str,
) -> Result<KaskSkillMetadata> {
    let encoded_id = urlencoding::encode(skill_id);
    let url = crate::publish::kask_marketplace_url(
        http_client,
        &format!("/api/kask-skills/{}", encoded_id),
        &[],
    )?;
    let mut response = http_client
        .get(&url, AsyncBody::empty(), true)
        .await
        .context("fetching kask skill metadata")?;
    let mut body = Vec::new();
    futures::AsyncReadExt::read_to_end(response.body_mut(), &mut body)
        .await
        .context("reading kask skill metadata response")?;
    if response.status().is_client_error() || response.status().is_server_error() {
        let text = String::from_utf8_lossy(&body);
        bail!(
            "kask skill metadata fetch failed (status {}): {text}",
            response.status().as_u16()
        );
    }
    let metadata: Option<KaskSkillMetadata> =
        serde_json::from_slice(&body).context("parsing kask skill metadata")?;
    metadata.ok_or_else(|| anyhow!("kask skill '{}' not found in catalog", skill_id))
}

/// zed-kask: Install a kask skill from a `kask-skill://` reference — the
/// discreet-piggyback consumer. Resolves the ref to its marketplace id,
/// fetches the signed metadata ([`fetch_skill_metadata`]), then reuses
/// [`install_skill`] (signature + expiry + SHA256 verification, presigned-S3
/// download, extract).
///
/// Fails closed if the catalog's latest version differs from the ref's
/// `version`: the URI promises a specific version, and silently installing a
/// different one would violate that promise. A per-version fetch route would
/// let us honor the exact version; until then, a stale ref errors loudly so
/// the sharer can re-share an updated `kask-skill://` reference.
pub async fn install_skill_from_ref(
    fs: &dyn Fs,
    http_client: &HttpClientWithUrl,
    reff: &KaskSkillRef,
    marketplace_dir: &Path,
) -> Result<PathBuf> {
    let skill_id = reff.id();
    let metadata = fetch_skill_metadata(http_client, &skill_id).await?;
    verify_ref_version(reff, &metadata.manifest)?;
    install_skill(
        fs,
        http_client,
        &skill_id,
        &metadata.manifest,
        marketplace_dir,
    )
    .await
}

/// Vote on a kask skill (+1 or -1).
pub async fn vote_skill(
    http_client: &HttpClientWithUrl,
    credentials: &client::Credentials,
    skill_id: &str,
    vote: i8,
) -> Result<(i64, i64)> {
    let encoded_id = urlencoding::encode(skill_id);
    let vote_url = crate::publish::kask_marketplace_url(
        http_client,
        &format!("/api/kask-skills/{}/vote", encoded_id),
        &[],
    )?;
    let body = serde_json::to_string(&cloud_api_types::KaskSkillVoteRequest { vote })?;
    let mut response = http_client
        .send(
            authed_request(
                http_client::http::method::Method::POST,
                &vote_url,
                http_client,
                credentials,
            )
            .header("Content-Type", "application/json")
            .body(AsyncBody::from_bytes(Bytes::from(body.into_bytes())))?,
        )
        .await
        .context("voting on kask skill")?;

    let mut resp_body = Vec::new();
    futures::AsyncReadExt::read_to_end(response.body_mut(), &mut resp_body)
        .await
        .context("reading vote response")?;

    let result: serde_json::Value = serde_json::from_slice(&resp_body)?;
    let upvote_count = result
        .get("upvote_count")
        .and_then(|v| v.as_i64())
        .context("missing upvote_count in response")?;
    let downvote_count = result
        .get("downvote_count")
        .and_then(|v| v.as_i64())
        .context("missing downvote_count in response")?;

    Ok((upvote_count, downvote_count))
}

/// Generate a timestamp-based version string for a skill publish.
/// Format: `YYYY-MM-DD.N` where N increments if published multiple times
/// in the same day.
pub fn generate_version() -> String {
    let now = chrono::Utc::now();
    format!(
        "{:04}-{:02}-{:02}.{}",
        now.year(),
        now.month(),
        now.day(),
        now.hour()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // zed-kask: pin the marketplace URL resolution order (env override →
    // client server_url → localhost dev fallback) per the `.rules` trap
    // "Tests must pin deliberate zed-kask deviations from upstream".
    #[test]
    fn env_override_wins() {
        assert_eq!(
            resolve_marketplace_base(
                Some("https://market.example.com/".to_string()),
                "https://collab.example.com".to_string(),
            ),
            "https://market.example.com"
        );
    }

    #[test]
    fn server_url_is_default() {
        assert_eq!(
            resolve_marketplace_base(None, "https://collab.example.com/".to_string()),
            "https://collab.example.com"
        );
    }

    #[test]
    fn blank_env_override_falls_through() {
        assert_eq!(
            resolve_marketplace_base(
                Some("  ".to_string()),
                "https://collab.example.com".to_string(),
            ),
            "https://collab.example.com"
        );
    }

    #[test]
    fn empty_server_url_falls_back_to_localhost() {
        assert_eq!(
            resolve_marketplace_base(None, String::new()),
            "http://localhost:3000"
        );
    }

    // zed-kask: pin the signing contract (plan Phase 1 acceptance) — a
    // manifest signature verifies over the canonical bytes, tampering with
    // `expires_at` invalidates it, and the canonical form excludes the
    // `signature` field value.
    #[test]
    fn manifest_signature_verifies_over_canonical_bytes() {
        let signing_key = generate_signing_keypair();
        let public_key = derive_public_key(&signing_key);

        let mut manifest = KaskSkillManifest {
            source_user: "alice".to_string(),
            skill_name: "essentialist".to_string(),
            version: "2026-08-02.1".to_string(),
            description: "test".to_string(),
            dependencies: vec![],
            tarball_sha256: "abc123".to_string(),
            public_key: public_key.to_string(),
            signature: String::new(),
            expires_at: (chrono::Utc::now() + chrono::Duration::days(KEY_MAX_AGE_DAYS as i64))
                .to_rfc3339(),
        };

        let canonical = manifest.canonical_signing_bytes().unwrap();
        manifest.signature = sign(&canonical, &signing_key).to_string();

        // The signature must verify over the canonical bytes of the signed
        // manifest (the `signature` field is cleared on both sides).
        let signed_canonical = manifest.canonical_signing_bytes().unwrap();
        let parsed_signature = manifest.signature.parse().unwrap();
        assert!(
            hkask_keystore::signing::verify(&signed_canonical, &parsed_signature, &public_key),
            "signature must verify over canonical bytes"
        );

        // A tampered expires_at (beyond the signing-time value) must fail
        // verification — the signature commits to the expiration (plan D2).
        let mut tampered = manifest;
        tampered.expires_at = "2099-01-01T00:00:00Z".to_string();
        let tampered_canonical = tampered.canonical_signing_bytes().unwrap();
        assert!(
            !hkask_keystore::signing::verify(&tampered_canonical, &parsed_signature, &public_key),
            "tampered expires_at must invalidate the signature"
        );
    }

    // zed-kask: pin the install-path verification (plan Phase 4 acceptance) —
    // a valid signed manifest passes `verify_install_manifest`, a tampered
    // one fails, and an expired one fails even with a valid signature.
    #[test]
    fn install_manifest_verification_accepts_valid_and_rejects_tampered_and_expired() {
        let signing_key = generate_signing_keypair();
        let public_key = derive_public_key(&signing_key);

        let make_manifest = |expires_at: String| {
            let mut manifest = KaskSkillManifest {
                source_user: "alice".to_string(),
                skill_name: "essentialist".to_string(),
                version: "2026-08-02.1".to_string(),
                description: "test".to_string(),
                dependencies: vec![],
                tarball_sha256: "abc123".to_string(),
                public_key: public_key.to_string(),
                signature: String::new(),
                expires_at,
            };
            let canonical = manifest.canonical_signing_bytes().unwrap();
            manifest.signature = sign(&canonical, &signing_key).to_string();
            manifest
        };

        // Valid: expires_at in the future, signature verifies.
        let fresh = make_manifest(
            (chrono::Utc::now() + chrono::Duration::days(KEY_MAX_AGE_DAYS as i64)).to_rfc3339(),
        );
        verify_install_manifest(&fresh).expect("valid manifest must pass install verification");

        // Tampered: a valid signature over different canonical bytes fails.
        let mut tampered = fresh;
        tampered.description = "tampered".to_string();
        assert!(
            verify_install_manifest(&tampered).is_err(),
            "tampered manifest must fail install verification"
        );

        // Expired: signature verifies, but expires_at is in the past.
        let expired = make_manifest("2020-01-01T00:00:00Z".to_string());
        assert!(
            verify_install_manifest(&expired).is_err(),
            "expired manifest must fail install verification"
        );
    }

    // zed-kask: pin the id<->manifest coupling (install-path enforcement
    // point) — `install_skill` must refuse a manifest that does not describe
    // the requested skill id, or a verified skill could be extracted into the
    // wrong directory.
    #[test]
    fn install_skill_refuses_mismatched_manifest_id() {
        let manifest = KaskSkillManifest {
            source_user: "alice".to_string(),
            skill_name: "essentialist".to_string(),
            version: "2026-08-02.1".to_string(),
            description: "test".to_string(),
            dependencies: vec![],
            tarball_sha256: "abc123".to_string(),
            public_key: "aa".repeat(32),
            signature: "bb".repeat(64),
            expires_at: "2999-01-01T00:00:00Z".to_string(),
        };

        // Matching id passes; any other id is refused before any download.
        ensure_manifest_matches_skill_id("alice/essentialist", &manifest)
            .expect("matching manifest/id must pass");
        assert!(
            ensure_manifest_matches_skill_id("alice/other", &manifest).is_err(),
            "mismatched manifest/skill id must be rejected"
        );
        assert!(
            ensure_manifest_matches_skill_id("bob/essentialist", &manifest).is_err(),
            "different source_user must be rejected"
        );
    }

    // zed-kask: pin the install-from-ref version-promise (S1). A
    // `kask-skill://…@version` ref must not silently install a different
    // version; a stale ref errors loudly so the sharer can re-share.
    #[test]
    fn install_from_ref_refuses_stale_version() {
        let manifest = KaskSkillManifest {
            source_user: "alice".to_string(),
            skill_name: "essentialist".to_string(),
            version: "2026-08-02.1".to_string(),
            description: "test".to_string(),
            dependencies: vec![],
            tarball_sha256: "abc123".to_string(),
            public_key: "aa".repeat(32),
            signature: "bb".repeat(64),
            expires_at: "2999-01-01T00:00:00Z".to_string(),
        };
        let matching = KaskSkillRef {
            source_user: "alice".into(),
            skill_name: "essentialist".into(),
            version: "2026-08-02.1".into(),
        };
        verify_ref_version(&matching, &manifest).expect("matching version must pass");
        let stale = KaskSkillRef {
            source_user: "alice".into(),
            skill_name: "essentialist".into(),
            version: "2026-08-01.9".into(),
        };
        let err =
            verify_ref_version(&stale, &manifest).expect_err("stale version must be rejected");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("reference is for version 2026-08-01.9"),
            "error must name the ref's version: {msg}"
        );
        assert!(
            msg.contains("catalog's latest is 2026-08-02.1"),
            "error must name the catalog's version: {msg}"
        );
    }
}
