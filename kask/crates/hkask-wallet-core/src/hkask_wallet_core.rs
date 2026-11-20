#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! Core types, validators, and spend-cap policy for D110 (`hkask-mcp-wallet`).
//!
//! This crate isolates `lightning-invoice` (which depends on `secp256k1`) and
//! BIP-39 / spend-policy logic from `hkask-nostr-core` and `hkask-nostr-signer`.
//!
//! Security invariants enforced here:
//! - T2: All secret buffers are wrapped in `zeroize::Zeroizing`; error variants
//!   never echo raw secrets, mnemonics, NWC URIs, or invoices.
//! - T5: Strict public-HTTPS URL and resolved-IP validation (loopback allowed
//!   only in `regtest` or under `test-fixture`).
//! - T6: Full BOLT11 parsing and validation (network, non-zero exact amount,
//!   expiry, optional NIP-57 `description_hash`).
//! - T7: Checked arithmetic on `Satoshis` and `Millisatoshis` newtypes.

pub mod amount;
pub mod bolt11;
pub mod lightning;
pub mod network;
pub mod policy;
pub mod secrets;
pub mod url_guard;

pub use amount::{AmountError, Millisatoshis, Satoshis};
pub use bolt11::{Bolt11Error, DecodedBolt11, decode_bolt11, validate_bolt11_for_payment};
pub use lightning::{
    CreatedLightningInvoice, LightningError, LightningInvoiceLookup, LightningInvoiceState,
    LightningPaymentResult, LightningRail, verify_preimage_matches_payment_hash,
};
pub use network::{PaymentRail, WalletNetwork};
pub use policy::{
    ApprovalReceipt, PolicyRefusal, ROLLING_WINDOW_SECS, RollingSpendLedger, SpendCapPolicy,
    SpendIntent, generate_session_hmac_key, mint_approval_receipt, verify_approval_receipt,
};
pub use secrets::{
    NwcUriSummary, SecretValidationError, derive_nut13_seed_from_mnemonic,
    derive_nut13_seed_hex_from_mnemonic, generate_bip39_mnemonic_12, generate_db_passphrase_hex,
    sha256_hex, validate_bip39_mnemonic, validate_nwc_uri,
};
pub use url_guard::{UrlError, ValidatedUrl, validate_public_https_url, validate_resolved_ip};
