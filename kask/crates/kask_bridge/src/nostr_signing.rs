//! Signing a Nostr note in the editor process, for the Nostr client MCP
//! server (D106).
//!
//! The server never holds the key. It asks over the inference IPC wire
//! (`InferenceMethod::SignNostrNote`); the request is admitted only when the
//! child's grant names `host/sign_nostr_note`; and this module, on the GPUI
//! side, applies the operator's autopublish setting, asks the operator when
//! it is off, and only then reads the two keychain slots, decrypts the key,
//! signs one untagged kind 1 note, wipes, and returns the event.
//!
//! The order is the point: the approval comes before any keychain read, so a
//! declined publish never decrypts the key, and the setting is read where the
//! signature is made, so neither the server nor a tool description can skip
//! the approval.

use std::sync::Arc;

use credentials_provider::CredentialsProvider;
use gpui::{AppContext as _, AsyncApp, PromptLevel};
use hkask_types::inference_ipc::InferenceErrorPayload;
use hkask_types::nostr_wire::MAX_CONTENT_CHARS;
use settings::Settings as _;
use tokio::sync::oneshot;
use zeroize::Zeroizing;

use crate::settings::KaskSettings;

/// The admitted sign request, carried from the tokio dispatch task to the
/// GPUI-side task over a channel (the `WorktreeSpawnRequest` pattern).
pub(crate) struct NostrSignRequest {
    pub(crate) content: String,
    pub(crate) created_at: u64,
    pub(crate) relays: Vec<String>,
    pub(crate) grant: String,
    pub(crate) reply: oneshot::Sender<Result<serde_json::Value, InferenceErrorPayload>>,
}

pub(crate) const NOSTR_SIGN_QUEUE_CAPACITY: usize = 8;

/// The label on the approval's affirmative button.
pub const APPROVE_LABEL: &str = "Publish";
/// The label on the approval's negative button.
pub const DECLINE_LABEL: &str = "Don't publish";

fn refusal(code: &str, message: impl Into<String>) -> InferenceErrorPayload {
    InferenceErrorPayload {
        code: code.into(),
        message: message.into(),
        status: None,
    }
}

/// The checks the tokio dispatch runs before anything is enqueued: the
/// grant and the shape of the note. Pure.
pub(crate) fn admit(
    grant: Option<&str>,
    content: Option<&str>,
    created_at: Option<u64>,
) -> Result<(String, u64), InferenceErrorPayload> {
    if !crate::delegation_grants::grant_holds(grant, crate::delegation_grants::NOSTR_SIGN) {
        return Err(refusal(
            "Auth",
            "Signing a Nostr note requires the parent-granted host/sign_nostr_note authority \
             (kask.mcp.delegated_tools for this server)",
        ));
    }
    let content = content.unwrap_or_default();
    if content.trim().is_empty() {
        return Err(refusal("InvalidArgument", "the note has no text"));
    }
    if content.chars().count() > MAX_CONTENT_CHARS {
        return Err(refusal(
            "InvalidArgument",
            format!("the note is longer than {MAX_CONTENT_CHARS} characters"),
        ));
    }
    let Some(created_at) = created_at else {
        return Err(refusal("InvalidArgument", "the note has no created_at"));
    };
    Ok((content.to_string(), created_at))
}

/// What the operator is shown when autopublish is off. The whole text is in
/// the detail so what is approved is what is signed.
pub(crate) fn approval_text(npub: &str, relays: &[String]) -> String {
    let identity = if npub.is_empty() {
        "the saved key".to_string()
    } else {
        npub.to_string()
    };
    let targets = if relays.is_empty() {
        "no relay".to_string()
    } else {
        relays.join(", ")
    };
    format!("The Curator wants to publish this note as {identity} to {targets}.")
}

/// Whether the operator's answer index on a prompt built with
/// `[APPROVE_LABEL, DECLINE_LABEL]` is an approval. A closed prompt (the
/// window went away) is not.
pub(crate) fn answer_approves<E>(answer: Result<usize, E>) -> bool {
    matches!(answer, Ok(0))
}

/// Decrypt, check and sign. Pure except for the signature's randomness;
/// runs on the background executor because NIP-49 decryption is scrypt at
/// up to 1 GiB. `configured_npub` is `kask.nostr.npub` as set; empty means
/// none, and then the key's own identity is the identity.
pub(crate) fn sign_with_slots(
    ncryptsec: &str,
    password: &str,
    configured_npub: &str,
    content: &str,
    created_at: u64,
) -> Result<serde_json::Value, InferenceErrorPayload> {
    let key = hkask_nostr_signer::decrypt_ncryptsec(ncryptsec, password).map_err(|_| {
        refusal(
            "Auth",
            "the saved key did not open under the saved password",
        )
    })?;
    let npub = hkask_nostr_core::encode_npub(&key.x_only_public_key());
    let configured = configured_npub.trim();
    if !configured.is_empty() && configured != npub {
        return Err(refusal(
            "Auth",
            "the saved key is not the identity set in kask.nostr.npub; nothing was signed",
        ));
    }
    let event =
        hkask_nostr_signer::sign_event(&key, hkask_nostr_signer::NOTE_KIND, created_at, content)
            .map_err(|error| refusal("Generation", format!("signing failed: {error}")))?;
    drop(key);
    serde_json::from_str(&event.to_json()).map_err(|error| {
        refusal(
            "Json",
            format!("the signed event did not serialize: {error}"),
        )
    })
}

/// Handle one admitted request on the GPUI side. Reads the setting, asks
/// the operator if the setting says so, then signs.
pub(crate) async fn handle(request: NostrSignRequest, cx: &mut AsyncApp) {
    if request.reply.is_closed() {
        return;
    }
    let NostrSignRequest {
        content,
        created_at,
        relays,
        grant,
        reply,
    } = request;

    // Re-check the grant at the admission boundary, as the worktree path does.
    if !crate::delegation_grants::grant_holds(Some(&grant), crate::delegation_grants::NOSTR_SIGN) {
        let _ = reply.send(Err(refusal(
            "Auth",
            "the sign grant was revoked before execution",
        )));
        return;
    }

    let (autopublish, configured_npub) = cx.update(|cx| {
        let nostr = &KaskSettings::get_global(cx).nostr;
        (nostr.autopublish, nostr.npub.clone())
    });

    if !autopublish {
        let Some(window) = cx.update(|cx| cx.active_window()) else {
            let _ = reply.send(Err(refusal(
                "Unavailable",
                "autopublish is off and no editor window is open to approve the note",
            )));
            return;
        };
        let message = approval_text(&configured_npub, &relays);
        let answer = cx.update_window(window, |_, window, cx| {
            window.prompt(
                PromptLevel::Warning,
                &message,
                Some(&content),
                &[APPROVE_LABEL, DECLINE_LABEL],
                cx,
            )
        });
        let approved = match answer {
            Ok(receiver) => answer_approves(receiver.await),
            Err(_) => false,
        };
        if !approved {
            let _ = reply.send(Err(refusal(
                "Cancelled",
                "the operator did not approve the note; nothing was signed",
            )));
            return;
        }
    }

    let outcome = sign_after_approval(&content, created_at, &configured_npub, cx).await;
    if reply.send(outcome).is_err() {
        tracing::debug!(
            target: "reg.inference",
            "Nostr sign caller disconnected after approval; the signed event was dropped"
        );
    }
}

/// Read the two slots and sign. Separate from [`handle`] so the test can
/// drive it with the fake credentials provider holding a throwaway key.
pub(crate) async fn sign_after_approval(
    content: &str,
    created_at: u64,
    configured_npub: &str,
    cx: &mut AsyncApp,
) -> Result<serde_json::Value, InferenceErrorPayload> {
    let provider: Arc<dyn CredentialsProvider> =
        cx.update(|cx| zed_credentials_provider::global(cx));
    let read = |url: &'static str| {
        let provider = provider.clone();
        let cx = cx.clone();
        async move {
            match provider.read_credentials(url, &cx).await {
                Ok(Some((_, bytes))) if !bytes.is_empty() => {
                    Ok(Zeroizing::new(String::from_utf8_lossy(&bytes).into_owned()))
                }
                Ok(_) => Err(refusal(
                    "NotConfigured",
                    "no Nostr key is saved; enter it on Settings > Kask > Nostr",
                )),
                Err(error) => Err(refusal(
                    "Unavailable",
                    format!("the keychain could not be read: {error}"),
                )),
            }
        }
    };
    let ncryptsec = read(crate::NOSTR_NCRYPTSEC_CREDENTIAL_URL).await?;
    let password = read(crate::NOSTR_PASSWORD_CREDENTIAL_URL).await?;
    let content = content.to_string();
    let configured_npub = configured_npub.to_string();
    cx.background_executor()
        .spawn(async move {
            sign_with_slots(
                &ncryptsec,
                &password,
                &configured_npub,
                &content,
                created_at,
            )
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway key made at run time from the published NIP-19 example
    /// nsec (specification test data, not anyone's key), encrypted under a
    /// test password at NIP-49's cost.
    const EXAMPLE_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
    const EXAMPLE_NPUB: &str = "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg";

    fn encrypted_example(password: &str) -> String {
        hkask_nostr_signer::encrypt_nsec(EXAMPLE_NSEC, password).expect("encrypts")
    }

    /// expect: nothing is enqueued without the sign grant, with no text, or
    /// with text over the wire bound; a worktree grant does not sign.
    #[test]
    fn admission_refuses_without_the_sign_grant_or_a_note() {
        let server = format!("sign-test-{}", uuid::Uuid::new_v4());
        assert_eq!(admit(None, Some("hi"), Some(1)).unwrap_err().code, "Auth");
        let worktree = crate::delegation_grants::grant_for_server(
            &server,
            &[crate::delegation_grants::WORKTREE_SPAWN.to_string()],
        )
        .expect("grant");
        assert_eq!(
            admit(Some(&worktree), Some("hi"), Some(1))
                .unwrap_err()
                .code,
            "Auth"
        );
        let signing = crate::delegation_grants::grant_for_server(
            &server,
            &[crate::delegation_grants::NOSTR_SIGN.to_string()],
        )
        .expect("grant");
        assert_eq!(
            admit(Some(&signing), Some("  "), Some(1)).unwrap_err().code,
            "InvalidArgument"
        );
        let long = "x".repeat(MAX_CONTENT_CHARS + 1);
        assert_eq!(
            admit(Some(&signing), Some(&long), Some(1))
                .unwrap_err()
                .code,
            "InvalidArgument"
        );
        assert_eq!(
            admit(Some(&signing), Some("hi"), None).unwrap_err().code,
            "InvalidArgument"
        );
        assert_eq!(
            admit(Some(&signing), Some("hi"), Some(1)).expect("admitted"),
            ("hi".to_string(), 1)
        );
        crate::delegation_grants::revoke_delegation_grant(&server);
    }

    /// expect: only the first button approves; a closed prompt does not.
    #[test]
    fn only_the_publish_button_approves() {
        assert!(answer_approves::<()>(Ok(0)));
        assert!(!answer_approves::<()>(Ok(1)));
        let (tx, rx) = futures::channel::oneshot::channel::<usize>();
        drop(tx);
        assert!(!answer_approves(futures::executor::block_on(rx)));
        let text = approval_text(EXAMPLE_NPUB, &["wss://a".into(), "wss://b".into()]);
        assert!(text.contains(EXAMPLE_NPUB));
        assert!(text.contains("wss://a, wss://b"));
        assert!(approval_text("", &[]).contains("no relay"));
    }

    /// expect: with the right password and a matching or empty identity, the
    /// saved slots yield a verified kind 1 event under the key's npub; the
    /// wrong password, a different identity, or a tagged kind are refused and
    /// no refusal carries key material.
    #[test]
    fn the_slots_sign_a_kind_1_note_under_the_saved_identity_or_refuse() {
        let ncryptsec = encrypted_example("correct horse");
        for configured in ["", EXAMPLE_NPUB] {
            let event = sign_with_slots(
                &ncryptsec,
                "correct horse",
                configured,
                "hello",
                1_700_000_000,
            )
            .expect("signs");
            assert_eq!(event["kind"], 1);
            assert_eq!(event["content"], "hello");
            assert_eq!(event["created_at"], 1_700_000_000u64);
            assert_eq!(event["tags"], serde_json::json!([]));
            let parsed = hkask_nostr_core::Event::from_json(&event.to_string())
                .expect("the returned event verifies");
            assert_eq!(hkask_nostr_core::encode_npub(parsed.pubkey()), EXAMPLE_NPUB);
        }
        let wrong = sign_with_slots(&ncryptsec, "wrong", "", "hello", 1).unwrap_err();
        assert_eq!(wrong.code, "Auth");
        let other = sign_with_slots(&ncryptsec, "correct horse", "npub1someoneelse", "hello", 1)
            .unwrap_err();
        assert_eq!(other.code, "Auth");
        for message in [wrong.message, other.message] {
            assert!(!message.contains("nsec1"));
            assert!(!message.contains("ncryptsec1"));
            assert!(!message.contains("correct horse"));
        }
    }

    /// expect: the post-approval path reads exactly the two named slots
    /// through the credentials provider and returns a verified event; with
    /// no key saved it refuses `NotConfigured` and reads nothing else. Runs
    /// against the fake provider, never the keyring.
    #[gpui::test]
    async fn after_approval_the_two_slots_are_read_and_the_note_is_signed(
        cx: &mut gpui::TestAppContext,
    ) {
        let fake = cx.update(zed_credentials_provider::set_fake_credentials_provider);
        let mut async_cx = cx.to_async();

        let missing = sign_after_approval("hello", 1, "", &mut async_cx)
            .await
            .unwrap_err();
        assert_eq!(missing.code, "NotConfigured");
        assert_eq!(
            fake.take_reads(),
            vec![crate::NOSTR_NCRYPTSEC_CREDENTIAL_URL.to_string()]
        );

        {
            let mut state = fake.state.lock();
            state.keys.insert(
                crate::NOSTR_NCRYPTSEC_CREDENTIAL_URL.to_string(),
                encrypted_example("correct horse").into_bytes(),
            );
            state.keys.insert(
                crate::NOSTR_PASSWORD_CREDENTIAL_URL.to_string(),
                b"correct horse".to_vec(),
            );
        }
        let event = sign_after_approval("hello", 1_700_000_000, EXAMPLE_NPUB, &mut async_cx)
            .await
            .expect("signs");
        let parsed = hkask_nostr_core::Event::from_json(&event.to_string()).expect("verifies");
        assert_eq!(parsed.kind(), 1);
        assert_eq!(parsed.content(), "hello");
        assert_eq!(hkask_nostr_core::encode_npub(parsed.pubkey()), EXAMPLE_NPUB);
        let mut reads = fake.take_reads();
        reads.sort();
        assert_eq!(
            reads,
            vec![
                crate::NOSTR_NCRYPTSEC_CREDENTIAL_URL.to_string(),
                crate::NOSTR_PASSWORD_CREDENTIAL_URL.to_string(),
            ]
        );
    }
}
