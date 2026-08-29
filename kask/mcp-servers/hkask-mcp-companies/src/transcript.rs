//! FMP earnings-call transcript fetch — `company_transcript` (earnings mode).
//!
//! Design: the `listening` skill (`.agents/skills/listening/SKILL.md`) §(a)
//! (original design doc in git history).
//! Fetches FMP `/stable/earning-call-transcript` for a window of quarters.
//! Coverage-honest: per-quarter failures collected into `coverage.missing`,
//! not propagated as whole-tool failure; the tool fails only when zero
//! quarters succeed.
//!
//! Probe-verified (earnings doc A2, 2026-08-05):
//! - Endpoint: `GET /stable/earning-call-transcript?symbol=&year=&quarter=`
//! - Response: array of `{symbol, period, year, date, content}`; empty `[]` = no call.
//! - `date` is UNRELIABLE (AAPL 2023Q1 → `date: "2012-03-19"`). `(year, quarter)`
//!   is the temporal key, never `date`.
//! - Legacy v3 endpoint is 403 — `/stable/` only.

use hkask_mcp_server::server::{McpToolError, classify_http_error};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const FMP_TRANSCRIPT_PATH: &str = "/earning-call-transcript";

// ── Public types (the result shape) ────────────────────────────────────────

/// One fetched earnings-call transcript. `(year, quarter)` is the canonical
/// temporal key; `date` is display-only (probe-verified unreliable).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TranscriptRecord {
    pub symbol: String,
    pub year: u32,
    pub quarter: u8,
    /// FMP `period` label verbatim (e.g. `"Q1"`). Diagnostics only.
    pub period: String,
    /// FMP `date` field verbatim. UNRELIABLE — display only.
    pub date: String,
    pub content: String,
    /// Provenance for the corpus pipeline's `dc:source`.
    pub source_endpoint: String,
    /// The `entity_ref_prefix` for the corpus pipeline (design §B3 convention:
    /// `company:{symbol}:earnings:{year}_Q{quarter}`). The agent passes this
    /// to `corpus_chunk`'s `entity_ref_prefix` parameter — this field is the
    /// enforcement point so the agent can't drift from the convention.
    pub entity_ref_prefix: String,
    /// Lightweight source footnote — a single human-readable attribution string
    /// (e.g. `"FMP earnings-call transcript — AAPL 2024 Q1"`). Carries the
    /// provenance the agent surfaces to the user without parsing multiple fields.
    pub attribution: String,
}

/// Why a requested quarter is missing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MissingReason {
    /// FMP returned an empty array — no call that quarter.
    NoCall,
    /// FMP returned a non-2xx status.
    HttpError { status: u16, message: String },
    /// FMP returned 2xx but the body could not be parsed.
    ParseError { message: String },
}

/// One entry in `coverage.missing`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct MissingQuarter {
    pub year: u32,
    pub quarter: u8,
    pub reason: MissingReason,
}

/// Coverage accounting — the honesty surface. Gaps reported, never filled.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TranscriptCoverage {
    pub requested_quarters: u32,
    pub retrieved_quarters: u32,
    pub missing: Vec<MissingQuarter>,
}

/// The full `company_transcript` result envelope.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TranscriptResult {
    pub transcripts: Vec<TranscriptRecord>,
    pub coverage: TranscriptCoverage,
}

// ── Quarter arithmetic ──────────────────────────────────────────────────────

/// A `(year, quarter)` pair — the temporal key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct YearQuarter {
    pub year: u32,
    pub quarter: u8,
}

impl YearQuarter {
    pub fn new(year: u32, quarter: u8) -> Option<Self> {
        if !(1..=4).contains(&quarter) {
            return None;
        }
        Some(Self { year, quarter })
    }

    /// The previous calendar quarter. `None` only on year-0 underflow.
    pub fn previous(self) -> Option<Self> {
        if self.quarter == 1 {
            self.year
                .checked_sub(1)
                .map(|year| Self { year, quarter: 4 })
        } else {
            Some(Self {
                year: self.year,
                quarter: self.quarter - 1,
            })
        }
    }

    /// The `quarters_back` quarters ending at `self` (inclusive), most-recent first.
    /// Stops early if `year` underflows.
    pub fn window(self, quarters_back: u32) -> Vec<YearQuarter> {
        let mut out = Vec::with_capacity(quarters_back as usize);
        let mut current = Some(self);
        for _ in 0..quarters_back {
            let Some(q) = current else { break };
            out.push(q);
            current = q.previous();
        }
        out
    }
}

// ── Fetch + coverage ────────────────────────────────────────────────────────

/// Fetch a window of quarters ending at `end`, collecting per-quarter failures
/// into `coverage.missing`. The tool fails only when zero quarters succeed.
pub async fn fetch_transcript_window(
    client: &reqwest::Client,
    symbol: &str,
    end: YearQuarter,
    quarters_back: u32,
    fmp_api_key: &str,
) -> TranscriptResult {
    let window = end.window(quarters_back);
    let requested = window.len() as u32;
    let mut transcripts = Vec::new();
    let mut missing = Vec::new();

    for quarter in window {
        match fetch_one_quarter(client, symbol, quarter.year, quarter.quarter, fmp_api_key).await {
            Ok(Some(record)) => transcripts.push(record),
            Ok(None) => missing.push(MissingQuarter {
                year: quarter.year,
                quarter: quarter.quarter,
                reason: MissingReason::NoCall,
            }),
            Err(reason) => missing.push(MissingQuarter {
                year: quarter.year,
                quarter: quarter.quarter,
                reason,
            }),
        }
    }

    let retrieved_quarters = transcripts.len() as u32;
    TranscriptResult {
        transcripts,
        coverage: TranscriptCoverage {
            requested_quarters: requested,
            retrieved_quarters,
            missing,
        },
    }
}

/// Fetch one quarter from FMP. `Ok(None)` = empty array (no call); `Err` =
/// fetch/parse failure classified into a `MissingReason`.
async fn fetch_one_quarter(
    client: &reqwest::Client,
    symbol: &str,
    year: u32,
    quarter: u8,
    fmp_api_key: &str,
) -> Result<Option<TranscriptRecord>, MissingReason> {
    let url = format!("https://financialmodelingprep.com/stable{FMP_TRANSCRIPT_PATH}");
    let year_str = year.to_string();
    let quarter_str = quarter.to_string();
    let query: Vec<(&str, &str)> = vec![
        ("symbol", symbol),
        ("year", &year_str),
        ("quarter", &quarter_str),
        ("apikey", fmp_api_key),
    ];

    let response = client
        .get(&url)
        .query(&query)
        .send()
        .await
        .map_err(|error| MissingReason::HttpError {
            status: 0,
            message: format!("FMP transcript request failed: {error}"),
        })?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| MissingReason::HttpError {
            status: status.as_u16(),
            message: format!("FMP transcript body read failed: {error}"),
        })?;

    if !status.is_success() {
        return Err(classify_fmp_status(status, &body));
    }
    if body.trim() == "[]" {
        return Ok(None);
    }
    parse_fmp_body(&body, symbol, year, quarter).map_err(|error| MissingReason::ParseError {
        message: error.to_string(),
    })
}

/// Classify an FMP non-2xx status into a `MissingReason`. 404 and empty-`[]`
/// bodies are `NoCall`; other statuses use `classify_http_error` for the message.
fn classify_fmp_status(status: reqwest::StatusCode, body: &str) -> MissingReason {
    if status.as_u16() == 404 || body.trim() == "[]" {
        MissingReason::NoCall
    } else {
        MissingReason::HttpError {
            status: status.as_u16(),
            message: classify_http_error("FMP", status, body).to_string(),
        }
    }
}

/// Parse the FMP response body for one quarter. The requested `year`/`quarter`
/// are authoritative for the temporal key (the FMP `date`/`year` labels are
/// unreliable). `year` may be a string or number in the response; both coerce.
fn parse_fmp_body(
    body: &str,
    symbol: &str,
    year: u32,
    quarter: u8,
) -> Result<Option<TranscriptRecord>, McpToolError> {
    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct FmpEntry {
        symbol: String,
        period: String,
        year: serde_json::Value,
        date: String,
        content: String,
    }

    let entries: Vec<FmpEntry> = serde_json::from_str(body).map_err(|error| {
        McpToolError::unavailable(format!(
            "FMP transcript parse failed for {symbol} {year}Q{quarter}: {error}"
        ))
    })?;
    let Some(entry) = entries.into_iter().next() else {
        return Ok(None);
    };
    Ok(Some(TranscriptRecord {
        symbol: entry.symbol.clone(),
        year,
        quarter,
        period: entry.period,
        date: entry.date,
        content: entry.content,
        source_endpoint: format!(
            "fmp:/stable{FMP_TRANSCRIPT_PATH}?symbol={symbol}&year={year}&quarter={quarter}"
        ),
        entity_ref_prefix: format!("company:{symbol}:earnings:{year}_Q{quarter}"),
        attribution: format!("FMP earnings-call transcript — {symbol} {year} Q{quarter}"),
    }))
}

// ── Corpus ingestion ──────────────────────────────────────────────────────────
//
// Design: company-corpus-design.md §B3. The pipeline is:
//   corpus_chunk(text, entity_ref_prefix) → corpus_tag_chunks → corpus_embed
//   → corpus_extract_assertions → centroids → corpus_query
// The tools already exist on the corpus server. The only transcript-specific
// logic is the entity-ref convention: `{company}:{kind}:{date}` so chunks,
// tags, h_mems, and centroids all reference the same provenance.
// The `entity_ref_prefix` is built in `parse_fmp_body` and carried on each
// `TranscriptRecord` — the agent reads it from the tool output and passes it
// to `corpus_chunk`, so the convention can't drift.

// ── Corpus mode (non-earnings transcripts via SerpAPI YouTube) ──────────────
//
// Design: company-corpus-design.md §B1 corpus mode + §B6 slice 3.
// Fetches non-earnings company transcripts (investor-day keynotes, executive
// interviews) via SerpAPI YouTube, channel-allowlisted. Does NOT segment.

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct CorpusTranscriptRecord {
    pub symbol: String,
    pub source_tier: u8,
    pub kind: String,
    pub title: String,
    pub url: String,
    pub channel: String,
    pub content: String,
    pub entity_ref_prefix: String,
    /// Lightweight source footnote (e.g. `"YouTube transcript — CNBC: Satya Nadella keynote"`).
    pub attribution: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ExcludedVideo {
    pub title: String,
    pub url: String,
    pub channel: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct CorpusTranscriptResult {
    pub transcripts: Vec<CorpusTranscriptRecord>,
    pub excluded: Vec<ExcludedVideo>,
}

const SERPAPI_BASE: &str = "https://serpapi.com/search";

pub async fn fetch_corpus_transcripts(
    client: &reqwest::Client,
    symbol: &str,
    query: &str,
    channels_allowlist: &[String],
    max_results: u32,
    serpapi_key: &str,
) -> Result<CorpusTranscriptResult, McpToolError> {
    let search_params: Vec<(&str, String)> = vec![
        ("q", query.to_string()),
        ("api_key", serpapi_key.to_string()),
        ("engine", "youtube".to_string()),
        ("num", max_results.to_string()),
    ];
    let search_response = client
        .get(SERPAPI_BASE)
        .query(&search_params)
        .send()
        .await
        .map_err(|error| {
            McpToolError::unavailable(format!("SerpAPI YouTube search failed: {error}"))
        })?;
    let search_body = search_response.text().await.map_err(|error| {
        McpToolError::unavailable(format!("SerpAPI search body read failed: {error}"))
    })?;
    let search_json: serde_json::Value = serde_json::from_str(&search_body).map_err(|error| {
        McpToolError::unavailable(format!("SerpAPI search returned malformed JSON: {error}"))
    })?;
    let video_results = search_json["video_results"]
        .as_array()
        .map(|array| {
            array
                .iter()
                .filter_map(|video| {
                    let title = video["title"].as_str()?.to_string();
                    let link = video["link"].as_str()?.to_string();
                    let channel = video["channel"]
                        .as_str()
                        .or_else(|| video["channel"]["name"].as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    Some((title, link, channel))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut transcripts = Vec::new();
    let mut excluded = Vec::new();
    for (title, url, channel) in video_results {
        let channel_allowed = channels_allowlist
            .iter()
            .any(|allowed| channel.contains(allowed.as_str()));
        if !channel_allowed {
            excluded.push(ExcludedVideo {
                title,
                url,
                channel,
                reason: "channel not on allowlist".to_string(),
            });
            continue;
        }
        let Some(video_id) = hkask_types::url_utils::extract_youtube_id(&url) else {
            excluded.push(ExcludedVideo {
                title,
                url,
                channel,
                reason: "could not extract video ID".to_string(),
            });
            continue;
        };
        match fetch_youtube_transcript(client, &video_id, serpapi_key).await {
            Ok(Some(content)) => {
                transcripts.push(CorpusTranscriptRecord {
                    attribution: format!("YouTube transcript — {channel}: {title}"),
                    symbol: symbol.to_string(),
                    source_tier: 2,
                    kind: "youtube".to_string(),
                    title,
                    url,
                    channel,
                    content,
                    entity_ref_prefix: youtube_entity_ref_prefix(symbol, &video_id),
                });
            }
            Ok(None) => {
                excluded.push(ExcludedVideo {
                    title,
                    url,
                    channel,
                    reason: "no transcript available".to_string(),
                });
            }
            Err(error) => {
                excluded.push(ExcludedVideo {
                    title,
                    url,
                    channel,
                    reason: format!("transcript fetch failed: {error}"),
                });
            }
        }
    }
    Ok(CorpusTranscriptResult {
        transcripts,
        excluded,
    })
}

async fn fetch_youtube_transcript(
    client: &reqwest::Client,
    video_id: &str,
    serpapi_key: &str,
) -> anyhow::Result<Option<String>> {
    let params: Vec<(&str, String)> = vec![
        ("v", video_id.to_string()),
        ("api_key", serpapi_key.to_string()),
        ("engine", "youtube_video_transcript".to_string()),
    ];
    let response = client
        .get(SERPAPI_BASE)
        .query(&params)
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("request failed: {error}"))?;
    let body = response
        .text()
        .await
        .map_err(|error| anyhow::anyhow!("body read failed: {error}"))?;
    let parsed: serde_json::Value =
        serde_json::from_str(&body).map_err(|error| anyhow::anyhow!("malformed JSON: {error}"))?;
    let snippets = parsed["transcripts"]
        .as_array()
        .or_else(|| parsed["organic_results"].as_array())
        .or_else(|| parsed["results"].as_array());
    let Some(snippets) = snippets else {
        return Ok(None);
    };
    let content: String = snippets
        .iter()
        .filter_map(|snippet| {
            snippet["snippet"]
                .as_str()
                .or_else(|| snippet["text"].as_str())
                .map(|text| text.to_string())
        })
        .collect::<Vec<_>>()
        .join(" ");
    if content.is_empty() {
        Ok(None)
    } else {
        Ok(Some(content))
    }
}

// ── Full-document entity-ref conventions (C5) ──────────────────────────────────

/// Build the entity-ref prefix for a YouTube transcript. The single source of
/// truth for the `company:{symbol}:youtube:{video_id}` format — wired into
/// `fetch_corpus_transcripts` output so the convention is load-bearing, not
/// advisory.
pub(crate) fn youtube_entity_ref_prefix(symbol: &str, video_id: &str) -> String {
    format!("company:{symbol}:youtube:{video_id}")
}
