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
    ARTICLE_KIND, ArticleDraft, BLOSSOM_AUTH_KIND, BOOKMARK_LIST_KIND, COMMENT_KIND, DELETION_KIND,
    FOLLOWS_KIND, GENERIC_REPOST_KIND, ImetaAttachment, MAX_ARTICLE_CHARS, MAX_BLOSSOM_SERVERS,
    MAX_DELETION_TARGETS, MAX_FOLLOWS, MAX_IMETA_ATTACHMENTS, MAX_LIST_ITEMS, MUTE_LIST_KIND,
    NOTE_KIND, NUTZAP_KIND, QuoteTarget, REACTION_KIND, REPOST_KIND, ReplyTarget, SignError,
    ZAP_REQUEST_KIND, sign_article, sign_blossom_upload_auth, sign_deletion, sign_event,
    sign_follows, sign_list, sign_note_with_imeta, sign_note_with_imeta_and_quote, sign_nutzap,
    sign_reaction, sign_reply, sign_repost, sign_zap_request,
};
