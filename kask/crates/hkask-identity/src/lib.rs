#![forbid(unsafe_code)]
//! Human identity and authentication types — Loop 6 (Cybernetics): Access Guard
//!
//! Cybernetics subloop 6.1 (Access Guard) governs who can access what.
//! Human users, userpod identities, and sessions are verified at this boundary.

use hkask_types::WebID;
use hkask_types::id::UserID;
use hkask_types::id::WalletId;
use hkask_types::identity::{OAuthProvider, Role};
use serde::{Deserialize, Serialize};

/// Loop: Cybernetics
/// Human user — owns contact info (email, phone for recovery only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanUser {
    pub user_id: UserID,
    pub email_enc: Vec<u8>,
    pub phone_enc: Option<Vec<u8>>,
    pub passphrase_hash: String,
    pub salt: String,
    pub master_salt: String,
    pub created_at: i64,
    pub last_active: Option<i64>,
    pub passphrase_set_at: Option<i64>,
    /// User role for access control (defaults to Member).
    /// expect: "Every user has a role that controls their access level"
    pub role: Role,
    /// OAuth provider used for sign-in (None = passphrase-only registration).
    /// expect: "System types preserve semantic identity and are provenance-aware"
    pub oauth_provider: Option<OAuthProvider>,
    /// External user ID from the OAuth provider (e.g., GitHub user ID).
    /// expect: "System types preserve semantic identity and are provenance-aware"
    pub oauth_provider_user_id: Option<String>,
    /// Display name from the OAuth provider (e.g., GitHub username).
    /// expect: "System types preserve semantic identity and are provenance-aware"
    pub oauth_display_name: Option<String>,
}

impl HumanUser {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  email_enc is an encrypted email byte vector; passphrase_hash and salt
    ///       are derived from a valid passphrase; master_salt is a random salt
    /// post: returns a new HumanUser with a fresh UserID, role=Member,
    ///       created_at set to current Unix timestamp, no OAuth linkage
    #[must_use]
    pub fn new(
        email_enc: Vec<u8>,
        phone_enc: Option<Vec<u8>>,
        passphrase_hash: String,
        salt: String,
        master_salt: String,
    ) -> Self {
        Self {
            user_id: UserID::new(),
            email_enc,
            phone_enc,
            passphrase_hash,
            salt,
            master_salt,
            created_at: chrono::Utc::now().timestamp(),
            last_active: None,
            passphrase_set_at: None,
            oauth_provider: None,
            oauth_provider_user_id: None,
            oauth_display_name: None,
            role: Role::Member,
        }
    }
}

/// Loop: Cybernetics
/// UserPod identity — the in-system persona users log in AS (1:1 per user).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPod {
    pub userpod_name: String,
    pub user_id: UserID,
    pub webid: WebID,
    /// Wallet ID for rJoule payments.
    pub wallet_id: Option<WalletId>,
    pub first_name_enc: Vec<u8>,
    pub last_name_enc: Vec<u8>,
    /// Capabilities granted to this userpod (e.g., ["tool:inference:call", "tool:mcp:invoke"]).
    pub capabilities: Vec<String>,
    pub created_at: i64,
    pub last_login: Option<i64>,
}

impl UserPod {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  userpod_name is a non-empty string (1–64 alphanumeric/hyphen/underscore chars)
    /// post: returns a deterministic WebID with the "userpod" namespace;
    ///       same userpod_name always produces the same WebID
    #[must_use]
    pub fn derive_webid(userpod_name: &str) -> WebID {
        WebID::from_persona_with_namespace(userpod_name.as_bytes(), "userpod")
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  userpod_name is non-empty; user_id is a valid UserID;
    ///       first_name_enc and last_name_enc are encrypted byte vectors
    /// post: returns a UserPod with derived webid, wallet_id=None,
    ///       created_at set to current Unix timestamp,
    ///       last_login=None
    /// Default capabilities granted to every userpod.
    pub const DEFAULT_CAPABILITIES: &[&str] = &["tool:inference:call", "tool:mcp:invoke"];

    pub fn new(
        userpod_name: String,
        user_id: UserID,
        first_name_enc: Vec<u8>,
        last_name_enc: Vec<u8>,
        capabilities: Vec<String>,
    ) -> Self {
        Self {
            webid: Self::derive_webid(&userpod_name),
            userpod_name,
            user_id,
            wallet_id: None,
            first_name_enc,
            last_name_enc,
            capabilities,
            created_at: chrono::Utc::now().timestamp(),
            last_login: None,
        }
    }
}

/// Loop: Cybernetics
/// Active user session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSession {
    pub session_id: String,
    pub userpod_name: String,
    pub webid: WebID,
    pub user_id: UserID,
    pub session_key_salt: String,
    pub expires_at: i64,
    pub last_active: i64,
}

impl UserSession {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  now is a Unix timestamp (i64); self.expires_at is a valid
    ///       expiry timestamp set at session creation
    /// post: returns true if now > self.expires_at (session has expired);
    ///       returns false if now <= self.expires_at (session still valid)
    #[must_use]
    pub fn is_expired(&self, now: i64) -> bool {
        now > self.expires_at
    }
}

/// Invite status for multi-user onboarding.
///
/// expect: "I can send invites to bring other users onto my server"
/// \[P2\] Goal: Affirmative Consent — invite requires explicit admin action
///Constraining: User Sovereignty — invite is scoped to a specific server
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InviteStatus {
    Pending,
    Accepted,
    Revoked,
    Expired,
}

impl std::fmt::Display for InviteStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InviteStatus::Pending => f.write_str("pending"),
            InviteStatus::Accepted => f.write_str("accepted"),
            InviteStatus::Revoked => f.write_str("revoked"),
            InviteStatus::Expired => f.write_str("expired"),
        }
    }
}

impl std::str::FromStr for InviteStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(InviteStatus::Pending),
            "accepted" => Ok(InviteStatus::Accepted),
            "revoked" => Ok(InviteStatus::Revoked),
            "expired" => Ok(InviteStatus::Expired),
            other => Err(format!("Unknown invite status: {other}")),
        }
    }
}

/// Multi-user invitation record.
///
/// expect: "I can track who was invited and whether they've joined"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invite {
    pub invite_id: String,
    pub created_by: UserID,
    pub code: String,
    pub status: InviteStatus,
    pub created_at: i64,
    pub expires_at: i64,
    pub accepted_at: Option<i64>,
    pub accepted_user_id: Option<UserID>,
}

/// Loop: Cybernetics
/// Registration request for new userpod identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationRequest {
    pub userpod_name: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub phone: Option<String>,
    pub passphrase: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, thiserror::Error)]
/// Loop: Cybernetics
///
/// Variants are grouped by shared recovery path:
/// - `InvalidUserPodName` — \[NORMATIVE\] name must be 1–64 alphanumeric/hyphen/underscore chars (P6 — Space for UserPods).
/// - `EmptyName` — required name field is missing
/// - `InvalidContact` — email or phone format is wrong
/// - `InvalidPassphrase` — passphrase doesn't meet requirements
pub enum RegistrationError {
    #[error("Invalid userpod name: must be 1-64 chars, alphanumeric with hyphens/underscores")]
    InvalidUserPodName,
    #[error("Required name field is empty")]
    EmptyName,
    #[error("Invalid contact information format")]
    InvalidContact,
    #[error("Passphrase does not meet requirements: 8+ alphanumeric chars, mixed case")]
    InvalidPassphrase,
}
