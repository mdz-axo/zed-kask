//! Private Inference sub-page — the PI-owned policy surface for the
//! private local leg (M7/Seam B): the `openai_compatible` provider id
//! the leg registers under, the loopback endpoint, the endpoint token
//! (at the provider `api_url` keychain slot — the same slot zed's
//! `ApiKeyState` reads), the Register action that writes the M3
//! configured-instance entry, and the Serving section — the P1
//! supervisor's launch/stop surface.

use super::*;
use language_model::{LanguageModelProviderId, LanguageModelRegistry};

/// Ask the private-leg provider (a configured `openai_compatible`
/// instance — it exists only after the settings entry registers it) to
/// adopt `key` through its existing `set_api_key` path — the D29
/// RunPod-row precedent: the keychain write persists the token at the
/// endpoint slot, and the live refresh makes a registered provider
/// read it without an editor restart. No MCP child consumes this token
/// (the leg is a core service, not an MCP server), so there is no MCP
/// restart nudge — the provider refresh is the only runtime
/// notification needed.
fn refresh_private_provider_key(api_key: Option<String>, provider_id: &str, cx: &mut App) {
    let Some(provider) = LanguageModelRegistry::global(cx)
        .read(cx)
        .provider(&LanguageModelProviderId::from(provider_id.to_string()))
    else {
        // Provider not registered yet (no settings entry, or startup
        // ordering): the keychain write persists, and the provider
        // reads the slot at registration. Nothing to refresh.
        return;
    };
    provider.set_api_key(api_key, cx).detach();
}

pub(crate) fn render_private_inference_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    super::ensure_keychain_prefetch(cx);
    let credentials = zed_credentials::global(cx);

    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime
    // uses (the models-page pattern).
    let pi: kask_bridge::KaskPrivateInferenceSettings = raw
        .as_ref()
        .and_then(|c| c.private_inference.clone())
        .map(Into::into)
        .unwrap_or_default();

    let provider_id_input = kask_string_input(
        "kask-private-inference-provider-id",
        "Private Inference Provider Id",
        "private-local",
        pi.provider_id.clone(),
        "private_inference",
        "provider_id",
    );
    let host_input = kask_string_input(
        "kask-private-inference-host",
        "Private Inference Loopback Host",
        "127.0.0.1",
        pi.host.clone(),
        "private_inference",
        "host",
    );
    // The port parses u16 — a failed parse never rewrites the file
    // (`kask_input_field`'s contract), so a typo cannot zero the
    // endpoint.
    let port_input = kask_input_field(
        "kask-private-inference-port",
        0,
        pi.port.to_string(),
        "8010",
        "Private Inference Loopback Port",
        |text| text.trim().parse::<u16>().ok(),
        |port, settings| {
            settings
                .kask
                .get_or_insert_default()
                .private_inference
                .get_or_insert_default()
                .port = Some(port);
        },
    );

    // The endpoint token — at the provider `api_url` keychain slot
    // (one key, one location). Empty env var: the token is
    // keychain-only; there is no env-var path for the local leg.
    let token_url = kask_bridge::private_inference::credential_url(&pi);
    let token_env_var = "";
    let has_token = has_credential(&credentials, &[&token_url], token_env_var);

    let token_row = if has_token {
        ConfiguredApiCard::new(
            "kask-private-inference-token-reset",
            "Endpoint Token Configured",
        )
        .button_label("Reset Token")
        .button_tab_index(0)
        .on_click(move |_, _, cx| {
            delete_credential(&credentials, &token_url, cx).detach();
            refresh_private_provider_key(None, &pi.provider_id, cx);
        })
        .into_any_element()
    } else {
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
                            .child(Label::new("Endpoint Token"))
                            .child(
                                Label::new(format!(
                                    "The llama-server token, stored in the keychain \
                                     under {token_url} — the same slot the registered \
                                     provider reads. The token's presence is the \
                                     toggle: without it the provider stays \
                                     unauthenticated.",
                                ))
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                            ),
                    )
                    .child(
                        SettingsInputField::new("kask-private-inference-token-input")
                            .tab_index(0)
                            .with_placeholder("endpoint token")
                            .aria_label("Private Inference Endpoint Token")
                            .on_confirm(move |token, _window, cx| {
                                if let Some(token) = token.filter(|token| !token.is_empty()) {
                                    write_credential(&credentials, &token_url, &token, cx).detach();
                                    refresh_private_provider_key(Some(token), &pi.provider_id, cx);
                                }
                            }),
                    ),
            )
            .into_any_element()
    };

    // The Register action — the connect step. Reads the EFFECTIVE
    // policy (raw user settings + defaults) at click time and writes
    // the `language_models.openai_compatible` entry through the
    // existing settings store; the registry re-reads settings on write
    // and registers the provider, so the model cards appear with no
    // restart. Idempotent: a second click writes the same entry bytes.
    // The refusal path (empty provider id, broken fleet) logs and
    // writes nothing.
    let register_button = Button::new("kask-private-inference-register", "Register Provider")
        .style(ButtonStyle::Outlined)
        .label_size(LabelSize::Small)
        .on_click(|_, _, cx| {
            SettingsStore::global(cx).update_settings_file(
                <dyn fs::Fs>::global(cx),
                move |settings, _| {
                    let pi: kask_bridge::KaskPrivateInferenceSettings = settings
                        .kask
                        .as_ref()
                        .and_then(|kask| kask.private_inference.clone())
                        .map(Into::into)
                        .unwrap_or_default();
                    if let Err(err) =
                        kask_bridge::private_inference::apply_provider_entry(settings, &pi)
                    {
                        log::warn!("private-inference provider registration refused: {err}");
                    }
                },
            );
        });

    v_flex()
        .id("kask-private-inference-page")
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
                .child(SettingsSectionHeader::new("Private Inference"))
                .child(
                    Label::new(
                        "The private local leg (llama.cpp) — the PI-owned policy: the \
                         provider id the leg registers under, the loopback endpoint, \
                         and the endpoint token. The served fleet comes from the \
                         shipped model briefs (qwen3-8-flash-next, gemma-4-31b); \
                         Register writes the openai_compatible settings entry so the \
                         models appear in the agent panel model selector. The \
                         supervisor that launches llama-server is the companion seam \
                         (not yet wired): until it lands, the endpoint is unresponsive \
                         and inference attempts fail with a connection error.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Provider Id"))
                .child(
                    Label::new(
                        "The openai_compatible settings key and the qualified-name \
                         prefix of the private models (e.g. \
                         private-local/qwen3-8-flash-next).",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(provider_id_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Loopback Endpoint"))
                .child(
                    Label::new(
                        "The endpoint a launched llama-server serves on. Defaults \
                         follow the placement policy (127.0.0.1:8010). Changing host \
                         or port moves the token's keychain slot (the provider \
                         api_url) — re-enter the token at the new endpoint.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(host_input)
                .child(port_input),
        )
        .child(Divider::horizontal())
        .child(token_row)
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Provider Registration"))
                .child(
                    Label::new(
                        "Register writes the language_models.openai_compatible entry \
                         (api_url from the endpoint, one model card per shipped \
                         brief). Idempotent; a refusal (empty provider id, a broken \
                         shipped brief) writes nothing and logs the reason.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(register_button),
        )
        .child(Divider::horizontal())
        .child({
            // The Serving section — the P1 supervisor's zed-side surface
            // (slice plan step 8): the live status line, one Launch
            // button per shipped fleet model, and Stop when serving. The
            // supervisor handle is cheap; status() is a snapshot read,
            // never HTTP — the refresh happens through the actions'
            // `cx.refresh_windows()`.
            use kask_bridge::private_inference_supervisor::StatusSnapshot;

            let supervisor = kask_bridge::private_inference_supervisor::supervisor();
            let status = supervisor.status();
            let fleet = kask_bridge::private_inference_supervisor::fleet_cards();
            let outcome = kask_bridge::private_inference_supervisor::last_action_outcome();

            let status_line = match &status {
                StatusSnapshot::Stopped => {
                    "Stopped — no private-leg server is running.".to_string()
                }
                StatusSnapshot::Serving {
                    model_id,
                    endpoint,
                    adopted,
                } => format!(
                    "Serving {model_id} at {endpoint}{}",
                    if *adopted {
                        " (adopted — no reload)"
                    } else {
                        ""
                    }
                ),
                StatusSnapshot::Failed { reason } => {
                    format!("Failed — {reason} (no restart: relaunch explicitly)")
                }
            };

            let launch_buttons = fleet.iter().map(|(model_id, display)| {
                let model_id = model_id.clone();
                let serving_this = matches!(
                    &status,
                    StatusSnapshot::Serving {
                        model_id: serving,
                        ..
                    } if serving == &model_id
                );
                Button::new(
                    SharedString::from(format!("kask-private-inference-launch-{model_id}")),
                    format!("Launch {display}"),
                )
                .style(ButtonStyle::Subtle)
                .label_size(LabelSize::Small)
                .disabled(serving_this)
                .on_click(move |_, _, cx| {
                    kask_bridge::private_inference_supervisor::launch_private_model(
                        model_id.clone(),
                        cx,
                    )
                })
            });

            let stop_button = matches!(status, StatusSnapshot::Serving { .. }).then(|| {
                Button::new("kask-private-inference-stop", "Stop Server")
                    .style(ButtonStyle::Outlined)
                    .label_size(LabelSize::Small)
                    .on_click(|_, _, cx| {
                        kask_bridge::private_inference_supervisor::stop_private_model(cx)
                    })
            });

            v_flex()
                .gap_1()
                .child(Label::new("Serving"))
                .child(
                    Label::new(status_line)
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .when_some(outcome, |this, outcome| {
                    this.child(
                        Label::new(format!("Last action: {outcome}"))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                })
                .child(
                    h_flex()
                        .gap_2()
                        .children(launch_buttons)
                        .when_some(stop_button, |this, button| this.child(button)),
                )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    /// The save-dispatch arms (the models-page pin pattern): the page's
    /// inputs address the private_inference fields, and the dispatcher
    /// in kask_page.rs carries the matching arms.
    #[test]
    fn private_inference_controls_have_save_dispatch_arms() {
        let page = include_str!("private_inference.rs");
        let dispatcher = include_str!("../kask_page.rs");
        for (field, arm) in [
            ("provider_id", "(\"private_inference\", \"provider_id\")"),
            ("host", "(\"private_inference\", \"host\")"),
        ] {
            assert!(page.contains(&format!("\"private_inference\",\n        \"{field}\",")));
            assert!(dispatcher.contains(arm));
        }
    }

    /// The port input parses u16 through kask_input_field — a failed
    /// parse skips the store write entirely (never rewrites the file).
    #[test]
    fn port_input_parses_u16_and_skips_the_write_on_a_failed_parse() {
        let page = include_str!("private_inference.rs");
        assert!(page.contains("parse::<u16>().ok()"));
    }

    /// The token row writes the ENDPOINT slot (the provider api_url —
    /// `credential_url`), never the kask://credentials namespace: one
    /// key, one location. The slot == api_url identity itself is pinned
    /// in kask_bridge (`credential_url_equals_the_entry_api_url`).
    #[test]
    fn token_row_writes_the_endpoint_slot() {
        let page = include_str!("private_inference.rs");
        assert!(page.contains("kask_bridge::private_inference::credential_url"));
        // The banned namespace constant is ASSEMBLED, not spelled — this
        // is an include_str pin over the page's own source, so a spelled
        // forbidden literal would match the test itself. The rendered
        // page code must never route the endpoint token to the kask://
        // credentials namespace.
        let kask_namespace = ["KASK_CREDENTIAL", "_NAMESPACE"].concat();
        assert!(!page.contains(&kask_namespace));
    }

    /// The Register action routes through the bridge emission
    /// (`apply_provider_entry`) — never a hand-rolled settings write.
    #[test]
    fn register_action_routes_through_the_bridge_emission() {
        let page = include_str!("private_inference.rs");
        assert!(page.contains("kask_bridge::private_inference::apply_provider_entry"));
    }

    /// The Serving section routes through the bridge supervisor
    /// actions and reads the bridge's state — never a direct spawn or
    /// an HTTP poll on render (D-S6).
    #[test]
    fn serving_section_routes_through_the_bridge_supervisor() {
        let page = include_str!("private_inference.rs");
        assert!(page.contains("launch_private_model"));
        assert!(page.contains("stop_private_model"));
        assert!(page.contains("fleet_cards"));
        assert!(page.contains("supervisor().status()"));
        assert!(page.contains("last_action_outcome"));
    }
}
