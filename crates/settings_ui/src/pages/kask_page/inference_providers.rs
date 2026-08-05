//! Inference Providers sub-page — API key entry + enable toggles for
//! OpenAI-compatible providers (DeepInfra, fal.ai, OpenRouter,
//! KiloCode, Cline, AtlasCloud). When enabled, an `openai_compatible.<provider_id>` entry
//! is written to settings.json so the provider appears in the LLM provider
//! picker. The API key is stored in the keychain under the provider's
//! `api_url` (so zed's OpenAI-compatible provider finds it) and mirrored to
//! `kask://credentials/<key>` for MCP server env injection.

use super::*;

/// Render the Inference Providers sub-page.
///
/// Each provider has an enable toggle and an API key input. When enabled,
/// an `openai_compatible.<provider_id>` entry is written to settings.json so
/// the provider appears in Settings → AI → LLM Providers. The API key is
/// stored in the keychain under the provider's `api_url` (so zed's
/// OpenAI-compatible provider finds it) and mirrored to
/// `kask://credentials/<key>` for MCP server env injection.
pub(crate) fn render_inference_providers_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let provider = zed_credentials::global(cx);
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    // When the subsection is absent or a field is `None`, `From` reads env vars
    // (`DEEPINFRA_API_KEY`, etc.), so a user with an API key set sees the
    // toggle as on even without an explicit `kask.inference_providers` entry.
    // `Default::default()` returns all-false (pure) — we must go through `From`
    // or `from_env()` to get the env-var auto-enable behavior.
    let inference: kask_bridge::KaskInferenceProvidersSettings = raw
        .and_then(|c| c.inference_providers)
        .map(Into::into)
        .unwrap_or_else(kask_bridge::KaskInferenceProvidersSettings::from_env);

    let mut rows: Vec<AnyElement> = Vec::new();
    for desc in kask_bridge::INFERENCE_PROVIDERS {
        // Match on `credential_key` (lowercase canonical key: "deepinfra",
        // "fal", "openrouter", "kilocode", "cline"), not `desc.id`
        // (which is the display-form "DeepInfra", "fal.ai", …).
        // The runtime matchers in `kask_bridge` use `credential_key`; the UI
        // must agree or every toggle renders off and writes no-op.
        let enabled = match desc.credential_key {
            "deepinfra" => inference.deepinfra_enabled,
            "fal" => inference.fal_enabled,
            "openrouter" => inference.openrouter_enabled,
            "kilocode" => inference.kilocode_enabled,
            "cline" => inference.cline_enabled,
            _ => false,
        };
        rows.push(render_inference_provider_row(
            desc,
            enabled,
            provider.clone(),
            cx,
        ));
    }

    v_flex()
        .id("kask-inference-providers-page")
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
                .child(SettingsSectionHeader::new("Inference Providers"))
                .child(
                    Label::new(
                        "API keys for OpenAI-compatible inference providers. \
                         Toggle a provider to register it as an LLM provider in zed \
                         (appears in Settings → AI → LLM Providers and the agent model picker). \
                         Keys are stored in the system keychain.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(v_flex().gap_6().children(rows))
        .into_any_element()
}

fn render_inference_provider_row(
    desc: &kask_bridge::InferenceProviderDescriptor,
    enabled: bool,
    credentials_provider: Arc<dyn CredentialsProvider>,
    _cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let has_key = kask_bridge::has_provider_api_key(desc)
        || was_recently_written(&desc.credential_url())
        || was_recently_written(desc.api_url);
    let provider_id = desc.id;
    let provider_name = desc.name;
    let dashboard_url = desc.dashboard_url;
    let env_var = desc.env_var;
    let api_url = desc.api_url.to_string();
    let credential_url = desc.credential_url();

    let toggle_id = format!("kask-inference-{provider_id}-enabled");
    let enable_toggle = SwitchField::new(
        toggle_id,
        Some(provider_name),
        Some(
            format!(
                "Enable {provider_name} as an OpenAI-compatible LLM provider. \
                 Writes an `openai_compatible.{provider_id}` entry to settings.json \
                 with api_url `{api_url}`. The API key is stored in the keychain."
            )
            .into(),
        ),
        if enabled {
            ToggleState::Selected
        } else {
            ToggleState::Unselected
        },
        move |state, _window, cx| {
            let is_enabled = *state == ToggleState::Selected;
            set_inference_provider_enabled(provider_id, is_enabled, cx);
        },
    )
    .tab_index(0);

    let key_input = if has_key {
        let reset_id = format!("kask-inference-{provider_id}-reset");
        ConfiguredApiCard::new(reset_id, "API Key Configured")
            .button_label("Reset Key")
            .button_tab_index(0)
            .on_click({
                let provider = credentials_provider.clone();
                let desc_credential_url = credential_url;
                let desc_api_url = api_url;
                move |_, _, cx| {
                    // Delete from both keychain locations.
                    let provider = provider.clone();
                    let url1 = desc_api_url.clone();
                    let url2 = desc_credential_url.clone();
                    // Remove from session cache so the UI shows the input field.
                    unmark_recently_written(&url1);
                    unmark_recently_written(&url2);
                    cx.refresh_windows();
                    cx.spawn(async move |cx| {
                        let _ = provider.delete_credentials(&url1, cx).await.log_err();
                        let _ = provider.delete_credentials(&url2, cx).await.log_err();
                    })
                    .detach();
                }
            })
            .into_any_element()
    } else {
        let input_id = format!("kask-inference-{provider_id}-api-key-input");
        let aria_label = format!("{provider_name} API Key");
        let credentials_provider = credentials_provider.clone();
        let desc_credential_url = credential_url;
        let desc_api_url = api_url;
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
                                            format!("{provider_name} dashboard"),
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
                                    "Or set the {env_var} env var and restart Zed for it to take effect. \
                                     The API URL is {desc_api_url}."
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
                                    let provider = credentials_provider.clone();
                                    let url1 = desc_api_url.clone();
                                    let url2 = desc_credential_url.clone();
                                    // Mark both URLs as written so the UI shows "Configured".
                                    mark_recently_written(&url1);
                                    mark_recently_written(&url2);
                                    cx.refresh_windows();
                                    cx.spawn(async move |cx| {
                                        // Write under the api_url (for zed's OpenAI-compatible provider).
                                        let _ = provider
                                            .write_credentials(&url1, "Bearer", key_value.as_bytes(), cx)
                                            .await
                                            .log_err();
                                        // Write under the kask credential URL (for MCP env injection).
                                        let _ = provider
                                            .write_credentials(&url2, "kask", key_value.as_bytes(), cx)
                                            .await
                                            .log_err();
                                    })
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

fn set_inference_provider_enabled(provider_id: &str, enabled: bool, cx: &mut App) {
    // `provider_id` arrives as `desc.id` (display-form: "DeepInfra",
    // "fal.ai", …). Translate to the canonical lowercase `credential_key`
    // so the match arms fire. Without this, toggling a provider writes
    // nothing because none of the lowercase arms match the display-form id.
    let credential_key = match provider_id {
        "DeepInfra" => "deepinfra",
        "fal.ai" => "fal",
        "OpenRouter" => "openrouter",
        "KiloCode" => "kilocode",
        "Cline" => "cline",
        "AtlasCloud" => "atlascloud",
        other => other,
    }
    .to_string();
    SettingsStore::global(cx).update_settings_file(<dyn fs::Fs>::global(cx), move |settings, _| {
        let kask = settings.kask.get_or_insert_default();
        let inference = kask.inference_providers.get_or_insert_default();
        match credential_key.as_str() {
            "deepinfra" => inference.deepinfra_enabled = Some(enabled),
            "fal" => inference.fal_enabled = Some(enabled),
            "openrouter" => inference.openrouter_enabled = Some(enabled),
            "kilocode" => inference.kilocode_enabled = Some(enabled),
            "cline" => inference.cline_enabled = Some(enabled),
            "atlascloud" => inference.atlascloud_enabled = Some(enabled),
            _ => {}
        }
    });
}
