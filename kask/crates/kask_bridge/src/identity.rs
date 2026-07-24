//! Identity resolution — derives the hKask userpod name from the Zed login.
//!
//! The userpod name is the sanitized `User::username` from the Zed account
//! (the GitHub-style login, e.g. `mdz-axo`). This collapses the former
//! `kask login <name>` onboarding step into a lookup: the userpod identity
//! is derived from the Zed session, not entered separately.
//!
//! Convention:
//! - `User::username` (SharedString) → `sanitize_name()` → userpod name
//! - `WebID::for_userpod_name(&sanitized)` → deterministic WebID
//! - `agent_paths::userpod_dir(&sanitized)` → filesystem paths
//!
//! When the user is not yet logged in, `resolve_userpod_name` returns `None`
//! and the caller defers userpod-dependent wiring until the session arrives.

use hkask_types::{WebID, agent_paths::sanitize_name};

/// Derive the userpod name from a Zed `User::username`.
///
/// The username is the stable, lowercase, GitHub-style handle from the Zed
/// account. We sanitize it for filesystem use (replaces `/ \ : * ? " < > | ( )`
/// and spaces with dashes) so it can be used directly as a directory name and
/// a `WebID` persona.
///
/// Returns `None` if the username is empty after sanitization.
pub fn userpod_name_from_username(username: &str) -> Option<String> {
    let sanitized = sanitize_name(username);
    if sanitized.is_empty() || sanitized == "unnamed" {
        None
    } else {
        Some(sanitized)
    }
}

/// Derive the `WebID` for a Zed username.
///
/// Deterministic: the same username always produces the same WebID
/// (via `WebID::for_userpod_name` in the `"hkask"` namespace).
pub fn webid_from_username(username: &str) -> Option<WebID> {
    userpod_name_from_username(username).map(|name| WebID::for_userpod_name(&name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_style_username_passes_through() {
        assert_eq!(
            userpod_name_from_username("mdz-axo").as_deref(),
            Some("mdz-axo")
        );
        assert_eq!(
            userpod_name_from_username("octocat").as_deref(),
            Some("octocat")
        );
    }

    #[test]
    fn spaces_become_dashes() {
        assert_eq!(
            userpod_name_from_username("Jacques Zuck").as_deref(),
            Some("Jacques-Zuck")
        );
    }

    #[test]
    fn path_traversal_rejected() {
        assert_eq!(userpod_name_from_username(".."), None);
        assert_eq!(userpod_name_from_username("."), None);
    }

    #[test]
    fn empty_rejected() {
        assert_eq!(userpod_name_from_username(""), None);
        assert_eq!(userpod_name_from_username("   "), None);
    }

    #[test]
    fn webid_is_deterministic() {
        let w1 = webid_from_username("mdz-axo").unwrap();
        let w2 = webid_from_username("mdz-axo").unwrap();
        assert_eq!(w1, w2);
    }

    #[test]
    fn different_users_get_different_webids() {
        let w1 = webid_from_username("alice").unwrap();
        let w2 = webid_from_username("bob").unwrap();
        assert_ne!(w1, w2);
    }
}
