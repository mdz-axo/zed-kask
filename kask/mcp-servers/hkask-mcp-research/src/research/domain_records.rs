//! Structured scientific domain-record resolution and 32 KiB corpus-cache spill (`F9`).
//!
//! Supports keyless REST lookups across seven canonical biomedical and
//! chemical databases:
//! - `pubmed` (NCBI E-utilities `esummary` `db=pubmed`)
//! - `ncbi_gene` (NCBI E-utilities `esummary` `db=gene`)
//! - `clinvar` (NCBI E-utilities `esummary` `db=clinvar`)
//! - `uniprotkb` (UniProtKB REST JSON)
//! - `chembl_molecule` (ChEMBL molecule REST JSON)
//! - `chembl_target` (ChEMBL target REST JSON)
//! - `rcsb_pdb` (RCSB PDB Data API core entry JSON)
//!
//! Payloads `<= 32 KiB` (`DOMAIN_RECORD_INLINE_MAX_BYTES`) return inline JSON;
//! payloads `> 32 KiB` spill to the corpus cache directory (`corpus-mcp/cache/`)
//! and return `{ "spilled": true, "corpus_ref": "<label>", "byte_len": N }`,
//! linking `corpus_ref` into `run_sources.corpus_ref` when `run_id` is supplied.

use std::path::Path;

use crate::research::runs::RUN_SOURCE_EXCERPT_MAX_CHARS;
use crate::research::types::{DomainRecordSpec, WebError, parse_closed_vocab};

/// Maximum payload size (32 KiB) returned inline before spilling to the
/// corpus cache directory.
pub const DOMAIN_RECORD_INLINE_MAX_BYTES: usize = 32 * 1024;

/// Canonical scientific databases supported by `resolve_paper`'s
/// `domain_record` mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScientificDatabase {
    Pubmed,
    NcbiGene,
    Clinvar,
    UniprotKb,
    ChemblMolecule,
    ChemblTarget,
    RcsbPdb,
}

impl ScientificDatabase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pubmed => "pubmed",
            Self::NcbiGene => "ncbi_gene",
            Self::Clinvar => "clinvar",
            Self::UniprotKb => "uniprotkb",
            Self::ChemblMolecule => "chembl_molecule",
            Self::ChemblTarget => "chembl_target",
            Self::RcsbPdb => "rcsb_pdb",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, WebError> {
        parse_closed_vocab(
            "domain_record.db",
            raw,
            &[
                (&["pubmed"], Self::Pubmed),
                (&["ncbi_gene", "gene"], Self::NcbiGene),
                (&["clinvar"], Self::Clinvar),
                (&["uniprotkb", "uniprot"], Self::UniprotKb),
                (&["chembl_molecule"], Self::ChemblMolecule),
                (&["chembl_target"], Self::ChemblTarget),
                (&["rcsb_pdb", "pdb"], Self::RcsbPdb),
            ],
        )
    }

    /// The REST API endpoint URL for fetching the structured JSON record.
    pub fn api_url(self, accession: &str) -> String {
        match self {
            Self::Pubmed => format!(
                "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=pubmed&id={accession}&retmode=json"
            ),
            Self::NcbiGene => format!(
                "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=gene&id={accession}&retmode=json"
            ),
            Self::Clinvar => format!(
                "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=clinvar&id={accession}&retmode=json"
            ),
            Self::UniprotKb => {
                let upper = accession.to_ascii_uppercase();
                format!("https://rest.uniprot.org/uniprotkb/{upper}.json")
            }
            Self::ChemblMolecule => {
                let upper = accession.to_ascii_uppercase();
                format!("https://www.ebi.ac.uk/chembl/api/data/molecule/{upper}.json")
            }
            Self::ChemblTarget => {
                let upper = accession.to_ascii_uppercase();
                format!("https://www.ebi.ac.uk/chembl/api/data/target/{upper}.json")
            }
            Self::RcsbPdb => {
                let upper = accession.to_ascii_uppercase();
                format!("https://data.rcsb.org/rest/v1/core/entry/{upper}")
            }
        }
    }

    /// The canonical human/ledger landing URL for the record.
    pub fn canonical_url(self, accession: &str) -> String {
        match self {
            Self::Pubmed => format!("https://pubmed.ncbi.nlm.nih.gov/{accession}/"),
            Self::NcbiGene => format!("https://www.ncbi.nlm.nih.gov/gene/{accession}"),
            Self::Clinvar => format!("https://www.ncbi.nlm.nih.gov/clinvar/variation/{accession}/"),
            Self::UniprotKb => {
                let upper = accession.to_ascii_uppercase();
                format!("https://www.uniprot.org/uniprotkb/{upper}/entry")
            }
            Self::ChemblMolecule => {
                let upper = accession.to_ascii_uppercase();
                format!("https://www.ebi.ac.uk/chembl/compound_report_card/{upper}/")
            }
            Self::ChemblTarget => {
                let upper = accession.to_ascii_uppercase();
                format!("https://www.ebi.ac.uk/chembl/target_report_card/{upper}/")
            }
            Self::RcsbPdb => {
                let upper = accession.to_ascii_uppercase();
                format!("https://www.rcsb.org/structure/{upper}")
            }
        }
    }
}

/// Validate and normalize a domain-record specification (`db` + `accession`).
pub fn validate_domain_record_spec(
    spec: &DomainRecordSpec,
) -> Result<(ScientificDatabase, String), WebError> {
    let db = ScientificDatabase::parse(&spec.db)?;
    let accession = spec.accession.trim();
    if accession.is_empty() {
        return Err(WebError::BadArgs(
            "domain_record.accession must not be empty".to_string(),
        ));
    }
    if accession.len() > 64
        || !accession
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(WebError::BadArgs(format!(
            "domain_record.accession must be 1..=64 ASCII alphanumeric/._- characters; got '{accession}'"
        )));
    }
    let normalized = match db {
        ScientificDatabase::UniprotKb
        | ScientificDatabase::ChemblMolecule
        | ScientificDatabase::ChemblTarget
        | ScientificDatabase::RcsbPdb => accession.to_ascii_uppercase(),
        ScientificDatabase::Pubmed | ScientificDatabase::NcbiGene | ScientificDatabase::Clinvar => {
            accession.to_string()
        }
    };
    Ok((db, normalized))
}

/// Raw fetched payload from a scientific database connector before spill
/// evaluation.
#[derive(Debug, Clone)]
pub struct FetchedDomainRecord {
    pub db: String,
    pub accession: String,
    pub canonical_url: String,
    pub title: Option<String>,
    pub raw_json: String,
}

/// Outcome of applying the 32 KiB inline-vs-spill threshold to a fetched
/// domain record.
#[derive(Debug, Clone)]
pub enum DomainRecordDisposition {
    Inline {
        record: serde_json::Value,
        byte_len: usize,
        excerpt: String,
    },
    Spilled {
        corpus_ref: String,
        byte_len: usize,
        excerpt: String,
    },
}

/// Default corpus cache directory (`~/Documents/zk-data/corpus-mcp/cache/`
/// or `HKASK_ARTIFACTS_DIR/corpus-mcp/cache/`).
pub fn default_corpus_cache_dir() -> std::path::PathBuf {
    hkask_types::agent_paths::resolve_under_artifacts_dir(Path::new("corpus-mcp/cache"))
}

/// Apply the 32 KiB inline-vs-spill threshold to a fetched JSON payload.
/// Payloads `<= DOMAIN_RECORD_INLINE_MAX_BYTES` return inline JSON; larger
/// payloads are written atomically to `cache_dir/{corpus_ref}.json` and
/// return the `corpus_ref` handle for `run_sources.corpus_ref`.
pub fn apply_spill_policy(
    db: &str,
    accession: &str,
    raw_json: &str,
    cache_dir: &Path,
) -> Result<DomainRecordDisposition, WebError> {
    let parsed: serde_json::Value = serde_json::from_str(raw_json).map_err(|e| {
        WebError::ProviderError(format!(
            "domain_record {db}:{accession} returned invalid JSON: {e}"
        ))
    })?;
    let byte_len = raw_json.len();
    let excerpt: String = raw_json
        .chars()
        .take(RUN_SOURCE_EXCERPT_MAX_CHARS)
        .collect();
    if byte_len <= DOMAIN_RECORD_INLINE_MAX_BYTES {
        return Ok(DomainRecordDisposition::Inline {
            record: parsed,
            byte_len,
            excerpt,
        });
    }
    std::fs::create_dir_all(cache_dir).map_err(|e| {
        WebError::ProviderError(format!(
            "failed to create corpus cache dir {}: {e}",
            cache_dir.display()
        ))
    })?;
    let safe_acc: String = accession
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let corpus_ref = format!("domain_{db}_{safe_acc}");
    let target_path = cache_dir.join(format!("{corpus_ref}.json"));
    std::fs::write(&target_path, raw_json.as_bytes()).map_err(|e| {
        WebError::ProviderError(format!(
            "failed to write spilled domain record {}: {e}",
            target_path.display()
        ))
    })?;
    Ok(DomainRecordDisposition::Spilled {
        corpus_ref,
        byte_len,
        excerpt,
    })
}

/// Extract a concise human-readable title from a scientific database JSON
/// record when present.
pub fn extract_record_title(
    db: ScientificDatabase,
    value: &serde_json::Value,
    accession: &str,
) -> Option<String> {
    match db {
        ScientificDatabase::Pubmed | ScientificDatabase::NcbiGene | ScientificDatabase::Clinvar => {
            value["result"][accession]["title"]
                .as_str()
                .or_else(|| value["result"][accession]["description"].as_str())
                .or_else(|| value["result"][accession]["name"].as_str())
                .map(str::to_string)
        }
        ScientificDatabase::UniprotKb => value["proteinDescription"]["recommendedName"]["fullName"]
            ["value"]
            .as_str()
            .or_else(|| value["uniProtkbId"].as_str())
            .map(str::to_string),
        ScientificDatabase::ChemblMolecule => value["pref_name"]
            .as_str()
            .or_else(|| value["molecule_chembl_id"].as_str())
            .map(str::to_string),
        ScientificDatabase::ChemblTarget => value["pref_name"]
            .as_str()
            .or_else(|| value["target_chembl_id"].as_str())
            .map(str::to_string),
        ScientificDatabase::RcsbPdb => value["struct"]["title"]
            .as_str()
            .or_else(|| value["rcsb_id"].as_str())
            .map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_and_normalizes_scientific_databases() {
        let (db, acc) = validate_domain_record_spec(&DomainRecordSpec {
            db: "uniprot".to_string(),
            accession: "p00533".to_string(),
        })
        .expect("valid uniprot");
        assert_eq!(db, ScientificDatabase::UniprotKb);
        assert_eq!(acc, "P00533");
        assert_eq!(
            db.canonical_url(&acc),
            "https://www.uniprot.org/uniprotkb/P00533/entry"
        );

        let (db, acc) = validate_domain_record_spec(&DomainRecordSpec {
            db: "rcsb_pdb".to_string(),
            accession: "4hhb".to_string(),
        })
        .expect("valid pdb");
        assert_eq!(db, ScientificDatabase::RcsbPdb);
        assert_eq!(acc, "4HHB");

        // Unknown db rejected with closed-vocabulary teaching message.
        let err = validate_domain_record_spec(&DomainRecordSpec {
            db: "unknown_db".to_string(),
            accession: "123".to_string(),
        })
        .expect_err("unknown db");
        assert!(err.to_string().contains("domain_record.db"));

        // Path traversal / injection characters in accession rejected.
        let err = validate_domain_record_spec(&DomainRecordSpec {
            db: "pubmed".to_string(),
            accession: "../etc/passwd".to_string(),
        })
        .expect_err("traversal rejected");
        assert!(err.to_string().contains("domain_record.accession"));
    }

    #[test]
    fn spill_policy_keeps_small_records_inline_and_spills_over_32kib() {
        let temp = tempfile::tempdir().expect("tempdir");
        let small = r#"{"rcsb_id":"4HHB","struct":{"title":"Deoxy human hemoglobin"}}"#;
        let inline =
            apply_spill_policy("rcsb_pdb", "4HHB", small, temp.path()).expect("inline policy");
        match inline {
            DomainRecordDisposition::Inline {
                record, byte_len, ..
            } => {
                assert_eq!(byte_len, small.len());
                assert_eq!(record["rcsb_id"], "4HHB");
            }
            DomainRecordDisposition::Spilled { .. } => panic!("small payload must stay inline"),
        }

        let large_payload = format!(
            r#"{{"uniProtkbId":"P00533","sequence":"{}"}}"#,
            "A".repeat(DOMAIN_RECORD_INLINE_MAX_BYTES + 1024)
        );
        let spilled = apply_spill_policy("uniprotkb", "P00533", &large_payload, temp.path())
            .expect("spilled policy");
        match spilled {
            DomainRecordDisposition::Spilled {
                corpus_ref,
                byte_len,
                ..
            } => {
                assert_eq!(corpus_ref, "domain_uniprotkb_p00533");
                assert_eq!(byte_len, large_payload.len());
                let written =
                    std::fs::read_to_string(temp.path().join("domain_uniprotkb_p00533.json"))
                        .expect("spilled file exists");
                assert_eq!(written.len(), large_payload.len());
            }
            DomainRecordDisposition::Inline { .. } => panic!("oversized payload must spill"),
        }
    }
}
