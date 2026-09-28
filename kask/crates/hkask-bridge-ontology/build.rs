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
    url: String,
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
        let [path, url, version, sha256, license] = fields.as_slice() else {
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
            url: (*url).to_string(),
            version: (*version).to_string(),
        });
    }

    let pinned: BTreeSet<&str> = pins.iter().map(|pin| pin.path.as_str()).collect();
    if pinned.len() != pins.len() {
        return Err("SOURCES.lock contains duplicate paths".to_string());
    }
    let mut pending = vec![sources.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.file_name().and_then(|name| name.to_str()) != Some("SOURCES.lock") {
                let relative = path
                    .strip_prefix(sources)
                    .map_err(|error| error.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/");
                if relative.ends_with("README.txt") {
                    continue;
                }
                if !pinned.contains(relative.as_str()) {
                    return Err(format!("sources/{relative} is not pinned in SOURCES.lock"));
                }
            }
        }
    }
    Ok(pins)
}

/// Parse one pinned RDF file into the reader-neutral triple form.
fn read_rdf(sources: &Path, pin: &Pin) -> Result<Vec<published_sources::RdfTriple>, String> {
    use oxrdf::{NamedOrBlankNode, Term};
    use oxrdfio::{RdfFormat, RdfParser};
    use published_sources::{RdfObject, RdfTriple};

    let format = match Path::new(&pin.path)
        .extension()
        .and_then(|ext| ext.to_str())
    {
        Some("ttl") => RdfFormat::Turtle,
        Some("owl" | "rdf" | "xml") => RdfFormat::RdfXml,
        other => return Err(format!("{}: unsupported RDF extension {other:?}", pin.path)),
    };
    let bytes =
        std::fs::read(sources.join(&pin.path)).map_err(|error| format!("{}: {error}", pin.path))?;
    let parser = RdfParser::from_format(format)
        .with_base_iri(pin.url.as_str())
        .map_err(|error| format!("{}: base IRI {}: {error}", pin.path, pin.url))?;
    let mut triples = Vec::new();
    for quad in parser.for_slice(&bytes) {
        let quad = quad.map_err(|error| format!("{}: {error}", pin.path))?;
        let subject = match quad.subject {
            NamedOrBlankNode::NamedNode(node) => Some(node.into_string()),
            NamedOrBlankNode::BlankNode(_) => None,
        };
        let object = match quad.object {
            Term::NamedNode(node) => RdfObject::Iri(node.into_string()),
            Term::Literal(literal) => RdfObject::Literal {
                value: literal.value().to_string(),
                language: literal.language().map(str::to_string),
            },
            _ => RdfObject::Blank,
        };
        triples.push(RdfTriple {
            subject,
            predicate: quad.predicate.into_string(),
            object,
        });
    }
    Ok(triples)
}

/// The fixture is an independent inventory of the tag's Release ontology
/// declarations. Checking it against both the pinned bytes and the lock
/// prevents a missing module from looking like a successful small index.
fn index_fibo(
    manifest: &Path,
    sources: &Path,
    pins: &[Pin],
) -> Result<Vec<published_sources::IndexedTerm>, String> {
    use published_sources::{RdfObject, RdfVocabulary};
    const BASE: &str = "https://spec.edmcouncil.org/fibo/ontology/";
    const MATURITY: &str = "https://spec.edmcouncil.org/fibo/ontology/FND/Utilities/AnnotationVocabulary/hasMaturityLevel";
    const RELEASE: &str =
        "https://spec.edmcouncil.org/fibo/ontology/FND/Utilities/AnnotationVocabulary/Release";
    let fixture = read(&manifest.join("fixtures/fibo-verified-terms.txt"))?;
    let mut modules = Vec::new();
    for line in fixture.lines().filter(|line| line.starts_with("module\t")) {
        let fields: Vec<_> = line.split('\t').collect();
        let [_, path, prefix, iri] = fields.as_slice() else {
            return Err(format!("invalid FIBO module row: {line}"));
        };
        if !iri.starts_with(BASE) || prefix.is_empty() || path.is_empty() {
            return Err(format!("invalid FIBO binding: {line}"));
        }
        modules.push((*path, *prefix, *iri));
    }
    if modules.len() != 157 {
        return Err(format!(
            "Q2 Release manifest expected 157 modules, got {}",
            modules.len()
        ));
    }
    let selected: BTreeSet<String> = modules
        .iter()
        .map(|(path, _, _)| format!("fibo/{path}"))
        .collect();
    if selected.len() != modules.len() {
        return Err("duplicate FIBO Release module".to_string());
    }
    let pinned: BTreeSet<String> = pins
        .iter()
        .filter(|pin| {
            pin.path.starts_with("fibo/")
                && pin.path.ends_with(".rdf")
                && pin.path != "fibo/AboutFIBOProd.rdf"
        })
        .map(|pin| pin.path.clone())
        .collect();
    if selected != pinned {
        return Err(format!(
            "FIBO Release manifest / lock mismatch: missing {:?}, extra {:?}",
            selected.difference(&pinned).collect::<Vec<_>>(),
            pinned.difference(&selected).collect::<Vec<_>>()
        ));
    }
    for path in ["fibo/LICENSE", "fibo/AboutFIBOProd.rdf"] {
        if !pins.iter().any(|pin| pin.path == path) {
            return Err(format!("{path} must be pinned"));
        }
    }
    let license = read(&sources.join("fibo/LICENSE"))?;
    if !license.starts_with("The MIT License (MIT)") {
        return Err("FIBO license is not the pinned MIT license".to_string());
    }
    let mut prefixes = BTreeSet::new();
    let mut iris = BTreeSet::new();
    for (_, prefix, iri) in &modules {
        if !prefixes.insert(*prefix) || !iris.insert(*iri) {
            return Err(format!(
                "duplicate or ambiguous FIBO namespace binding: {prefix} {iri}"
            ));
        }
    }
    let prefix_bindings: Vec<(&str, &str)> = modules
        .iter()
        .map(|(_, prefix, iri)| (*prefix, *iri))
        .collect();
    let version = pins
        .iter()
        .find(|pin| pin.path == "fibo/AboutFIBOProd.rdf")
        .ok_or("FIBO production manifest missing")?
        .version
        .as_str();
    let production = read_rdf(
        sources,
        pins.iter()
            .find(|pin| pin.path == "fibo/AboutFIBOProd.rdf")
            .ok_or("FIBO production manifest missing")?,
    )?;
    let imports: BTreeSet<String> = production
        .iter()
        .filter(|triple| triple.predicate == "http://www.w3.org/2002/07/owl#imports")
        .filter_map(|triple| match &triple.object {
            RdfObject::Iri(iri) if iri.starts_with(BASE) => Some(iri.clone()),
            _ => None,
        })
        .collect();
    let release_iris: BTreeSet<&str> = modules.iter().map(|(_, _, iri)| *iri).collect();
    let imported_iris: BTreeSet<&str> = imports.iter().map(String::as_str).collect();
    // The tag's AboutFIBOProd import list omits five Release modules and
    // includes MarketsIndividuals (not Release). This check records that
    // divergence rather than silently changing the maturity policy.
    if imports.len() != 153
        || release_iris.difference(&imported_iris).count() != 5
        || imported_iris.difference(&release_iris).count() != 1
        || !imported_iris.difference(&release_iris).all(|iri|
            *iri == "https://spec.edmcouncil.org/fibo/ontology/FBC/FunctionalEntities/MarketsIndividuals/")
    {
        return Err(
            "FIBO Release maturity set diverges unexpectedly from AboutFIBOProd imports"
                .to_string(),
        );
    }
    let mut terms = Vec::new();
    let mut concepts = BTreeSet::new();
    for (path, prefix, iri) in modules {
        let pin_path = format!("fibo/{path}");
        let pin = pins
            .iter()
            .find(|pin| pin.path == pin_path)
            .ok_or_else(|| format!("missing {pin_path}"))?;
        let expected_url = format!(
            "https://raw.githubusercontent.com/EDMCouncil/FIBO/f59157fe156e3d91b1c045222d0a7dc06b7d78a2/{path}"
        );
        if pin.url != expected_url || pin.version != version {
            return Err(format!("{path}: FIBO source must be pinned to the Q2 tag"));
        }
        let xml = read(&sources.join(&pin.path))?;
        if !xml.contains(&format!("xmlns:{prefix}=\"{iri}\"")) {
            return Err(format!(
                "{path}: actual XML namespace is not {prefix} = {iri}"
            ));
        }
        let triples = read_rdf(sources, pin)?;
        let is_release = triples.iter().any(|triple| {
            triple.subject.as_deref() == Some(iri)
                && triple.predicate == MATURITY
                && triple.object == RdfObject::Iri(RELEASE.to_string())
        }) && triples.iter().any(|triple| {
            triple.subject.as_deref() == Some(iri)
                && triple.predicate == "http://www.w3.org/1999/02/22-rdf-syntax-ns#type"
                && triple.object
                    == RdfObject::Iri("http://www.w3.org/2002/07/owl#Ontology".to_string())
        });
        if !is_release {
            return Err(format!(
                "{path}: ontology does not declare Release maturity"
            ));
        }
        let vocabulary = RdfVocabulary {
            namespace: "FIBO",
            prefix,
            iri,
            directory: "fibo",
        };
        let indexed = published_sources::index_rdf_with_prefixes(
            &vocabulary,
            &[(path.to_string(), triples)],
            version,
            &prefix_bindings,
        );
        if indexed.is_empty() {
            return Err(format!("{path}: Release module indexed no terms"));
        }
        for term in indexed {
            if !concepts.insert(term.concept.clone()) {
                return Err(format!("FIBO duplicate concept: {}", term.concept));
            }
            terms.push(term);
        }
    }
    if terms.is_empty() {
        return Err("FIBO Release indexed no terms".to_string());
    }
    Ok(terms)
}

fn build() -> Result<(), String> {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").map_err(|e| e.to_string())?);
    let sources = manifest.join("sources");
    println!("cargo:rerun-if-changed=sources");
    println!("cargo:rerun-if-changed=src/published_sources.rs");

    let pins = verify_lock(&sources)?;
    let fibo_terms = index_fibo(&manifest, &sources, &pins)?;

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

    // RDF vocabularies: parse each directory once, index each namespace.
    let mut parsed: std::collections::HashMap<
        &str,
        Vec<(String, Vec<published_sources::RdfTriple>)>,
    > = std::collections::HashMap::new();
    for vocabulary in published_sources::RDF_VOCABULARIES {
        let prefix = format!("{}/", vocabulary.directory);
        let directory_pins: Vec<&Pin> = pins
            .iter()
            .filter(|pin| pin.path.starts_with(&prefix))
            .collect();
        let version = directory_pins
            .first()
            .map(|pin| pin.version.clone())
            .ok_or_else(|| format!("SOURCES.lock pins no files for {}", vocabulary.namespace))?;
        if !parsed.contains_key(vocabulary.directory) {
            let mut files = Vec::new();
            for pin in &directory_pins {
                let name = pin.path.trim_start_matches(&prefix).to_string();
                files.push((name, read_rdf(&sources, pin)?));
            }
            parsed.insert(vocabulary.directory, files);
        }
        let vocabulary_terms =
            published_sources::index_rdf(vocabulary, &parsed[vocabulary.directory], &version);
        if vocabulary_terms.is_empty() {
            return Err(format!(
                "{} ({}) indexed no terms",
                vocabulary.namespace, vocabulary.prefix
            ));
        }
        terms.extend(vocabulary_terms);
    }

    terms.extend(fibo_terms);
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
