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

use scraper::{ElementRef, Html, Selector};

// FetchResult is used by the caller (rss_fetch_synthetic), not by this module.

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
    LlmSchema,
    PdfOcr,
    // Reserved for future implementation:
    // Xpath,
}

impl ExtractorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Css => "css",
            Self::JsonPath => "json_path",
            Self::DiffHash => "diff_hash",
            Self::LlmSchema => "llm_schema",
            Self::PdfOcr => "pdf_ocr",
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
            "llm_schema" => Ok(Self::LlmSchema),
            "pdf_ocr" => Ok(Self::PdfOcr),
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
    /// For `llm_schema`, unused (the LLM extracts items directly).
    /// For `pdf_ocr`, unused (the PDF text is passed to post_ocr extraction).
    pub items_selector: Option<String>,
    /// Per-field selectors. Keys: title, link, date, summary, entry_id.
    /// Values are CSS selectors (with optional `@attr`) or JSONPath expressions.
    /// For `llm_schema`, unused (the JSON schema defines the fields).
    pub fields: HashMap<String, String>,
    /// Optional template for building the entry_id from extracted fields.
    /// e.g. `"{link}"` or `"{title}|{date}"`. Defaults to the link field.
    pub entry_id_template: Option<String>,
    /// Optional base URL for resolving relative links (css only).
    /// Defaults to the source_url.
    pub base_url: Option<String>,
    /// For `llm_schema`: a JSON schema (as a string) defining the array of
    /// items to extract. The schema should describe a single item; the
    /// extractor wraps it in an array.
    pub json_schema: Option<String>,
    /// For `llm_schema`: a natural-language prompt guiding extraction.
    /// e.g. "Extract a list of datasets with title, url, date, and summary."
    pub prompt: Option<String>,
    /// For `pdf_ocr`: the post-OCR extractor kind. After PDF text extraction,
    /// the text is passed to this extractor. Currently supports "llm_schema"
    /// and "diff_hash". If None, defaults to "diff_hash".
    pub post_ocr_kind: Option<String>,
    /// For `pdf_ocr`: the post-OCR extractor spec (JSON-encoded ExtractorSpec).
    /// Used when `post_ocr_kind` is "llm_schema".
    pub post_ocr_spec: Option<String>,
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

impl From<SyntheticError> for hkask_mcp_server::server::McpToolError {
    fn from(e: SyntheticError) -> Self {
        use hkask_mcp_server::server::McpToolError;
        match e {
            SyntheticError::UnsupportedKind(_) | SyntheticError::InvalidSpec(_)
            | SyntheticError::MissingField(_) => McpToolError::invalid_argument(e.to_string()),
            SyntheticError::ExtractionFailed(_) => {
                McpToolError::unavailable(e.to_string())
            }
        }
    }
}

// ── Extraction entry point ─────────────────────────────────────────────────

/// Extract items from a fetched page/API response.
///
/// The `body` is the raw response bytes. The `content_type` is used to
/// decide whether to parse as HTML or JSON when the extractor kind doesn't
/// imply it (e.g. `css` always parses as HTML, `json_path` always as JSON,
/// `diff_hash` doesn't parse at all).
///
/// For `llm_schema`, this function does NOT perform the LLM extraction —
/// that requires the `WebSearchPort` pool and is done by `extract_llm_schema`.
/// This function returns an empty vec for `llm_schema`.
pub fn extract(
    kind: ExtractorKind,
    spec: &ExtractorSpec,
    source_url: &str,
    body: &[u8],
    content_type: &str,
) -> Result<Vec<ExtractedItem>, SyntheticError> {
    let _ = content_type; // reserved for future content-type-based dispatch
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
            Ok(Vec::new())
        }
        ExtractorKind::LlmSchema => {
            // LLM extraction requires the pool; caller uses extract_llm_schema.
            Ok(Vec::new())
        }
        ExtractorKind::PdfOcr => {
            // PDF text extraction is done by extract_pdf_text; caller handles.
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

// ── LLM schema extraction ───────────────────────────────────────────────────

/// Extract items from a URL using the LLM-based `web_extract` provider.
///
/// This calls the `WebSearchPort::extract` method with `format="json"`,
/// a JSON schema, and a prompt. The provider fetches the URL, extracts
/// content, and returns structured JSON matching the schema.
///
/// The returned JSON is expected to be an array of objects with fields
/// matching the schema (title, url, date, summary, etc.).
pub async fn extract_llm_schema(
    pool: &dyn crate::research::providers::WebSearchPort,
    spec: &ExtractorSpec,
    source_url: &str,
) -> Result<Vec<ExtractedItem>, SyntheticError> {
    let schema_str = spec
        .json_schema
        .as_deref()
        .ok_or_else(|| SyntheticError::MissingField("json_schema".into()))?;

    let schema: serde_json::Value = serde_json::from_str(schema_str)
        .map_err(|e| SyntheticError::InvalidSpec(format!("json_schema: {e}")))?;

    let prompt = spec
        .prompt
        .clone()
        .unwrap_or_else(|| "Extract a list of items with title, url, date, and summary.".into());

    let opts = crate::research::types::ExtractOptions {
        format: "json".to_string(),
        json_prompt: Some(prompt),
        json_schema: Some(schema),
        main_content_only: true,
        wait_for_ms: 0,
    };

    let result = pool
        .extract(source_url, &opts)
        .await
        .map_err(|e| SyntheticError::ExtractionFailed(format!("web_extract: {e}")))?;

    // The extracted content should be a JSON string.
    let parsed: serde_json::Value = serde_json::from_str(&result.content)
        .map_err(|e| SyntheticError::ExtractionFailed(format!("parse extracted JSON: {e}")))?;

    // The parsed value should be an array of items.
    let items_arr = match parsed {
        serde_json::Value::Array(arr) => arr,
        serde_json::Value::Object(obj) => {
            // Some providers wrap the array in an object with a key like "items".
            if let Some(arr) = obj.get("items").and_then(|v| v.as_array()) {
                arr.clone()
            } else {
                // Single object — treat as one item.
                vec![serde_json::json!({"_raw": serde_json::Value::Object(obj)})]
            }
        }
        other => vec![serde_json::json!({"_raw": other})],
    };

    let template = spec
        .entry_id_template
        .as_deref()
        .unwrap_or("{link}");

    let mut items = Vec::new();
    for item_val in items_arr {
        let mut item = ExtractedItem::default();
        if let Some(obj) = item_val.as_object() {
            for (key, val) in obj {
                let s = json_value_to_string(val);
                match key.as_str() {
                    "title" | "name" => item.title = s,
                    "link" | "url" | "href" => item.link = s,
                    "date" | "published" | "published_at" | "timestamp" => item.date = s,
                    "summary" | "description" | "abstract" => item.summary = s,
                    "entry_id" | "id" => item.entry_id = s,
                    _ => {} // ignore unknown fields
                }
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

// ── PDF text extraction ────────────────────────────────────────────────────

/// Extract text from a PDF byte slice using the `pdf-extract` crate.
///
/// This performs text-layer extraction only (no OCR). For scanned PDFs
/// without a text layer, the result will be empty — the caller should
/// fall back to the corpus server's `corpus_ocr` tool via MCP dispatch.
///
/// Returns the extracted text and whether the text layer was non-empty.
pub fn extract_pdf_text(body: &[u8]) -> Result<(String, bool), SyntheticError> {
    let text = pdf_extract::extract_text_from_mem(body)
        .map_err(|e| SyntheticError::ExtractionFailed(format!("pdf-extract: {e}")))?;
    let non_empty = !text.trim().is_empty();
    Ok((text, non_empty))
}

/// Extract items from a PDF by first extracting text, then applying a
/// post-OCR extractor (typically `llm_schema` or `diff_hash`).
///
/// For `diff_hash` post-processing: hashes the text and returns an empty
/// item list (caller uses `build_diff_hash_feed`).
/// For `llm_schema` post-processing: the text is sent to the LLM extractor
/// as if it were a web page. This requires the pool.
pub async fn extract_pdf_ocr(
    pool: &dyn crate::research::providers::WebSearchPort,
    spec: &ExtractorSpec,
    source_url: &str,
    body: &[u8],
) -> Result<Vec<ExtractedItem>, SyntheticError> {
    let (text, has_text) = extract_pdf_text(body)?;

    if !has_text {
        return Err(SyntheticError::ExtractionFailed(
            "PDF has no text layer; OCR fallback requires the corpus server's corpus_ocr tool"
                .to_string(),
        ));
    }

    let post_kind = spec
        .post_ocr_kind
        .as_deref()
        .unwrap_or("diff_hash");

    match post_kind {
        "diff_hash" => {
            // Hash the text and return empty items (caller uses build_diff_hash_feed).
            Ok(Vec::new())
        }
        "llm_schema" => {
            // Parse the post_ocr_spec as an ExtractorSpec and use it for LLM extraction.
            // We pass the extracted text as the "body" to the LLM extractor.
            // Since extract_llm_schema takes a URL (not raw text), we need to
            // use the pool's extract method directly with the text as content.
            let post_spec_str = spec
                .post_ocr_spec
                .as_deref()
                .ok_or_else(|| SyntheticError::MissingField("post_ocr_spec".into()))?;
            let post_spec: ExtractorSpec = serde_json::from_str(post_spec_str)
                .map_err(|e| SyntheticError::InvalidSpec(format!("post_ocr_spec: {e}")))?;

            let schema_str = post_spec
                .json_schema
                .as_deref()
                .ok_or_else(|| SyntheticError::MissingField("json_schema in post_ocr_spec".into()))?;
            let schema: serde_json::Value = serde_json::from_str(schema_str)
                .map_err(|e| SyntheticError::InvalidSpec(format!("json_schema: {e}")))?;

            let prompt = post_spec
                .prompt
                .clone()
                .unwrap_or_else(|| {
                    "Extract a list of items with title, date, and summary from this document.".into()
                });

            // Use the pool to extract from the source URL (the PDF URL).
            // The extract provider will fetch the URL; for PDFs, Firecrawl
            // or RawFetch may handle PDF content-type. If they don't, the
            // caller should use the corpus server's corpus_convert tool.
            let opts = crate::research::types::ExtractOptions {
                format: "json".to_string(),
                json_prompt: Some(format!("{prompt}\n\nDocument text:\n{text}")),
                json_schema: Some(schema),
                main_content_only: true,
                wait_for_ms: 0,
            };

            let result = pool
                .extract(source_url, &opts)
                .await
                .map_err(|e| SyntheticError::ExtractionFailed(format!("web_extract: {e}")))?;

            let parsed: serde_json::Value = serde_json::from_str(&result.content)
                .map_err(|e| SyntheticError::ExtractionFailed(format!("parse: {e}")))?;

            let items_arr = match parsed {
                serde_json::Value::Array(arr) => arr,
                serde_json::Value::Object(obj) => {
                    if let Some(arr) = obj.get("items").and_then(|v| v.as_array()) {
                        arr.clone()
                    } else {
                        vec![serde_json::json!({"_raw": serde_json::Value::Object(obj)})]
                    }
                }
                other => vec![serde_json::json!({"_raw": other})],
            };

            let template = post_spec
                .entry_id_template
                .as_deref()
                .unwrap_or("{link}");

            let mut items = Vec::new();
            for item_val in items_arr {
                let mut item = ExtractedItem::default();
                if let Some(obj) = item_val.as_object() {
                    for (key, val) in obj {
                        let s = json_value_to_string(val);
                        match key.as_str() {
                            "title" | "name" => item.title = s,
                            "link" | "url" | "href" => item.link = s,
                            "date" | "published" | "published_at" => item.date = s,
                            "summary" | "description" | "abstract" => item.summary = s,
                            "entry_id" | "id" => item.entry_id = s,
                            _ => {}
                        }
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
        other => Err(SyntheticError::InvalidSpec(format!(
            "unsupported post_ocr_kind: {other}"
        ))),
    }
}

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
            let title = if item.title.is_empty() {
                None
            } else {
                Some(make_text(&item.title))
            };
            let links = if item.link.is_empty() {
                Vec::new()
            } else {
                vec![feed_rs::model::Link {
                    href: item.link,
                    rel: None,
                    media_type: None,
                    href_lang: None,
                    title: None,
                    length: None,
                }]
            };
            let summary = if item.summary.is_empty() {
                None
            } else {
                Some(make_text(&item.summary))
            };
            let published = item
                .date
                .as_str()
                .parse::<DateTime<chrono::FixedOffset>>()
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc));
            let authors = if feed_title.is_empty() {
                Vec::new()
            } else {
                vec![feed_rs::model::Person {
                    name: feed_title.to_string(),
                    uri: None,
                    email: None,
                }]
            };
            feed_rs::model::Entry {
                id: item.entry_id,
                title,
                updated: published,
                authors,
                content: None,
                links,
                summary,
                categories: Vec::new(),
                contributors: Vec::new(),
                published,
                source: None,
                rights: None,
                media: Vec::new(),
                language: None,
                base: None,
            }
        })
        .collect()
}

/// Construct a `feed_rs::model::Text` with `text/plain` content type.
fn make_text(content: &str) -> feed_rs::model::Text {
    use mediatype::{names, MediaTypeBuf};
    feed_rs::model::Text {
        content_type: MediaTypeBuf::new(names::TEXT, names::PLAIN),
        src: None,
        content: content.trim().to_string(),
    }
}

/// Build a synthetic feed_rs::Feed for upsert_feed. The feed's `url` is
/// `synthetic://<feed_id_placeholder>` — the actual feed_id is assigned by
/// the DB and the url is updated after insert.
pub fn build_synthetic_feed(
    source_url: &str,
    title: &str,
    description: &str,
) -> feed_rs::model::Feed {
    feed_rs::model::Feed {
        feed_type: feed_rs::model::FeedType::Atom,
        id: source_url.to_string(),
        title: Some(make_text(title)),
        updated: Some(chrono::Utc::now()),
        authors: Vec::new(),
        description: Some(make_text(description)),
        links: vec![feed_rs::model::Link {
            href: source_url.to_string(),
            rel: None,
            media_type: None,
            href_lang: None,
            title: None,
            length: None,
        }],
        categories: Vec::new(),
        contributors: Vec::new(),
        generator: None,
        icon: None,
        language: None,
        logo: None,
        published: None,
        rating: None,
        rights: None,
        ttl: None,
        entries: Vec::new(),
    }
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
    let now = chrono::Utc::now();
    let entry = feed_rs::model::Entry {
        id: format!("diffhash:{hash}"),
        title: Some(make_text(&format!("{title} (updated)"))),
        updated: Some(now),
        authors: Vec::new(),
        content: None,
        links: vec![feed_rs::model::Link {
            href: source_url.to_string(),
            rel: None,
            media_type: None,
            href_lang: None,
            title: None,
            length: None,
        }],
        summary: None,
        categories: Vec::new(),
        contributors: Vec::new(),
        published: Some(now),
        source: None,
        rights: None,
        media: Vec::new(),
        language: None,
        base: None,
    };
    let feed = feed_rs::model::Feed {
        feed_type: feed_rs::model::FeedType::Atom,
        id: source_url.to_string(),
        title: Some(make_text(title)),
        updated: Some(now),
        authors: Vec::new(),
        description: None,
        links: vec![feed_rs::model::Link {
            href: source_url.to_string(),
            rel: None,
            media_type: None,
            href_lang: None,
            title: None,
            length: None,
        }],
        categories: Vec::new(),
        contributors: Vec::new(),
        generator: None,
        icon: None,
        language: None,
        logo: None,
        published: None,
        rating: None,
        rights: None,
        ttl: None,
        entries: vec![entry],
    };
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
            json_schema: None,
            prompt: None,
            post_ocr_kind: None,
            post_ocr_spec: None,
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
            json_schema: None,
            prompt: None,
            post_ocr_kind: None,
            post_ocr_spec: None,
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
            json_schema: None,
            prompt: None,
            post_ocr_kind: None,
            post_ocr_spec: None,
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
