//! MCP Servers sub-page — toggle which of the built-in kask MCP servers
//! are loaded, plus a master `load_default` toggle.

use super::*;

pub(crate) fn render_mcp_servers_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let mcp: kask_bridge::KaskMcpSettings =
        raw.and_then(|c| c.mcp).map(Into::into).unwrap_or_default();
    let load_default = mcp.load_default;
    let overrides = &mcp.overrides;

    let master_toggle = SwitchField::new(
        "kask-mcp-load-default",
        Some("Load Default MCP Servers"),
        Some(
            "When enabled, all built-in kask MCP servers are loaded unless \
             individually overridden below. Disable to load no kask MCP servers."
                .into(),
        ),
        if load_default {
            ToggleState::Selected
        } else {
            ToggleState::Unselected
        },
        move |state, _window, cx| {
            let enabled = *state == ToggleState::Selected;
            SettingsStore::global(cx).update_settings_file(
                <dyn fs::Fs>::global(cx),
                move |settings, _| {
                    settings
                        .kask
                        .get_or_insert_default()
                        .mcp
                        .get_or_insert_default()
                        .load_default = Some(enabled);
                },
            );
        },
    )
    .tab_index(0);

    let mut server_rows: Vec<AnyElement> = Vec::new();
    for (server_id, description) in builtin_mcp_servers() {
        let loaded = load_default && *overrides.get(server_id).unwrap_or(&true);
        server_rows.push(render_mcp_server_toggle(
            server_id,
            description,
            loaded,
            load_default,
        ));
    }

    v_flex()
        .id("kask-mcp-servers-page")
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
                .child(SettingsSectionHeader::new("MCP Servers"))
                .child(
                    Label::new(
                        "Toggle which of the built-in kask MCP servers are loaded. \
                         Individual overrides take precedence over the master toggle.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(master_toggle)
        .child(Divider::horizontal())
        .child(v_flex().gap_4().children(server_rows))
        .into_any_element()
}

fn render_mcp_server_toggle(
    server_id: &'static str,
    description: &'static str,
    loaded: bool,
    load_default: bool,
) -> AnyElement {
    let toggle_id = format!("kask-mcp-{server_id}");
    let server_id_for_write = server_id.to_string();
    SwitchField::new(
        toggle_id,
        Some(server_id),
        Some((*description).into()),
        if loaded {
            ToggleState::Selected
        } else {
            ToggleState::Unselected
        },
        move |state, _window, cx| {
            let enabled = *state == ToggleState::Selected;
            let server_id = server_id_for_write.clone();
            SettingsStore::global(cx).update_settings_file(
                <dyn fs::Fs>::global(cx),
                move |settings, _| {
                    settings
                        .kask
                        .get_or_insert_default()
                        .mcp
                        .get_or_insert_default()
                        .overrides
                        .insert(server_id, enabled);
                },
            );
        },
    )
    .when(!load_default, |this| {
        this.tooltip(ui::Tooltip::text(
            "The master \"Load Default MCP Servers\" toggle is off — \
             enable it for this override to take effect.",
        ))
    })
    .tab_index(0)
    .into_any_element()
}
