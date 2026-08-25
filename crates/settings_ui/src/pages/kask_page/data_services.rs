//! Data Services sub-page — API key entry for EODHD, FMP, Exa, Tavily, Brave,
//! RunPod, Nebius, HuggingFace, etc. Keys live in the OS keychain. The key's
//! presence is the toggle — there is no separate enable/disable bool.

use super::*;
use language_model::{LanguageModelProviderId, LanguageModelRegistry};

/// The RunPod language-model provider id (D29). Must match the provider's
/// `PROVIDER_ID` (`"runpod"`); [`LanguageModelRegistry::provider`] is an exact
/// id match and returns `None` on mismatch, which would silently turn the live
/// refresh below into a no-op.
const RUNPOD_PROVIDER_ID: LanguageModelProviderId = LanguageModelProviderId::new("runpod");

/// Ask the RunPod endpoint provider to adopt `key` through its existing
/// `set_api_key` path. That writes the keychain entry at its `api_url`, updates
/// the in-memory `ApiKeyState`, and notifies discovery so `RunPod/*` models
/// refresh immediately — no restart required. The Data Services RunPod row
/// stores the key under `kask://credentials/runpod` for MCP/training env
/// injection; this is the second half that drives the same key into the OCR
/// endpoint provider live.
fn refresh_runpod_endpoint_key(api_key: Option<String>, cx: &mut App) {
    let Some(provider) = LanguageModelRegistry::global(cx)
        .read(cx)
        .provider(&RUNPOD_PROVIDER_ID)
    else {
        // Provider not registered yet (startup ordering). The key is persisted
        // on the provider's next authenticate/restart; nothing to refresh.
        return;
    };
    provider.set_api_key(api_key, cx).detach();
}

pub(crate) fn render_data_services_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let provider = zed_credentials::global(cx);

    let mut rows: Vec<AnyElement> = Vec::new();
    for (key, label, dashboard_url, env_var) in data_service_descriptors() {
        rows.push(render_data_service_row(
            key,
            label,
            dashboard_url,
            env_var,
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
                        "API keys are stored in the system keychain \
                         (kask://credentials/<key>). A service is enabled when its \
                         key is present — enter the key to activate it.",
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
    provider: Arc<dyn CredentialsProvider>,
    _cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let credential_url = format!("{KASK_CREDENTIAL_NAMESPACE}/{key}");
    let has_key = has_credential(&provider, &[&credential_url], env_var);

    let key_input = if has_key {
        let reset_id = format!("kask-{key}-reset");
        ConfiguredApiCard::new(reset_id, "API Key Configured")
            .button_label("Reset Key")
            .button_tab_index(0)
            .on_click({
                let provider = provider.clone();
                move |_, _, cx| {
                    delete_credential(&provider, &credential_url, cx).detach();
                    if key == "runpod" {
                        refresh_runpod_endpoint_key(None, cx);
                    }
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
                                    if key == "runpod" {
                                        refresh_runpod_endpoint_key(Some(key_value), cx);
                                    }
                                }
                            }),
                    ),
            )
            .into_any_element()
    };

    v_flex()
        .gap_2()
        .child(
            v_flex().gap_0p5().child(Label::new(label)).child(
                Label::new(format!(
                    "Enabled when the API key is present. Stored in the \
                     keychain under kask://credentials/{key}, or set the \
                     {env_var} environment variable."
                ))
                .size(LabelSize::Small)
                .color(Color::Muted),
            ),
        )
        .child(key_input)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The endpoint-refresh id must identify the RunPod provider in the
    /// `LanguageModelRegistry`. [`LanguageModelRegistry::provider`] is an exact
    /// id match and returns `None` on mismatch — a drift would silently turn the
    /// live refresh into a no-op (broken feedback loop). Pin it against the
    /// canonical lowercase id the RunPod provider registers under (D29
    /// `PROVIDER_ID` is `"runpod"`).
    #[test]
    fn runpod_provider_id_resolves_against_registry() {
        assert_eq!(RUNPOD_PROVIDER_ID.0.to_string(), "runpod");
        // Also case-insensitive vs the display-form descriptor id, matching the
        // IPC contract that resolves model names case-insensitively.
        let desc = kask_bridge::INFERENCE_PROVIDERS
            .iter()
            .find(|p| p.credential_key == "runpod")
            .expect("RunPod descriptor present");
        assert!(
            desc.id.eq_ignore_ascii_case(RUNPOD_PROVIDER_ID.0.as_ref()),
            "registry lookup must be case-insensitive per the IPC contract"
        );
    }

    /// Keeps `refresh_runpod_endpoint_key` callable from the Data Services
    /// RunPod row (render write and reset paths), pinning its signature.
    #[test]
    fn refresh_runpod_endpoint_key_symbol_exists() {
        let _: fn(Option<String>, &mut gpui::App) = refresh_runpod_endpoint_key;
    }
}
