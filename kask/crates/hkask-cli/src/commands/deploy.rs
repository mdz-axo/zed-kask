//! `kask deploy` — Remote cluster deployment (K3s/Hetzner bootstrap).
//!
//! Extracted from the former `kask curator init` command, which was misnamed:
//! it deploys the cluster (Conduit + hKask pod via kubectl), not the Curator
//! daemon. This is admin/startup work, not runtime.

use crate::cli::DeployAction;
use crate::error::CliError;

/// Dispatch `kask deploy` subcommands.
pub fn run(rt: &tokio::runtime::Runtime, action: DeployAction) {
    match action {
        DeployAction::Init { domain } => match rt.block_on(deploy_init(&domain)) {
            Ok(()) => {}
            Err(e) => eprintln!("Deploy init failed: {e}"),
        },
    }
}

/// Initialize the hKask system on a Hetzner K3s cluster.
///
/// This is a one-time setup command. It:
///   1. Validates all required env vars (HCLOUD_TOKEN, CONTAINER_REGISTRY, LITESTREAM_*, etc.)
///   2. Confirms kubectl access to the K3s cluster
///   3. Validates the Hetzner API token and object storage credentials
///   4. Generates a Conduit Matrix signing key (Ed25519)
///   5. Deploys the shared Conduit Matrix homeserver from deploy/k8s/conduit/
///   6. Deploys the hKask pod from deploy/k8s/
///   7. Prints the Matrix URL and Curator URL
pub async fn deploy_init(domain: &str) -> Result<(), CliError> {
    // 1. Validate environment
    let hcloud_token = std::env::var("HCLOUD_TOKEN").map_err(|_| {
        CliError::Config("HCLOUD_TOKEN not set. Set your Hetzner Cloud API token.".into())
    })?;
    if hcloud_token.is_empty() {
        return Err(CliError::Config("HCLOUD_TOKEN is empty.".into()));
    }

    let _container_registry = std::env::var("CONTAINER_REGISTRY").map_err(|_| {
        CliError::Config("CONTAINER_REGISTRY not set (e.g., ghcr.io/your-org/hkask).".into())
    })?;
    let _version = std::env::var("HKASK_VERSION").unwrap_or_else(|_| "0.30.0".to_string());

    let litestream_bucket = std::env::var("LITESTREAM_BUCKET")
        .map_err(|_| CliError::Config("LITESTREAM_BUCKET not set.".into()))?;
    let litestream_endpoint = std::env::var("LITESTREAM_ENDPOINT")
        .map_err(|_| CliError::Config("LITESTREAM_ENDPOINT not set.".into()))?;
    let litestream_access_key = std::env::var("LITESTREAM_ACCESS_KEY_ID")
        .map_err(|_| CliError::Config("LITESTREAM_ACCESS_KEY_ID not set.".into()))?;
    let litestream_secret_key = std::env::var("LITESTREAM_SECRET_ACCESS_KEY")
        .map_err(|_| CliError::Config("LITESTREAM_SECRET_ACCESS_KEY not set.".into()))?;

    let _base_url = std::env::var("HKASK_BASE_URL").map_err(|_| {
        CliError::Config("HKASK_BASE_URL not set (e.g., https://hkask.example.com).".into())
    })?;

    let _keystore_passphrase = std::env::var("HKASK_KEYSTORE_PASSPHRASE").unwrap_or_default();

    // 2. Check kubectl availability
    let kubectl_ok = std::process::Command::new("kubectl")
        .arg("version")
        .arg("--client")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !kubectl_ok {
        return Err(CliError::Config(
            "kubectl not found. Install it and ensure KUBECONFIG points to your K3s cluster."
                .into(),
        ));
    }

    // 3. Validate cloud provider access
    println!("=== hKask Deploy Init ===");
    println!("Domain: {domain}");
    println!();

    let hetzner = crate::cloud::hetzner::HetznerClient::new(hcloud_token.clone());
    println!("Validating Hetzner API token...");
    hetzner.validate_token().await.map_err(|e| {
        CliError::Config(format!(
            "Hetzner API token validation failed: {e}\nCheck HCLOUD_TOKEN in your .env file."
        ))
    })?;
    println!("  Hetzner API: OK");

    println!("Validating object storage...");
    crate::cloud::hetzner::validate_object_storage(
        &litestream_endpoint,
        &litestream_bucket,
        &litestream_access_key,
        &litestream_secret_key,
    )
    .await
    .map_err(|e| {
        CliError::Config(format!(
            "Object storage validation failed: {e}\nCheck LITESTREAM_* vars in your .env file."
        ))
    })?;
    println!("  Object storage: OK");
    println!();

    // 4. Generate Conduit signing key
    println!("Generating Conduit signing key...");
    use rand::RngCore;
    let mut key_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut key_bytes);
    let signing_key = {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(key_bytes)
    };
    println!("  Signing key generated.");

    // 5. Deploy shared Conduit from deploy/k8s/conduit/
    println!("Deploying shared Conduit...");

    let conduit_dir = std::env::temp_dir().join("hkask-conduit-init");
    let _ = std::fs::create_dir_all(&conduit_dir);

    copy_conduit_manifests(domain, &signing_key, &conduit_dir)?;

    let apply_output = std::process::Command::new("kubectl")
        .arg("apply")
        .arg("-f")
        .arg(&conduit_dir)
        .output()
        .map_err(|e| CliError::Io(format!("Failed to run kubectl apply for Conduit: {e}")))?;

    if !apply_output.status.success() {
        let stderr = String::from_utf8_lossy(&apply_output.stderr);
        return Err(CliError::Io(format!(
            "kubectl apply for Conduit failed:\n{stderr}"
        )));
    }

    let _ = std::fs::remove_dir_all(&conduit_dir);

    let matrix_url = "http://conduit.hkask-conduit.svc.cluster.local:8008";
    println!("  Conduit deployed. Matrix URL: {matrix_url}");
    println!();

    // 6. Store signing key (pending keystore integration)
    println!("  Signing key stored in memory (keystore integration pending).");

    // 7. Deploy hKask pod from deploy/k8s/
    println!("Deploying hKask pod...");

    let curator_dir = std::env::temp_dir().join("hkask-curator-init");
    let _ = std::fs::create_dir_all(&curator_dir);

    crate::commands::pod::export_k8s(&curator_dir)
        .map_err(|e| CliError::Io(format!("Failed to export K8s manifests: {e}")))?;

    let apply_output = std::process::Command::new("kubectl")
        .arg("apply")
        .arg("-f")
        .arg(&curator_dir)
        .output()
        .map_err(|e| CliError::Io(format!("Failed to run kubectl apply: {e}")))?;

    if !apply_output.status.success() {
        let stderr = String::from_utf8_lossy(&apply_output.stderr);
        return Err(CliError::Io(format!("kubectl apply failed:\n{stderr}")));
    }

    let _ = std::fs::remove_dir_all(&curator_dir);

    let curator_url = format!("https://{domain}");

    println!();
    println!("=== hKask system initialized ===");
    println!("  Curator URL:  {curator_url}");
    println!("  Matrix URL:   {matrix_url}");
    println!();

    Ok(())
}

/// Copy conduit manifests from deploy/k8s/conduit/ and populate the
/// secret with the generated signing key and domain.
fn copy_conduit_manifests(
    domain: &str,
    signing_key: &str,
    output_dir: &std::path::Path,
) -> Result<(), CliError> {
    let source_dir = crate::commands::helpers::resolve_deploy_dir()?.join("conduit");
    if !source_dir.is_dir() {
        return Err(CliError::Io(format!(
            "Cannot find deploy/k8s/conduit at {source_dir:?}"
        )));
    }

    for entry in std::fs::read_dir(&source_dir)
        .map_err(|e| CliError::Io(format!("Cannot read {source_dir:?}: {e}")))?
    {
        let entry = entry.map_err(|e| CliError::Io(format!("read_dir entry: {e}")))?;
        let path = entry.path();
        let name = path
            .file_name()
            .ok_or_else(|| CliError::Io("missing filename".into()))?;
        if name == "secret.yaml" {
            let content = format!(
                "apiVersion: v1\n\
                 kind: Secret\n\
                 metadata:\n\
                   name: conduit-secrets\n\
                   namespace: hkask-conduit\n\
                 type: Opaque\n\
                 stringData:\n\
                   CONDUIT_SERVER_NAME: \"{domain}\"\n\
                   CONDUIT_SIGNING_KEY: \"{signing_key}\"\n\
                   CONDUIT_PORT: \"8008\"\n\
                   CONDUIT_DATABASE_PATH: \"/data/conduit.db\"\n\
                   CONDUIT_ALLOW_REGISTRATION: \"false\"\n"
            );
            std::fs::write(output_dir.join(name), &content)
                .map_err(|e| CliError::Io(format!("Failed to write conduit secret: {e}")))?;
        } else if path.is_file() {
            std::fs::copy(&path, output_dir.join(name)).map_err(|e| {
                CliError::Io(format!("Failed to copy conduit manifest {name:?}: {e}"))
            })?;
        }
    }
    Ok(())
}
