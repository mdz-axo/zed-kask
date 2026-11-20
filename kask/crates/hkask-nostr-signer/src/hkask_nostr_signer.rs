#![forbid(unsafe_code)]
//! Secret Nostr operations: everything here reads, derives or uses the
//! identity key.
//!
//! A Nostr key cannot be rotated or recovered, so only the editor process may
//! depend on this crate. No MCP server links it; the server's side of the
//! protocol lives in `hkask-nostr-core`.
//!
//! What "wiped" means here: the buffers this crate owns (the decrypted key,
//! the derived symmetric key, the normalized password) are zeroed when they
//! are dropped. Temporaries inside the dependencies are outside its control.

mod nip49;
mod secret;
mod sign;

pub use nip49::{
    DecryptError, ENCRYPT_LOG_N, EncryptError, MAX_LOG_N, decrypt_ncryptsec, encrypt_nsec,
};
pub use secret::{NsecError, SecretKey};
pub use sign::{
    BLOSSOM_AUTH_KIND, COMMENT_KIND, ImetaAttachment, MAX_BLOSSOM_SERVERS, MAX_IMETA_ATTACHMENTS,
    NOTE_KIND, NUTZAP_KIND, REACTION_KIND, ReplyTarget, SignError, ZAP_REQUEST_KIND,
    sign_blossom_upload_auth, sign_event, sign_note_with_imeta, sign_nutzap, sign_reaction,
    sign_reply, sign_zap_request,
};
