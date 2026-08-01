//! Behavioral contract tests for hkask-keystore.
//!
//! Covering encryption/decryption, key derivation, and internal secrets.
//! OS keychain operations are excluded —
//! these tests are deterministic and require no keychain or network.

use hkask_keystore::{
    encryption::{EncryptionError, EncryptionService, derive_key},
    keychain::resolve_db_passphrase,
};

// ---------------------------------------------------------------------------
// 1. EncryptionService encrypt/decrypt roundtrip
// ---------------------------------------------------------------------------

#[test]
fn encrypt_decrypt_roundtrip() {
    let passphrase = "roundtrip-test-passphrase";
    let salt = EncryptionService::generate_salt();
    let svc = EncryptionService::new(passphrase, &salt).expect("create service");

    let plaintext = b"The quick brown fox jumps over the lazy dog";
    let ciphertext = svc.encrypt(plaintext).expect("encrypt");
    let decrypted = svc.decrypt(&ciphertext).expect("decrypt");

    assert_eq!(
        decrypted, plaintext,
        "decrypted text must match original plaintext"
    );
}

#[test]
fn encrypt_decrypt_empty_plaintext() {
    let salt = EncryptionService::generate_salt();
    let svc = EncryptionService::new("empty-test-passphrase", &salt).expect("create service");

    let ciphertext = svc.encrypt(b"").expect("encrypt empty");
    let decrypted = svc.decrypt(&ciphertext).expect("decrypt empty");

    assert_eq!(decrypted, b"", "empty roundtrip must produce empty output");
}

#[test]
fn encrypt_decrypt_binary_data() {
    let salt = EncryptionService::generate_salt();
    let svc = EncryptionService::new("binary-test-passphrase", &salt).expect("create service");

    let plaintext: Vec<u8> = (0..=255).collect();
    let ciphertext = svc.encrypt(&plaintext).expect("encrypt binary");
    let decrypted = svc.decrypt(&ciphertext).expect("decrypt binary");

    assert_eq!(
        decrypted, plaintext,
        "binary roundtrip must preserve all 256 byte values"
    );
}

// ---------------------------------------------------------------------------
// 2. EncryptionService tampered ciphertext rejection
// ---------------------------------------------------------------------------

#[test]
fn decrypt_rejects_tampered_ciphertext() {
    let salt = EncryptionService::generate_salt();
    let svc = EncryptionService::new("tamper-test", &salt).expect("create service");

    let plaintext = b"sensitive data";
    let mut ciphertext = svc.encrypt(plaintext).expect("encrypt");

    // Flip a byte in the encrypted portion (after the 12-byte nonce)
    let flip_index = ciphertext.len() / 2;
    ciphertext[flip_index] ^= 0x01;

    let result = svc.decrypt(&ciphertext);
    assert!(
        result.is_err(),
        "tampered ciphertext must be rejected (AES-GCM auth tag)"
    );
}

#[test]
fn decrypt_rejects_truncated_ciphertext() {
    let salt = EncryptionService::generate_salt();
    let svc = EncryptionService::new("truncation-test", &salt).expect("create service");

    let plaintext = b"some data";
    let ciphertext = svc.encrypt(plaintext).expect("encrypt");

    // Feed only the 12-byte nonce — not enough for nonce + auth tag
    let too_short = &ciphertext[..12];
    let result = svc.decrypt(too_short);
    assert!(
        result.is_err(),
        "truncated ciphertext (< 12 bytes) must be rejected"
    );
}

// ---------------------------------------------------------------------------
// 3. EncryptionService wrong key rejection
// ---------------------------------------------------------------------------

#[test]
fn decrypt_rejects_wrong_key() {
    let salt = EncryptionService::generate_salt();
    let svc_a = EncryptionService::new("passphrase-alpha", &salt).expect("create svc A");
    let svc_b = EncryptionService::new("passphrase-bravo", &salt).expect("create svc B");

    let plaintext = b"top secret";
    let ciphertext = svc_a.encrypt(plaintext).expect("encrypt with A");

    let result = svc_b.decrypt(&ciphertext);
    assert!(
        result.is_err(),
        "decrypt with a different key must fail (AES-GCM auth tag)"
    );
}

// ---------------------------------------------------------------------------
// 4. derive_key properties
// ---------------------------------------------------------------------------

#[test]
fn derive_key_deterministic() {
    let passphrase = "deterministic-test";
    let salt = EncryptionService::generate_salt();

    let k1 = derive_key(passphrase, &salt).expect("first derivation");
    let k2 = derive_key(passphrase, &salt).expect("second derivation");

    assert_eq!(
        &*k1, &*k2,
        "same passphrase + salt must produce the same key"
    );
}

#[test]
fn derive_key_salt_dependent() {
    let passphrase = "salt-test-passphrase";
    let salt_a = EncryptionService::generate_salt();
    let salt_b = EncryptionService::generate_salt();

    // Salts are random — in the extremely unlikely event they collide, retry
    if salt_a == salt_b {
        return; // skip; astronomically improbable
    }

    let k1 = derive_key(passphrase, &salt_a).expect("derivation with salt A");
    let k2 = derive_key(passphrase, &salt_b).expect("derivation with salt B");

    assert_ne!(&*k1, &*k2, "different salts must produce different keys");
}

#[test]
fn derive_key_rejects_empty_passphrase() {
    let salt = EncryptionService::generate_salt();
    let result = derive_key("", &salt);
    assert!(
        matches!(result, Err(EncryptionError::InvalidPassphrase)),
        "empty passphrase must be rejected with InvalidPassphrase"
    );
}

// ---------------------------------------------------------------------------
// 5. resolve_db_passphrase env precedence
// ---------------------------------------------------------------------------

#[test]
fn resolve_db_passphrase_from_env() {
    // SAFETY: test-only env var mutation.
    unsafe {
        std::env::set_var("HKASK_DB_PASSPHRASE", "test-db-passphrase");
    }
    let passphrase = resolve_db_passphrase().expect("resolve from env");
    assert_eq!(&*passphrase, b"test-db-passphrase");
}

// ---------------------------------------------------------------------------
// 6. Nonce uniqueness
// ---------------------------------------------------------------------------

#[test]
fn encrypt_produces_different_ciphertext_each_time() {
    let salt = EncryptionService::generate_salt();
    let svc = EncryptionService::new("nonce-uniqueness-test", &salt).expect("create service");

    let plaintext = b"same plaintext, different nonce";

    let ct1 = svc.encrypt(plaintext).expect("first encrypt");
    let ct2 = svc.encrypt(plaintext).expect("second encrypt");

    assert_ne!(
        ct1, ct2,
        "each encryption must use a unique nonce, producing different ciphertext"
    );

    // Sanity: both must decrypt successfully
    let d1 = svc.decrypt(&ct1).expect("decrypt ct1");
    let d2 = svc.decrypt(&ct2).expect("decrypt ct2");
    assert_eq!(d1, plaintext);
    assert_eq!(d2, plaintext);
}
