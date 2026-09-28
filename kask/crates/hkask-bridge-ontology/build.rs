//! Compile the vendored full-ontology sources into the embedded index.
//!
//! Every file under `sources/` must be pinned in `sources/SOURCES.lock` with
//! its upstream URL, version, sha256 and license; the build fails on any
//! drift, unlisted file or missing file. The index carries vocabulary only
//! (terms, labels, direct parents, published definitions, source file).

#[path = "src/published_sources.rs"]
mod published_sources;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

struct Pin {
    path: String,
    version: String,
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn verify_lock(sources: &Path) -> Result<Vec<Pin>, String> {
    let lock = read(&sources.join("SOURCES.lock"))?;
    let mut pins = Vec::new();
    for line in lock
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
    {
        let fields: Vec<&str> = line.split('\t').collect();
        let [path, _url, version, sha256, license] = fields.as_slice() else {
            return Err(format!(
                "SOURCES.lock: expected 5 tab-separated fields: {line}"
            ));
        };
        if license.trim().is_empty() {
            return Err(format!("SOURCES.lock: {path} has no license"));
        }
        let bytes =
            std::fs::read(sources.join(path)).map_err(|error| format!("{path}: {error}"))?;
        let actual: String = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if actual != *sha256 {
            return Err(format!(
                "{path}: sha256 {actual} does not match the pinned {sha256} — \
                 re-vendor from the pinned upstream version, never edit a source by hand"
            ));
        }
        pins.push(Pin {
            path: (*path).to_string(),
            version: (*version).to_string(),
        });
    }

    let pinned: BTreeSet<&str> = pins.iter().map(|pin| pin.path.as_str()).collect();
    for directory in ["sumo", "schema-org"] {
        let entries = std::fs::read_dir(sources.join(directory))
            .map_err(|error| format!("sources/{directory}: {error}"))?;
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == "README.txt" {
                continue;
            }
            let relative = format!("{directory}/{name}");
            if !pinned.contains(relative.as_str()) {
                return Err(format!("sources/{relative} is not pinned in SOURCES.lock"));
            }
        }
    }
    Ok(pins)
}

fn build() -> Result<(), String> {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").map_err(|e| e.to_string())?);
    let sources = manifest.join("sources");
    println!("cargo:rerun-if-changed=sources");
    println!("cargo:rerun-if-changed=src/published_sources.rs");

    let pins = verify_lock(&sources)?;

    // SUMO: Merge.kif and the mid-level ontology first — their English
    // documentation is the definition of record; domain files follow.
    let mut sumo: Vec<&Pin> = pins
        .iter()
        .filter(|pin| pin.path.starts_with("sumo/"))
        .collect();
    let rank = |path: &str| match path {
        "sumo/Merge.kif" => 0,
        "sumo/Mid-level-ontology.kif" => 1,
        _ => 2,
    };
    sumo.sort_by(|a, b| (rank(&a.path), &a.path).cmp(&(rank(&b.path), &b.path)));
    let sumo_version = sumo
        .first()
        .map(|pin| pin.version.clone())
        .ok_or("SOURCES.lock pins no SUMO files")?;
    let mut sumo_files = Vec::new();
    for pin in &sumo {
        let name = pin.path.trim_start_matches("sumo/").to_string();
        sumo_files.push((name, read(&sources.join(&pin.path))?));
    }
    let mut terms = published_sources::index_sumo(&sumo_files, &sumo_version)?;

    let schema_version = pins
        .iter()
        .find(|pin| pin.path.starts_with("schema-org/"))
        .map(|pin| pin.version.clone())
        .ok_or("SOURCES.lock pins no schema.org files")?;
    terms.extend(published_sources::index_schema_org(
        &read(&sources.join("schema-org/schemaorg-all-https-types.csv"))?,
        &read(&sources.join("schema-org/schemaorg-all-https-properties.csv"))?,
        &schema_version,
    )?);

    let mut index = String::new();
    for term in &terms {
        index.push_str(&term.to_line());
        index.push('\n');
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").map_err(|e| e.to_string())?);
    std::fs::write(out.join("published_index.tsv"), index).map_err(|error| error.to_string())?;
    Ok(())
}

fn main() {
    if let Err(error) = build() {
        panic!("hkask-bridge-ontology source index: {error}");
    }
}
