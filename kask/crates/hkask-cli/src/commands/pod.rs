//! Pod admin commands — deployment artifact generation only.
//!
//! Pod lifecycle ops (create/activate/deactivate/assign/mode) are runtime
//! operations available from the TUI REPL or the HTTP API. The CLI exposes
//! only `export-container` and `export-k8s` (deployment artifact generation),
//! which operate on static files, not the live system.

use crate::cli::PodAction;
use crate::error::CliError;

/// Run a pod admin command.
pub fn run_pod(rt: &tokio::runtime::Runtime, action: PodAction) {
    rt.block_on(run_pod_inner(action));
}

async fn run_pod_inner(action: PodAction) {
    match action {
        PodAction::ExportContainer { pod_id, output } => {
            match export_container(&pod_id, &output).await {
                Ok(()) => println!(
                    "Pod '{}' exported as container build context to {}",
                    pod_id,
                    output.display()
                ),
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
        }
        PodAction::ExportK8s {
            pod_id,
            volume_size_gb: _,
            max_replicas: _,
            output,
        } => {
            // Validate pod exists before copying manifests
            match get_pod_status(&pod_id).await {
                Ok(_) => match export_k8s(&output) {
                    Ok(count) => {
                        println!(
                            "K8s manifests for '{}' exported to {} ({} files)",
                            pod_id,
                            output.display(),
                            count
                        );
                    }
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                },
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}

fn parse_pod_id(id: &str) -> Result<hkask_pods::pod::PodID, CliError> {
    uuid::Uuid::parse_str(id)
        .map(hkask_pods::pod::PodID::from_uuid)
        .map_err(|_| CliError::InvalidInput(format!("Invalid pod ID '{}'", id)))
}

async fn get_pod_status(pod_id: &str) -> Result<hkask_pods::pod::PodStatusInfo, CliError> {
    let ctx = super::helpers::build_agent_service();
    let pid = parse_pod_id(pod_id)?;
    ctx.infra()
        .pods
        .clone()
        .get_pod_status(&pid)
        .await
        .map_err(|e| CliError::AgentService(format!("Failed to get pod status: {e}")))
}

/// Export a pod as a container build context (delegates to PodFactory).
pub async fn export_container(pod_id: &str, output_dir: &std::path::Path) -> Result<(), CliError> {
    let ctx = super::helpers::build_agent_service();
    let pm = ctx.infra().pods.clone();
    let pid = hkask_pods::pod::PodID::from_name(pod_id);
    pm.export_container(pid, output_dir)
        .map_err(|e| CliError::AgentService(format!("Failed to export container: {e}")))
}

/// Export K8s manifests from the canonical `deploy/k8s/` directory.
///
/// Copies all files from the repo's `deploy/k8s/` into `output_dir`.
/// The canonical manifests include namespace, deployment (kask + Litestream
/// sidecar), service, PVC, configmap, secret, ingress, and the conduit/
/// subdirectory for the standalone Conduit Matrix homeserver.
/// No inline generation — single source of truth in `deploy/k8s/`.
///
/// Source directory resolution via `helpers::resolve_deploy_dir()`.
///
/// Returns the count of copied files.
pub fn export_k8s(output_dir: &std::path::Path) -> Result<usize, CliError> {
    let source_dir = crate::commands::helpers::resolve_deploy_dir()?;

    std::fs::create_dir_all(output_dir)
        .map_err(|e| CliError::Io(format!("Failed to create output directory: {e}")))?;

    let mut copied = 0usize;
    for entry in std::fs::read_dir(&source_dir)
        .map_err(|e| CliError::Io(format!("Cannot read deploy/k8s/ ({source_dir:?}): {e}")))?
    {
        let entry = entry.map_err(|e| CliError::Io(format!("read_dir entry: {e}")))?;
        let path = entry.path();
        if path.is_file() {
            let name = path
                .file_name()
                .ok_or_else(|| CliError::Io("missing filename".into()))?;
            std::fs::copy(&path, output_dir.join(name))
                .map_err(|e| CliError::Io(format!("Failed to copy {name:?}: {e}")))?;
            copied += 1;
        }
    }

    if copied == 0 {
        return Err(CliError::Io(format!("No files found in {source_dir:?}")));
    }

    Ok(copied)
}
