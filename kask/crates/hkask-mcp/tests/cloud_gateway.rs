//! Integration test — mTLS client → cloud gateway → mock daemon round-trip.
//!
//! Verifies the full chain: client connects via mTLS + DelegationToken,
//! gateway verifies token against CN, dispatches to daemon via Unix socket.

use hkask_capability::auth::derive_signing_key;
use hkask_capability::{DelegationAction, DelegationResource, DelegationToken};
use hkask_mcp_cloud_gateway::server::{GatewayConfig, build_tls_config};
use hkask_mcp_server::daemon::{DaemonClient, DaemonHandler, DaemonListener};
use hkask_types::WebID;
use rustls::ClientConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, pem::PemObject};
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as AsyncBufReader};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

/// Generate a DelegationToken for testing.
fn token_for(cn: &str, tool: &str) -> DelegationToken {
    let sk = derive_signing_key(b"integration-test-secret");
    let webid = WebID::from_persona(cn.as_bytes());
    DelegationToken::new(
        DelegationResource::Tool,
        tool.into(),
        DelegationAction::Execute,
        WebID::from_persona(b"issuer"),
        webid,
        &sk,
    )
}

/// Generate a self-signed CA + server cert + client cert into a temp dir.
/// Returns (temp_dir, ca_pem, client_cert_pem, client_key_pem).
fn generate_certs() -> (tempfile::TempDir, String, String, String) {
    let tmp = tempfile::TempDir::new().unwrap();
    let p = tmp.path();

    let ca_key = rcgen::KeyPair::generate().unwrap();
    let mut ca_params = rcgen::CertificateParams::default();
    ca_params.distinguished_name = rcgen::DistinguishedName::new();
    ca_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Test CA");
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let ca_pem = ca_cert.pem();
    std::fs::write(p.join("ca.crt"), &ca_pem).unwrap();

    let server_key = rcgen::KeyPair::generate().unwrap();
    let mut sp = rcgen::CertificateParams::default();
    sp.distinguished_name = rcgen::DistinguishedName::new();
    sp.distinguished_name
        .push(rcgen::DnType::CommonName, "localhost");
    sp.subject_alt_names = vec![rcgen::SanType::DnsName("localhost".try_into().unwrap())];
    let server_cert = sp.signed_by(&server_key, &ca_cert, &ca_key).unwrap();
    std::fs::write(p.join("server.crt"), server_cert.pem()).unwrap();
    std::fs::write(p.join("server.key"), server_key.serialize_pem()).unwrap();

    let client_key = rcgen::KeyPair::generate().unwrap();
    let mut cp = rcgen::CertificateParams::default();
    cp.distinguished_name = rcgen::DistinguishedName::new();
    cp.distinguished_name
        .push(rcgen::DnType::CommonName, "alice");
    let client_cert = cp.signed_by(&client_key, &ca_cert, &ca_key).unwrap();
    let client_cert_pem = client_cert.pem();
    let client_key_pem = client_key.serialize_pem();
    std::fs::write(p.join("alice.crt"), &client_cert_pem).unwrap();
    std::fs::write(p.join("alice.key"), &client_key_pem).unwrap();

    (tmp, ca_pem, client_cert_pem, client_key_pem)
}

/// Mock daemon handler — all methods return success defaults.
/// Lives in hkask-mcp crate to avoid cross-crate async_trait lifetime issues.
struct TestDaemon {
    dispatched: AtomicBool,
}

#[async_trait::async_trait]
impl DaemonHandler for TestDaemon {
    async fn check_auth(&self, _r: &str) -> (bool, Option<String>) {
        (true, None)
    }
    async fn check_capability(&self, _r: &str, _t: &str) -> bool {
        true
    }
    async fn store_experience(
        &self,
        _r: &str,
        _e: &str,
        _a: &str,
        _v: &serde_json::Value,
        _c: Option<f64>,
    ) -> (bool, Option<String>, Option<String>) {
        (true, None, None)
    }
    async fn dispatch_tool(
        &self,
        _r: &str,
        tool: &str,
        _i: &serde_json::Value,
    ) -> (bool, Option<serde_json::Value>, Option<String>) {
        self.dispatched.store(true, Ordering::SeqCst);
        (true, Some(json!({"tool": tool, "status": "ok"})), None)
    }
    async fn curator_health(&self, _r: &str) -> serde_json::Value {
        json!({"reg_health": "healthy"})
    }
    async fn reg_status(&self, _userpod: &str, _domain: Option<&str>) -> serde_json::Value {
        json!({"domains": []})
    }
}

#[tokio::test]
async fn cloud_gateway_round_trip() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (_tmp, ca_pem, client_cert_pem, client_key_pem) = generate_certs();
    let tmp_path = _tmp.path();

    // ── Mock daemon listener ─────────────────────────────────────────
    let sock = tmp_path.join("daemon.sock");
    let _ = std::fs::remove_file(&sock);
    let mut listener = DaemonListener::with_path(sock.clone());
    listener.bind().await.unwrap();
    let mock = Arc::new(TestDaemon {
        dispatched: AtomicBool::new(false),
    });
    let serve_mock = Arc::clone(&mock);
    tokio::spawn(async move {
        let _ = listener.serve(serve_mock).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // ── Inline gateway ───────────────────────────────────────────────
    let gw_config = GatewayConfig {
        server_cert: tmp_path.join("server.crt"),
        server_key: tmp_path.join("server.key"),
        client_ca: tmp_path.join("ca.crt"),
        bind_addr: "127.0.0.1:0".into(),
    };
    let tls_cfg = build_tls_config(&gw_config).unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls_cfg));
    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = tcp.local_addr().unwrap();
    let daemon = DaemonClient::with_path(sock);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = tcp.accept().await else {
                break;
            };
            let acc = acceptor.clone();
            let d = daemon.clone();
            tokio::spawn(async move {
                let Ok(tls_s) = acc.accept(stream).await else {
                    return;
                };
                let (r, mut w) = tokio::io::split(tls_s);
                let mut buf = AsyncBufReader::new(r);
                let mut line = String::new();
                if buf.read_line(&mut line).await.is_err() {
                    return;
                }
                let Ok(req) = serde_json::from_str::<serde_json::Value>(&line) else {
                    return;
                };
                let tool = req["tool"].as_str().unwrap_or("");
                let params = &req["params"];
                let (ok, out, err) = match d.tool_dispatch("gateway", tool, params).await {
                    Ok(hkask_mcp_server::daemon::DaemonResponse::ToolDispatchResponse {
                        ok,
                        output,
                        error,
                    }) => (ok, output, error),
                    _ => (false, None, Some("daemon error".into())),
                };
                let resp = json!({"ok": ok, "output": out, "error": err});
                let mut rl = serde_json::to_string(&resp).unwrap();
                rl.push('\n');
                let _ = w.write_all(rl.as_bytes()).await;
            });
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // ── Client mTLS + token ──────────────────────────────────────────
    let client_cert_der = CertificateDer::pem_slice_iter(client_cert_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let client_key_der = PrivateKeyDer::from_pem_slice(client_key_pem.as_bytes()).unwrap();
    let mut ca_store = rustls::RootCertStore::empty();
    for c in CertificateDer::pem_slice_iter(ca_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    {
        ca_store.add(c).unwrap();
    }
    let client_tls = ClientConfig::builder()
        .with_root_certificates(ca_store)
        .with_client_auth_cert(client_cert_der, client_key_der)
        .unwrap();
    let connector = TlsConnector::from(Arc::new(client_tls));
    let stream = TcpStream::connect(addr).await.unwrap();
    let tls_stream = connector
        .connect(ServerName::try_from("localhost").unwrap(), stream)
        .await
        .unwrap();

    let token = token_for("alice", "curator_health");
    let req = json!({"tool": "curator_health", "params": {}, "token": token});
    let (reader, mut writer) = tokio::io::split(tls_stream);
    let mut req_line = serde_json::to_string(&req).unwrap();
    req_line.push('\n');
    writer.write_all(req_line.as_bytes()).await.unwrap();

    let mut buf = AsyncBufReader::new(reader);
    let mut resp_line = String::new();
    buf.read_line(&mut resp_line).await.unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert_eq!(resp["ok"], true, "Response: {resp}");
    assert_eq!(resp["output"]["tool"], "curator_health");
    assert!(
        mock.dispatched.load(Ordering::SeqCst),
        "Daemon not dispatched"
    );
}
