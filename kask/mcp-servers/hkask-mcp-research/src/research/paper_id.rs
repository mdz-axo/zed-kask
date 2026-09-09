//! Paper identity — typed identifiers, one parse function, canonical URLs.
//!
//! Invalid states are unrepresentable: every `PaperId` is constructed
//! through `parse_paper_id`, and every rejection is a typed `WebError::BadArgs`
//! naming what was expected — no `Option`-silence. Parse rules ported from
//! Feynman (`paper-rank.ts`): DOI case-normalized with the `10.` prefix
//! check; arXiv `NNNN.NNNNN(vN)`; PMID digits-only with `pmid:`/URL-prefix
//! stripping; PMCID `PMC\d+`; OpenAlex `W`-prefixed short form.

use crate::research::types::WebError;

/// A typed paper identifier. The payload is the normalized form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PaperId {
    Doi(String),
    Arxiv(String),
    Pmid(String),
    Pmcid(String),
    OpenAlex(String),
}

impl PaperId {
    /// The stable wire name for the identifier kind.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            PaperId::Doi(_) => "doi",
            PaperId::Arxiv(_) => "arxiv",
            PaperId::Pmid(_) => "pmid",
            PaperId::Pmcid(_) => "pmcid",
            PaperId::OpenAlex(_) => "openalex",
        }
    }

    /// The normalized identifier value.
    pub(crate) fn value(&self) -> &str {
        match self {
            PaperId::Doi(value)
            | PaperId::Arxiv(value)
            | PaperId::Pmid(value)
            | PaperId::Pmcid(value)
            | PaperId::OpenAlex(value) => value,
        }
    }

    /// The canonical landing URL for the identifier's registry.
    pub(crate) fn canonical_url(&self) -> String {
        match self {
            PaperId::Doi(doi) => format!("https://doi.org/{doi}"),
            PaperId::Arxiv(id) => format!("https://arxiv.org/abs/{id}"),
            PaperId::Pmid(pmid) => format!("https://pubmed.ncbi.nlm.nih.gov/{pmid}/"),
            PaperId::Pmcid(pmcid) => format!("https://pmc.ncbi.nlm.nih.gov/articles/{pmcid}/"),
            PaperId::OpenAlex(id) => format!("https://openalex.org/work/{id}"),
        }
    }
}

/// The ledger-usable key: kind-prefixed normalized value. Distinct kinds
/// with the same numeric payload key differently — a PMID and a bare arXiv
/// number are not the same paper.
pub(crate) fn stable_paper_key(id: &PaperId) -> String {
    format!("{}:{}", id.kind(), id.value())
}

/// Parse any identifier form — bare, prefixed (`doi:`, `arxiv:`, `pmid:`,
/// `pmc:`/`pmcid:`), or registry URL — into a typed `PaperId`. Every
/// rejection names what was expected.
pub(crate) fn parse_paper_id(input: &str) -> Result<PaperId, WebError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(WebError::BadArgs(expected_forms("empty input")));
    }
    let lowered = trimmed.to_lowercase();
    let candidate = strip_wrappers(&lowered);
    parse_bare(&candidate)
}

fn expected_forms(got: &str) -> String {
    format!(
        "expected a paper identifier — DOI (10.…), arXiv ID (NNNN.NNNNN[vN]), \
         PMID (digits), PMCID (PMC…), or OpenAlex work ID (W…); got '{got}'"
    )
}

/// Strip known registry URL hosts and identifier prefixes down to the
/// bare form. PMC article URLs and `pmc:`/`pmcid:` prefixes re-prepend the
/// `pmc` marker so a digits-only PMCID tail is not mistaken for a PMID.
fn strip_wrappers(lowered: &str) -> String {
    for prefix in [
        "https://doi.org/",
        "https://dx.doi.org/",
        "http://doi.org/",
        "https://arxiv.org/abs/",
        "https://arxiv.org/pdf/",
        "https://pubmed.ncbi.nlm.nih.gov/",
        "https://openalex.org/work/",
    ] {
        if let Some(rest) = lowered.strip_prefix(prefix) {
            return rest.trim_end_matches('/').to_string();
        }
    }
    for prefix in [
        "https://pmc.ncbi.nlm.nih.gov/articles/",
        "https://www.ncbi.nlm.nih.gov/pmc/articles/",
    ] {
        if let Some(rest) = lowered.strip_prefix(prefix) {
            return format!(
                "pmc{}",
                rest.trim_end_matches('/').trim_start_matches("pmc")
            );
        }
    }
    for prefix in ["doi:", "arxiv:", "pmid:"] {
        if let Some(rest) = lowered.strip_prefix(prefix) {
            return rest.trim().to_string();
        }
    }
    for prefix in ["pmcid:", "pmc:"] {
        if let Some(rest) = lowered.strip_prefix(prefix) {
            return format!("pmc{}", rest.trim().trim_start_matches("pmc"));
        }
    }
    lowered.to_string()
}

/// Parse a bare (wrapper-stripped, lowercased) candidate by shape, in a
/// fixed order: DOI, OpenAlex, PMCID, PMID, arXiv.
fn parse_bare(candidate: &str) -> Result<PaperId, WebError> {
    if candidate.len() > 3 && candidate.starts_with("10.") {
        return Ok(PaperId::Doi(candidate.to_string()));
    }
    if let Some(digits) = candidate.strip_prefix('w') {
        if !digits.is_empty() && digits.chars().all(|character| character.is_ascii_digit()) {
            return Ok(PaperId::OpenAlex(format!("W{digits}")));
        }
    }
    if let Some(digits) = candidate.strip_prefix("pmc") {
        if !digits.is_empty() && digits.chars().all(|character| character.is_ascii_digit()) {
            return Ok(PaperId::Pmcid(format!("PMC{digits}")));
        }
    }
    if !candidate.is_empty()
        && candidate
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return Ok(PaperId::Pmid(candidate.to_string()));
    }
    if is_arxiv_modern(candidate) {
        return Ok(PaperId::Arxiv(candidate.to_string()));
    }
    Err(WebError::BadArgs(expected_forms(candidate)))
}

/// Modern arXiv shape: `NNNN.NNNNN` (4 digits, dot, 4-5 digits) with an
/// optional `vN` version suffix.
fn is_arxiv_modern(candidate: &str) -> bool {
    let base = match candidate.split_once('v') {
        Some((base, version))
            if !version.is_empty() && version.chars().all(|digit| digit.is_ascii_digit()) =>
        {
            base
        }
        _ => candidate,
    };
    let Some((major, minor)) = base.split_once('.') else {
        return false;
    };
    major.len() == 4
        && major.chars().all(|digit| digit.is_ascii_digit())
        && (minor.len() == 4 || minor.len() == 5)
        && minor.chars().all(|digit| digit.is_ascii_digit())
}

#[cfg(test)]
mod paper_id_tests {
    use super::*;

    fn parse_ok(input: &str) -> PaperId {
        parse_paper_id(input).unwrap_or_else(|error| panic!("'{input}' should parse: {error}"))
    }

    #[test]
    fn parses_doi_forms_case_normalized() {
        // Bare, doi: prefix, and both doi.org URL hosts — lowercased.
        for input in [
            "10.1234/foo.Bar",
            "doi:10.1234/foo.Bar",
            "DOI:10.1234/foo.Bar",
            "https://doi.org/10.1234/foo.Bar",
            "https://dx.doi.org/10.1234/foo.Bar",
        ] {
            assert_eq!(
                parse_ok(input),
                PaperId::Doi("10.1234/foo.bar".to_string()),
                "input: {input}"
            );
        }
    }

    #[test]
    fn parses_arxiv_forms() {
        // Modern NNNN.NNNNN with optional version; bare, prefixed, URL.
        for (input, expected) in [
            ("2401.12345", "2401.12345"),
            ("arxiv:2401.12345", "2401.12345"),
            ("https://arxiv.org/abs/2401.12345", "2401.12345"),
            ("2401.12345v2", "2401.12345v2"),
            ("arXiv:2401.12345v2", "2401.12345v2"),
        ] {
            assert_eq!(
                parse_ok(input),
                PaperId::Arxiv(expected.to_string()),
                "input: {input}"
            );
        }
    }

    #[test]
    fn parses_pmid_forms() {
        // Digits-only, pmid: prefix (case-insensitive), PubMed URL.
        for input in [
            "12345678",
            "pmid:12345678",
            "PMID:12345678",
            "https://pubmed.ncbi.nlm.nih.gov/12345678/",
        ] {
            assert_eq!(
                parse_ok(input),
                PaperId::Pmid("12345678".to_string()),
                "input: {input}"
            );
        }
    }

    #[test]
    fn parses_pmcid_forms() {
        // PMC\d+ — case-insensitive input, normalized to uppercase,
        // pmc:/pmcid: prefixes, PMC article URLs.
        for input in [
            "PMC1234567",
            "pmc1234567",
            "pmc:1234567",
            "pmcid:PMC1234567",
            "https://pmc.ncbi.nlm.nih.gov/articles/PMC1234567/",
        ] {
            assert_eq!(
                parse_ok(input),
                PaperId::Pmcid("PMC1234567".to_string()),
                "input: {input}"
            );
        }
    }

    #[test]
    fn parses_openalex_forms() {
        // W-prefixed short form — case-insensitive W, URL.
        for input in [
            "W1234567890",
            "w1234567890",
            "https://openalex.org/work/W1234567890",
        ] {
            assert_eq!(
                parse_ok(input),
                PaperId::OpenAlex("W1234567890".to_string()),
                "input: {input}"
            );
        }
    }

    #[test]
    fn canonical_urls_per_kind() {
        assert_eq!(
            PaperId::Doi("10.1234/foo.bar".to_string()).canonical_url(),
            "https://doi.org/10.1234/foo.bar"
        );
        assert_eq!(
            PaperId::Arxiv("2401.12345v2".to_string()).canonical_url(),
            "https://arxiv.org/abs/2401.12345v2"
        );
        assert_eq!(
            PaperId::Pmid("12345678".to_string()).canonical_url(),
            "https://pubmed.ncbi.nlm.nih.gov/12345678/"
        );
        assert_eq!(
            PaperId::Pmcid("PMC1234567".to_string()).canonical_url(),
            "https://pmc.ncbi.nlm.nih.gov/articles/PMC1234567/"
        );
        assert_eq!(
            PaperId::OpenAlex("W1234567890".to_string()).canonical_url(),
            "https://openalex.org/work/W1234567890"
        );
    }

    #[test]
    fn stable_keys_are_prefixed_and_kind_distinct() {
        assert_eq!(
            stable_paper_key(&PaperId::Doi("10.1234/foo".to_string())),
            "doi:10.1234/foo"
        );
        assert_eq!(
            stable_paper_key(&PaperId::Arxiv("2401.12345".to_string())),
            "arxiv:2401.12345"
        );
        // The same numeric value under different kinds keys differently —
        // a PMID and a bare arXiv number are not the same paper.
        let pmid_key = stable_paper_key(&PaperId::Pmid("12345678".to_string()));
        let openalex_key = stable_paper_key(&PaperId::OpenAlex("W1234567890".to_string()));
        let pmcid_key = stable_paper_key(&PaperId::Pmcid("PMC1234567".to_string()));
        assert_eq!(pmid_key, "pmid:12345678");
        assert_eq!(openalex_key, "openalex:W1234567890");
        assert_eq!(pmcid_key, "pmcid:PMC1234567");
    }

    #[test]
    fn rejects_empty_and_garbage_naming_expected_forms() {
        for input in [
            "",
            "   ",
            "not-an-id",
            "10.",
            "w",
            "pmc",
            "1234.567",
            "1234.56789v",
        ] {
            match parse_paper_id(input) {
                Err(WebError::BadArgs(message)) => {
                    assert!(
                        message.contains("expected"),
                        "rejection names what was expected: {message}"
                    );
                    assert!(
                        message.contains(input.trim()) || input.trim().is_empty(),
                        "rejection names the input: {message}"
                    );
                }
                other => panic!("'{input}' should be rejected, got {other:?}"),
            }
        }
    }
}
