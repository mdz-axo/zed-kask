use super::{ProviderSearchOutput, WebError, WebSearchProvider};
use crate::research::types::*;
use async_trait::async_trait;
use std::time::Duration;

/// arXiv preprint search provider.
///
/// Free: no API key required. Queries `export.arxiv.org/api/query` which returns
/// Atom XML. Parses entry titles, authors, summaries (abstracts), PDF links,
/// and published dates into SearchResult structs.
pub(crate) struct ArxivProvider {
    client: reqwest::Client,
    pacer: super::ProviderPacer,
    /// The query endpoint; a test points it at a fixture.
    endpoint: String,
}

impl ArxivProvider {
    pub fn new() -> Result<Self, WebError> {
        Ok(Self {
            client: super::provider_http_client()?,
            pacer: super::ProviderPacer::new(Duration::from_secs(ARXIV_MIN_REQUEST_INTERVAL_SECS)),
            endpoint: ARXIV_API_BASE.to_string(),
        })
    }
}

/// Field prefixes of arXiv's query syntax (API user manual, appendix 5.1).
const ARXIV_FIELD_PREFIXES: &[&str] = &[
    "ti:",
    "au:",
    "abs:",
    "co:",
    "jr:",
    "cat:",
    "rn:",
    "id:",
    "all:",
    "submittedDate:",
];

/// The `search_query` sent to arXiv for a caller's query.
///
/// A query that already uses arXiv's syntax (a field prefix, a Boolean
/// operator, a quoted phrase or parentheses) is sent as written.
///
/// Plain words are not: arXiv reads bare words as alternatives, so
/// "reciprocal rank fusion" matched 122,634 papers and one of the first
/// five contained all three words (measured 2026-11-12). Requiring every
/// word instead returned nothing for a ten-word question. The form sent
/// asks for both in one request, `(all:a AND all:b) OR (a b)`. Measured:
/// for the three-word query the first four results contain every word;
/// for the ten-word question the results and their order are the same as
/// for the bare words. A single word is sent as `all:word`, the form the
/// arXiv manual gives for a one-word search (not measured against the
/// bare word).
fn arxiv_search_query(query: &str) -> String {
    let structured = query.contains(['"', '(', ')'])
        || query.split_whitespace().any(|word| {
            matches!(word, "AND" | "OR" | "ANDNOT")
                || ARXIV_FIELD_PREFIXES
                    .iter()
                    .any(|prefix| word.starts_with(prefix))
        });
    if structured {
        return query.to_string();
    }
    // Punctuation separates words, so nothing in the bare tail can be read
    // as query syntax.
    let words: Vec<&str> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let mut required: Vec<String> = Vec::new();
    for word in &words {
        let term = format!("all:{word}");
        if !required.iter().any(|seen| seen.eq_ignore_ascii_case(&term)) {
            required.push(term);
        }
    }
    match required.as_slice() {
        // Nothing searchable in the query: arXiv reports the problem.
        [] => query.to_string(),
        [only] => only.clone(),
        _ => format!("({}) OR ({})", required.join(" AND "), words.join(" ")),
    }
}

const ARXIV_API_BASE: &str = "https://export.arxiv.org/api/query";

/// arXiv's published client guidance: one request every 3 seconds. They
/// enforce it with fast 429s AND slow-walking (observed 2026-11-01: a
/// 15.8s response carrying a 429, and 20s stalls); the pacer keeps our
/// side of the contract so parallel fan-out cannot trigger the enforcement.
const ARXIV_MIN_REQUEST_INTERVAL_SECS: u64 = 3;

/// arXiv's compound-fusion budget: its healthy p50 is sub-second, and under
/// throttling it slow-walks — a 10s budget fails a stalled leg twice as
/// fast as the pool-wide 20s (observed 2026-11-01: arXiv stalls dominated
/// fusion latency, three of eight calls completing at exactly 20s).
const ARXIV_COMPOUND_TIMEOUT_SECS: u64 = 10;
#[async_trait]
impl WebSearchProvider for ArxivProvider {
    fn kind(&self) -> &str {
        "arxiv"
    }
    fn capabilities(&self) -> Vec<SearchCapability> {
        vec![SearchCapability::Keyword, SearchCapability::Semantic]
    }
    /// arXiv overrides the flat pool budget: see `ARXIV_COMPOUND_TIMEOUT_SECS`.
    fn compound_timeout_secs(&self) -> u64 {
        ARXIV_COMPOUND_TIMEOUT_SECS
    }

    async fn search(&self, query: &SearchQuery) -> Result<ProviderSearchOutput, WebError> {
        // Outbound pacing at arXiv's own published floor; a queued request
        // that cannot start within this provider's compound budget is
        // refused honestly (RateLimited, request not sent) instead of
        // surfacing as an opaque timeout after burning the whole budget.
        self.pacer
            .wait_turn(Duration::from_secs(ARXIV_COMPOUND_TIMEOUT_SECS))
            .await?;

        let params: Vec<(&str, String)> = vec![
            ("search_query", arxiv_search_query(&query.query)),
            ("max_results", query.num_results.to_string()),
            ("sortBy", "relevance".to_string()),
        ];

        let resp = self
            .client
            .get(&self.endpoint)
            .query(&params)
            .send()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("arXiv request failed: {e}")))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Arxiv body read failed: {e}")))?;
        if !status.is_success() {
            return Err(match status.as_u16() {
                429 => WebError::RateLimited(format!("arXiv rate limited: {status}")),
                503 => WebError::ProviderUnavailable(format!(
                    "arXiv temporarily unavailable: {status}"
                )),
                _ => WebError::ProviderError(format!(
                    "arXiv error {status}: {}",
                    hkask_inference::openai_compat::sanitize_error_body(&body)
                )),
            });
        }

        // Parse Atom XML — extract entries manually to avoid heavy XML deps
        let results = parse_arxiv_atom(&body);

        Ok(ProviderSearchOutput {
            results,
            ..Default::default()
        })
    }

    async fn health(&self) -> Result<(), WebError> {
        let resp = self
            .client
            .get(ARXIV_API_BASE)
            .query(&[("search_query", "all:test"), ("max_results", "1")])
            .send()
            .await
            .map_err(|e| {
                WebError::ProviderUnavailable(format!("arXiv health check failed: {e}"))
            })?;
        if resp.status().is_success() || resp.status().as_u16() == 503 {
            // 503 is arXiv's "busy" response — still alive
            Ok(())
        } else {
            Err(WebError::ProviderUnavailable(format!(
                "arXiv health check returned {}",
                resp.status()
            )))
        }
    }
}

/// Parse arXiv Atom XML response into SearchResults.
///
/// Avoids pulling in a full XML parser — arXiv's Atom format is predictable
/// enough for simple string extraction between known tags.
fn parse_arxiv_atom(xml: &str) -> Vec<SearchResult> {
    let mut results = Vec::new();

    // Split on <entry> tags to isolate each paper
    for entry_str in xml.split("<entry>").skip(1) {
        // Close the entry at </entry>
        let entry = match entry_str.split("</entry>").next() {
            Some(e) => e,
            None => continue,
        };

        let title = extract_tag(entry, "title");
        let summary = extract_tag(entry, "summary");
        let published = extract_tag(entry, "published");

        // Extract authors from <author><name>...</name></author> blocks
        let authors: Vec<String> = entry_str
            .split("<author>")
            .skip(1)
            .filter_map(|auth_block| {
                let name = extract_tag(auth_block, "name");
                if name.is_empty() { None } else { Some(name) }
            })
            .collect();

        // arXiv ID from <id> tag (full URL like https://arxiv.org/abs/...)
        let arxiv_url = extract_tag(entry, "id");
        // PDF link from <link title="pdf" href="..."/>
        // Search the whole entry (not line-by-line) — arXiv's Atom XML may
        // emit <link> tags with attributes spanning multiple lines.
        // Scope the href search to the <link> tag containing title="pdf"
        // to avoid matching an href from a different <link> tag.
        let pdf_url = entry.find("title=\"pdf\"").and_then(|tag_pos| {
            // Find the <link tag start before title="pdf".
            let link_start = entry[..tag_pos].rfind("<link")?;
            // Find the tag end (> or />) after title="pdf".
            let link_end_rel = entry[tag_pos..].find('>')?;
            let link_end = tag_pos + link_end_rel + 1;
            let link_tag = &entry[link_start..link_end];
            // Now search for href="..." within this single <link> tag.
            let href_marker = "href=\"";
            let href_pos = link_tag.find(href_marker)?;
            let start = href_pos + href_marker.len();
            let end = link_tag[start..].find('"')?;
            Some(link_tag[start..start + end].to_string())
        });

        let url = if !pdf_url.as_ref().is_none_or(|u| u.is_empty()) {
            pdf_url.unwrap_or(arxiv_url)
        } else {
            arxiv_url
        };

        if title.is_empty() {
            continue;
        }

        // Build description: authors + date + abstract snippet
        let mut desc_parts: Vec<String> = Vec::new();
        if !authors.is_empty() {
            desc_parts.push(authors.join(", "));
        }
        if !published.is_empty() {
            // Extract just the date part (YYYY-MM-DD)
            let date = published
                .split('T')
                .next()
                .unwrap_or(&published)
                .to_string();
            desc_parts.push(format!("({date})"));
        }
        if !summary.is_empty() {
            let short_summary: String = summary.chars().take(300).collect();
            desc_parts.push(short_summary);
        }

        let description = if desc_parts.is_empty() {
            None
        } else {
            Some(desc_parts.join(" — "))
        };

        results.push(SearchResult {
            title,
            url,
            description,
            source: Some("arXiv".to_string()),
            published: if published.is_empty() {
                None
            } else {
                Some(published.split('T').next().unwrap_or("").to_string())
            },
            oa_pdf_url: None,
        });
    }

    results
}

/// Extract text content between XML tags like <title>...</title>.
/// Strips leading/trailing whitespace, normalizes newlines, and decodes
/// common XML entities (&amp;, &lt;, &gt;, &quot;, &apos;).
fn extract_tag(xml: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");

    let start = match xml.find(&open) {
        Some(pos) => pos + open.len(),
        None => return String::new(),
    };
    let end = match xml[start..].find(&close) {
        Some(pos) => start + pos,
        None => return String::new(),
    };

    let raw = xml[start..end].trim().replace('\n', " ");
    let decoded = decode_xml_entities(&raw);
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Decode common XML character entities into their literal characters.
fn decode_xml_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: plain words are sent as one query that ranks papers with
    /// every word first and still matches papers with some of them.
    #[test]
    fn plain_words_ask_for_all_of_them_first_and_any_of_them_after() {
        assert_eq!(
            arxiv_search_query("reciprocal rank fusion"),
            "(all:reciprocal AND all:rank AND all:fusion) OR (reciprocal rank fusion)"
        );
        assert_eq!(arxiv_search_query("transformers"), "all:transformers");
        // Punctuation separates words; a repeated word is required once
        // and kept in the bare tail, which is the caller's words as given.
        assert_eq!(
            arxiv_search_query("Learning-to-rank: learning, to rank?"),
            "(all:Learning AND all:to AND all:rank) OR (Learning to rank learning to rank)"
        );
        // The ten-word question measured on 2026-11-12.
        assert_eq!(
            arxiv_search_query("how does reciprocal rank fusion compare to learning to rank"),
            "(all:how AND all:does AND all:reciprocal AND all:rank AND all:fusion AND \
             all:compare AND all:to AND all:learning) OR \
             (how does reciprocal rank fusion compare to learning to rank)"
        );
    }

    /// expect: a query already written in arXiv's syntax is sent as
    /// written: a field prefix, a Boolean operator, a phrase, a group.
    #[test]
    fn a_query_in_arxiv_syntax_is_sent_as_written() {
        for query in [
            "au:dunning",
            "ti:\"reciprocal rank fusion\"",
            "electron AND proton",
            "au:del_maestro ANDNOT (ti:checkerboard OR ti:Pyrochlore)",
            "cat:cs.IR fusion",
            "\"learning to rank\"",
        ] {
            assert_eq!(arxiv_search_query(query), query, "{query}");
        }
        // Lowercase "and"/"or" are words, not operators.
        assert_eq!(
            arxiv_search_query("search and rescue"),
            "(all:search AND all:and AND all:rescue) OR (search and rescue)"
        );
        // Nothing to search for is left for arXiv to report.
        assert_eq!(arxiv_search_query("?!"), "?!");
    }

    /// expect: the query built above is what reaches arXiv, percent-encoded
    /// once, with the result count and relevance order asked for; the Atom
    /// answer is parsed into results.
    #[tokio::test]
    async fn search_sends_the_built_query_and_parses_the_answer() {
        let atom = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<feed xmlns=\"http://www.w3.org/2005/Atom\">\n\
            <entry>\n<id>http://arxiv.org/abs/2501.00001v1</id>\n\
            <published>2025-01-01T00:00:00Z</published>\n\
            <title>MMMORRF: Multimodal Multilingual Modularized\n  Reciprocal Rank Fusion</title>\n\
            <summary>A summary.</summary>\n<author><name>A. Author</name></author>\n\
            <link title=\"pdf\" href=\"http://arxiv.org/pdf/2501.00001v1\" rel=\"related\"/>\n\
            </entry>\n</feed>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/atom+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{atom}",
            atom.len()
        )
        .into_bytes();
        let (base, requests) = crate::test_support::http_fixture_capturing_requests(response).await;
        let mut provider = ArxivProvider::new().expect("provider");
        provider.endpoint = format!("{base}/api/query");

        let output = provider
            .search(&SearchQuery {
                query: "reciprocal rank fusion".to_string(),
                num_results: 5,
                include_domains: Vec::new(),
                exclude_domains: Vec::new(),
                freshness: None,
            })
            .await
            .expect("search succeeds");
        assert_eq!(output.results.len(), 1);
        assert_eq!(
            output.results[0].title,
            "MMMORRF: Multimodal Multilingual Modularized Reciprocal Rank Fusion"
        );
        assert_eq!(output.results[0].url, "http://arxiv.org/pdf/2501.00001v1");
        assert_eq!(output.results[0].published.as_deref(), Some("2025-01-01"));

        let sent = requests.lock().expect("requests").clone();
        assert_eq!(sent.len(), 1, "one request");
        let url = reqwest::Url::parse(&format!("{base}{}", sent[0].target)).expect("target");
        let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        assert_eq!(
            pairs,
            [
                (
                    "search_query".to_string(),
                    "(all:reciprocal AND all:rank AND all:fusion) OR (reciprocal rank fusion)"
                        .to_string()
                ),
                ("max_results".to_string(), "5".to_string()),
                ("sortBy".to_string(), "relevance".to_string()),
            ]
        );
    }
}
