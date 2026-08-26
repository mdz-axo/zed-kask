//! Security sub-page — DB passphrase rotation for all kask memory databases.
//!
//! Shows the current DB passphrase status (configured/not configured) and
//! provides an input field to change the passphrase. On confirm, the DB is
//! re-encrypted with the new passphrase (via `kask_bridge::rotate_curator_db_passphrase`),
//! the new passphrase is written to the keychain (`kask://credentials/hkask_db_passphrase`),
//! and MCP servers are nudged to restart with the new passphrase.
//!
//! The swarm memory passphrase has its own field on the Swarm page, which
//! also triggers rotation via `kask_bridge::rotate_swarm_memory_db_passphrase`.

use super::*;

/// The credential URL for the DB passphrase in zed's keychain.
const DB_PASSPHRASE_URL: &str = "kask://credentials/hkask_db_passphrase";

pub(crate) fn render_security_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let credentials_provider = zed_credentials::global(cx);
    let db_passphrase_configured =
        has_credential(&credentials_provider, &[DB_PASSPHRASE_URL], "HKASK_DB_PASSPHRASE");

    // DB passphrase — keychain-backed. Shows "Configured" card if the
    // passphrase exists, or an input field to set/change it. On confirm,
    // the DB is rotated before the keychain write so a rotation failure
    // leaves the old passphrase intact.
    let db_passphrase_card = if db_passphrase_configured {
        let provider = credentials_provider.clone();
        v_flex()
            .gap_2()
            .child(
                ConfiguredApiCard::new(
                    "kask-security-db-passphrase-reset",
                    "DB Passphrase Configured",
                )
                .button_label("Change Passphrase")
                .button_tab_index(0)
                .into_any_element(),
            )
            .child(
                v_flex().gap_0p5().child(
                    Label::new(
                        "The DB passphrase encrypts the curator, corpus, and kata-kanban \
                         SQLCipher databases. It is provisioned on first run with the \
                         default 'allostery'. To change it, enter a new passphrase below \
                         (>=8 chars) — the DB will be re-encrypted atomically before the \
                         new passphrase is saved. If rotation fails, the old passphrase \
                         remains in effect.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
            )
            .child(
                SettingsInputField::new("kask-security-db-passphrase-change")
                    .tab_index(0)
                    .with_placeholder("new-passphrase")
                    .aria_label("New DB Passphrase")
                    .confirm_on_focus_out()
                    .on_confirm(move |value, _window, cx| {
                        if let Some(pw) = value.filter(|v| !v.is_empty()) {
                            spawn_db_passphrase_rotation(&pw.clone(), cx).detach();
                        }
                    }),
            )
            .into_any_element()
    } else {
        let provider = credentials_provider.clone();
        v_flex()
            .gap_2()
            .child(
                v_flex().gap_0p5().child(Label::new("DB Passphrase")).child(
                    Label::new(
                        "The DB passphrase encrypts the curator, corpus, and kata-kanban \
                         SQLCipher databases. It is provisioned on first run with the \
                         default 'allostery'. If no passphrase is configured, set one \
                         below (>=8 chars). The DB will be re-encrypted atomically before \
                         the new passphrase is saved. If rotation fails, the old passphrase \
                         remains in effect. Or set HKASK_DB_PASSPHRASE and restart Zed.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
            )
            .child(
                SettingsInputField::new("kask-security-db-passphrase-input")
                    .tab_index(0)
                    .with_placeholder("allostery")
                    .aria_label("DB Passphrase")
                    .confirm_on_focus_out()
                    .on_confirm(move |value, _window, cx| {
                        if let Some(pw) = value.filter(|v| !v.is_empty()) {
                            spawn_db_passphrase_rotation(&pw.clone(), cx).detach();
                        }
                    }),
            )
            .into_any_element()
    };

    v_flex()
        .id("kask-security-page")
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
                .child(SettingsSectionHeader::new("Security"))
                .child(
                    Label::new(
                        "Manage SQLCipher passphrases for kask memory databases. \
                         Changing a passphrase re-encrypts the database atomically — \
                         the old DB is preserved until the new one is verified, so no \
                         data is lost on failure. After rotation, MCP servers restart \
                         automatically with the new passphrase.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(db_passphrase_card)
        .into_any_element()
}

/// Spawn a DB passphrase rotation task.
///
/// This runs the rotation on a background thread (it does file I/O and SQL
/// operations that can take seconds), then writes the new passphrase to the
/// keychain and nudges MCP servers to restart. Rotation failures are logged
/// at `warn` level — the old passphrase remains in effect.
///
/// The rotation MUST complete before the keychain write — if the rotation
/// fails, we do NOT write the new passphrase, so the old one stays in the
/// keychain and MCP servers continue using it.
fn spawn_db_passphrase_rotation(new_passphrase: &str, cx: &mut App) -> Task<()> {
    let new_passphrase = new_passphrase.to_string();
    let credentials_provider = zed_credentials::global(cx);
    cx.spawn(async move |cx| {
        // 1. Rotate the DB. This runs on the background executor because it
        //    does file I/O and SQL operations. The rotation resolves the old
        //    passphrase from the keychain internally.
        let passphrase_for_rotation = new_passphrase.clone();
        let rotation_result = cx
            .background_spawn(async move {
                kask_bridge::rotate_curator_db_passphrase(&passphrase_for_rotation)
            })
            .await;

        match rotation_result {
            Ok(()) => {
                tracing::info!(
                    target: "hkask.settings.security",
                    "DB passphrase rotation succeeded — writing new passphrase to keychain"
                );
                // 2. Write the new passphrase to the keychain. This triggers
                //    nudge_mcp_servers via write_credential, which restarts
                //    MCP servers with the new passphrase.
                let url = DB_PASSPHRASE_URL.to_string();
                let _ = credentials_provider
                    .write_credentials(&url, "kask", new_passphrase.as_bytes(), &cx)
                    .await
                    .log_err();
                // Mark as recently written so the UI shows "Configured".
                mark_recently_written(&url);
                // 3. Nudge MCP servers to restart with the new passphrase.
                cx.update(|cx| nudge_mcp_servers(cx));
            }
            Err(error) => {
                tracing::warn!(
                    target: "hkask.settings.security",
                    %error,
                    "DB passphrase rotation failed — the old passphrase remains in effect. \
                     The new passphrase was NOT saved to the keychain."
                );
                // Also update the UI to show the error. We can't show a toast
                // from here, but the tracing span surfaces in the logs.
                // The operator can check the logs for the failure reason.
            }
        }
    })
}
