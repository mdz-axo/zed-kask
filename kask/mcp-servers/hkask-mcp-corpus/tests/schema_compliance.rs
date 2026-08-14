//! Schema-compliance tests for hkask-mcp-corpus tool request structs.
//!
//! Layer-1 deterministic schema scan enforcing the `.rules` trap "kask MCP
//! tool inputs that accept arbitrary JSON use `AnyJsonValue`": `schemars`
//! renders `serde_json::Value` as the bare boolean `true` in schema-valued
//! positions, which strict-schema-decoding providers (Ollama, Gemini) reject
//! (`400 cannot unmarshal bool into api.ToolProperty`). One bare boolean in
//! any enabled tool's schema fails the whole chat-completion request.
//!
//! Layer 1 only — the `schema_clean_test!` macro asserts no request struct's
//! JSON schema has a bare-boolean schema-valued position. Layer 2 (a
//! `proptest!` deserialization-totality property) is intentionally omitted: it
//! needs `proptest` + `hkask-test-harness` dev-deps to guard a different
//! invariant (P4 deserialization totality) that is out of scope here.
//!
//! Request structs live in the `tools` submodule tree (corpus, document,
//! gather, persona, semantic, storage, tagging) and are referenced by their
//! full crate-qualified paths. This avoids the `ComposeRequest`,
//! `DiscoverRequest`, `QueryRequest`, and `BuildPromptsRequest` name
//! collisions across submodules — only the variants used as `Parameters<T>`
//! at a `#[tool]` site are tested (the sibling definitions in `compose.rs`,
//! `corpus/discover/types.rs`, and `services/prompt_builder.rs` are internal
//! and never reach a provider's schema decoder).

use hkask_mcp_server::find_boolean_schema_positions;
use schemars::schema_for;

macro_rules! schema_clean_test {
    ($test_name:ident, $ty:ty) => {
        #[test]
        fn $test_name() {
            let schema = serde_json::to_value(&schema_for!($ty)).expect("schema serializes");
            let violations = find_boolean_schema_positions(&schema);
            assert!(
                violations.is_empty(),
                "{} schema has bare-boolean schema positions (Ollama/Gemini would reject): {violations:?}",
                stringify!($ty),
            );
        }
    };
}

// tools::corpus — dedup, consolidate, build prompts, ingest QA, prepare
// training dataset (output stage). `BuildPromptsRequest` here is the tool
// request in `tools/corpus/mod.rs`, not the internal one in
// `services/prompt_builder.rs`.
schema_clean_test!(
    dedup_chunks_request_schema,
    hkask_mcp_corpus::tools::corpus::DedupChunksRequest
);
schema_clean_test!(
    consolidate_chunks_request_schema,
    hkask_mcp_corpus::tools::corpus::ConsolidateChunksRequest
);
schema_clean_test!(
    corpus_build_prompts_request_schema,
    hkask_mcp_corpus::tools::corpus::BuildPromptsRequest
);
schema_clean_test!(
    corpus_ingest_qa_request_schema,
    hkask_mcp_corpus::tools::corpus::IngestQaRequest
);
schema_clean_test!(
    prepare_training_dataset_request_schema,
    hkask_mcp_corpus::tools::corpus::PrepareTrainingDatasetRequest
);

// tools::document — convert, OCR, is_complex, chunk (process stage).
schema_clean_test!(
    convert_request_schema,
    hkask_mcp_corpus::tools::document::ConvertRequest
);
schema_clean_test!(
    ocr_request_schema,
    hkask_mcp_corpus::tools::document::OcrRequest
);
schema_clean_test!(
    is_complex_request_schema,
    hkask_mcp_corpus::tools::document::IsComplexRequest
);
schema_clean_test!(
    chunk_request_schema,
    hkask_mcp_corpus::tools::document::ChunkRequest
);

// tools::gather — discover, cache_work. `DiscoverRequest` here is the tool
// request in `tools/gather/mod.rs`, not the internal one in
// `corpus/discover/types.rs`.
schema_clean_test!(
    discover_request_schema,
    hkask_mcp_corpus::tools::gather::DiscoverRequest
);
schema_clean_test!(
    cache_work_request_schema,
    hkask_mcp_corpus::tools::gather::CacheWorkRequest
);

// tools::persona — build, compose, rewrite, compare, mashup, registry.
// `ComposeRequest` here is the tool request in `tools/persona/mod.rs`, not the
// internal one in `compose.rs`.
schema_clean_test!(
    build_request_schema,
    hkask_mcp_corpus::tools::persona::BuildRequest
);
schema_clean_test!(
    compose_request_schema,
    hkask_mcp_corpus::tools::persona::ComposeRequest
);
schema_clean_test!(
    rewrite_request_schema,
    hkask_mcp_corpus::tools::persona::RewriteRequest
);
schema_clean_test!(
    compare_request_schema,
    hkask_mcp_corpus::tools::persona::CompareRequest
);
schema_clean_test!(
    mashup_request_schema,
    hkask_mcp_corpus::tools::persona::MashupRequest
);
schema_clean_test!(
    registry_request_schema,
    hkask_mcp_corpus::tools::persona::RegistryRequest
);

// tools::semantic — generate_qa, generate_qa_batch, extract_assertions, embed.
schema_clean_test!(
    generate_qa_request_schema,
    hkask_mcp_corpus::tools::semantic::GenerateQaRequest
);
schema_clean_test!(
    generate_qa_batch_request_schema,
    hkask_mcp_corpus::tools::semantic::GenerateQaBatchRequest
);
schema_clean_test!(
    extract_assertions_request_schema,
    hkask_mcp_corpus::tools::semantic::ExtractAssertionsRequest
);
schema_clean_test!(
    embed_request_schema,
    hkask_mcp_corpus::tools::semantic::EmbedRequest
);

// tools::storage — cache, query, clear_index, purge_qa. `QueryRequest` here
// is the corpus query tool request, distinct from any sibling crate's
// same-named struct.
schema_clean_test!(
    cache_request_schema,
    hkask_mcp_corpus::tools::storage::CacheRequest
);
schema_clean_test!(
    query_request_schema,
    hkask_mcp_corpus::tools::storage::QueryRequest
);
schema_clean_test!(
    clear_index_request_schema,
    hkask_mcp_corpus::tools::storage::ClearIndexRequest
);
schema_clean_test!(
    purge_qa_request_schema,
    hkask_mcp_corpus::tools::storage::PurgeQaRequest
);

// tools::tagging::ops — tag_chunks.
schema_clean_test!(
    tag_chunks_request_schema,
    hkask_mcp_corpus::tools::tagging::ops::TagChunksRequest
);
