//! Nostr sub-page — the client's identity and relays (`kask.nostr.npub`,
//! `kask.nostr.seed_relays`), the autopublish switch
//! (`kask.nostr.autopublish`), and the local relay server's policy
//! (`kask.nostr.relay`).
//!
//! The key block is the one place the secret key is entered (operator
//! rulings 2026-11-12 and 2026-11-13: the nsec and a password are saved in
//! the Nostr settings). Neither goes to `settings.json`: the editor encrypts
//! the nsec under the password (NIP-49) and stores the `ncryptsec` and the
//! password in two keychain slots named in `kask_bridge`, then writes the
//! derived public `npub` to `kask.nostr.npub`. The relay verifies signatures
//! and never signs, and the client server receives no credential.
//!
//! The rows write what the operator types. They do not judge whether an
//! `npub`, an author key or a relay address is usable: each server does that
//! when it starts and reports what it refused (`nostr_account`,
//! `nostr_relay_status`), so there is one place that decides.
//!
//! Autopublish is off by default (operator ruling 2026-11-13). The toggle
//! writes the setting; the switch is read where the signature is made, in
//! the editor, never by a server.

use super::*;
use editor::Editor;
use gpui::{Entity, Global};

/// What the key block is doing, kept across frames. The nsec and the
/// password are never in here: they live in the two masked editors until
/// Save clears them, and in the save task until it drops them.
#[derive(Default)]
struct NostrKeyState {
    /// Whether each slot was found in the keychain, once the presence read
    /// has answered. `None` until it has.
    presence: Option<(bool, bool)>,
    presence_read_started: bool,
    saving: bool,
    error: Option<String>,
}
impl Global for NostrKeyState {}

/// Why a key was not saved. No variant repeats any part of what was typed.
#[derive(Debug, PartialEq, Eq)]
enum KeySaveRefusal {
    EmptyNsec,
    EmptyPassword,
    NotAnNsec,
    /// `kask.nostr.npub` is set and is not this key's. The identity is
    /// cleared on the page first, then the key saved; a silent overwrite
    /// would change which account the Curator publishes as.
    DifferentIdentity,
    Encryption,
}

impl KeySaveRefusal {
    fn message(&self) -> &'static str {
        match self {
            Self::EmptyNsec => "Enter the nsec.",
            Self::EmptyPassword => "Enter a password to encrypt the key with.",
            Self::NotAnNsec => "That is not an nsec. Nothing was saved.",
            Self::DifferentIdentity => {
                "This key belongs to a different npub than the one set above. Clear \
                 the Identity row first if you mean to change accounts. Nothing was saved."
            }
            Self::Encryption => "The key could not be encrypted. Nothing was saved.",
        }
    }
}

/// The two things a save writes, built before anything is written: the
/// encrypted key for the keychain and the public identity for settings.
struct PreparedKey {
    ncryptsec: String,
    npub: String,
}

/// Parse, check and encrypt. Pure, so it is tested without a window or a
/// keychain. `configured_npub` is the Identity row as set; empty means
/// none. Runs scrypt at NIP-49's 256 MiB cost, so callers run it off the
/// main thread.
fn prepare_key(
    nsec: &str,
    password: &str,
    configured_npub: &str,
) -> Result<PreparedKey, KeySaveRefusal> {
    let nsec = nsec.trim();
    if nsec.is_empty() {
        return Err(KeySaveRefusal::EmptyNsec);
    }
    if password.is_empty() {
        return Err(KeySaveRefusal::EmptyPassword);
    }
    let key =
        hkask_nostr_signer::SecretKey::from_nsec(nsec).map_err(|_| KeySaveRefusal::NotAnNsec)?;
    let npub = hkask_nostr_core::encode_npub(&key.x_only_public_key());
    drop(key);
    let configured = configured_npub.trim();
    if !configured.is_empty() && configured != npub {
        return Err(KeySaveRefusal::DifferentIdentity);
    }
    let ncryptsec =
        hkask_nostr_signer::encrypt_nsec(nsec, password).map_err(|_| KeySaveRefusal::Encryption)?;
    Ok(PreparedKey { ncryptsec, npub })
}

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

/// Parse the npub row. The text is written trimmed and as typed; the client
/// server decides whether it is an `npub` and reports a value it cannot
/// read, without repeating it. Empty clears the identity.
fn parse_npub_row(text: &str) -> Option<String> {
    Some(text.trim().to_string())
}

/// Read whether each key slot is present, once, off the main thread. Its
/// own read rather than `ensure_keychain_prefetch`, which walks the
/// data-service registry the Nostr slots are deliberately not in.
fn ensure_key_presence_read(cx: &mut App) {
    if !cx.has_global::<NostrKeyState>() {
        cx.set_global(NostrKeyState::default());
    }
    if cx.global::<NostrKeyState>().presence_read_started {
        return;
    }
    cx.global_mut::<NostrKeyState>().presence_read_started = true;
    cx.spawn(async move |cx| {
        async fn present(url: &str) -> bool {
            hkask_keystore::Keychain
                .retrieve_by_url_async(url)
                .await
                .is_ok_and(|secret| !secret.is_empty())
        }
        let ncryptsec = present(kask_bridge::NOSTR_NCRYPTSEC_CREDENTIAL_URL).await;
        let password = present(kask_bridge::NOSTR_PASSWORD_CREDENTIAL_URL).await;
        cx.update(|cx| {
            cx.global_mut::<NostrKeyState>().presence = Some((ncryptsec, password));
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

/// Save: prepare off the main thread, then write the two slots, then the
/// derived npub, then clear the fields. A refusal writes nothing and names
/// itself without repeating what was typed.
fn save_key(
    nsec_editor: Entity<Editor>,
    password_editor: Entity<Editor>,
    configured_npub: String,
    window: &mut Window,
    cx: &mut App,
) {
    let nsec = zeroize::Zeroizing::new(nsec_editor.read(cx).text(cx));
    let password = zeroize::Zeroizing::new(password_editor.read(cx).text(cx));
    {
        let state = cx.global_mut::<NostrKeyState>();
        state.saving = true;
        state.error = None;
    }
    let provider = zed_credentials::global(cx);
    let prepared =
        cx.background_spawn(async move { prepare_key(&nsec, &password, &configured_npub) });
    window
        .spawn(cx, async move |cx| {
            let outcome = prepared.await;
            let prepared = match outcome {
                Ok(prepared) => prepared,
                Err(refusal) => {
                    cx.update(|_, cx| {
                        let state = cx.global_mut::<NostrKeyState>();
                        state.saving = false;
                        state.error = Some(refusal.message().to_string());
                    })
                    .ok();
                    return;
                }
            };
            // The password is written from the editor, not from `prepared`:
            // `prepare_key` does not return it.
            let password = cx
                .update(|_, cx| zeroize::Zeroizing::new(password_editor.read(cx).text(cx)))
                .unwrap_or_default();
            let writes = cx.update(|_, cx| {
                (
                    write_credential(
                        &provider,
                        kask_bridge::NOSTR_NCRYPTSEC_CREDENTIAL_URL,
                        &prepared.ncryptsec,
                        cx,
                    ),
                    write_credential(
                        &provider,
                        kask_bridge::NOSTR_PASSWORD_CREDENTIAL_URL,
                        &password,
                        cx,
                    ),
                )
            });
            let Ok((write_key, write_password)) = writes else {
                return;
            };
            write_key.await;
            write_password.await;
            let npub = prepared.npub;
            cx.update(|window, cx| {
                SettingsStore::global(cx).update_settings_file(
                    <dyn fs::Fs>::global(cx),
                    move |settings, _| {
                        nostr_content(settings).npub = Some(npub);
                    },
                );
                nsec_editor.update(cx, |editor, cx| editor.set_text("", window, cx));
                password_editor.update(cx, |editor, cx| editor.set_text("", window, cx));
                let state = cx.global_mut::<NostrKeyState>();
                state.saving = false;
                state.presence = Some((true, true));
                cx.refresh_windows();
            })
            .ok();
        })
        .detach();
}

/// Reset: remove both slots and clear the identity the key derived.
fn reset_key(cx: &mut App) {
    let provider = zed_credentials::global(cx);
    let delete_key = delete_credential(&provider, kask_bridge::NOSTR_NCRYPTSEC_CREDENTIAL_URL, cx);
    let delete_password =
        delete_credential(&provider, kask_bridge::NOSTR_PASSWORD_CREDENTIAL_URL, cx);
    cx.spawn(async move |cx| {
        delete_key.await;
        delete_password.await;
        cx.update(|cx| {
            SettingsStore::global(cx).update_settings_file(
                <dyn fs::Fs>::global(cx),
                |settings, _| {
                    nostr_content(settings).npub = Some(String::new());
                },
            );
            let state = cx.global_mut::<NostrKeyState>();
            state.presence = Some((false, false));
            state.error = None;
            cx.refresh_windows();
        });
    })
    .detach();
}

fn render_key_block(
    configured_npub: String,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    ensure_key_presence_read(cx);
    let (presence, saving, error) = {
        let state = cx.global::<NostrKeyState>();
        (state.presence, state.saving, state.error.clone())
    };

    let body: AnyElement = match presence {
        None => Label::new("Checking the keychain…")
            .size(LabelSize::Small)
            .color(Color::Muted)
            .into_any_element(),
        Some((true, true)) => {
            let for_whom = if configured_npub.is_empty() {
                "Key Saved".to_string()
            } else {
                format!("Key Saved for {configured_npub}")
            };
            ConfiguredApiCard::new("kask-nostr-key-reset", for_whom)
                .button_label("Reset Key")
                .button_tab_index(0)
                .on_click(|_, _, cx| reset_key(cx))
                .into_any_element()
        }
        Some(partial) => {
            let nsec_editor = masked_input("kask-nostr-nsec", "nsec1…", window, cx);
            let password_editor =
                masked_input("kask-nostr-password", "encryption password", window, cx);
            let mut block = v_flex().gap_2();
            if partial != (false, false) {
                block = block.child(
                    Label::new(
                        "One of the two keychain entries is missing. Saving writes both; \
                         Reset Key removes what is there.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Warning),
                );
            }
            block
                .child(
                    v_flex()
                        .gap_0p5()
                        .child(Label::new("Secret key (nsec)").size(LabelSize::Small))
                        .child(nsec_editor.clone()),
                )
                .child(
                    v_flex()
                        .gap_0p5()
                        .child(Label::new("Encryption password").size(LabelSize::Small))
                        .child(password_editor.clone()),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new(
                                "kask-nostr-key-save",
                                if saving { "Encrypting…" } else { "Save Key" },
                            )
                            .disabled(saving)
                            .on_click({
                                move |_, window, cx| {
                                    save_key(
                                        nsec_editor.clone(),
                                        password_editor.clone(),
                                        configured_npub.clone(),
                                        window,
                                        cx,
                                    )
                                }
                            }),
                        )
                        .when(partial != (false, false), |this| {
                            this.child(
                                Button::new("kask-nostr-key-reset-partial", "Reset Key")
                                    .on_click(|_, _, cx| reset_key(cx)),
                            )
                        }),
                )
                .when_some(error, |this, error| {
                    this.child(Label::new(error).size(LabelSize::Small).color(Color::Error))
                })
                .into_any_element()
        }
    };

    v_flex()
        .gap_1()
        .child(Label::new("Key"))
        .child(
            Label::new(
                "The secret key the Curator publishes with, and a password of your \
                 choosing. The key is encrypted under the password (NIP-49) and both \
                 are stored in the system keychain; nothing is written to \
                 settings.json except the public npub derived from the key, which \
                 fills the Identity row. No MCP server receives either. A Nostr key \
                 cannot be rotated: whoever holds it is this account.",
            )
            .size(LabelSize::Small)
            .color(Color::Muted),
        )
        .child(body)
        .into_any_element()
}

fn nostr_content(
    settings: &mut settings::SettingsContent,
) -> &mut settings::KaskNostrSettingsContent {
    settings
        .kask
        .get_or_insert_default()
        .nostr
        .get_or_insert_default()
}

fn relay_content(
    settings: &mut settings::SettingsContent,
) -> &mut settings::KaskNostrRelaySettingsContent {
    nostr_content(settings).relay.get_or_insert_default()
}

pub(crate) fn render_nostr_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the page shows what the servers receive.
    let nostr: kask_bridge::KaskNostrSettings = raw
        .and_then(|c| c.nostr)
        .map(Into::into)
        .unwrap_or_default();
    let relay = nostr.relay.clone();

    let npub_input = kask_input_field(
        "kask-nostr-npub",
        0,
        nostr.npub.clone(),
        "npub1…",
        "Identity",
        parse_npub_row,
        |parsed, settings| {
            nostr_content(settings).npub = Some(parsed);
        },
    );

    let seed_relays_input = kask_input_field(
        "kask-nostr-seed-relays",
        0,
        nostr.seed_relays.join(", "),
        "wss://relay.example, wss://…",
        "Seed Relays",
        parse_list_row,
        |parsed, settings| {
            nostr_content(settings).seed_relays = Some(parsed);
        },
    );

    let autopublish_toggle = kask_toggle(
        "kask-nostr-autopublish",
        Some("Autopublish"),
        Some(
            "Off: when the Curator publishes, the editor shows the note, the \
             signing identity and the target relays, and signs only when you \
             approve. On: that step is removed and the Curator publishes \
             directly. Publishing is not built yet; this switch is read where \
             the signature is made."
                .into(),
        ),
        nostr.autopublish,
        0,
        |enabled, settings| {
            nostr_content(settings).autopublish = Some(enabled);
        },
    );

    let key_block = render_key_block(nostr.npub, window, cx);

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
                .child(SettingsSectionHeader::new("Nostr Account"))
                .child(
                    Label::new(
                        "The identity the Nostr client server reads for, and the public \
                         relays it reads from. Public data only: the secret key is never \
                         entered here. The client server reports in nostr_account whether \
                         the identity is usable and which relays answer. Only you set \
                         these; no agent tool can change them.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Identity"))
                .child(
                    Label::new(
                        "Your public key as an npub. Never an nsec: a secret key typed \
                         here is reported as invalid and is not used. Empty: no identity, \
                         and nostr_notifications has nothing to read for.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(npub_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Seed Relays"))
                .child(
                    Label::new(
                        "Public relays the client reads from, separated by commas; the \
                         first eight are used. Each must start with wss:// and be a \
                         public host. Once set, every nostr_account call opens one \
                         connection to each. The local relay is always read as well.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(seed_relays_input),
        )
        .child(Divider::horizontal())
        .child(key_block)
        .child(Divider::horizontal())
        .child(autopublish_toggle)
        .child(Divider::horizontal())
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

    /// expect: the identity row writes the text trimmed and as typed, and an
    /// empty confirm clears it (how the operator removes the identity).
    #[test]
    fn the_npub_row_trims_and_an_empty_row_clears() {
        assert_eq!(
            parse_npub_row("  npub-as-typed  "),
            Some("npub-as-typed".to_string())
        );
        assert_eq!(parse_npub_row(""), Some(String::new()));
    }

    /// expect: the account rows and the toggle write under `kask.nostr`,
    /// beside and not inside the relay section, and leave it untouched.
    #[test]
    fn account_edits_land_in_kask_nostr_beside_the_relay_section() {
        let mut settings = settings::SettingsContent::default();
        relay_content(&mut settings).port = Some(7447);
        nostr_content(&mut settings).npub = Some("npub-as-typed".to_string());
        nostr_content(&mut settings).seed_relays = Some(vec!["wss://a".to_string()]);
        nostr_content(&mut settings).autopublish = Some(true);
        let nostr = settings
            .kask
            .and_then(|kask| kask.nostr)
            .expect("kask.nostr written");
        assert_eq!(nostr.npub.as_deref(), Some("npub-as-typed"));
        assert_eq!(nostr.seed_relays, Some(vec!["wss://a".to_string()]));
        assert_eq!(nostr.autopublish, Some(true));
        assert_eq!(nostr.relay.and_then(|relay| relay.port), Some(7447));
    }

    /// expect: a settings file that never mentions autopublish shows the
    /// toggle off. The resolved value the page renders is the bridge's, and
    /// its default is `false` (operator ruling 2026-11-13).
    #[test]
    fn autopublish_renders_off_when_the_file_does_not_mention_it() {
        let nostr: kask_bridge::KaskNostrSettings =
            settings::KaskNostrSettingsContent::default().into();
        assert!(!nostr.autopublish);
    }

    /// NIP-19's published example secret key and the npub it lists for it.
    /// Test data from the specification, the same the signer's tests use;
    /// not anyone's key.
    const NIP19_EXAMPLE_NSEC: &str =
        "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
    const NIP19_EXAMPLE_NPUB: &str =
        "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg";

    /// expect: a save is refused, writing nothing, when a field is empty,
    /// when the text is not an nsec, or when the key is not the configured
    /// identity's. No refusal message carries what was typed.
    #[test]
    fn a_key_save_is_refused_for_each_named_reason() {
        assert_eq!(
            prepare_key("", "pw", "").err(),
            Some(KeySaveRefusal::EmptyNsec)
        );
        assert_eq!(
            prepare_key(NIP19_EXAMPLE_NSEC, "", "").err(),
            Some(KeySaveRefusal::EmptyPassword)
        );
        assert_eq!(
            prepare_key("npub1notasecretkey", "pw", "").err(),
            Some(KeySaveRefusal::NotAnNsec)
        );
        assert_eq!(
            prepare_key(NIP19_EXAMPLE_NSEC, "pw", "npub1someoneelse").err(),
            Some(KeySaveRefusal::DifferentIdentity)
        );
        for refusal in [
            KeySaveRefusal::EmptyNsec,
            KeySaveRefusal::EmptyPassword,
            KeySaveRefusal::NotAnNsec,
            KeySaveRefusal::DifferentIdentity,
            KeySaveRefusal::Encryption,
        ] {
            assert!(!refusal.message().contains("nsec1"));
            assert!(!refusal.message().contains("pw"));
        }
    }

    /// expect: a usable key with no identity configured, or with its own
    /// identity configured, prepares an `ncryptsec` that opens under the
    /// password to the key that was typed, and the npub NIP-19 publishes.
    /// Runs scrypt at 256 MiB twice; the workspace builds scrypt at
    /// opt-level 3 so this stays short.
    #[test]
    fn a_usable_key_prepares_its_encrypted_form_and_its_npub() {
        for configured in ["", NIP19_EXAMPLE_NPUB, &format!("  {NIP19_EXAMPLE_NPUB} ")] {
            let prepared = prepare_key(NIP19_EXAMPLE_NSEC, "correct horse", configured)
                .expect("the published example prepares");
            assert_eq!(prepared.npub, NIP19_EXAMPLE_NPUB);
            assert!(prepared.ncryptsec.starts_with("ncryptsec1"));
            let opened =
                hkask_nostr_signer::decrypt_ncryptsec(&prepared.ncryptsec, "correct horse")
                    .expect("opens under the password");
            assert_eq!(
                hkask_nostr_core::encode_npub(&opened.x_only_public_key()),
                NIP19_EXAMPLE_NPUB
            );
            assert!(hkask_nostr_signer::decrypt_ncryptsec(&prepared.ncryptsec, "wrong").is_err());
        }
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
