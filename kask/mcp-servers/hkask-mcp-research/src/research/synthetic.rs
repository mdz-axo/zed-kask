//! Synthetic feed generation: turn non-feed websites and JSON APIs into
//! entries in the existing `feeds`/`entries` tables.
//!
//! A synthetic feed is a stored binding from a `source_url` + an
//! `extractor_spec` to a feed row in `feeds` (with `url = synthetic://<feed_id>`).
//! The existing `rss_fetch` machinery then polls it on the same schedule as
//! real feeds, and `rss_get_entries` / `rss_search` / OPML tools work on it
//! unchanged.
//!
//! Extractor kinds:
//! - `css`:        CSS selectors against static HTML (html2rss/rsspls model)
//! - `json_path`:  JSONPath expressions against JSON APIs
//! - `diff_hash`:  whole-content hash; new entry only when content changes
//!
//! Future kinds (`xpath`, `llm_schema`, `pdf_ocr`) are reserved in the enum
//! but not yet implemented here.

use std::collections::HashMap;

use blake3::Hasher;
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::research::rss_types::FetchResult;

// ── Spec types ─────────────────────────────────────────────────────────────

/// A field selector. For `css`, this is a CSS selector with an optional
/// `@attr` suffix (e.g. `"a@href"`). For `json_path`, this is a JSONPath
/// expression (e.g. `"$.title"`). For `diff_hash`, this is unused.
pub type FieldSelector = String;

/// The extractor kind. Determines how `extractor_spec` is interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractorKind {
    Css,
    JsonPath,
    DiffHash,
    // Reserved for future implementation:
    // Xpath,
    // LlmSchema,
    // PdfOcr,
}

impl ExtractorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Css => "css",
            Self::JsonPath => "json_path",
            Self::DiffHash => "diff_hash",
        }
    }
}

impl std::str::FromStr for ExtractorKind {
    type Err = SyntheticError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "css" => Ok(Self::Css),
            "json_path" => Ok(Self::JsonPath),
            "diff_hash" => Ok(Self::DiffHash),
            other => Err(SyntheticError::UnsupportedKind(other.to_string())),
        }
    }
}

/// The declarative spec for a synthetic feed. Stored as JSON in
/// `synthetic_feeds.extractor_spec`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractorSpec {
    /// CSS selector or JSONPath for the container of each item.
    /// For `diff_hash`, unused (the whole page is the "item").
    pub items_selector: Option<String>,
    /// Per-field selectors. Keys: title, link, date, summary, entry_id.
    /// Values are CSS selectors (with optional `@attr`) or JSONPath expressions.
    pub fields: HashMap<String, String>,
    /// Optional template for building the entry_id from extracted fields.
    /// e.g. `"{link}"` or `"{title}|{date}"`. Defaults to the link field.
    pub entry_id_template: Option<String>,
    /// Optional base URL for resolving relative links (css only).
    /// Defaults to the source_url.
    pub base_url: Option<String>,
}

/// A single extracted item, before it becomes a `feed_rs::Entry`.
#[derive(Debug, Clone, Default)]
pub struct ExtractedItem {
    pub title: String,
    pub link: String,
    pub date: String,
    pub summary: String,
    pub entry_id: String,
}

// ── Errors ─────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum SyntheticError {
    #[error("unsupported extractor kind: {0}")]
    UnsupportedKind(String),
    #[error("invalid extractor spec: {0}")]
    InvalidSpec(String),
    #[error("extraction failed: {0}")]
    ExtractionFailed(String),
    #[error("field selector missing for required field: {0}")]
    MissingField(String),
}

// ── Extraction entry point ─────────────────────────────────────────────────

/// Extract items from a fetched page/API response.
///
/// The `body` is the raw response bytes. The `content_type` is used to
/// decide whether to parse as HTML or JSON when the extractor kind doesn't
/// imply it (e.g. `css` always parses as HTML, `json_path` always as JSON,
/// `diff_hash` doesn't parse at all).
pub fn extract(
    kind: ExtractorKind,
    spec: &ExtractorSpec,
    source_url: &str,
    body: &[u8],
    content_type: &str,
) -> Result<Vec<ExtractedItem>, SyntheticError> {
    match kind {
        ExtractorKind::Css => {
            let html = std::str::from_utf8(body)
                .map_err(|e| SyntheticError::ExtractionFailed(format!("utf8: {e}")))?;
            extract_css(spec, source_url, html)
        }
        ExtractorKind::JsonPath => {
            let json: Value = serde_json::from_slice(body)
                .map_err(|e| SyntheticError::ExtractionFailed(format!("json parse: {e}")))?;
            extract_json_path(spec, &json)
        }
        ExtractorKind::DiffHash => {
            // diff_hash doesn't extract items; the caller handles hashing.
            // Return an empty vec; the caller creates a single "entry" from
            // the content hash if it changed.
            Ok(Vec::new())
        }
    }
}

/// Compute a blake3 hash of the body for diff detection.
pub fn content_hash(body: &[u8]) -> String {
    let mut hasher = Hasher::new();
    hasher.update(body);
    hasher.finalize().to_hex().to_string()
}

// ── CSS extraction ─────────────────────────────────────────────────────────

/// Extract items from an HTML page using CSS selectors.
///
/// The spec's `items_selector` selects the container for each item.
/// The spec's `fields` map field names (title, link, date, summary, entry_id)
/// to CSS selectors. Selectors may have an `@attr` suffix to extract an
/// attribute rather than text (e.g. `"a@href"`).
fn extract_css(
    spec: &ExtractorSpec,
    source_url: &str,
    html: &str,
) -> Result<Vec<ExtractedItem>, SyntheticError> {
    use scraper::{ElementRef, Html, Selector};

    let document = Html::parse_document(html);
    let base_url = spec.base_url.as_deref().unwrap_or(source_url);
    let base = reqwest::Url::parse(base_url)
        .map_err(|e| SyntheticError::InvalidSpec(format!("base_url: {e}")))?;

    let items_sel = spec
        .items_selector
        .as_deref()
        .ok_or_else(|| SyntheticError::MissingField("items_selector".into()))?;
    let items_selector = Selector::parse(items_sel)
        .map_err(|e| SyntheticError::InvalidSpec(format!("items_selector: {e}")))?;

    let field_selectors = compile_field_selectors_css(&spec.fields)?;

    let template = spec
        .entry_id_template
        .as_deref()
        .unwrap_or("{link}");

    let mut items = Vec::new();
    for element in document.select(&items_selector) {
        let mut item = ExtractedItem::default();
        for (field, sel) in &field_selectors {
            let value = extract_field_css(element, sel, &base);
            match field.as_str() {
                "title" => item.title = value,
                "link" | "url" => item.link = value,
                "date" | "published" | "published_at" => item.date = value,
                "summary" | "description" => item.summary = value,
                "entry_id" | "id" => item.entry_id = value,
                _ => {} // ignore unknown fields
            }
        }
        // If entry_id was not explicitly extracted, build it from the template.
        if item.entry_id.is_empty() {
            item.entry_id = render_template(template, &item);
        }
        // If entry_id is still empty, fall back to link, then title.
        if item.entry_id.is_empty() {
            item.entry_id = if !item.link.is_empty() {
                item.link.clone()
            } else {
                blake3::hash(item.title.as_bytes()).to_hex().to_string()
            };
        }
        items.push(item);
    }

    Ok(items)
}

/// A compiled CSS field selector: the selector plus the optional attribute.
struct CompiledCssField {
    selector: Selector,
    attr: Option<String>,
}

/// Compile the spec's `fields` map into CSS selectors.
fn compile_field_selectors_css(
    fields: &HashMap<String, String>,
) -> Result<Vec<(String, CompiledCssField)>, SyntheticError> {
    let mut out = Vec::new();
    for (field, sel_str) in fields {
        // Split on `@` to separate selector from attribute.
        let (sel_part, attr) = match sel_str.rfind('@') {
            Some(idx) if !sel_str[..idx].is_empty() => {
                (sel_str[..idx].to_string(), Some(sel_str[idx + 1..].to_string()))
            }
            _ => (sel_str.clone(), None),
        };
        let selector = Selector::parse(&sel_part)
            .map_err(|e| SyntheticError::InvalidSpec(format!("field '{field}': {e}")))?;
        out.push((
            field.clone(),
            CompiledCssField {
                selector,
                attr,
            },
        ));
    }
    Ok(out)
}

/// Extract a single field from an element using a compiled CSS selector.
fn extract_field_css(
    element: ElementRef,
    sel: &CompiledCssField,
    base: &reqwest::Url,
) -> String {
    let matched = element.select(&sel.selector).next();
    let matched = match matched {
        Some(m) => m,
        None => return String::new(),
    };
    let value = if let Some(ref attr) = sel.attr {
        matched
            .attr(attr)
            .unwrap_or("")
            .to_string()
    } else {
        // Concatenate all text nodes, trimmed.
        matched
            .text()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    };
    // If this looks like a link field and the value is relative, resolve it.
    if sel.attr.as_deref() == Some("href") || sel.attr.as_deref() == Some("src") {
        if let Ok(abs) = base.join(&value) {
            return abs.to_string();
        }
    }
    value
}

// ── JSONPath extraction ───────────────────────────────────────────────────

/// Extract items from a JSON document using JSONPath expressions.
///
/// The spec's `items_selector` is a JSONPath selecting the array of items
/// (e.g. `"$.items[*]"` or `"$..paper"`)`. The spec's `fields` map field
/// names to JSONPath expressions evaluated against each item (e.g. `"$.title"`).
fn extract_json_path(
    spec: &ExtractorSpec,
    json: &Value,
) -> Result<Vec<ExtractedItem>, SyntheticError> {
    use jsonpath_rust::JsonPath;

    let items_path = spec
        .items_selector
        .as_deref()
        .ok_or_else(|| SyntheticError::MissingField("items_selector".into()))?;

    let item_refs: Vec<&Value> = json
        .query(items_path)
        .map_err(|e| SyntheticError::ExtractionFailed(format!("items_selector: {e}")))?;

    let template = spec
        .entry_id_template
        .as_deref()
        .unwrap_or("{link}");

    let mut items = Vec::new();
    for item_val in item_refs {
        let mut item = ExtractedItem::default();
        for (field, path) in &spec.fields {
            let value = query_json_field(item_val, path);
            match field.as_str() {
                "title" => item.title = value,
                "link" | "url" => item.link = value,
                "date" | "published" | "published_at" => item.date = value,
                "summary" | "description" => item.summary = value,
                "entry_id" | "id" => item.entry_id = value,
                _ => {} // ignore unknown fields
            }
        }
        if item.entry_id.is_empty() {
            item.entry_id = render_template(template, &item);
        }
        if item.entry_id.is_empty() {
            item.entry_id = if !item.link.is_empty() {
                item.link.clone()
            } else {
                blake3::hash(item.title.as_bytes()).to_hex().to_string()
            };
        }
        items.push(item);
    }

    Ok(items)
}

/// Query a single JSON field from an item value. Returns the string
/// representation of the first match, or empty string if no match.
fn query_json_field(item: &Value, path: &str) -> String {
    use jsonpath_rust::JsonPath;
    match item.query(path) {
        Ok(matches) => {
            if let Some(first) = matches.first() {
                json_value_to_string(first)
            } else {
                String::new()
            }
        }
        Err(_) => String::new(),
    }
}

/// Convert a JSON value to a string suitable for a feed entry field.
fn json_value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        // For arrays/objects, serialize to compact JSON.
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

// ── Template rendering ────────────────────────────────────────────────────

/// Render a template like `"{link}"` or `"{title}|{date}"` by substituting
/// field values from an extracted item.
fn render_template(template: &str, item: &ExtractedItem) -> String {
    let mut out = template.to_string();
    out = out.replace("{title}", &item.title);
    out = out.replace("{link}", &item.link);
    out = out.replace("{date}", &item.date);
    out = out.replace("{summary}", &item.summary);
    out = out.replace("{entry_id}", &item.entry_id);
    out
}

// ── Conversion to feed_rs entries ──────────────────────────────────────────

/// Convert extracted items into `feed_rs::Entry` objects for insertion
/// via the existing `insert_entries` function.
pub fn items_to_entries(items: Vec<ExtractedItem>, feed_title: &str) -> Vec<feed_rs::model::Entry> {
    items
        .into_iter()
        .map(|item| {
            let mut entry = feed_rs::model::Entry::default();
            entry.id = item.entry_id;
            if !item.title.is_empty() {
                entry.title = Some(feed_rs::model::Text {
                    content: item.title,
                    ..Default::default()
                });
            }
            if !item.link.is_empty() {
                entry.links = vec![feed_rs::model::Link {
                    href: item.link,
                    ..Default::default()
                }];
            }
            if !item.summary.is_empty() {
                entry.summary = Some(feed_rs::model::Text {
                    content: item.summary,
                    ..Default::default()
                });
            }
            if !item.date.is_empty() {
                if let Ok(dt) = DateTime::parse_from_rfc3339(&item.date) {
                    entry.published = Some(dt.with_timezone(&chrono::Utc).into());
                }
            }
            // Use feed_title as a fallback author if the entry has none.
            if entry.authors.is_empty() && !feed_title.is_empty() {
                entry.authors = vec![feed_rs::model::Person {
                    name: feed_title.to_string(),
                    ..Default::default()
                }];
            }
            entry
        })
        .collect()
}

/// Build a synthetic feed_rs::Feed for upsert_feed. The feed's `url` is
/// `synthetic://<feed_id_placeholder>` — the actual feed_id is assigned by
/// the DB and the url is updated after insert.
pub fn build_synthetic_feed(
    source_url: &str,
    title: &str,
    description: &str,
) -> feed_rs::model::Feed {
    let mut feed = feed_rs::model::Feed::default();
    feed.title = Some(feed_rs::model::Text {
        content: title.to_string(),
        ..Default::default()
    });
    feed.description = Some(feed_rs::model::Text {
        content: description.to_string(),
        ..Default::default()
    });
    feed.links = vec![feed_rs::model::Link {
        href: source_url.to_string(),
        ..Default::default()
    }];
    feed
}

/// Build a FetchResult-shaped object for the diff_hash path. This is a
/// single-entry "feed" where the entry_id is the content hash and the
/// content is the raw body.
pub fn build_diff_hash_feed(
    body: &[u8],
    source_url: &str,
    title: &str,
) -> (feed_rs::model::Feed, String) {
    let hash = content_hash(body);
    let mut feed = feed_rs::model::Feed::default();
    feed.title = Some(feed_rs::model::Text {
        content: title.to_string(),
        ..Default::default()
    });
    feed.links = vec![feed_rs::model::Link {
        href: source_url.to_string(),
        ..Default::default()
    }];

    let mut entry = feed_rs::model::Entry::default();
    entry.id = format!("diffhash:{hash}");
    entry.title = Some(feed_rs::model::Text {
        content: format!("{title} (updated)"),
        ..Default::default()
    });
    entry.published = Some(chrono::Utc::now().into());

    feed.entries = vec![entry];
    (feed, hash)
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_HTML: &str = r#"<!DOCTYPE html>
<html>
<head><title>Test News</title></head>
<body>
<div class="news-list">
  <div class="item">
    <h3><a href="/news/1">First Story</a></h3>
    <time datetime="2026-01-15T10:00:00Z">Jan 15, 2026</time>
    <p class="summary">Summary of first story</p>
  </div>
  <div class="item">
    <h3><a href="/news/2">Second Story</a></h3>
    <time datetime="2026-01-16T12:00:00Z">Jan 16, 2026</time>
    <p class="summary">Summary of second story</p>
  </div>
</div>
</body>
</html>"#;

    const SAMPLE_JSON: &str = r#"{
  "items": [
    {"title": "Paper A", "url": "https://example.com/a", "date": "2026-01-15T00:00:00Z", "abstract": "Abstract A"},
    {"title": "Paper B", "url": "https://example.com/b", "date": "2026-01-16T00:00:00Z", "abstract": "Abstract B"}
  ]
}"#;

    fn css_spec() -> ExtractorSpec {
        let mut fields = HashMap::new();
        fields.insert("title".into(), "h3 a".into());
        fields.insert("link".into(), "h3 a@href".into());
        fields.insert("date".into(), "time@datetime".into());
        fields.insert("summary".into(), ".summary".into());
        ExtractorSpec {
            items_selector: Some(".news-list .item".into()),
            fields,
            entry_id_template: Some("{link}".into()),
            base_url: Some("https://example.com".into()),
        }
    }

    fn json_spec() -> ExtractorSpec {
        let mut fields = HashMap::new();
        fields.insert("title".into(), "$.title".into());
        fields.insert("link".into(), "$.url".into());
        fields.insert("date".into(), "$.date".into());
        fields.insert("summary".into(), "$.abstract".into());
        ExtractorSpec {
            items_selector: Some("$.items[*]".into()),
            fields,
            entry_id_template: Some("{link}".into()),
            base_url: None,
        }
    }

    #[test]
    fn css_extracts_items() {
        let spec = css_spec();
        let items = extract(ExtractorKind::Css, &spec, "https://example.com/news", SAMPLE_HTML.as_bytes(), "text/html").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "First Story");
        assert_eq!(items[0].link, "https://example.com/news/1");
        assert_eq!(items[0].date, "2026-01-15T10:00:00Z");
        assert_eq!(items[0].summary, "Summary of first story");
        assert_eq!(items[0].entry_id, "https://example.com/news/1");
        assert_eq!(items[1].title, "Second Story");
    }

    #[test]
    fn css_resolves_relative_links() {
        let spec = css_spec();
        let items = extract(ExtractorKind::Css, &spec, "https://example.com/news", SAMPLE_HTML.as_bytes(), "text/html").unwrap();
        assert!(items[0].link.starts_with("https://example.com/"));
    }

    #[test]
    fn css_entry_id_template_uses_title() {
        let mut spec = css_spec();
        spec.entry_id_template = Some("{title}".into());
        let items = extract(ExtractorKind::Css, &spec, "https://example.com/news", SAMPLE_HTML.as_bytes(), "text/html").unwrap();
        assert_eq!(items[0].entry_id, "First Story");
    }

    #[test]
    fn css_falls_back_to_hash_when_no_link() {
        let mut spec = css_spec();
        spec.fields.remove("link");
        let items = extract(ExtractorKind::Css, &spec, "https://example.com/news", SAMPLE_HTML.as_bytes(), "text/html").unwrap();
        // entry_id falls back to blake3 of title since no link and no template match
        assert!(!items[0].entry_id.is_empty());
        assert_ne!(items[0].entry_id, "First Story"); // it's a hash, not the title
    }

    #[test]
    fn json_path_extracts_items() {
        let spec = json_spec();
        let items = extract(ExtractorKind::JsonPath, &spec, "https://example.com/api", SAMPLE_JSON.as_bytes(), "application/json").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Paper A");
        assert_eq!(items[0].link, "https://example.com/a");
        assert_eq!(items[0].date, "2026-01-15T00:00:00Z");
        assert_eq!(items[0].summary, "Abstract A");
        assert_eq!(items[0].entry_id, "https://example.com/a");
    }

    #[test]
    fn json_path_handles_missing_fields() {
        let json = r#"{"items": [{"title": "Only Title"}]}"#;
        let spec = json_spec();
        let items = extract(ExtractorKind::JsonPath, &spec, "https://example.com/api", json.as_bytes(), "application/json").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Only Title");
        assert!(items[0].link.is_empty());
        // entry_id falls back to hash of title
        assert!(!items[0].entry_id.is_empty());
    }

    #[test]
    fn content_hash_is_deterministic() {
        let body = b"hello world";
        let h1 = content_hash(body);
        let h2 = content_hash(body);
        assert_eq!(h1, h2);
        assert_ne!(h1, content_hash(b"hello world!"));
    }

    #[test]
    fn diff_hash_returns_empty_items() {
        let spec = ExtractorSpec {
            items_selector: None,
            fields: HashMap::new(),
            entry_id_template: None,
            base_url: None,
        };
        let items = extract(ExtractorKind::DiffHash, &spec, "https://example.com", b"some content", "text/html").unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn diff_hash_builds_single_entry_feed() {
        let body = b"page content here";
        let (feed, hash) = build_diff_hash_feed(body, "https://example.com", "Test Feed");
        assert_eq!(feed.entries.len(), 1);
        assert_eq!(feed.entries[0].id, format!("diffhash:{hash}"));
        assert!(hash.starts_with("0") || hash.len() == 64);
    }

    #[test]
    fn items_to_entries_preserves_fields() {
        let items = vec![ExtractedItem {
            title: "Test".into(),
            link: "https://example.com/test".into(),
            date: "2026-01-15T00:00:00Z".into(),
            summary: "A summary".into(),
            entry_id: "test-id".into(),
        }];
        let entries = items_to_entries(items, "Feed Title");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "test-id");
        assert_eq!(entries[0].title.as_ref().unwrap().content, "Test");
        assert_eq!(entries[0].links[0].href, "https://example.com/test");
        assert_eq!(entries[0].summary.as_ref().unwrap().content, "A summary");
        assert!(entries[0].published.is_some());
    }

    #[test]
    fn extractor_kind_round_trips() {
        assert_eq!("css".parse::<ExtractorKind>().unwrap(), ExtractorKind::Css);
        assert_eq!("json_path".parse::<ExtractorKind>().unwrap(), ExtractorKind::JsonPath);
        assert_eq!("diff_hash".parse::<ExtractorKind>().unwrap(), ExtractorKind::DiffHash);
        assert!("unknown".parse::<ExtractorKind>().is_err());
    }

    #[test]
    fn render_template_substitutes_all_fields() {
        let item = ExtractedItem {
            title: "T".into(),
            link: "L".into(),
            date: "D".into(),
            summary: "S".into(),
            entry_id: "I".into(),
        };
        let result = render_template("{title}|{link}|{date}|{summary}|{entry_id}", &item);
        assert_eq!(result, "T|L|D|S|I");
    }
}
