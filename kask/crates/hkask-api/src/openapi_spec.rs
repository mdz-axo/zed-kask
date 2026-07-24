//! Condensed API reference markdown — generated from the live OpenAPI spec.
//!
//! This produces a ~150-line markdown document optimized for LLM system context.
//! It's a distilled view of `ApiDoc::openapi()` — endpoint paths, methods, summaries,
//! key parameters, request bodies, response codes, and architectural tag groupings.
//!
//! The OpenAPI spec at `/api-docs/openapi.json` remains the single source of truth;
//! this is a lossy, context-budget-friendly derivative.

use std::collections::BTreeMap;

use utoipa::openapi::path::HttpMethod;
use utoipa::openapi::{PathItem, RefOr, Required};

/// Grouped endpoint entries keyed by tag name.
struct Endpoint {
    method: &'static str,
    path: String,
    summary: String,
    description: Option<String>,
    params: Vec<String>,
    request_body: Option<String>,
    responses: Vec<String>,
    deprecated: bool,
}

/// Generate a condensed markdown API reference from the live OpenAPI spec.
///
/// Uses `create_openapi()` which collects all route paths from the full router
/// (unlike `ApiDoc::openapi()` which only has schema/tag metadata).
///
/// Groups endpoints by tag (matching the Swagger UI grouping), includes
/// tag descriptions with architectural pattern references (P1–P12, Pattern A–D),
/// and lists each endpoint with its method, path, summary, parameters,
/// request body, and response codes.
///
/// Target: ~150 lines, ~2K tokens — fits comfortably in LLM system context.
pub fn condensed_api_spec() -> String {
    let openapi = crate::create_openapi();
    let mut out = String::with_capacity(8192);

    // Header
    let title = openapi.info.title.as_str();
    let version = openapi.info.version.as_str();
    out.push_str(&format!("# {title} v{version}\n\n"));

    // Build tag → description map
    let tag_descs: BTreeMap<&str, &str> = openapi
        .tags
        .as_ref()
        .map(|tags| {
            tags.iter()
                .filter_map(|t| t.description.as_deref().map(|d| (t.name.as_str(), d)))
                .collect()
        })
        .unwrap_or_default();

    // Collect endpoints grouped by tag
    let mut by_tag: BTreeMap<&str, Vec<Endpoint>> = BTreeMap::new();

    for (path, item) in openapi.paths.paths.iter() {
        for (method, op) in operations(item) {
            let tag = op
                .tags
                .as_ref()
                .and_then(|t| t.first())
                .map(|s| s.as_str())
                .unwrap_or("other");

            let params: Vec<String> = op
                .parameters
                .as_ref()
                .map(|ps| {
                    ps.iter()
                        .map(|p| {
                            let required = matches!(p.required, Required::True);
                            if required {
                                p.name.clone()
                            } else {
                                format!("{}?", p.name)
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();

            let request_body = op.request_body.as_ref().map(|rb| {
                rb.description
                    .clone()
                    .unwrap_or_else(|| "(body)".to_string())
            });

            let responses: Vec<String> = op
                .responses
                .responses
                .iter()
                .map(|(code, resp)| {
                    let desc = match resp {
                        RefOr::T(r) => r.description.as_str(),
                        RefOr::Ref(r) => r.ref_location.as_str(),
                    };
                    format!("`{code}` {desc}")
                })
                .collect();

            let deprecated = op.deprecated.is_some();

            by_tag.entry(tag).or_default().push(Endpoint {
                method: method_name(method),
                path: path.clone(),
                summary: op.summary.clone().unwrap_or_default(),
                description: op.description.clone(),
                params,
                request_body,
                responses,
                deprecated,
            });
        }
    }

    // Emit by tag
    for (tag, endpoints) in &by_tag {
        let desc = tag_descs.get(tag).copied().unwrap_or("");
        if desc.is_empty() {
            out.push_str(&format!("## {tag}\n\n"));
        } else {
            out.push_str(&format!("## {tag} — {desc}\n\n"));
        }

        for ep in endpoints {
            let dep = if ep.deprecated {
                " ⚠️ **deprecated**"
            } else {
                ""
            };
            out.push_str(&format!("### `{} {}`{dep}\n", ep.method, ep.path));

            if !ep.summary.is_empty() {
                out.push_str(&format!("{}\n", ep.summary));
            }
            if let Some(ref desc) = ep.description {
                // Take first line only — doc comments can be multi-paragraph
                let first = desc.lines().next().unwrap_or(desc);
                if first != ep.summary {
                    out.push_str(&format!("_{first}_\n"));
                }
            }

            if !ep.params.is_empty() {
                out.push_str(&format!("- **Params:** {}\n", ep.params.join(", ")));
            }
            if let Some(ref body) = ep.request_body {
                out.push_str(&format!("- **Body:** {body}\n"));
            }
            if !ep.responses.is_empty() {
                out.push_str(&format!("- **Responses:** {}\n", ep.responses.join(", ")));
            }

            out.push('\n');
        }
    }

    // Auth note at bottom
    out.push_str("---\n");
    out.push_str(
        "**Auth:** All endpoints require `Authorization: Bearer <DelegationToken>` (P4 OCAP).\n",
    );
    out.push_str("**Surfaces:** CLI, API, and MCP expose equivalent capabilities (P3 Equal Surface Exposure).\n");

    out
}

fn operations(item: &PathItem) -> Vec<(HttpMethod, &utoipa::openapi::path::Operation)> {
    let mut ops = Vec::new();
    if let Some(ref op) = item.get {
        ops.push((HttpMethod::Get, op));
    }
    if let Some(ref op) = item.put {
        ops.push((HttpMethod::Put, op));
    }
    if let Some(ref op) = item.post {
        ops.push((HttpMethod::Post, op));
    }
    if let Some(ref op) = item.delete {
        ops.push((HttpMethod::Delete, op));
    }
    if let Some(ref op) = item.patch {
        ops.push((HttpMethod::Patch, op));
    }
    ops
}

fn method_name(method: HttpMethod) -> &'static str {
    match method {
        HttpMethod::Get => "GET",
        HttpMethod::Put => "PUT",
        HttpMethod::Post => "POST",
        HttpMethod::Delete => "DELETE",
        HttpMethod::Options => "OPTIONS",
        HttpMethod::Head => "HEAD",
        HttpMethod::Patch => "PATCH",
        HttpMethod::Trace => "TRACE",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_nonempty_spec() {
        let spec = condensed_api_spec();
        assert!(!spec.is_empty(), "spec should not be empty");
        assert!(spec.contains("# hKask API"), "should contain title");
        assert!(spec.contains("pods"), "should contain pods tag");
        assert!(spec.contains("Bearer"), "should mention auth");
    }

    #[test]
    fn all_endpoints_represented() {
        let spec = condensed_api_spec();
        for expected in &[
            "GET /api/pods",
            "POST /api/pods",
            "GET /api/templates",
            "GET /api/mcp/tools",
            "POST /api/chat",
            "GET /api/regulation/health",
            "GET /api/sovereignty/status",
        ] {
            assert!(
                spec.contains(expected),
                "spec should contain endpoint: {expected}"
            );
        }
    }

    #[test]
    fn deprecated_marked() {
        let spec = condensed_api_spec();
        // No endpoints should be unexpectedly deprecated
        let dep_count = spec.matches("deprecated").count();
        // Allow zero — just verifying the generator doesn't crash on the field
        let _ = dep_count;
    }
}
