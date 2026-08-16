use hkask_templates::load_manifest_from_yaml;
use std::path::Path;

/// Regression test: every YAML file with a `manifest:` key in
/// `registry/manifests/` must load successfully via `load_manifest_from_yaml`.
///
/// This catches:
/// - Unknown top-level fields (deny_unknown_fields on ManifestFile)
/// - Missing required step fields (e.g. `description`)
/// - Type mismatches (e.g. rjoule.cap as float instead of u32)
/// - Invalid manifest header fields
///
/// Files without a `manifest:` key (e.g. training recipes in
/// `manifests/training/`) are skipped — they are not process manifests and
/// are not embedded by build.rs (which uses non-recursive read_dir).
#[test]
fn all_manifests_load_successfully() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/manifests");
    if !dir.exists() {
        eprintln!("{} not found — skipping test", dir.display());
        return;
    }

    let mut errors = Vec::new();
    let mut ok = 0;

    for entry in walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "yaml") {
            let yaml = std::fs::read_to_string(path).unwrap();
            // Skip non-manifest YAML files (e.g. training recipes in
            // manifests/training/ that don't have a `manifest:` key).
            // build.rs only embeds top-level manifests/*.yaml (non-recursive),
            // so training subdirectory files are not embedded at build time.
            if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
                continue;
            }
            match load_manifest_from_yaml(&yaml) {
                Ok(_m) => {
                    ok += 1;
                }
                Err(e) => {
                    errors.push(format!(
                        "{}: {}",
                        path.file_name().unwrap().to_string_lossy(),
                        e
                    ));
                }
            }
        }
    }

    eprintln!("OK: {}, ERR: {}", ok, errors.len());
    for e in &errors {
        eprintln!("  ERR: {}", e);
    }
    assert!(
        errors.is_empty(),
        "{} manifests failed to load:\n{}",
        errors.len(),
        errors.join("\n")
    );
}

/// Known MCP tool names registered across all built-in MCP servers.
/// This is a static snapshot for validation purposes — at runtime, the
/// tool set is dynamic (servers register at startup). This list is
/// maintained manually and should be updated when new MCP tools are added.
///
/// To regenerate: grep for `execute_tool_semantic.*self.*"` across all
/// `kask/mcp-servers/hkask-mcp-*/src/` files and collect the tool names.
const KNOWN_MCP_TOOLS: &[&str] = &[
    // research server
    "web_search",
    "web_find_similar",
    "web_extract",
    "web_browse",
    "web_ping",
    "rss_subscribe",
    "rss_unsubscribe",
    "rss_list_subscriptions",
    "rss_fetch",
    "rss_get_entries",
    "rss_get_unread_count",
    "rss_mark_all_read",
    "rss_search",
    "rss_edit_tag",
    "rss_export_opml",
    "rss_import_opml",
    "rss_list_synthetic",
    "rss_synthesize",
    "rss_fetch_synthetic",
    "rss_delete_synthetic",
    "rss_discover_feeds",
    "evaluate_evidence",
    "cite_sources",
    // companies server
    "company_profile",
    "company_transcript",
    "company_screener",
    "stock_quote",
    "historical_price",
    "symbol_search",
    "income_statement",
    "balance_sheet",
    "cash_flow_statement",
    "key_metrics",
    "dcf_valuation",
    "reverse_dcf",
    "comparable_analysis",
    "expectations_gap",
    "scenario_analysis",
    "sensitivity_analysis",
    "monte_carlo_dcf",
    "ep_valuation",
    "equity_duration",
    "moat_check",
    "management_scorecard",
    "working_capital_cycle",
    "calibrate_forecast",
    "forecast_list",
    "forecast_get",
    "forecast_record",
    "forecast_persist",
    "research_search",
    "note_add",
    "note_list",
    "note_delete",
    "file_attach",
    "file_list",
    "file_delete",
    "ledger_apply",
    "ledger_read",
    "companies_ledger_export",
    "companies_ledger_import",
    "companies_portfolio_list",
    "companies_portfolio_delete",
    "companies_portfolio_returns",
    "portfolio_create",
    "portfolio_portfolio_list",
    "portfolio_portfolio_delete",
    "portfolio_portfolio_returns",
    "portfolio_snapshot",
    "portfolio_seed_price",
    "portfolio_materialize_returns",
    "portfolio_daily_returns",
    "portfolio_rebuild_views",
    "portfolio_attribution",
    "portfolio_characteristics",
    "portfolio_comparison",
    "portfolio_ledger_export",
    "portfolio_ledger_import",
    "portfolio_roll",
    "transaction_note_append",
    // scenarios server
    "scenario_status",
    "scenario_full",
    "scenario_from_markets",
    "scenario_from_markets_set",
    "scenario_from_cmp_indices",
    "scenario_cross_validate",
    "scenario_frame",
    "scenario_frame_document",
    "scenario_brainstorm",
    "scenario_build",
    "scenario_research",
    "scenario_quantify",
    "scenario_propagate",
    "scenario_score",
    "scenario_sensitivity",
    "scenario_synthesize",
    "scenario_triage",
    "scenario_calibrate",
    "scenario_calibration",
    "scenario_update",
    "scenario_assess",
    "scenario_from_cmp_indices",
    "scenario_impact_valuation",
    // prediction-markets server
    "market_lookup",
    "market_match",
    "market_history",
    "market_ladder",
    "market_volatility",
    "market_residual",
    "market_cmp",
    "market_cmp_index",
    "market_cmp_index_store",
    "market_cmp_portfolio_store",
    "market_cmp_context_suggest",
    "market_calibration",
    "market_check_resolutions",
    "market_record_resolution",
    "market_subscribe_resolutions",
    "market_score_rationale",
    "market_ontology_map",
    "prediction_markets_status",
    // codegraph server
    "codegraph_query",
    "codegraph_traverse",
    "codegraph_context",
    "codegraph_analysis",
    "codegraph_impact",
    "codegraph_reindex",
    "codegraph_stats",
    "codegraph_structure",
    "codegraph_index_embeddings",
    // portfolio server (tools exposed via portfolio MCP, distinct from companies)
    // condenser server
    "condenser_persist",
    "condenser_ping",
    "condenser_score_saliency",
    "condenser_thread_summary",
    // corpus server
    "corpus_cache",
    "corpus_cache_work",
    "corpus_chunk",
    "corpus_clear_index",
    "corpus_compare",
    "corpus_compose",
    "corpus_consolidate_chunks",
    "corpus_convert",
    "corpus_dedup_chunks",
    "corpus_discover",
    "corpus_discover_company",
    "corpus_embed",
    "corpus_explain",
    "corpus_extract_assertions",
    "corpus_generate_qa",
    "corpus_generate_qa_batch",
    "corpus_ingest_qa",
    "corpus_is_complex",
    "corpus_mashup",
    "corpus_ocr",
    "corpus_purge_qa",
    "corpus_query",
    "corpus_registry",
    "corpus_rewrite",
    "corpus_tag_chunks",
    "corpus_build_persona",
    "corpus_build_prompts",
    // curator server
    "curator_algedonic_log",
    "curator_consult",
    "curator_escalation_dismiss",
    "curator_escalation_resolve",
    "curator_escalations",
    "curator_memory_recall",
    "curator_ping",
    "curator_semantic_search",
    // kata-kanban server
    "kanban_board_create",
    "kanban_board_delete",
    "kanban_board_list",
    "kanban_task_create",
    "kanban_task_delete",
    "kanban_task_list",
    "kanban_task_move",
    "kanban_task_assign",
    "kanban_task_unassign",
    "kanban_task_update",
    "kanban_task_comment",
    "kanban_task_comments_since",
    "kanban_task_verify",
    "kanban_task_add_deliverable",
    "kanban_task_add_gas",
    "kanban_task_add_rjoules",
    "kanban_task_spawn",
    "kanban_task_delegate_result",
    "kanban_task_reopen",
    "kanban_task_kata_coaching",
    "kanban_task_kata_improvement",
    "kanban_task_kata_practice",
    "contract_propose_expect",
    // media server
    "generate_image",
    "transform_image",
    "upscale_image",
    "describe_image",
    "expand_prompt",
    "generate_video",
    "generate_speech",
    "voice_design",
    "gallery_organize",
    "gallery_search",
    "gallery_status",
    "gallery_refresh",
    "gallery_analyze",
    "gallery_find_similar",
    "gallery_timeline",
    "gallery_lineage",
    "gallery_record_generation",
    "gallery_reproduce",
    "gallery_name_face",
    "image_apply_style",
    "image_create_collage",
    "image_remove_background",
    "image_to_video",
    "video_add_caption",
    "video_caption",
    "video_clip",
    "video_concat",
    "video_extract_frames",
    "video_from_images",
    "video_meme",
    "video_remix",
    "video_to_gif",
    "face_list",
    "face_register",
    "face_remove",
    "face_scan_folder",
    "face_validate",
    "audio_capture",
    "transcribe",
    "transcribe_bundle",
    "record_and_transcribe",
    // swarm server
    "swarm_list_agents",
    "swarm_get_agent",
    "swarm_create_agent",
    "swarm_delete_agent",
    "swarm_fork_agent",
    "swarm_publish_agent",
    "swarm_publish_checks",
    "swarm_execute_agent",
    "swarm_create_swarm",
    "swarm_delete_swarm",
    "swarm_get_swarm",
    "swarm_hire",
    "swarm_fire",
    "swarm_delegate",
    "swarm_delegate_and_wait",
    "swarm_fanout",
    "swarm_run_status",
    "swarm_request_consent",
    "swarm_hire_cost",
    "swarm_search_knowledge",
    "swarm_generate_ontology",
    "swarm_generate_prompt",
    "swarm_ontology_templates",
    "swarm_xaman",
    "swarm_create_app",
    "swarm_list_apps",
    "swarm_push_to_cloud",
    "swarm_clone_to_local",
    "swarm_create_local_agent",
    "swarm_reconfigure_local_agent",
    "swarm_remove_local",
    "swarm_list_local_agents",
    "swarm_create_local_swarm",
    "swarm_delete_local_swarm",
    "swarm_get_local_swarm",
    "swarm_list_local_swarms",
    "swarm_add_agent_local",
    "swarm_remove_agent_local",
    "swarm_delegate_local",
    "swarm_fanout_local",
    "swarm_pipeline_local",
    "swarm_execute_plan_local",
    "swarm_evaluate_local",
    "swarm_a2a_card",
    "swarm_a2a_send",
    "swarm_authorize_session",
    "swarm_balance_local",
    "swarm_fund_local",
    "swarm_local_history",
    "swarm_ai_assist",
    // training server
    "training_assemble_dataset",
    "training_cancel",
    "training_evaluate",
    "training_ingest_dataset",
    "training_ingest_qa",
    "training_status",
    "training_submit",
    "training_validate_config",
    // dbnomics (via research server? or separate — listed in tool list)
    "dbnomics_list_providers",
    "dbnomics_search",
    "dbnomics_get_dataset",
    "dbnomics_get_series",
    // fred (via research server)
    "fred_search_series",
    "fred_get_series_info",
    "fred_get_observations",
    "fred_get_release",
    "fred_list_categories",
    // world bank (via research server)
    "wb_list_countries",
    "wb_list_topics",
    "wb_search_indicators",
    "wb_get_indicator_info",
    "wb_get_observations",
    // regulation
    "reg_query",
];

/// Validate that every `mcp:` reference in every manifest points to a tool
/// that exists in the known MCP tool set. This catches manifest-vs-registry
/// drift (e.g. `mcp: fetch` when no `fetch` tool is registered) at test time,
/// preventing runtime failures that are invisible at manifest-load time.
#[test]
fn all_mcp_references_point_to_known_tools() {
    use hkask_templates::{load_manifest_from_yaml, validate_mcp_references};
    use std::collections::HashSet;

    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/manifests");
    if !dir.exists() {
        eprintln!("{} not found — skipping test", dir.display());
        return;
    }

    let known: HashSet<&str> = KNOWN_MCP_TOOLS.iter().copied().collect();
    let mut warnings_total = Vec::new();

    for entry in walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "yaml") {
            let yaml = std::fs::read_to_string(path).unwrap();
            if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
                continue;
            }
            match load_manifest_from_yaml(&yaml) {
                Ok(manifest) => {
                    let warnings = validate_mcp_references(&manifest, &known);
                    for w in &warnings {
                        eprintln!("WARN: {}", w);
                    }
                    warnings_total.extend(warnings);
                }
                Err(_) => continue,
            }
        }
    }

    assert!(
        warnings_total.is_empty(),
        "{} manifest(s) reference MCP tools not in the known set:\n{}",
        warnings_total.len(),
        warnings_total
            .iter()
            .map(|w| format!("  - {}", w))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
