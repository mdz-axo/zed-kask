//! Evaluates the weight-sum invariant for `*-evaluate.j2` templates.
//!
//! Some evaluate templates define weighted scoring dimensions as prose lines
//! like `### C1: Entity Completeness (weight: 0.30)`. If these weights don't
//! sum to 1.0, the convergence metric is mathematically incorrect — the skill
//! will never converge (or always converges) regardless of the actual
//! findings.
//!
//! This test replaces the deleted `kask/scripts/check-convergence-weights.sh`
//! gate, which globbed `registry/templates/*/convergence-check.j2` — a
//! template name that no longer exists (replaced by deterministic Kata
//! primitives in `compute.rs`). The weight-sum invariant survived the
//! migration: it now lives in `*-evaluate.j2` templates. This test enforces
//! it at the new location.
//!
//! The test is a floor, not a ceiling: it only checks templates that use the
//! literal `weight: 0.NN` syntax. Templates using a different weight notation
//! (e.g. MCDA prose "weight each, total = 1.0") are not checked here — those
//! are reviewed manually.

use std::fs;
use std::path::Path;

use regex::Regex;
use walkdir::WalkDir;

/// Tolerance for floating-point weight sums. Matches the deleted shell gate's
/// ±0.02 tolerance.
const WEIGHT_SUM_TOLERANCE: f64 = 0.02;

#[test]
fn evaluate_template_weights_sum_to_one() {
    let templates_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/templates");
    if !templates_dir.exists() {
        eprintln!("{} not found — skipping test", templates_dir.display());
        return;
    }

    // Match `weight: 0.NN` literals (the prose form used in evaluate
    // templates). This deliberately does NOT match scoring contributions
    // like `+0.40` or `(0.40)` — only the dimension-weight declaration.
    let weight_re = Regex::new(r"weight:\s*(0\.\d+)").expect("valid regex");

    let mut checked = 0usize;
    let mut errors = Vec::new();

    for entry in WalkDir::new(&templates_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        // Only evaluate templates carry weighted dimensions.
        if !name.ends_with("-evaluate.j2") {
            continue;
        }

        let contents = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                errors.push(format!("{}: read error: {}", path.display(), e));
                continue;
            }
        };

        let weights: Vec<f64> = weight_re
            .captures_iter(&contents)
            .filter_map(|c| c.get(1).and_then(|m| m.as_str().parse::<f64>().ok()))
            .collect();

        if weights.is_empty() {
            // Not all evaluate templates use weighted dimensions — skip.
            continue;
        }

        checked += 1;
        let sum: f64 = weights.iter().sum();
        if (sum - 1.0).abs() > WEIGHT_SUM_TOLERANCE {
            errors.push(format!(
                "{}: weights {:?} sum to {:.4} (expected ~1.0)",
                path.display(),
                weights,
                sum,
            ));
        }
    }

    if !errors.is_empty() {
        panic!(
            "weight-sum invariant violated in {} template(s):\n{}",
            errors.len(),
            errors.join("\n"),
        );
    }

    assert!(
        checked > 0,
        "no evaluate templates with weights found — the test glob may be stale (the deleted shell gate had this exact failure mode)"
    );
}
