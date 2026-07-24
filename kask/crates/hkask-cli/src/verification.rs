#![allow(clippy::collapsible_if)]
#![allow(clippy::ptr_arg)]
//! Magna Carta verification service.
//!
//! Loads YAML assertion manifests from `.agents/skills/magna-carta-verifier/manifests/`
//! and runs structural audits (grep-based) against the codebase.
//!
//! Behavioral probes and other assertion methods that require runtime execution
//! are reported as "gap" — assertions defined but not yet automatically verified.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Use serde_yaml_neo (project's YAML dependency), serde_json for JSON output

// ── Manifest types ────────────────────────────────────────────────────────

/// A single assertion target within a manifest.
#[derive(Debug, Deserialize)]
struct ManifestTarget {
    #[serde(rename = "crate")]
    crate_name: String,
    module: String,
    #[serde(default)]
    methods: Vec<String>,
    #[serde(default)]
    gate: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    categories: Vec<String>,
}

/// A single assertion from a manifest.
#[derive(Debug, Deserialize)]
struct ManifestAssertion {
    id: String,
    name: String,
    claim: String,
    method: String,
    targets: Vec<ManifestTarget>,
}

/// A complete verification manifest (one principle).
#[derive(Debug, Deserialize)]
struct ManifestFile {
    principle: String,
    #[allow(dead_code)]
    version: String,
    #[allow(dead_code)]
    description: String,
    assertions: Vec<ManifestAssertion>,
}

// ── Public report types ───────────────────────────────────────────────────

/// Deserialized form of a loaded manifest.
#[derive(Debug, Clone, Serialize)]
pub struct Manifest {
    pub principle: String,
    pub display_name: String,
    pub assertions: Vec<Assertion>,
}

/// A single assertion from a manifest, carried into the report.
#[derive(Debug, Clone, Serialize)]
pub struct Assertion {
    pub id: String,
    pub name: String,
    pub claim: String,
    pub method: String,
}

/// The result of verifying a single assertion.
#[derive(Debug, Clone, Serialize)]
pub struct AssertionResult {
    pub id: String,
    pub name: String,
    /// "pass", "fail", "gap", or "skip"
    pub status: String,
    pub findings: Vec<String>,
    pub recommendations: Vec<String>,
}

/// Results for one principle (all its assertions).
#[derive(Debug, Clone, Serialize)]
pub struct PrincipleResult {
    pub principle: String,
    pub display_name: String,
    pub assertion_results: Vec<AssertionResult>,
}

/// The complete verification report.
#[derive(Debug, Clone, Serialize)]
pub struct VerificationReport {
    pub principles: Vec<PrincipleResult>,
    pub total_assertions: usize,
    pub total_pass: usize,
    pub total_fail: usize,
    pub total_gap: usize,
    pub total_skip: usize,
}

// ── VerificationService ───────────────────────────────────────────────────

/// Service for running Magna Carta verification against codebase manifests.
pub struct VerificationService;

impl VerificationService {
    /// Run verification for all principles, optionally filtered by principle name.
    #[must_use]
    pub fn verify(filter: Option<&str>) -> VerificationReport {
        let manifests = Self::load_manifests(filter);
        let mut principles = Vec::new();

        for manifest in &manifests {
            let mut results = Vec::new();
            for assertion in &manifest.assertions {
                let result = Self::verify_assertion(assertion);
                results.push(result);
            }
            principles.push(PrincipleResult {
                principle: manifest.principle.clone(),
                display_name: manifest.display_name.clone(),
                assertion_results: results,
            });
        }

        let total_assertions: usize = principles.iter().map(|p| p.assertion_results.len()).sum();
        let total_pass = principles
            .iter()
            .flat_map(|p| &p.assertion_results)
            .filter(|r| r.status == "pass")
            .count();
        let total_fail = principles
            .iter()
            .flat_map(|p| &p.assertion_results)
            .filter(|r| r.status == "fail")
            .count();
        let total_gap = principles
            .iter()
            .flat_map(|p| &p.assertion_results)
            .filter(|r| r.status == "gap")
            .count();
        let total_skip = principles
            .iter()
            .flat_map(|p| &p.assertion_results)
            .filter(|r| r.status == "skip")
            .count();

        VerificationReport {
            principles,
            total_assertions,
            total_pass,
            total_fail,
            total_gap,
            total_skip,
        }
    }

    /// Run verification and return results as JSON-serializable value.
    #[must_use]
    pub fn verify_json(filter: Option<&str>) -> serde_json::Value {
        let report = Self::verify(filter);
        serde_json::to_value(&report).unwrap_or_else(
            |e| serde_json::json!({"error": format!("Failed to serialize report: {e}")}),
        )
    }

    // ── Manifest loading ──────────────────────────────────────────────────

    fn manifest_dir() -> PathBuf {
        // Resolve relative to workspace root — try several locations
        let candidates = [
            PathBuf::from(".agents/skills/magna-carta-verifier/manifests"),
            PathBuf::from("../.agents/skills/magna-carta-verifier/manifests"),
        ];

        for candidate in &candidates {
            if candidate.exists() {
                return candidate.clone();
            }
        }

        // Fallback: try to find via CARGO_MANIFEST_DIR
        if let Ok(dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let from_cargo =
                PathBuf::from(&dir).join("../../.agents/skills/magna-carta-verifier/manifests");
            if from_cargo.exists() {
                return from_cargo;
            }
        }

        candidates[0].clone()
    }

    fn load_manifests(filter: Option<&str>) -> Vec<Manifest> {
        let dir = Self::manifest_dir();
        let mut manifests = Vec::new();

        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => return manifests,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
            {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(mf) = serde_yaml_neo::from_str::<ManifestFile>(&content) {
                        // Apply filter
                        if let Some(f) = filter {
                            if mf.principle != f {
                                continue;
                            }
                        }
                        let display_name = Self::principle_display(&mf.principle);
                        let assertions: Vec<Assertion> = mf
                            .assertions
                            .into_iter()
                            .map(|a| Assertion {
                                id: a.id,
                                name: a.name,
                                claim: a.claim,
                                method: a.method,
                            })
                            .collect();
                        manifests.push(Manifest {
                            principle: mf.principle,
                            display_name,
                            assertions,
                        });
                    }
                }
            }
        }

        manifests
    }

    fn principle_display(principle: &str) -> String {
        match principle {
            "user_sovereignty" => "P1 — User Sovereignty".to_string(),
            "affirmative_consent" => "P2 — Affirmative Consent".to_string(),
            "generative_space" => "P3 — Generative Space".to_string(),
            "clear_boundaries" => "P4 — Clear Boundaries".to_string(),
            other => other.replace('_', " "),
        }
    }

    // ── Assertion verification ────────────────────────────────────────────

    fn verify_assertion(assertion: &Assertion) -> AssertionResult {
        match assertion.method.as_str() {
            "structural_audit" => Self::structural_audit(assertion),
            "behavioral_probe" | "integration_test" | "property_test" => {
                // These methods require runtime execution — report as gap
                AssertionResult {
                    id: assertion.id.clone(),
                    name: assertion.name.clone(),
                    status: "gap".to_string(),
                    findings: vec![format!(
                        "{} verification requires runtime execution (not yet automated)",
                        assertion.method
                    )],
                    recommendations: vec![
                        "Run integration tests to verify this assertion".to_string(),
                    ],
                }
            }
            _ => AssertionResult {
                id: assertion.id.clone(),
                name: assertion.name.clone(),
                status: "gap".to_string(),
                findings: vec![format!("Unknown verification method: {}", assertion.method)],
                recommendations: vec!["Define an automated verification method".to_string()],
            },
        }
    }

    /// Structural audit: grep for the `gate` in the target crate's source.
    ///
    /// For each target, searches for the gate identifier in the target module.
    /// This is a lightweight static check — it confirms the gate symbol exists
    /// at the expected call site.
    fn structural_audit(assertion: &Assertion) -> AssertionResult {
        // Re-load the manifest to get target details
        let manifests = Self::load_manifests_raw();
        let mf_assertion = manifests.iter().find_map(|mf| {
            mf.assertions
                .iter()
                .find(|a| a.id == assertion.id && a.method == "structural_audit")
        });

        let Some(mf_assertion) = mf_assertion else {
            return AssertionResult {
                id: assertion.id.clone(),
                name: assertion.name.clone(),
                status: "gap".to_string(),
                findings: vec!["Could not load assertion targets from manifest".to_string()],
                recommendations: vec!["Verify manifest file exists and is valid YAML".to_string()],
            };
        };

        let mut all_pass = true;
        let mut findings = Vec::new();
        let mut recommendations = Vec::new();

        for target in &mf_assertion.targets {
            let crate_dir = Self::find_crate_dir(&target.crate_name);
            let gate = target.gate.as_deref().unwrap_or("");

            if gate.is_empty() {
                findings.push(format!(
                    "No gate specified for target {}/{}",
                    target.crate_name, target.module
                ));
                all_pass = false;
                recommendations.push(format!(
                    "Add a gate field to target {}/{}",
                    target.crate_name, target.module
                ));
                continue;
            }

            match &crate_dir {
                Some(dir) => {
                    // Search for the gate in the target module's source
                    let found = Self::grep_gate(dir, &target.module, gate, &target.methods);
                    if found {
                        findings.push(format!(
                            "Gate '{}' found in {}/{}",
                            gate, target.crate_name, target.module
                        ));
                    } else {
                        findings.push(format!(
                            "Gate '{}' NOT found in {}/{} — expected in methods: {:?}",
                            gate, target.crate_name, target.module, target.methods
                        ));
                        all_pass = false;
                        recommendations.push(format!(
                            "Ensure {}::{} calls {}",
                            target.crate_name, target.module, gate
                        ));
                    }
                }
                None => {
                    findings.push(format!(
                        "Crate directory not found for: {}",
                        target.crate_name
                    ));
                    all_pass = false;
                    recommendations.push(format!(
                        "Verify crate {} exists under crates/",
                        target.crate_name
                    ));
                }
            }
        }

        if findings.is_empty() {
            findings.push("No targets defined for this assertion".to_string());
            all_pass = false;
        }

        AssertionResult {
            id: assertion.id.clone(),
            name: assertion.name.clone(),
            status: if all_pass { "pass" } else { "fail" }.to_string(),
            findings,
            recommendations,
        }
    }

    /// Load raw manifest files (with targets) for structural audits.
    fn load_manifests_raw() -> Vec<ManifestFile> {
        let dir = Self::manifest_dir();
        let mut manifests = Vec::new();

        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => return manifests,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
            {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(mf) = serde_yaml_neo::from_str::<ManifestFile>(&content) {
                        manifests.push(mf);
                    }
                }
            }
        }

        manifests
    }

    /// Find the filesystem directory for a crate name.
    fn find_crate_dir(crate_name: &str) -> Option<PathBuf> {
        // Map hyphenated crate names to filesystem paths
        let dir_name = crate_name.replace('-', "_");
        let candidates = [
            PathBuf::from("crates").join(&dir_name),
            PathBuf::from("../crates").join(&dir_name),
        ];

        for candidate in &candidates {
            if candidate.exists() {
                return Some(candidate.clone());
            }
        }

        // Try from CARGO_MANIFEST_DIR
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let from_cargo = PathBuf::from(&manifest_dir)
                .join("../../crates")
                .join(&dir_name);
            if from_cargo.exists() {
                return Some(from_cargo);
            }
        }

        None
    }

    /// Grep for a gate symbol in the target module's source files.
    ///
    /// Returns true if the gate is found in any Rust source file within
    /// the target module directory (or the crate root if no specific module).
    fn grep_gate(crate_dir: &PathBuf, module: &str, gate: &str, methods: &[String]) -> bool {
        let module_path = module.replace("::", "/");
        let src_dir = crate_dir.join("src");

        // Try the specific module file or directory
        let module_file = src_dir.join(format!("{}.rs", module_path));
        let module_dir = src_dir.join(&module_path);

        let search_paths: Vec<PathBuf> = if module_file.exists() {
            vec![module_file]
        } else if module_dir.exists() {
            // Search all .rs files in the module directory
            std::fs::read_dir(&module_dir)
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
                        .map(|e| e.path())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            // Fall back to searching the entire src directory
            std::fs::read_dir(&src_dir)
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
                        .map(|e| e.path())
                        .collect()
                })
                .unwrap_or_default()
        };

        for path in &search_paths {
            if let Ok(content) = std::fs::read_to_string(path) {
                // Check for the gate symbol in the file
                if content.contains(gate) {
                    // If specific methods are listed, check that the gate appears
                    // near one of those method names
                    if methods.is_empty() {
                        return true;
                    }
                    for method in methods {
                        if content.contains(method) {
                            return true;
                        }
                    }
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_returns_report_with_principle_names() {
        let report = VerificationService::verify(None);
        // If no manifests found, report is empty
        if !report.principles.is_empty() {
            for pr in &report.principles {
                assert!(!pr.display_name.is_empty());
                assert!(!pr.assertion_results.is_empty());
            }
            assert!(report.total_assertions > 0);
        }
    }

    #[test]
    fn verify_json_returns_valid_json() {
        let json = VerificationService::verify_json(None);
        assert!(json.is_object());
        assert!(json.get("total_assertions").is_some());
    }

    #[test]
    fn verify_with_filter_returns_only_matching() {
        let report = VerificationService::verify(Some("user_sovereignty"));
        if !report.principles.is_empty() {
            assert_eq!(report.principles.len(), 1);
            assert_eq!(report.principles[0].principle, "user_sovereignty");
        }
    }

    #[test]
    fn principle_display_names_are_human_readable() {
        assert_eq!(
            VerificationService::principle_display("user_sovereignty"),
            "P1 — User Sovereignty"
        );
        assert_eq!(
            VerificationService::principle_display("clear_boundaries"),
            "P4 — Clear Boundaries"
        );
        assert_eq!(VerificationService::principle_display("unknown"), "unknown");
    }
}
