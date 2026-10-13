//! Provider-profile registry validity — the parse half of the structural
//! contract on `kask/registry/providers/*.yaml`.
//!
//! `kask/scripts/check-provider-profiles.sh` owns the derived-set
//! reconciliation (which vendors the tree adopts, orphan/zombie logic) but
//! only greps for keys — it cannot parse YAML. That gap is how
//! `nebius.yaml` and `runpod.yaml` shipped invalid (a mapping key inside a
//! plain sequence; a colon inside an unquoted scalar) in 9b45cdc723 while
//! the gate stayed green. This test owns what bash cannot do: every
//! profile parses as YAML, and every required key is present with the
//! right shape — typed, not grepped.
//!
//! Set-reconciliation stays in the bash gate (never duplicated here): a
//! missing profile for an adopted vendor, an active orphan, or a zombie
//! retirement is an anchor-derivation question, and the anchors live in
//! four source crates the gate walks. This test iterates whatever profiles
//! exist and holds each one to the document contract.
//!
//! `PROFILES_DIR` overrides the directory (same convention as the gate's
//! override) so the selftest can point the test at synthetic trees.

// The contract structs below are deserialization-only: their fields are
// read by serde while parsing, and requiring each one at parse time IS
// the check — they are intentionally never read by name in the
// assertions.
#![allow(dead_code)]

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Debug)]
struct Keychain {
    slot: String,
    env_var: String,
}

#[derive(Deserialize, Debug)]
struct ConsumedCapability {
    capability: String,
    source: String,
}

#[derive(Deserialize, Debug)]
struct NonConsumption {
    capability: String,
    reason: String,
}

#[derive(Deserialize, Debug)]
struct DataTerms {
    /// `null` while the first audit is pending; a date string once read.
    extracted_at: Option<String>,
    trains_on_customer_data: String,
    retention_published: String,
    /// Present only where the operator has ruled on the posture.
    operator_ruling: Option<String>,
}

#[derive(Deserialize, Debug)]
struct StatusSurface {
    status_page: String,
    sla: String,
}

#[derive(Deserialize, Debug)]
struct Pricing {
    machine_readable: String,
}

#[derive(Deserialize, Debug)]
struct ProviderProfile {
    id: String,
    name: String,
    status: String,
    roles: Vec<String>,
    keychain: Keychain,
    consumed_capabilities: Vec<ConsumedCapability>,
    deliberate_non_consumption: Vec<NonConsumption>,
    data_terms: DataTerms,
    status_surface: StatusSurface,
    pricing: Pricing,
}

fn profiles_dir() -> PathBuf {
    std::env::var_os("PROFILES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry/providers"))
}

#[test]
fn every_provider_profile_parses_with_the_document_contract() {
    let dir = profiles_dir();
    let entries =
        std::fs::read_dir(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    let mut checked = 0usize;
    for entry in entries {
        let entry = entry.expect("read_dir entry");
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("yaml") {
            continue;
        }
        let stem = path
            .file_stem()
            .expect("yaml file stem")
            .to_string_lossy()
            .into_owned();
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let profile: ProviderProfile = serde_yaml_neo::from_str(&text).unwrap_or_else(|error| {
            panic!(
                "{} is not valid YAML carrying the profile contract: {error}",
                path.display()
            )
        });
        assert_eq!(
            profile.id,
            stem,
            "id must match the filename stem ({})",
            path.display()
        );
        assert!(
            matches!(profile.status.as_str(), "active" | "retired"),
            "{}: status must be 'active' or 'retired', not {:?}",
            path.display(),
            profile.status
        );
        assert!(
            !profile.name.trim().is_empty(),
            "{}: name is empty",
            path.display()
        );
        assert!(
            !profile.roles.is_empty(),
            "{}: roles is empty",
            path.display()
        );
        checked += 1;
    }
    assert!(checked > 0, "no profiles found in {}", dir.display());
}
