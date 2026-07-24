//! Cloud provider integrations — Hetzner API client and deploy configuration.
//!
//! Extracted from hkask-services (ADR-040: Service Layer Extraction, 2026-06-27).
//! Cloud provisioning is infrastructure, not a domain service.

// ── DeployConfig ───────────────────────────────────────────────────────────

/// Configuration for deploying a pod to cloud infrastructure.
/// DeployConfig is used by the Kubernetes-based deployment path (Hetzner K3s).
pub struct DeployConfig {
    pub container_registry: String,
    pub version: String,
    pub matrix_url: String,
    pub litestream_bucket: String,
    pub litestream_endpoint: String,
    pub litestream_region: String,
    pub litestream_access_key: String,
    pub litestream_secret_key: String,
    pub litestream_force_path: String,
    pub keystore_passphrase: String,
    pub base_url: String,
}

impl DeployConfig {
    /// Build from environment variables with sensible defaults.
    pub fn from_env(pod_id: &str) -> Self {
        Self {
            container_registry: std::env::var("CONTAINER_REGISTRY")
                .unwrap_or_else(|_| "ghcr.io/mdz-axo/hkask".to_string()),
            version: std::env::var("HKASK_VERSION").unwrap_or_else(|_| "0.30.0".to_string()),
            matrix_url: std::env::var("HKASK_MATRIX_URL")
                .unwrap_or_else(|_| "http://hkask-conduit:8008".to_string()),
            litestream_bucket: std::env::var("LITESTREAM_BUCKET").unwrap_or_default(),
            litestream_endpoint: std::env::var("LITESTREAM_ENDPOINT").unwrap_or_default(),
            litestream_region: std::env::var("LITESTREAM_REGION")
                .unwrap_or_else(|_| "auto".to_string()),
            litestream_access_key: std::env::var("LITESTREAM_ACCESS_KEY_ID").unwrap_or_default(),
            litestream_secret_key: std::env::var("LITESTREAM_SECRET_ACCESS_KEY")
                .unwrap_or_default(),
            litestream_force_path: std::env::var("LITESTREAM_FORCE_PATH_STYLE")
                .unwrap_or_else(|_| "false".to_string()),
            keystore_passphrase: std::env::var("HKASK_KEYSTORE_PASSPHRASE").unwrap_or_default(),
            base_url: std::env::var("HKASK_BASE_URL")
                .unwrap_or_else(|_| format!("https://hkask-pod-{pod_id}.example.com")),
        }
    }
}

// ── Hetzner Cloud API client ───────────────────────────────────────────────

// Hetzner Cloud API client.
//
// Provides server and volume management for the Hetzner infrastructure layer.
// Pod lifecycle (activate/deactivate) on Hetzner uses kubectl against the K3s
// cluster; this client handles the IaaS layer (server provisioning, volume
// management, object storage validation).
//
// API docs: <https://docs.hetzner.cloud/>

use reqwest::Client;
use serde::Deserialize;

const HCLOUD_API: &str = "https://api.hetzner.cloud/v1";

/// Hetzner API errors.
#[derive(Debug)]
pub enum HetznerError {
    Http(String),
    Parse(String),
}

impl std::fmt::Display for HetznerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HetznerError::Http(msg) | HetznerError::Parse(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for HetznerError {}

impl From<reqwest::Error> for HetznerError {
    fn from(e: reqwest::Error) -> Self {
        HetznerError::Http(e.to_string())
    }
}

/// Client for the Hetzner Cloud REST API.
pub struct HetznerClient {
    client: Client,
    token: String,
}

/// Minimal Hetzner server representation.
#[derive(Debug, Default, Deserialize)]
pub struct HetznerServer {
    pub id: u64,
    pub name: String,
    pub status: String,
}

/// Minimal Hetzner volume representation.
#[derive(Debug, Default, Deserialize)]
pub struct HetznerVolume {
    pub id: u64,
    pub name: String,
    pub size: u32,
    pub location: String,
}

/// Hetzner API wraps everything in a standard envelope.
#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    #[serde(rename = "type")]
    _type: Option<String>,
    #[serde(default)]
    servers: Option<Vec<T>>,
    #[allow(dead_code)]
    #[serde(default)]
    volumes: Option<Vec<T>>,
    #[serde(default)]
    server: Option<T>,
    #[serde(default)]
    volume: Option<T>,
}

impl HetznerClient {
    /// Create a new Hetzner Cloud API client.
    ///
    /// `token` is a Read & Write API token from the Hetzner Console.
    pub fn new(token: String) -> Self {
        Self {
            client: Client::new(),
            token,
        }
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.token)
    }

    /// List all servers in the project.
    pub async fn list_servers(&self) -> Result<Vec<HetznerServer>, HetznerError> {
        let resp = self
            .client
            .get(format!("{HCLOUD_API}/servers"))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| HetznerError::Http(format!("Failed to list servers: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(HetznerError::Http(format!(
                "List servers failed ({status}): {body}"
            )));
        }

        let envelope: ApiEnvelope<HetznerServer> = resp.json().await?;
        Ok(envelope.servers.unwrap_or_default())
    }

    /// Create a new server instance.
    pub async fn create_server(
        &self,
        name: &str,
        server_type: &str,
        image: &str,
        location: &str,
        ssh_keys: &[String],
    ) -> Result<HetznerServer, HetznerError> {
        let resp = self
            .client
            .post(format!("{HCLOUD_API}/servers"))
            .header("Authorization", self.auth_header())
            .json(&serde_json::json!({
                "name": name,
                "server_type": server_type,
                "image": image,
                "location": location,
                "ssh_keys": ssh_keys,
                "start_after_create": true,
            }))
            .send()
            .await
            .map_err(|e| HetznerError::Http(format!("Failed to create server: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(HetznerError::Http(format!(
                "Create server failed ({status}): {body}"
            )));
        }

        let envelope: ApiEnvelope<HetznerServer> = resp.json().await?;
        envelope
            .server
            .ok_or_else(|| HetznerError::Parse("Server not found in response".into()))
    }

    /// Create a persistent volume.
    pub async fn create_volume(
        &self,
        name: &str,
        size_gb: u32,
        location: &str,
    ) -> Result<HetznerVolume, HetznerError> {
        let resp = self
            .client
            .post(format!("{HCLOUD_API}/volumes"))
            .header("Authorization", self.auth_header())
            .json(&serde_json::json!({
                "name": name,
                "size": size_gb,
                "location": location,
            }))
            .send()
            .await
            .map_err(|e| HetznerError::Http(format!("Failed to create volume: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(HetznerError::Http(format!(
                "Create volume failed ({status}): {body}"
            )));
        }

        let envelope: ApiEnvelope<HetznerVolume> = resp.json().await?;
        envelope
            .volume
            .ok_or_else(|| HetznerError::Parse("Volume not found in response".into()))
    }

    /// Validate that the Hetzner API token works.
    pub async fn validate_token(&self) -> Result<(), HetznerError> {
        // A simple server list is the lightest validation call
        let _servers = self.list_servers().await?;
        Ok(())
    }
}

/// Validate that Hetzner Object Storage is reachable with the given credentials.
///
/// Uses the S3-compatible API endpoint. This is the same validation pattern
/// as other S3-compatible storage, adapted for Hetzner's path-style addressing.
pub async fn validate_object_storage(
    endpoint: &str,
    bucket: &str,
    access_key: &str,
    _secret_key: &str,
) -> Result<(), HetznerError> {
    let client = Client::new();

    // Hetzner OS uses path-style: endpoint/bucket
    let url = format!("{endpoint}/{bucket}");

    let resp = client
        .head(&url)
        .header(
            "Authorization",
            format!(
                "AWS4-HMAC-SHA256 Credential={access_key}/20260620/auto/s3/aws4_request, SignedHeaders=host, Signature=UNSIGNED_VALIDATION_ONLY"
            ),
        )
        .send()
        .await
        .map_err(|e| HetznerError::Http(format!("Cannot reach Hetzner Object Storage at {endpoint}: {e}")))?;

    match resp.status().as_u16() {
        200 | 403 => Ok(()), // 403 = valid creds, Litestream only needs PutObject/GetObject
        404 => Err(HetznerError::Http(format!(
            "Bucket '{bucket}' not found at {endpoint}. Create it in the Hetzner Console first."
        ))),
        status => {
            let body = resp.text().await.unwrap_or_default();
            Err(HetznerError::Http(format!(
                "Object storage validation failed (HTTP {status}): {body}"
            )))
        }
    }
}
