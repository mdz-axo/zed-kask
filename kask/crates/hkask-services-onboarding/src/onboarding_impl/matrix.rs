//! Matrix username derivation helpers.
//!
//! Matrix account registration during onboarding was removed (Matrix is now
//! deferred out of the onboarding flow). Only the localpart derivation helper
//! remains, used by the onboarding web page for display purposes.

/// Derive Matrix username localparts from display and userpod names.
///
/// Returns `(human_localpart, userpod_localpart)` for use in Matrix APIs or
/// display. Sanitizes to lowercase alphanumeric/hyphen/underscore/dot.
pub fn derive_matrix_localparts(display_name: &str, userpod_name: &str) -> (String, String) {
    let sanitize = |s: &str| -> String {
        s.to_lowercase()
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' {
                    c
                } else {
                    '-'
                }
            })
            .collect()
    };
    let (first, last) = {
        let mut parts = display_name.splitn(2, ' ');
        let f = sanitize(parts.next().unwrap_or("user"));
        let l = sanitize(parts.next().unwrap_or("user"));
        (f, l)
    };
    let human_localpart = format!("{}-{}", first, last);
    let userpod_localpart = format!("{}-bot", sanitize(userpod_name).replace(' ', "-"));
    (human_localpart, userpod_localpart)
}
