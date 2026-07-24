//! Token issuance and management — DelegationToken lifecycle.
//!
//! `kask token issue` — issue an Ed25519-signed DelegationToken for a userpod
//! `kask token list` — list tokens issued for a userpod
//! `kask token revoke` — revoke a token by ID

use crate::cli::TokenAction;
use crate::error::CliError;
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_types::WebID;

/// Parse a human-readable TTL string into seconds.
/// Supports: "30s", "5m", "24h", "7d".
fn parse_ttl(ttl: &str) -> Result<i64, CliError> {
    let (value_str, unit) = ttl.split_at(ttl.len().saturating_sub(1));
    let value: i64 = value_str
        .parse()
        .map_err(|_| CliError::InvalidInput(format!("Invalid TTL value: {}", value_str)))?;
    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => {
            return Err(CliError::InvalidInput(format!(
                "Unknown TTL unit: {unit}. Use s, m, h, or d."
            )));
        }
    };
    Ok(value * multiplier)
}

/// Issue a new DelegationToken for a userpod.
///
/// Returns the token serialized as JSON (for use as HKASK_DELEGATION_TOKEN).
pub async fn token_issue(
    userpod: &str,
    capabilities: Vec<String>,
    ttl: &str,
) -> Result<String, ServiceError> {
    let ctx = crate::commands::helpers::build_agent_service();
    let webid = WebID::from_persona(userpod.as_bytes());
    let ttl_secs = parse_ttl(ttl).map_err(|e| ServiceError::Domain {
        kind: ErrorKind::BadRequest,
        domain: DomainKind::Infrastructure,
        source: None,
        message: e.to_string(),
    })?;
    let expires_at = chrono::Utc::now().timestamp() + ttl_secs;

    let (_system_webid, a2a) = ctx.identity();
    let mut token = a2a
        .register_agent(webid, capabilities.clone())
        .await
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Agent,
            source: None,
            message: e.to_string(),
        })?;

    // Set expiry on the token
    token.expires_at = Some(expires_at);

    serde_json::to_string_pretty(&token).map_err(|e| ServiceError::Domain {
        kind: ErrorKind::BadRequest,
        domain: DomainKind::Infrastructure,
        source: Some(Box::new(e)),
        message: "Failed to serialize token".into(),
    })
}

/// List tokens for a userpod (or all userpods if None).
pub fn token_list(userpod: Option<&str>) -> Result<Vec<TokenEntry>, ServiceError> {
    let ctx = crate::commands::helpers::build_agent_service();
    let (_system_webid, a2a) = ctx.identity();
    let agents = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(a2a.list_agents())
    });
    let entries: Vec<TokenEntry> = agents
        .into_iter()
        .filter(|a| userpod.is_none_or(|r| a.webid.to_string() == r))
        .map(|a| TokenEntry {
            name: a.webid.to_string(),
            token_hash: String::new(),
            capabilities: a.capabilities,
            registered_at: a.registered_at.to_string(),
        })
        .collect();
    Ok(entries)
}

/// Revoke a token by ID.
pub async fn token_revoke(token_id: &str) -> Result<(), ServiceError> {
    let ctx = crate::commands::helpers::build_agent_service();
    let (_system_webid, a2a) = ctx.identity();
    a2a.revoke_capability(token_id).await;
    Ok(())
}

/// Display-friendly token entry for listing.
#[derive(Debug)]
pub struct TokenEntry {
    pub name: String,
    pub token_hash: String,
    pub capabilities: Vec<String>,
    pub registered_at: String,
}

/// Dispatch a token subcommand.
pub fn run_token(rt: &tokio::runtime::Runtime, action: TokenAction) {
    match action {
        TokenAction::Issue {
            userpod,
            capabilities,
            ttl,
        } => match rt.block_on(token_issue(&userpod, capabilities, &ttl)) {
            Ok(token_json) => {
                println!("{}", token_json);
                eprintln!("Token issued for {userpod} (TTL: {ttl}). Store this securely.");
                eprintln!("Use in IDE:  HKASK_DELEGATION_TOKEN='{token_json}'");
            }
            Err(e) => eprintln!("Token issue failed: {e}"),
        },
        TokenAction::List { userpod } => match token_list(userpod.as_deref()) {
            Ok(entries) if entries.is_empty() => {
                println!("No tokens found.");
            }
            Ok(entries) => {
                for e in &entries {
                    println!(
                        "{} — {} — {}",
                        e.name,
                        e.capabilities.join(", "),
                        e.registered_at
                    );
                }
            }
            Err(e) => eprintln!("Token list failed: {e}"),
        },
        TokenAction::Revoke { token_id } => match rt.block_on(token_revoke(&token_id)) {
            Ok(()) => println!("Token {token_id} revoked."),
            Err(e) => eprintln!("Token revoke failed: {e}"),
        },
    }
}
