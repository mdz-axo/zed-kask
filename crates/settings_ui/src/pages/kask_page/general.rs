//! General sub-page — kask-wide data and artifacts directory configuration.

use super::*;

pub(crate) fn render_general_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let (data_dir, artifacts_dir, general): (String, String, kask_bridge::KaskGeneralSettings) =
        raw.map(|c| {
            (
                c.data_dir.unwrap_or_default(),
                c.artifacts_dir.unwrap_or_default(),
                c.general.map(Into::into).unwrap_or_default(),
            )
        })
        .unwrap_or_default();

    let resolved_default = kask_bridge::resolve_data_dir()
        .to_string_lossy()
        .to_string();
    let resolved_artifacts_default = kask_bridge::resolve_artifacts_dir()
        .to_string_lossy()
        .to_string();

    let data_dir_input = kask_string_input(
        "kask-general-data-dir",
        "Kask Data Directory (Databases)",
        format!("Default: {resolved_default}"),
        data_dir,
        "kask",
        "data_dir",
    );
    let artifacts_dir_input = kask_string_input(
        "kask-general-artifacts-dir",
        "Kask Artifacts Directory",
        format!("Default: {resolved_artifacts_default}"),
        artifacts_dir,
        "kask",
        "artifacts_dir",
    );
    let max_concurrency_input = kask_string_input(
        "kask-general-max-concurrency",
        "Max Concurrency",
        "Default: 96",
        general.max_concurrency.to_string(),
        "general",
        "max_concurrency",
    );

    v_flex()
        .id("kask-general-page")
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
                .child(SettingsSectionHeader::new("General"))
                .child(
                    Label::new(
                        "Zed-kask stores persistent state in two rooted trees. \
                         The data directory (hidden) holds ONLY infrastructure — \
                         databases and machine state (agents/, mcp/, skills/, \
                         threads/). The artifacts directory (visible, default \
                         ~/Documents/zk-data/) holds EVERY artifact file and \
                         output the MCP servers produce for you — reports, \
                         screens, transaction files, generated media, corpus \
                         cache — at {server}-mcp/{artifact-type}/. Every MCP \
                         server receives both roots (HKASK_DATA_DIR / \
                         HKASK_ARTIFACTS_DIR) so paths resolve consistently.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Kask Data Directory (Databases)"))
                .child(
                    Label::new(
                        "Hidden root for all kask databases and agent state \
                         (infrastructure only). Leave empty to use the platform \
                         default (~/.local/share/zed-kask on Linux).",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(data_dir_input),
        )
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Kask Artifacts Directory"))
                .child(
                    Label::new(
                        "Visible root for ALL artifact files and outputs of the \
                         MCP servers ({server}-mcp/{artifact-type}/). Leave \
                         empty to use the platform default (~/Documents/zk-data/).",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(artifacts_dir_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Concurrency"))
                .child(
                    Label::new(
                        "Global inference concurrency — the process-wide ceiling on \
                         concurrent cloud inference provider calls. Applies to skill \
                         cascades, corpus OCR, and MCP tool calls. Providers \
                         throttle at different levels; OpenRouter scales \
                         to the ceiling. Changes require a restart to take effect.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(max_concurrency_input),
        )
        .into_any_element()
}
