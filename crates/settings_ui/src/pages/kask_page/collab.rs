//! Collab Server sub-page — local collab server launch configuration.
//!
//! When enabled, zed-kask launches a local `collab serve api` process at
//! startup so the kask extensions panel can fetch `/api/kask-skills` without
//! depending on the deployed `zed.dev` server having the kask route. The
//! server uses SQLite (no Postgres/S3 needed) for local dev.

use super::*;

pub(crate) fn render_collab_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let collab: kask_bridge::KaskCollabSettings = raw
        .and_then(|c| c.collab)
        .map(Into::into)
        .unwrap_or_default();
    let enabled = collab.enabled;
    let database_url = collab.database_url;
    let http_port = collab.http_port.to_string();
    let zed_environment = collab.zed_environment;
    let marketplace_url = collab.marketplace_url;

    let database_url_input = kask_string_input(
        "kask-collab-database-url",
        "Database URL",
        "sqlite:kask_marketplace.db?mode=rwc",
        database_url,
        "collab",
        "database_url",
    );

    let http_port_input = SettingsInputField::new("kask-collab-http-port")
        .tab_index(0)
        .with_initial_text(http_port)
        .with_placeholder("3000")
        .aria_label("HTTP Port")
        .confirm_on_focus_out()
        .on_confirm(move |value, _window, cx| {
            if let Some(text) = value {
                if let Ok(parsed) = text.trim().parse::<u16>() {
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            settings
                                .kask
                                .get_or_insert_default()
                                .collab
                                .get_or_insert_default()
                                .http_port = Some(parsed);
                        },
                    );
                }
            }
        });

    let zed_environment_input = kask_string_input(
        "kask-collab-zed-environment",
        "Zed Environment",
        "development",
        zed_environment,
        "collab",
        "zed_environment",
    );

    let marketplace_url_input = kask_string_input(
        "kask-collab-marketplace-url",
        "Marketplace URL",
        "http://localhost:3000",
        marketplace_url,
        "collab",
        "marketplace_url",
    );

    v_flex()
        .id("kask-collab-page")
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
                .child(SettingsSectionHeader::new("Collab Server"))
                .child(
                    Label::new(
                        "Configure the local kask marketplace server. When enabled, \
                         zed-kask launches a local collab server at startup so the \
                         kask extensions panel can fetch skills without depending on \
                         the deployed zed.dev server. Uses SQLite — no Postgres or \
                         S3 required for browsing.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            SwitchField::new(
                "kask-collab-enabled",
                Some("Auto-Launch Collab Server"),
                Some(
                    "Whether to launch a local collab server at startup so the \
                     kask extensions panel can fetch skills."
                        .into(),
                ),
                enabled,
                move |state, _window, cx| {
                    let value = *state == ToggleState::Selected;
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            settings
                                .kask
                                .get_or_insert_default()
                                .collab
                                .get_or_insert_default()
                                .enabled = Some(value);
                        },
                    );
                },
            )
            .tab_index(0),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Database URL"))
                .child(
                    Label::new(
                        "SQLite connection string for the local collab server \
                         (e.g. sqlite:kask_marketplace.db?mode=rwc).",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(database_url_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("HTTP Port"))
                .child(
                    Label::new("Port the local collab server listens on.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(http_port_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Zed Environment"))
                .child(
                    Label::new(
                        "Zed environment label (development, staging, production). \
                         Use development for local dev.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(zed_environment_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Marketplace URL"))
                .child(
                    Label::new(
                        "Base URL the kask extensions panel uses for marketplace \
                         requests. When set, overrides the server_url-based \
                         resolution. Defaults to http://localhost:3000.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(marketplace_url_input),
        )
        .into_any_element()
}
