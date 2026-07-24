//! hkask-cloud-gateway — binary entrypoint.
//!
//! mTLS + DelegationToken reverse proxy for remote access to the hKask daemon.
//!
//! Environment variables:
//!   HKASK_GATEWAY_BIND       — Address to bind (default: "0.0.0.0:9443")
//!   HKASK_GATEWAY_SERVER_CERT — Path to server TLS certificate (PEM)
//!   HKASK_GATEWAY_SERVER_KEY  — Path to server private key (PEM)
//!   HKASK_GATEWAY_CLIENT_CA   — Path to client CA certificate (PEM)

#![allow(unused_crate_dependencies)] // All deps used in this binary — lint produces false positives

use hkask_mcp_cloud_gateway::server::{GatewayConfig, run};
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "hkask.gateway=info".into()),
        )
        .init();

    let config = GatewayConfig {
        bind_addr: std::env::var("HKASK_GATEWAY_BIND").unwrap_or_else(|_| "0.0.0.0:9443".into()),
        server_cert: PathBuf::from(
            std::env::var("HKASK_GATEWAY_SERVER_CERT")
                .expect("HKASK_GATEWAY_SERVER_CERT must be set"),
        ),
        server_key: PathBuf::from(
            std::env::var("HKASK_GATEWAY_SERVER_KEY")
                .expect("HKASK_GATEWAY_SERVER_KEY must be set"),
        ),
        client_ca: PathBuf::from(
            std::env::var("HKASK_GATEWAY_CLIENT_CA").expect("HKASK_GATEWAY_CLIENT_CA must be set"),
        ),
    };

    let daemon = hkask_mcp_server::DaemonClient::new();

    tracing::info!(
        target: "hkask.gateway",
        bind_addr = %config.bind_addr,
        "Starting cloud gateway"
    );

    if let Err(e) = run(config, daemon).await {
        tracing::error!(target: "hkask.gateway", error = %e, "Gateway fatal error");
        std::process::exit(1);
    }
}
