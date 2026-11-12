#![forbid(unsafe_code)]
//! Secret Nostr operations: everything here reads, derives or uses the
//! identity key.
//!
//! A Nostr key cannot be rotated or recovered, so only the editor process may
//! depend on this crate. No MCP server links it; the server's side of the
//! protocol lives in `hkask-nostr-core`.

use bech32::Bech32;
use bech32::primitives::decode::CheckedHrpstring;
use chacha20poly1305::{AeadInOut, KeyInit, Tag, XChaCha20Poly1305, XNonce};
use k256::schnorr::SigningKey;
use scrypt::{Params, scrypt};
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

/// NIP-49's only defined version byte.
const VERSION: u8 = 0x02;
/// version, cost, 16-byte salt, 24-byte nonce, security byte, 32-byte
/// ciphertext, 16-byte tag.
const BLOB_LEN: usize = 91;

/// The highest scrypt cost byte this crate will run. NIP-49's table puts
/// `log_n` 20 at 1 GiB; a blob that asks for more is refused before any
/// allocation.
pub const MAX_LOG_N: u8 = 20;

/// Why an `ncryptsec` did not yield a key. No variant carries key material or
/// the password.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DecryptError {
    /// The text is not a bech32 `ncryptsec` of the NIP-49 length and version.
    #[error("not a NIP-49 ncryptsec")]
    Malformed,
    /// The blob's scrypt cost byte is above [`MAX_LOG_N`].
    #[error("scrypt cost {log_n} is above the accepted maximum {MAX_LOG_N}")]
    CostTooHigh { log_n: u8 },
    /// The cipher's authentication failed: wrong password, or a damaged blob.
    #[error("wrong password, or the ncryptsec is damaged")]
    WrongPassword,
    /// The decrypted bytes are not a valid secp256k1 secret key.
    #[error("the decrypted bytes are not a secp256k1 secret key")]
    InvalidKey,
}

/// A valid secp256k1 secret key. It implements neither `Debug` nor `Display`,
/// and the scalar it holds is wiped when it is dropped (`k256`'s `SigningKey`
/// zeroizes on drop).
pub struct SecretKey {
    signing_key: SigningKey,
}

impl SecretKey {
    /// The BIP-340 x-only public key for this secret key.
    pub fn x_only_public_key(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes().into()
    }
}

/// Decrypt a NIP-49 `ncryptsec` under `password`.
///
/// The password is normalized to NFKC, as NIP-49 requires. The cost byte is
/// checked against [`MAX_LOG_N`] before key derivation allocates anything.
pub fn decrypt_ncryptsec(ncryptsec: &str, password: &str) -> Result<SecretKey, DecryptError> {
    let parsed = CheckedHrpstring::new::<Bech32>(ncryptsec).map_err(|_| DecryptError::Malformed)?;
    if parsed.hrp().to_lowercase() != "ncryptsec" {
        return Err(DecryptError::Malformed);
    }
    let blob: Vec<u8> = parsed.byte_iter().collect();
    let blob: [u8; BLOB_LEN] = blob.try_into().map_err(|_| DecryptError::Malformed)?;
    if blob[0] != VERSION {
        return Err(DecryptError::Malformed);
    }
    let log_n = blob[1];
    if log_n > MAX_LOG_N {
        return Err(DecryptError::CostTooHigh { log_n });
    }
    let salt = &blob[2..18];
    let nonce = <&XNonce>::try_from(&blob[18..42]).map_err(|_| DecryptError::Malformed)?;
    let security_byte = &blob[42..43];
    let ciphertext = &blob[43..75];
    let tag = <&Tag>::try_from(&blob[75..91]).map_err(|_| DecryptError::Malformed)?;

    let params = Params::new(log_n, 8, 1).map_err(|_| DecryptError::Malformed)?;
    let normalized = Zeroizing::new(password.nfkc().collect::<String>());
    let mut symmetric_key = Zeroizing::new([0u8; 32]);
    scrypt(
        normalized.as_bytes(),
        salt,
        &params,
        symmetric_key.as_mut_slice(),
    )
    .map_err(|_| DecryptError::Malformed)?;

    let cipher = XChaCha20Poly1305::new_from_slice(symmetric_key.as_slice())
        .map_err(|_| DecryptError::Malformed)?;
    let mut key_bytes = Zeroizing::new([0u8; 32]);
    key_bytes.copy_from_slice(ciphertext);
    cipher
        .decrypt_inout_detached(nonce, security_byte, key_bytes.as_mut_slice().into(), tag)
        .map_err(|_| DecryptError::WrongPassword)?;

    let signing_key =
        SigningKey::from_slice(key_bytes.as_slice()).map_err(|_| DecryptError::InvalidKey)?;
    Ok(SecretKey { signing_key })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_from_hex(text: &str) -> SecretKey {
        let bytes = hex::decode(text).expect("test key is hex");
        SecretKey {
            signing_key: SigningKey::from_slice(&bytes).expect("test key is a valid scalar"),
        }
    }

    /// expect: NIP-49's published encrypted key decrypts, under its published
    /// password, to its published key.
    /// pre: the "Decryption" test data in NIP-49 (password `nostr`, log_n 16).
    /// post: the decrypted bytes equal the published hex key.
    #[test]
    fn nip49_published_ncryptsec_decrypts_to_published_key() {
        let key = decrypt_ncryptsec(PUBLISHED_NCRYPTSEC, "nostr")
            .unwrap_or_else(|error| panic!("published vector must decrypt: {error}"));
        assert_eq!(
            hex::encode(key.signing_key.to_bytes()),
            "3501454135014541350145413501453fefb02227e449e57cf4d3a3ce05378683"
        );
    }

    /// NIP-49's published test blob.
    const PUBLISHED_NCRYPTSEC: &str = "ncryptsec1qgg9947rlpvqu76pj5ecreduf9jxhselq2nae2kghhvd5g7dgjtcxfqtd67p9m0w57lspw8gsq6yphnm8623nsl8xn9j4jdzz84zm3frztj3z7s35vpzmqf6ksu8r89qk5z2zxfmu5gv8th8wclt0h4p";

    /// expect: a wrong password is a typed refusal that yields no key.
    /// pre: NIP-49's published blob and a password that is not `nostr`.
    /// post: `WrongPassword`, whose text does not contain the published key.
    #[test]
    fn wrong_password_is_refused_without_key_material() {
        let error = match decrypt_ncryptsec(PUBLISHED_NCRYPTSEC, "not-the-password") {
            Ok(_) => panic!("a wrong password must not decrypt"),
            Err(error) => error,
        };
        assert_eq!(error, DecryptError::WrongPassword);
        let shown = format!("{error} {error:?}");
        assert!(
            !shown.contains("3501454135014541350145413501453fefb02227e449e57cf4d3a3ce05378683")
        );
        assert!(!shown.contains("not-the-password"));
    }

    /// expect: a blob that asks for more scrypt memory than the bound is
    /// refused on its cost byte, before key derivation runs.
    /// pre: a well-formed 91-byte blob whose cost byte is 21 (2 GiB in
    /// NIP-49's table) and whose other fields are zero.
    /// post: `CostTooHigh { log_n: 21 }`. Had derivation run first, the zero
    /// ciphertext would fail authentication and report `WrongPassword`.
    #[test]
    fn cost_byte_above_the_bound_is_refused_before_derivation() {
        let mut blob = [0u8; 91];
        blob[0] = 0x02;
        blob[1] = MAX_LOG_N + 1;
        let hrp = bech32::Hrp::parse("ncryptsec").expect("valid prefix");
        let text = bech32::encode::<bech32::Bech32>(hrp, &blob).expect("encodes");
        let error = match decrypt_ncryptsec(&text, "nostr") {
            Ok(_) => panic!("an over-cost blob must not decrypt"),
            Err(error) => error,
        };
        assert_eq!(error, DecryptError::CostTooHigh { log_n: 21 });
    }

    /// expect: the public key derived from each BIP-340 secret key is the
    /// published public key, so the signer can check a decrypted key against
    /// the recorded identity before it signs.
    /// pre: BIP-340 test vectors 0 to 3 (the vectors that publish a secret key).
    /// post: `x_only_public_key` equals the vector's "public key" column.
    #[test]
    fn bip340_published_secret_keys_derive_published_public_keys() {
        for (secret, public) in [
            (
                "0000000000000000000000000000000000000000000000000000000000000003",
                "f9308a019258c31049344f85f89d5229b531c845836f99b08601f113bce036f9",
            ),
            (
                "b7e151628aed2a6abf7158809cf4f3c762e7160f38b4da56a784d9045190cfef",
                "dff1d77f2a671c5f36183726db2341be58feae1da2deced843240f7b502ba659",
            ),
            (
                "c90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74020bbea63b14e5c9",
                "dd308afec5777e13121fa72b9cc1b7cc0139715309b086c960e18fd969774eb8",
            ),
            (
                "0b432b2677937381aef05bb02a66ecd012773062cf3fa2549e44f58ed2401710",
                "25d1dff95105f5253c4022f628a996ad3a0d95fbf21d468a1b33f8c160d8f517",
            ),
        ] {
            assert_eq!(
                hex::encode(key_from_hex(secret).x_only_public_key()),
                public
            );
        }
    }
}
