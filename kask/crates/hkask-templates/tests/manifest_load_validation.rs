use hkask_templates::load_manifest_from_yaml;
use std::path::Path;

/// Regression test: every YAML file with a `manifest:` key in
/// `registry/manifests/` must load successfully via `load_manifest_from_yaml`.
///
/// This catches:
/// - Unknown top-level fields (deny_unknown_fields on ManifestFile)
/// - Missing required step fields (e.g. `description`)
/// - Type mismatches (e.g. rjoule.cap as float instead of u32)
/// - Invalid manifest header fields
///
/// Files without a `manifest:` key (e.g. training recipes in
/// `manifests/training/`) are skipped — they are not process manifests and
/// are not embedded by build.rs (which uses non-recursive read_dir).
#[test]
fn all_manifests_load_successfully() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/manifests");
    if !dir.exists() {
        eprintln!("{} not found — skipping test", dir.display());
        return;
    }

    let mut errors = Vec::new();
    let mut ok = 0;

    for entry in walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "yaml") {
            let yaml = std::fs::read_to_string(path).unwrap();
            // Skip non-manifest YAML files (e.g. training recipes in
            // manifests/training/ that don't have a `manifest:` key).
            // build.rs only embeds top-level manifests/*.yaml (non-recursive),
            // so training subdirectory files are not embedded at build time.
            if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
                continue;
            }
            match load_manifest_from_yaml(&yaml) {
                Ok(_m) => {
                    ok += 1;
                }
                Err(e) => {
                    errors.push(format!(
                        "{}: {}",
                        path.file_name().unwrap().to_string_lossy(),
                        e
                    ));
                }
            }
        }
    }

    eprintln!("OK: {}, ERR: {}", ok, errors.len());
    for e in &errors {
        eprintln!("  ERR: {}", e);
    }
    assert!(
        errors.is_empty(),
        "{} manifests failed to load:\n{}",
        errors.len(),
        errors.join("\n")
    );
}
