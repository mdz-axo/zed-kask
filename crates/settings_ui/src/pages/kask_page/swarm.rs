//! Swarm sub-page — local swarm agents and local swarms: default agent
//! model, A2A HTTP gateway, and local semantic-memory configuration.
//!
//! The swarm server is local-only (the former ABW cloud backend is removed
//! — no API key, API URL, credit ceiling, or curator consent settings
//! remain). No per-server path fields — the swarm server's local agent
//! registry, local swarms registry, and memory DB are derived from the
//! global `data_dir` as `mcp/swarm/agents/curated/`, `mcp/swarm/swarms/`,
//! and `mcp/swarm/memory.db` by `mcp_env()`. The server reads them via
//! `HKASK_LOCAL_AGENTS_DIR`, `HKASK_LOCAL_SWARMS_DIR`, and
//! `HKASK_SWARM_MEMORY_DB`.

use super::*;

pub(crate) fn render_swarm_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let swarm: kask_bridge::KaskSwarmSettings = raw
        .and_then(|c| c.swarm)
        .map(Into::into)
        .unwrap_or_default();
    let default_agent_model = swarm.default_agent_model;
    let a2a_http_enabled = swarm.a2a_http_enabled;
    let embedding_dim = swarm.embedding_dim.to_string();

    let default_agent_model_input = kask_input_field(
        "kask-swarm-default-agent-model",
        1,
        default_agent_model,
        "claude-haiku-4-5-20251001",
        "Default Agent Model",
        |text| Some(text.trim().to_string()),
        move |parsed, settings| {
            settings
                .kask
                .get_or_insert_default()
                .swarm
                .get_or_insert_default()
                .default_agent_model = if parsed.is_empty() {
                None
            } else {
                Some(parsed)
            };
        },
    );

    let a2a_http_toggle = kask_toggle(
        "kask-swarm-a2a-http-enabled",
        Some("A2A HTTP Gateway"),
        Some(
            "Enable the A2A HTTP gateway (loopback JSON-RPC server that exposes local \
             agents to external A2A clients). Opens a loopback port — only enable when \
             you need external A2A clients to reach your local agents. Or set \
             HKASK_A2A_HTTP_ENABLE=1."
                .into(),
        ),
        a2a_http_enabled,
        2,
        move |enabled, settings| {
            settings
                .kask
                .get_or_insert_default()
                .swarm
                .get_or_insert_default()
                .a2a_http_enabled = Some(enabled);
        },
    );

    let embedding_dim_input = kask_input_field(
        "kask-swarm-embedding-dim",
        3,
        embedding_dim,
        "1024",
        "Embedding Dimension",
        |text| text.trim().parse::<usize>().ok(),
        move |parsed, settings| {
            settings
                .kask
                .get_or_insert_default()
                .swarm
                .get_or_insert_default()
                .embedding_dim = Some(parsed);
        },
    );

    v_flex()
        .id("kask-swarm-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Swarm"))
                .child(
                    Label::new(
                        "Local swarm agents and local swarms. Local agent cards, local \
                         swarms, and the semantic-memory DB persist to mcp/swarm/ under \
                         the shared kask data directory (set on the General page). There \
                         are no per-server path settings — all MCP servers share the \
                         single data directory.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Default Agent Model"))
                .child(
                    Label::new(
                        "Default model for new local agents and for local agents without \
                         an explicit model. Leave empty for the host session model. \
                         Explicit agent models take precedence. Env: \
                         HKASK_SWARM_DEFAULT_AGENT_MODEL.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(default_agent_model_input),
        )
        .child(Divider::horizontal())
        .child(a2a_http_toggle)
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Embedding Dimension"))
                .child(
                    Label::new(
                        "Embedding vector dimension for the semantic-memory embedding \
                         store. Default 1024. Or set HKASK_SWARM_EMBEDDING_DIM.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(embedding_dim_input),
        )
        .into_any_element()
}
