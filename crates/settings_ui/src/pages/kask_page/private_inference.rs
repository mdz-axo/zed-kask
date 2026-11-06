//! Private Inference sub-page — the PI-owned policy surface for the
//! private local leg (M7/Seam B): the `openai_compatible` provider id
//! the leg registers under, the loopback endpoint, the endpoint token
//! (at the provider `api_url` keychain slot — the same slot zed's
//! `ApiKeyState` reads), the Register action that writes the M3
//! configured-instance entry, and the Serving section — the P1
//! supervisor's launch/stop surface.

use super::*;

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

    // zed-kask: D100 (the review's F4) — bind the policy fields up front:
    // the token row's closures below move `pi`'s fields, so the three
    // policy rows (built after it) read from these bindings.
    let local_providers_initial = pi.local_provider_ids.join(", ");
    let disabled_servers_initial = pi.private_thread_disabled_servers.join(", ");
    let embedding_model_initial = pi.embedding_model.clone();
    // The entry-time prefix validation's allowed set: the private provider
    // id plus the local-provider set (the same set
    // `resolved_local_providers` merges; the wiring-time builder refusal
    // stays the enforcement home).
    let mut allowed_prefixes = vec![pi.provider_id.clone()];
    allowed_prefixes.extend(pi.local_provider_ids.iter().cloned());

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
            kask_bridge::private_inference_supervisor::refresh_provider_key(
                None,
                &pi.provider_id,
                cx,
            );
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
                                    "Optional — a token you supply is stored in the \
                                     keychain under {token_url} (the same slot the \
                                     registered provider reads). When absent, one is \
                                     minted automatically at first use: the leg needs \
                                     zero manual credential entry.",
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
                                    kask_bridge::private_inference_supervisor::refresh_provider_key(
                                        Some(token),
                                        &pi.provider_id,
                                        cx,
                                    );
                                }
                            }),
                    ),
            )
            .into_any_element()
    };

    // The Register action — the explicit refresh: mint-if-absent
    // provisioning, then the M3 emission written through the settings
    // store (an overwrite — Register is the refresh after host/port
    // edits). Startup does the same write-only-if-absent
    // (`ensure_registered`), so this button is the mid-session tool.
    let register_button = Button::new("kask-private-inference-register", "Register Provider")
        .style(ButtonStyle::Outlined)
        .label_size(LabelSize::Small)
        .on_click(|_, _, cx| {
            kask_bridge::private_inference_supervisor::register_provider_entry(cx)
        });

    // zed-kask: D100 (the review's F4) — the three policy fields the
    // content struct carries that the page lacked: the local-leg set, the
    // private-thread tool perimeter, and the private embedding model.
    // Each writes through the settings store with the parse-gate contract
    // (a failed parse never rewrites the file). The embedding_model row
    // additionally validates the local prefix at ENTRY — the wiring-time
    // refusal (the port build) remains the enforcement home; the row
    // fails the obvious mistake (a cloud-prefixed model string) where the
    // operator types it. The empty string is the explicit opt-out (the
    // keyword-only degrade); the CSV rows treat an empty confirm as NO
    // write (an empty list is not the same as an absent override).
    fn split_csv(text: &str) -> Vec<String> {
        text.split(',')
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(str::to_string)
            .collect()
    }

    let local_providers_input = kask_input_field(
        "kask-private-inference-local-providers",
        0,
        local_providers_initial,
        "ollama",
        "Local Provider Ids",
        |text| {
            let parsed = split_csv(text);
            (!parsed.is_empty()).then_some(parsed)
        },
        |parsed, settings| {
            settings
                .kask
                .get_or_insert_default()
                .private_inference
                .get_or_insert_default()
                .local_provider_ids = Some(parsed);
        },
    );

    let disabled_servers_input = kask_input_field(
        "kask-private-inference-disabled-servers",
        0,
        disabled_servers_initial,
        "media",
        "Private-Thread Disabled Servers",
        |text| {
            let parsed = split_csv(text);
            (!parsed.is_empty()).then_some(parsed)
        },
        |parsed, settings| {
            settings
                .kask
                .get_or_insert_default()
                .private_inference
                .get_or_insert_default()
                .private_thread_disabled_servers = Some(parsed);
        },
    );

    let embedding_model_input = kask_input_field(
        "kask-private-inference-embedding-model",
        0,
        embedding_model_initial,
        "empty = the keyword-only degrade (no local embedding)",
        "Private Embedding Model",
        move |text| {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
            let (prefix, _) = trimmed.split_once('/')?;
            allowed_prefixes
                .iter()
                .any(|allowed| allowed == prefix)
                .then(|| trimmed.to_string())
        },
        |parsed, settings| {
            settings
                .kask
                .get_or_insert_default()
                .private_inference
                .get_or_insert_default()
                .embedding_model = Some(parsed);
        },
    );

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
                        "The private local leg (llama.cpp): the provider id the leg \
                         registers under and the loopback endpoint. The served fleet \
                         comes from the shipped model briefs (qwen3-8-flash-next, \
                         flint, gemma-4-31b, muse-glimmer-30b) — all four downloaded \
                         models. The provider entry and the endpoint token are \
                         provisioned automatically (a token you enter here wins), so \
                         the private models appear in the agent panel model selector \
                         with zero manual steps; Register is the explicit mid-session \
                         refresh (e.g. after host/port edits). One model holds the \
                         RAM at a time — launching another swaps (stops the current \
                         server, then loads the new one).",
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
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Private-Thread Policy"))
                .child(
                    Label::new(
                        "The three policy fields a privacy-marked thread reads \
                         (kask.private_inference): the local-leg provider ids \
                         (local by construction — a marked thread accepts these \
                         providers' models and never a cloud provider's), the MCP \
                         servers whose tools are disabled on marked threads \
                         (their assisting inference cannot run locally), and the \
                         LOCAL embedding model for private memory (empty = the \
                         keyword-only degrade; a cloud-prefixed value is refused \
                         here and at wiring).",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(local_providers_input)
                .child(disabled_servers_input)
                .child(embedding_model_input),
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
                    format!("Failed — {reason} (launch again to retry: the supervisor never restarts on its own)")
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

    /// The Register action routes through the bridge's registration
    /// (`register_provider_entry` — mint-if-absent + the emission in
    /// one home), never a hand-rolled settings write at the page.
    #[test]
    fn register_action_routes_through_the_bridge_registration() {
        let page = include_str!("private_inference.rs");
        assert!(
            page.contains("kask_bridge::private_inference_supervisor::register_provider_entry")
        );
        // The banned hand-rolled emission is ASSEMBLED — an include_str
        // pin over the page's own source would match its own forbidden
        // literal (the token-row pin's class).
        let hand_rolled = ["apply_", "provider_entry"].concat();
        assert!(
            !page.contains(&hand_rolled),
            "the page never hand-writes the entry — the bridge owns the emission"
        );
    }

    /// The zero-manual-entry contract is stated where the operator
    /// reads it: the token row explains the auto-mint, and the page
    /// header explains the swap (the "downloaded = available" UX
    /// ruling, 2026-11-05).
    #[test]
    fn the_page_states_the_auto_mint_and_the_swap() {
        let page = include_str!("private_inference.rs");
        assert!(page.contains("minted automatically"));
        assert!(page.contains("launching another swaps"));
        assert!(page.contains("all four downloaded models"));
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

    /// zed-kask: D100 (the review's F4) — the three policy fields the
    /// content struct carries have page rows, and each writes through the
    /// settings store with the parse-gate contract (a failed parse never
    /// rewrites the file): the local-leg set, the private-thread tool
    /// perimeter, and the private embedding model.
    #[test]
    fn policy_fields_have_rows_that_write_their_settings() {
        let page = include_str!("private_inference.rs");
        for (field_id, field) in [
            (
                "kask-private-inference-local-providers",
                "local_provider_ids",
            ),
            (
                "kask-private-inference-disabled-servers",
                "private_thread_disabled_servers",
            ),
            ("kask-private-inference-embedding-model", "embedding_model"),
        ] {
            assert!(
                page.contains(field_id),
                "the {field} row exists on the page"
            );
            assert!(
                page.contains(&format!(".{field} = Some(parsed);")),
                "the {field} row writes its settings field"
            );
        }
    }

    /// The embedding_model row validates the local prefix at ENTRY — the
    /// non-empty value must be provider-qualified with the prefix in the
    /// local leg (the same set `resolved_local_providers` merges); the
    /// wiring-time builder refusal stays the enforcement home, and the
    /// empty confirm writes the explicit opt-out (the keyword-only
    /// degrade). The CSV rows treat an empty confirm as NO write (an
    /// empty list is not the same as an absent override).
    #[test]
    fn embedding_model_row_validates_the_local_prefix_at_entry() {
        let page = include_str!("private_inference.rs");
        assert!(
            page.contains("split_once('/')?;"),
            "the row requires a provider-qualified value"
        );
        assert!(
            page.contains("allowed_prefixes"),
            "the prefix check reads the local leg (the private provider id plus the local set)"
        );
        assert!(
            page.contains("(!parsed.is_empty()).then_some(parsed)"),
            "the CSV rows skip the write on an empty confirm"
        );
        // The empty embedding confirm writes the explicit opt-out — Some(""),
        // never a skipped write (the opt-out is a storable state).
        assert!(
            page.contains("Some(trimmed.to_string())"),
            "an empty embedding confirm writes the explicit empty opt-out"
        );
    }
}
