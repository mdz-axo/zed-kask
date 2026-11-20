//! NIP-49: a secret key encrypted under a password (`ncryptsec`).

use bech32::Bech32;
use bech32::primitives::decode::CheckedHrpstring;
use chacha20poly1305::aead::Generate;
use chacha20poly1305::{AeadInOut, KeyInit, Tag, XChaCha20Poly1305, XNonce};
use scrypt::{Params, scrypt};
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

use crate::secret::{NsecError, SecretKey};

/// The scrypt cost byte used when encrypting. NIP-49's table puts `log_n` 18
/// at 256 MiB.
pub const ENCRYPT_LOG_N: u8 = 18;

/// The highest scrypt cost byte this crate will run. NIP-49's table puts
/// `log_n` 20 at 1 GiB; a blob that asks for more is refused before any
/// allocation.
pub const MAX_LOG_N: u8 = 20;

/// NIP-49's only defined version byte.
const VERSION: u8 = 0x02;
/// version, cost, 16-byte salt, 24-byte nonce, security byte, 32-byte
/// ciphertext, 16-byte tag.
const BLOB_LEN: usize = 91;
/// NIP-49 key security byte: the key "has been known to have been handled
/// insecurely (stored unencrypted, cut and paste unencrypted, etc)". An nsec
/// typed or pasted into a settings field is that case.
const HANDLED_INSECURELY: u8 = 0x00;

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

/// Why an `nsec` was not encrypted. No variant carries key material or the
/// password.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum EncryptError {
    /// The text is not a usable `nsec`.
    #[error("{0}")]
    Nsec(#[from] NsecError),
    /// The operating system did not supply random bytes for the salt and
    /// nonce.
    #[error("the operating system did not supply random bytes")]
    Randomness,
    /// The cipher or key derivation refused its inputs. Not reachable with
    /// this crate's fixed parameters; present so no path panics.
    #[error("encryption failed")]
    Cipher,
}

/// Encrypt a NIP-19 `nsec` under `password`, returning a NIP-49 `ncryptsec`.
///
/// The password is normalized to NFKC. The blob records cost
/// [`ENCRYPT_LOG_N`] and marks the key as handled unencrypted, because the
/// caller held it as text.
pub fn encrypt_nsec(nsec: &str, password: &str) -> Result<String, EncryptError> {
    let key = SecretKey::from_nsec(nsec)?;
    let salt = <[u8; 16]>::try_generate().map_err(|_| EncryptError::Randomness)?;
    let nonce = <[u8; 24]>::try_generate().map_err(|_| EncryptError::Randomness)?;
    seal(&key, password, ENCRYPT_LOG_N, &salt, &nonce)
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

    let symmetric_key = derive_key(password, salt, log_n).ok_or(DecryptError::Malformed)?;
    let cipher = XChaCha20Poly1305::new_from_slice(symmetric_key.as_slice())
        .map_err(|_| DecryptError::Malformed)?;
    let mut key_bytes = Zeroizing::new([0u8; 32]);
    key_bytes.copy_from_slice(ciphertext);
    cipher
        .decrypt_inout_detached(nonce, security_byte, key_bytes.as_mut_slice().into(), tag)
        .map_err(|_| DecryptError::WrongPassword)?;

    SecretKey::from_bytes(&key_bytes).map_err(|_| DecryptError::InvalidKey)
}

/// NIP-49's password form: Unicode NFKC, so the same password typed on
/// another machine or client yields the same key.
fn normalize_password(password: &str) -> Zeroizing<String> {
    Zeroizing::new(password.nfkc().collect())
}

/// `scrypt(password, salt, log_n, r=8, p=1)` to a 32-byte key. `None` only if
/// scrypt rejects its parameters.
fn derive_key(password: &str, salt: &[u8], log_n: u8) -> Option<Zeroizing<[u8; 32]>> {
    let params = Params::new(log_n, 8, 1).ok()?;
    let normalized = normalize_password(password);
    let mut symmetric_key = Zeroizing::new([0u8; 32]);
    scrypt(
        normalized.as_bytes(),
        salt,
        &params,
        symmetric_key.as_mut_slice(),
    )
    .ok()?;
    Some(symmetric_key)
}

/// Build the `ncryptsec` for `key` from fixed inputs. [`encrypt_nsec`] supplies
/// fresh random `salt` and `nonce`; tests supply known ones.
fn seal(
    key: &SecretKey,
    password: &str,
    log_n: u8,
    salt: &[u8; 16],
    nonce: &[u8; 24],
) -> Result<String, EncryptError> {
    let symmetric_key = derive_key(password, salt, log_n).ok_or(EncryptError::Cipher)?;
    let cipher = XChaCha20Poly1305::new_from_slice(symmetric_key.as_slice())
        .map_err(|_| EncryptError::Cipher)?;
    let mut key_bytes = key.expose_bytes();
    let tag = cipher
        .encrypt_inout_detached(
            &XNonce::from(*nonce),
            &[HANDLED_INSECURELY],
            key_bytes.as_mut_slice().into(),
        )
        .map_err(|_| EncryptError::Cipher)?;

    // `key_bytes` now holds ciphertext.
    let mut blob = [0u8; BLOB_LEN];
    blob[0] = VERSION;
    blob[1] = log_n;
    blob[2..18].copy_from_slice(salt);
    blob[18..42].copy_from_slice(nonce);
    blob[42] = HANDLED_INSECURELY;
    blob[43..75].copy_from_slice(key_bytes.as_slice());
    blob[75..91].copy_from_slice(tag.as_slice());

    let hrp = bech32::Hrp::parse("ncryptsec").map_err(|_| EncryptError::Cipher)?;
    bech32::encode::<Bech32>(hrp, &blob).map_err(|_| EncryptError::Cipher)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NIP-49's published test blob, its password, and the key it states.
    const PUBLISHED_NCRYPTSEC: &str = "ncryptsec1qgg9947rlpvqu76pj5ecreduf9jxhselq2nae2kghhvd5g7dgjtcxfqtd67p9m0w57lspw8gsq6yphnm8623nsl8xn9j4jdzz84zm3frztj3z7s35vpzmqf6ksu8r89qk5z2zxfmu5gv8th8wclt0h4p";
    const PUBLISHED_KEY_HEX: &str =
        "3501454135014541350145413501453fefb02227e449e57cf4d3a3ce05378683";

    /// NIP-19's published `nsec` example, used as a throwaway key, and the
    /// public key NIP-19 lists for it.
    const NIP19_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
    const NIP19_NSEC_HEX: &str = "67dea2ed018072d675f5415ecfaed7d2597555e202d85b3d65ea4e58d2d92ffa";
    const NIP19_PUBKEY_HEX: &str =
        "7e7e9c42a91bfef19fa929e5fda1b72e0ebc1a4c1141673e2794234d86addf4e";

    /// NIP-49's normalization example: the password as entered, and the NFKC
    /// form the NIP says must reach scrypt.
    const PASSWORD_AS_ENTERED: &[u8] = &[
        0xE2, 0x84, 0xAB, 0xE2, 0x84, 0xA6, 0xE1, 0xBA, 0x9B, 0xCC, 0xA3,
    ];
    const PASSWORD_NORMALIZED: &[u8] = &[0xC3, 0x85, 0xCE, 0xA9, 0xE1, 0xB9, 0xA9];

    /// `NIP19_NSEC` encrypted by the Amethyst command-line client (`amy key
    /// encrypt`, 2026-11-12) under `PASSWORD_AS_ENTERED`. Its cost byte is 16.
    const AMY_NCRYPTSEC: &str = "ncryptsec1qggp8kd8l9jn4c3fz5lhsjv8v8rnrxt87zfrcxk4n5rel86r2g75mgva5ztcvm33hhzqygpqedmjsyrrysstq9mjnyke32emssqvmcu4frw6n65kuhdw4k2dnveezsnq74ejryg3mc6etnkxmqq9ym9k";

    fn text(bytes: &[u8]) -> &str {
        std::str::from_utf8(bytes).expect("test password is UTF-8")
    }

    fn blob_of(ncryptsec: &str) -> Vec<u8> {
        CheckedHrpstring::new::<Bech32>(ncryptsec)
            .expect("test blob is bech32")
            .byte_iter()
            .collect()
    }

    fn ncryptsec_of(blob: &[u8]) -> String {
        let hrp = bech32::Hrp::parse("ncryptsec").expect("valid prefix");
        bech32::encode::<Bech32>(hrp, blob).expect("encodes")
    }

    fn refusal(ncryptsec: &str, password: &str) -> DecryptError {
        match decrypt_ncryptsec(ncryptsec, password) {
            Ok(_) => panic!("must not decrypt"),
            Err(error) => error,
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
        assert_eq!(hex::encode(*key.expose_bytes()), PUBLISHED_KEY_HEX);
    }

    /// expect: NIP-49's normalization example yields the stated bytes.
    /// pre: the "Password Unicode Normalization" example in NIP-49.
    /// post: the normalized password is the UTF-8 the NIP states.
    #[test]
    fn nip49_normalization_example_yields_the_stated_bytes() {
        let normalized = normalize_password(text(PASSWORD_AS_ENTERED));
        assert_eq!(normalized.as_bytes(), PASSWORD_NORMALIZED);
    }

    /// expect: a key another client encrypted opens here whichever Unicode
    /// form the password is typed in.
    /// pre: a blob made by `amy` from NIP-19's example key under NIP-49's
    /// example password in its un-normalized form.
    /// post: both forms of the password yield NIP-19's public key.
    #[test]
    fn a_blob_from_another_client_opens_under_either_password_form() {
        for password in [PASSWORD_AS_ENTERED, PASSWORD_NORMALIZED] {
            let key = decrypt_ncryptsec(AMY_NCRYPTSEC, text(password))
                .unwrap_or_else(|error| panic!("amy's blob must decrypt: {error}"));
            assert_eq!(hex::encode(key.x_only_public_key()), NIP19_PUBKEY_HEX);
        }
    }

    /// expect: a wrong password is a typed refusal that yields no key.
    /// pre: NIP-49's published blob and a password that is not `nostr`.
    /// post: `WrongPassword`, whose text contains neither the published key
    /// nor the password tried.
    #[test]
    fn wrong_password_is_refused_without_key_material() {
        let error = refusal(PUBLISHED_NCRYPTSEC, "not-the-password");
        assert_eq!(error, DecryptError::WrongPassword);
        let shown = format!("{error} {error:?}");
        assert!(!shown.contains(PUBLISHED_KEY_HEX));
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
        let mut blob = [0u8; BLOB_LEN];
        blob[0] = VERSION;
        blob[1] = MAX_LOG_N + 1;
        assert_eq!(
            refusal(&ncryptsec_of(&blob), "nostr"),
            DecryptError::CostTooHigh { log_n: 21 }
        );
    }

    /// expect: text that is not a NIP-49 blob is refused as malformed, before
    /// any key derivation.
    /// pre: an `nsec`; a 90-byte `ncryptsec`; a 91-byte `ncryptsec` with
    /// version byte 1; text that is not bech32.
    /// post: `Malformed` for each.
    #[test]
    fn text_that_is_not_a_nip49_blob_is_malformed() {
        let mut wrong_version = [0u8; BLOB_LEN];
        wrong_version[0] = 0x01;
        wrong_version[1] = 16;
        for candidate in [
            NIP19_NSEC.to_string(),
            ncryptsec_of(&[0u8; BLOB_LEN - 1]),
            ncryptsec_of(&wrong_version),
            "not bech32 at all".to_string(),
        ] {
            assert_eq!(refusal(&candidate, "nostr"), DecryptError::Malformed);
        }
    }

    /// expect: the key security byte is authenticated, so a blob cannot be
    /// relabelled as "never handled unencrypted".
    /// pre: NIP-49's published blob with its security byte changed.
    /// post: authentication fails under the right password.
    #[test]
    fn a_changed_security_byte_fails_authentication() {
        let mut blob = blob_of(PUBLISHED_NCRYPTSEC);
        blob[42] ^= 0x01;
        assert_eq!(
            refusal(&ncryptsec_of(&blob), "nostr"),
            DecryptError::WrongPassword
        );
    }

    /// expect: this crate's encryption is NIP-49's, byte for byte.
    /// pre: NIP-19's example key, password `nostr`, cost 16, salt of sixteen
    /// 0x01 bytes, nonce of twenty-four 0x02 bytes.
    /// post: the blob equals the literal below. The literal was first printed
    /// by this function and then decrypted by `amy key decrypt --password
    /// nostr` to NIP-19's example key (2026-11-12), so another implementation
    /// stands behind it.
    #[test]
    fn sealing_with_fixed_inputs_matches_a_blob_another_client_opens() {
        let key = SecretKey::from_nsec(NIP19_NSEC).expect("published nsec parses");
        let sealed = seal(&key, "nostr", 16, &[0x01; 16], &[0x02; 24]).expect("sealing succeeds");
        assert_eq!(
            sealed,
            "ncryptsec1qggqzqgpqyqszqgpqyqszqgpqyqszqszqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgpqptgxj4asl0lagp5sj33vlq8ey6mks2d9qkkpvrksq7vtec66gf0nuenyn4als4nrmdyas0mpqq7dzv4k28xd"
        );
    }

    /// expect: a key survives the round trip through its encrypted form, and
    /// the blob carries the cost and handling marks this crate promises.
    /// pre: NIP-19's example `nsec` and a password.
    /// post: the blob decrypts under the password to the same key; its cost
    /// byte is 18 and its security byte is 0x00; a second encryption of the
    /// same key and password differs (fresh salt and nonce).
    #[test]
    fn an_nsec_survives_the_round_trip_at_the_shipped_cost() {
        let first = encrypt_nsec(NIP19_NSEC, "correct horse")
            .unwrap_or_else(|error| panic!("encryption must succeed: {error}"));
        let blob = blob_of(&first);
        assert_eq!(blob.len(), BLOB_LEN);
        assert_eq!(blob[0], VERSION);
        assert_eq!(blob[1], 18);
        assert_eq!(blob[42], 0x00);

        let key = decrypt_ncryptsec(&first, "correct horse")
            .unwrap_or_else(|error| panic!("round trip must decrypt: {error}"));
        assert_eq!(hex::encode(*key.expose_bytes()), NIP19_NSEC_HEX);

        let second = encrypt_nsec(NIP19_NSEC, "correct horse").expect("encrypts again");
        assert_ne!(first, second, "salt and nonce must be fresh each time");
    }

    /// expect: text that is not a usable `nsec` is not encrypted, and the
    /// refusal repeats none of it.
    /// pre: an `npub` in place of the `nsec`.
    /// post: `EncryptError::Nsec(WrongPrefix)`.
    #[test]
    fn encrypting_something_that_is_not_an_nsec_is_refused() {
        let npub = "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg";
        let error = encrypt_nsec(npub, "correct horse").expect_err("an npub is not an nsec");
        assert_eq!(error, EncryptError::Nsec(NsecError::WrongPrefix));
        assert!(!format!("{error} {error:?}").contains(npub));
    }
}
