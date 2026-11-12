//! The secret-key type.

use bech32::Bech32;
use bech32::primitives::decode::CheckedHrpstring;
use k256::schnorr::SigningKey;
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

/// Why an `nsec` was not accepted. No variant carries any part of the text:
/// it is, or may be, a secret key.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum NsecError {
    /// The text is not a valid bech32 string.
    #[error("not a bech32 string")]
    NotBech32,
    /// The text is bech32 but is not an `nsec`.
    #[error("not an nsec")]
    WrongPrefix,
    /// The payload is not 32 bytes.
    #[error("an nsec holds exactly 32 bytes")]
    WrongLength,
    /// The 32 bytes are zero or not below the curve order.
    #[error("not a valid secp256k1 secret key")]
    InvalidKey,
}

/// A valid secp256k1 secret key.
///
/// ```
/// use hkask_nostr_signer::SecretKey;
///
/// // NIP-19's published example key, not anyone's identity.
/// let key = SecretKey::from_nsec("nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5")
///     .expect("the published example parses");
/// assert_eq!(key.x_only_public_key().len(), 32);
/// ```
///
/// It implements neither `Debug` nor `Display`, so it cannot reach a log line
/// or an error message by accident. Each example below is the one above plus
/// one formatting call, and must not compile:
///
/// ```compile_fail
/// use hkask_nostr_signer::SecretKey;
///
/// let key = SecretKey::from_nsec("nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5")
///     .expect("the published example parses");
/// assert_eq!(key.x_only_public_key().len(), 32);
/// println!("{key:?}");
/// ```
///
/// ```compile_fail
/// use hkask_nostr_signer::SecretKey;
///
/// let key = SecretKey::from_nsec("nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5")
///     .expect("the published example parses");
/// assert_eq!(key.x_only_public_key().len(), 32);
/// println!("{key}");
/// ```
///
/// The scalar is wiped when the key is dropped: the only field is a
/// `k256::schnorr::SigningKey`, which zeroizes its scalar in `Drop`.
pub struct SecretKey {
    signing_key: SigningKey,
}

// Fails to compile if a `k256` release stops wiping its signing key on drop.
const _: () = {
    const fn wiped_on_drop<T: ZeroizeOnDrop>() {}
    wiped_on_drop::<SigningKey>();
};

impl ZeroizeOnDrop for SecretKey {}

impl SecretKey {
    /// Parse a NIP-19 `nsec`.
    pub fn from_nsec(nsec: &str) -> Result<Self, NsecError> {
        let parsed = CheckedHrpstring::new::<Bech32>(nsec).map_err(|_| NsecError::NotBech32)?;
        if parsed.hrp().to_lowercase() != "nsec" {
            return Err(NsecError::WrongPrefix);
        }
        // Decoded straight into a wiped stack buffer: the key never sits in a
        // heap allocation this function would have to track.
        let mut bytes = Zeroizing::new([0u8; 32]);
        let mut length = 0usize;
        for byte in parsed.byte_iter() {
            if let Some(slot) = bytes.get_mut(length) {
                *slot = byte;
            }
            length += 1;
        }
        if length != bytes.len() {
            return Err(NsecError::WrongLength);
        }
        Self::from_bytes(&bytes)
    }

    /// The BIP-340 x-only public key for this secret key.
    pub fn x_only_public_key(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes().into()
    }

    /// Build a key from 32 raw bytes, refusing zero and values not below the
    /// curve order.
    pub(crate) fn from_bytes(bytes: &[u8; 32]) -> Result<Self, NsecError> {
        let signing_key = SigningKey::from_slice(bytes).map_err(|_| NsecError::InvalidKey)?;
        Ok(Self { signing_key })
    }

    /// BIP-340 signature of `message` with the given auxiliary randomness.
    /// `None` only if the curve library rejects the derived nonce, which
    /// BIP-340 puts at negligible probability.
    pub(crate) fn sign_raw(&self, message: &[u8], aux_rand: &[u8; 32]) -> Option<[u8; 64]> {
        self.signing_key
            .sign_raw(message, aux_rand)
            .ok()
            .map(|signature| signature.to_bytes())
    }

    /// The raw key bytes, in a buffer that is wiped when dropped.
    pub(crate) fn expose_bytes(&self) -> Zeroizing<[u8; 32]> {
        let mut field_bytes = self.signing_key.to_bytes();
        let mut bytes = Zeroizing::new([0u8; 32]);
        bytes.copy_from_slice(field_bytes.as_slice());
        field_bytes.as_mut_slice().zeroize();
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NIP-19's published `nsec` example and the hex key it states.
    const NIP19_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
    const NIP19_NSEC_HEX: &str = "67dea2ed018072d675f5415ecfaed7d2597555e202d85b3d65ea4e58d2d92ffa";

    fn key_from_hex(text: &str) -> SecretKey {
        let bytes: [u8; 32] = hex::decode(text)
            .expect("test key is hex")
            .try_into()
            .expect("test key is 32 bytes");
        SecretKey::from_bytes(&bytes).expect("test key is a valid scalar")
    }

    fn nsec_for(payload: &[u8]) -> String {
        let hrp = bech32::Hrp::parse("nsec").expect("valid prefix");
        bech32::encode::<bech32::Bech32>(hrp, payload).expect("encodes")
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

    /// expect: NIP-19's published `nsec` parses to the published key.
    /// pre: the `nsec` example in NIP-19.
    /// post: the key bytes equal the hex the NIP states. The public key is the
    /// one NIP-19 lists beside it; `amy key public` on the same `nsec` printed
    /// that value too (2026-11-12), so the pairing does not rest on this crate.
    #[test]
    fn nip19_published_nsec_parses_to_published_key() {
        let key = SecretKey::from_nsec(NIP19_NSEC)
            .unwrap_or_else(|error| panic!("published nsec must parse: {error}"));
        assert_eq!(hex::encode(*key.expose_bytes()), NIP19_NSEC_HEX);
        assert_eq!(
            hex::encode(key.x_only_public_key()),
            "7e7e9c42a91bfef19fa929e5fda1b72e0ebc1a4c1141673e2794234d86addf4e"
        );
    }

    /// expect: text that is not a usable `nsec` is refused with a typed error
    /// that repeats none of it.
    /// pre: an `npub`; an `nsec` of 31 bytes; an `nsec` of 32 zero bytes; an
    /// `nsec` holding the curve order (BIP-340: keys must be below it); text
    /// that is not bech32.
    /// post: `WrongPrefix`, `WrongLength`, `InvalidKey`, `InvalidKey`,
    /// `NotBech32`.
    #[test]
    fn from_nsec_refuses_what_is_not_a_usable_nsec() {
        let curve_order =
            hex::decode("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141")
                .expect("hex");
        for (text, expected) in [
            (
                "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg".to_string(),
                NsecError::WrongPrefix,
            ),
            (nsec_for(&[7u8; 31]), NsecError::WrongLength),
            (nsec_for(&[0u8; 32]), NsecError::InvalidKey),
            (nsec_for(&curve_order), NsecError::InvalidKey),
            ("not bech32 at all".to_string(), NsecError::NotBech32),
        ] {
            let error = match SecretKey::from_nsec(&text) {
                Ok(_) => panic!("must be refused: {expected:?}"),
                Err(error) => error,
            };
            assert_eq!(error, expected);
            let shown = format!("{error} {error:?}");
            assert!(!shown.contains(&text), "the error repeats its input");
        }
    }

    /// expect: the key type is declared wiped-on-drop.
    /// pre: none.
    /// post: `SecretKey: ZeroizeOnDrop` holds at compile time. This checks the
    /// declaration; observing freed memory would need `unsafe`, which this
    /// crate forbids.
    #[test]
    fn secret_key_is_declared_wiped_on_drop() {
        fn wiped_on_drop<T: ZeroizeOnDrop>() {}
        wiped_on_drop::<SecretKey>();
    }
}
