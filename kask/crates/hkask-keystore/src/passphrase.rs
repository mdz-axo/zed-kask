//! Random passphrase generation for first-run DB provisioning.
//!
//! The passphrase protects an encrypted SQLCipher database, so generation uses
//! `rand::rng()` for cryptographic randomness — a predictable seed would let an
//! attacker who can read the keychain recover the DB key. The word list is
//! curated to common English words of 8+ letters so the generated passphrase
//! is human-readable and the user can change it later via the keychain or the
//! `HKASK_DB_PASSPHRASE` env var.

use rand::seq::IndexedRandom;

/// A curated list of common English words, each 8+ letters.
///
/// Used to generate a human-readable DB passphrase on first run. The user can
/// change it later via the keychain or the `HKASK_DB_PASSPHRASE` env var.
const PASSPHRASE_WORDS: &[&str] = &[
    "absolute",
    "adventure",
    "amplitude",
    "architect",
    "asteroid",
    "atmosphere",
    "backbone",
    "blueprint",
    "boundary",
    "butterfly",
    "calendar",
    "catalyst",
    "cathedral",
    "champion",
    "chandelier",
    "cheesecake",
    "cinnamon",
    "composer",
    "computer",
    "constellation",
    "corridor",
    "courtyard",
    "daffodil",
    "daybreak",
    "dinosaur",
    "directory",
    "driftwood",
    "elephant",
    "epiphany",
    "eternity",
    "festival",
    "flamingo",
    "fountain",
    "gossamer",
    "helicopter",
    "hospital",
    "hummingbird",
    "identity",
    "infinity",
    "inspiration",
    "kaleidoscope",
    "lavender",
    "lemonade",
    "lighthouse",
    "limousine",
    "magnolia",
    "manuscript",
    "marigold",
    "meridian",
    "midnight",
    "mountain",
    "mushroom",
    "mystique",
    "nightingale",
    "novelette",
    "oblivion",
    "opulence",
    "orchestra",
    "palindrome",
    "panorama",
    "paradise",
    "parchment",
    "passenger",
    "pavilion",
    "peppermint",
    "pinnacle",
    "platinum",
    "pomegranate",
    "porcelain",
    "primrose",
    "propeller",
    "quicksilver",
    "radiance",
    "reflection",
    "refrigerator",
    "renaissance",
    "resonance",
    "rhinoceros",
    "riverbed",
    "rosewood",
    "sapphire",
    "satellite",
    "scintilla",
    "seashell",
    "serenity",
    "silhouette",
    "snowfall",
    "solstice",
    "spectrum",
    "stardust",
    "starlight",
    "sunflower",
    "tapestry",
    "tortoise",
    "tradition",
    "tranquility",
    "turbulence",
    "umbrella",
    "undertow",
    "universe",
    "upholstery",
    "vanguard",
    "waterfall",
    "whimsical",
    "wildflower",
    "windmill",
    "yesterday",
];

/// Generate a random passphrase word from the curated list.
///
/// Uses `rand::rng()` for cryptographic randomness — the passphrase protects an
/// encrypted database, so a predictable seed would let an attacker who can read
/// the keychain recover the DB key. Falls back to `"kask"` only if the word
/// list is somehow empty (it is a compile-time constant, so this is a
/// defense-in-depth guard, not an expected path).
pub fn generate_random_passphrase() -> String {
    let mut rng = rand::rng();
    PASSPHRASE_WORDS
        .choose(&mut rng)
        .map(|word| word.to_string())
        .unwrap_or_else(|| "kask".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_passphrase_is_in_word_list() {
        for _ in 0..100 {
            let word = generate_random_passphrase();
            assert!(
                PASSPHRASE_WORDS.contains(&word.as_str()),
                "generated word '{word}' is not in PASSPHRASE_WORDS"
            );
        }
    }

    #[test]
    fn word_list_has_expected_length() {
        // Pin the list size so a future edit doesn't silently shrink entropy.
        assert_eq!(PASSPHRASE_WORDS.len(), 107);
    }

    #[test]
    fn all_words_are_at_least_eight_letters() {
        for word in PASSPHRASE_WORDS {
            assert!(word.len() >= 8, "word '{word}' is shorter than 8 letters");
        }
    }
}
