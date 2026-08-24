//! Natural language screening prompt parser for the EODHD Screener API.
//!
//! Parses human-readable screening queries into EODHD Screener API filter
//! triples: `[field, operation, value]`. Supports all 14 EODHD screener
//! filter fields plus post-screen criteria (revenue growth, ROIC, ROE,
//! P/E, debt/equity, price/book, beta) that require per-company
//! fundamentals from the EODHD Fundamental Data API.
//!
//! ## EODHD Screener API
//!
//! Endpoint: `GET /api/screener`
//! Filters are passed as a JSON array of `[field, op, value]` triples,
//! AND-combined. Included in the All-In-One plan.
//! See: https://eodhd.com/financial-apis/stock-market-screener-api
//!
//! ## Field mapping
//!
//! | Prompt keyword       | EODHD field            | Kind    | Screener? |
//! |----------------------|------------------------|---------|-----------|
//! | market cap           | market_capitalization  | Dollar  | Yes       |
//! | price                | adjusted_close         | Dollar  | Yes       |
//! | volume               | avgvol_1d              | Bare    | Yes       |
//! | average volume       | avgvol_200d            | Bare    | Yes       |
//! | EPS                  | earnings_share         | Dollar  | Yes       |
//! | dividend yield       | dividend_yield         | Percent | Yes       |
//! | sector               | sector                 | String  | Yes       |
//! | industry             | industry               | String  | Yes       |
//! | exchange             | exchange               | String  | Yes       |
//! | daily change         | refund_1d_p            | Percent | Yes       |
//! | weekly change        | refund_5d_p            | Percent | Yes       |
//! | revenue growth       | revenue_growth         | Percent | No        |
//! | ROIC                 | roic                   | Bare    | No        |
//! | ROE                  | roe                    | Bare    | No        |
//! | P/E                  | pe_ratio               | Bare    | No        |
//! | debt/equity          | debt_equity            | Bare    | No        |
//! | price/book           | price_book             | Bare    | No        |
//! | beta                 | beta                   | Bare    | No        |

use regex::Regex;

/// Build the "more than" operator array for a given criteria field name.
fn more_than(param: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![
        ("above", param),
        ("over", param),
        ("more than", param),
        (">", param),
    ]
}

/// Build the "less than" operator array for a given criteria field name.
fn less_than(param: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![
        ("below", param),
        ("under", param),
        ("less than", param),
        ("<", param),
    ]
}

/// EODHD Screener API filter fields (the 14 fields the screener supports).
const EODHD_SCREENER_FIELDS: &[&str] = &[
    "exchange",
    "sub_exchange",
    "sector",
    "industry",
    "market_capitalization",
    "earnings_share",
    "dividend_yield",
    "adjusted_close",
    "avgvol_1d",
    "avgvol_200d",
    "refund_1d_p",
    "refund_5d_p",
    "code",
    "name",
];

/// String-valued screener fields (use `=` operation).
const STRING_SCREENER_FIELDS: &[&str] = &[
    "exchange",
    "sub_exchange",
    "sector",
    "industry",
    "code",
    "name",
];

/// Post-screen filter fields (not in EODHD screener — need per-company
/// fundamentals from the EODHD Fundamental Data API).
const POST_SCREEN_FIELDS: &[&str] = &[
    "revenue_growth",
    "roic",
    "roe",
    "pe_ratio",
    "debt_equity",
    "price_book",
    "beta",
];

/// Parse a natural language screening prompt into criteria.
///
/// Returns a JSON object with criteria using EODHD field names. Numeric
/// bounds use `_min`/`_max` suffixes (e.g., `market_capitalization_min`).
/// String fields use the bare field name (e.g., `exchange`).
///
/// The output can be converted to EODHD filter triples with
/// [`build_screener_filters`] and separated from post-screen criteria
/// with [`build_post_screen_filters`].
pub(crate) fn parse_screening_prompt(prompt: &str) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    let lower = prompt.to_lowercase();

    // ── Universe: market cap ────────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &[
            "market cap",
            "mkt cap",
            "market capitalization",
            "mkt capitalization",
        ],
        &more_than("market_capitalization_min"),
        &less_than("market_capitalization_max"),
        ValueKind::Dollar,
    );

    // ── Universe: price ─────────────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["price", "share price", "stock price"],
        &more_than("adjusted_close_min"),
        &less_than("adjusted_close_max"),
        ValueKind::Dollar,
    );

    // ── Universe: daily volume ──────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["volume", "daily volume", "trading volume"],
        &more_than("avgvol_1d_min"),
        &less_than("avgvol_1d_max"),
        ValueKind::Bare,
    );

    // ── Universe: average volume (200-day) ──────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &[
            "average volume",
            "avg volume",
            "200 day volume",
            "200-day volume",
        ],
        &more_than("avgvol_200d_min"),
        &less_than("avgvol_200d_max"),
        ValueKind::Bare,
    );

    // ── Company: EPS ────────────────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["eps", "earnings per share", "earnings share"],
        &more_than("earnings_share_min"),
        &less_than("earnings_share_max"),
        ValueKind::Dollar,
    );

    // ── Company: dividend yield ─────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["dividend yield", "div yield", "yield"],
        &more_than("dividend_yield_min"),
        &less_than("dividend_yield_max"),
        ValueKind::Percent,
    );

    // ── Company: daily price change ─────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["daily change", "1 day change", "1-day change", "day change"],
        &more_than("refund_1d_p_min"),
        &less_than("refund_1d_p_max"),
        ValueKind::Percent,
    );

    // ── Company: weekly price change ────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &[
            "weekly change",
            "5 day change",
            "5-day change",
            "week change",
        ],
        &more_than("refund_5d_p_min"),
        &less_than("refund_5d_p_max"),
        ValueKind::Percent,
    );

    // ── Post-screen: revenue growth ─────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &[
            "revenue growth",
            "sales growth",
            "revenue growth rate",
            "revenue cagr",
        ],
        &more_than("revenue_growth_min"),
        &[],
        ValueKind::Percent,
    );

    // ── Post-screen: ROIC ───────────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["roic", "return on invested capital"],
        &more_than("roic_min"),
        &[],
        ValueKind::Percent,
    );

    // ── Post-screen: ROE ────────────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["roe", "return on equity"],
        &more_than("roe_min"),
        &[],
        ValueKind::Percent,
    );

    // ── Post-screen: P/E ratio ──────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["pe", "pe ratio", "p/e", "p/e ratio", "price to earnings"],
        &more_than("pe_ratio_min"),
        &less_than("pe_ratio_max"),
        ValueKind::Bare,
    );

    // ── Post-screen: debt/equity ────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["debt/equity", "debt to equity", "d/e", "debt equity"],
        &[],
        &less_than("debt_equity_max"),
        ValueKind::Bare,
    );

    // ── Post-screen: price/book ─────────────────────────────────
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
        &less_than("price_book_max"),
        ValueKind::Bare,
    );

    // ── Post-screen: beta ───────────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["beta"],
        &more_than("beta_min"),
        &less_than("beta_max"),
        ValueKind::Bare,
    );

    // ── String fields ───────────────────────────────────────────
    // Try keyword-first ("sector Technology") then value-first
    // ("Technology sector") for sector and industry.
    parse_string(prompt, &mut map, "sector", "sector");
    parse_string_value_first(prompt, &mut map, "sector", "sector");
    parse_string(prompt, &mut map, "industry", "industry");
    parse_string_value_first(prompt, &mut map, "industry", "industry");

    // ── Exchange ──────────────────────────────────────────────
    // Try keyword-first ("exchange NASDAQ"), then value-first
    // ("NASDAQ exchange"), then geography pattern ("US stocks").
    parse_string(prompt, &mut map, "exchange", "exchange");
    parse_exchange_from_geography(prompt, &mut map);

    serde_json::Value::Object(map)
}

/// Build EODHD Screener API filter triples from parsed criteria.
///
/// Converts `_min`/`_max` suffixed fields to `["field", ">=", value]` /
/// `["field", "<", value]` and string fields to `["field", "=", value]`.
/// Only includes fields that the EODHD Screener API supports — post-screen
/// fields are excluded.
pub(crate) fn build_screener_filters(criteria: &serde_json::Value) -> Vec<serde_json::Value> {
    let empty = serde_json::Map::new();
    let obj = criteria.as_object().unwrap_or(&empty);
    let mut filters = Vec::new();

    for (key, value) in obj {
        if key == "limit" {
            continue;
        }

        if let Some(field) = key.strip_suffix("_min") {
            if EODHD_SCREENER_FIELDS.contains(&field) {
                filters.push(serde_json::json!([field, ">=", value]));
            }
        } else if let Some(field) = key.strip_suffix("_max") {
            if EODHD_SCREENER_FIELDS.contains(&field) {
                filters.push(serde_json::json!([field, "<", value]));
            }
        } else if STRING_SCREENER_FIELDS.contains(&key.as_str()) {
            filters.push(serde_json::json!([key.as_str(), "=", value]));
        }
    }

    filters
}

/// Extract post-screen filters (criteria not available in the EODHD
/// screener, requiring per-company fundamentals).
///
/// Returns a JSON object with the post-screen criteria, preserving the
/// `_min`/`_max` suffix convention.
pub(crate) fn build_post_screen_filters(criteria: &serde_json::Value) -> serde_json::Value {
    let empty = serde_json::Map::new();
    let obj = criteria.as_object().unwrap_or(&empty);
    let mut post = serde_json::Map::new();

    for (key, value) in obj {
        if key == "limit" {
            continue;
        }

        let base_field = key
            .strip_suffix("_min")
            .or_else(|| key.strip_suffix("_max"))
            .unwrap_or(key.as_str());

        if POST_SCREEN_FIELDS.contains(&base_field) {
            post.insert(key.clone(), value.clone());
        }
    }

    serde_json::Value::Object(post)
}

/// Split parsed criteria into screener-compatible and post-screen sets.
///
/// Convenience function that calls [`build_screener_filters`] and
/// [`build_post_screen_filters`] together.
pub(crate) fn split_criteria(
    criteria: &serde_json::Value,
) -> (Vec<serde_json::Value>, serde_json::Value) {
    (
        build_screener_filters(criteria),
        build_post_screen_filters(criteria),
    )
}

// ── Helpers ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum ValueKind {
    Dollar,  // "$10B", "$500M", "$1.5T", "$50"
    Percent, // "5%", "10%"
    Bare,    // "20", "1.5"
}

/// Parse a numeric criterion from the prompt.
///
/// Searches for patterns like "KEYWORD OPERATOR VALUE" using regex.
/// Runs two independent passes — one for more-than operators, one for
/// less-than — so compound prompts like "market cap above $500 million and
/// below $100 billion" capture both bounds.
fn parse_numeric(
    prompt: &str,
    map: &mut serde_json::Map<String, serde_json::Value>,
    keywords: &[&str],
    more_than_ops: &[(&str, &str)],
    less_than_ops: &[(&str, &str)],
    value_kind: ValueKind,
) {
    parse_numeric_direction(prompt, map, keywords, more_than_ops, value_kind);
    parse_numeric_direction(prompt, map, keywords, less_than_ops, value_kind);
}

/// Search for numeric criteria in one direction (more-than or less-than).
fn parse_numeric_direction(
    prompt: &str,
    map: &mut serde_json::Map<String, serde_json::Value>,
    keywords: &[&str],
    ops: &[(&str, &str)],
    value_kind: ValueKind,
) {
    if ops.is_empty() {
        return;
    }
    for keyword in keywords {
        let pattern = build_directional_pattern(keyword, ops);
        if let Some(captures) = Regex::new(&pattern).ok().and_then(|re| re.captures(prompt)) {
            let value_str = captures.name("value").map(|m| m.as_str()).unwrap_or("");
            let operator = captures.name("op").map(|m| m.as_str()).unwrap_or("");

            let value = match value_kind {
                ValueKind::Dollar => parse_dollar_value(value_str),
                ValueKind::Percent => parse_percent_value(value_str),
                ValueKind::Bare => value_str.trim().parse::<f64>().ok(),
            };

            if let Some(v) = value {
                let param = ops
                    .iter()
                    .find(|(op, _)| *op == operator.trim())
                    .map(|(_, param)| *param);
                if let Some(p) = param
                    && !map.contains_key(p)
                {
                    map.insert(
                        p.to_string(),
                        serde_json::Value::Number(
                            serde_json::Number::from_f64(v)
                                .unwrap_or_else(|| serde_json::Number::from(0)),
                        ),
                    );
                }
                break;
            }
        }
    }
}

/// Build a regex for a numeric criterion in one direction.
fn build_directional_pattern(keyword: &str, ops: &[(&str, &str)]) -> String {
    let mut all_ops: Vec<&str> = ops.iter().map(|(o, _)| *o).collect();
    all_ops.sort_by_key(|b| std::cmp::Reverse(b.len()));

    let ops_alt = all_ops
        .iter()
        .map(|o| regex::escape(o))
        .collect::<Vec<_>>()
        .join("|");

    let kw_escaped = regex::escape(keyword);
    format!(
        r"(?i){}\s*(?:is\s+)?(?P<op>{})(?:\s+than)?\s+(?P<value>\$?\d+(?:[.,]\d+)?\s*[BMKT%bmkt]?)",
        kw_escaped, ops_alt
    )
}

/// Parse a string criterion from the prompt.
fn parse_string(
    prompt: &str,
    map: &mut serde_json::Map<String, serde_json::Value>,
    keyword: &str,
    field: &str,
) {
    if map.contains_key(field) {
        return;
    }

    let kw = regex::escape(keyword);

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
            field.to_string(),
            serde_json::Value::String(val.trim().to_string()),
        );
        return;
    }

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
                field.to_string(),
                serde_json::Value::String(val.to_string()),
            );
        }
    }
}

/// Parse a string criterion where the value comes BEFORE the keyword
/// (e.g., "Technology sector" instead of "sector Technology").
fn parse_string_value_first(
    prompt: &str,
    map: &mut serde_json::Map<String, serde_json::Value>,
    keyword: &str,
    field: &str,
) {
    if map.contains_key(field) {
        return;
    }

    let kw = regex::escape(keyword);
    let pattern = format!(
        r"(?i)([a-zA-Z][a-zA-Z\s&.]+?)\s+{}(?:\s*(?:,|and|with|$))",
        kw
    );
    if let Some(captures) = Regex::new(&pattern).ok().and_then(|re| re.captures(prompt)) {
        let val = captures.get(1).map(|m| m.as_str().trim()).unwrap_or("");
        if !val.is_empty() && !is_operator_word(val) && !is_numeric_word(val) {
            map.insert(
                field.to_string(),
                serde_json::Value::String(val.to_string()),
            );
        }
    }
}

/// Extract exchange code from geography patterns like "US stocks",
/// "NYSE listed", "NASDAQ stocks".
fn parse_exchange_from_geography(
    prompt: &str,
    map: &mut serde_json::Map<String, serde_json::Value>,
) {
    if map.contains_key("exchange") {
        return;
    }

    let pattern = r"(?i)\b(us|usa|nyse|nasdaq|amex|lse|to|tsx|bats|neo)\b\s+(?:stocks?|exchange|listed|traded)";
    if let Some(captures) = Regex::new(pattern).ok().and_then(|re| re.captures(prompt)) {
        let val = captures.get(1).map(|m| m.as_str()).unwrap_or("");
        if !val.is_empty() {
            map.insert(
                "exchange".to_string(),
                serde_json::Value::String(val.to_uppercase()),
            );
        }
    }
}

/// Parse a dollar value: "$10B" → 10_000_000_000, "$500M" → 500_000_000.
fn parse_dollar_value(s: &str) -> Option<f64> {
    let s = s.trim();
    let s = s.strip_prefix('$').unwrap_or(s);
    // Strip commas (thousands separators or trailing list commas)
    let s = s.replace(',', "");
    let s = s.trim();

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

/// Parse a percentage value: "5%" → 0.05, "10%" → 0.10.
fn parse_percent_value(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', "");
    let s = s.strip_suffix('%').unwrap_or(&s).trim();
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
