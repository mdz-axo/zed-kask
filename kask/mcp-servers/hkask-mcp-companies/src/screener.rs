//! Natural language screening prompt parser for the EODHD Screener API.
//!
//! Parses human-readable screening queries into EODHD Screener API filter
//! triples: `[field, operation, value]`. Supports the EODHD screener filter
//! fields plus post-screen criteria (revenue growth, ROIC, ROE, P/E,
//! debt/equity, price/book, beta) that require per-company fundamentals from
//! the EODHD Fundamental Data API.
//!
//! ## Grammar
//!
//! - Keywords: field names in space or underscore form — "market cap" and
//!   "market_capitalization" both select the market-cap field.
//! - Operators: above/over/more than/greater than/higher than/>/>=/at least
//!   (lower bound); below/under/less than/lower than/fewer than/</<=/at most
//!   (upper bound); "between X and Y" (both bounds); equals/is/= for string
//!   fields.
//! - Values: optional `$`, thousands commas, a decimal point, and a scale
//!   suffix — `$10B`, `2,000,000,000`, `2 billion`, `1M`, `5%`. The suffix
//!   binds to the number it follows: "between 2 and 200 billion" parses as
//!   min 2, max 2e11.
//! - Geography: country/region names and major exchange codes ("US", "Japan",
//!   "Europe", "NASDAQ", …) map to EODHD exchange codes under the `exchanges`
//!   key; the tool handler fans out one query per code. "US" matches
//!   case-sensitively so the pronoun "us" cannot select US listings.
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
//! | Prompt keyword                          | EODHD field            | Kind        | Screener? |
//! |-----------------------------------------|------------------------|-------------|-----------|
//! | market cap / market_capitalization      | market_capitalization  | Dollar      | Yes       |
//! | price / adjusted_close                  | adjusted_close         | Dollar      | Yes       |
//! | volume / avgvol_1d                      | avgvol_1d              | Bare        | Yes       |
//! | average volume / avgvol_200d            | avgvol_200d            | Bare        | Yes       |
//! | EPS / earnings_share                    | earnings_share         | Dollar      | Yes       |
//! | dividend yield                          | dividend_yield         | Percent     | Yes       |
//! | sector                                  | sector                 | String      | Yes       |
//! | industry                                | industry               | String      | Yes       |
//! | geography / exchange phrase             | exchanges              | String list | Handler   |
//! | daily change / refund_1d_p              | refund_1d_p            | Percent     | Yes       |
//! | weekly change / refund_5d_p             | refund_5d_p            | Percent     | Yes       |
//! | revenue growth                          | revenue_growth         | Percent     | No        |
//! | ROIC                                    | roic                   | Percent     | No        |
//! | ROE                                     | roe                    | Percent     | No        |
//! | P/E                                     | pe_ratio               | Bare        | No        |
//! | debt/equity                             | debt_equity            | Bare        | No        |
//! | price/book                              | price_book             | Bare        | No        |
//! | beta                                    | beta                   | Bare        | No        |

use regex::Regex;

/// Build the "more than" operator array for a given criteria field name.
fn more_than(param: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![
        ("above", param),
        ("over", param),
        ("more", param),
        ("greater", param),
        ("higher", param),
        ("at least", param),
        (">", param),
        (">=", param),
        ("≥", param),
    ]
}

/// Build the "less than" operator array for a given criteria field name.
fn less_than(param: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![
        ("below", param),
        ("under", param),
        ("less", param),
        ("lower", param),
        ("fewer", param),
        ("at most", param),
        ("up to", param),
        ("<", param),
        ("<=", param),
        ("≤", param),
    ]
}

/// EODHD Screener API filter fields (the fields the screener supports).
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

/// String-valued screener fields (use `=` operation). `exchange` is absent:
/// exchange criteria are handler-owned (`exchanges` array, one query per
/// code), never emitted as a filter triple.
const STRING_SCREENER_FIELDS: &[&str] = &["sub_exchange", "sector", "industry", "code", "name"];

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

/// EODHD exchange codes for "Europe" (major Western, Nordic, and Central
/// European venues, London included).
const EUROPE_CODES: &[&str] = &[
    "LSE", "IL", "F", "XETRA", "PA", "AS", "BR", "LS", "MI", "MC", "SW", "ST", "OL", "HE", "CO",
    "VI", "IR", "LU", "WAR", "PR", "AT",
];

/// Geographic entity → EODHD exchange codes.
///
/// Verification tiers (2026-09-09): US, LSE, XETRA, TO, WAR verified against
/// EODHD's Exchanges API documentation; NEO, IL, F, PR observed in live
/// screener output; the remaining codes follow EODHD symbol-suffix
/// conventions and are unverified — a wrong code surfaces as a zero-match
/// exchange in the tool's `exchange_match_counts`.
///
/// "US" is matched case-sensitively so the pronoun "us" cannot select US
/// listings; every other pattern matches case-insensitively.
const GEOGRAPHY: &[(&str, bool, &[&str])] = &[
    // Regions
    ("europe", false, EUROPE_CODES),
    ("european", false, EUROPE_CODES),
    // United States (the composite US code covers NYSE/NASDAQ/AMEX/OTC)
    ("US", true, &["US"]),
    ("usa", false, &["US"]),
    ("united states", false, &["US"]),
    ("america", false, &["US"]),
    ("american", false, &["US"]),
    ("new york", false, &["US"]),
    ("nyse", false, &["US"]),
    ("nasdaq", false, &["US"]),
    ("amex", false, &["US"]),
    // Japan
    ("japan", false, &["JP"]),
    ("japanese", false, &["JP"]),
    ("tokyo", false, &["JP"]),
    ("jp", false, &["JP"]),
    // Canada
    ("canada", false, &["TO", "V", "NEO", "CNQ"]),
    ("canadian", false, &["TO", "V", "NEO", "CNQ"]),
    ("toronto", false, &["TO"]),
    ("tsx", false, &["TO"]),
    ("vancouver", false, &["V"]),
    // Mexico
    ("mexico", false, &["MX"]),
    ("mexican", false, &["MX"]),
    ("bmv", false, &["MX"]),
    ("mx", false, &["MX"]),
    // United Kingdom
    ("uk", false, &["LSE", "IL"]),
    ("united kingdom", false, &["LSE", "IL"]),
    ("britain", false, &["LSE", "IL"]),
    ("british", false, &["LSE", "IL"]),
    ("england", false, &["LSE", "IL"]),
    ("london", false, &["LSE"]),
    ("lse", false, &["LSE"]),
    // Germany
    ("germany", false, &["F", "XETRA"]),
    ("german", false, &["F", "XETRA"]),
    ("frankfurt", false, &["F"]),
    ("xetra", false, &["XETRA"]),
    // France
    ("france", false, &["PA"]),
    ("french", false, &["PA"]),
    ("paris", false, &["PA"]),
    // Netherlands
    ("netherlands", false, &["AS"]),
    ("dutch", false, &["AS"]),
    ("amsterdam", false, &["AS"]),
    // Belgium
    ("belgium", false, &["BR"]),
    ("brussels", false, &["BR"]),
    // Portugal
    ("portugal", false, &["LS"]),
    ("lisbon", false, &["LS"]),
    // Italy
    ("italy", false, &["MI"]),
    ("italian", false, &["MI"]),
    ("milan", false, &["MI"]),
    // Spain
    ("spain", false, &["MC"]),
    ("spanish", false, &["MC"]),
    ("madrid", false, &["MC"]),
    // Switzerland
    ("switzerland", false, &["SW"]),
    ("swiss", false, &["SW"]),
    ("zurich", false, &["SW"]),
    // Nordics
    ("sweden", false, &["ST"]),
    ("swedish", false, &["ST"]),
    ("stockholm", false, &["ST"]),
    ("norway", false, &["OL"]),
    ("norwegian", false, &["OL"]),
    ("oslo", false, &["OL"]),
    ("denmark", false, &["CO"]),
    ("danish", false, &["CO"]),
    ("copenhagen", false, &["CO"]),
    ("finland", false, &["HE"]),
    ("finnish", false, &["HE"]),
    ("helsinki", false, &["HE"]),
    // Other Europe
    ("austria", false, &["VI"]),
    ("vienna", false, &["VI"]),
    ("ireland", false, &["IR"]),
    ("irish", false, &["IR"]),
    ("dublin", false, &["IR"]),
    ("luxembourg", false, &["LU"]),
    ("poland", false, &["WAR"]),
    ("polish", false, &["WAR"]),
    ("warsaw", false, &["WAR"]),
    ("czech", false, &["PR"]),
    ("prague", false, &["PR"]),
    ("greece", false, &["AT"]),
    ("athens", false, &["AT"]),
];

/// Parse a natural language screening prompt into criteria.
///
/// Returns a JSON object with criteria using EODHD field names. Numeric
/// bounds use `_min`/`_max` suffixes (e.g., `market_capitalization_min`).
/// String fields use the bare field name (e.g., `sector`). Geography parses
/// to an `exchanges` array of EODHD exchange codes.
///
/// The output can be converted to EODHD filter triples with
/// [`build_screener_filters`] and separated from post-screen criteria with
/// [`build_post_screen_filters`].
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
            "mcap",
            "market_capitalization",
            "mkt_capitalization",
        ],
        &more_than("market_capitalization_min"),
        &less_than("market_capitalization_max"),
        ValueKind::Dollar,
    );

    // ── Universe: price ─────────────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["price", "share price", "stock price", "adjusted_close"],
        &more_than("adjusted_close_min"),
        &less_than("adjusted_close_max"),
        ValueKind::Dollar,
    );

    // ── Universe: daily volume ──────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["volume", "daily volume", "trading volume", "avgvol_1d"],
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
            "avgvol_200d",
        ],
        &more_than("avgvol_200d_min"),
        &less_than("avgvol_200d_max"),
        ValueKind::Bare,
    );

    // ── Company: EPS ────────────────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &[
            "eps",
            "earnings per share",
            "earnings share",
            "earnings_share",
        ],
        &more_than("earnings_share_min"),
        &less_than("earnings_share_max"),
        ValueKind::Dollar,
    );

    // ── Company: dividend yield ─────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &["dividend yield", "div yield", "yield", "dividend_yield"],
        &more_than("dividend_yield_min"),
        &less_than("dividend_yield_max"),
        ValueKind::Percent,
    );

    // ── Company: daily price change ─────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &[
            "daily change",
            "1 day change",
            "1-day change",
            "day change",
            "refund_1d_p",
        ],
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
            "refund_5d_p",
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
            "revenue_growth",
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
        &[
            "pe",
            "pe ratio",
            "p/e",
            "p/e ratio",
            "price to earnings",
            "pe_ratio",
        ],
        &more_than("pe_ratio_min"),
        &less_than("pe_ratio_max"),
        ValueKind::Bare,
    );

    // ── Post-screen: debt/equity ────────────────────────────────
    parse_numeric(
        &lower,
        &mut map,
        &[
            "debt/equity",
            "debt to equity",
            "d/e",
            "debt equity",
            "debt_equity",
        ],
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
            "price_book",
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

    // ── Exchange geography ──────────────────────────────────────
    parse_exchange_criteria(prompt, &mut map);

    serde_json::Value::Object(map)
}

/// Build EODHD Screener API filter triples from parsed criteria.
///
/// Converts `_min`/`_max` suffixed fields to `["field", ">=", value]` /
/// `["field", "<", value]` and string fields to `["field", "=", value]`.
/// Exchange criteria (`exchange`/`exchanges`) are skipped — the tool handler
/// owns the exchange dimension (one query per code). Only fields the EODHD
/// Screener API supports are included; post-screen fields are excluded.
pub(crate) fn build_screener_filters(criteria: &serde_json::Value) -> Vec<serde_json::Value> {
    let empty = serde_json::Map::new();
    let obj = criteria.as_object().unwrap_or(&empty);
    let mut filters = Vec::new();

    for (key, value) in obj {
        if key == "limit" || key == "exchange" || key == "exchanges" {
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
pub(crate) fn split_criteria(
    criteria: &serde_json::Value,
) -> (Vec<serde_json::Value>, serde_json::Value) {
    (
        build_screener_filters(criteria),
        build_post_screen_filters(criteria),
    )
}

/// Extract the exchange codes to screen: the `exchanges` array unioned with
/// a singular `exchange` value (criteria_overrides may supply either form).
pub(crate) fn extract_exchange_codes(criteria: &serde_json::Value) -> Vec<String> {
    let mut codes: Vec<String> = Vec::new();
    let mut push = |code: String, codes: &mut Vec<String>| {
        if !code.is_empty() && !codes.contains(&code) {
            codes.push(code);
        }
    };
    if let Some(list) = criteria.get("exchanges").and_then(|value| value.as_array()) {
        for value in list {
            if let Some(code) = value.as_str() {
                push(code.to_string(), &mut codes);
            }
        }
    }
    if let Some(code) = criteria.get("exchange").and_then(|value| value.as_str()) {
        push(code.trim().to_string(), &mut codes);
    }
    codes
}

// ── Helpers ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum ValueKind {
    Dollar,  // "$10B", "$500M", "$1.5T", "$50"
    Percent, // "5%", "10%"
    Bare,    // "20", "1.5", "1M"
}

/// Parse a numeric criterion from the prompt.
///
/// Runs three passes: "between X and Y" first (most specific), then one
/// directional pass per bound, so compound prompts capture every stated
/// bound.
fn parse_numeric(
    prompt: &str,
    map: &mut serde_json::Map<String, serde_json::Value>,
    keywords: &[&str],
    more_than_ops: &[(&str, &str)],
    less_than_ops: &[(&str, &str)],
    value_kind: ValueKind,
) {
    if let (Some((_, min_param)), Some((_, max_param))) =
        (more_than_ops.first(), less_than_ops.first())
    {
        for keyword in keywords {
            let pattern = build_between_pattern(keyword);
            if let Some(captures) = Regex::new(&pattern).ok().and_then(|re| re.captures(prompt)) {
                let low = captures.name("low").map(|m| m.as_str()).unwrap_or("");
                let high = captures.name("high").map(|m| m.as_str()).unwrap_or("");
                if let (Some(low_value), Some(high_value)) =
                    (parse_value(low, value_kind), parse_value(high, value_kind))
                {
                    insert_number(map, min_param, low_value);
                    insert_number(map, max_param, high_value);
                    break;
                }
            }
        }
    }
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

            if let Some(value) = parse_value(value_str, value_kind) {
                let param = ops
                    .iter()
                    .find(|(op, _)| *op == operator.trim())
                    .map(|(_, param)| *param);
                if let Some(p) = param {
                    insert_number(map, p, value);
                }
                break;
            }
        }
    }
}

/// Insert a numeric criterion unless the parameter is already set.
fn insert_number(map: &mut serde_json::Map<String, serde_json::Value>, param: &str, value: f64) {
    if map.contains_key(param) {
        return;
    }
    if let Some(number) = serde_json::Number::from_f64(value) {
        map.insert(param.to_string(), serde_json::Value::Number(number));
    }
}

/// Numeric value grammar shared by all numeric criteria: optional `$`, digits
/// with optional thousands commas and a decimal point, and an optional scale
/// suffix. The suffix binds to the number it follows — "between 2 and 200
/// billion" parses as min 2, max 2e11.
const VALUE_PATTERN: &str =
    r"\$?\d[\d,]*(?:\.\d+)?\s*(?:trillions?|billions?|millions?|thousands?|bn|mn|mm|k|b|m|t|%)?";

/// Build a regex for a numeric criterion in one direction.
fn build_directional_pattern(keyword: &str, ops: &[(&str, &str)]) -> String {
    let mut all_ops: Vec<&str> = ops.iter().map(|(o, _)| *o).collect();
    all_ops.sort_by_key(|b| std::cmp::Reverse(b.len()));

    let ops_alt = all_ops
        .iter()
        .map(|o| regex::escape(o))
        .collect::<Vec<_>>()
        .join("|");

    format!(
        r"(?i)\b{}\s*(?:is\s+|of\s+)?(?P<op>{})(?:\s+than)?\s+(?P<value>{})",
        regex::escape(keyword),
        ops_alt,
        VALUE_PATTERN
    )
}

/// Build a regex for a "between X and Y" numeric criterion.
fn build_between_pattern(keyword: &str) -> String {
    format!(
        r"(?i)\b{}\s*(?:is\s+|of\s+)?(?:between|from)\s+(?P<low>{})\s+(?:and|to|through)\s+(?P<high>{})",
        regex::escape(keyword),
        VALUE_PATTERN,
        VALUE_PATTERN
    )
}

/// Parse a string criterion from the prompt ("sector Technology",
/// "sector is Technology", "sector = Technology").
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

    let quoted_pattern = format!(r#"(?i)\b{}\s+(?:"([^"]+)"|'([^']+)')"#, kw);
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
        r"(?i)\b{}\s*(?:(?:equals|is|of|in|on|at|the|for|listed|traded)\s+|=\s*)*([a-zA-Z][a-zA-Z\s&.-]+?)(?:\s*(?:,|and|or|with|$))",
        kw
    );
    if let Some(captures) = Regex::new(&bare_pattern)
        .ok()
        .and_then(|re| re.captures(prompt))
    {
        let val = captures.get(1).map(|m| m.as_str()).unwrap_or("").trim();
        if !val.is_empty() && !is_operator_word(val) && !is_numeric_word(val) {
            map.insert(
                field.to_string(),
                serde_json::Value::String(val.to_string()),
            );
        }
    }
}

/// Parse a string criterion where the value comes BEFORE the keyword
/// ("Technology sector" instead of "sector Technology"). Leading filler
/// words are stripped so "companies in the Technology sector" yields
/// "Technology", not "companies in the Technology".
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
        r"(?i)\b([a-zA-Z][a-zA-Z\s&.-]+?)\s+{}\b(?:\s*(?:,|and|or|with|$))",
        kw
    );
    if let Some(captures) = Regex::new(&pattern).ok().and_then(|re| re.captures(prompt)) {
        let val = strip_leading_fillers(captures.get(1).map(|m| m.as_str()).unwrap_or(""));
        if !val.is_empty() && !is_operator_word(&val) && !is_numeric_word(&val) {
            map.insert(field.to_string(), serde_json::Value::String(val));
        }
    }
}

/// Strip leading filler words from a value-first capture.
fn strip_leading_fillers(value: &str) -> String {
    const FILLERS: &[&str] = &[
        "a",
        "an",
        "the",
        "in",
        "on",
        "at",
        "of",
        "for",
        "from",
        "with",
        "and",
        "or",
        "companies",
        "stocks",
        "equities",
        "firms",
        "listed",
        "traded",
    ];
    let mut words: Vec<&str> = value.split_whitespace().collect();
    while let Some(first) = words.first() {
        if FILLERS.contains(&first.to_lowercase().as_str()) {
            words.remove(0);
        } else {
            break;
        }
    }
    words.join(" ")
}

/// Parse exchange criteria into the `exchanges` key (array of EODHD exchange
/// codes, first-mention order): every geographic entity mentioned anywhere in
/// the prompt, plus an explicit "exchange(s) …" phrase whose value list may
/// contain geography names or literal exchange codes.
fn parse_exchange_criteria(prompt: &str, map: &mut serde_json::Map<String, serde_json::Value>) {
    let mut codes: Vec<String> = Vec::new();

    // Geographic entities anywhere in the prompt, ordered by first mention.
    let mut mentions: Vec<(usize, &[&str])> = Vec::new();
    for (pattern, case_sensitive, entity_codes) in GEOGRAPHY {
        let flags = if *case_sensitive { "" } else { "(?i)" };
        let expression = format!(r"{flags}\b{}\b", regex::escape(pattern));
        if let Ok(re) = Regex::new(&expression)
            && let Some(found) = re.find(prompt)
        {
            mentions.push((found.start(), entity_codes));
        }
    }
    mentions.sort_by_key(|(position, _)| *position);
    for (_, entity_codes) in &mentions {
        for code in entity_codes.iter() {
            let owned = (*code).to_string();
            if !codes.contains(&owned) {
                codes.push(owned);
            }
        }
    }

    // Explicit "exchange(s) [equals/is/in/the/=] value-list" phrase. Adds
    // geography names the scan may miss and literal codes (e.g. "exchange
    // VN"). Prose values cannot become codes: a literal must be short and
    // uppercase as written, so "exchange rate" does not screen exchange=RATE.
    let phrase = r"(?i)\bexchanges?\b\s*(?:(?:equals|is|of|in|on|at|the|for|listed|traded)\s+|=\s*)*([a-zA-Z][a-zA-Z\s&.,-]*?)(?:\s*$|\s*(?=[.;]|\b(?:and|with|where|that|having)\b))";
    if let Some(captures) = Regex::new(phrase).ok().and_then(|re| re.captures(prompt)) {
        if let Some(tail) = captures.get(1).map(|m| m.as_str()) {
            for token in tail.split(',').flat_map(|part| part.split(" or ")) {
                let token = token.trim();
                if token.is_empty() {
                    continue;
                }
                if let Some(entity_codes) = geography_lookup(&token.to_lowercase()) {
                    for code in entity_codes.iter() {
                        let owned = (*code).to_string();
                        if !codes.contains(&owned) {
                            codes.push(owned);
                        }
                    }
                } else if token.len() <= 5
                    && token.chars().all(|character| {
                        character.is_ascii_uppercase() || character.is_ascii_digit()
                    })
                {
                    codes.push(token.to_string());
                }
            }
        }
    }

    if !codes.is_empty() {
        map.insert(
            "exchanges".to_string(),
            serde_json::Value::Array(codes.into_iter().map(serde_json::Value::String).collect()),
        );
    }
}

/// Look up a lowercased token in the geography table (case-sensitive
/// entries are excluded — they are scan-only).
fn geography_lookup(token: &str) -> Option<&'static [&'static str]> {
    GEOGRAPHY
        .iter()
        .find(|(pattern, case_sensitive, _)| !*case_sensitive && *pattern == token)
        .map(|(_, _, codes)| *codes)
}

/// Parse a value according to its field kind.
fn parse_value(text: &str, value_kind: ValueKind) -> Option<f64> {
    match value_kind {
        ValueKind::Dollar | ValueKind::Bare => parse_scaled_value(text, false),
        ValueKind::Percent => parse_scaled_value(text, true),
    }
}

/// Parse a scaled numeric value: "$10B" → 1e10, "2,000 million" → 2e9,
/// "2 thousand" → 2e3, "5%" → 0.05 when `percent`, "1.5" → 1.5.
///
/// Word suffixes are matched before letter suffixes so "2mm" is two million
/// ("mm" before "m") and "2bn" is two billion ("bn" before "b").
fn parse_scaled_value(raw: &str, percent: bool) -> Option<f64> {
    let text = raw.trim();
    let text = text.strip_prefix('$').unwrap_or(text);
    let text = text.replace(',', "");
    let lowered = text.trim().to_lowercase();
    let without_percent = lowered.strip_suffix('%').unwrap_or(&lowered).trim();
    const SUFFIXES: &[(&str, f64)] = &[
        ("trillions", 1e12),
        ("trillion", 1e12),
        ("billions", 1e9),
        ("billion", 1e9),
        ("millions", 1e6),
        ("million", 1e6),
        ("thousands", 1e3),
        ("thousand", 1e3),
        ("bn", 1e9),
        ("mn", 1e6),
        ("mm", 1e6),
        ("t", 1e12),
        ("b", 1e9),
        ("m", 1e6),
        ("k", 1e3),
    ];
    let (number_text, multiplier) = SUFFIXES
        .iter()
        .find_map(|(suffix, multiplier)| {
            without_percent
                .strip_suffix(suffix)
                .map(|rest| (rest, *multiplier))
        })
        .unwrap_or((without_percent, 1.0));
    let number = number_text.trim().parse::<f64>().ok()?;
    Some(if percent {
        number / 100.0
    } else {
        number * multiplier
    })
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
            | "fewer"
            | "equals"
            | "in"
            | "on"
            | "of"
            | "the"
            | "at"
            | "for"
            | "from"
            | "between"
            | "to"
            | ">"
            | "<"
            | ">="
            | "<="
            | "="
    )
}

/// Words that look like numbers — skip them as string values.
fn is_numeric_word(s: &str) -> bool {
    let text = s.trim().strip_prefix('$').unwrap_or(s.trim());
    let text = text.strip_suffix('%').unwrap_or(text).trim();
    let mut characters = text.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_digit())
        && characters.all(|c| {
            c.is_ascii_digit()
                || c == '.'
                || c == ','
                || c == 'b'
                || c == 'm'
                || c == 'k'
                || c == 't'
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// expect: the prompt that motivated the parser repair parses fully —
    /// both market-cap bounds and every stated geography.
    #[test]
    fn operators_original_prompt_parses_completely() {
        let criteria = parse_screening_prompt(
            "Companies with market capitalization between 2 billion and 200 billion USD, listed on exchanges in the United States, Japan, Canada, Mexico, or Europe",
        );
        assert_eq!(
            criteria["market_capitalization_min"],
            json!(2_000_000_000.0)
        );
        assert_eq!(
            criteria["market_capitalization_max"],
            json!(200_000_000_000.0)
        );
        let codes = criteria["exchanges"].as_array().expect("exchanges array");
        for expected in ["US", "JP", "TO", "MX", "LSE", "XETRA", "PA", "WAR"] {
            assert!(
                codes.iter().any(|code| code == expected),
                "expected {expected} in {codes}"
            );
        }
    }

    /// expect: the field-name syntax the tool's own framework string
    /// advertises (underscore names, comparison operators, equals) parses.
    #[test]
    fn advertised_field_name_syntax_parses() {
        let criteria = parse_screening_prompt(
            "market_capitalization greater than 2000000000 and market_capitalization less than 200000000000 and exchange equals US",
        );
        assert_eq!(
            criteria["market_capitalization_min"],
            json!(2_000_000_000.0)
        );
        assert_eq!(
            criteria["market_capitalization_max"],
            json!(200_000_000_000.0)
        );
        assert_eq!(criteria["exchanges"], json!(["US"]));
    }

    #[test]
    fn greater_than_word_operator_parses() {
        let criteria = parse_screening_prompt("market cap greater than $2B");
        assert_eq!(
            criteria["market_capitalization_min"],
            json!(2_000_000_000.0)
        );
    }

    /// expect: thousands commas are consumed whole — "2,000,000,000" is two
    /// billion, not two thousand.
    #[test]
    fn comma_thousands_parse_fully() {
        let criteria = parse_screening_prompt("market cap above 2,000,000,000");
        assert_eq!(
            criteria["market_capitalization_min"],
            json!(2_000_000_000.0)
        );
    }

    /// expect: "2 thousand" is two thousand, not two trillion (the old
    /// single-letter suffix class mapped t → trillion).
    #[test]
    fn thousand_is_not_trillion() {
        let criteria = parse_screening_prompt("market cap above 2 thousand");
        assert_eq!(criteria["market_capitalization_min"], json!(2000.0));
    }

    #[test]
    fn bare_values_accept_scale_suffixes() {
        let criteria = parse_screening_prompt("volume above 1M");
        assert_eq!(criteria["avgvol_1d_min"], json!(1_000_000.0));
    }

    #[test]
    fn percent_values_parse() {
        let criteria = parse_screening_prompt("dividend yield over 2%");
        assert_eq!(criteria["dividend_yield_min"], json!(0.02));
    }

    /// expect: the documented suffix-binding wrinkle — the suffix binds to
    /// the number it follows, so the low bound of "between 2 and 200 billion"
    /// is 2, not 2 billion.
    #[test]
    fn suffix_binds_to_its_number() {
        let criteria = parse_screening_prompt("market cap between 2 and 200 billion");
        assert_eq!(criteria["market_capitalization_min"], json!(2.0));
        assert_eq!(
            criteria["market_capitalization_max"],
            json!(200_000_000_000.0)
        );
    }

    /// expect: "exchange equals US" parses as the US exchange — the operator
    /// word is consumed, not captured as part of the value.
    #[test]
    fn exchange_equals_is_not_swallowed() {
        let criteria = parse_screening_prompt("exchange equals US");
        assert_eq!(criteria["exchanges"], json!(["US"]));
    }

    /// expect: prefix words between "exchange" and the value are consumed —
    /// "exchange in the US" is the US exchange, not the value "the US".
    #[test]
    fn exchange_prefix_words_are_consumed() {
        let criteria = parse_screening_prompt("exchange in the US");
        assert_eq!(criteria["exchanges"], json!(["US"]));
    }

    #[test]
    fn exchange_lists_map_through_geography() {
        let criteria = parse_screening_prompt("exchanges US, Japan, or Europe");
        let codes = criteria["exchanges"].as_array().expect("exchanges array");
        assert!(codes.iter().any(|code| code == "US"));
        assert!(codes.iter().any(|code| code == "JP"));
        assert!(codes.iter().any(|code| code == "LSE"));
    }

    /// expect: an unmapped short uppercase token passes through as a literal
    /// exchange code (the criteria_overrides affordance for codes the
    /// geography table does not know).
    #[test]
    fn literal_exchange_codes_pass_through() {
        let criteria = parse_screening_prompt("exchange VN");
        assert_eq!(criteria["exchanges"], json!(["VN"]));
    }

    /// expect: prose following "exchange" cannot become an exchange code —
    /// "exchange rate" is not exchange=RATE.
    #[test]
    fn prose_after_exchange_cannot_become_a_code() {
        let criteria = parse_screening_prompt("companies with exchange rate exposure");
        assert!(criteria.get("exchanges").is_none());
    }

    /// expect: the lowercase pronoun "us" does not select US listings
    /// (case-sensitive matching).
    #[test]
    fn pronoun_us_is_not_the_us() {
        let criteria = parse_screening_prompt("show us companies with pe under 20");
        assert!(criteria.get("exchanges").is_none());
        assert_eq!(criteria["pe_ratio_max"], json!(20.0));
    }

    #[test]
    fn geography_nouns_and_adjectives_parse() {
        for (prompt, expected) in [
            ("US stocks", "US"),
            ("Japanese stocks", "JP"),
            ("NYSE listed companies", "US"),
            ("Canadian equities", "TO"),
            ("European stocks", "LSE"),
        ] {
            let criteria = parse_screening_prompt(prompt);
            let codes = criteria["exchanges"]
                .as_array()
                .unwrap_or_else(|| panic!("no exchanges for {prompt}"));
            assert!(
                codes.iter().any(|code| code == expected),
                "expected {expected} for {prompt}, got {codes}"
            );
        }
    }

    #[test]
    fn sector_prefixes_and_equals_parse() {
        assert_eq!(
            parse_screening_prompt("sector is Technology")["sector"],
            json!("Technology")
        );
        assert_eq!(
            parse_screening_prompt("sector = Technology")["sector"],
            json!("Technology")
        );
        assert_eq!(
            parse_screening_prompt("sector Technology")["sector"],
            json!("Technology")
        );
    }

    /// expect: value-first capture strips filler words — "companies in the
    /// Technology sector" yields "Technology", not "companies in the
    /// Technology".
    #[test]
    fn value_first_sector_strips_fillers() {
        assert_eq!(
            parse_screening_prompt("companies in the Technology sector")["sector"],
            json!("Technology")
        );
    }

    /// expect: exchange criteria never become filter triples — the handler
    /// owns the exchange dimension.
    #[test]
    fn build_screener_filters_exclude_exchange_keys() {
        let criteria = json!({
            "market_capitalization_min": 2_000_000_000.0,
            "exchanges": ["US"],
            "exchange": "JP",
            "sector": "Technology",
        });
        let filters = build_screener_filters(&criteria);
        let text = serde_json::to_string(&filters).expect("filters serialize");
        assert!(text.contains("market_capitalization"));
        assert!(text.contains("sector"));
        assert!(!text.contains("exchange"));
    }

    #[test]
    fn extract_exchange_codes_unions_both_keys() {
        let criteria = json!({"exchanges": ["US", "JP"], "exchange": "US"});
        assert_eq!(
            extract_exchange_codes(&criteria),
            vec!["US".to_string(), "JP".to_string()]
        );
    }
}
