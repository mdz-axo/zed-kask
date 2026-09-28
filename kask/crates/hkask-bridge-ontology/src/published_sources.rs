//! Readers for the vendored full-ontology sources, shared by `build.rs`
//! (which compiles the sources into the embedded index) and this crate's
//! tests. Dependency-free on purpose: the build script includes this file
//! with `#[path]`, so it may use only `std`.
//!
//! The readers extract vocabulary, never axioms: each published term with its
//! labels, direct parents, published definition and source file. There is no
//! reasoning here — a parent is only what the source states directly.

use std::collections::{BTreeMap, HashMap};

/// One indexed term, ready to serialize as an index line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexedTerm {
    pub namespace: String,
    pub concept: String,
    pub name: String,
    pub kind: String,
    pub labels: Vec<String>,
    pub parents: Vec<String>,
    pub inverse_of: Vec<String>,
    pub definition: String,
    pub source: String,
}

const FIELD_SEP: char = '\t';
const LIST_SEP: char = '\u{1f}';

fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl IndexedTerm {
    /// Serialize as one tab-separated index line (lists joined by U+001F).
    pub fn to_line(&self) -> String {
        let list = |items: &[String]| {
            items
                .iter()
                .map(|item| clean(item))
                .collect::<Vec<_>>()
                .join(&LIST_SEP.to_string())
        };
        [
            clean(&self.namespace),
            clean(&self.concept),
            clean(&self.name),
            clean(&self.kind),
            list(&self.labels),
            list(&self.parents),
            list(&self.inverse_of),
            clean(&self.definition),
            clean(&self.source),
        ]
        .join(&FIELD_SEP.to_string())
    }
}

// ── KIF (SUMO) ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sexp {
    Atom(String),
    Str(String),
    List(Vec<Sexp>),
}

/// Read every top-level form of a SUO-KIF document. `;` starts a comment only
/// outside a string; strings honour backslash escapes.
pub fn read_kif(source: &str) -> Result<Vec<Sexp>, String> {
    let mut stack: Vec<Vec<Sexp>> = vec![Vec::new()];
    let mut chars = source.char_indices().peekable();
    while let Some((offset, c)) = chars.next() {
        match c {
            ';' => {
                for (_, next) in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
            }
            '(' => stack.push(Vec::new()),
            ')' => {
                let list = stack
                    .pop()
                    .filter(|_| !stack.is_empty())
                    .ok_or_else(|| format!("unbalanced ')' at byte {offset}"))?;
                stack
                    .last_mut()
                    .ok_or_else(|| format!("unbalanced ')' at byte {offset}"))?
                    .push(Sexp::List(list));
            }
            '"' => {
                let mut text = String::new();
                let mut closed = false;
                while let Some((_, next)) = chars.next() {
                    match next {
                        '\\' => {
                            if let Some((_, escaped)) = chars.next() {
                                text.push(escaped);
                            }
                        }
                        '"' => {
                            closed = true;
                            break;
                        }
                        other => text.push(other),
                    }
                }
                if !closed {
                    return Err(format!("unterminated string starting at byte {offset}"));
                }
                push_item(&mut stack, Sexp::Str(text), offset)?;
            }
            c if c.is_whitespace() => {}
            _ => {
                let mut atom = String::from(c);
                while let Some(&(_, next)) = chars.peek() {
                    if next.is_whitespace() || matches!(next, '(' | ')' | '"' | ';') {
                        break;
                    }
                    atom.push(next);
                    chars.next();
                }
                push_item(&mut stack, Sexp::Atom(atom), offset)?;
            }
        }
    }
    if stack.len() != 1 {
        return Err(format!("{} unclosed '('", stack.len() - 1));
    }
    stack.pop().ok_or_else(|| "empty reader stack".to_string())
}

fn push_item(stack: &mut [Vec<Sexp>], item: Sexp, offset: usize) -> Result<(), String> {
    stack
        .last_mut()
        .map(|top| top.push(item))
        .ok_or_else(|| format!("reader stack empty at byte {offset}"))
}

fn is_term_atom(atom: &str) -> bool {
    !atom.starts_with('?')
        && !atom.starts_with('@')
        && atom
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic())
}

/// SUMO documentation markup: `&%Term` marks a cross-reference.
fn strip_kif_markup(text: &str) -> String {
    clean(&text.replace("&%", ""))
}

#[derive(Default)]
struct SumoAccumulator {
    order: Vec<String>,
    labels: HashMap<String, Vec<String>>,
    parents: HashMap<String, Vec<String>>,
    instance_of: HashMap<String, Vec<String>>,
    is_relation: HashMap<String, bool>,
    definition: HashMap<String, (String, String)>,
    first_file: HashMap<String, String>,
}

impl SumoAccumulator {
    fn note(&mut self, term: &str, file: &str) {
        if !self.first_file.contains_key(term) {
            self.first_file.insert(term.to_string(), file.to_string());
            self.order.push(term.to_string());
        }
    }

    fn push_unique(map: &mut HashMap<String, Vec<String>>, key: &str, value: &str) {
        let values = map.entry(key.to_string()).or_default();
        if !values.iter().any(|existing| existing == value) {
            values.push(value.to_string());
        }
    }
}

/// Index a set of SUMO KIF files. Files are processed in the given order; the
/// first English `documentation` seen for a term is its definition, so pass
/// `Merge.kif` and `Mid-level-ontology.kif` first.
pub fn index_sumo(
    files: &[(String, String)],
    provenance: &str,
) -> Result<Vec<IndexedTerm>, String> {
    let mut acc = SumoAccumulator::default();
    for (file, text) in files {
        let forms = read_kif(text).map_err(|error| format!("{file}: {error}"))?;
        for form in &forms {
            let Sexp::List(items) = form else { continue };
            let [Sexp::Atom(head), rest @ ..] = items.as_slice() else {
                continue;
            };
            match (head.as_str(), rest) {
                (
                    "subclass" | "instance" | "subrelation" | "subAttribute",
                    [Sexp::Atom(child), Sexp::Atom(parent)],
                ) if is_term_atom(child) && is_term_atom(parent) => {
                    acc.note(child, file);
                    let target = if head == "instance" {
                        &mut acc.instance_of
                    } else {
                        &mut acc.parents
                    };
                    SumoAccumulator::push_unique(target, child, &format!("sumo:{parent}"));
                    if head == "subrelation"
                        || (head == "instance"
                            && (parent.ends_with("Relation")
                                || parent.ends_with("Predicate")
                                || parent.ends_with("Function")))
                    {
                        acc.is_relation.insert(child.clone(), true);
                    }
                }
                ("documentation", [Sexp::Atom(term), Sexp::Atom(language), Sexp::Str(text)])
                    if is_term_atom(term) && language.starts_with("English") =>
                {
                    acc.note(term, file);
                    acc.definition
                        .entry(term.clone())
                        .or_insert_with(|| (strip_kif_markup(text), file.clone()));
                }
                ("termFormat", [Sexp::Atom(language), Sexp::Atom(term), Sexp::Str(label)])
                    if is_term_atom(term) && language.starts_with("English") =>
                {
                    acc.note(term, file);
                    SumoAccumulator::push_unique(&mut acc.labels, term, &clean(label));
                }
                _ => {}
            }
        }
    }

    let mut out = Vec::with_capacity(acc.order.len());
    for term in &acc.order {
        let parents = acc.parents.get(term).cloned().unwrap_or_default();
        let instance_of = acc.instance_of.get(term).cloned().unwrap_or_default();
        let kind = if acc.is_relation.contains_key(term) {
            "relation"
        } else if !parents.is_empty() {
            "class"
        } else if !instance_of.is_empty() {
            "instance"
        } else {
            "term"
        };
        let (definition, def_file) = acc
            .definition
            .get(term)
            .cloned()
            .unwrap_or_else(|| (String::new(), acc.first_file[term].clone()));
        out.push(IndexedTerm {
            namespace: "SUMO".to_string(),
            concept: format!("sumo:{term}"),
            name: term.clone(),
            kind: kind.to_string(),
            labels: acc.labels.get(term).cloned().unwrap_or_default(),
            parents: parents.into_iter().chain(instance_of).collect(),
            inverse_of: Vec::new(),
            definition,
            source: format!("{def_file} ({provenance})"),
        });
    }
    Ok(out)
}

// ── CSV (schema.org) ─────────────────────────────────────────────────────────

/// RFC 4180 reader: quoted fields may contain commas, newlines and `""`.
pub fn read_csv(text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => in_quotes = false,
                other => field.push(other),
            }
            continue;
        }
        match c {
            '"' => in_quotes = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            other => field.push(other),
        }
    }
    if in_quotes {
        return Err("unterminated quoted field".to_string());
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

/// schema.org comments carry HTML; keep the text.
fn strip_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut tag: Option<String> = None;
    for c in text.chars() {
        match (&mut tag, c) {
            (None, '<') => tag = Some(String::new()),
            (Some(name), '>') => {
                // Block-level tags separate words; inline tags (links, code)
                // must not split them (`<a>encoding</a>s` stays `encodings`).
                let name = name.trim_start_matches('/').to_ascii_lowercase();
                if ["br", "p", "li", "ul", "ol", "div", "h", "tr", "td"]
                    .iter()
                    .any(|block| name.starts_with(block))
                {
                    out.push(' ');
                }
                tag = None;
            }
            (Some(name), other) => name.push(other),
            (None, other) => out.push(other),
        }
    }
    let decoded = out
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&");
    clean(&decoded)
}

fn schema_id(url: &str) -> Option<String> {
    url.trim()
        .strip_prefix("https://schema.org/")
        .filter(|local| !local.is_empty())
        .map(|local| format!("schema:{local}"))
}

fn schema_list(cell: &str) -> Vec<String> {
    cell.split(',').filter_map(schema_id).collect()
}

/// Index the schema.org types and properties CSV exports of one release.
pub fn index_schema_org(
    types_csv: &str,
    properties_csv: &str,
    release: &str,
) -> Result<Vec<IndexedTerm>, String> {
    let mut out = Vec::new();
    for (csv, file, is_types) in [
        (types_csv, "schemaorg-all-https-types.csv", true),
        (properties_csv, "schemaorg-all-https-properties.csv", false),
    ] {
        let rows = read_csv(csv).map_err(|error| format!("{file}: {error}"))?;
        let Some((header, records)) = rows.split_first() else {
            return Err(format!("{file}: empty"));
        };
        let column: BTreeMap<&str, usize> = header
            .iter()
            .enumerate()
            .map(|(index, name)| (name.as_str(), index))
            .collect();
        let cell = |record: &[String], name: &str| -> String {
            column
                .get(name)
                .and_then(|&index| record.get(index))
                .cloned()
                .unwrap_or_default()
        };
        for record in records {
            if record.iter().all(String::is_empty) {
                continue;
            }
            let Some(concept) = schema_id(&cell(record, "id")) else {
                return Err(format!("{file}: row without a schema.org id: {record:?}"));
            };
            let name = concept.trim_start_matches("schema:").to_string();
            let (kind, parents) = if is_types {
                let enumeration = schema_list(&cell(record, "enumerationtype"));
                if enumeration.is_empty() {
                    ("class", schema_list(&cell(record, "subTypeOf")))
                } else {
                    let mut parents = enumeration;
                    parents.extend(schema_list(&cell(record, "subTypeOf")));
                    ("enumeration_member", parents)
                }
            } else {
                ("property", schema_list(&cell(record, "subPropertyOf")))
            };
            let mut definition = strip_html(&cell(record, "comment"));
            let superseded = schema_list(&cell(record, "supersededBy"));
            if !superseded.is_empty() {
                definition = format!("{definition} [superseded by {}]", superseded.join(", "));
            }
            let layer = cell(record, "isPartOf");
            let layer = layer
                .trim()
                .strip_prefix("https://")
                .and_then(|host| host.strip_suffix(".schema.org"))
                .unwrap_or("core");
            let label = clean(&cell(record, "label"));
            out.push(IndexedTerm {
                namespace: "schema.org".to_string(),
                concept,
                name: name.clone(),
                kind: kind.to_string(),
                labels: if label.is_empty() || label == name {
                    Vec::new()
                } else {
                    vec![label]
                },
                parents,
                inverse_of: if is_types {
                    Vec::new()
                } else {
                    schema_list(&cell(record, "inverseOf"))
                },
                definition,
                source: format!("{file} ({release}, {layer})"),
            });
        }
    }
    Ok(out)
}

// ── RDF (Turtle, RDF/XML) ───────────────────────────────────────────────────────────

/// An RDF vocabulary loaded in full from `sources/<directory>/`. A term
/// belongs to exactly one vocabulary: the one owning its namespace IRI.
/// Terms a file merely reuses from another namespace are indexed by that
/// namespace's owner, never duplicated.
pub struct RdfVocabulary<'a> {
    pub namespace: &'a str,
    pub prefix: &'a str,
    pub iri: &'a str,
    pub directory: &'a str,
}

pub const RDF_VOCABULARIES: &[RdfVocabulary] = &[
    RdfVocabulary {
        namespace: "RDF",
        prefix: "rdf",
        iri: "http://www.w3.org/1999/02/22-rdf-syntax-ns#",
        directory: "rdf-11",
    },
    RdfVocabulary {
        namespace: "RDFS",
        prefix: "rdfs",
        iri: "http://www.w3.org/2000/01/rdf-schema#",
        directory: "rdf-11",
    },
    RdfVocabulary {
        namespace: "RDF Data Cube",
        prefix: "qb",
        iri: "http://purl.org/linked-data/cube#",
        directory: "rdf-data-cube",
    },
    RdfVocabulary {
        namespace: "Dublin Core",
        prefix: "dcterms",
        iri: "http://purl.org/dc/terms/",
        directory: "dublin-core",
    },
    RdfVocabulary {
        namespace: "Dublin Core",
        prefix: "dcmitype",
        iri: "http://purl.org/dc/dcmitype/",
        directory: "dublin-core",
    },
    RdfVocabulary {
        namespace: "Dublin Core",
        prefix: "dc",
        iri: "http://purl.org/dc/elements/1.1/",
        directory: "dublin-core",
    },
    RdfVocabulary {
        namespace: "Dublin Core",
        prefix: "dcam",
        iri: "http://purl.org/dc/dcam/",
        directory: "dublin-core",
    },
    RdfVocabulary {
        namespace: "BIBO",
        prefix: "bibo",
        iri: "http://purl.org/ontology/bibo/",
        directory: "bibo",
    },
    RdfVocabulary {
        namespace: "CiTO",
        prefix: "cito",
        iri: "http://purl.org/spar/cito/",
        directory: "cito",
    },
    RdfVocabulary {
        namespace: "PKO",
        prefix: "pko",
        iri: "https://w3id.org/pko#",
        directory: "pko",
    },
    RdfVocabulary {
        namespace: "P-Plan",
        prefix: "pplan",
        iri: "http://purl.org/net/p-plan#",
        directory: "p-plan",
    },
    RdfVocabulary {
        namespace: "PROV",
        prefix: "prov",
        iri: "http://www.w3.org/ns/prov#",
        directory: "prov",
    },
    RdfVocabulary {
        namespace: "ML-Schema",
        prefix: "mls",
        iri: "http://www.w3.org/ns/mls#",
        directory: "ml-schema",
    },
    RdfVocabulary {
        namespace: "SEPIO",
        prefix: "SEPIO",
        iri: "http://purl.obolibrary.org/obo/SEPIO_",
        directory: "sepio",
    },
    RdfVocabulary {
        namespace: "GOLEM",
        prefix: "gc",
        iri: "https://w3id.org/golem/ontology#",
        directory: "golem",
    },
    RdfVocabulary {
        namespace: "CIDOC-CRM",
        prefix: "crm",
        iri: "http://www.cidoc-crm.org/cidoc-crm/",
        directory: "cidoc-crm",
    },
    RdfVocabulary {
        namespace: "LRMoo",
        prefix: "lrmoo",
        iri: "http://iflastandards.info/ns/lrm/lrmoo/",
        directory: "lrmoo",
    },
    RdfVocabulary {
        namespace: "OMC",
        prefix: "omc",
        iri: "https://movielabs.com/omc/rdf/schema/v2.8#",
        directory: "omc",
    },
    RdfVocabulary {
        namespace: "OMC",
        prefix: "omcT",
        iri: "https://movielabs.com/omc/rdf/schema/v2.8Tentative#",
        directory: "omc",
    },
    RdfVocabulary {
        namespace: "OMC",
        prefix: "cw",
        iri: "https://movielabs.com/cw/rdf/schema/v2.8#",
        directory: "omc",
    },
];

const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const RDFS: &str = "http://www.w3.org/2000/01/rdf-schema#";
const OWL: &str = "http://www.w3.org/2002/07/owl#";
const SKOS: &str = "http://www.w3.org/2004/02/skos/core#";
const VS: &str = "http://www.w3.org/2003/06/sw-vocab-status/ns#";

/// Prefixes used to compact parent and inverse IRIs; anything else stays a
/// full IRI so no namespace is invented.
const WELL_KNOWN_PREFIXES: &[(&str, &str)] = &[
    ("rdf", RDF),
    ("rdfs", RDFS),
    ("owl", OWL),
    ("skos", SKOS),
    ("xsd", "http://www.w3.org/2001/XMLSchema#"),
    ("foaf", "http://xmlns.com/foaf/0.1/"),
    ("dcat", "http://www.w3.org/ns/dcat#"),
    ("schema", "https://schema.org/"),
    // OBO Foundry ontologies SEPIO specializes (parent IRIs).
    ("IAO", "http://purl.obolibrary.org/obo/IAO_"),
    ("BFO", "http://purl.obolibrary.org/obo/BFO_"),
    ("RO", "http://purl.obolibrary.org/obo/RO_"),
    ("OBI", "http://purl.obolibrary.org/obo/OBI_"),
];

/// The object of one RDF statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RdfObject {
    Iri(String),
    Blank,
    Literal {
        value: String,
        language: Option<String>,
    },
}

/// One RDF statement; a blank-node subject is `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RdfTriple {
    pub subject: Option<String>,
    pub predicate: String,
    pub object: RdfObject,
}

/// Compact an IRI with a loaded vocabulary or well-known prefix.
pub fn compact_iri(iri: &str) -> String {
    RDF_VOCABULARIES
        .iter()
        .map(|vocabulary| (vocabulary.prefix, vocabulary.iri))
        .chain(WELL_KNOWN_PREFIXES.iter().copied())
        .find_map(|(prefix, namespace)| {
            iri.strip_prefix(namespace)
                .filter(|local| !local.is_empty())
                .map(|local| format!("{prefix}:{local}"))
        })
        .unwrap_or_else(|| iri.to_string())
}

#[derive(Default)]
struct RdfSubject {
    types: Vec<String>,
    labels: Vec<String>,
    parents: Vec<String>,
    inverse_of: Vec<String>,
    definitions: Vec<(usize, String)>,
    deprecated: bool,
    file: String,
}

fn english(language: &Option<String>) -> bool {
    language.as_deref().is_none_or(|tag| {
        tag.eq_ignore_ascii_case("en") || tag.to_ascii_lowercase().starts_with("en-")
    })
}

fn is_meta_iri(iri: &str) -> bool {
    [RDF, RDFS, OWL, SKOS]
        .iter()
        .any(|namespace| iri.starts_with(namespace))
}

/// Index every term the vocabulary's own namespace defines across its files.
/// Definition precedence: `skos:definition` / `prov:definition` /
/// `prov:editorsDefinition` (PROV and P-Plan publish definitions there) /
/// `IAO:0000115` (the OBO definition annotation SEPIO uses), `rdfs:comment`, then usage notes
/// (`dcterms:description`, `dc:description`, `skos:scopeNote`). Parents are the named
/// `rdfs:subClassOf`/`rdfs:subPropertyOf` objects (OWL restriction blank
/// nodes are axioms and are skipped) and, for individuals, their non-meta
/// `rdf:type`.
pub fn index_rdf(
    vocabulary: &RdfVocabulary<'_>,
    files: &[(String, Vec<RdfTriple>)],
    version: &str,
) -> Vec<IndexedTerm> {
    index_rdf_with_prefixes(vocabulary, files, version, &[])
}

/// As `index_rdf`, but also compacts cross-module links using the exact
/// namespace bindings declared by the pinned source, not constructed IRIs.
pub fn index_rdf_with_prefixes(
    vocabulary: &RdfVocabulary<'_>,
    files: &[(String, Vec<RdfTriple>)],
    version: &str,
    prefixes: &[(&str, &str)],
) -> Vec<IndexedTerm> {
    let compact = |iri: &str| {
        prefixes
            .iter()
            .find_map(|(prefix, namespace)| {
                iri.strip_prefix(namespace)
                    .filter(|local| !local.is_empty())
                    .map(|local| format!("{prefix}:{local}"))
            })
            .unwrap_or_else(|| compact_iri(iri))
    };
    let mut order: Vec<String> = Vec::new();
    let mut subjects: HashMap<String, RdfSubject> = HashMap::new();
    let definition_rank = |predicate: &str| match predicate {
        p if p == format!("{SKOS}definition") => Some(0),
        "http://www.w3.org/ns/prov#definition" => Some(0),
        "http://www.w3.org/ns/prov#editorsDefinition" => Some(0),
        // OBO Foundry definition annotation (SEPIO and its imports).
        "http://purl.obolibrary.org/obo/IAO_0000115" => Some(0),
        p if p == format!("{RDFS}comment") => Some(1),
        "http://purl.org/dc/terms/description" => Some(2),
        "http://purl.org/dc/elements/1.1/description" => Some(3),
        p if p == format!("{SKOS}scopeNote") => Some(4),
        _ => None,
    };
    for (file, triples) in files {
        for triple in triples {
            let Some(subject) = triple.subject.as_deref() else {
                continue;
            };
            let Some(local) = subject.strip_prefix(vocabulary.iri) else {
                continue;
            };
            if local.is_empty() || local.contains(['/', '#']) {
                continue;
            }
            let entry = subjects.entry(subject.to_string()).or_insert_with(|| {
                order.push(subject.to_string());
                RdfSubject {
                    file: file.clone(),
                    ..RdfSubject::default()
                }
            });
            let push = |values: &mut Vec<String>, value: String| {
                if !values.contains(&value) {
                    values.push(value);
                }
            };
            let predicate = triple.predicate.as_str();
            match &triple.object {
                RdfObject::Iri(object) => {
                    if predicate == format!("{RDF}type") {
                        push(&mut entry.types, object.clone());
                    } else if predicate == format!("{RDFS}subClassOf")
                        || predicate == format!("{RDFS}subPropertyOf")
                    {
                        push(&mut entry.parents, compact(object));
                    } else if predicate == format!("{OWL}inverseOf") {
                        push(&mut entry.inverse_of, compact(object));
                    }
                }
                RdfObject::Literal { value, language } => {
                    if (predicate == format!("{RDFS}label")
                        || predicate == format!("{SKOS}prefLabel"))
                        && english(language)
                    {
                        push(&mut entry.labels, clean(value));
                    } else if let Some(rank) =
                        definition_rank(predicate).filter(|_| english(language))
                    {
                        entry.definitions.push((rank, clean(value)));
                    } else if predicate == format!("{OWL}deprecated") && value == "true" {
                        entry.deprecated = true;
                    } else if predicate == format!("{VS}term_status")
                        && value.trim() == "deprecated"
                    {
                        entry.deprecated = true;
                    }
                }
                RdfObject::Blank => {}
            }
        }
    }

    order
        .into_iter()
        .map(|iri| {
            let subject = &subjects[&iri];
            let local = iri.trim_start_matches(vocabulary.iri).to_string();
            let has_type = |suffixes: &[&str]| {
                subject.types.iter().any(|kind| {
                    is_meta_iri(kind) && suffixes.iter().any(|suffix| kind.ends_with(suffix))
                })
            };
            let non_meta_types: Vec<String> = subject
                .types
                .iter()
                .filter(|kind| !is_meta_iri(kind))
                .map(|kind| compact(kind))
                .collect();
            let kind = if has_type(&["#Class"]) {
                "class"
            } else if has_type(&["Property"]) {
                "property"
            } else if has_type(&["#NamedIndividual"]) || !non_meta_types.is_empty() {
                "instance"
            } else {
                "term"
            };
            let mut parents = subject.parents.clone();
            if kind == "instance" {
                parents.extend(non_meta_types);
            }
            let mut definitions = subject.definitions.clone();
            definitions.sort_by_key(|(rank, _)| *rank);
            let mut definition = definitions
                .into_iter()
                .next()
                .map(|(_, text)| text)
                .unwrap_or_default();
            if subject.deprecated {
                definition = format!("{definition} [deprecated]").trim().to_string();
            }
            let labels = subject
                .labels
                .iter()
                .filter(|label| **label != local)
                .cloned()
                .collect();
            IndexedTerm {
                namespace: vocabulary.namespace.to_string(),
                concept: format!("{}:{local}", vocabulary.prefix),
                name: local,
                kind: kind.to_string(),
                labels,
                parents,
                inverse_of: subject.inverse_of.clone(),
                definition,
                source: format!("{} ({version})", subject.file),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iri_triple(subject: &str, predicate: &str, object: &str) -> RdfTriple {
        RdfTriple {
            subject: Some(subject.into()),
            predicate: predicate.into(),
            object: RdfObject::Iri(object.into()),
        }
    }

    fn literal_triple(
        subject: &str,
        predicate: &str,
        value: &str,
        language: Option<&str>,
    ) -> RdfTriple {
        RdfTriple {
            subject: Some(subject.into()),
            predicate: predicate.into(),
            object: RdfObject::Literal {
                value: value.into(),
                language: language.map(str::to_string),
            },
        }
    }

    /// expect: own-namespace terms only, typed by their RDF declaration,
    /// named parents only, English text only, definition precedence honoured.
    #[test]
    fn rdf_index_keeps_own_namespace_terms_with_published_text() {
        let pko = &RDF_VOCABULARIES
            .iter()
            .find(|vocabulary| vocabulary.prefix == "pko")
            .expect("pko");
        let p = |local: &str| format!("https://w3id.org/pko#{local}");
        let triples = vec![
            iri_triple(
                &p("Procedure"),
                &format!("{RDF}type"),
                &format!("{OWL}Class"),
            ),
            iri_triple(
                &p("Procedure"),
                &format!("{RDFS}subClassOf"),
                "http://purl.org/net/p-plan#Plan",
            ),
            RdfTriple {
                subject: Some(p("Procedure")),
                predicate: format!("{RDFS}subClassOf"),
                object: RdfObject::Blank,
            },
            literal_triple(
                &p("Procedure"),
                "http://purl.org/dc/terms/description",
                "usage note",
                Some("en"),
            ),
            literal_triple(
                &p("Procedure"),
                &format!("{RDFS}comment"),
                "A sequence of actions.",
                Some("en"),
            ),
            literal_triple(
                &p("Procedure"),
                &format!("{RDFS}comment"),
                "Una sequenza.",
                Some("it"),
            ),
            literal_triple(
                &p("Procedure"),
                &format!("{RDFS}label"),
                "procedure",
                Some("en"),
            ),
            iri_triple(
                &p("Completed"),
                &format!("{RDF}type"),
                &format!("{OWL}NamedIndividual"),
            ),
            iri_triple(
                &p("Completed"),
                &format!("{RDF}type"),
                &p("ProcedureExecutionStatus"),
            ),
            iri_triple(
                &p("hasStep"),
                &format!("{RDF}type"),
                &format!("{OWL}ObjectProperty"),
            ),
            iri_triple(&p("hasStep"), &format!("{OWL}inverseOf"), &p("isStepOf")),
            literal_triple(&p("old"), &format!("{OWL}deprecated"), "true", None),
            iri_triple(
                "http://purl.org/net/p-plan#Step",
                &format!("{RDF}type"),
                &format!("{OWL}Class"),
            ),
            iri_triple(
                "https://w3id.org/pko",
                &format!("{RDF}type"),
                &format!("{OWL}Ontology"),
            ),
        ];
        let terms = index_rdf(pko, &[("pko.ttl".into(), triples)], "PKO 2.0.0");
        let names: Vec<_> = terms.iter().map(|term| term.concept.as_str()).collect();
        assert_eq!(
            names,
            ["pko:Procedure", "pko:Completed", "pko:hasStep", "pko:old"],
            "reused pplan:Step is its owner's term"
        );
        let procedure = &terms[0];
        assert_eq!(
            (procedure.kind.as_str(), procedure.definition.as_str()),
            ("class", "A sequence of actions.")
        );
        assert_eq!(procedure.parents, ["pplan:Plan"]);
        assert_eq!(procedure.labels, ["procedure"]);
        assert_eq!(procedure.source, "pko.ttl (PKO 2.0.0)");
        assert_eq!(
            (terms[1].kind.as_str(), terms[1].parents.as_slice()),
            (
                "instance",
                &["pko:ProcedureExecutionStatus".to_string()][..]
            )
        );
        assert_eq!(
            (terms[2].kind.as_str(), terms[2].inverse_of.as_slice()),
            ("property", &["pko:isStepOf".to_string()][..])
        );
        assert_eq!(terms[3].definition, "[deprecated]");
        assert_eq!(
            compact_iri("http://example.org/x#Y"),
            "http://example.org/x#Y",
            "no invented prefix"
        );
    }

    /// expect: each RDF vocabulary owns one namespace IRI and has pinned
    /// sources in its directory.
    #[test]
    fn rdf_vocabularies_are_distinct_and_pinned() {
        let lock = include_str!("../sources/SOURCES.lock");
        let mut iris = std::collections::HashSet::new();
        for vocabulary in RDF_VOCABULARIES {
            assert!(
                iris.insert(vocabulary.iri),
                "{} owned twice",
                vocabulary.iri
            );
            assert!(
                lock.lines()
                    .any(|line| line.starts_with(&format!("{}/", vocabulary.directory))),
                "{} has no pinned source",
                vocabulary.namespace
            );
        }
    }

    /// expect: `prov:definition` is a definition of record, ahead of a comment.
    #[test]
    fn prov_definition_outranks_comment() {
        let pplan = RDF_VOCABULARIES
            .iter()
            .find(|vocabulary| vocabulary.prefix == "pplan")
            .expect("p-plan");
        let step = "http://purl.org/net/p-plan#Step";
        let terms = index_rdf(
            pplan,
            &[(
                "p-plan.owl".into(),
                vec![
                    literal_triple(step, &format!("{RDFS}comment"), "comment", Some("en")),
                    literal_triple(
                        step,
                        "http://www.w3.org/ns/prov#definition",
                        "planned activity",
                        Some("en"),
                    ),
                ],
            )],
            "P-Plan 1.3",
        );
        assert_eq!(terms[0].definition, "planned activity");
    }

    #[test]
    fn kif_reader_handles_comments_strings_and_escapes() {
        let forms = read_kif(
            "; header\n(documentation Game EnglishLanguage \"A &%Contest; for fun \\\"x\\\"\")\n(subclass Game Contest)",
        )
        .expect("parse");
        assert_eq!(forms.len(), 2);
        assert_eq!(
            forms[0],
            Sexp::List(vec![
                Sexp::Atom("documentation".into()),
                Sexp::Atom("Game".into()),
                Sexp::Atom("EnglishLanguage".into()),
                Sexp::Str("A &%Contest; for fun \"x\"".into()),
            ])
        );
        assert!(read_kif("(a (b)").is_err());
        assert!(read_kif("(a))").is_err());
    }

    #[test]
    fn sumo_index_collects_parents_labels_and_first_definition() {
        let files = vec![
            (
                "Merge.kif".to_string(),
                "(subclass Game Contest)\n(subclass Game RecreationOrExercise)\n\
                 (documentation Game EnglishWrittenLanguage \"A &%Contest whose purpose is enjoyment.\")\n\
                 (instance part PartialOrderingRelation)\n(=> (instance ?X Game) (exists (?Y) (agent ?X ?Y)))"
                    .to_string(),
            ),
            (
                "english_format.kif".to_string(),
                "(termFormat EnglishLanguage Game \"game\")\n\
                 (documentation Game EnglishLanguage \"later text is ignored\")"
                    .to_string(),
            ),
        ];
        let terms = index_sumo(&files, "test@1").expect("index");
        let game = terms.iter().find(|term| term.name == "Game").expect("Game");
        assert_eq!(game.kind, "class");
        assert_eq!(game.parents, ["sumo:Contest", "sumo:RecreationOrExercise"]);
        assert_eq!(game.labels, ["game"]);
        assert_eq!(game.definition, "A Contest whose purpose is enjoyment.");
        assert_eq!(game.source, "Merge.kif (test@1)");
        let part = terms.iter().find(|term| term.name == "part").expect("part");
        assert_eq!(part.kind, "relation");
        assert!(terms.iter().all(|term| !term.name.starts_with('?')));
    }

    /// expect: the index line format `published.rs` reads — nine tab fields,
    /// lists joined by U+001F, embedded whitespace collapsed.
    #[test]
    fn index_line_has_nine_fields_and_collapses_whitespace() {
        let line = IndexedTerm {
            namespace: "SUMO".into(),
            concept: "sumo:Game".into(),
            name: "Game".into(),
            kind: "class".into(),
            labels: vec!["game".into()],
            parents: vec!["sumo:Contest".into(), "sumo:RecreationOrExercise".into()],
            inverse_of: Vec::new(),
            definition: "A\tcontest\n  for fun.".into(),
            source: "Merge.kif (pin)".into(),
        }
        .to_line();
        let fields: Vec<&str> = line.split(FIELD_SEP).collect();
        assert_eq!(fields.len(), 9, "{line}");
        assert_eq!(
            fields[5],
            format!("sumo:Contest{LIST_SEP}sumo:RecreationOrExercise")
        );
        assert_eq!(fields[7], "A contest for fun.");
    }

    #[test]
    fn csv_reader_handles_quotes_commas_and_newlines() {
        let rows = read_csv("\"id\",\"comment\"\n\"a\",\"x, \"\"y\"\"\nz\"\n").expect("csv");
        assert_eq!(rows, [vec!["id", "comment"], vec!["a", "x, \"y\"\nz"]]);
    }

    #[test]
    fn schema_index_reads_types_members_and_properties() {
        let types = "\"id\",\"label\",\"comment\",\"subTypeOf\",\"enumerationtype\",\"isPartOf\"\n\
                     \"https://schema.org/Game\",\"Game\",\"The Game type <a href=\"\"/x\"\">x</a>.\",\"https://schema.org/CreativeWork\",\"\",\"\"\n\
                     \"https://schema.org/Monday\",\"Monday\",\"Monday.\",\"\",\"https://schema.org/DayOfWeek\",\"\"\n";
        let props = "\"id\",\"label\",\"comment\",\"subPropertyOf\",\"inverseOf\",\"supersededBy\",\"isPartOf\"\n\
                     \"https://schema.org/hasPart\",\"hasPart\",\"Parts.\",\"\",\"https://schema.org/isPartOf\",\"\",\"\"\n\
                     \"https://schema.org/old\",\"old\",\"Old.\",\"\",\"\",\"https://schema.org/new\",\"https://pending.schema.org\"\n";
        let terms = index_schema_org(types, props, "30.1").expect("index");
        let game = &terms[0];
        assert_eq!(
            (game.concept.as_str(), game.kind.as_str()),
            ("schema:Game", "class")
        );
        assert_eq!(game.parents, ["schema:CreativeWork"]);
        assert_eq!(game.definition, "The Game type x.");
        assert_eq!(terms[1].kind, "enumeration_member");
        assert_eq!(terms[2].inverse_of, ["schema:isPartOf"]);
        assert_eq!(terms[3].definition, "Old. [superseded by schema:new]");
        assert!(terms[3].source.ends_with("(30.1, pending)"));
    }
}
