//! Nostr sub-page — the local relay server's policy (`kask.nostr.relay`).
//!
//! Public data only. No key is entered or shown here: the relay verifies
//! signatures and never signs.
//!
//! The rows write what the operator types. They do not judge whether an
//! author key or a relay address is usable: the relay server does that when
//! it starts and reports what it refused in `nostr_relay_status`, so there is
//! one place that decides.

use super::*;

fn split_list(text: &str) -> Vec<String> {
    text.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect()
}

/// Parse a list row. Unlike the private-thread server list, an empty confirm
/// IS a write here: clearing the allowlist is how the operator stops the
/// relay admitting anything, and it must take effect.
fn parse_list_row(text: &str) -> Option<Vec<String>> {
    Some(split_list(text))
}

/// Parse the port row. Empty means "pick a free port" and is written as 0.
/// Anything that is not a port number writes nothing.
fn parse_port_row(text: &str) -> Option<u16> {
    let text = text.trim();
    if text.is_empty() {
        return Some(0);
    }
    text.parse::<u16>().ok()
}

fn relay_content(
    settings: &mut settings::SettingsContent,
) -> &mut settings::KaskNostrRelaySettingsContent {
    settings
        .kask
        .get_or_insert_default()
        .nostr
        .get_or_insert_default()
        .relay
        .get_or_insert_default()
}

pub(crate) fn render_nostr_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the page shows what the relay server receives.
    let relay: kask_bridge::KaskNostrRelaySettings = raw
        .and_then(|c| c.nostr)
        .and_then(|c| c.relay)
        .map(Into::into)
        .unwrap_or_default();

    let authors_input = kask_input_field(
        "kask-nostr-relay-allowed-authors",
        0,
        relay.allowed_authors.join(", "),
        "npub1…, npub1…",
        "Archived Authors",
        parse_list_row,
        |parsed, settings| {
            relay_content(settings).allowed_authors = Some(parsed);
        },
    );

    let sync_relays_input = kask_input_field(
        "kask-nostr-relay-sync-relays",
        0,
        relay.sync_relays.join(", "),
        "wss://relay.example, wss://…",
        "Sync Relays",
        parse_list_row,
        |parsed, settings| {
            relay_content(settings).sync_relays = Some(parsed);
        },
    );

    let port_input = kask_input_field(
        "kask-nostr-relay-port",
        0,
        if relay.port > 0 {
            relay.port.to_string()
        } else {
            String::new()
        },
        "free port at each start",
        "Relay Port",
        parse_port_row,
        |parsed, settings| {
            relay_content(settings).port = Some(parsed);
        },
    );

    v_flex()
        .id("kask-nostr-page")
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
                .child(SettingsSectionHeader::new("Local Nostr Relay"))
                .child(
                    Label::new(
                        "The relay server keeps a local archive of public Nostr events by \
                         the authors listed here. It runs while the editor runs, holds no \
                         key, and signs nothing. Only you set this policy: no agent tool \
                         can change it. A change here restarts the relay server.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(
                    Label::new(
                        "The archive is not private. Any program on this computer can \
                         read it, and it is not encrypted on disk, so the relay refuses \
                         direct messages and stores public events only.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Archived Authors"))
                .child(
                    Label::new(
                        "Whose events the relay stores, as npubs or 64-character hex \
                         public keys, separated by commas. Empty: the relay stores \
                         nothing. An entry the relay cannot read as a key is left out \
                         and named in nostr_relay_status.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(authors_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Sync Relays"))
                .child(
                    Label::new(
                        "Public relays the relay copies those authors' events from when \
                         asked to sync, separated by commas. Each must start with wss:// \
                         and be a public host. An address the relay will not use is left \
                         out and named in nostr_relay_status.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(sync_relays_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Relay Port"))
                .child(
                    Label::new(
                        "The port on this computer (127.0.0.1) that other Nostr apps \
                         connect to. Empty: a free port is picked each time the relay \
                         starts, so its address changes. Set a port to keep it stable.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(port_input),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_row_splits_on_commas_and_whitespace() {
        assert_eq!(
            parse_list_row("npub1a, npub1b\n npub1c"),
            Some(vec![
                "npub1a".to_string(),
                "npub1b".to_string(),
                "npub1c".to_string()
            ])
        );
    }

    /// Clearing the row writes an empty list. That is the operator's way to
    /// stop the relay admitting anything; a no-op here would leave the old
    /// allowlist in force.
    #[test]
    fn an_empty_list_row_is_written_as_an_empty_list() {
        assert_eq!(parse_list_row(""), Some(Vec::new()));
        assert_eq!(parse_list_row("  ,  "), Some(Vec::new()));
    }

    #[test]
    fn the_port_row_accepts_a_port_or_empty_and_nothing_else() {
        assert_eq!(parse_port_row("7447"), Some(7447));
        assert_eq!(parse_port_row(" 7447 "), Some(7447));
        assert_eq!(parse_port_row(""), Some(0));
        assert_eq!(parse_port_row("70000"), None);
        assert_eq!(parse_port_row("abc"), None);
        assert_eq!(parse_port_row("-1"), None);
    }

    #[test]
    fn a_row_edit_lands_in_kask_nostr_relay() {
        let mut settings = settings::SettingsContent::default();
        relay_content(&mut settings).allowed_authors = Some(vec!["npub1a".to_string()]);
        relay_content(&mut settings).port = Some(7447);
        let relay = settings
            .kask
            .and_then(|kask| kask.nostr)
            .and_then(|nostr| nostr.relay)
            .expect("kask.nostr.relay written");
        assert_eq!(relay.allowed_authors, Some(vec!["npub1a".to_string()]));
        assert_eq!(relay.port, Some(7447));
        assert_eq!(relay.sync_relays, None);
    }
}
