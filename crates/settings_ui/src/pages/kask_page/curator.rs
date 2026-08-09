//! Curator sub-pages:
//! - Curator: `always_on` toggle + `algedonic_threshold`.
//! - Curator Email: MXroute SMTP config + keychain-backed password.

use super::*;

// ---------------------------------------------------------------------------
// Curator sub-page
// ---------------------------------------------------------------------------

pub(crate) fn render_curator_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let curator: kask_bridge::KaskCuratorSettings = raw
        .and_then(|c| c.curator)
        .map(Into::into)
        .unwrap_or_default();
    let always_on = curator.always_on;
    let algedonic_threshold = curator.algedonic_threshold.to_string();

    let always_on_toggle = SwitchField::new(
        "kask-curator-always-on",
        Some("Always On"),
        Some(
            "Whether the Curator agent is always-on (runs regulation loops in background).".into(),
        ),
        if always_on {
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
                        .curator
                        .get_or_insert_default()
                        .always_on = Some(enabled);
                },
            );
        },
    )
    .tab_index(0);

    let threshold_input = SettingsInputField::new("kask-curator-algedonic-threshold")
        .tab_index(0)
        .with_initial_text(algedonic_threshold)
        .with_placeholder("0.8")
        .aria_label("Algedonic Threshold")
        .confirm_on_focus_out()
        .on_confirm(move |value, _window, cx| {
            if let Some(text) = value {
                if let Ok(parsed) = text.parse::<f64>() {
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            settings
                                .kask
                                .get_or_insert_default()
                                .curator
                                .get_or_insert_default()
                                .algedonic_threshold = Some(parsed);
                        },
                    );
                }
            }
        });

    v_flex()
        .id("kask-curator-page")
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
                .child(SettingsSectionHeader::new("Curator"))
                .child(
                    Label::new(
                        "The Curator agent runs regulation loops and monitors algedonic signals.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(always_on_toggle)
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Algedonic Threshold"))
                .child(
                    Label::new(
                        "Algedonic signal threshold (0.0–1.0). Signals above this trigger the Curator.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(threshold_input),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Curator Email sub-page
// ---------------------------------------------------------------------------

/// Render the Curator Email sub-page.
///
/// Non-secret email fields (MXroute server, SMTP username, From address,
/// alert recipient, authorized senders, poll/digest intervals) live in
/// settings.json under `kask.curator.email`. The SMTP password is stored in
/// the OS keychain under `kask://credentials/hkask_smtp_password` and
/// injected into MCP server child processes as `HKASK_SMTP_PASSWORD`.
pub(crate) fn render_curator_email_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let provider = zed_credentials::global(cx);
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let email: kask_bridge::KaskCuratorEmailSettings = raw
        .and_then(|c| c.curator)
        .and_then(|c| c.email)
        .map(Into::into)
        .unwrap_or_default();

    let mxroute_server = email.mxroute_server;
    let smtp_username = email.smtp_username;
    let curator_email = email.curator_email;
    let alert_email = email.alert_email;
    let authorized_emails = email.authorized_emails.join(", ");
    let inbox_poll_interval = email.inbox_poll_interval_secs.to_string();
    let digest_interval = email.digest_interval_secs.to_string();

    // SMTP password — keychain-backed, mirrors the data-service API key pattern.
    let smtp_password_url = format!("{KASK_CREDENTIAL_NAMESPACE}/hkask_smtp_password");
    let has_password = has_credential(&provider, &[&smtp_password_url], "HKASK_SMTP_PASSWORD");
    let password_card = if has_password {
        ConfiguredApiCard::new(
            "kask-curator-email-smtp-password-reset",
            "SMTP Password Configured",
        )
        .button_label("Reset Password")
        .button_tab_index(0)
        .on_click({
            let provider = provider.clone();
            let url = smtp_password_url;
            move |_, _, cx| {
                delete_credential(&provider, &url, cx).detach();
            }
        })
        .into_any_element()
    } else {
        let provider = provider.clone();
        let url = smtp_password_url;
        v_flex()
            .gap_2()
            .child(
                v_flex().gap_0p5().child(Label::new("SMTP Password")).child(
                    Label::new(
                        "The mailbox password for HKASK_SMTP_USERNAME. Stored in the \
                             keychain under kask://credentials/hkask_smtp_password, or set \
                             the HKASK_SMTP_PASSWORD env var and restart Zed.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
            )
            .child(
                SettingsInputField::new("kask-curator-email-smtp-password-input")
                    .tab_index(0)
                    .with_placeholder("••••••••••••")
                    .aria_label("SMTP Password")
                    .confirm_on_focus_out()
                    .on_confirm(move |value, _window, cx| {
                        if let Some(pw) = value.filter(|v| !v.is_empty()) {
                            write_credential(&provider, &url, &pw, cx).detach();
                        }
                    }),
            )
            .into_any_element()
    };

    // Helper to build a labeled text input bound to a settings.json field.
    let make_text_input = |id: &'static str,
                           label: &'static str,
                           help: &'static str,
                           initial: String,
                           placeholder: &'static str| {
        let input = SettingsInputField::new(id)
            .tab_index(0)
            .with_initial_text(initial)
            .with_placeholder(placeholder)
            .aria_label(label)
            .confirm_on_focus_out()
            .on_confirm(move |value, _window, cx| {
                if let Some(text) = value {
                    // Compute the final field value up front so the
                    // `update_settings_file` closure only needs to move
                    // already-owned values into place (it requires `'static`).
                    //
                    // For string fields: `None` when empty, else `Some(text)`.
                    // For numeric fields: parsed `Option<u64>`.
                    // For authorized-emails: split + trimmed `Vec<String>`.
                    let string_value: Option<String> = if text.is_empty() {
                        None
                    } else {
                        Some(text.clone())
                    };
                    let authorized_emails: Option<Vec<String>> =
                        if id == "kask-curator-email-authorized-emails" && !text.is_empty() {
                            Some(
                                text.split(',')
                                    .map(|p| p.trim().to_string())
                                    .filter(|p| !p.is_empty())
                                    .collect(),
                            )
                        } else {
                            None
                        };
                    let inbox_poll: Option<u64> = if id == "kask-curator-email-inbox-poll-interval"
                    {
                        text.parse::<u64>().ok()
                    } else {
                        None
                    };
                    let digest: Option<u64> = if id == "kask-curator-email-digest-interval" {
                        text.parse::<u64>().ok()
                    } else {
                        None
                    };
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            let email = settings
                                .kask
                                .get_or_insert_default()
                                .curator
                                .get_or_insert_default()
                                .email
                                .get_or_insert_default();
                            // Dispatch on `id` to set the right field.
                            match id {
                                "kask-curator-email-mxroute-server" => {
                                    email.mxroute_server = string_value;
                                }
                                "kask-curator-email-smtp-username" => {
                                    email.smtp_username = string_value;
                                }
                                "kask-curator-email-curator-email" => {
                                    email.curator_email = string_value;
                                }
                                "kask-curator-email-alert-email" => {
                                    email.alert_email = string_value;
                                }
                                "kask-curator-email-authorized-emails" => {
                                    email.authorized_emails = authorized_emails;
                                }
                                "kask-curator-email-inbox-poll-interval" => {
                                    email.inbox_poll_interval_secs = inbox_poll;
                                }
                                "kask-curator-email-digest-interval" => {
                                    email.digest_interval_secs = digest;
                                }
                                _ => {}
                            }
                        },
                    );
                }
            });
        v_flex()
            .gap_1()
            .child(Label::new(label))
            .child(Label::new(help).size(LabelSize::Small).color(Color::Muted))
            .child(input)
    };

    // Compute the test email recipient up front, before the strings are
    // moved into `make_text_input` calls.
    let test_email_recipient = if !alert_email.is_empty() {
        alert_email.clone()
    } else {
        smtp_username.clone()
    };
    let test_email_enabled = !test_email_recipient.is_empty();

    let mxroute_input = make_text_input(
        "kask-curator-email-mxroute-server",
        "MXroute Server",
        "MXroute server hostname (e.g. \"tuesday.mxrouting.net\"). Or set HKASK_MXROUTE_SERVER.",
        mxroute_server,
        "tuesday.mxrouting.net",
    );
    let smtp_username_input = make_text_input(
        "kask-curator-email-smtp-username",
        "SMTP Username",
        "Full email address used for SMTP auth and the From header. Or set HKASK_SMTP_USERNAME.",
        smtp_username,
        "curator@example.com",
    );
    let curator_email_input = make_text_input(
        "kask-curator-email-curator-email",
        "From Address",
        "From address (defaults to SMTP Username when empty). Or set HKASK_CURATOR_EMAIL.",
        curator_email,
        "curator@example.com",
    );
    let alert_email_input = make_text_input(
        "kask-curator-email-alert-email",
        "Alert Recipient",
        "Where algedonic alert emails are sent (defaults to SMTP Username when empty). Or set HKASK_ALERT_EMAIL.",
        alert_email,
        "ops@example.com",
    );
    let authorized_input = make_text_input(
        "kask-curator-email-authorized-emails",
        "Authorized Senders",
        "Comma-separated allowlist of senders who may reply with curator commands (P12). Empty means inbound replies are rejected. Or set HKASK_AUTHORIZED_EMAILS.",
        authorized_emails,
        "ops@example.com, alice@example.com",
    );
    let inbox_poll_input = make_text_input(
        "kask-curator-email-inbox-poll-interval",
        "Inbox Poll Interval (secs)",
        "IMAP inbox poll interval for inbound command replies. 0 = disabled. Default 60. Or set HKASK_INBOX_POLL_INTERVAL_SECS.",
        inbox_poll_interval,
        "0",
    );
    let digest_input = make_text_input(
        "kask-curator-email-digest-interval",
        "Digest Interval (secs)",
        "Periodic escalation digest email interval. 0 = disabled. Default 86400 (daily). Or set HKASK_DIGEST_INTERVAL_SECS.",
        digest_interval,
        "0",
    );

    // Test Email button — sends a test email to the alert recipient to verify
    // MXroute credentials. Uses the alert recipient (or SMTP username) as the
    // destination. The send runs on the kask tokio runtime via
    // `kask_bridge::spawn_test_email`; success/failure surfaces in the logs.
    let test_email_button = Button::new("kask-curator-email-test", "Send Test Email")
        .style(ButtonStyle::Outlined)
        .label_size(LabelSize::Small)
        .tab_index(0isize)
        .disabled(!test_email_enabled)
        .on_click(move |_, _, cx| {
            kask_bridge::spawn_test_email(test_email_recipient.clone(), cx);
        });

    v_flex()
        .id("kask-curator-email-page")
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
                .child(SettingsSectionHeader::new("Curator Email"))
                .child(
                    Label::new(
                        "Outbound algedonic alert emails via MXroute. The SMTP password is \
                         stored in the system keychain; non-secret fields live in settings.json. \
                         When unconfigured, the alert sink falls back to log-only.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(mxroute_input)
        .child(Divider::horizontal())
        .child(smtp_username_input)
        .child(Divider::horizontal())
        .child(password_card)
        .child(Divider::horizontal())
        .child(curator_email_input)
        .child(Divider::horizontal())
        .child(alert_email_input)
        .child(Divider::horizontal())
        .child(authorized_input)
        .child(Divider::horizontal())
        .child(inbox_poll_input)
        .child(Divider::horizontal())
        .child(digest_input)
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Test Configuration"))
                .child(
                    Label::new(
                        "Send a test email to the alert recipient to verify MXroute \
                         credentials. Check the logs (reg.email.sent) for the result. \
                         Requires SMTP Username and SMTP Password to be configured.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(test_email_button),
        )
        .into_any_element()
}
