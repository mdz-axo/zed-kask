//! Credential namespace constants and startup key purges.
//!
//! The `kask://credentials` prefix is the single source of truth for
//! kask-namespaced keychain entries. Every credential URL written by kask
//! (data-service API keys, the mirrored DB passphrase, etc.) is built by
//! formatting this constant with a per-credential suffix. Keeping the constant
//! here — rather than re-declaring it in each consumer — prevents the silent
//! drift that occurred when the former private duplicate in
//! `inference_providers` diverged from the crate-root value.

/// The URL prefix for kask-namespaced credentials in the keychain.
/// Used by the settings UI to read/write API keys via zed's CredentialsProvider.
pub const KASK_CREDENTIAL_NAMESPACE: &str = "kask://credentials";

/// The keychain slot for the operator's Nostr secret key, encrypted under
/// their password as a NIP-49 `ncryptsec` (operator rulings 2026-11-12 and
/// 2026-11-13: the nsec and the password are saved in the Nostr settings;
/// D5: the keychain, never `settings.json`). Written by the Nostr settings
/// page, read by the editor's signer and by nothing else: the slot is not a
/// `DataServiceDescriptor`, so `credential_urls_for_mcp` cannot deliver it
/// to a child process (`nostr_key_slots_reach_no_mcp_child`).
pub const NOSTR_NCRYPTSEC_CREDENTIAL_URL: &str = "kask://credentials/nostr_ncryptsec";

/// The keychain slot for the password that opens
/// [`NOSTR_NCRYPTSEC_CREDENTIAL_URL`]. A separate slot, so the encrypted key
/// and what opens it are separable entries and a reset removes both by name.
pub const NOSTR_PASSWORD_CREDENTIAL_URL: &str = "kask://credentials/nostr_password";

/// One-time startup purge of the removed ABW API key (ABW-removal plan
/// Decision A; recorded in DIVERGENCE.md D5).
///
/// The ABW cloud swarm integration was removed; its keychain entry
/// `kask://credentials/hkask_abw_api_key` is a dormant third-party secret
/// with no remaining read path or management surface — worse to keep than
/// to delete once, loudly. Safe to run on every startup:
/// `delete_by_url_async` on an absent entry is a no-op, so the steady state
/// logs at debug and the actual purge logs at info. A failed delete of a
/// live key warns naming manual remediation.
pub fn purge_removed_abw_api_key(cx: &gpui::App) {
    use gpui::AppContext as _;

    let url = format!("{KASK_CREDENTIAL_NAMESPACE}/hkask_abw_api_key");
    cx.background_spawn(async move {
        let keychain = hkask_keystore::keychain::Keychain;
        match keychain.retrieve_by_url_async(&url).await {
            Ok(_) => match keychain.delete_by_url_async(&url).await {
                Ok(()) => tracing::info!(
                    target: "reg.keystore",
                    "Purged the removed ABW API key at {url} — the ABW cloud swarm \
                     integration no longer exists; the orphaned third-party secret \
                     is deleted (D5, ABW-removal plan Decision A)"
                ),
                Err(e) => tracing::warn!(
                    target: "reg.keystore",
                    "Found the removed ABW API key at {url} but the purge delete \
                     failed: {e} — delete the keychain entry manually"
                ),
            },
            Err(hkask_keystore::KeychainError::NotFound(_)) => {
                tracing::debug!(
                    target: "reg.keystore",
                    "No ABW API key at {url} — keychain already clean (the ABW \
                     integration is removed)"
                );
            }
            Err(e) => tracing::warn!(
                target: "reg.keystore",
                "Could not check for the removed ABW API key at {url}: {e} — the \
                 key may still exist; delete it manually if present"
            ),
        }
    })
    .detach();
}

/// One-time startup purge of the removed Reduct.cloud API key
/// (Reduct-removal plan, Decision A mirrored from the ABW precedent;
/// recorded in DIVERGENCE.md D5/D9).
///
/// The Reduct.cloud integration was removed from the media MCP server
/// (2026-10-22); its keychain entry `kask://credentials/reduct_api_key` is a
/// dormant third-party secret with no remaining read path or management
/// surface — worse to keep than to delete once, loudly. Safe to run on every
/// startup: `delete_by_url_async` on an absent entry is a no-op, so the steady
/// state logs at debug and the actual purge logs at info. A failed delete of a
/// live key warns naming manual remediation.
pub fn purge_removed_reduct_api_key(cx: &gpui::App) {
    use gpui::AppContext as _;

    let url = format!("{KASK_CREDENTIAL_NAMESPACE}/reduct_api_key");
    cx.background_spawn(async move {
        let keychain = hkask_keystore::keychain::Keychain;
        match keychain.retrieve_by_url_async(&url).await {
            Ok(_) => match keychain.delete_by_url_async(&url).await {
                Ok(()) => tracing::info!(
                    target: "reg.keystore",
                    "Purged the removed Reduct.cloud API key at {url} — the media \
                     server's Reduct cloud connection no longer exists; the orphaned \
                     third-party secret is deleted (D5/D9, Reduct-removal plan)"
                ),
                Err(e) => tracing::warn!(
                    target: "reg.keystore",
                    "Found the removed Reduct.cloud API key at {url} but the purge \
                     delete failed: {e} — delete the keychain entry manually"
                ),
            },
            Err(hkask_keystore::KeychainError::NotFound(_)) => {
                tracing::debug!(
                    target: "reg.keystore",
                    "No Reduct.cloud API key at {url} — keychain already clean (the \
                     Reduct.cloud integration is removed)"
                );
            }
            Err(e) => tracing::warn!(
                target: "reg.keystore",
                "Could not check for the removed Reduct.cloud API key at {url}: {e} — \
                 the key may still exist; delete it manually if present"
            ),
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: the two Nostr key slots are in the kask namespace and are
    /// distinct, so a reset can remove each by name.
    #[test]
    fn nostr_key_slots_are_two_named_entries_in_the_namespace() {
        for url in [
            NOSTR_NCRYPTSEC_CREDENTIAL_URL,
            NOSTR_PASSWORD_CREDENTIAL_URL,
        ] {
            assert!(url.starts_with(KASK_CREDENTIAL_NAMESPACE), "{url}");
        }
        assert_ne!(
            NOSTR_NCRYPTSEC_CREDENTIAL_URL,
            NOSTR_PASSWORD_CREDENTIAL_URL
        );
    }

    /// expect: neither slot is a credential any MCP child is given. The
    /// child-delivery set is built from `DATA_SERVICES` and the inference
    /// providers; the Nostr key is in neither registry, by design, so the
    /// key reaches only the editor process that signs.
    #[test]
    fn nostr_key_slots_reach_no_mcp_child() {
        let delivered = crate::credential_urls_for_mcp();
        for url in [
            NOSTR_NCRYPTSEC_CREDENTIAL_URL,
            NOSTR_PASSWORD_CREDENTIAL_URL,
        ] {
            assert!(
                !delivered.iter().any(|(_, slot)| slot == url),
                "{url} is delivered to a child"
            );
        }
        let prefetched = crate::data_service_credential_urls();
        for url in [
            NOSTR_NCRYPTSEC_CREDENTIAL_URL,
            NOSTR_PASSWORD_CREDENTIAL_URL,
        ] {
            assert!(
                !prefetched.iter().any(|(_, slot)| slot == url),
                "{url} is in the data-service registry"
            );
        }
    }
}
