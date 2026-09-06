//! Storage and query tools — cache, passage query, similarity.
use crate::helpers::map_corpus_io_error;
use crate::index::RetrievedPassage;
use crate::{
    CorpusServer, LLMParameters, McpToolError, Parameters,
    execute_tool, json, render_docproc_template, tool, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;

#[tool_router(router = storage_router, vis = "pub")]
impl CorpusServer {
    #[tool(
        description = "Cache processed document text for reference. Stores content keyed by label in the corpus cache directory (corpus-mcp/cache/ under the visible artifacts dir, ~/Documents/zk-data/)."
    )]
    pub async fn corpus_cache(
        &self,
        Parameters(CacheRequest { content, label }): Parameters<CacheRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_cache", async {
            if content.is_empty() {
                return Err(McpToolError::invalid_argument("content must not be empty"));
            }

            if label.is_empty() {
                return Err(McpToolError::invalid_argument("label must not be empty"));
            }

            // Canonical storage route. Cache directory lives at
            // `{artifacts_dir}/corpus-mcp/cache/` (visible under
            // ~/Documents/zk-data/).
            let cache_dir = hkask_types::agent_paths::resolve_under_artifacts_dir(
                &hkask_types::agent_paths::mcp_artifacts_subdir("corpus", "cache"),
            );

            if let Err(e) = std::fs::create_dir_all(&cache_dir) {
                return Err(map_corpus_io_error(
                    e,
                    &format!("Failed to create cache directory '{}'", cache_dir.display()),
                ));
            }

            // Sanitize label for filesystem
            let safe_label: String = label
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            let cache_path = cache_dir.join(format!("{}.md", safe_label));

            match std::fs::write(&cache_path, &content) {
                Ok(()) => {
                    let result = json!({
                        "label": label,
                        "path": cache_path.display().to_string(),
                        "size_bytes": content.len(),
                    });
                    Ok(result)
                }
                Err(e) => Err(map_corpus_io_error(
                    e,
                    &format!("Failed to write cache file '{}'", cache_path.display()),
                )),
            }
        })
        .await
    }

    #[tool(
        description = "Query the in-memory vector index for passages relevant to a natural language question. Embeds the query, computes cosine similarity against indexed passages, and returns top-k results. Optionally generates an LLM-augmented answer from retrieved context."
    )]
    /// Query the in-memory vector index for top-k relevant passages.
    ///
    /// expect: Answers use retrieved original text even when result text is hidden.
    /// [P8] Motivating: grounded answers independent of output projection.
    /// pre: plain or Lisp query; post: generation receives normalized question and
    /// usable passages, or answer_error reports why grounding was unavailable.
    pub async fn corpus_query(
        &self,
        Parameters(QueryRequest {
            query,
            top_k,
            generate_answer,
            include_text,
            min_score,
            db_path,
            passphrase,
        }): Parameters<QueryRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_query", async {
            if query.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "query must not be empty",
                ));
            }

            // Parse the query string. When it starts with `(list`, it's a
            // Lisp S-expression specifying query options. Otherwise it's a
            // plain natural-language query (backward compatible).
            let (nl_query, k, include_text_flag, min_score_val, gen_answer) =
                if query.trim_start().starts_with("(list") {
                    parse_lisp_query(&query)?
                } else {
                    (
                        query.clone(),
                        top_k.unwrap_or(5).clamp(1, 50),
                        include_text.unwrap_or(false),
                        min_score.unwrap_or(0.0),
                        generate_answer.unwrap_or(false),
                    )
                };

            let k = k.clamp(1, 50);
            // Fail-visible: no configured embedding model is a typed error
            // naming the setting — never a hidden constant.
            let model_name = crate::default_embedding_model().ok_or_else(|| {
                McpToolError::permission_denied(
                    "no embedding model configured — set \\
                     kask.models.embedding_model (injected as \\
                     HKASK_EMBEDDING_MODEL); kask never falls back to a \\
                     hidden code constant",
                )
            })?;

            self.index.hydrate_if_empty(db_path.as_deref(), passphrase.as_deref())?;

            let query_embedding = match self
                .inference_router
                .embed(&model_name, std::slice::from_ref(&nl_query))
                .await
            {
                Ok(v) => v.into_iter().next().unwrap_or_default(),
                Err(e) => {
                    return Err(McpToolError::unavailable(format!(
                        "Query embedding failed: {}",
                        e
                    )));
                }
            };

            if query_embedding.is_empty() {
                return Err(McpToolError::unavailable(
                    "Query embedding returned empty vector",
                ));
            }

            let crate::index::Retrieval { matches, total_indexed, missing_text } =
                self.index.retrieve(&query_embedding, k, min_score_val)?;
            let results: Vec<_> = matches.iter().map(|matched| matched.project(include_text_flag)).collect();

            let mut result = json!({
                "query": nl_query,
                "results": results,
                "total_indexed": total_indexed,
            });

            if total_indexed == 0 {
                result["note"] = json!("Index empty. Provide db_path for empty-index hydration, or embed passages.");
            }
            if missing_text > 0 {
                result["missing_passage_text"] = json!(missing_text);
                result["note"] = json!("Some persisted embeddings have no passage_text (legacy or non-passage rows); omitted from answer context. Re-embed original sources to restore grounding.");
            }
            let context = matches.iter().filter_map(|matched| matched.passage.text.as_deref())
                .filter(|text| !text.trim().is_empty()).collect::<Vec<_>>().join("\n\n");
            if gen_answer && context.is_empty() {
                result["answer_error"] = json!("No usable passage text retrieved; cannot generate a grounded answer. Re-embed original sources if persisted passage_text is missing.");
            } else if gen_answer {
                let context = crate::guard_content(&context);

                // C10: Load prompt from registry template, fall back to inline if unavailable
                let mut vars = std::collections::HashMap::new();
                vars.insert("context", context.clone());
                vars.insert("question", nl_query.clone());
                let prompt = render_docproc_template("rag-answer", &vars);
                let prompt = if prompt.is_empty() {
                    format!(
                        "{CONTENT_GUARD_INSTRUCTION}Answer the following question based on the provided context. If the context doesn't contain enough information, say so.\n\n\
                         Context:\n{context}\n\n\
                         Question: {nl_query}\n\n\
                         Answer:",
                        CONTENT_GUARD_INSTRUCTION = crate::CONTENT_GUARD_INSTRUCTION
                    )
                } else {
                    prompt
                };

                let params = LLMParameters {
                    temperature: 0.3,
                    ..Default::default()
                };

                match self.inference_router.generate(&prompt, &params, None).await {
                    Ok(response) => {
                        result["answer"] = json!(response.text);
                        result["answer_tokens"] = json!(response.usage.total_tokens);
                    }
                    Err(e) => {
                        result["answer_error"] = json!(format!("{}", e));
                    }
                }
            }

            Ok(result)
        })
        .await
    }

    #[tool(
        description = "Clear the in-memory vector index. Call this when starting a new document set to avoid cross-document contamination in query results."
    )]
    pub async fn corpus_clear_index(
        &self,
        Parameters(ClearIndexRequest {}): Parameters<ClearIndexRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_clear_index", async {
            let cleared = self.index.clear()?;
            Ok(json!({"cleared": cleared}))
        })
        .await
    }

    #[tool(
        description = "Purge QA embeddings and h_mems by entity-ref prefix. Deletes embeddings matching the prefix, then deletes h_mems with matching entity or attribute. Useful for clearing old training data before re-ingesting."
    )]
    pub async fn corpus_purge_qa(
        &self,
        Parameters(req): Parameters<PurgeQaRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_purge_qa", async {
            if req.passphrase.is_empty() {
                return Err(McpToolError::permission_denied(
                    "HKASK_DB_PASSPHRASE not configured — corpus_purge_qa requires the DB passphrase. \
                     Set it via the keychain (kask://credentials/hkask_db_passphrase) or environment variable."
                ));
            }
            self.index.purge(&req.db_path, &req.passphrase, &req.prefix)
        })
        .await
    }
}

// ── Request structs ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CacheRequest {
    /// Text content to cache.
    pub content: String,
    /// Label/key for the cached entry.
    pub label: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct QueryRequest {
    /// Natural language question to search for, OR a Lisp S-expression
    /// specifying query options.
    ///
    /// When the string starts with `(list`, it is parsed as a Lisp
    /// S-expression via `hkask_lisp::eval_sandboxed_with_budget`.
    /// The result is an association list (alist) of key-value pairs:
    ///
    /// ```lisp
    /// (list (list "query" "investment philosophy")
    ///       (list "top-k" 5)
    ///       (list "include-text" t)
    ///       (list "min-score" 0.6))
    /// ```
    ///
    /// Supported keys:
    /// - `"query"` (string, required) — the natural language query
    /// - `"top-k"` (int, default 5) — number of top results to return
    /// - `"include-text"` (bool, default false) — include passage text in results
    /// - `"min-score"` (float, default 0.0) — only return matches with score >= this
    /// - `"generate-answer"` (bool, default false) — generate LLM answer from results
    ///
    /// When the string does NOT start with `(list`, it is treated as a
    /// plain natural-language query (backward compatible with the original
    /// `query` + `top_k` + `generate_answer` parameters).
    pub query: String,
    /// Number of top results to return (default 5). Ignored when `query`
    /// is a Lisp S-expression (use `"top-k"` in the expression instead).
    #[serde(default)]
    pub top_k: Option<usize>,
    /// If true, generate an LLM-augmented answer from retrieved passages.
    /// Ignored when `query` is a Lisp S-expression.
    #[serde(default)]
    pub generate_answer: Option<bool>,
    /// Include passage text in returned results (default false). Answer generation
    /// always retains usable text internally. For Lisp use `"include-text"`.
    #[serde(default)]
    pub include_text: Option<bool>,
    /// Minimum score threshold (0.0–1.0). Only return matches with score
    /// >= this value. For Lisp use `"min-score"`.
    #[serde(default)]
    pub min_score: Option<f32>,
    /// Path to the memory DB for vector search when the in-memory index is empty
    /// (e.g. after server restart). Ignored on a nonempty index; it does not switch DBs.
    #[serde(default)]
    pub db_path: Option<String>,
    /// Passphrase for the memory DB. Defaults to `HKASK_DB_PASSPHRASE`.
    #[serde(default)]
    pub passphrase: Option<String>,
}

/// Parse a Lisp S-expression query string into query options.
///
/// The input must start with `(list` and evaluate to an association list
/// (alist) — a JSON array of 2-element arrays. Each pair is `(key, value)`.
///
/// Supported keys:
/// - `"query"` (string, required) — the natural language query
/// - `"top-k"` (int, default 5) — number of top results to return
/// - `"include-text"` (bool, default false) — include passage text in results
/// - `"min-score"` (float, default 0.0) — only return matches with score >= this
/// - `"generate-answer"` (bool, default false) — generate LLM answer from results
///
/// Returns `(query, top_k, include_text, min_score, generate_answer)`.
fn parse_lisp_query(expr: &str) -> Result<(String, usize, bool, f32, bool), McpToolError> {
    let result =
        hkask_lisp::eval_sandboxed_with_budget(expr, &serde_json::Value::Null, 100_000, 100)
            .map_err(|e| {
                McpToolError::invalid_argument(format!("Invalid Lisp query expression: {e}"))
            })?;

    // The result can be either a JSON object (when `list` of pairs is
    // evaluated — the interpreter converts 2-element lists to key-value pairs)
    // or a JSON array of pairs. Handle both.
    let alist: Vec<serde_json::Value> = if let Some(obj) = result.as_object() {
        obj.iter().map(|(k, v)| json!([k, v])).collect()
    } else if let Some(arr) = result.as_array() {
        arr.to_vec()
    } else {
        return Err(McpToolError::invalid_argument(
            "Lisp query expression must evaluate to an association list (array of pairs) or object",
        ));
    };

    let mut nl_query = String::new();
    let mut top_k = 5usize;
    let mut include_text = false;
    let mut min_score = 0.0f32;
    let mut generate_answer = false;

    for pair in &alist {
        let arr = pair.as_array().ok_or_else(|| {
            McpToolError::invalid_argument(
                "Each element in the Lisp query alist must be a 2-element array",
            )
        })?;
        if arr.len() != 2 {
            return Err(McpToolError::invalid_argument(format!(
                "Each element in the Lisp query alist must be a 2-element array, got {} elements",
                arr.len()
            )));
        }
        let key = arr[0].as_str().ok_or_else(|| {
            McpToolError::invalid_argument(
                "First element of each query alist pair must be a string key",
            )
        })?;
        let value = &arr[1];
        match key {
            "query" => {
                nl_query = value
                    .as_str()
                    .ok_or_else(|| {
                        McpToolError::invalid_argument("\"query\" value must be a string")
                    })?
                    .to_string();
            }
            "top-k" => {
                top_k = value.as_u64().ok_or_else(|| {
                    McpToolError::invalid_argument("\"top-k\" value must be an integer")
                })? as usize;
            }
            "include-text" => {
                include_text = value.as_bool().ok_or_else(|| {
                    McpToolError::invalid_argument(
                        "\"include-text\" value must be a boolean (t or nil)",
                    )
                })?;
            }
            "min-score" => {
                min_score = value.as_f64().ok_or_else(|| {
                    McpToolError::invalid_argument("\"min-score\" value must be a number")
                })? as f32;
            }
            "generate-answer" => {
                generate_answer = value.as_bool().ok_or_else(|| {
                    McpToolError::invalid_argument(
                        "\"generate-answer\" value must be a boolean (t or nil)",
                    )
                })?;
            }
            _ => {
                return Err(McpToolError::invalid_argument(format!(
                    "Unknown query option key: '{key}'. Supported keys: \
                     query, top-k, include-text, min-score, generate-answer"
                )));
            }
        }
    }

    if nl_query.is_empty() {
        return Err(McpToolError::invalid_argument(
            "Lisp query expression must include a \"query\" key with a string value",
        ));
    }

    Ok((nl_query, top_k, include_text, min_score, generate_answer))
}

impl RetrievedPassage {
    fn project(&self, include_text: bool) -> serde_json::Value {
        let mut result = json!({"metadata": self.passage.metadata, "score": self.score});
        if self.passage.text.is_none() { result["text_available"] = json!(false); }
        if include_text { result["text"] = json!(self.passage.text); }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::IndexedPassage;

    fn make_passage(text: &str, embedding: Vec<f32>) -> IndexedPassage {
        IndexedPassage {
            text: Some(text.to_string()),
            metadata: json!({"entity_ref": "test:chunk:1"}),
            embedding,
        }
    }

    fn search_passages(passages: &[IndexedPassage], query: &[f32], k: usize, min_score: f32, include_text: bool) -> Vec<serde_json::Value> {
        crate::index::search_passages(passages.iter(), query, k, min_score).iter().map(|matched| matched.project(include_text)).collect()
    }

    // ── search_passages ──────────────────────────────────────────────────

    #[test]
    fn search_returns_results_sorted_by_score() {
        let passages = vec![
            make_passage("low match", vec![0.0, 1.0]), // orthogonal → 0.0
            make_passage("high match", vec![1.0, 0.0]), // parallel → 1.0
            make_passage("medium match", vec![1.0, 1.0]), // 45° → ~0.707
        ];
        let query = vec![1.0, 0.0];
        let results = search_passages(&passages, &query, 3, 0.0, false);
        assert_eq!(results.len(), 3);
        // Highest score first
        assert!(results[0]["score"].as_f64().unwrap() > results[1]["score"].as_f64().unwrap());
        assert!(results[1]["score"].as_f64().unwrap() > results[2]["score"].as_f64().unwrap());
    }

    #[test]
    fn search_top_k_truncates() {
        let passages = vec![
            make_passage("a", vec![0.9, 0.0]),
            make_passage("b", vec![0.8, 0.0]),
            make_passage("c", vec![0.7, 0.0]),
        ];
        let query = vec![1.0, 0.0];
        let results = search_passages(&passages, &query, 2, 0.0, false);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_min_score_filters() {
        let passages = vec![
            make_passage("orthogonal", vec![0.0, 1.0]), // score 0.0
            make_passage("parallel", vec![1.0, 0.0]),   // score 1.0
        ];
        let query = vec![1.0, 0.0];
        let results = search_passages(&passages, &query, 10, 0.5, false);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["metadata"]["entity_ref"], "test:chunk:1");
    }

    #[test]
    fn search_include_text_adds_text_field() {
        let passages = vec![make_passage("secret text", vec![1.0, 0.0])];
        let query = vec![1.0, 0.0];
        let with_text = search_passages(&passages, &query, 1, 0.0, true);
        assert!(with_text[0].get("text").is_some());
        assert_eq!(with_text[0]["text"], "secret text");

        let without_text = search_passages(&passages, &query, 1, 0.0, false);
        assert!(without_text[0].get("text").is_none());
    }

    #[test]
    fn search_empty_passages_returns_empty() {
        let results = search_passages(&[], &[1.0, 0.0], 5, 0.0, false);
        assert!(results.is_empty());
    }

    // ── parse_lisp_query ────────────────────────────────────────────────

    #[test]
    fn parse_lisp_query_basic() {
        let expr = r#"(list (list "query" "investment philosophy") (list "top-k" 3) (list "include-text" t))"#;
        let (query, k, include_text, min_score, gen_answer) =
            parse_lisp_query(expr).expect("should parse");
        assert_eq!(query, "investment philosophy");
        assert_eq!(k, 3);
        assert!(include_text);
        assert_eq!(min_score, 0.0);
        assert!(!gen_answer);
    }

    #[test]
    fn parse_lisp_query_with_min_score() {
        let expr = r#"(list (list "query" "test") (list "min-score" 0.7) (list "top-k" 10))"#;
        let (_, k, _, min_score, _) = parse_lisp_query(expr).expect("should parse");
        assert_eq!(k, 10);
        assert!((min_score - 0.7).abs() < 0.001);
    }

    #[test]
    fn parse_lisp_query_missing_query_returns_error() {
        let expr = r#"(list (list "top-k" 5))"#;
        assert!(parse_lisp_query(expr).is_err());
    }

    #[test]
    fn parse_lisp_query_unknown_key_returns_error() {
        let expr = r#"(list (list "query" "test") (list "unknown-key" 42))"#;
        assert!(parse_lisp_query(expr).is_err());
    }

    #[test]
    fn parse_lisp_query_generate_answer() {
        let expr = r#"(list (list "query" "test") (list "generate-answer" t))"#;
        let (_, _, _, _, gen_answer) = parse_lisp_query(expr).expect("should parse");
        assert!(gen_answer);
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ClearIndexRequest {}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PurgeQaRequest {
    /// Entity-ref prefix to purge (e.g. "corpus:researcher:").
    #[serde(default = "default_purge_prefix")]
    pub prefix: String,
    /// Path to the SQLCipher memory DB.
    pub db_path: String,
    /// Passphrase for the memory DB.
    #[serde(default = "crate::helpers::default_corpus_passphrase")]
    pub passphrase: String,
}

fn default_purge_prefix() -> String {
    "corpus:researcher:".to_string()
}
