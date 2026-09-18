//! Validate the governance inventory, not the truth of its assertions.
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    principles: Vec<Principle>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Principle {
    id: String,
    name: String,
    description: String,
    date: String,
    source: String,
    constraint_set: Vec<Constraint>,
}

#[derive(Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Status {
    DocumentedProcess,
    RuntimeEnforced,
    CiEnforced,
    VerifiedAtRevision,
    Gap,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Constraint {
    id: String,
    assertion: String,
    status: Status,
    scope: String,
    owner: String,
    enforced_at: PathBuf,
    check: Option<Check>,
    evidence: Option<Evidence>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Check {
    path: PathBuf,
    symbol: Option<String>,
    command: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    revision: String,
    artifact: PathBuf,
    checker: String,
    assumptions: String,
}

fn present(text: &str) -> Result<()> {
    ensure!(
        !text.trim().is_empty() && !text.starts_with("MISSING:"),
        "missing required value"
    );
    Ok(())
}

fn source_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    ensure!(
        !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
        "expected a repository-relative file: {}",
        relative.display()
    );
    let resolved = root
        .join(relative)
        .canonicalize()
        .with_context(|| format!("missing source {}", relative.display()))?;
    ensure!(
        resolved.starts_with(root) && resolved.is_file(),
        "source escapes repository or is not a file: {}",
        relative.display()
    );
    Ok(resolved)
}

fn validate(root: &Path, text: &str) -> Result<Vec<String>> {
    let root = root.canonicalize()?;
    let registry: Registry =
        serde_yaml_neo::from_str(text).context("invalid constraint registry")?;
    ensure!(
        !registry.principles.is_empty(),
        "required inventory has no principles"
    );
    let mut ids = HashSet::new();
    let mut reports = Vec::new();
    for principle in registry.principles {
        for field in [
            &principle.id,
            &principle.name,
            &principle.description,
            &principle.date,
            &principle.source,
        ] {
            present(field)?;
        }
        ensure!(
            ids.insert(principle.id.clone()),
            "duplicate id {}",
            principle.id
        );
        ensure!(
            !principle.constraint_set.is_empty(),
            "{} has no constraints",
            principle.id
        );
        for constraint in principle.constraint_set {
            for field in [
                &constraint.id,
                &constraint.assertion,
                &constraint.scope,
                &constraint.owner,
            ] {
                present(field)?;
            }
            ensure!(
                ids.insert(constraint.id.clone()),
                "duplicate id {}",
                constraint.id
            );
            source_path(&root, &constraint.enforced_at)?;
            if matches!(
                constraint.status,
                Status::RuntimeEnforced | Status::CiEnforced | Status::VerifiedAtRevision
            ) {
                ensure!(
                    constraint.check.is_some(),
                    "{} claims enforcement/verification without a check",
                    constraint.id
                );
            }
            if let Some(check) = constraint.check {
                present(&check.command)?;
                let path = source_path(&root, &check.path)?;
                if let Some(symbol) = check.symbol {
                    present(&symbol)?;
                    let source = std::fs::read_to_string(path)?;
                    ensure!(
                        source.contains(&format!("fn {symbol}(")),
                        "{} check function not found: {symbol}",
                        constraint.id
                    );
                }
            }
            if constraint.status == Status::CiEnforced {
                ensure!(
                    constraint.enforced_at.starts_with(".github/workflows"),
                    "{} CI claim must name a workflow",
                    constraint.id
                );
            }
            if constraint.status == Status::VerifiedAtRevision {
                let evidence = constraint
                    .evidence
                    .context("verified_at_revision requires evidence")?;
                ensure!(
                    evidence.revision.len() == 40
                        && evidence.revision.bytes().all(|b| b.is_ascii_hexdigit()),
                    "evidence requires a full commit id"
                );
                present(&evidence.checker)?;
                present(&evidence.assumptions)?;
                source_path(&root, &evidence.artifact)?;
            } else {
                ensure!(
                    constraint.evidence.is_none(),
                    "revision evidence requires verified_at_revision status"
                );
            }
            reports.push(format!(
                "{}: {:?}; scope={}; owner={} (inventory only, checks not executed)",
                constraint.id, constraint.status, constraint.scope, constraint.owner
            ));
        }
    }
    Ok(reports)
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let Some(root) = args.next() else {
        bail!("usage: check_principle_constraints ROOT REGISTRY");
    };
    let Some(registry) = args.next() else {
        bail!("usage: check_principle_constraints ROOT REGISTRY");
    };
    ensure!(args.next().is_none(), "unexpected arguments");
    let text = std::fs::read_to_string(&registry).context("cannot read required registry")?;
    for report in validate(Path::new(&root), &text)? {
        println!("{report}");
    }
    println!(
        "Inventory valid; no runtime tests, proofs, commands, or approval evidence executed or authenticated."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn fixture(status: &str, extra: &str) -> String {
        format!(
            "principles:\n  - id: p\n    name: n\n    description: d\n    date: '2026-09-18'\n    source: s\n    constraint_set:\n      - id: c\n        assertion: a\n        status: {status}\n        scope: test\n        owner: test\n        enforced_at: crates/hkask-regulation/Cargo.toml\n{extra}"
        )
    }

    /// expect: "Invalid or empty governance inventories never become a green empty check" [P8]
    #[test]
    fn rejects_empty_malformed_and_unknown_inventory() {
        for text in [
            "",
            "[",
            "principles: []",
            "principles: []\nunexpected: true",
        ] {
            assert!(validate(&root(), text).is_err(), "{text}");
        }
        assert!(validate(&root(), &fixture("enforced", "")).is_err());
        assert!(validate(&root(), &fixture("runtime_enforced", "")).is_err());
        assert!(validate(&root(), &fixture("verified_at_revision", "")).is_err());
    }

    /// expect: "Process and gap reports are explicit, never claims of verification" [P8]
    #[test]
    fn reports_process_and_gaps_without_executing_commands() -> Result<()> {
        for status in ["documented_process", "gap"] {
            let reports = validate(&root(), &fixture(status, ""))?;
            assert_eq!(reports.len(), 1);
            assert!(reports[0].contains("inventory only, checks not executed"));
        }
        Ok(())
    }

    /// expect: "Inventory references cannot escape the repository or silently disappear" [P4]
    #[test]
    fn rejects_missing_paths_and_duplicate_ids() {
        for replacement in ["../Cargo.toml", "/etc/passwd", "missing.rs"] {
            assert!(
                validate(
                    &root(),
                    &fixture("gap", "").replace("crates/hkask-regulation/Cargo.toml", replacement)
                )
                .is_err()
            );
        }
        assert!(validate(&root(), &fixture("gap", "").replace("id: c", "id: p")).is_err());
    }

    /// expect: "Enforcement claims require inspectable checks, never MISSING placeholders" [P8]
    #[test]
    fn enforcement_requires_real_check_references() -> Result<()> {
        let check = "        check:\n          path: crates/hkask-regulation/src/bin/check_principle_constraints.rs\n          symbol: enforcement_requires_real_check_references\n          command: cargo test -p hkask-regulation --bin check_principle_constraints\n";
        assert_eq!(
            validate(&root(), &fixture("runtime_enforced", check))?.len(),
            1
        );
        for broken in [
            check.replace(
                "enforcement_requires_real_check_references",
                "nonexistent_check",
            ),
            check.replace(
                "command: cargo test -p hkask-regulation --bin check_principle_constraints",
                "command: 'MISSING: check'",
            ),
        ] {
            assert!(validate(&root(), &fixture("runtime_enforced", &broken)).is_err());
        }
        assert!(validate(&root(), &fixture("ci_enforced", check)).is_err());
        assert!(validate(&root(), &fixture("verified_at_revision", check)).is_err());
        let evidence = format!(
            "{check}        evidence:\n          revision: '1111111111111111111111111111111111111111'\n          artifact: crates/hkask-regulation/Cargo.toml\n          checker: fixture-checker\n          assumptions: fixture-only\n"
        );
        assert_eq!(
            validate(&root(), &fixture("verified_at_revision", &evidence))?.len(),
            1
        );
        assert!(
            validate(
                &root(),
                &fixture(
                    "verified_at_revision",
                    &evidence.replace("1111111111111111111111111111111111111111", "short")
                )
            )
            .is_err()
        );
        assert!(validate(&root(), &fixture("documented_process", &evidence)).is_err());
        Ok(())
    }
}
