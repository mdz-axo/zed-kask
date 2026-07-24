//! UserStore — Human user identity, Argon2id auth, encrypted PII, session management.
use crate::Database;
use crate::database::SqliteDriver;
use crate::database::driver::{query_map, query_row};
use crate::database::value::DbValue;
use crate::{define_driver_store, impl_from_db_error};
use argon2::{PasswordHasher, PasswordVerifier, password_hash::PasswordHash};
use base64::Engine;
use hkask_identity::{HumanUser, Invite, InviteStatus, RegistrationRequest, UserPod, UserSession};
use hkask_types::id::{WalletId, WebID};
use hkask_types::identity::Role;
use hkask_types::{InfrastructureError, NotFound, UserID};
use rand::RngCore;
use rusqlite::OptionalExtension;
use std::str::FromStr;
use thiserror::Error;
use zeroize::Zeroizing;

const USERPOD_COLUMNS: &str = "userpod_name, user_id, webid, wallet_id, first_name_enc, last_name_enc, capabilities, created_at, last_login";
const SESSION_COLUMNS: &str =
    "session_id, userpod_name, webid, user_id, session_key_salt, expires_at, last_active";

#[derive(Error, Debug)]
pub enum UserStoreError {
    #[error(transparent)]
    Infra(#[from] InfrastructureError),
    #[error("User not found: {0}")]
    NotFound(NotFound),
    #[error("UserPod name already registered: {0}")]
    UserPodNameTaken(String),
    #[error("Invalid credentials")]
    InvalidCredentials,
    #[error("Encryption error: {0}")]
    Encryption(String),
    #[error("Decryption error: {0}")]
    Decryption(String),
    #[error("Key derivation error: {0}")]
    KeyDerivation(String),
    #[error("Password hash error: {0}")]
    PasswordHash(String),
    #[error("Passphrase expired {0} days ago — must change")]
    PassphraseExpired(i64),
}
impl_from_db_error!(UserStoreError, Infra);

impl From<rusqlite::Error> for UserStoreError {
    fn from(e: rusqlite::Error) -> Self {
        UserStoreError::Infra(InfrastructureError::database(e.to_string()))
    }
}

pub type UserResult<T> = std::result::Result<T, UserStoreError>;

define_driver_store!(UserStore);

fn userpod_from_row(
    row: &crate::database::value::DbRow,
) -> Result<UserPod, crate::database::types::DbError> {
    Ok(UserPod {
        userpod_name: row.get_str(0)?.to_string(),
        user_id: UserID::from_str(row.get_str(1)?)
            .map_err(|e| crate::database::types::DbError::Database(e.to_string()))?,
        webid: WebID::from_str(row.get_str(2)?)
            .map_err(|e| crate::database::types::DbError::Database(e.to_string()))?,
        wallet_id: match row.get(3)? {
            DbValue::Null => None,
            v => Some(
                WalletId::from_str(v.as_text()?)
                    .map_err(|e| crate::database::types::DbError::Database(e.to_string()))?,
            ),
        },
        first_name_enc: row.get_blob(4)?.to_vec(),
        last_name_enc: row.get_blob(5)?.to_vec(),
        capabilities: {
            let json: String = match row.get(6)? {
                DbValue::Null => "[]".to_string(),
                v => v.as_text()?.to_string(),
            };
            serde_json::from_str(&json).unwrap_or_default()
        },
        created_at: row.get_int(7)?,
        last_login: match row.get(8)? {
            DbValue::Null => None,
            v => Some(v.as_int()?),
        },
    })
}

fn session_from_row(
    row: &crate::database::value::DbRow,
) -> Result<UserSession, crate::database::types::DbError> {
    Ok(UserSession {
        session_id: row.get_str(0)?.to_string(),
        userpod_name: row.get_str(1)?.to_string(),
        webid: WebID::from_str(row.get_str(2)?)
            .map_err(|e| crate::database::types::DbError::Database(e.to_string()))?,
        user_id: UserID::from_str(row.get_str(3)?)
            .map_err(|e| crate::database::types::DbError::Database(e.to_string()))?,
        session_key_salt: row.get_str(4)?.to_string(),
        expires_at: row.get_int(5)?,
        last_active: row.get_int(6)?,
    })
}

impl UserStore {
    /// Initialize the user store schema.
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// \[P1\] Motivating: User Sovereignty — schema for users, userpods, sessions
    /// post: users, userpods, sessions tables created if not exists
    fn init_schema(driver: &std::sync::Arc<dyn crate::database::driver::DatabaseDriver>) {
        let _ = driver.execute_batch(include_str!("sql/users.sql"));
    }

    /// Open a UserStore from a database path and passphrase.
    ///
    /// Encapsulates the Database::open → sqlite_pool → SqliteDriver → from_driver
    /// chain so callers don't need to know about the storage internals.
    ///
    /// pre:  db_path is a valid SQLCipher database path; passphrase is correct
    /// post: returns UserStore backed by the given database; Err on open failure
    pub fn open(db_path: &str, passphrase: &str) -> UserResult<Self> {
        let db = Database::open(db_path, passphrase)
            .map_err(|e| UserStoreError::Infra(InfrastructureError::database(e.to_string())))?;
        let pool = db
            .sqlite_pool()
            .map_err(|e| UserStoreError::Infra(InfrastructureError::database(e.to_string())))?;
        let driver = std::sync::Arc::new(SqliteDriver::new(pool));
        Ok(Self::from_driver(driver))
    }
    /// Register a new userpod.
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// \[P1\] Motivating: User Sovereignty — register a userpod
    /// \[P2\] Constraining: Affirmative Consent — passphrase requirements enforced
    /// pre:  userpod_name is non-empty, passphrase meets requirements
    /// post: userpod and user records created
    pub fn register_userpod(&self, request: &RegistrationRequest) -> UserResult<UserPod> {
        let userpod_name = &request.userpod_name;
        let email = &request.email;
        let phone = &request.phone;
        let first_name = &request.first_name;
        let last_name = &request.last_name;
        let passphrase = &request.passphrase;
        let capabilities = &request.capabilities;
        let user_id = UserID::new();
        let salt = Self::generate_salt();
        let master_salt = Self::generate_salt();
        let passphrase_hash = Self::hash_passphrase(passphrase, &salt)?;
        let pii_key = Self::derive_pii_key(passphrase, &master_salt)?;
        let email_enc = Self::encrypt_pii(email.as_bytes(), &pii_key)?;
        let phone_enc = phone
            .as_ref()
            .map(|p| Self::encrypt_pii(p.as_bytes(), &pii_key))
            .transpose()?;
        let first_name_enc = Self::encrypt_pii(first_name.as_bytes(), &pii_key)?;
        let last_name_enc = Self::encrypt_pii(last_name.as_bytes(), &pii_key)?;

        // Use raw SQLite transaction via downcast for TOCTOU safety
        self.with_raw_conn(|conn| {
            let existing: Option<String> = conn
                .query_row(
                    "SELECT userpod_name FROM userpod_identities WHERE userpod_name = ?1",
                    rusqlite::params![userpod_name],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| crate::database::types::DbError::Database(e.to_string()))?;
            if existing.is_some() {
                return Err(UserStoreError::UserPodNameTaken(userpod_name.clone()));
            }
            conn.execute(
                "INSERT INTO human_users (user_id, email_enc, phone_enc, passphrase_hash, salt, master_salt, created_at, passphrase_set_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    user_id,
                    email_enc,
                    phone_enc,
                    passphrase_hash,
                    salt,
                    master_salt,
                    chrono::Utc::now().timestamp(),
                    chrono::Utc::now().timestamp(),
                ],
            )?;
            let caps_json = serde_json::to_string(capabilities).unwrap_or_else(|_| "[]".to_string());
            let identity =
                UserPod::new(userpod_name.clone(), user_id, first_name_enc, last_name_enc, capabilities.clone());
            conn.execute(
                "INSERT INTO userpod_identities
                 (userpod_name, user_id, webid, first_name_enc, last_name_enc, capabilities, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    identity.userpod_name,
                    identity.user_id,
                    identity.webid,
                    identity.first_name_enc,
                    identity.last_name_enc,
                    caps_json,
                    chrono::Utc::now().timestamp()
                ],
            )?;
            Ok::<_, UserStoreError>(identity)
        })
    }
    /// Find or create a human user via OAuth sign-in.
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// pre:  provider is a valid OAuthProvider; provider_user_id is the external ID from the provider
    /// post: if user exists with matching provider + provider_user_id → returns existing (user, userpod)
    /// post: if user does not exist → creates new HumanUser + primary UserPod + returns both
    pub fn find_or_create_oauth_user(
        &self,
        provider: &hkask_types::identity::OAuthProvider,
        provider_user_id: &str,
        email: &str,
        display_name: &str,
    ) -> UserResult<(HumanUser, UserPod)> {
        // Try to find existing user by OAuth identity
        if let Some((user, userpod)) = self.find_user_by_oauth(provider, provider_user_id)? {
            // Update last_active and display_name
            let now = chrono::Utc::now().timestamp();
            self.driver.execute(
                "UPDATE human_users SET last_active = ?1, oauth_display_name = ?2 WHERE user_id = ?3",
                &[
                    DbValue::Integer(now),
                    DbValue::Text(display_name.to_string()),
                    DbValue::Text(user.user_id.to_string()),
                ],
            )?;
            self.driver.execute(
                "UPDATE userpod_identities SET last_login = ?1 WHERE userpod_name = ?2",
                &[
                    DbValue::Integer(now),
                    DbValue::Text(userpod.userpod_name.clone()),
                ],
            )?;
            return Ok((user, userpod));
        }
        // Create new user — OAuth users get a generated passphrase (never used directly)
        let generated_passphrase = uuid::Uuid::new_v4().to_string();
        let user_id = UserID::new();
        let salt = Self::generate_salt();
        let master_salt = Self::generate_salt();
        let passphrase_hash = Self::hash_passphrase(&generated_passphrase, &salt)?;
        let pii_key = Self::derive_pii_key(&generated_passphrase, &master_salt)?;
        let email_enc = Self::encrypt_pii(email.as_bytes(), &pii_key)?;
        let first_name_enc = Self::encrypt_pii(display_name.as_bytes(), &pii_key)?;
        let last_name_enc = Self::encrypt_pii(b"", &pii_key)?;
        let provider_str = provider.to_string();
        let now = chrono::Utc::now().timestamp();

        let identity = self.with_raw_conn(|conn| {
            // Derive userpod name from display name, with dedup
            let base_name = sanitize_userpod_name(display_name);
            let mut userpod_name = base_name.clone();
            let mut suffix: u32 = 1;
            loop {
                let exists: Option<String> = conn
                    .query_row(
                        "SELECT userpod_name FROM userpod_identities WHERE userpod_name = ?1",
                        rusqlite::params![userpod_name],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|e| crate::database::types::DbError::Database(e.to_string()))?;
                if exists.is_none() {
                    break;
                }
                suffix += 1;
                userpod_name = format!("{}_{}", base_name, suffix);
                if suffix > 100 {
                    userpod_name =
                        format!("{}_{}", base_name, &uuid::Uuid::new_v4().to_string()[..8]);
                    break;
                }
            }
            conn.execute(
                "INSERT INTO human_users (user_id, email_enc, phone_enc, passphrase_hash, salt, master_salt, created_at, passphrase_set_at, oauth_provider, oauth_provider_user_id, oauth_display_name)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                rusqlite::params![
                    user_id,
                    email_enc,
                    Option::<Vec<u8>>::None, // no phone for OAuth users
                    passphrase_hash,
                    salt,
                    master_salt,
                    now,
                    now,
                    provider_str,
                    provider_user_id,
                    display_name,
                ],
            )?;
            let oauth_caps: Vec<String> = UserPod::DEFAULT_CAPABILITIES.iter().map(|s| s.to_string()).collect();
            let caps_json = serde_json::to_string(&oauth_caps).unwrap_or_else(|_| "[]".to_string());
            let identity = UserPod::new(
                userpod_name.clone(),
                user_id,
                first_name_enc,
                last_name_enc,
                oauth_caps,
            );
            conn.execute(
                "INSERT INTO userpod_identities
                 (userpod_name, user_id, webid, first_name_enc, last_name_enc, capabilities, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    identity.userpod_name,
                    identity.user_id,
                    identity.webid,
                    identity.first_name_enc,
                    identity.last_name_enc,
                    caps_json,
                    now
                ],
            )?;
            Ok::<_, UserStoreError>(identity)
        })?;

        let user = self.get_user(&user_id)?;
        Ok((user, identity))
    }
    /// Find a human user by OAuth provider identity.
    ///
    /// expect: "The system provides durable storage for archival data"
    /// pre:  provider is a valid OAuthProvider; provider_user_id is non-empty
    /// post: returns Some((user, primary_userpod)) if found; None if not found
    fn find_user_by_oauth(
        &self,
        provider: &hkask_types::identity::OAuthProvider,
        provider_user_id: &str,
    ) -> UserResult<Option<(HumanUser, UserPod)>> {
        let provider_str = provider.to_string();
        let user_id: Option<String> = query_row(
            &*self.driver,
            "SELECT user_id FROM human_users WHERE oauth_provider = ?1 AND oauth_provider_user_id = ?2",
            &[
                DbValue::Text(provider_str),
                DbValue::Text(provider_user_id.to_string()),
            ],
            |row| Ok(row.get_str(0)?.to_string()),
        )?;
        match user_id {
            Some(uid_str) => {
                let uid = UserID::from_str(&uid_str).map_err(|e| {
                    UserStoreError::Infra(hkask_types::InfrastructureError::database(format!(
                        "Invalid user_id: {e}"
                    )))
                })?;
                let user = self.get_user(&uid)?;
                let primary = self.get_userpod_by_user(&uid)?.ok_or_else(|| {
                    UserStoreError::NotFound(NotFound {
                        entity_type: "userpod".to_string(),
                        id: "user userpod".to_string(),
                    })
                })?;
                Ok(Some((user, primary)))
            }
            None => Ok(None),
        }
    }
    /// Create a session and return it (used by OAuth flow and login).
    ///
    /// expect: "The system provides durable storage for archival data"
    /// pre:  identity is a valid UserPod
    /// post: returns a new UserSession with 7-day expiry
    pub fn create_oauth_session(&self, identity: &UserPod) -> UserResult<UserSession> {
        let session = self.create_session(identity)?;
        self.update_last_login(&identity.userpod_name)?;
        Ok(session)
    }
    /// Rename a userpod.
    ///
    /// expect: "The system provides durable storage for archival data"
    /// pre:  from_name exists; to_name does not exist
    /// post: userpod_identities.userpod_name updated
    pub fn rename_userpod(&self, from_name: &str, to_name: &str) -> UserResult<()> {
        let rows = self.driver.execute(
            "UPDATE userpod_identities SET userpod_name = ?1 WHERE userpod_name = ?2",
            &[
                DbValue::Text(to_name.to_string()),
                DbValue::Text(from_name.to_string()),
            ],
        )?;
        if rows == 0 {
            return Err(UserStoreError::NotFound(NotFound {
                entity_type: "userpod".to_string(),
                id: from_name.to_string(),
            }));
        }
        Ok(())
    }
    /// Delete a userpod and all its associated data.
    ///
    /// expect: "The system provides durable storage for archival data"
    /// pre:  userpod_name exists
    /// post: userpod_identities row deleted; sessions deleted
    pub fn delete_userpod(&self, userpod_name: &str) -> UserResult<()> {
        let rows = self.driver.execute(
            "DELETE FROM userpod_identities WHERE userpod_name = ?1",
            &[DbValue::Text(userpod_name.to_string())],
        )?;
        if rows == 0 {
            return Err(UserStoreError::NotFound(NotFound {
                entity_type: "userpod".to_string(),
                id: userpod_name.to_string(),
            }));
        }
        self.driver.execute(
            "DELETE FROM user_sessions WHERE userpod_name = ?1",
            &[DbValue::Text(userpod_name.to_string())],
        )?;
        Ok(())
    }
    /// Find a userpod by WebID.
    ///
    /// expect: "The system provides durable storage for archival data"
    /// pre:  webid is a valid WebID
    /// post: returns Some(UserPod) if found, None otherwise
    pub fn get_userpod_by_webid(&self, webid: &hkask_types::WebID) -> UserResult<Option<UserPod>> {
        let sql = format!("SELECT {USERPOD_COLUMNS} FROM userpod_identities WHERE webid = ?1");
        query_row(
            &*self.driver,
            &sql,
            &[DbValue::Text(webid.to_string())],
            userpod_from_row,
        )
        .map_err(|e| UserStoreError::Infra(InfrastructureError::from(e)))
    }
    /// Login a userpod with passphrase.
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// \[P1\] Motivating: User Sovereignty — authenticate userpod session
    /// pre:  userpod_name is registered, passphrase is correct
    /// post: returns UserSession on success
    /// post: returns Err if credentials invalid
    pub fn login(&self, userpod_name: &str, passphrase: &str) -> UserResult<UserSession> {
        let identity = self
            .get_userpod(userpod_name)?
            .ok_or(UserStoreError::NotFound(NotFound {
                entity_type: "userpod".to_string(),
                id: userpod_name.to_string(),
            }))?;
        let human = self.get_user(&identity.user_id)?;
        let verified = Self::verify_passphrase(passphrase, &human.passphrase_hash)?;
        if !verified {
            return Err(UserStoreError::InvalidCredentials);
        }
        // Check passphrase expiry BEFORE creating session
        if let Some(days_old) = self.check_passphrase_expiry(userpod_name, 60)? {
            tracing::warn!(
                userpod = %userpod_name,
                days_old,
                "Passphrase expired — user must change"
            );
            return Err(UserStoreError::PassphraseExpired(days_old));
        }
        let session = self.create_session(&identity)?;
        self.update_last_login(&identity.userpod_name)?;
        Ok(session)
    }
    /// Logout a session.
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// \[P1\] Motivating: User Sovereignty — invalidate session
    /// pre:  session_id is valid
    /// post: session invalidated
    pub fn logout(&self, session_id: &str) -> UserResult<()> {
        self.driver.execute(
            "DELETE FROM user_sessions WHERE session_id = ?1",
            &[DbValue::Text(session_id.to_string())],
        )?;
        Ok(())
    }
    /// Change a userpod's passphrase. Requires the old passphrase for verification.
    /// Change a userpod's passphrase.
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// \[P1\] Motivating: User Sovereignty — change userpod passphrase
    /// pre:  userpod_name is registered, old_passphrase is correct
    /// post: passphrase updated
    pub fn change_passphrase(
        &self,
        userpod_name: &str,
        old_passphrase: &str,
        new_passphrase: &str,
    ) -> UserResult<()> {
        let identity = self
            .get_userpod(userpod_name)?
            .ok_or(UserStoreError::NotFound(NotFound {
                entity_type: "userpod".to_string(),
                id: userpod_name.to_string(),
            }))?;
        let human = self.get_user(&identity.user_id)?;
        let verified = Self::verify_passphrase(old_passphrase, &human.passphrase_hash)?;
        if !verified {
            return Err(UserStoreError::InvalidCredentials);
        }
        // Hash new passphrase with existing salt and master_salt
        let new_hash = Self::hash_passphrase(new_passphrase, &human.salt)?;
        let now = chrono::Utc::now().timestamp();
        self.driver.execute(
            "UPDATE human_users SET passphrase_hash = ?1, passphrase_set_at = ?2 WHERE user_id = ?3",
            &[
                DbValue::Text(new_hash),
                DbValue::Integer(now),
                DbValue::Text(identity.user_id.to_string()),
            ],
        )?;
        // Invalidate all existing sessions for this userpod
        self.driver.execute(
            "DELETE FROM user_sessions WHERE userpod_name = ?1",
            &[DbValue::Text(userpod_name.to_string())],
        )?;
        Ok(())
    }
    /// Check if a userpod's passphrase is older than `max_age_days`.
    /// Returns `Some(days_old)` if expired, `None` if still valid or no timestamp.
    /// Check if a passphrase has expired.
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — detect passphrase rotation need
    /// pre:  userpod_name is registered
    /// post: returns true if passphrase needs rotation
    pub fn check_passphrase_expiry(
        &self,
        userpod_name: &str,
        max_age_days: i64,
    ) -> UserResult<Option<i64>> {
        let identity = self
            .get_userpod(userpod_name)?
            .ok_or(UserStoreError::NotFound(NotFound {
                entity_type: "userpod".to_string(),
                id: userpod_name.to_string(),
            }))?;
        let human = self.get_user(&identity.user_id)?;
        let set_at = match human.passphrase_set_at {
            Some(ts) => ts,
            None => return Ok(None),
        };
        let now = chrono::Utc::now().timestamp();
        let age_seconds = now - set_at;
        let age_days = age_seconds / 86400;
        if age_days > max_age_days {
            Ok(Some(age_days))
        } else {
            Ok(None)
        }
    }
    /// Get a session by ID.
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// \[P1\] Motivating: User Sovereignty — get session by ID
    /// pre:  session_id is non-empty
    /// post: returns Some(session) if valid, None otherwise
    #[must_use = "result must be used"]
    pub fn get_session(&self, session_id: &str) -> UserResult<Option<UserSession>> {
        let sql = format!("SELECT {SESSION_COLUMNS} FROM user_sessions WHERE session_id = ?1");
        query_row(
            &*self.driver,
            &sql,
            &[DbValue::Text(session_id.to_string())],
            |row| {
                session_from_row(row)
                    .map_err(|e| crate::database::types::DbError::Database(e.to_string()))
            },
        )
        .map_err(|e| UserStoreError::Infra(InfrastructureError::database(e.to_string())))
    }
    /// List sessions for a userpod.
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// \[P1\] Motivating: User Sovereignty — list active sessions
    /// pre:  userpod_name is non-empty
    /// post: returns Vec of active sessions
    #[must_use = "result must be used"]
    pub fn list_sessions(&self, userpod_name: &str) -> UserResult<Vec<UserSession>> {
        let sql = format!(
            "SELECT {SESSION_COLUMNS} FROM user_sessions WHERE userpod_name = ?1 ORDER BY last_active DESC"
        );
        query_map(
            &*self.driver,
            &sql,
            &[DbValue::Text(userpod_name.to_string())],
            |row| {
                session_from_row(row)
                    .map_err(|e| crate::database::types::DbError::Database(e.to_string()))
            },
        )
        .map_err(|e| UserStoreError::Infra(InfrastructureError::database(e.to_string())))
    }
    /// Get a userpod by name.
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// \[P1\] Motivating: User Sovereignty — get userpod by name
    /// pre:  userpod_name is non-empty
    /// post: returns Some(identity) if found, None otherwise
    #[must_use = "result must be used"]
    pub fn get_userpod(&self, userpod_name: &str) -> UserResult<Option<UserPod>> {
        let sql =
            format!("SELECT {USERPOD_COLUMNS} FROM userpod_identities WHERE userpod_name = ?1");
        query_row(
            &*self.driver,
            &sql,
            &[DbValue::Text(userpod_name.to_string())],
            |row| {
                userpod_from_row(row)
                    .map_err(|e| crate::database::types::DbError::Database(e.to_string()))
            },
        )
        .map_err(|e| UserStoreError::Infra(InfrastructureError::database(e.to_string())))
    }
    /// Get a human user by ID.
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// \[P1\] Motivating: User Sovereignty — get human user by ID
    /// pre:  user_id is valid
    /// post: returns HumanUser
    #[must_use = "result must be used"]
    pub fn get_user(&self, user_id: &UserID) -> UserResult<HumanUser> {
        query_row(
            &*self.driver,
            "SELECT user_id, email_enc, phone_enc, passphrase_hash, salt, master_salt, created_at, last_active, passphrase_set_at,
                    oauth_provider, oauth_provider_user_id, oauth_display_name, role
             FROM human_users WHERE user_id = ?1",
            &[DbValue::Text(user_id.to_string())],
            |row| {
                Ok(HumanUser {
                    user_id: *user_id,
                    email_enc: row.get_blob(1)?.to_vec(),
                    phone_enc: match row.get(2)? { DbValue::Null => None, v => Some(v.as_blob()?.to_vec()) },
                    passphrase_hash: row.get_str(3)?.to_string(),
                    salt: row.get_str(4)?.to_string(),
                    master_salt: row.get_str(5)?.to_string(),
                    created_at: row.get_int(6)?,
                    last_active: match row.get(7)? { DbValue::Null => None, v => Some(v.as_int()?) },
                    passphrase_set_at: match row.get(8)? { DbValue::Null => None, v => Some(v.as_int()?) },
                    oauth_provider: match row.get(9)? {
                        DbValue::Null => None,
                        v => Some(v.as_text()?.to_string().parse().ok()).flatten(),
                    },
                    oauth_provider_user_id: match row.get(10)? { DbValue::Null => None, v => Some(v.as_text()?.to_string()) },
                    oauth_display_name: match row.get(11)? { DbValue::Null => None, v => Some(v.as_text()?.to_string()) },
                    role: match row.get(12)? {
                        DbValue::Null => Role::Member,
                        v => v.as_text().ok().and_then(|s| s.parse().ok()).unwrap_or(Role::Member),
                    },
                })
            },
        )?
        .ok_or_else(|| UserStoreError::NotFound(NotFound {
            entity_type: "user".to_string(),
            id: user_id.as_uuid().to_string(),
        }))
    }
    /// Create an invite code for a new member.
    ///
    /// expect: "I can send an invite to bring a new user onto my server"
    /// \[P2\] Goal: Affirmative Consent — admin explicitly invites each member
    ///Constraining: User Sovereignty — invite expires and is revocable
    /// pre:  created_by is a valid UserID with Admin role
    /// post: invite row created with status Pending, 7-day expiry
    pub fn create_invite(&self, created_by: &UserID) -> UserResult<Invite> {
        let invite_id = uuid::Uuid::new_v4().to_string();
        let code = uuid::Uuid::new_v4().to_string().replace('-', "")[..12].to_string();
        let now = chrono::Utc::now().timestamp();
        let expires_at = now + 7 * 24 * 3600;
        self.driver.execute(
            "INSERT INTO invites (invite_id, created_by, code, status, created_at, expires_at)
             VALUES (?1, ?2, ?3, 'pending', ?4, ?5)",
            &[
                DbValue::Text(invite_id.clone()),
                DbValue::Text(created_by.to_string()),
                DbValue::Text(code.clone()),
                DbValue::Integer(now),
                DbValue::Integer(expires_at),
            ],
        )?;
        Ok(Invite {
            invite_id,
            created_by: *created_by,
            code,
            status: InviteStatus::Pending,
            created_at: now,
            expires_at,
            accepted_at: None,
            accepted_user_id: None,
        })
    }
    /// Look up an invite by code.
    ///
    /// expect: "I can look up an invite code to see if it's still valid"
    /// pre:  code is a valid invite code
    /// post: returns Some(Invite) if found and not expired, None otherwise
    pub fn lookup_invite(&self, code: &str) -> UserResult<Option<Invite>> {
        Ok(query_row(
            &*self.driver,
            "SELECT invite_id, created_by, code, status, created_at, expires_at, accepted_at, accepted_user_id
             FROM invites WHERE code = ?1",
            &[DbValue::Text(code.to_string())],
            |row| {
                Ok(Invite {
                    invite_id: row.get_str(0)?.to_string(),
                    created_by: UserID::from_str(row.get_str(1)?).map_err(|e| crate::database::types::DbError::Database(e.to_string()))?,
                    code: row.get_str(2)?.to_string(),
                    status: row.get_str(3)?.parse().unwrap_or(InviteStatus::Pending),
                    created_at: row.get_int(4)?,
                    expires_at: row.get_int(5)?,
                    accepted_at: match row.get(6)? { DbValue::Null => None, v => Some(v.as_int()?) },
                    accepted_user_id: match row.get(7)? {
                        DbValue::Null => None,
                        v => Some(UserID::from_str(v.as_text()?).map_err(|e| crate::database::types::DbError::Database(e.to_string()))?),
                    },
                })
            },
        )?)
    }
    /// Accept an invite, linking the accepting user to the invite.
    ///
    /// expect: "I can accept an invite to join a server"
    /// pre:  code is valid, invite is Pending and not expired
    /// post: invite status updated to Accepted, accepted_user_id set
    pub fn accept_invite(&self, code: &str, accepted_user_id: &UserID) -> UserResult<Invite> {
        let now = chrono::Utc::now().timestamp();
        let rows = self.driver.execute(
            "UPDATE invites SET status = 'accepted', accepted_at = ?1, accepted_user_id = ?2
             WHERE code = ?3 AND status = 'pending' AND expires_at > ?4",
            &[
                DbValue::Integer(now),
                DbValue::Text(accepted_user_id.to_string()),
                DbValue::Text(code.to_string()),
                DbValue::Integer(now),
            ],
        )?;
        if rows == 0 {
            return Err(UserStoreError::NotFound(NotFound {
                entity_type: "invite".to_string(),
                id: "Invite not found or expired".to_string(),
            }));
        }
        self.lookup_invite(code)?.ok_or_else(|| {
            UserStoreError::NotFound(NotFound {
                entity_type: "invite".to_string(),
                id: "Invite not found after accept".to_string(),
            })
        })
    }
    /// Revoke a pending invite.
    ///
    /// expect: "As an admin I can revoke an invite I've sent"
    /// pre:  code is a valid pending invite; revoker must match created_by (checked at API layer)
    /// post: invite status updated to 'revoked'; returns the updated Invite
    pub fn revoke_invite(&self, code: &str, revoked_by: &UserID) -> UserResult<Invite> {
        let rows = self.driver.execute(
            "UPDATE invites SET status = 'revoked'
             WHERE code = ?1 AND created_by = ?2 AND status = 'pending'",
            &[
                DbValue::Text(code.to_string()),
                DbValue::Text(revoked_by.to_string()),
            ],
        )?;
        if rows == 0 {
            return Err(UserStoreError::NotFound(NotFound {
                entity_type: "invite".to_string(),
                id: "Invite not found, already accepted, or not owned by you".to_string(),
            }));
        }
        self.lookup_invite(code)?.ok_or_else(|| {
            UserStoreError::NotFound(NotFound {
                entity_type: "invite".to_string(),
                id: "Invite not found after revoke".to_string(),
            })
        })
    }
    /// List all invites created by a user.
    ///
    /// expect: "I can see all the invites I've sent and their status"
    /// pre:  created_by is a valid UserID
    /// post: returns list of Invite records ordered by creation time (newest first)
    pub fn list_invites(&self, created_by: &UserID) -> UserResult<Vec<Invite>> {
        Ok(query_map(
            &*self.driver,
            "SELECT invite_id, created_by, code, status, created_at, expires_at, accepted_at, accepted_user_id
             FROM invites WHERE created_by = ?1 ORDER BY created_at DESC",
            &[DbValue::Text(created_by.to_string())],
            |row| {
                Ok(Invite {
                    invite_id: row.get_str(0)?.to_string(),
                    created_by: UserID::from_str(row.get_str(1)?).map_err(|e| crate::database::types::DbError::Database(e.to_string()))?,
                    code: row.get_str(2)?.to_string(),
                    status: row.get_str(3)?.parse().unwrap_or(InviteStatus::Pending),
                    created_at: row.get_int(4)?,
                    expires_at: row.get_int(5)?,
                    accepted_at: match row.get(6)? { DbValue::Null => None, v => Some(v.as_int()?) },
                    accepted_user_id: match row.get(7)? {
                        DbValue::Null => None,
                        v => Some(UserID::from_str(v.as_text()?).map_err(|e| crate::database::types::DbError::Database(e.to_string()))?),
                    },
                })
            },
        )?)
    }
    /// List all active sessions across all users (admin-only).
    ///
    /// expect: "As an admin I can see all active sessions on my server"
    /// pre:  caller must have Admin role (checked at middleware layer)
    /// post: returns list of UserSession records with active (non-expired) sessions
    pub fn list_all_sessions(&self) -> UserResult<Vec<UserSession>> {
        let now = chrono::Utc::now().timestamp();
        Ok(query_map(
            &*self.driver,
            "SELECT session_id, userpod_name, webid, user_id, session_key_salt, expires_at, last_active
             FROM user_sessions WHERE expires_at > ?1 ORDER BY last_active DESC",
            &[DbValue::Integer(now)],
            |row| {
                Ok(UserSession {
                    session_id: row.get_str(0)?.to_string(),
                    userpod_name: row.get_str(1)?.to_string(),
                    webid: WebID::from_str(row.get_str(2)?).map_err(|e| crate::database::types::DbError::Database(e.to_string()))?,
                    user_id: UserID::from_str(row.get_str(3)?).map_err(|e| crate::database::types::DbError::Database(e.to_string()))?,
                    session_key_salt: row.get_str(4)?.to_string(),
                    expires_at: row.get_int(5)?,
                    last_active: row.get_int(6)?,
                })
            },
        )?)
    }
    /// Set the role of a user (admin-only).
    ///
    /// expect: "As an admin I can promote a user to admin or demote to member"
    /// pre:  caller must have Admin role (checked at middleware layer)
    /// post: user's role updated in database
    pub fn set_user_role(
        &self,
        user_id: &UserID,
        role: hkask_types::identity::Role,
    ) -> UserResult<()> {
        let rows = self.driver.execute(
            "UPDATE human_users SET role = ?1 WHERE user_id = ?2",
            &[
                DbValue::Text(role.to_string()),
                DbValue::Text(user_id.to_string()),
            ],
        )?;
        if rows == 0 {
            return Err(UserStoreError::NotFound(NotFound {
                entity_type: "user".to_string(),
                id: user_id.as_uuid().to_string(),
            }));
        }
        Ok(())
    }
    /// List all human users with minimal info for admin display.
    ///
    /// Returns (user_id, role, display_name, created_at, last_active) for each user.
    /// Does not return encrypted PII or auth credentials.
    #[allow(clippy::type_complexity)]
    pub fn list_all_users_summary(
        &self,
    ) -> UserResult<Vec<(UserID, String, String, i64, Option<i64>)>> {
        Ok(query_map(
            &*self.driver,
            "SELECT user_id, role, oauth_display_name, created_at, last_active FROM human_users ORDER BY created_at ASC",
            &[],
            |row| {
                Ok((
                    UserID::from_str(row.get_str(0)?)
                        .map_err(|e| crate::database::types::DbError::Database(e.to_string()))?,
                    row.get_str(1)?.to_string(),
                    match row.get(2)? {
                        DbValue::Null => "Unknown".to_string(),
                        v => v.as_text()?.to_string(),
                    },
                    row.get_int(3)?,
                    match row.get(4)? {
                        DbValue::Null => None,
                        v => Some(v.as_int()?),
                    },
                ))
            },
        )?)
    }
    /// Get the userpod for a user (1:1 relationship).
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// \[P1\] Motivating: User Sovereignty — get user's userpod
    /// pre:  user_id is valid
    /// post: returns Some(UserPod) if found, None otherwise
    #[must_use = "result must be used"]
    pub fn get_userpod_by_user(&self, user_id: &UserID) -> UserResult<Option<UserPod>> {
        let sql = format!("SELECT {USERPOD_COLUMNS} FROM userpod_identities WHERE user_id = ?1");
        query_row(
            &*self.driver,
            &sql,
            &[DbValue::Text(user_id.to_string())],
            |row| {
                userpod_from_row(row)
                    .map_err(|e| crate::database::types::DbError::Database(e.to_string()))
            },
        )
        .map_err(|e| UserStoreError::Infra(InfrastructureError::database(e.to_string())))
    }

    /// List all userpods across all users.
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// \[P1\] Motivating: User Sovereignty — list all userpods
    /// pre:  (none)
    /// post: returns Vec of all userpods ordered by creation time
    #[must_use = "result must be used"]
    pub fn list_userpods(&self) -> UserResult<Vec<UserPod>> {
        let sql =
            format!("SELECT {USERPOD_COLUMNS} FROM userpod_identities ORDER BY created_at ASC");
        query_map(&*self.driver, &sql, &[], |row| {
            userpod_from_row(row)
                .map_err(|e| crate::database::types::DbError::Database(e.to_string()))
        })
        .map_err(|e| UserStoreError::Infra(InfrastructureError::database(e.to_string())))
    }
    /// Get the wallet ID for a userpod.
    /// Get wallet ID for a userpod.
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// \[P1\] Motivating: User Sovereignty — get wallet ID for userpod
    /// pre:  userpod_name is non-empty
    /// post: returns Some(WalletId) if set, None otherwise
    pub fn get_wallet_id(&self, userpod_name: &str) -> UserResult<Option<WalletId>> {
        let identity = self
            .get_userpod(userpod_name)?
            .ok_or(UserStoreError::NotFound(NotFound {
                entity_type: "userpod".to_string(),
                id: userpod_name.to_string(),
            }))?;
        Ok(identity.wallet_id)
    }
    /// Set the wallet ID for a userpod (called during onboarding after wallet creation).
    /// Set wallet ID for a userpod.
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// \[P1\] Motivating: User Sovereignty — set wallet ID for userpod
    /// pre:  userpod_name is registered, wallet_id is valid
    /// post: wallet_id stored for userpod
    pub fn set_wallet_id(&self, userpod_name: &str, wallet_id: WalletId) -> UserResult<()> {
        let rows = self.driver.execute(
            "UPDATE userpod_identities SET wallet_id = ?1 WHERE userpod_name = ?2",
            &[
                DbValue::Text(wallet_id.to_string()),
                DbValue::Text(userpod_name.to_string()),
            ],
        )?;
        if rows == 0 {
            return Err(UserStoreError::NotFound(NotFound {
                entity_type: "userpod".to_string(),
                id: userpod_name.to_string(),
            }));
        }
        Ok(())
    }

    // ── Private helpers ──

    /// Execute operations on a raw SQLite connection with a transaction.
    /// Uses the driver's typed `sqlite_pool()` method to acquire a connection.
    fn with_raw_conn<T, F>(&self, f: F) -> UserResult<T>
    where
        F: FnOnce(&rusqlite::Connection) -> Result<T, UserStoreError>,
    {
        if let Some(pool) = self.driver.sqlite_pool() {
            let conn = pool
                .get()
                .map_err(|e| UserStoreError::Infra(InfrastructureError::database(e.to_string())))?;
            let result = f(&conn)?;
            Ok(result)
        } else {
            Err(UserStoreError::Infra(InfrastructureError::database(
                "UserStore requires SQLite driver for transaction support",
            )))
        }
    }

    fn create_session(&self, identity: &UserPod) -> UserResult<UserSession> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let session_key_salt = Self::generate_salt();
        let now = chrono::Utc::now().timestamp();
        let expires_at = now + 86400 * 7;
        self.driver.execute(
            "INSERT INTO user_sessions
             (session_id, userpod_name, webid, user_id, session_key_salt, expires_at, last_active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            &[
                DbValue::Text(session_id.clone()),
                DbValue::Text(identity.userpod_name.clone()),
                DbValue::Text(identity.webid.to_string()),
                DbValue::Text(identity.user_id.to_string()),
                DbValue::Text(session_key_salt.clone()),
                DbValue::Integer(expires_at),
                DbValue::Integer(now),
            ],
        )?;
        Ok(UserSession {
            session_id,
            userpod_name: identity.userpod_name.clone(),
            webid: identity.webid,
            user_id: identity.user_id,
            session_key_salt,
            expires_at,
            last_active: now,
        })
    }
    fn update_last_login(&self, userpod_name: &str) -> UserResult<()> {
        self.driver.execute(
            "UPDATE userpod_identities SET last_login = ?1 WHERE userpod_name = ?2",
            &[
                DbValue::Integer(chrono::Utc::now().timestamp()),
                DbValue::Text(userpod_name.to_string()),
            ],
        )?;
        Ok(())
    }
    fn generate_salt() -> String {
        let mut salt = [0u8; 16];
        rand::rng().fill_bytes(&mut salt);
        hex::encode(salt)
    }
    fn hash_passphrase(passphrase: &str, salt: &str) -> UserResult<String> {
        use argon2::password_hash::SaltString;
        use argon2::{Algorithm, Argon2, Params, Version};
        let salt_bytes = hex::decode(salt)
            .map_err(|e| UserStoreError::KeyDerivation(format!("Invalid salt hex: {}", e)))?;
        let salt_string = SaltString::from_b64(
            &base64::engine::general_purpose::STANDARD_NO_PAD.encode(&salt_bytes),
        )
        .map_err(|e| UserStoreError::KeyDerivation(format!("Salt error: {}", e)))?;
        let params = Params::new(19456, 2, 1, None)
            .map_err(|e| UserStoreError::KeyDerivation(e.to_string()))?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let password_hash = argon2
            .hash_password(passphrase.as_bytes(), &salt_string)
            .map_err(|e| UserStoreError::PasswordHash(e.to_string()))?;
        Ok(password_hash.to_string())
    }
    fn verify_passphrase(passphrase: &str, hash: &str) -> UserResult<bool> {
        let parsed_hash =
            PasswordHash::new(hash).map_err(|e| UserStoreError::PasswordHash(e.to_string()))?;
        match argon2::Argon2::default().verify_password(passphrase.as_bytes(), &parsed_hash) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
    pub(crate) fn derive_pii_key(
        passphrase: &str,
        master_salt: &str,
    ) -> UserResult<Zeroizing<[u8; 32]>> {
        use hkask_keystore::encryption::derive_key;
        derive_key(
            passphrase,
            &hex::decode(master_salt).map_err(|e| UserStoreError::KeyDerivation(e.to_string()))?,
        )
        .map_err(|e| UserStoreError::KeyDerivation(e.to_string()))
    }
    pub(crate) fn encrypt_pii(plaintext: &[u8], key: &Zeroizing<[u8; 32]>) -> UserResult<Vec<u8>> {
        use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
        let cipher = Aes256Gcm::new_from_slice(&**key)
            .map_err(|e| UserStoreError::Encryption(e.to_string()))?;
        let mut nonce_bytes = [0u8; 12];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| UserStoreError::Encryption(e.to_string()))?;
        let mut result = nonce_bytes.to_vec();
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }
}

fn sanitize_userpod_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
