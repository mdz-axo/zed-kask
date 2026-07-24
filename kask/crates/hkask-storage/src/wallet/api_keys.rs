use super::WalletStore;
use crate::database::driver::{query_map, query_row};
use crate::database::value::DbValue;
use hkask_types::time::now_rfc3339;
use hkask_types::{ApiKeyCapability, ChainId, PrivacyMode, RJoule, RateLimitConfig, WalletError};
use hkask_types::{ApiKeyId, Ed25519PublicKey, InfrastructureError, WalletId};
use std::str::FromStr;

// ── Row type for query mapping ─────────────────────────────────────────────────

#[allow(dead_code)] // fields populated by query mapping
struct ApiKeyRow {
    key_id: String,
    wallet_id: String,
    public_key: Vec<u8>,
    spending_limit_rj: i64,
    spent_rj: i64,
    scope: String,
    purpose: String,
    rate_limit_json: Option<String>,
    privacy_mode: String,
    preferred_chain: Option<String>,
    expires_at: Option<String>,
    issued_at: String,
}

// ── API Key methods ────────────────────────────────────────────────────────────

impl WalletStore {
    /// Store a newly issued API key capability.
    /// Store an API key capability.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — store API key capability
    /// pre:  capability has valid key_id and wallet_id
    /// post: API key stored
    pub fn store_api_key(&self, capability: &ApiKeyCapability) -> Result<(), WalletError> {
        let scope_json =
            serde_json::to_string(&capability.scope).unwrap_or_else(|_| "[]".to_string());
        let rate_limit_json = capability
            .rate_limit
            .as_ref()
            .and_then(|rl| serde_json::to_string(rl).ok());
        self.driver.execute(
            "INSERT INTO api_keys (key_id, wallet_id, public_key, spending_limit_rj, spent_rj, scope, purpose, rate_limit_json, privacy_mode, preferred_chain, expires_at, issued_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            &[
                DbValue::Text(capability.key_id.to_string()),
                DbValue::Text(capability.wallet_id.to_string()),
                DbValue::Blob(capability.public_key.as_bytes().to_vec()),
                DbValue::Integer(capability.spending_limit_rj.as_u64() as i64),
                DbValue::Integer(capability.spent_rj.as_u64() as i64),
                DbValue::Text(scope_json),
                DbValue::Text(capability.purpose.clone()),
                rate_limit_json.map_or(DbValue::Null, DbValue::Text),
                DbValue::Text(capability.privacy_mode.to_string()),
                capability.preferred_chain.map_or(DbValue::Null, |c| DbValue::Text(c.to_string())),
                capability.expiry.map_or(DbValue::Null, |e| DbValue::Text(e.to_rfc3339())),
                DbValue::Text(capability.issued_at.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    /// Look up an API key by its ID.
    /// Get an API key by key ID.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — get API key by ID
    /// pre:  key_id is valid
    /// post: returns Some(capability) if found, None otherwise
    #[must_use = "result must be used"]
    pub fn get_api_key(&self, key_id: ApiKeyId) -> Result<Option<ApiKeyCapability>, WalletError> {
        Ok(
            query_map(
                &*self.driver,
                "SELECT key_id, wallet_id, public_key, spending_limit_rj, spent_rj, scope, purpose, rate_limit_json, privacy_mode, preferred_chain, expires_at, issued_at FROM api_keys WHERE key_id = ?1",
                &[DbValue::Text(key_id.to_string())],
                |row| {
                    let r = ApiKeyRow {
                        key_id: row.get_str(0)?.to_string(),
                        wallet_id: row.get_str(1)?.to_string(),
                        public_key: row.get_blob(2)?.to_vec(),
                        spending_limit_rj: row.get_int(3)?,
                        spent_rj: row.get_int(4)?,
                        scope: row.get_str(5)?.to_string(),
                        purpose: row.get_str(6)?.to_string(),
                        rate_limit_json: match row.get(7)? { DbValue::Null => None, v => Some(v.as_text()?.to_string()) },
                        privacy_mode: row.get_str(8)?.to_string(),
                        preferred_chain: match row.get(9)? { DbValue::Null => None, v => Some(v.as_text()?.to_string()) },
                        expires_at: match row.get(10)? { DbValue::Null => None, v => Some(v.as_text()?.to_string()) },
                        issued_at: row.get_str(11)?.to_string(),
                    };
                    row_to_api_key_capability(r).map_err(|e| crate::database::types::DbError::Database(e.to_string()))
                },
            )?
            .into_iter()
            .next(),
        )
    }

    /// Look up an API key by its Ed25519 public key (for Bearer token auth).
    /// Get an API key by public key.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — get API key by public key
    /// pre:  public_key is valid
    /// post: returns Some(capability) if found, None otherwise
    pub fn get_api_key_by_public_key(
        &self,
        public_key: &[u8],
    ) -> Result<Option<ApiKeyCapability>, WalletError> {
        Ok(
            query_map(
                &*self.driver,
                "SELECT key_id, wallet_id, public_key, spending_limit_rj, spent_rj, scope, purpose, rate_limit_json, privacy_mode, preferred_chain, expires_at, issued_at FROM api_keys WHERE public_key = ?1 AND revoked_at IS NULL",
                &[DbValue::Blob(public_key.to_vec())],
                |row| {
                    let r = ApiKeyRow {
                        key_id: row.get_str(0)?.to_string(),
                        wallet_id: row.get_str(1)?.to_string(),
                        public_key: row.get_blob(2)?.to_vec(),
                        spending_limit_rj: row.get_int(3)?,
                        spent_rj: row.get_int(4)?,
                        scope: row.get_str(5)?.to_string(),
                        purpose: row.get_str(6)?.to_string(),
                        rate_limit_json: match row.get(7)? { DbValue::Null => None, v => Some(v.as_text()?.to_string()) },
                        privacy_mode: row.get_str(8)?.to_string(),
                        preferred_chain: match row.get(9)? { DbValue::Null => None, v => Some(v.as_text()?.to_string()) },
                        expires_at: match row.get(10)? { DbValue::Null => None, v => Some(v.as_text()?.to_string()) },
                        issued_at: row.get_str(11)?.to_string(),
                    };
                    row_to_api_key_capability(r).map_err(|e| crate::database::types::DbError::Database(e.to_string()))
                },
            )?
            .into_iter()
            .next(),
        )
    }

    /// List all active (non-revoked) API keys for a wallet.
    /// List API keys for a wallet.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — list API keys
    /// pre:  wallet_id is valid
    /// post: returns Vec of API key capabilities
    #[must_use = "result must be used"]
    pub fn list_api_keys(&self, wallet_id: WalletId) -> Result<Vec<ApiKeyCapability>, WalletError> {
        Ok(query_map(
            &*self.driver,
            "SELECT key_id, wallet_id, public_key, spending_limit_rj, spent_rj, scope, purpose, rate_limit_json, privacy_mode, preferred_chain, expires_at, issued_at FROM api_keys WHERE wallet_id = ?1 AND revoked_at IS NULL ORDER BY issued_at DESC",
            &[DbValue::Text(wallet_id.to_string())],
            |row| {
                let r = ApiKeyRow {
                    key_id: row.get_str(0)?.to_string(),
                    wallet_id: row.get_str(1)?.to_string(),
                    public_key: row.get_blob(2)?.to_vec(),
                    spending_limit_rj: row.get_int(3)?,
                    spent_rj: row.get_int(4)?,
                    scope: row.get_str(5)?.to_string(),
                    purpose: row.get_str(6)?.to_string(),
                    rate_limit_json: match row.get(7)? {
                        DbValue::Null => None,
                        v => Some(v.as_text()?.to_string()),
                    },
                    privacy_mode: row.get_str(8)?.to_string(),
                    preferred_chain: match row.get(9)? {
                        DbValue::Null => None,
                        v => Some(v.as_text()?.to_string()),
                    },
                    expires_at: match row.get(10)? {
                        DbValue::Null => None,
                        v => Some(v.as_text()?.to_string()),
                    },
                    issued_at: row.get_str(11)?.to_string(),
                };
                row_to_api_key_capability(r)
                    .map_err(|e| crate::database::types::DbError::Database(e.to_string()))
            },
        )?)
    }

    /// Revoke an API key. Returns unspent rJoules to the wallet.
    /// Idempotent — revoking an already-revoked key is a no-op.
    /// Revoke an API key.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — revoke API key
    /// pre:  key_id is valid
    /// post: API key revoked, unspent rJ returned to wallet
    pub fn revoke_api_key(&self, key_id: ApiKeyId) -> Result<(), WalletError> {
        let now = now_rfc3339();
        let rows = self.driver.execute(
            "UPDATE api_keys SET revoked_at = ?1 WHERE key_id = ?2 AND revoked_at IS NULL",
            &[
                DbValue::Text(now.clone()),
                DbValue::Text(key_id.to_string()),
            ],
        )?;
        if rows == 0 {
            return Ok(()); // already revoked or doesn't exist — no-op
        }
        // Return unspent rJoules to wallet
        let (wallet_id_str, spent, limit): (String, i64, i64) = match query_row(
            &*self.driver,
            "SELECT wallet_id, spent_rj, spending_limit_rj FROM api_keys WHERE key_id = ?1",
            &[DbValue::Text(key_id.to_string())],
            |row| {
                Ok((
                    row.get_str(0)?.to_string(),
                    row.get_int(1)?,
                    row.get_int(2)?,
                ))
            },
        )? {
            Some(r) => r,
            None => return Ok(()),
        };
        let unspent = limit - spent;
        if unspent > 0 {
            self.driver.execute(
                "UPDATE wallet_balances SET balance_rj = balance_rj + ?1, updated_at = ?2 WHERE wallet_id = ?3",
                &[
                    DbValue::Integer(unspent),
                    DbValue::Text(now),
                    DbValue::Text(wallet_id_str),
                ],
            )?;
        }
        Ok(())
    }

    /// Update the spent_rj counter on an API key (called after each tool invocation).
    /// Update spent rJoules for an API key.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — update spent rJ for key
    /// pre:  key_id is valid
    /// post: spent_rj updated
    pub fn update_spent_rj(&self, key_id: ApiKeyId, spent: RJoule) -> Result<(), WalletError> {
        self.driver.execute(
            "UPDATE api_keys SET spent_rj = ?1 WHERE key_id = ?2",
            &[
                DbValue::Integer(spent.as_u64() as i64),
                DbValue::Text(key_id.to_string()),
            ],
        )?;
        Ok(())
    }
}

// ── Row conversion helper ──────────────────────────────────────────────────────

fn row_to_api_key_capability(r: ApiKeyRow) -> Result<ApiKeyCapability, WalletError> {
    let public_key_bytes: [u8; 32] = r.public_key.try_into().map_err(|_| {
        WalletError::Infra(InfrastructureError::database("public_key must be 32 bytes"))
    })?;
    let scope: Vec<String> = serde_json::from_str(&r.scope).unwrap_or_default();
    let rate_limit: Option<RateLimitConfig> = r
        .rate_limit_json
        .as_deref()
        .and_then(|j| serde_json::from_str(j).ok());
    let privacy_mode = PrivacyMode::from_str(&r.privacy_mode)
        .map_err(|e| WalletError::Infra(InfrastructureError::database(e)))?;
    let preferred_chain = r
        .preferred_chain
        .as_deref()
        .map(ChainId::from_str)
        .transpose()
        .map_err(|e| WalletError::Infra(InfrastructureError::database(e)))?;
    let expiry = r
        .expires_at
        .map(|e| {
            chrono::DateTime::parse_from_rfc3339(&e)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|e| {
                    WalletError::Infra(InfrastructureError::database(format!(
                        "Invalid expiry timestamp: {e}"
                    )))
                })
        })
        .transpose()?;
    let issued_at = chrono::DateTime::parse_from_rfc3339(&r.issued_at)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|e| {
            WalletError::Infra(InfrastructureError::database(format!(
                "Invalid issued_at timestamp: {e}"
            )))
        })?;
    Ok(ApiKeyCapability {
        wallet_id: WalletId::from_str(&r.wallet_id)?,
        key_id: ApiKeyId::from_str(&r.key_id)?,
        public_key: Ed25519PublicKey(public_key_bytes),
        spending_limit_rj: RJoule::new(r.spending_limit_rj as u64),
        spent_rj: RJoule::new(r.spent_rj as u64),
        scope,
        purpose: r.purpose,
        rate_limit,
        expiry,
        issued_at,
        privacy_mode,
        preferred_chain,
    })
}
