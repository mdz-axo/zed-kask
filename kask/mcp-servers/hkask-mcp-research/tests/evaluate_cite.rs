//! Functional tests for the evaluate_evidence and cite_sources tools.
//!
//! These tools are deterministic (no LLM relay, no external API calls), so
//! they can be tested without mocking providers. The tests verify:
//! - Request validation (empty inputs rejected)
//! - Deterministic signal computation (corroboration, recency, confidence)
//! - ESO-anchored output fields (eso:hasConfidence, eso:corroboratedBy)
//! - Citation formatting in all 4 styles (apa, bibtex, chicago, json)
//! - PKO-anchored output fields (pko:stepVerification, pko:referencesResource)

use hkask_mcp_research::ResearchServer;
use hkask_mcp_research::research::{
    CiteSource, CiteSourcesRequest, CiteStyle, EvaluateArtifact, EvaluateEvidenceRequest,
    RateLimiter, ResponseCache, build_provider_pool,
};
use hkask_types::WebID;
use rmcp::handler::server::wrapper::Parameters;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

fn test_server() -> ResearchServer {
    let pool = build_provider_pool(&HashMap::new()).expect("empty provider pool");
    ResearchServer::new(
        WebID::new(),
        Arc::new(pool),
        Arc::new(ResponseCache::new(10, Duration::from_secs(60))),
        RateLimiter::new(30, 60),
        None,
        reqwest::Client::new(),
    )
}

fn parse_content(out: &str) -> serde_json::Value {
    hkask_types::tool_response::parse_tool_response(out).expect("tool output is JSON")
}

// ── evaluate_evidence ─────────────────────────────────────────────────────

#[tokio::test]
async fn evaluate_evidence_rejects_empty_question() {
    let server = test_server();
    let req = EvaluateEvidenceRequest {
        question: "   ".to_string(),
        artifacts: vec![EvaluateArtifact {
            url: "https://example.com".to_string(),
            title: None,
            published: None,
            source: None,
            content: None,
        }],
    };
    let out = server.evaluate_evidence(Parameters(req)).await;
    let parsed = parse_content(&out);
    assert!(parsed.get("error").is_some() || parsed.get("kind").is_some());
}

#[tokio::test]
async fn evaluate_evidence_rejects_empty_artifacts() {
    let server = test_server();
    let req = EvaluateEvidenceRequest {
        question: "What causes climate change?".to_string(),
        artifacts: vec![],
    };
    let out = server.evaluate_evidence(Parameters(req)).await;
    let parsed = parse_content(&out);
    assert!(parsed.get("error").is_some() || parsed.get("kind").is_some());
}

#[tokio::test]
async fn evaluate_evidence_computes_deterministic_signals() {
    let server = test_server();
    let req = EvaluateEvidenceRequest {
        question: "Is Rust memory-safe?".to_string(),
        artifacts: vec![
            EvaluateArtifact {
                url: "https://rust-lang.org".to_string(),
                title: Some("Rust Safety".to_string()),
                published: Some("2024-01-15".to_string()),
                source: Some("rust-lang.org".to_string()),
                content: Some("Rust prevents memory errors".to_string()),
            },
            EvaluateArtifact {
                url: "https://example.com/rust".to_string(),
                title: Some("Rust Review".to_string()),
                published: Some("2023-06-01".to_string()),
                source: Some("rust-lang.org".to_string()),
                content: Some("Rust is safe".to_string()),
            },
            EvaluateArtifact {
                url: "https://blog.com/cpp".to_string(),
                title: Some("C++ vs Rust".to_string()),
                published: None,
                source: Some("blog.com".to_string()),
                content: None,
            },
        ],
    };
    let out = server.evaluate_evidence(Parameters(req)).await;
    let parsed = parse_content(&out);

    assert_eq!(parsed["question"], "Is Rust memory-safe?");
    assert_eq!(parsed["artifacts_evaluated"], 3);
    assert!(parsed["average_confidence"].is_number());

    let evaluations = parsed["evaluations"].as_array().expect("evaluations array");
    assert_eq!(evaluations.len(), 3);

    // The two rust-lang.org sources should corroborate each other.
    let rust_org_eval = evaluations
        .iter()
        .find(|e| e["url"] == "https://rust-lang.org")
        .expect("rust-lang.org evaluation");
    assert_eq!(rust_org_eval["corroboration_count"], 2);
    assert!(
        rust_org_eval["has_published_date"]
            .as_bool()
            .unwrap_or(false)
    );
    assert!(rust_org_eval["has_content"].as_bool().unwrap_or(false));
    assert!(rust_org_eval["confidence"].as_f64().unwrap_or(0.0) > 0.5);

    // The blog.com source has no date, no content, no corroboration.
    let blog_eval = evaluations
        .iter()
        .find(|e| e["url"] == "https://blog.com/cpp")
        .expect("blog.com evaluation");
    assert_eq!(blog_eval["corroboration_count"], 1);
    assert!(!blog_eval["has_published_date"].as_bool().unwrap_or(true));
    assert!(!blog_eval["has_content"].as_bool().unwrap_or(true));

    // ESO-anchored fields present.
    assert!(rust_org_eval["eso:hasConfidence"].is_string());
    assert!(rust_org_eval["eso:corroboratedBy"].is_string());

    // PKO-anchored field present.
    assert_eq!(parsed["pko:stepVerification"], "evidence_quality_assessed");
}

// ── cite_sources ──────────────────────────────────────────────────────────

#[tokio::test]
async fn cite_sources_rejects_empty_sources() {
    let server = test_server();
    let req = CiteSourcesRequest {
        sources: vec![],
        style: CiteStyle::Apa,
    };
    let out = server.cite_sources(Parameters(req)).await;
    let parsed = parse_content(&out);
    assert!(parsed.get("error").is_some() || parsed.get("kind").is_some());
}

#[tokio::test]
async fn cite_sources_formats_apa() {
    let server = test_server();
    let req = CiteSourcesRequest {
        sources: vec![CiteSource {
            url: "https://example.com/paper".to_string(),
            title: Some("On the Safety of Rust".to_string()),
            published: Some("2024-03-15".to_string()),
            source: Some("example.com".to_string()),
            authors: Some(vec!["Jane Doe".to_string(), "John Smith".to_string()]),
        }],
        style: CiteStyle::Apa,
    };
    let out = server.cite_sources(Parameters(req)).await;
    let parsed = parse_content(&out);

    assert_eq!(parsed["count"], 1);
    assert_eq!(parsed["style"], "apa");
    let citation = parsed["citations"][0].as_str().expect("citation string");
    assert!(citation.contains("Jane Doe, John Smith"));
    assert!(citation.contains("2024"));
    assert!(citation.contains("On the Safety of Rust"));
    assert!(citation.contains("https://example.com/paper"));
    assert_eq!(parsed["pko:referencesResource"], "citations_generated");
}

#[tokio::test]
async fn cite_sources_formats_bibtex() {
    let server = test_server();
    let req = CiteSourcesRequest {
        sources: vec![CiteSource {
            url: "https://example.com/paper".to_string(),
            title: Some("On the Safety of Rust".to_string()),
            published: Some("2024-03-15".to_string()),
            source: Some("example.com".to_string()),
            authors: Some(vec!["Jane Doe".to_string()]),
        }],
        style: CiteStyle::Bibtex,
    };
    let out = server.cite_sources(Parameters(req)).await;
    let parsed = parse_content(&out);

    let citation = parsed["citations"][0].as_str().expect("citation string");
    assert!(citation.starts_with("@misc{"));
    assert!(citation.contains("author = {Jane Doe}"));
    assert!(citation.contains("year = {2024}"));
    assert!(citation.contains("title = {On the Safety of Rust}"));
}

#[tokio::test]
async fn cite_sources_formats_chicago() {
    let server = test_server();
    let req = CiteSourcesRequest {
        sources: vec![CiteSource {
            url: "https://example.com/article".to_string(),
            title: Some("Memory Safety".to_string()),
            published: None,
            source: Some("example.com".to_string()),
            authors: None,
        }],
        style: CiteStyle::Chicago,
    };
    let out = server.cite_sources(Parameters(req)).await;
    let parsed = parse_content(&out);

    let citation = parsed["citations"][0].as_str().expect("citation string");
    assert!(citation.contains("Memory Safety"));
    assert!(citation.contains("https://example.com/article"));
}

#[tokio::test]
async fn cite_sources_formats_json() {
    let server = test_server();
    let req = CiteSourcesRequest {
        sources: vec![CiteSource {
            url: "https://example.com/paper".to_string(),
            title: Some("On the Safety of Rust".to_string()),
            published: Some("2024-03-15".to_string()),
            source: Some("example.com".to_string()),
            authors: Some(vec!["Jane Doe".to_string()]),
        }],
        style: CiteStyle::Json,
    };
    let out = server.cite_sources(Parameters(req)).await;
    let parsed = parse_content(&out);

    let citation_str = parsed["citations"][0].as_str().expect("citation string");
    let citation: serde_json::Value = serde_json::from_str(citation_str).expect("valid JSON");
    assert_eq!(citation["url"], "https://example.com/paper");
    assert_eq!(citation["title"], "On the Safety of Rust");
    assert_eq!(citation["year"], "2024");
}

#[tokio::test]
async fn cite_sources_handles_missing_fields() {
    let server = test_server();
    let req = CiteSourcesRequest {
        sources: vec![CiteSource {
            url: "https://example.com/untitled".to_string(),
            title: None,
            published: None,
            source: None,
            authors: None,
        }],
        style: CiteStyle::Apa,
    };
    let out = server.cite_sources(Parameters(req)).await;
    let parsed = parse_content(&out);

    let citation = parsed["citations"][0].as_str().expect("citation string");
    // Should fall back to "Anonymous" and "n.d." for missing fields.
    assert!(citation.contains("Anonymous") || citation.contains("example.com"));
    assert!(citation.contains("n.d.") || citation.contains("Untitled"));
}
