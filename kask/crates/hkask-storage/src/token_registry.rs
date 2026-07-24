//! Token Registry — SQLite persistence for DelegationToken lifecycle
//!
//! Persists Ed25519-signed OCAP delegation tokens so the platform can
//! audit consent (P2 — Affirmative Consent). OCAP gates enforce consent
//! at runtime; this store proves it after the fact.
//!
//! Regulation spans record token *usage*; this store records token *issuance*.
//! Together they enable the full consent audit picture.

use crate::database::driver::{query_map, query_row};
use crate::database::value::DbValue;
use crate::define_driver_store;
use hkask_capability::{
    DelegationAction, DelegationResource, DelegationToken, TokenRegistry, TokenRegistryError,
};
use hkask_types::WebID;

define_driver_store!(TokenRegistryStore);

impl TokenRegistryStore {
    /// Initialize the delegation_tokens table.
    ///
    /// expect: "Token issuance is persisted for consent audit"
    /// `[P2]` Motivating: Affirmative Consent — audit trail for delegation tokens
    /// post: delegation_tokens table created if not exists
    fn init_schema(driver: &std::sync::Arc<dyn crate::database::driver::DatabaseDriver>) {
        let _ = driver.execute_batch(
                "CREATE TABLE IF NOT EXISTS delegation_tokens (
                id TEXT PRIMARY KEY,
                resource TEXT NOT NULL,
                resource_id TEXT NOT NULL,
                action TEXT NOT NULL,
                delegated_from TEXT NOT NULL,
                delegated_to TEXT NOT NULL,
                signature_hex TEXT NOT NULL,
                public_key_hex TEXT NOT NULL,
                expires_at INTEGER,
                attenuation_level INTEGER NOT NULL DEFAULT 0,
                max_attenuation INTEGER NOT NULL DEFAULT 7,
                context_nonce TEXT NOT NULL DEFAULT '',
                revoked INTEGER NOT NULL DEFAULT 0,
                issued_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE INDEX IF NOT EXISTS idx_tokens_issuer ON delegation_tokens(delegated_from, issued_at);
            CREATE INDEX IF NOT EXISTS idx_tokens_recipient ON delegation_tokens(delegated_to, issued_at);
            CREATE INDEX IF NOT EXISTS idx_tokens_revoked ON delegation_tokens(revoked);
            "
        );
    }

    fn row_to_token(
        row: &crate::database::value::DbRow,
    ) -> Result<DelegationToken, crate::database::types::DbError> {
        let id: String = row.get_str_named("id")?.to_string();
        let resource_str: String = row.get_str_named("resource")?.to_string();
        let resource_id: String = row.get_str_named("resource_id")?.to_string();
        let action_str: String = row.get_str_named("action")?.to_string();
        let from_str: String = row.get_str_named("delegated_from")?.to_string();
        let to_str: String = row.get_str_named("delegated_to")?.to_string();
        let sig_hex: String = row.get_str_named("signature_hex")?.to_string();
        let pk_hex: String = row.get_str_named("public_key_hex")?.to_string();
        let expires_at: Option<i64> = match row.get_named("expires_at")? {
            DbValue::Null => None,
            v => Some(v.as_int()?),
        };
        let attenuation_level: i64 = row.get_int_named("attenuation_level")?;
        let max_attenuation: i64 = row.get_int_named("max_attenuation")?;
        let context_nonce: String = row.get_str_named("context_nonce")?.to_string();

        let resource = match resource_str.as_str() {
            "tool" => DelegationResource::Tool,
            "template" => DelegationResource::Template,
            "registry" | "memory" => DelegationResource::Registry,
            "key" => DelegationResource::Key,
            other => DelegationResource::parse_str(other).ok_or_else(|| {
                crate::database::types::DbError::Database(format!(
                    "unknown delegation resource: {other}"
                ))
            })?,
        };
        let action = DelegationAction::parse_str(&action_str).ok_or_else(|| {
            crate::database::types::DbError::Database(format!(
                "unknown delegation action: {action_str}"
            ))
        })?;

        let signature = {
            let bytes = hex::decode(&sig_hex).map_err(|e| {
                crate::database::types::DbError::Database(format!("invalid signature hex: {e}"))
            })?;
            let mut arr = [0u8; 64];
            let len = bytes.len().min(64);
            arr[..len].copy_from_slice(&bytes[..len]);
            hkask_capability::token_types::TokenSignature(arr)
        };
        let public_key = {
            let bytes = hex::decode(&pk_hex).map_err(|e| {
                crate::database::types::DbError::Database(format!("invalid public key hex: {e}"))
            })?;
            let mut arr = [0u8; 32];
            let len = bytes.len().min(32);
            arr[..len].copy_from_slice(&bytes[..len]);
            hkask_types::Ed25519PublicKey(arr)
        };

        let from_wid: WebID = from_str.parse().map_err(|e| {
            crate::database::types::DbError::Database(format!(
                "invalid delegated_from WebID '{from_str}': {e}"
            ))
        })?;
        let to_wid: WebID = to_str.parse().map_err(|e| {
            crate::database::types::DbError::Database(format!(
                "invalid delegated_to WebID '{to_str}': {e}"
            ))
        })?;

        Ok(DelegationToken {
            id,
            resource,
            resource_id,
            action,
            delegated_from: from_wid,
            delegated_to: to_wid,
            signature,
            public_key,
            expires_at,
            attenuation_level: attenuation_level as u8,
            max_attenuation: max_attenuation as u8,
            context_nonce,
            caveats: vec![],
        })
    }
}

impl TokenRegistry for TokenRegistryStore {
    fn store(&self, token: &DelegationToken) -> Result<(), TokenRegistryError> {
        let sig_hex = hex::encode(token.signature.0);
        let pk_hex = hex::encode(token.public_key.0);
        let resource_str = token.resource.as_str();
        let action_str = token.action.as_str();

        self.driver
            .execute(
                "INSERT INTO delegation_tokens
             (id, resource, resource_id, action, delegated_from, delegated_to,
              signature_hex, public_key_hex, expires_at, attenuation_level,
              max_attenuation, context_nonce, issued_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, unixepoch())",
                &[
                    DbValue::Text(token.id.clone()),
                    DbValue::Text(resource_str.to_string()),
                    DbValue::Text(token.resource_id.clone()),
                    DbValue::Text(action_str.to_string()),
                    DbValue::Text(token.delegated_from.to_string()),
                    DbValue::Text(token.delegated_to.to_string()),
                    DbValue::Text(sig_hex),
                    DbValue::Text(pk_hex),
                    token.expires_at.map_or(DbValue::Null, DbValue::Integer),
                    DbValue::Integer(token.attenuation_level as i64),
                    DbValue::Integer(token.max_attenuation as i64),
                    DbValue::Text(token.context_nonce.clone()),
                ],
            )
            .map_err(|e| {
                if e.to_string().contains("UNIQUE constraint") {
                    TokenRegistryError::Duplicate(token.id.clone())
                } else {
                    TokenRegistryError::Storage(e.to_string())
                }
            })?;
        Ok(())
    }

    fn get(&self, token_id: &str) -> Result<Option<DelegationToken>, TokenRegistryError> {
        query_row(
            &*self.driver,
            "SELECT * FROM delegation_tokens WHERE id = ?1",
            &[DbValue::Text(token_id.to_string())],
            Self::row_to_token,
        )
        .map_err(|e| TokenRegistryError::Storage(e.to_string()))
    }

    fn query_by_issuer(
        &self,
        webid: &WebID,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<DelegationToken>, TokenRegistryError> {
        let since_ts = since.timestamp();
        query_map(
            &*self.driver,
            "SELECT * FROM delegation_tokens WHERE delegated_from = ?1 AND issued_at >= ?2 AND revoked = 0",
            &[
                DbValue::Text(webid.to_string()),
                DbValue::Integer(since_ts),
            ],
            Self::row_to_token,
        )
        .map_err(|e| TokenRegistryError::Storage(e.to_string()))
    }

    fn query_by_recipient(
        &self,
        webid: &WebID,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<DelegationToken>, TokenRegistryError> {
        let since_ts = since.timestamp();
        query_map(
            &*self.driver,
            "SELECT * FROM delegation_tokens WHERE delegated_to = ?1 AND issued_at >= ?2 AND revoked = 0",
            &[
                DbValue::Text(webid.to_string()),
                DbValue::Integer(since_ts),
            ],
            Self::row_to_token,
        )
        .map_err(|e| TokenRegistryError::Storage(e.to_string()))
    }

    fn query_all(
        &self,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<DelegationToken>, TokenRegistryError> {
        let since_ts = since.timestamp();
        query_map(
            &*self.driver,
            "SELECT * FROM delegation_tokens WHERE issued_at >= ?1 AND revoked = 0",
            &[DbValue::Integer(since_ts)],
            Self::row_to_token,
        )
        .map_err(|e| TokenRegistryError::Storage(e.to_string()))
    }

    fn revoke(&self, token_id: &str) -> Result<(), TokenRegistryError> {
        let affected = self
            .driver
            .execute(
                "UPDATE delegation_tokens SET revoked = 1 WHERE id = ?1",
                &[DbValue::Text(token_id.to_string())],
            )
            .map_err(|e| TokenRegistryError::Storage(e.to_string()))?;
        if affected == 0 {
            return Err(TokenRegistryError::NotFound(hkask_types::NotFound {
                entity_type: "token".to_string(),
                id: token_id.to_string(),
            }));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::sqlite::SqliteDriver;
    use hkask_capability::DelegationAction;
    use hkask_capability::DelegationResource;
    use hkask_capability::token_types::TokenSignature;
    use hkask_types::WebID;
    use std::sync::Arc;

    fn test_token(from: WebID, to: WebID) -> DelegationToken {
        let id = format!(
            "tok-{}-{}",
            from.to_string().chars().take(8).collect::<String>(),
            uuid::Uuid::new_v4()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        );
        DelegationToken {
            id,
            resource: DelegationResource::Registry,
            resource_id: "test-skill".into(),
            action: DelegationAction::Execute,
            delegated_from: from,
            delegated_to: to,
            signature: TokenSignature([0u8; 64]),
            public_key: hkask_types::Ed25519PublicKey([0u8; 32]),
            expires_at: None,
            attenuation_level: 0,
            max_attenuation: 7,
            context_nonce: "test".into(),
            caveats: vec![],
        }
    }

    fn test_db() -> TokenRegistryStore {
        let pool = SqliteDriver::in_memory_pool().expect("in-memory SQLite pool");
        let driver = SqliteDriver::new(pool);
        TokenRegistryStore::from_driver(Arc::new(driver))
    }

    #[test]
    fn store_and_retrieve_token() {
        let store = test_db();
        let from = WebID::from_persona(b"issuer");
        let to = WebID::from_persona(b"recipient");
        let token = test_token(from, to);
        let token_id = token.id.clone();

        store.store(&token).unwrap();
        let retrieved = store.get(&token_id).unwrap().expect("token should exist");

        assert_eq!(retrieved.id, token.id);
        assert_eq!(retrieved.delegated_from.to_string(), from.to_string());
        assert_eq!(retrieved.delegated_to.to_string(), to.to_string());
    }

    #[test]
    fn query_by_issuer_filters_correctly() {
        let store = test_db();
        let alice = WebID::from_persona(b"alice");
        let bob = WebID::from_persona(b"bob");
        let carol = WebID::from_persona(b"carol");

        store.store(&test_token(alice, bob)).unwrap();
        store.store(&test_token(alice, carol)).unwrap();
        store.store(&test_token(bob, carol)).unwrap();

        let alice_tokens = store
            .query_by_issuer(&alice, chrono::Utc::now() - chrono::Duration::hours(1))
            .unwrap();
        assert_eq!(alice_tokens.len(), 2);

        let bob_tokens = store
            .query_by_issuer(&bob, chrono::Utc::now() - chrono::Duration::hours(1))
            .unwrap();
        assert_eq!(bob_tokens.len(), 1);
    }

    #[test]
    fn query_by_recipient_filters_correctly() {
        let store = test_db();
        let alice = WebID::from_persona(b"alice");
        let bob = WebID::from_persona(b"bob");
        let carol = WebID::from_persona(b"carol");

        store.store(&test_token(alice, bob)).unwrap();
        store.store(&test_token(carol, bob)).unwrap();

        let bob_tokens = store
            .query_by_recipient(&bob, chrono::Utc::now() - chrono::Duration::hours(1))
            .unwrap();
        assert_eq!(bob_tokens.len(), 2);
    }

    #[test]
    fn revoke_removes_from_queries() {
        let store = test_db();
        let alice = WebID::from_persona(b"alice");
        let bob = WebID::from_persona(b"bob");
        let token = test_token(alice, bob);
        let token_id = token.id.clone();

        store.store(&token).unwrap();
        store.revoke(&token_id).unwrap();

        let results = store
            .query_all(chrono::Utc::now() - chrono::Duration::hours(1))
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn duplicate_store_returns_error() {
        let store = test_db();
        let alice = WebID::from_persona(b"alice");
        let bob = WebID::from_persona(b"bob");
        let token = test_token(alice, bob);

        store.store(&token).unwrap();
        let result = store.store(&token);
        assert!(result.is_err());
    }

    #[test]
    fn revoke_nonexistent_returns_not_found() {
        let store = test_db();
        let result = store.revoke("nonexistent");
        assert!(matches!(result, Err(TokenRegistryError::NotFound(_))));
    }
}
