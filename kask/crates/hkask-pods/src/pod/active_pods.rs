//! ActivePods — Runtime registry for active pod deployments.
//!
//! Stores PodFactory + port references so the API matches
//! the old PodManager signature exactly. Zero consumer changes needed.

use super::AgentPodError;
use super::context::PodContext;
use super::deployment::{PodDeployment, PodFactory, PodRegistry};
use super::types::{PodID, PodKind, PodLifecycleState};
use crate::a2a::A2ARuntime;
use crate::curation::SemanticIndex;
use hkask_capability::CapabilityChecker;
use hkask_mcp::McpRuntime;
use hkask_types::InferencePort;
use hkask_types::WebID;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct ActivePods {
    deployments: RwLock<HashMap<PodID, PodDeployment>>,
    factory: Arc<PodFactory>,
    a2a_runtime: Arc<A2ARuntime>,
    mcp_runtime: Arc<McpRuntime>,
    capability_checker: Arc<CapabilityChecker>,
    inference_port: Option<Arc<dyn InferencePort>>,
    /// CuratorPod's SemanticIndex — shared with all pod contexts for
    /// merged-lens semantic recall (Step 5).
    curator_index: RwLock<Option<Arc<std::sync::RwLock<SemanticIndex>>>>,
}

impl ActivePods {
    pub fn new(
        factory: Arc<PodFactory>,
        a2a_runtime: Arc<A2ARuntime>,
        mcp_runtime: Arc<McpRuntime>,
        capability_checker: Arc<CapabilityChecker>,
    ) -> Self {
        Self {
            deployments: RwLock::new(HashMap::new()),
            factory,
            a2a_runtime,
            mcp_runtime,
            capability_checker,
            inference_port: None,
            curator_index: RwLock::new(None),
        }
    }

    /// Create a mock ActivePods for testing — matches old PodManager::new_mock().
    /// Wires in-memory adapters and a test factory so create_pod/activate_pod work.
    /// Full test harness with in-memory adapters, AllowAllConsent,
    /// mock templates, and master key set. One call sets up everything
    /// needed for integration tests.
    #[allow(unsafe_code)]
    pub fn new_test_harness(data_dir: &std::path::Path) -> Self {
        // Set test secrets for OCAP signing and canonical SQLCipher encryption.
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            );
            std::env::set_var("HKASK_DB_PASSPHRASE", "hkask-test-db-passphrase");
        }
        // Create mock template directories
        let tmpl = data_dir.join("templates");
        let persona_yaml = "agent:\n  name: test\n  type: Bot\n  version: \"0.1.0\"\ncharter:\n  description: Test\n  editor: test\n";
        for name in &["curator", "userpod", "team", "solo"] {
            let dir = tmpl.join(name);
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join("agent_persona.yaml"), persona_yaml);
            let _ = std::fs::write(dir.join("dispatch_manifest.yaml"), "selector: test\n");
        }
        Self::new_test_harness_inner(data_dir)
    }

    /// Inner test harness.
    #[allow(unsafe_code)]
    fn new_test_harness_inner(data_dir: &std::path::Path) -> Self {
        use crate::AllowAllConsent;
        use crate::a2a::A2ARuntime;
        use crate::pod::{PodFactory, system_capability_checker};
        use hkask_storage::database::types::DbProvider;

        // A deterministic master key so token issuance and the capability checker
        // derive the SAME system OCAP key. SAFETY: test-only, single-threaded setup.
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            );
        }

        let a2a = Arc::new(A2ARuntime::new(b"mock"));
        // Anchor the checker to BOTH the system OCAP authority (pre-registration
        // tokens) and the A2A root (post-registration tokens), so legitimate pod
        // tokens verify while forged tokens are rejected (matches production).
        let checker = Arc::new(
            system_capability_checker()
                .expect("system capability checker (test master key set)")
                .trust_root(a2a.root_public_key()),
        );
        let factory = Arc::new(PodFactory::new(
            Arc::new(hkask_templates::TemplateCrateLoader::from_path(
                data_dir.join("templates"),
            )),
            Arc::new(AllowAllConsent),
            data_dir.to_path_buf(),
            DbProvider::Sqlite,
        ));
        Self::new(factory, a2a, Arc::new(McpRuntime::new()), checker)
    }

    /// Set the inference port for pods to use.
    #[must_use = "builder methods return self for chaining"]
    pub fn with_inference_port(mut self, port: Arc<dyn InferencePort>) -> Self {
        self.inference_port = Some(port);
        self
    }

    /// Get the inference port, if one is wired.
    pub fn inference_port(&self) -> Option<Arc<dyn InferencePort>> {
        self.inference_port.clone()
    }

    /// Get a clone of the Curator's shared SemanticIndex, if a CuratorPod
    /// has been deployed. Used to construct CuratorSync at startup.
    pub async fn curator_index(&self) -> Option<Arc<std::sync::RwLock<SemanticIndex>>> {
        self.curator_index.read().await.clone()
    }

    pub async fn insert(&self, deployment: PodDeployment) {
        self.deployments
            .write()
            .await
            .insert(deployment.pod_id, deployment);
    }

    pub async fn remove(&self, pod_id: &PodID) -> Option<PodDeployment> {
        self.deployments.write().await.remove(pod_id)
    }

    /// List all active pod names (for daemon self-healing loops).
    pub async fn pod_names(&self) -> Vec<String> {
        self.deployments
            .read()
            .await
            .values()
            .map(|d| d.pod.name.clone())
            .collect()
    }

    /// Get a PodContext for an active pod.
    /// Wires the Curator's SemanticIndex for merged-lens semantic recall (Step 5).
    pub async fn context(&self, pod_id: &PodID) -> Result<PodContext, AgentPodError> {
        let deployments = self.deployments.read().await;
        let deployment = deployments
            .get(pod_id)
            .ok_or(AgentPodError::PodNotFound(*pod_id))?;
        let mut ctx = PodContext::from_deployment(deployment)?;
        // Wire curator index for merged-lens semantic recall (Step 5)
        if let Some(ref curator) = *self.curator_index.read().await {
            ctx = ctx.with_curator_index(Arc::clone(curator));
        }
        Ok(ctx)
    }

    /// Find a pod by userpod name — matches old PodManager::find_pod_by_name.
    pub async fn find_by_name(&self, name: &str) -> Option<PodID> {
        let deployments = self.deployments.read().await;
        for (id, d) in deployments.iter() {
            if d.pod.name == name {
                return Some(*id);
            }
        }
        None
    }

    /// Alias: matches old PodManager API.
    pub async fn find_pod_by_name(&self, name: &str) -> Option<PodID> {
        self.find_by_name(name).await
    }

    /// Get a pod's WebID — matches old PodManager::get_pod_webid.
    pub async fn get_pod_webid(&self, pod_id: &PodID) -> Option<WebID> {
        self.deployments
            .read()
            .await
            .get(pod_id)
            .map(|d| d.pod.webid)
    }

    /// Alias for get_pod_webid.
    pub async fn webid(&self, pod_id: &PodID) -> Option<WebID> {
        self.get_pod_webid(pod_id).await
    }

    pub async fn has_capability(&self, pod_id: &PodID, tool: &str) -> bool {
        self.deployments
            .read()
            .await
            .get(pod_id)
            .map(|d| {
                d.pod
                    .capabilities
                    .iter()
                    .any(|cap| cap == tool || cap.starts_with(&format!("{}:", tool)))
            })
            .unwrap_or(false)
    }

    /// Create a pod — matches old PodManager::create_pod(template, persona, name).
    /// Uses internally stored PodFactory and port adapters.
    pub async fn create_pod(
        &self,
        template_name: &str,
        name: &str,
        webid: WebID,
        capabilities: Vec<String>,
        pod_kind: PodKind,
    ) -> Result<PodID, AgentPodError> {
        let factory = &self.factory;
        let mcp = Arc::clone(&self.mcp_runtime);
        // Enforce CuratorPod singleton (P5 Essentialism).
        if pod_kind == PodKind::Curator {
            let ci = self.curator_index.read().await;
            if ci.is_some() {
                return Err(AgentPodError::DuplicateCurator);
            }
            drop(ci);
        }

        let deployment = factory
            .deploy(
                template_name,
                name,
                webid,
                capabilities,
                pod_kind,
                mcp,
                Arc::clone(&self.capability_checker),
                self.inference_port.clone(),
            )
            .await
            .map_err(|e| AgentPodError::DeployError(e.to_string()))?;
        let pod_id = deployment.pod_id;

        // Step 5: If this is a CuratorPod, wire its SemanticIndex as the
        // shared curator_index that all PodContexts use for merged-lens recall.
        // The sync loop (Step 4) writes to the same Arc<RwLock<>> that PodContext reads from.
        if pod_kind == PodKind::Curator
            && let Some(ref index) = deployment.semantic_index
        {
            let mut ci = self.curator_index.write().await;
            if ci.is_some() {
                return Err(AgentPodError::DuplicateCurator);
            }
            *ci = Some(Arc::clone(index));
        }

        self.insert(deployment).await;
        Ok(pod_id)
    }

    /// Ensure a CuratorPod exists, is activated, and has CuratorSync running.
    /// Idempotent — if a Curator already exists, returns its SemanticIndex.
    /// If not, creates one, activates it, spawns the sync loop.
    ///
    /// Returns the shared SemanticIndex Arc for consumers that need it.
    /// The sync loop runs as a background task until runtime shutdown.
    pub async fn ensure_curator(
        &self,
        data_dir: std::path::PathBuf,
    ) -> Result<Option<Arc<std::sync::RwLock<SemanticIndex>>>, AgentPodError> {
        // Idempotent — if curator already exists, don't spawn a second sync
        {
            let ci = self.curator_index.read().await;
            if let Some(ref index) = *ci {
                return Ok(Some(Arc::clone(index)));
            }
        }

        let (index, registry) = self.create_curator_pod(&data_dir).await?;
        let sync = crate::curation::CuratorSync::new(Arc::clone(&index), registry);
        tokio::spawn(async move {
            sync.run().await;
        });
        tracing::info!("CuratorSync spawned — polling semantic h_mems from all pods");

        Ok(Some(index))
    }

    /// Like `ensure_curator` but returns the `CuratorSync` without spawning
    /// the background loop. Integration tests call `sync.tick()` directly
    /// after storing an h_mem — deterministic, no polling, no timeout.
    pub async fn ensure_curator_for_test(
        &self,
        data_dir: std::path::PathBuf,
    ) -> Result<
        (
            Arc<std::sync::RwLock<SemanticIndex>>,
            crate::curation::CuratorSync,
        ),
        AgentPodError,
    > {
        let (index, registry) = self.create_curator_pod(&data_dir).await?;
        let sync = crate::curation::CuratorSync::new(Arc::clone(&index), registry);
        Ok((index, sync))
    }

    /// Create the CuratorPod, activate it, and build the PodRegistry.
    /// Shared by `ensure_curator` (production) and `ensure_curator_for_test` (tests).
    async fn create_curator_pod(
        &self,
        data_dir: &std::path::Path,
    ) -> Result<(Arc<std::sync::RwLock<SemanticIndex>>, Arc<PodRegistry>), AgentPodError> {
        // Check if curator already exists
        {
            let ci = self.curator_index.read().await;
            if let Some(ref index) = *ci {
                // Already exists — build registry from data_dir, return existing index
                let registry = Arc::new(PodRegistry::new(data_dir));
                return Ok((Arc::clone(index), registry));
            }
        }

        // Create CuratorPod — system daemon, identity derived from "curator".
        let curator_webid = WebID::from_persona(b"curator");
        let pod_id = self
            .create_pod(
                "curator",
                "curator",
                curator_webid,
                vec!["tool:execute".to_string()],
                PodKind::Curator,
            )
            .await?;

        // Activate it
        self.activate_pod(&pod_id).await?;

        // Extract the SemanticIndex (set by create_pod when PodKind::Curator)
        let index = {
            let ci = self.curator_index.read().await;
            ci.clone()
                .ok_or_else(|| AgentPodError::SemanticIndexMissing)?
        };

        let registry = Arc::new(PodRegistry::new(data_dir));
        Ok((index, registry))
    }

    /// Activate a pod — matches old PodManager::activate_pod(id).
    /// Handles full lifecycle: Populated → Registered → Activated.
    pub async fn activate_pod(&self, pod_id: &PodID) -> Result<(), AgentPodError> {
        let a2a = &self.a2a_runtime;

        // Register with A2A if still Populated
        let registration_data = {
            let d = self.deployments.read().await;
            let d = d.get(pod_id).ok_or(AgentPodError::PodNotFound(*pod_id))?;
            if d.pod.state == PodLifecycleState::Active {
                Some((d.pod.webid, d.pod.capabilities.clone()))
            } else {
                None
            }
        };

        // Perform A2A registration outside the write lock
        let token = if let Some((webid, capabilities)) = registration_data {
            Some(
                a2a.register_agent(webid, capabilities)
                    .await
                    .map_err(|e| AgentPodError::A2ARegistrationError(e.to_string()))?,
            )
        } else {
            None
        };

        // Apply registration token + activate
        let mut d = self.deployments.write().await;
        let d = d
            .get_mut(pod_id)
            .ok_or(AgentPodError::PodNotFound(*pod_id))?;
        if let Some(token) = token {
            d.pod.capability_token = token;
            d.pod.state = PodLifecycleState::Active;
        }
        d.pod.activate(&self.capability_checker)
    }

    /// Sleep a pod — transitions Active → Sleeping (user logged out / inactive).
    pub async fn sleep_pod(&self, pod_id: &PodID) -> Result<(), AgentPodError> {
        let mut d = self.deployments.write().await;
        d.get_mut(pod_id)
            .ok_or(AgentPodError::PodNotFound(*pod_id))?
            .pod
            .sleep()
    }

    /// Get pod status — matches old PodManager::get_pod_status(id).
    pub async fn get_pod_status(&self, pod_id: &PodID) -> Result<PodStatusInfo, AgentPodError> {
        let d = self.deployments.read().await;
        let d = d.get(pod_id).ok_or(AgentPodError::PodNotFound(*pod_id))?;
        Ok(PodStatusInfo {
            pod_id: d.pod_id.to_string(),
            name: Some(d.pod.name.clone()),
            state: d.pod.state,
            webid: d.pod.webid.to_string(),
            template: d.pod.template_crate.name.clone(),
            pod_kind: d.pod_kind,
            created_at: d.pod.created_at,
        })
    }

    /// List all pods — matches old PodManager::list_pods().
    pub async fn list_pods(&self) -> Result<Vec<PodStatusInfo>, AgentPodError> {
        self.deployments
            .read()
            .await
            .values()
            .map(|d| {
                Ok(PodStatusInfo {
                    pod_id: d.pod_id.to_string(),
                    name: Some(d.pod.name.clone()),
                    state: d.pod.state,
                    webid: d.pod.webid.to_string(),
                    template: d.pod.template_crate.name.clone(),
                    pod_kind: d.pod_kind,
                    created_at: d.pod.created_at,
                })
            })
            .collect()
    }

    /// Return (pod_name, pod_db_path) for all activated pods with existing databases.
    /// Used by the backup system to snapshot pod state.
    pub async fn pod_db_paths(&self) -> Vec<(String, std::path::PathBuf)> {
        self.deployments
            .read()
            .await
            .values()
            .filter(|d| d.storage.db_path.exists())
            .map(|d| (d.pod.name.clone(), d.storage.db_path.clone()))
            .collect()
    }

    /// Export a pod as a container build context (delegates to PodFactory).
    pub fn export_container(
        &self,
        pod_id: PodID,
        output_dir: &std::path::Path,
    ) -> Result<(), super::AgentPodError> {
        self.factory
            .export_container(pod_id, output_dir)
            .map_err(|e| super::AgentPodError::DeployError(e.to_string()))
    }
}

#[derive(Debug, Clone)]
pub struct PodStatusInfo {
    pub pod_id: String,
    pub name: Option<String>,
    pub state: PodLifecycleState,
    pub webid: String,
    pub template: String,
    pub pod_kind: PodKind,
    pub created_at: i64,
}
