//! Wallet sub-page (D110) — network selector, mainnet consent toggle,
//! hard spending caps, allowlisted Cashu mints, BIP-39 seed & SQLCipher DB
//! key management, and masked NIP-47 NWC URI entry.
//!
//! Public policy (`kask.wallet.*`) lives in `settings.json`.
//! All secrets (`wallet_seed_mnemonic`, `wallet_db_passphrase`,
//! `wallet_nwc_uri`, and the `wallet_mainnet_consent` belt) live exclusively
//! in dedicated OS keychain slots named in `kask_bridge`.

use super::*;
use editor::Editor;
use gpui::{Entity, Global};
use kask_bridge::WalletNetworkContent;
use ui::{ContextMenu, DropdownMenu, DropdownStyle, IconPosition};

const WALLET_NETWORKS: [WalletNetworkContent; 3] = [
    WalletNetworkContent::Regtest,
    WalletNetworkContent::Signet,
    WalletNetworkContent::Mainnet,
];

fn network_label(network: WalletNetworkContent) -> &'static str {
    match network {
        WalletNetworkContent::Regtest => "Regtest (local development)",
        WalletNetworkContent::Signet => "Signet (public test network)",
        WalletNetworkContent::Mainnet => "Mainnet (real Bitcoin / satoshis)",
    }
}

#[derive(Default)]
struct WalletSecretState {
    /// Presence of `(seed_mnemonic, db_passphrase, nwc_uri)` in OS keychain.
    presence: Option<(bool, bool, bool)>,
    presence_read_started: bool,
    saving_seed: bool,
    seed_error: Option<String>,
    /// Newly generated mnemonic shown once so the operator can back it up
    /// before saving. Cleared as soon as Save or Reset completes.
    generated_backup_notice: Option<String>,
    saving_nwc: bool,
    nwc_error: Option<String>,
}
impl Global for WalletSecretState {}

fn wallet_content(
    settings: &mut settings::SettingsContent,
) -> &mut settings::KaskWalletSettingsContent {
    settings
        .kask
        .get_or_insert_default()
        .wallet
        .get_or_insert_default()
}

/// Add or remove the D110 parent-owned grants (`host/wallet_db_unlock` and
/// `host/wallet_spend_approve`) for the `wallet` MCP server.
fn set_wallet_grants(settings: &mut settings::SettingsContent, enabled: bool) {
    let mcp = settings
        .kask
        .get_or_insert_default()
        .mcp
        .get_or_insert_default();
    let delegated = mcp.delegated_tools.get_or_insert_default();
    let server_key = hkask_types::wallet_wire::WALLET_SERVER_ID.to_string();
    let grants = [
        hkask_types::inference_ipc::WALLET_DB_UNLOCK_GRANT,
        hkask_types::inference_ipc::WALLET_SPEND_APPROVE_GRANT,
    ];
    if enabled {
        let list = delegated.entry(server_key).or_default();
        for grant in grants {
            if !list.iter().any(|g| g == grant) {
                list.push(grant.to_string());
            }
        }
    } else if let Some(list) = delegated.get_mut(&server_key) {
        list.retain(|g| !grants.contains(&g.as_str()));
        if list.is_empty() {
            delegated.remove(&server_key);
        }
    }
}

fn parse_u64_row(text: &str) -> Option<u64> {
    text.trim().parse::<u64>().ok()
}

fn parse_mint_list_row(text: &str) -> Option<Vec<String>> {
    Some(
        text.split(|c: char| c == ',' || c.is_whitespace())
            .filter(|entry| !entry.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

fn parse_optional_string_row(text: &str) -> Option<String> {
    Some(text.trim().to_string())
}

fn ensure_wallet_presence_read(cx: &mut App) {
    if !cx.has_global::<WalletSecretState>() {
        cx.set_global(WalletSecretState::default());
    }
    if cx.global::<WalletSecretState>().presence_read_started {
        return;
    }
    cx.global_mut::<WalletSecretState>().presence_read_started = true;
    cx.spawn(async move |cx| {
        async fn present(url: &str) -> bool {
            hkask_keystore::Keychain
                .retrieve_by_url_async(url)
                .await
                .is_ok_and(|secret| !secret.is_empty())
        }
        let seed = present(kask_bridge::WALLET_SEED_MNEMONIC_CREDENTIAL_URL).await;
        let db_key = present(kask_bridge::WALLET_DB_PASSPHRASE_CREDENTIAL_URL).await;
        let nwc = present(kask_bridge::WALLET_NWC_URI_CREDENTIAL_URL).await;
        cx.update(|cx| {
            cx.global_mut::<WalletSecretState>().presence = Some((seed, db_key, nwc));
            cx.refresh_windows();
        });
    })
    .detach();
}

fn masked_input(
    id: &'static str,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Editor> {
    window.use_keyed_state((ElementId::from(id), "editor"), cx, |window, cx| {
        let mut editor = Editor::single_line(window, cx);
        editor.set_placeholder_text(placeholder, window, cx);
        editor.set_masked(true, cx);
        editor
    })
}

fn save_seed(mnemonic_editor: Entity<Editor>, window: &mut Window, cx: &mut App) {
    let phrase = zeroize::Zeroizing::new(mnemonic_editor.read(cx).text(cx));
    if let Err(err) = hkask_wallet_core::validate_bip39_mnemonic(&phrase) {
        let state = cx.global_mut::<WalletSecretState>();
        state.seed_error = Some(err.to_string());
        cx.refresh_windows();
        return;
    }
    let db_passphrase = match hkask_wallet_core::generate_db_passphrase_hex() {
        Ok(key) => key,
        Err(err) => {
            let state = cx.global_mut::<WalletSecretState>();
            state.seed_error = Some(err.to_string());
            cx.refresh_windows();
            return;
        }
    };
    {
        let state = cx.global_mut::<WalletSecretState>();
        state.saving_seed = true;
        state.seed_error = None;
    }
    let provider = zed_credentials::global(cx);
    let write_seed = write_credential(
        &provider,
        kask_bridge::WALLET_SEED_MNEMONIC_CREDENTIAL_URL,
        phrase.trim(),
        cx,
    );
    let write_db = write_credential(
        &provider,
        kask_bridge::WALLET_DB_PASSPHRASE_CREDENTIAL_URL,
        &db_passphrase,
        cx,
    );
    window
        .spawn(cx, async move |cx| {
            write_seed.await;
            write_db.await;
            cx.update(|window, cx| {
                SettingsStore::global(cx).update_settings_file(
                    <dyn fs::Fs>::global(cx),
                    |settings, _| {
                        set_wallet_grants(settings, true);
                    },
                );
                mnemonic_editor.update(cx, |editor, cx| editor.set_text("", window, cx));
                let state = cx.global_mut::<WalletSecretState>();
                state.saving_seed = false;
                state.generated_backup_notice = None;
                let nwc = state.presence.map(|(_, _, nwc)| nwc).unwrap_or(false);
                state.presence = Some((true, true, nwc));
                cx.refresh_windows();
            })
            .ok();
        })
        .detach();
}

fn reset_seed(cx: &mut App) {
    let provider = zed_credentials::global(cx);
    let del_seed = delete_credential(
        &provider,
        kask_bridge::WALLET_SEED_MNEMONIC_CREDENTIAL_URL,
        cx,
    );
    let del_db = delete_credential(
        &provider,
        kask_bridge::WALLET_DB_PASSPHRASE_CREDENTIAL_URL,
        cx,
    );
    cx.spawn(async move |cx| {
        del_seed.await;
        del_db.await;
        cx.update(|cx| {
            SettingsStore::global(cx).update_settings_file(
                <dyn fs::Fs>::global(cx),
                |settings, _| {
                    set_wallet_grants(settings, false);
                },
            );
            let state = cx.global_mut::<WalletSecretState>();
            let nwc = state.presence.map(|(_, _, nwc)| nwc).unwrap_or(false);
            state.presence = Some((false, false, nwc));
            state.seed_error = None;
            state.generated_backup_notice = None;
            cx.refresh_windows();
        });
    })
    .detach();
}

fn save_nwc(nwc_editor: Entity<Editor>, window: &mut Window, cx: &mut App) {
    let uri = zeroize::Zeroizing::new(nwc_editor.read(cx).text(cx));
    let summary = match hkask_wallet_core::validate_nwc_uri(&uri) {
        Ok(summary) => summary,
        Err(err) => {
            let state = cx.global_mut::<WalletSecretState>();
            state.nwc_error = Some(err.to_string());
            cx.refresh_windows();
            return;
        }
    };
    {
        let state = cx.global_mut::<WalletSecretState>();
        state.saving_nwc = true;
        state.nwc_error = None;
    }
    let label = summary
        .relays
        .first()
        .cloned()
        .unwrap_or_else(|| "NWC connected".to_string());
    let provider = zed_credentials::global(cx);
    let write_nwc = write_credential(
        &provider,
        kask_bridge::WALLET_NWC_URI_CREDENTIAL_URL,
        uri.trim(),
        cx,
    );
    window
        .spawn(cx, async move |cx| {
            write_nwc.await;
            cx.update(|window, cx| {
                SettingsStore::global(cx).update_settings_file(
                    <dyn fs::Fs>::global(cx),
                    move |settings, _| {
                        wallet_content(settings).nwc_label = Some(label);
                    },
                );
                nwc_editor.update(cx, |editor, cx| editor.set_text("", window, cx));
                let state = cx.global_mut::<WalletSecretState>();
                state.saving_nwc = false;
                let (seed, db_key) = state
                    .presence
                    .map(|(seed, db_key, _)| (seed, db_key))
                    .unwrap_or((false, false));
                state.presence = Some((seed, db_key, true));
                cx.refresh_windows();
            })
            .ok();
        })
        .detach();
}

fn reset_nwc(cx: &mut App) {
    let provider = zed_credentials::global(cx);
    let del_nwc = delete_credential(&provider, kask_bridge::WALLET_NWC_URI_CREDENTIAL_URL, cx);
    cx.spawn(async move |cx| {
        del_nwc.await;
        cx.update(|cx| {
            SettingsStore::global(cx).update_settings_file(
                <dyn fs::Fs>::global(cx),
                |settings, _| {
                    wallet_content(settings).nwc_label = Some(String::new());
                },
            );
            let state = cx.global_mut::<WalletSecretState>();
            let (seed, db_key) = state
                .presence
                .map(|(seed, db_key, _)| (seed, db_key))
                .unwrap_or((false, false));
            state.presence = Some((seed, db_key, false));
            state.nwc_error = None;
            cx.refresh_windows();
        });
    })
    .detach();
}

fn render_seed_block(window: &mut Window, cx: &mut Context<SettingsWindow>) -> AnyElement {
    ensure_wallet_presence_read(cx);
    let (presence, saving, error, notice) = {
        let state = cx.global::<WalletSecretState>();
        (
            state.presence,
            state.saving_seed,
            state.seed_error.clone(),
            state.generated_backup_notice.clone(),
        )
    };

    let body: AnyElement = match presence {
        None => Label::new("Checking the keychain…")
            .size(LabelSize::Small)
            .color(Color::Muted)
            .into_any_element(),
        Some((true, true, _)) => ConfiguredApiCard::new(
            "kask-wallet-seed-reset",
            "BIP-39 Seed & SQLCipher DB Key Saved in Keychain",
        )
        .button_label("Reset Seed")
        .button_tab_index(0)
        .on_click(|_, _, cx| reset_seed(cx))
        .into_any_element(),
        Some(_) => {
            let mnemonic_editor = masked_input(
                "kask-wallet-seed-mnemonic",
                "12 or 24 BIP-39 words…",
                window,
                cx,
            );
            let mut block = v_flex().gap_2();
            if let Some(notice) = notice {
                block = block.child(
                    Label::new(notice)
                        .size(LabelSize::Small)
                        .color(Color::Warning),
                );
            }
            if let Some(error) = error {
                block = block.child(Label::new(error).size(LabelSize::Small).color(Color::Error));
            }
            let editor_for_gen = mnemonic_editor.clone();
            let editor_for_save = mnemonic_editor.clone();
            block
                .child(mnemonic_editor)
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("kask-wallet-seed-generate", "Generate 12-Word Seed")
                                .on_click(move |_, window, cx| {
                                    match hkask_wallet_core::generate_bip39_mnemonic_12() {
                                        Ok(phrase) => {
                                            editor_for_gen.update(cx, |ed, cx| {
                                                ed.set_text(phrase.as_str(), window, cx);
                                            });
                                            let state = cx.global_mut::<WalletSecretState>();
                                            state.seed_error = None;
                                            state.generated_backup_notice = Some(
                                                "New 12-word seed populated. Back it up offline \
                                                 before clicking Save Seed."
                                                    .to_string(),
                                            );
                                            cx.refresh_windows();
                                        }
                                        Err(err) => {
                                            let state = cx.global_mut::<WalletSecretState>();
                                            state.seed_error = Some(err.to_string());
                                            cx.refresh_windows();
                                        }
                                    }
                                }),
                        )
                        .child(
                            Button::new(
                                "kask-wallet-seed-save",
                                if saving { "Saving…" } else { "Save Seed" },
                            )
                            .disabled(saving)
                            .on_click(move |_, window, cx| {
                                save_seed(editor_for_save.clone(), window, cx);
                            }),
                        ),
                )
                .into_any_element()
        }
    };

    v_flex()
        .gap_2()
        .child(Label::new("Cashu Seed & Encrypted DB Key").size(LabelSize::Default))
        .child(
            Label::new(
                "Stored exclusively in the OS keychain (wallet_seed_mnemonic and \
                 wallet_db_passphrase). Used for NUT-13 deterministic secrets and \
                 SQLCipher database encryption; never written to settings.json.",
            )
            .size(LabelSize::Small)
            .color(Color::Muted),
        )
        .child(body)
        .into_any_element()
}

fn render_nwc_block(
    nwc_label: String,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    ensure_wallet_presence_read(cx);
    let (presence, saving, error) = {
        let state = cx.global::<WalletSecretState>();
        (state.presence, state.saving_nwc, state.nwc_error.clone())
    };

    let body: AnyElement = match presence {
        None => Label::new("Checking the keychain…")
            .size(LabelSize::Small)
            .color(Color::Muted)
            .into_any_element(),
        Some((_, _, true)) => {
            let status = if nwc_label.is_empty() {
                "NWC Connection Saved in Keychain".to_string()
            } else {
                format!("NWC Connection Saved ({nwc_label})")
            };
            ConfiguredApiCard::new("kask-wallet-nwc-reset", status)
                .button_label("Remove NWC")
                .button_tab_index(0)
                .on_click(|_, _, cx| reset_nwc(cx))
                .into_any_element()
        }
        Some((_, _, false)) => {
            let nwc_editor = masked_input(
                "kask-wallet-nwc-uri",
                "nostr+walletconnect://<pubkey>?relay=wss://…&secret=…",
                window,
                cx,
            );
            let mut block = v_flex().gap_2();
            if let Some(error) = error {
                block = block.child(Label::new(error).size(LabelSize::Small).color(Color::Error));
            }
            let editor_for_save = nwc_editor.clone();
            block
                .child(nwc_editor)
                .child(
                    Button::new(
                        "kask-wallet-nwc-save",
                        if saving { "Saving…" } else { "Save NWC URI" },
                    )
                    .disabled(saving)
                    .on_click(move |_, window, cx| {
                        save_nwc(editor_for_save.clone(), window, cx);
                    }),
                )
                .into_any_element()
        }
    };

    v_flex()
        .gap_2()
        .child(Label::new("Lightning NWC Connection (NIP-47)").size(LabelSize::Default))
        .child(
            Label::new(
                "Stored exclusively in the OS keychain (wallet_nwc_uri) and read only by \
                 the editor after native spend approval. The wallet MCP child never sees \
                 the raw NWC secret.",
            )
            .size(LabelSize::Small)
            .color(Color::Muted),
        )
        .child(body)
        .into_any_element()
}

pub(crate) fn render_wallet_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let wallet: kask_bridge::KaskWalletSettings = raw
        .and_then(|c| c.wallet)
        .map(Into::into)
        .unwrap_or_default();

    let current_network = wallet.network;
    let network_menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
        for net in WALLET_NETWORKS {
            menu = menu.toggleable_entry(
                network_label(net),
                net == current_network,
                IconPosition::Start,
                None,
                move |_, cx| {
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            wallet_content(settings).network = Some(net);
                        },
                    );
                },
            );
        }
        menu
    });
    let network_dropdown = DropdownMenu::new(
        "kask-wallet-network",
        network_label(current_network),
        network_menu,
    )
    .style(DropdownStyle::Outlined)
    .full_width(true);

    // Mainnet consent toggle writes BOTH `kask.wallet.mainnet_enabled` and the
    // keychain-backed `WALLET_MAINNET_CONSENT_CREDENTIAL_URL` flag (T10).
    let mainnet_toggle = SwitchField::new(
        "kask-wallet-mainnet-enabled",
        Some("Enable Mainnet Spending"),
        Some(
            "Off by default. Enabling records an OS keychain consent flag alongside \
             kask.wallet.mainnet_enabled. Every spend still requires a native editor \
             confirmation modal."
                .into(),
        ),
        ToggleState::from(wallet.mainnet_enabled),
        |state, _, cx| {
            let enabled = *state == ToggleState::Selected;
            let provider = zed_credentials::global(cx);
            if enabled {
                write_credential(
                    &provider,
                    kask_bridge::WALLET_MAINNET_CONSENT_CREDENTIAL_URL,
                    "enabled",
                    cx,
                )
                .detach();
            } else {
                delete_credential(
                    &provider,
                    kask_bridge::WALLET_MAINNET_CONSENT_CREDENTIAL_URL,
                    cx,
                )
                .detach();
            }
            SettingsStore::global(cx).update_settings_file(
                <dyn fs::Fs>::global(cx),
                move |settings, _| {
                    wallet_content(settings).mainnet_enabled = Some(enabled);
                },
            );
        },
    )
    .tab_index(0);

    let per_tx_input = kask_input_field(
        "kask-wallet-per-tx-cap",
        0,
        wallet.per_tx_cap_sats.to_string(),
        "10000",
        "Per-Transaction Cap (sats)",
        parse_u64_row,
        |parsed, settings| {
            wallet_content(settings).per_tx_cap_sats = Some(parsed);
        },
    );

    let daily_cap_input = kask_input_field(
        "kask-wallet-daily-cap",
        0,
        wallet.daily_cap_sats.to_string(),
        "50000",
        "Rolling 24h Spend Cap (sats)",
        parse_u64_row,
        |parsed, settings| {
            wallet_content(settings).daily_cap_sats = Some(parsed);
        },
    );

    let max_fee_input = kask_input_field(
        "kask-wallet-max-fee",
        0,
        wallet.max_fee_sats.to_string(),
        "100",
        "Max Fee Cap (sats)",
        parse_u64_row,
        |parsed, settings| {
            wallet_content(settings).max_fee_sats = Some(parsed);
        },
    );

    let allowed_mints_input = kask_input_field(
        "kask-wallet-allowed-mints",
        0,
        wallet.allowed_mints.join(", "),
        "https://mint.example.com, …",
        "Allowlisted Cashu Mints",
        parse_mint_list_row,
        |parsed, settings| {
            wallet_content(settings).allowed_mints = Some(parsed);
        },
    );

    let default_mint_input = kask_input_field(
        "kask-wallet-default-mint",
        0,
        wallet.default_mint.clone(),
        "https://mint.example.com",
        "Default Cashu Mint",
        parse_optional_string_row,
        |parsed, settings| {
            wallet_content(settings).default_mint = Some(parsed);
        },
    );

    let seed_block = render_seed_block(window, cx);
    let nwc_block = render_nwc_block(wallet.nwc_label, window, cx);

    v_flex()
        .id("kask-wallet-page")
        .track_scroll(scroll_handle)
        .overflow_y_scroll()
        .size_full()
        .gap_4()
        .p_4()
        .child(SettingsSectionHeader::new("Network & Mainnet Gate"))
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Network").size(LabelSize::Default))
                .child(network_dropdown),
        )
        .child(mainnet_toggle)
        .child(Divider::horizontal())
        .child(SettingsSectionHeader::new("Editor-Enforced Spending Caps"))
        .child(per_tx_input)
        .child(daily_cap_input)
        .child(max_fee_input)
        .child(Divider::horizontal())
        .child(SettingsSectionHeader::new("Allowlisted Cashu Mints"))
        .child(allowed_mints_input)
        .child(default_mint_input)
        .child(Divider::horizontal())
        .child(SettingsSectionHeader::new(
            "Keychain Credentials (Never in settings.json)",
        ))
        .child(seed_block)
        .child(nwc_block)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallet_grants_toggle_adds_and_removes_both_host_effects() {
        let mut settings = settings::SettingsContent::default();
        set_wallet_grants(&mut settings, true);
        let grants = settings
            .kask
            .as_ref()
            .and_then(|k| k.mcp.as_ref())
            .and_then(|m| m.delegated_tools.as_ref())
            .and_then(|d| d.get(hkask_types::wallet_wire::WALLET_SERVER_ID))
            .expect("wallet grants present");
        assert!(grants.contains(&hkask_types::inference_ipc::WALLET_DB_UNLOCK_GRANT.to_string()));
        assert!(
            grants.contains(&hkask_types::inference_ipc::WALLET_SPEND_APPROVE_GRANT.to_string())
        );

        set_wallet_grants(&mut settings, false);
        let after = settings
            .kask
            .as_ref()
            .and_then(|k| k.mcp.as_ref())
            .and_then(|m| m.delegated_tools.as_ref())
            .and_then(|d| d.get(hkask_types::wallet_wire::WALLET_SERVER_ID));
        assert!(after.is_none(), "reset removes wallet grants cleanly");
    }

    #[test]
    fn cap_and_mint_parsers_handle_valid_and_invalid_inputs() {
        assert_eq!(parse_u64_row("10000"), Some(10_000));
        assert_eq!(parse_u64_row(" -5 "), None);
        assert_eq!(parse_u64_row("abc"), None);
        assert_eq!(
            parse_mint_list_row("https://a.example, https://b.example "),
            Some(vec![
                "https://a.example".to_string(),
                "https://b.example".to_string(),
            ])
        );
        assert_eq!(parse_mint_list_row(""), Some(Vec::new()));
    }
}
