//! mTLS server setup for the cloud gateway.
//!
//! Configures rustls for mutual TLS 1.3, extracts client certificate
//! Common Name for identity binding, reads token-bearing JSON requests,
//! verifies DelegationTokens, and forwards to the local daemon handler.

use crate::auth::{self, AuthError};
use hkask_capability::DelegationToken;
use hkask_mcp_server::daemon::DaemonResponse;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as AsyncBufReader};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

/// Errors that can occur during gateway server operation.
#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("TLS configuration error: {0}")]
    Tls(#[from] rustls::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Certificate error: {0}")]
    Cert(String),

    #[error("Auth error: {0}")]
    Auth(#[from] AuthError),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Configuration for the cloud gateway server.
pub struct GatewayConfig {
    pub server_cert: PathBuf,
    pub server_key: PathBuf,
    pub client_ca: PathBuf,
    pub bind_addr: String,
}

// ── Wire protocol ──────────────────────────────────────────────────────

/// A request from a remote MCP client.
#[derive(Debug, Deserialize)]
struct CloudRequest {
    tool: String,
    #[serde(default)]
    params: Value,
    token: DelegationToken,
}

/// A response sent back to the remote MCP client.
#[derive(Debug, Serialize)]
struct CloudResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// ── TLS setup ──────────────────────────────────────────────────────────

pub fn build_tls_config(config: &GatewayConfig) -> Result<ServerConfig, GatewayError> {
    let certs = load_certificates(&config.server_cert)?;
    let key = load_private_key(&config.server_key)?;

    let mut client_ca_store = RootCertStore::empty();
    let ca_certs = load_certificates(&config.client_ca)?;
    for cert in &ca_certs {
        client_ca_store
            .add(cert.clone())
            .map_err(|e| GatewayError::Cert(format!("Failed to add CA cert: {e}")))?;
    }

    let client_verifier = WebPkiClientVerifier::builder(Arc::new(client_ca_store))
        .build()
        .map_err(|e| GatewayError::Cert(format!("Failed to build client verifier: {e}")))?;

    let tls_config = ServerConfig::builder()
        .with_client_cert_verifier(client_verifier)
        .with_single_cert(certs, key)
        .map_err(|e| GatewayError::Cert(format!("Failed to build TLS config: {e}")))?;

    Ok(tls_config)
}

// ── Server ─────────────────────────────────────────────────────────────

pub async fn run(
    config: GatewayConfig,
    daemon: hkask_mcp_server::DaemonClient,
) -> Result<(), GatewayError> {
    let tls_config = build_tls_config(&config)?;
    let acceptor = TlsAcceptor::from(Arc::new(tls_config));
    let listener = TcpListener::bind(&config.bind_addr).await?;
    let daemon = Arc::new(daemon);

    tracing::info!(
        target: "hkask.gateway",
        bind_addr = %config.bind_addr,
        "Cloud gateway listening with mTLS"
    );

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let acceptor = acceptor.clone();
        let daemon = Arc::clone(&daemon);

        tokio::spawn(async move {
            if let Err(e) = handle_connection(acceptor, stream, peer_addr, daemon).await {
                tracing::warn!(
                    target: "hkask.gateway",
                    peer = %peer_addr,
                    error = %e,
                    "Connection error"
                );
            }
        });
    }
}

async fn handle_connection(
    acceptor: TlsAcceptor,
    stream: tokio::net::TcpStream,
    peer_addr: std::net::SocketAddr,
    daemon: Arc<hkask_mcp_server::DaemonClient>,
) -> Result<(), GatewayError> {
    let tls_stream = acceptor.accept(stream).await?;
    let (_tcp, server_conn) = tls_stream.get_ref();
    let cert_cn = server_conn
        .peer_certificates()
        .and_then(|certs| certs.first())
        .and_then(extract_cn)
        .unwrap_or_else(|| "unknown".to_string());

    tracing::info!(
        target: "hkask.gateway",
        peer = %peer_addr,
        cn = %cert_cn,
        "mTLS connection established"
    );

    let (reader, mut writer) = tokio::io::split(tls_stream);
    let mut buf_reader = AsyncBufReader::new(reader);
    let mut request_count: u64 = 0;

    loop {
        let mut line = String::new();
        match buf_reader.read_line(&mut line).await {
            Ok(0) => {
                // EOF — client closed connection
                tracing::info!(
                    target: "hkask.gateway",
                    peer = %peer_addr,
                    cn = %cert_cn,
                    requests = request_count,
                    "Connection closed by client"
                );
                return Ok(());
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(
                    target: "hkask.gateway",
                    peer = %peer_addr,
                    error = %e,
                    "Read error"
                );
                return Err(GatewayError::Io(e));
            }
        }

        let request: CloudRequest = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                let resp = CloudResponse {
                    ok: false,
                    output: None,
                    error: Some(format!("Invalid JSON: {e}")),
                };
                write_response(&mut writer, &resp).await?;
                continue;
            }
        };

        // Verify token
        if let Err(e) = auth::verify_cloud_request(&request.token, &cert_cn, &request.tool) {
            let resp = CloudResponse {
                ok: false,
                output: None,
                error: Some(format!("Auth error: {e}")),
            };
            write_response(&mut writer, &resp).await?;
            continue;
        }

        request_count += 1;

        // Forward to daemon via Unix socket
        let (ok, output, error) = dispatch_to_daemon(&daemon, &request.tool, &request.params).await;

        let response = CloudResponse { ok, output, error };
        write_response(&mut writer, &response).await?;
    }
}

async fn write_response(
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    response: &CloudResponse,
) -> Result<(), GatewayError> {
    let mut json = serde_json::to_string(response)?;
    json.push('\n');
    writer.write_all(json.as_bytes()).await?;
    Ok(())
}

// ── Daemon forwarding ────────────────────────────────────────────────────

async fn dispatch_to_daemon(
    daemon: &hkask_mcp_server::DaemonClient,
    tool: &str,
    params: &Value,
) -> (bool, Option<Value>, Option<String>) {
    match daemon.tool_dispatch("gateway", tool, params).await {
        Ok(DaemonResponse::ToolDispatchResponse { ok, output, error }) => (ok, output, error),
        Ok(_) => (false, None, Some("Unexpected daemon response".into())),
        Err(e) => (false, None, Some(format!("Daemon error: {e}"))),
    }
}

// ── Certificate helpers ────────────────────────────────────────────────

fn load_certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>, GatewayError> {
    use rustls::pki_types::pem::PemObject;
    let file = std::fs::File::open(path)
        .map_err(|e| GatewayError::Cert(format!("Cannot open {}: {e}", path.display())))?;
    let mut reader = BufReader::new(file);
    let certs = CertificateDer::pem_reader_iter(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| GatewayError::Cert(format!("Failed to parse certs: {e}")))?;
    if certs.is_empty() {
        return Err(GatewayError::Cert(format!(
            "No certificates in {}",
            path.display()
        )));
    }
    Ok(certs)
}

fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, GatewayError> {
    use rustls::pki_types::pem::PemObject;
    let file = std::fs::File::open(path)
        .map_err(|e| GatewayError::Cert(format!("Cannot open {}: {e}", path.display())))?;
    let mut reader = BufReader::new(file);
    PrivateKeyDer::pem_reader_iter(&mut reader)
        .next()
        .transpose()
        .map_err(|e| GatewayError::Cert(format!("Failed to parse key: {e}")))?
        .ok_or_else(|| GatewayError::Cert(format!("No private key in {}", path.display())))
}

fn extract_cn(cert: &CertificateDer) -> Option<String> {
    use x509_parser::prelude::*;
    let (_, parsed) = X509Certificate::from_der(cert.as_ref()).ok()?;
    for attr in parsed.subject().iter_common_name() {
        if let Ok(cn) = attr.as_str() {
            return Some(cn.to_string());
        }
    }
    None
}
