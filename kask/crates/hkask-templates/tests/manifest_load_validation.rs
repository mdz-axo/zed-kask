use hkask_templates::KNOWN_MCP_TOOLS;
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

/// Validate that every `mcp:` reference in every manifest points to a tool
/// that exists in the known MCP tool set. This catches manifest-vs-registry
/// drift (e.g. `mcp: fetch` when no `fetch` tool is registered) at test time,
/// preventing runtime failures that are invisible at manifest-load time.
#[test]
fn all_mcp_references_point_to_known_tools() {
    use hkask_templates::{load_manifest_from_yaml, validate_mcp_references};
    use std::collections::HashSet;

    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/manifests");
    if !dir.exists() {
        eprintln!("{} not found — skipping test", dir.display());
        return;
    }

    let known: HashSet<&str> = KNOWN_MCP_TOOLS.iter().copied().collect();
    let mut warnings_total = Vec::new();

    for entry in walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "yaml") {
            let yaml = std::fs::read_to_string(path).unwrap();
            if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
                continue;
            }
            match load_manifest_from_yaml(&yaml) {
                Ok(manifest) => {
                    let warnings = validate_mcp_references(&manifest, &known);
                    for w in &warnings {
                        eprintln!("WARN: {}", w);
                    }
                    warnings_total.extend(warnings);
                }
                Err(_) => continue,
            }
        }
    }

    assert!(
        warnings_total.is_empty(),
        "{} manifest(s) reference MCP tools not in the known set:\n{}",
        warnings_total.len(),
        warnings_total
            .iter()
            .map(|w| format!("  - {}", w))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The manifest `category` field parses as a closed taxonomy
/// (`ManifestCategory`). An unknown value must be a load error naming the
/// value — previously it was a free-form `Option<String>`, so a typo like
/// `catgory: skill` (or any unrecognized string) silently classified the
/// manifest as infrastructure ("not a skill") with no signal. This test pins
/// the strict parse: the error message must name the offending value.
#[test]
fn unknown_manifest_category_is_a_load_error_naming_the_value() {
    let yaml = r#"
manifest:
  id: typo-category-test
  name: Typo Category Test
  description: A manifest whose category value is misspelled
  version: "1.0.0"
  category: catgory
steps:
  - id: step-1
    ordinal: 1
    action: render
    description: Render something
    template_ref: some/template
"#;
    let err = load_manifest_from_yaml(yaml)
        .expect_err("unknown category value must fail to load, not silently parse");
    let message = format!("{err}");
    assert!(
        message.contains("catgory"),
        "error must name the offending value; got: {message}"
    );
    assert!(
        message.contains("skill"),
        "error must list the valid categories; got: {message}"
    );
}

/// Every documented category value parses to the right variant, and the
/// round trip preserves `is_skill()` semantics: only `skill` (and unset)
/// classify as skills.
#[test]
fn manifest_category_values_parse_to_typed_variants() {
    for (value, expected) in [
        ("skill", hkask_templates::ManifestCategory::Skill),
        ("qa-script", hkask_templates::ManifestCategory::QaScript),
        (
            "runtime-config",
            hkask_templates::ManifestCategory::RuntimeConfig,
        ),
        (
            "daemon-process",
            hkask_templates::ManifestCategory::DaemonProcess,
        ),
        ("pipeline", hkask_templates::ManifestCategory::Pipeline),
        (
            "company-source-manifest",
            hkask_templates::ManifestCategory::CompanySourceManifest,
        ),
    ] {
        let yaml = format!(
            r#"
manifest:
  id: category-roundtrip-{value}
  name: Category Roundtrip
  description: Parses category {value}
  version: "1.0.0"
  category: {value}
steps:
  - id: step-1
    ordinal: 1
    action: render
    description: Render something
    template_ref: some/template
"#
        );
        let manifest =
            load_manifest_from_yaml(&yaml).unwrap_or_else(|e| panic!("'{value}' must parse: {e}"));
        assert_eq!(manifest.category, Some(expected), "for value '{value}'");
        assert_eq!(
            manifest.is_skill(),
            expected == hkask_templates::ManifestCategory::Skill,
            "is_skill must be true only for 'skill' (value '{value}')"
        );
    }

    // Unset category is back-compat skill.
    let yaml = r#"
manifest:
  id: no-category-test
  name: No Category
  description: A manifest with no category
  version: "1.0.0"
steps:
  - id: step-1
    ordinal: 1
    action: render
    description: Render something
    template_ref: some/template
"#;
    let manifest = load_manifest_from_yaml(yaml).expect("unset category must parse");
    assert_eq!(manifest.category, None);
    assert!(manifest.is_skill(), "unset category is back-compat skill");
}
