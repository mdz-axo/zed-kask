//! SSRF-hardened NIP-57 LNURL-pay zap preparation and NIP-61 nutzap
//! validation for `hkask-mcp-nostr-client` (D110 Phase 4).
//!
//! Keeps `hkask-mcp-nostr-client` 100% zero-credential and free of
//! `libsecp256k1` / `hkask-nostr-signer`.

use std::net::IpAddr;
use std::time::Duration;

use hkask_nostr_core::{validate_nip61_p2pk_pubkey, verify_bolt11_zap_invoice};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

const LNURL_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ZapClientError {
    #[error("invalid Lightning Address (expected user@domain)")]
    InvalidLightningAddress,
    #[error("LNURL/callback URL rejected by SSRF perimeter: {0}")]
    SsrfBlocked(&'static str),
    #[error("recipient LNURL endpoint does not advertise allowsNostr: true (NIP-57 unsupported)")]
    NostrZapsUnsupported,
    #[error("requested zap amount {amount_msat} msat is outside recipient bounds ({min}..={max})")]
    AmountOutOfBounds {
        amount_msat: u64,
        min: u64,
        max: u64,
    },
    #[error("LNURL HTTP request failed: {0}")]
    Http(String),
    #[error("LNURL response was malformed: {0}")]
    MalformedResponse(String),
    #[error("BOLT11 invoice failed NIP-57 verification: {0}")]
    Bolt11Verification(String),
    #[error("NIP-61 nutzap mint URL is not in the operator-allowlisted mints")]
    UntrustedMint,
    #[error("invalid NIP-61 nutzap input: {0}")]
    InvalidNutzap(String),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LnurlPayMetadata {
    pub callback: String,
    pub min_sendable: u64,
    pub max_sendable: u64,
    #[serde(default)]
    pub allows_nostr: bool,
    #[serde(default)]
    pub nostr_pubkey: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LnurlCallbackResponse {
    #[serde(default)]
    pr: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedZapInvoice {
    pub bolt11: String,
    pub amount_msat: u64,
    pub description_hash: String,
    pub recipient_nostr_pubkey: Option<String>,
}

const MAX_LNURL_RESPONSE_BYTES: usize = 64 * 1024;

/// Validate an LNURL or callback URL under the D110 SSRF perimeter (Threat T5).
///
/// Loopback is permitted ONLY when `allow_loopback` is explicitly true (tests).
pub fn validate_lnurl_url(raw: &str, allow_loopback: bool) -> Result<reqwest::Url, ZapClientError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().any(|c| c.is_control()) {
        return Err(ZapClientError::SsrfBlocked("empty or control chars"));
    }
    let parsed =
        reqwest::Url::parse(trimmed).map_err(|_| ZapClientError::SsrfBlocked("unparseable URL"))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(ZapClientError::SsrfBlocked(
            "embedded credentials forbidden",
        ));
    }
    let host_raw = parsed
        .host_str()
        .ok_or(ZapClientError::SsrfBlocked("missing host"))?;
    let host_unbracketed = hkask_types::net_guard::normalize_host(host_raw);

    if allow_loopback && hkask_types::net_guard::is_loopback_hostname(&host_unbracketed) {
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err(ZapClientError::SsrfBlocked("invalid scheme"));
        }
        return Ok(parsed);
    }

    if parsed.scheme() != "https" {
        return Err(ZapClientError::SsrfBlocked("only https:// is permitted"));
    }
    if let Ok(ip) = host_unbracketed.parse::<IpAddr>() {
        if !hkask_types::net_guard::is_public_ip(ip) {
            return Err(ZapClientError::SsrfBlocked("non-public IP address"));
        }
    } else if !hkask_types::net_guard::is_public_fqdn(&host_unbracketed) {
        return Err(ZapClientError::SsrfBlocked("non-public hostname"));
    }
    Ok(parsed)
}

/// Convert a Lightning Address (`user@domain`) into its LNURL-pay discovery URL.
pub fn lud16_to_lnurlp_url(
    lud16: &str,
    allow_loopback: bool,
) -> Result<reqwest::Url, ZapClientError> {
    let trimmed = lud16.trim();
    let (user, domain) = trimmed
        .split_once('@')
        .ok_or(ZapClientError::InvalidLightningAddress)?;
    if user.is_empty()
        || domain.is_empty()
        || user.contains('/')
        || user.contains('?')
        || user.contains('#')
        || domain.contains('/')
        || trimmed.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(ZapClientError::InvalidLightningAddress);
    }
    let scheme = if allow_loopback
        && (domain.starts_with("127.0.0.1:") || domain.starts_with("localhost:"))
    {
        "http"
    } else {
        "https"
    };
    let raw = format!("{scheme}://{domain}/.well-known/lnurlp/{user}");
    validate_lnurl_url(&raw, allow_loopback)
}

/// Build an SSRF-hardened HTTP client with `.no_proxy()`, redirects disabled,
/// and connect-time DNS validation (SEC-07).
pub fn build_no_redirect_http_client() -> Result<reqwest::Client, ZapClientError> {
    build_no_redirect_http_client_with_loopback(false)
}

/// Build an SSRF-hardened HTTP client optionally admitting loopback (test
/// fixtures only).
pub fn build_no_redirect_http_client_with_loopback(
    allow_loopback: bool,
) -> Result<reqwest::Client, ZapClientError> {
    hkask_mcp_server::http_guard::build_ssrf_safe_client(LNURL_TIMEOUT, allow_loopback)
        .map_err(|e| ZapClientError::Http(e.to_string()))
}

/// Resolve DNS for `url` and verify every returned IP address is publicly
/// routable before issuing an HTTP request (Threat T5 DNS-rebinding guard).
pub async fn verify_resolved_dns(
    url: &reqwest::Url,
    allow_loopback: bool,
) -> Result<(), ZapClientError> {
    let host = url
        .host_str()
        .ok_or(ZapClientError::SsrfBlocked("missing host"))?;
    let port = url.port_or_known_default().unwrap_or(443);
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| ZapClientError::Http(format!("DNS resolution failed: {e}")))?;
    let mut resolved_any = false;
    for addr in addrs {
        resolved_any = true;
        let ip = addr.ip();
        if allow_loopback && hkask_types::net_guard::is_loopback_ip(ip) {
            continue;
        }
        if !hkask_types::net_guard::is_public_ip(ip) {
            return Err(ZapClientError::SsrfBlocked(
                "DNS resolved to a non-public IP address",
            ));
        }
    }
    if !resolved_any {
        return Err(ZapClientError::Http(
            "DNS resolution returned no addresses".to_string(),
        ));
    }
    Ok(())
}

/// Fetch and validate LNURL-pay metadata from `endpoint_url`.
pub async fn fetch_lnurl_pay_metadata(
    client: &reqwest::Client,
    endpoint_url: &reqwest::Url,
    amount_msat: u64,
    allow_loopback: bool,
) -> Result<LnurlPayMetadata, ZapClientError> {
    verify_resolved_dns(endpoint_url, allow_loopback).await?;
    let resp = client
        .get(endpoint_url.clone())
        .send()
        .await
        .map_err(|e| ZapClientError::Http(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(ZapClientError::Http(format!(
            "LNURL endpoint returned HTTP {}",
            resp.status()
        )));
    }
    let raw_bytes =
        hkask_mcp_server::http_guard::read_response_bytes_bounded(resp, MAX_LNURL_RESPONSE_BYTES)
            .await
            .map_err(ZapClientError::MalformedResponse)?;
    let meta: LnurlPayMetadata = serde_json::from_slice(&raw_bytes)
        .map_err(|e| ZapClientError::MalformedResponse(e.to_string()))?;
    if !meta.allows_nostr {
        return Err(ZapClientError::NostrZapsUnsupported);
    }
    let _ = validate_lnurl_url(&meta.callback, allow_loopback)?;
    if amount_msat < meta.min_sendable || amount_msat > meta.max_sendable {
        return Err(ZapClientError::AmountOutOfBounds {
            amount_msat,
            min: meta.min_sendable,
            max: meta.max_sendable,
        });
    }
    Ok(meta)
}

/// Request the BOLT11 invoice from the LNURL callback with the signed NIP-57
/// `kind: 9734` event and strictly verify `description_hash` + `amount_msat`.
pub async fn fetch_and_verify_zap_invoice(
    client: &reqwest::Client,
    callback: &str,
    amount_msat: u64,
    signed_zap_request_json: &str,
    recipient_nostr_pubkey: Option<String>,
    allow_loopback: bool,
) -> Result<VerifiedZapInvoice, ZapClientError> {
    let mut url = validate_lnurl_url(callback, allow_loopback)?;
    verify_resolved_dns(&url, allow_loopback).await?;
    url.query_pairs_mut()
        .append_pair("amount", &amount_msat.to_string())
        .append_pair("nostr", signed_zap_request_json);

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| ZapClientError::Http(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(ZapClientError::Http(format!(
            "LNURL callback returned HTTP {}",
            resp.status()
        )));
    }
    let raw_bytes =
        hkask_mcp_server::http_guard::read_response_bytes_bounded(resp, MAX_LNURL_RESPONSE_BYTES)
            .await
            .map_err(ZapClientError::MalformedResponse)?;
    let body: LnurlCallbackResponse = serde_json::from_slice(&raw_bytes)
        .map_err(|e| ZapClientError::MalformedResponse(e.to_string()))?;
    let Some(pr) = body.pr.filter(|s| !s.trim().is_empty()) else {
        return Err(ZapClientError::MalformedResponse(
            body.reason
                .unwrap_or_else(|| "callback response missing 'pr' BOLT11 invoice".to_string()),
        ));
    };

    let expected_desc_hash = hex::encode(Sha256::digest(signed_zap_request_json.as_bytes()));
    verify_bolt11_zap_invoice(&pr, amount_msat, &expected_desc_hash)
        .map_err(|e| ZapClientError::Bolt11Verification(e.to_string()))?;

    Ok(VerifiedZapInvoice {
        bolt11: pr,
        amount_msat,
        description_hash: expected_desc_hash,
        recipient_nostr_pubkey,
    })
}

/// Validate NIP-61 nutzap publish inputs before asking the editor to sign `kind: 9321`.
///
/// Enforces:
/// 1. `recipient_p2pk_pubkey` is a `02`-prefixed 66-char compressed hex key (NIP-61).
/// 2. `mint_url` is explicitly present in `allowed_mints` (never auto-trusts `kind: 10019` mints).
/// 3. `proofs_json` is non-empty and each entry parses as a JSON object.
pub fn validate_nutzap_publish_inputs(
    recipient_p2pk_pubkey: &str,
    mint_url: &str,
    allowed_mints: &[String],
    proofs_json: &[String],
) -> Result<(String, String), ZapClientError> {
    let p2pk = validate_nip61_p2pk_pubkey(recipient_p2pk_pubkey)
        .map_err(|e| ZapClientError::InvalidNutzap(e.to_string()))?;

    let norm_mint = mint_url.trim().trim_end_matches('/').to_string();
    let allowlisted = allowed_mints
        .iter()
        .any(|m| m.trim().trim_end_matches('/') == norm_mint);
    if !allowlisted {
        return Err(ZapClientError::UntrustedMint);
    }

    if proofs_json.is_empty() {
        return Err(ZapClientError::InvalidNutzap(
            "at least one Cashu proof JSON object is required".to_string(),
        ));
    }
    for proof in proofs_json {
        let parsed: serde_json::Value = serde_json::from_str(proof)
            .map_err(|_| ZapClientError::InvalidNutzap("proof entry is not valid JSON".into()))?;
        if !parsed.is_object() {
            return Err(ZapClientError::InvalidNutzap(
                "each proof entry must be a JSON object".into(),
            ));
        }
    }

    Ok((p2pk, norm_mint))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lnurl_ssrf_guard_blocks_private_loopback_and_credentials() {
        for bad in [
            "http://example.com/.well-known/lnurlp/alice",
            "https://127.0.0.1:8080/lnurl",
            "https://169.254.169.254/latest/meta-data/",
            "https://10.0.0.1/lnurl",
            "https://user:pass@example.com/lnurl",
            "https://localhost/lnurl",
        ] {
            assert!(
                validate_lnurl_url(bad, false).is_err(),
                "expected SSRF block for {bad}"
            );
        }
        assert!(validate_lnurl_url("https://getalby.com/.well-known/lnurlp/alice", false).is_ok());
    }

    #[test]
    fn nutzap_validation_refuses_untrusted_mint_and_non_02_p2pk() {
        let good_p2pk = format!("02{}", "ab".repeat(32));
        let bad_p2pk = format!("03{}", "ab".repeat(32));
        let allowed = vec!["https://mint.example.com".to_string()];
        let proofs = vec![r#"{"amount":100,"id":"00ad","secret":"s","C":"02..."}"#.to_string()];

        assert!(
            validate_nutzap_publish_inputs(
                &good_p2pk,
                "https://mint.example.com/",
                &allowed,
                &proofs
            )
            .is_ok()
        );
        assert_eq!(
            validate_nutzap_publish_inputs(
                &good_p2pk,
                "https://evil-mint.example.com",
                &allowed,
                &proofs
            ),
            Err(ZapClientError::UntrustedMint)
        );
        assert!(matches!(
            validate_nutzap_publish_inputs(
                &bad_p2pk,
                "https://mint.example.com",
                &allowed,
                &proofs
            ),
            Err(ZapClientError::InvalidNutzap(_))
        ));
    }
}
