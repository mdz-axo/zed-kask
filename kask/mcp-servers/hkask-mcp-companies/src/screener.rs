//! Natural language screening prompt parser.
//!
//! Parses human-readable screening queries into FMP Stock Screener API parameters.
//! Handles compound patterns like "market cap over $10B and pe under 20" and
//! value suffixes ($10B = 10_000_000_000, 5% = 0.05).

use regex::Regex;

/// Build the "more than" operator array for a given FMP parameter name suffix.
fn more_than(param: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![
        ("above", param),
        ("over", param),
        ("more than", param),
        (">", param),
    ]
}

/// Build the "less than" operator array for a given FMP parameter name suffix.
fn less_than(param: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![
        ("below", param),
        ("under", param),
        ("less than", param),
        ("<", param),
    ]
}

/// Parse a natural language screening prompt into FMP screener API parameters.
///
/// Returns a JSON object with only the fields that were successfully parsed.
pub fn parse_screening_prompt(prompt: &str) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    let lower = prompt.to_lowercase();

    // ── Market cap ──────────────────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &[
            "market cap",
            "mkt cap",
            "market capitalization",
            "mkt capitalization",
        ],
        &more_than("marketCapMoreThan"),
        &less_than("marketCapLowerThan"),
        ValueKind::Dollar,
    );

    // ── Price ──────────────────────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["price", "share price", "stock price"],
        &more_than("priceMoreThan"),
        &less_than("priceLowerThan"),
        ValueKind::Dollar,
    );

    // ── Volume ─────────────────────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["volume"],
        &more_than("volumeMoreThan"),
        &less_than("volumeLowerThan"),
        ValueKind::Bare,
    );

    // ── Beta ───────────────────────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["beta"],
        &more_than("betaMoreThan"),
        &less_than("betaLowerThan"),
        ValueKind::Bare,
    );

    // ── P/E ratio ─────────────────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["pe", "pe ratio", "p/e", "p/e ratio", "price to earnings"],
        &more_than("peMoreThan"),
        &less_than("peLowerThan"),
        ValueKind::Bare,
    );

    // ── Dividend yield ─────────────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["dividend", "dividend yield", "yield", "div yield"],
        &more_than("dividendMoreThan"),
        &less_than("dividendLowerThan"),
        ValueKind::Percent,
    );

    // ── ROE ────────────────────────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["roe", "return on equity"],
        &more_than("roeMoreThan"),
        &[], // No less-than for ROE (FMP doesn't support it)
        ValueKind::Bare,
    );

    // ── ROIC ───────────────────────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["roic", "return on invested capital"],
        &more_than("roicMoreThan"),
        &[],
        ValueKind::Bare,
    );

    // ── Debt/Equity ────────────────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["debt/equity", "debt to equity", "d/e", "debt equity"],
        &[],
        &less_than("debtEquityRatioLowerThan"),
        ValueKind::Bare,
    );

    // ── Price/Book ─────────────────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &[
            "price to book",
            "p/b",
            "price/book",
            "pb",
            "price to book ratio",
        ],
        &[],
        &less_than("priceToBookLowerThan"),
        ValueKind::Bare,
    );

    // ── Sector ─────────────────────────────────────────────────
    parse_string(prompt, &mut map, "sector", "sector");

    // ── Industry ───────────────────────────────────────────────
    parse_string(prompt, &mut map, "industry", "industry");

    // ── Country ────────────────────────────────────────────────
    parse_string(prompt, &mut map, "country", "country");

    // ── Exchange ───────────────────────────────────────────────
    parse_string(prompt, &mut map, "exchange", "exchange");

    serde_json::Value::Object(map)
}

// ── Helpers ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum ValueKind {
    Dollar,  // "$10B", "$500M", "$1.5T", "$50"
    Percent, // "5%"
    Bare,    // "20", "1.5"
}

/// Parse a numeric criterion from the prompt.
///
/// Searches for patterns like "KEYWORD OPERATOR VALUE" using regex.
/// Uses the *first* match found for each keyword group, and within a group
/// picks the more-than or less-than variant depending on operator.
fn parse_numeric(
    prompt: &str,
    map: &mut serde_json::Map<String, serde_json::Value>,
    keywords: &[&str],
    more_than_ops: &[(&str, &str)], // (operator_word, fmp_param)
    less_than_ops: &[(&str, &str)],
    value_kind: ValueKind,
) {
    // Try each keyword
    for keyword in keywords {
        // Build a regex: keyword followed by operator and value
        // We allow whitespace and optional commas/ands between
        let pattern = build_numeric_pattern(keyword, more_than_ops, less_than_ops);
        if let Some(captures) = Regex::new(&pattern).ok().and_then(|re| re.captures(prompt)) {
            let value_str = captures.name("value").map(|m| m.as_str()).unwrap_or("");
            let operator = captures.name("op").map(|m| m.as_str()).unwrap_or("");

            // Parse the value
            let value = match value_kind {
                ValueKind::Dollar => parse_dollar_value(value_str),
                ValueKind::Percent => parse_percent_value(value_str),
                ValueKind::Bare => value_str.trim().parse::<f64>().ok(),
            };

            if let Some(v) = value {
                // Determine which param to use
                let param = find_param(operator.trim(), more_than_ops, less_than_ops);
                if let Some(p) = param {
                    map.insert(
                        p.to_string(),
                        serde_json::Value::Number(
                            serde_json::Number::from_f64(v)
                                .unwrap_or_else(|| serde_json::Number::from(0)),
                        ),
                    );
                    break; // Stop trying this keyword group, continue to next
                }
            }
        }
    }
}

/// Build a regex for a numeric criterion.
///
/// Matches: KEYWORD (optionally followed by "is") OPERATOR VALUE
fn build_numeric_pattern(
    keyword: &str,
    more_than_ops: &[(&str, &str)],
    less_than_ops: &[(&str, &str)],
) -> String {
    // Collect all operator strings, sorted longest-first for greedy matching
    let mut all_ops: Vec<&str> = more_than_ops
        .iter()
        .map(|(o, _)| *o)
        .chain(less_than_ops.iter().map(|(o, _)| *o))
        .collect();
    all_ops.sort_by_key(|b| std::cmp::Reverse(b.len()));

    let ops_alt = all_ops
        .iter()
        .map(|o| regex::escape(o))
        .collect::<Vec<_>>()
        .join("|");

    // Pattern: keyword (is)? (op) (value)
    // Value: $10B, $500M, $1.5T, 5%, 20, 1.5, or quoted
    let kw_escaped = regex::escape(keyword);
    format!(
        r"(?i){}\s*(?:is\s+)?(?P<op>{})(?:\s+than)?\s+(?P<value>\$?\d+[.,]?\d*\s*[BMKT%bmkt]?|\d+[.,]?\d*\s*%?)",
        kw_escaped, ops_alt
    )
}

/// Parse a string criterion from the prompt.
///
/// Matches patterns like "sector Technology", "industry \"Consumer Goods\"",
/// "exchange NASDAQ".
fn parse_string(
    prompt: &str,
    map: &mut serde_json::Map<String, serde_json::Value>,
    keyword: &str,
    fmp_param: &str,
) {
    if map.contains_key(fmp_param) {
        return;
    }

    let kw = regex::escape(keyword);

    // Try quoted value first: sector "Information Technology"
    let quoted_pattern = format!(r#"(?i){}\s+(?:"([^"]+)"|'([^']+)')"#, kw);
    if let Some(captures) = Regex::new(&quoted_pattern)
        .ok()
        .and_then(|re| re.captures(prompt))
    {
        let val = captures
            .get(1)
            .or_else(|| captures.get(2))
            .map(|m| m.as_str())
            .unwrap_or("");
        map.insert(
            fmp_param.to_string(),
            serde_json::Value::String(val.trim().to_string()),
        );
        return;
    }

    // Try bare single word: sector Technology
    // Match next word (or words until comma/and)
    let bare_pattern = format!(
        r"(?i){}\s+([a-zA-Z][a-zA-Z\s&.]+?)(?:\s*(?:,|and|with|$))",
        kw
    );
    if let Some(captures) = Regex::new(&bare_pattern)
        .ok()
        .and_then(|re| re.captures(prompt))
    {
        let val = captures.get(1).map(|m| m.as_str().trim()).unwrap_or("");
        if !val.is_empty() && !is_operator_word(val) && !is_numeric_word(val) {
            map.insert(
                fmp_param.to_string(),
                serde_json::Value::String(val.to_string()),
            );
        }
    }
}

/// Find which FMP param corresponds to the matched operator.
fn find_param<'a>(
    operator: &str,
    more_than_ops: &[(&str, &'a str)],
    less_than_ops: &[(&str, &'a str)],
) -> Option<&'a str> {
    for (op_word, param) in more_than_ops {
        if operator == *op_word {
            return Some(param);
        }
    }
    for (op_word, param) in less_than_ops {
        if operator == *op_word {
            return Some(param);
        }
    }
    None
}

/// Parse a dollar value: "$10B" → 10_000_000_000, "$500M" → 500_000_000,
/// "$1.5T" → 1_500_000_000_000, "$50" → 50.0.
fn parse_dollar_value(s: &str) -> Option<f64> {
    let s = s.trim();
    let s = s.strip_prefix('$').unwrap_or(s);

    let (num_str, multiplier) = if let Some(rest) = s.strip_suffix(|c: char| c == 'B' || c == 'b') {
        (rest.trim(), 1_000_000_000.0)
    } else if let Some(rest) = s.strip_suffix(|c: char| c == 'M' || c == 'm') {
        (rest.trim(), 1_000_000.0)
    } else if let Some(rest) = s.strip_suffix(|c: char| c == 'T' || c == 't') {
        (rest.trim(), 1_000_000_000_000.0)
    } else if let Some(rest) = s.strip_suffix(|c: char| c == 'K' || c == 'k') {
        (rest.trim(), 1_000.0)
    } else {
        (s.trim(), 1.0)
    };

    num_str.parse::<f64>().ok().map(|n| n * multiplier)
}

/// Parse a percentage value: "5%" → 0.05, "2.5%" → 0.025.
fn parse_percent_value(s: &str) -> Option<f64> {
    let s = s.trim().strip_suffix('%').unwrap_or(s).trim();
    s.parse::<f64>().ok().map(|n| n / 100.0)
}

/// Words that are operators or noise — skip them when parsing string values.
fn is_operator_word(s: &str) -> bool {
    matches!(
        s.to_lowercase().as_str(),
        "above"
            | "over"
            | "below"
            | "under"
            | "more"
            | "less"
            | "than"
            | "more than"
            | "less than"
            | "is"
            | "and"
            | "or"
            | "with"
            | "greater"
            | "lower"
            | "higher"
            | ">"
            | "<"
    )
}

/// Words that look like numbers — skip them as string values.
fn is_numeric_word(s: &str) -> bool {
    s.trim()
        .strip_prefix('$')
        .unwrap_or(s)
        .trim()
        .strip_suffix('%')
        .unwrap_or(s)
        .trim()
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_market_cap_and_pe() {
        let result = parse_screening_prompt(
            "large cap tech stocks with market cap over $10B and pe under 20",
        );
        let obj = result.as_object().unwrap();

        assert_eq!(
            obj.get("marketCapMoreThan").unwrap().as_f64().unwrap(),
            10_000_000_000.0
        );
        assert_eq!(obj.get("peLowerThan").unwrap().as_f64().unwrap(), 20.0);
    }

    #[test]
    fn test_parse_dividend_and_sector() {
        let result =
            parse_screening_prompt("sector Financials with dividend above 3% and beta below 1.5");
        let obj = result.as_object().unwrap();

        assert_eq!(obj.get("sector").unwrap().as_str().unwrap(), "Financials");
        assert_eq!(obj.get("dividendMoreThan").unwrap().as_f64().unwrap(), 0.03);
        assert_eq!(obj.get("betaLowerThan").unwrap().as_f64().unwrap(), 1.5);
    }

    #[test]
    fn test_parse_price_and_volume() {
        let result =
            parse_screening_prompt("price over $50 and volume above 1000000 on exchange NASDAQ");
        let obj = result.as_object().unwrap();

        assert_eq!(obj.get("priceMoreThan").unwrap().as_f64().unwrap(), 50.0);
        assert_eq!(
            obj.get("volumeMoreThan").unwrap().as_f64().unwrap(),
            1_000_000.0
        );
        assert_eq!(obj.get("exchange").unwrap().as_str().unwrap(), "NASDAQ");
    }

    #[test]
    fn test_parse_no_match_returns_empty() {
        let result = parse_screening_prompt("show me some good stocks to buy");
        let obj = result.as_object().unwrap();
        assert!(obj.is_empty());
    }

    #[test]
    fn test_parse_market_cap_below() {
        let result = parse_screening_prompt("mkt cap below $500M");
        let obj = result.as_object().unwrap();
        assert_eq!(
            obj.get("marketCapLowerThan").unwrap().as_f64().unwrap(),
            500_000_000.0
        );
    }
}
