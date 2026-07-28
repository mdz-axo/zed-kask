//! Data Services sub-page — API key entry for EODHD, FMP, Exa, Tavily, Brave,
//! RunPod, Nebius, HuggingFace, etc. Keys live in the OS keychain; enable
//! toggles live in settings.json.

use super::*;

pub(crate) fn render_data_services_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let provider = zed_credentials::global(cx);
    let raw = raw_kask_settings(cx);
    let data_services = raw.and_then(|c| c.data_services).unwrap_or_default();

    let mut rows: Vec<AnyElement> = Vec::new();
    for (key, label, dashboard_url, env_var) in DATA_SERVICES {
        let enabled = match *key {
            "eodhd" => data_services.eodhd_enabled.unwrap_or(false),
            "fmp" => data_services.fmp_enabled.unwrap_or(false),
            "exa" => data_services.exa_enabled.unwrap_or(false),
            "tavily" => data_services.tavily_enabled.unwrap_or(false),
            "brave" => data_services.brave_enabled.unwrap_or(false),
            "runpod" => data_services.runpod_enabled.unwrap_or(false),
            "runpod_s3_access_key" | "runpod_s3_secret" => {
                data_services.runpod_enabled.unwrap_or(false)
            }
            "nebius_project_id" | "nebius_subnet_id" => {
                data_services.nebius_enabled.unwrap_or(false)
            }
            // These services don't have individual toggles — they're enabled
            // when their API key is present. We show them as enabled if the
            // key is in the keychain (checked via env var for display).
            "serpapi" | "firecrawl" | "browserbase" | "hf_token" => std::env::var(env_var).is_ok(),
            _ => false,
        };
        rows.push(render_data_service_row(
            key,
            label,
            dashboard_url,
            env_var,
            enabled,
            provider.clone(),
            cx,
        ));
    }

    v_flex()
        .id("kask-data-services-page")
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
                .child(SettingsSectionHeader::new("Data Services"))
                .child(
                    Label::new(
                        "API keys are stored in the system keychain (kask://credentials/<key>). \
                         Toggle a service to enable it, then enter its API key.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(v_flex().gap_6().children(rows))
        .into_any_element()
}

fn render_data_service_row(
    key: &'static str,
    label: &'static str,
    dashboard_url: &'static str,
    env_var: &'static str,
    enabled: bool,
    provider: Arc<dyn CredentialsProvider>,
    _cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let credential_url = format!("{KASK_CREDENTIAL_NAMESPACE}/{key}");
    let has_key = has_credential(&provider, &credential_url, env_var);

    let toggle_id = format!("kask-{key}-enabled");
    let enable_toggle = SwitchField::new(
        toggle_id,
        Some(label),
        Some(
            format!(
                "Enable {label}. API key is stored in the keychain under \
             kask://credentials/{key}. Or set the {env_var} environment variable."
            )
            .into(),
        ),
        if enabled {
            ToggleState::Selected
        } else {
            ToggleState::Unselected
        },
        move |state, _window, cx| {
            let enabled = *state == ToggleState::Selected;
            set_data_service_enabled(key, enabled, cx);
        },
    )
    .tab_index(0);

    let key_input = if has_key {
        let reset_id = format!("kask-{key}-reset");
        ConfiguredApiCard::new(reset_id, "API Key Configured")
            .button_label("Reset Key")
            .button_tab_index(0)
            .on_click({
                let provider = provider.clone();
                move |_, _, cx| {
                    delete_credential(&provider, &credential_url, cx).detach();
                }
            })
            .into_any_element()
    } else {
        let input_id = format!("kask-{key}-api-key-input");
        let aria_label = format!("{label} API Key");
        let provider = provider.clone();
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .pt_2p5()
                    .w_full()
                    .min_w_0()
                    .gap_4()
                    .justify_between()
                    .child(
                        v_flex()
                            .w_full()
                            .min_w_0()
                            .max_w_1_2()
                            .gap_0p5()
                            .child(Label::new("API Key"))
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .flex_wrap()
                                    .gap_0p5()
                                    .child(
                                        Label::new("Visit the")
                                            .size(LabelSize::Small)
                                            .color(Color::Muted),
                                    )
                                    .child(
                                        ButtonLink::new(
                                            format!("{label} dashboard"),
                                            dashboard_url,
                                        )
                                        .no_icon(true)
                                        .label_size(LabelSize::Small)
                                        .label_color(Color::Muted),
                                    )
                                    .child(
                                        Label::new("to generate an API key.")
                                            .size(LabelSize::Small)
                                            .color(Color::Muted),
                                    ),
                            )
                            .child(
                                Label::new(format!(
                                    "Or set the {env_var} env var and restart Zed for it to take effect."
                                ))
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                            ),
                    )
                    .child(
                        SettingsInputField::new(input_id)
                            .tab_index(0)
                            .with_placeholder("xxxxxxxxxxxxxxxxxxxx")
                            .aria_label(aria_label)
                            .on_confirm(move |api_key, _window, cx| {
                                if let Some(key_value) =
                                    api_key.filter(|key_value| !key_value.is_empty())
                                {
                                    write_credential(
                                        &provider,
                                        &credential_url,
                                        &key_value,
                                        cx,
                                    )
                                    .detach();
                                }
                            }),
                    ),
            )
            .into_any_element()
    };

    v_flex()
        .gap_2()
        .child(enable_toggle)
        .when(enabled, |this| this.child(key_input))
        .into_any_element()
}

fn set_data_service_enabled(key: &str, enabled: bool, cx: &mut App) {
    let key = key.to_string();
    SettingsStore::global(cx).update_settings_file(<dyn fs::Fs>::global(cx), move |settings, _| {
        let kask = settings.kask.get_or_insert_default();
        let data_services = kask.data_services.get_or_insert_default();
        match key.as_str() {
            "eodhd" => data_services.eodhd_enabled = Some(enabled),
            "fmp" => data_services.fmp_enabled = Some(enabled),
            "exa" => data_services.exa_enabled = Some(enabled),
            "tavily" => data_services.tavily_enabled = Some(enabled),
            "brave" => data_services.brave_enabled = Some(enabled),
            "runpod" => data_services.runpod_enabled = Some(enabled),
            "runpod_s3_access_key" | "runpod_s3_secret" => {
                data_services.runpod_enabled = Some(enabled);
            }
            "nebius_project_id" | "nebius_subnet_id" => {
                data_services.nebius_enabled = Some(enabled);
            }
            // These services don't have individual toggles — no-op.
            _ => {}
        }
    });
}
