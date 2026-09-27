//! Tier 1 of the two-tier company-research verification: a fast, deterministic
//! scan of every cited line in a frozen report. Each numeric figure and quoted
//! phrase on a line citing a retained source (`[output_key]`) is looked up in
//! that source's full text. Mismatches are promotion candidates for the Tier 2
//! independent review of the report's key claims; a match is a mechanical
//! string/number coincidence, not a verified claim.
use regex::Regex;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::LazyLock;

static NUMBER: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(\$?)(\d{1,3}(?:,\d{3})+|\d+)(\.\d+)?").ok());
/// A citation is a bracketed list of source keys (`[msft-fy26-10k]`,
/// `[a#p3; b]`); bracketed numbers such as a regulator's `[30-40]%` are text.
static CITATION: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"\[([A-Za-z][A-Za-z0-9_.:#/ ;,-]*)\]").ok());
static QUOTE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r#""([^"]{20,400})""#).ok());
static ISO_DATE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"\b\d{4}-\d{2}-\d{2}\b").ok());

const MAX_LISTED: usize = 150;

/// Normalize typography so a report's straight quotes and spaces compare
/// equal to the source's curly quotes and non-breaking spaces.
pub(crate) fn normalize(text: &str) -> String {
    let mapped: String = text
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => '\'',
            '\u{201C}' | '\u{201D}' => '"',
            '\u{00A0}' | '\t' => ' ',
            '\u{2013}' | '\u{2014}' => '-',
            other => other,
        })
        .collect();
    mapped.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Unit {
    None,
    Million,
    Billion,
    Trillion,
    Percent,
}

impl Unit {
    fn millions(self) -> Option<f64> {
        match self {
            Unit::Million => Some(1.0),
            Unit::Billion => Some(1_000.0),
            Unit::Trillion => Some(1_000_000.0),
            Unit::None | Unit::Percent => None,
        }
    }
}

#[derive(Debug, Clone)]
struct Figure {
    text: String,
    value: f64,
    decimals: i32,
    unit: Unit,
    comma: bool,
}

fn unit_after(rest: &str) -> Unit {
    let trimmed = rest.trim_start();
    let word: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_alphabetic() || *c == '%')
        .collect();
    match word.as_str() {
        "%" | "percent" => Unit::Percent,
        "trillion" | "T" => Unit::Trillion,
        "billion" | "B" | "bn" => Unit::Billion,
        "million" | "M" | "mn" => Unit::Million,
        _ if trimmed.starts_with('%') => Unit::Percent,
        _ => Unit::None,
    }
}

fn figures(text: &str) -> Vec<Figure> {
    let Some(number) = NUMBER.as_ref() else {
        return Vec::new();
    };
    let cleaned = ISO_DATE.as_ref().map_or_else(
        || text.to_string(),
        |r| r.replace_all(text, " ").into_owned(),
    );
    let mut out = Vec::new();
    for caps in number.captures_iter(&cleaned) {
        let Some(whole) = caps.get(0) else { continue };
        let previous = cleaned[..whole.start()].chars().next_back();
        if previous.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '#' || c == '.') {
            continue;
        }
        let integer = caps.get(2).map_or("", |m| m.as_str());
        let fraction = caps.get(3).map_or("", |m| m.as_str());
        let Ok(value) = format!("{}{}", integer.replace(',', ""), fraction).parse::<f64>() else {
            continue;
        };
        let unit = unit_after(&cleaned[whole.end()..]);
        let dollar = caps.get(1).is_some_and(|m| !m.as_str().is_empty());
        let comma = integer.contains(',');
        let decimals = fraction.len().saturating_sub(1) as i32;
        let is_year = !dollar
            && !comma
            && fraction.is_empty()
            && integer.len() == 4
            && (1900.0..=2099.0).contains(&value)
            && unit != Unit::Percent;
        let bare_small =
            !dollar && !comma && fraction.is_empty() && unit == Unit::None && value < 1000.0;
        if is_year || bare_small {
            continue;
        }
        out.push(Figure {
            text: format!("{integer}{fraction}"),
            value,
            decimals,
            unit,
            comma,
        });
    }
    out
}

/// Per-source lookup: normalized full text plus every figure's value, raw and
/// scaled to millions when the source states its unit.
struct SourceIndex {
    text: String,
    raw: Vec<f64>,
    millions: Vec<f64>,
    percents: Vec<f64>,
}

impl SourceIndex {
    fn new(text: &str) -> Self {
        let text = normalize(text);
        let mut raw = Vec::new();
        let mut millions = Vec::new();
        let mut percents = Vec::new();
        if let Some(number) = NUMBER.as_ref() {
            for caps in number.captures_iter(&text) {
                let (Some(whole), Some(integer)) = (caps.get(0), caps.get(2)) else {
                    continue;
                };
                let fraction = caps.get(3).map_or("", |m| m.as_str());
                let Ok(value) =
                    format!("{}{}", integer.as_str().replace(',', ""), fraction).parse::<f64>()
                else {
                    continue;
                };
                let unit = unit_after(&text[whole.end()..]);
                raw.push(value);
                if let Some(scale) = unit.millions() {
                    millions.push(value * scale);
                }
                if unit == Unit::Percent {
                    percents.push(value);
                }
            }
        }
        Self {
            text,
            raw,
            millions,
            percents,
        }
    }

    /// `needle` occurs in the text as a whole number: not preceded or followed
    /// by a digit, a digit-group comma or a decimal point ("211.3" is not in
    /// "1,211.3").
    fn contains_number(&self, needle: &str) -> bool {
        let bytes = self.text.as_bytes();
        let joins = |i: usize| bytes.get(i).is_some_and(|b| b.is_ascii_digit());
        self.text.match_indices(needle).any(|(start, _)| {
            let end = start + needle.len();
            let before_ok = start == 0
                || !(bytes[start - 1].is_ascii_digit()
                    || (matches!(bytes[start - 1], b',' | b'.') && start >= 2 && joins(start - 2)));
            let after_ok =
                !(joins(end) || (matches!(bytes.get(end), Some(b',' | b'.')) && joins(end + 1)));
            before_ok && after_ok
        })
    }

    fn matches(&self, figure: &Figure) -> bool {
        let tolerance = 0.5 * 10f64.powi(-figure.decimals);
        let near = |values: &[f64], target: f64, tol: f64| {
            values.iter().any(|v| (v - target).abs() <= tol + 1e-9)
        };
        match figure.unit {
            Unit::Percent => {
                self.contains_number(&format!("{}%", figure.text))
                    || self.contains_number(&format!("{} percent", figure.text))
                    || near(&self.percents, figure.value, tolerance)
            }
            unit => {
                if let Some(scale) = unit.millions() {
                    let target = figure.value * scale;
                    let tol = tolerance * scale;
                    // A table states millions without a unit word: match a
                    // raw source value only when it is comma-grouped at the
                    // same scale ($331.8 billion vs 331,839).
                    near(&self.millions, target, tol)
                        || near(&self.raw, target, tol)
                        || (figure.comma && self.contains_number(&figure.text))
                } else {
                    self.contains_number(&figure.text) || near(&self.raw, figure.value, tolerance)
                }
            }
        }
    }
}

fn cited_keys(line: &str) -> Vec<String> {
    let Some(citation) = CITATION.as_ref() else {
        return Vec::new();
    };
    citation
        .captures_iter(line)
        .filter_map(|caps| caps.get(1))
        .flat_map(|m| {
            m.as_str()
                .split([';', ','])
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .map(|key| {
            let key = key.trim();
            key.split('#').next().unwrap_or(key).trim().to_string()
        })
        .filter(|key| !key.is_empty())
        .collect()
}

/// Scan `target` against `sources` (output_key → full original text).
/// Lines without their own citation inherit the most recent citation in the
/// same markdown section (tables and bullet runs under a cited lead line).
pub(crate) fn scan(target: &str, sources: &HashMap<String, String>) -> Value {
    let index: HashMap<&str, SourceIndex> = sources
        .iter()
        .map(|(key, text)| (key.as_str(), SourceIndex::new(text)))
        .collect();
    let quote = QUOTE.as_ref();
    let mut inherited: Vec<String> = Vec::new();
    let (mut lines_scanned, mut figures_checked, mut figures_matched) = (0usize, 0usize, 0usize);
    let (mut quotes_checked, mut quotes_matched) = (0usize, 0usize);
    let (mut uncited_lines, mut non_retained_lines, mut mixed_unmatched) = (0usize, 0usize, 0usize);
    let mut unmatched = Vec::new();
    let mut in_code = false;
    for (line_no, raw_line) in target.lines().enumerate() {
        let line = normalize(raw_line);
        if line.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        if line.starts_with('#') || line.starts_with("===") {
            inherited.clear();
            continue;
        }
        let own = cited_keys(&line);
        if !own.is_empty() {
            inherited = own.clone();
        }
        let keys = if own.is_empty() { &inherited } else { &own };
        let retained: Vec<&SourceIndex> =
            keys.iter().filter_map(|k| index.get(k.as_str())).collect();
        // A line that also cites non-retained sources (tool outputs) mixes
        // provenance: its unmatched figures may come from those sources, so
        // they are reported as not checkable rather than as mismatches.
        let mixed = keys.iter().any(|k| !index.contains_key(k.as_str()));
        let citation_free = CITATION
            .as_ref()
            .map_or_else(|| line.clone(), |r| r.replace_all(&line, " ").into_owned());
        let line_figures = figures(&citation_free);
        let line_quotes: Vec<String> = quote
            .map(|q| {
                q.captures_iter(&citation_free)
                    .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        if line_figures.is_empty() && line_quotes.is_empty() {
            continue;
        }
        lines_scanned += 1;
        if retained.is_empty() {
            if keys.is_empty() {
                uncited_lines += 1;
            } else {
                non_retained_lines += 1;
            }
            continue;
        }
        for figure in &line_figures {
            let matched = retained.iter().any(|s| s.matches(figure));
            if !matched && mixed {
                mixed_unmatched += 1;
                continue;
            }
            figures_checked += 1;
            if matched {
                figures_matched += 1;
            } else if unmatched.len() < MAX_LISTED {
                unmatched.push(
                    json!({"line": line_no + 1, "kind": "figure", "token": figure.text,
                    "sources": keys, "context": line.chars().take(160).collect::<String>()}),
                );
            }
        }
        for phrase in &line_quotes {
            quotes_checked += 1;
            if retained.iter().any(|s| s.text.contains(phrase.as_str())) {
                quotes_matched += 1;
            } else if unmatched.len() < MAX_LISTED {
                unmatched.push(
                    json!({"line": line_no + 1, "kind": "quote", "token": phrase,
                    "sources": keys}),
                );
            }
        }
    }
    let mismatches = (figures_checked - figures_matched) + (quotes_checked - quotes_matched);
    json!({
        "status": if mismatches == 0 { "clean" } else { "mismatches" },
        "lines_scanned": lines_scanned,
        "figures_checked": figures_checked,
        "figures_matched": figures_matched,
        "quotes_checked": quotes_checked,
        "quotes_matched": quotes_matched,
        "mismatches": mismatches,
        "unmatched": unmatched,
        "not_checkable": {
            "uncited_lines": uncited_lines,
            "lines_citing_only_non_retained_sources": non_retained_lines,
            "figures_on_mixed_provenance_lines": mixed_unmatched,
        },
        "limitation": "Tier 1 matches figures and quotes to the cited source text; it does not check context, period, sign or materiality. Unmatched items are Tier 2 promotion candidates.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources() -> HashMap<String, String> {
        HashMap::from([
            (
                "k10".to_string(),
                "Total revenue\t331,839\t281,724\nMicrosoft Cloud revenue increased 27% to $214.4 billion.\nWe hold an approximate 25\u{00A0}interest; we recorded revenue of $24.1 billion.\nMicrosoft and AWS each has a high share of supply at [30-40]% in 2024.".to_string(),
            ),
            (
                "call".to_string(),
                "Capital expenditures were $41 billion. Customer demand continues to exceed available capacity.".to_string(),
            ),
        ])
    }

    /// expect: exact, rounded and unit-converted figures and verbatim quotes
    /// match their cited source; a wrong figure and an altered quote do not.
    #[test]
    fn matches_rounded_and_scaled_figures_and_flags_wrong_ones() {
        let report = "## Finance\n\"Microsoft and AWS each has a high share of supply at [30-40]% in 2024\" [k10].\nRevenue $331,839M; cloud $214.4 billion (+27%) [k10].\n| Total | 331,839 |\nRevenue was about $331.8 billion [k10].\nCapex $41B; \"Customer demand continues to exceed available capacity.\" [call]\nCloud grew 29% to $999.9 billion [k10].\n\"Customer demand is wildly exceeding everything we have\" [call]";
        let result = scan(report, &sources());
        assert_eq!(result["figures_checked"], 8, "{result}");
        assert_eq!(result["figures_matched"], 6, "{result}");
        assert_eq!(result["quotes_checked"], 3, "{result}");
        assert_eq!(result["quotes_matched"], 2, "{result}");
        let tokens: Vec<&str> = result["unmatched"]
            .as_array()
            .map(|a| a.iter().filter_map(|u| u["token"].as_str()).collect())
            .unwrap_or_default();
        assert!(
            tokens.contains(&"29") && tokens.contains(&"999.9"),
            "{result}"
        );
    }

    /// expect: a figure embedded in a larger number is not a match — a
    /// provider operating income of 211.3 stated against a source that only
    /// shows revenue 1,211.3 is flagged (live-observed 2026-09-26, VIRI.PA).
    #[test]
    fn embedded_figure_is_not_a_match() {
        let sources = HashMap::from([(
            "urd".to_string(),
            "Operating revenues 1,070.5 1,211.3\nOperating income 237.3 143.5".to_string(),
        )]);
        let result = scan(
            "Operating income was 211.3 (audited) [urd].\nRevenue 1,070.5 [urd].",
            &sources,
        );
        assert_eq!(result["figures_checked"], 2, "{result}");
        assert_eq!(result["figures_matched"], 1, "{result}");
        let tokens: Vec<&str> = result["unmatched"]
            .as_array()
            .map(|a| a.iter().filter_map(|u| u["token"].as_str()).collect())
            .unwrap_or_default();
        assert_eq!(tokens, vec!["211.3"], "{result}");
    }

    /// Calibration probe over a real frozen report (not run by default):
    /// `CLAIM_SCAN_PROBE=<run dir> cargo test -p hkask-mcp-companies claim_scan_probe -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn claim_scan_probe() {
        let Ok(dir) = std::env::var("CLAIM_SCAN_PROBE") else {
            return;
        };
        let dir = std::path::Path::new(&dir);
        let read = |p: &str| std::fs::read_to_string(dir.join(p)).unwrap_or_default();
        let target = format!(
            "{}\n{}",
            read("drafts/full-report.md"),
            read("drafts/summary.md")
        );
        let sources = HashMap::from([
            ("msft-fy26-10k".to_string(), read("msft-fy26-10k.txt")),
            (
                "msft-fy26q4-transcript".to_string(),
                read("msft-fy26q4-transcript.txt"),
            ),
            (
                "msft-fy26q3-transcript".to_string(),
                read("msft-fy26q3-transcript.txt"),
            ),
            (
                "cma-cloud-final-2025".to_string(),
                read("cma-cloud-final-2025.txt"),
            ),
        ]);
        let result = scan(&target, &sources);
        println!(
            "{}",
            serde_json::to_string_pretty(&result).unwrap_or_default()
        );
    }

    /// expect: years, dates, list numbers and figures citing only tool outputs
    /// are not counted as matches or mismatches; an uncited figure is counted
    /// as not checkable rather than silently passing.
    #[test]
    fn skips_years_dates_and_reports_uncheckable_lines() {
        let report = "As of 2026-09-26, for fiscal 2026 and 3 segments [k10].\nP/E about 28.8 [stock_quote].\nCloud $214.4 billion; stability 0.976 [k10; moat_check].\n## Other\nMargin stability 0.976.\n```mermaid\nA[331,000,000] --> B\n```";
        let result = scan(report, &sources());
        assert_eq!(result["figures_checked"], 1, "{result}");
        assert_eq!(
            result["not_checkable"]["lines_citing_only_non_retained_sources"],
            1
        );
        assert_eq!(result["not_checkable"]["uncited_lines"], 1);
        assert_eq!(
            result["not_checkable"]["figures_on_mixed_provenance_lines"],
            1
        );
        assert_eq!(result["figures_matched"], 1, "{result}");
        assert_eq!(result["status"], "clean");
    }
}
