//! Multi-provider fundamental research engine.
//!
//! Searches across Exa, Tavily, and Brave for company-specific
//! financial claims. Provides both raw text retrieval and lightweight
//! claim classification (FinGPT §3.4 — structured extraction without
//! full LLM inference).

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Research result types ──────────────────────────────────────────────

/// A single research claim from search results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchClaim {
    /// The claim text (e.g., "App Store revenue grew 12% YoY in Q2")
    pub text: String,
    /// Source URL
    pub source: String,
    /// Source title/domain
    pub source_title: String,
    /// Date if available
    pub date: Option<String>,
    /// Which provider found this claim
    pub provider: String,
}

/// Aggregated research results for a company.
#[derive(Debug, Clone, Serialize)]
pub struct ResearchResult {
    pub query: String,
    pub claims: Vec<ResearchClaim>,
    pub provider_summary: Vec<ProviderSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderSummary {
    pub provider: String,
    pub claims_found: usize,
    pub status: String, // "ok" or "error: ..."
}

// ── Multi-provider search ──────────────────────────────────────────────

/// Search for company research across all available providers.
/// Returns aggregated, de-duplicated claims.
///
/// Each optional API key enables the corresponding provider. Providers
/// without a key are silently skipped.
pub async fn search_fundamental(
    client: &reqwest::Client,
    symbol: &str,
    company_name: &str,
    research_query: &str,
    exa_key: Option<&str>,
    tavily_key: Option<&str>,
    brave_key: Option<&str>,
) -> ResearchResult {
    let query = format!(
        "{} {} {}",
        symbol.trim(),
        company_name.trim(),
        research_query.trim()
    );

    // Run all available providers in parallel
    let (exa_result, tavily_result, brave_result) = tokio::join!(
        async {
            match exa_key.filter(|k| !k.is_empty()) {
                Some(key) => search_exa(client, &query, key).await,
                None => Ok(Vec::new()),
            }
        },
        async {
            match tavily_key.filter(|k| !k.is_empty()) {
                Some(key) => search_tavily(client, &query, key).await,
                None => Ok(Vec::new()),
            }
        },
        async {
            match brave_key.filter(|k| !k.is_empty()) {
                Some(key) => search_brave(client, &query, key).await,
                None => Ok(Vec::new()),
            }
        },
    );

    // Collect all claims, tracking which providers contributed
    let mut all_claims: Vec<ResearchClaim> = Vec::new();
    let mut provider_summary: Vec<ProviderSummary> = Vec::new();

    for (provider_name, result) in [
        ("exa", exa_result),
        ("tavily", tavily_result),
        ("brave", brave_result),
    ] {
        match result {
            Ok(claims) => {
                let count = claims.len();
                all_claims.extend(claims);
                provider_summary.push(ProviderSummary {
                    provider: provider_name.to_string(),
                    claims_found: count,
                    status: "ok".to_string(),
                });
            }
            Err(e) => {
                provider_summary.push(ProviderSummary {
                    provider: provider_name.to_string(),
                    claims_found: 0,
                    status: format!("error: {e}"),
                });
            }
        }
    }

    // De-duplicate by source URL (keep first occurrence)
    let mut seen_urls = std::collections::HashSet::new();
    let mut deduped_claims: Vec<ResearchClaim> = Vec::new();
    for claim in all_claims {
        if seen_urls.insert(claim.source.clone()) {
            deduped_claims.push(claim);
        }
    }

    ResearchResult {
        query,
        claims: deduped_claims,
        provider_summary,
    }
}

// ── Exa search ────────────────────────────────────────────────────────

/// Search Exa API and extract claims from results.
async fn search_exa(
    client: &reqwest::Client,
    query: &str,
    api_key: &str,
) -> Result<Vec<ResearchClaim>, anyhow::Error> {
    let url = "https://api.exa.ai/search";
    let body = serde_json::json!({
        "query": query,
        "numResults": 5,
        "type": "auto",
        "contents": {"text": true}
    });

    let resp = client
        .post(url)
        .header("x-api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow!("Exa request failed: {e}"))?;

    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_else(|_| String::new());
    if !status.is_success() {
        return Err(anyhow!("Exa returned {status}: {body_text}"));
    }

    let parsed: Value =
        serde_json::from_str(&body_text).map_err(|e| anyhow!("Exa parse error: {e}"))?;

    parse_exa_results(&parsed)
}

fn parse_exa_results(parsed: &Value) -> Result<Vec<ResearchClaim>, anyhow::Error> {
    let results = parsed["results"]
        .as_array()
        .ok_or_else(|| anyhow!("Exa response missing 'results' array"))?;

    let mut claims = Vec::new();
    for result in results {
        let text = result["text"].as_str().unwrap_or("").to_string();
        let source = result["url"].as_str().unwrap_or("").to_string();
        let source_title = result["title"].as_str().unwrap_or("").to_string();
        let date = result["publishedDate"].as_str().map(|s| s.to_string());

        if text.is_empty() && source.is_empty() {
            continue;
        }

        // Truncate very long texts to a reasonable snippet length
        let snippet = if text.len() > 2000 {
            format!("{}…", &text[..2000])
        } else {
            text
        };

        claims.push(ResearchClaim {
            text: snippet,
            source,
            source_title,
            date,
            provider: "exa".to_string(),
        });
    }

    Ok(claims)
}

// ── Tavily search ─────────────────────────────────────────────────────

/// Search Tavily API and extract claims from results.
async fn search_tavily(
    client: &reqwest::Client,
    query: &str,
    api_key: &str,
) -> Result<Vec<ResearchClaim>, anyhow::Error> {
    let url = "https://api.tavily.com/search";
    let body = serde_json::json!({
        "api_key": api_key,
        "query": query,
        "search_depth": "advanced",
        "max_results": 5,
        "include_raw_content": true
    });

    let resp = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow!("Tavily request failed: {e}"))?;

    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_else(|_| String::new());
    if !status.is_success() {
        return Err(anyhow!("Tavily returned {status}: {body_text}"));
    }

    let parsed: Value =
        serde_json::from_str(&body_text).map_err(|e| anyhow!("Tavily parse error: {e}"))?;

    parse_tavily_results(&parsed)
}

fn parse_tavily_results(parsed: &Value) -> Result<Vec<ResearchClaim>, anyhow::Error> {
    let results = parsed["results"]
        .as_array()
        .ok_or_else(|| anyhow!("Tavily response missing 'results' array"))?;

    let mut claims = Vec::new();
    for result in results {
        let content = result["content"].as_str().unwrap_or("").to_string();
        let source = result["url"].as_str().unwrap_or("").to_string();
        let source_title = result["title"].as_str().unwrap_or("").to_string();

        if content.is_empty() && source.is_empty() {
            continue;
        }

        let snippet = if content.len() > 2000 {
            format!("{}…", &content[..2000])
        } else {
            content
        };

        claims.push(ResearchClaim {
            text: snippet,
            source,
            source_title,
            date: None,
            provider: "tavily".to_string(),
        });
    }

    Ok(claims)
}

// ── Brave search ──────────────────────────────────────────────────────

/// Search Brave Search API and extract claims from results.
async fn search_brave(
    client: &reqwest::Client,
    query: &str,
    api_key: &str,
) -> Result<Vec<ResearchClaim>, anyhow::Error> {
    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count=5",
        urlencoding(query)
    );

    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .header("Accept-Encoding", "gzip")
        .header("X-Subscription-Token", api_key)
        .send()
        .await
        .map_err(|e| anyhow!("Brave request failed: {e}"))?;

    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_else(|_| String::new());
    if !status.is_success() {
        return Err(anyhow!("Brave returned {status}: {body_text}"));
    }

    let parsed: Value =
        serde_json::from_str(&body_text).map_err(|e| anyhow!("Brave parse error: {e}"))?;

    parse_brave_results(&parsed)
}

fn urlencoding(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push('+'),
            _ => {
                encoded.push('%');
                encoded.push(hex_char(byte >> 4));
                encoded.push(hex_char(byte & 0x0F));
            }
        }
    }
    encoded
}

fn hex_char(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        _ => (b'A' + (n - 10)) as char,
    }
}

fn parse_brave_results(parsed: &Value) -> Result<Vec<ResearchClaim>, anyhow::Error> {
    let results = parsed["web"]["results"]
        .as_array()
        .ok_or_else(|| anyhow!("Brave response missing 'web.results' array"))?;

    let mut claims = Vec::new();
    for result in results {
        let text = result["description"].as_str().unwrap_or("").to_string();
        let source = result["url"].as_str().unwrap_or("").to_string();
        let source_title = result["title"].as_str().unwrap_or("").to_string();

        if text.is_empty() && source.is_empty() {
            continue;
        }

        let snippet = if text.len() > 2000 {
            format!("{}…", &text[..2000])
        } else {
            text
        };

        claims.push(ResearchClaim {
            text: snippet,
            source,
            source_title,
            date: None,
            provider: "brave".to_string(),
        });
    }

    Ok(claims)
}

// ── Claim classification (FinGPT §3.4 NER + Information Extraction) ──────────

/// Claim category from fundamental research.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ClaimCategory {
    RevenueGuidance,
    EarningsGuidance,
    CompetitiveThreat,
    RegulatoryAction,
    MergerAcquisition,
    ProductLaunch,
    ManagementChange,
    MarketExpansion,
    CostReduction,
    OtherFinancial,
    GeneralNews,
}

/// Structured extraction from a research claim.
#[derive(Debug, Clone, Serialize)]
pub struct ExtractedClaim {
    pub text: String,
    pub source: String,
    pub category: ClaimCategory,
    /// Numeric values found in the claim text (e.g., "12% growth" → 0.12).
    pub numeric_values: Vec<ExtractedNumber>,
    /// Ticker symbols mentioned.
    pub tickers: Vec<String>,
    /// Date mentioned in the claim, if any.
    pub date_mentioned: Option<String>,
}

/// A numeric value extracted from claim text.
#[derive(Debug, Clone, Serialize)]
pub struct ExtractedNumber {
    pub value: f64,
    pub unit: String,
    pub context: String,
}

/// Lightweight classifier using regex-based pattern matching.
/// Same NLP approach proven in screener.rs — structured extraction, not LLM inference.
pub struct ResearchClaimClassifier;

impl ResearchClaimClassifier {
    /// Classify and extract structured data from a research claim.
    pub fn classify(claim: &ResearchClaim) -> ExtractedClaim {
        let category = Self::categorize(&claim.text);
        let numeric_values = Self::extract_numerics(&claim.text);
        let tickers = Self::extract_tickers(&claim.text);
        let date_mentioned = Self::extract_date(&claim.text);

        ExtractedClaim {
            text: claim.text.clone(),
            source: claim.source.clone(),
            category,
            numeric_values,
            tickers,
            date_mentioned,
        }
    }

    /// Classify all claims and return the enhanced result.
    pub fn classify_all(result: &ResearchResult) -> EnhancedResearchResult {
        let classified: Vec<ExtractedClaim> = result.claims.iter().map(Self::classify).collect();

        // Count by category
        let mut category_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for c in &classified {
            let key = serde_json::to_string(&c.category)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string();
            *category_counts.entry(key).or_insert(0) += 1;
        }

        EnhancedResearchResult {
            query: result.query.clone(),
            claims: classified,
            provider_summary: result.provider_summary.clone(),
            category_summary: category_counts,
        }
    }

    fn categorize(text: &str) -> ClaimCategory {
        let lower = text.to_lowercase();

        // Revenue guidance patterns
        if lower.contains("revenue guid")
            || lower.contains("revenue forecast")
            || lower.contains("revenue grow")
            || lower.contains("sales forecast")
            || lower.contains("top-line")
        {
            return ClaimCategory::RevenueGuidance;
        }

        // Earnings guidance patterns
        if lower.contains("earnings guid")
            || lower.contains("eps forecast")
            || lower.contains("earnings per share")
            || lower.contains("profit forecast")
            || lower.contains("bottom-line")
            || lower.contains("net income")
        {
            return ClaimCategory::EarningsGuidance;
        }

        // Competitive threat
        if lower.contains("compet")
            || lower.contains("market share")
            || lower.contains("rival")
            || lower.contains("disrupt")
            || lower.contains("losing ground")
            || lower.contains("competitive pressure")
        {
            return ClaimCategory::CompetitiveThreat;
        }

        // Regulatory
        if lower.contains("regulat")
            || lower.contains("antitrust")
            || lower.contains("fine")
            || lower.contains("lawsuit")
            || lower.contains("sec ")
            || lower.contains("investigation")
        {
            return ClaimCategory::RegulatoryAction;
        }

        // M&A
        if lower.contains("acquis")
            || lower.contains("merger")
            || lower.contains("takeover")
            || lower.contains("buyout")
            || lower.contains("m&a")
        {
            return ClaimCategory::MergerAcquisition;
        }

        // Management change (before ProductLaunch — "launch" is too broad)
        if lower.contains("ceo")
            || lower.contains("cfo")
            || lower.contains("appoint")
            || lower.contains("resign")
            || lower.contains("executive")
        {
            return ClaimCategory::ManagementChange;
        }

        // Product launch — deliberately AFTER more specific patterns
        // because "launch" substring matches "launch investigation",
        // "launch tender offer", "launch cost-cutting", etc.
        if lower.contains("launch")
            || lower.contains("new product")
            || lower.contains("announce")
            || lower.contains("unveil")
        {
            return ClaimCategory::ProductLaunch;
        }

        // Market expansion
        if lower.contains("expand")
            || lower.contains("enter")
            || lower.contains("new market")
            || lower.contains("international")
        {
            return ClaimCategory::MarketExpansion;
        }

        // Cost reduction
        if lower.contains("layoff")
            || lower.contains("cost cut")
            || lower.contains("restructur")
            || lower.contains("efficiency")
            || lower.contains("margin improv")
        {
            return ClaimCategory::CostReduction;
        }

        // Financial-related patterns
        if lower.contains("revenue")
            || lower.contains("earn")
            || lower.contains("profit")
            || lower.contains("margin")
            || lower.contains("growth")
            || lower.contains("stock")
            || lower.contains("share")
            || lower.contains("dividend")
        {
            return ClaimCategory::OtherFinancial;
        }

        ClaimCategory::GeneralNews
    }

    fn extract_numerics(text: &str) -> Vec<ExtractedNumber> {
        let mut results = Vec::new();
        // Pattern: number followed by %, $, B, M, or a unit word
        let re =
            regex::Regex::new(r"(\d+\.?\d*)\s*(%|\$|billion|million|B|M|bps|points|percent|pct)")
                .unwrap();
        for cap in re.captures_iter(text) {
            if let (Some(num_str), Some(unit)) = (cap.get(1), cap.get(2))
                && let Ok(value) = num_str.as_str().parse::<f64>()
            {
                let adjusted = match unit.as_str() {
                    "%" | "percent" | "pct" => value / 100.0,
                    "bps" => value / 10000.0,
                    "points" | "point" => value,
                    "billion" | "B" => value * 1_000_000_000.0,
                    "million" | "M" => value * 1_000_000.0,
                    _ => value,
                };
                // Get context (up to 20 chars before the number)
                let start = cap.get(0).unwrap().start();
                let context_start = start.saturating_sub(20);
                let context = text[context_start..start].trim().to_string();
                results.push(ExtractedNumber {
                    value: adjusted,
                    unit: unit.as_str().to_string(),
                    context,
                });
            }
        }
        results
    }

    fn extract_tickers(text: &str) -> Vec<String> {
        // Match uppercase 1-5 letter tickers, possibly with exchange suffix (.L, .DE, etc.)
        let re = regex::Regex::new(r"\b([A-Z]{1,5}(?:\.[A-Z]{2})?)\b").unwrap();
        let known_not_tickers: std::collections::HashSet<&str> = [
            "CEO", "CFO", "COO", "CTO", "IPO", "ETF", "SEC", "ESG", "KYC", "AML", "FY", "Q1", "Q2",
            "Q3", "Q4", "YOY", "YTD", "MTD", "USD", "EUR", "GBP", "EBIT", "NYSE",
        ]
        .iter()
        .copied()
        .collect();

        re.captures_iter(text)
            .filter_map(|cap| cap.get(1))
            .map(|m| m.as_str().to_string())
            .filter(|t| !known_not_tickers.contains(t.as_str()))
            .collect()
    }

    fn extract_date(text: &str) -> Option<String> {
        let re = regex::Regex::new(r"\b(\d{4}-\d{2}-\d{2})\b").unwrap();
        re.captures(text)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_string())
    }
}

/// Enhanced research result with classified claims.
#[derive(Debug, Clone, Serialize)]
pub struct EnhancedResearchResult {
    pub query: String,
    pub claims: Vec<ExtractedClaim>,
    pub provider_summary: Vec<ProviderSummary>,
    /// Count of claims per category.
    pub category_summary: std::collections::HashMap<String, usize>,
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn research_claim_serialization() {
        let claim = ResearchClaim {
            text: "App Store revenue grew 12% YoY in Q2".to_string(),
            source: "https://example.com/report".to_string(),
            source_title: "Example Report".to_string(),
            date: Some("2025-06-01".to_string()),
            provider: "exa".to_string(),
        };

        let json = serde_json::to_string(&claim).expect("serialization should succeed");
        let parsed: ResearchClaim =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(parsed.text, claim.text);
        assert_eq!(parsed.source, claim.source);
        assert_eq!(parsed.date, Some("2025-06-01".to_string()));
        assert_eq!(parsed.provider, "exa");
    }

    #[test]
    fn provider_summary_construction() {
        let ok_summary = ProviderSummary {
            provider: "exa".to_string(),
            claims_found: 5,
            status: "ok".to_string(),
        };
        assert_eq!(ok_summary.provider, "exa");
        assert_eq!(ok_summary.claims_found, 5);
        assert_eq!(ok_summary.status, "ok");

        let err_summary = ProviderSummary {
            provider: "tavily".to_string(),
            claims_found: 0,
            status: "error: timeout".to_string(),
        };
        assert!(err_summary.status.starts_with("error:"));
    }

    #[test]
    fn parse_exa_results_basic() {
        let json = serde_json::json!({
            "results": [{
                "text": "Revenue grew 15% YoY",
                "url": "https://example.com/1",
                "title": "Report 1",
                "publishedDate": "2025-06-01"
            }]
        });
        let claims = parse_exa_results(&json).expect("should parse");
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].text, "Revenue grew 15% YoY");
        assert_eq!(claims[0].source, "https://example.com/1");
        assert_eq!(claims[0].source_title, "Report 1");
        assert_eq!(claims[0].date, Some("2025-06-01".to_string()));
        assert_eq!(claims[0].provider, "exa");
    }

    #[test]
    fn parse_brave_results_basic() {
        let json = serde_json::json!({
            "web": {
                "results": [{
                    "description": "Apple reported record earnings",
                    "url": "https://example.com/2",
                    "title": "Article 2"
                }]
            }
        });
        let claims = parse_brave_results(&json).expect("should parse");
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].text, "Apple reported record earnings");
        assert_eq!(claims[0].source, "https://example.com/2");
        assert_eq!(claims[0].source_title, "Article 2");
        assert_eq!(claims[0].date, None);
        assert_eq!(claims[0].provider, "brave");
    }

    #[test]
    fn urlencoding_basic() {
        assert_eq!(urlencoding("hello world"), "hello+world");
        assert_eq!(
            urlencoding("AAPL revenue guidance"),
            "AAPL+revenue+guidance"
        );
        assert_eq!(urlencoding("test&foo=bar"), "test%26foo%3Dbar");
    }
}
