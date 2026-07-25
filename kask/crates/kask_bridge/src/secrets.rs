//! `SecretsPort` adapter over zed's `CredentialsProvider`.
//!
//! hKask MCP servers and the trimmed keystore read API keys and sovereignty secrets
//! through `SecretsPort`. This adapter stores them in the OS keychain via zed's
//! `CredentialsProvider`, namespaced under `kask://credentials/<key>` so they don't
//! collide with zed's own provider keys (which use their provider URLs).
//!
//! `AsyncApp` is not `Send` (GPUI's `ForegroundExecutor` holds `Rc`-based state),
//! so the bridge uses a channel: trait methods send a request to a GPUI-side task
//! that holds the `AsyncApp` and executes the credential call, then returns the
//! result. The adapter struct itself only holds a channel sender (`Send + Sync`).

use std::sync::Arc;

use credentials_provider::CredentialsProvider;
use futures_util::FutureExt;
use gpui::AsyncApp;
use hkask_types::SecretsPort;
use tokio::sync::oneshot;

/// The URL prefix for kask-namespaced credentials in the keychain.
pub const KASK_CREDENTIAL_NAMESPACE: &str = "kask://credentials";

/// Request sent from the tokio side (trait method) to the GPUI side (executor).
enum CredentialRequest {
    Read {
        key: String,
        reply: oneshot::Sender<Result<Option<String>, String>>,
    },
    Write {
        key: String,
        value: String,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Delete {
        key: String,
        reply: oneshot::Sender<Result<(), String>>,
    },
}

/// `SecretsPort` implementation over zed's `CredentialsProvider`.
///
/// Keys are namespaced: a `SecretsPort::read("fmp")` call reads the credential
/// stored at URL `kask://credentials/fmp`. This keeps kask secrets isolated from
/// zed's own provider credentials (which use their provider URLs as keys).
///
/// The adapter holds only a channel sender (`Send + Sync`); the actual credential
/// I/O happens on the GPUI side via a spawned task that owns the `AsyncApp`.
pub struct CredentialsSecretsPort {
    tx: tokio::sync::mpsc::UnboundedSender<CredentialRequest>,
}

impl CredentialsSecretsPort {
    /// Construct the adapter and spawn the GPUI-side receiver task.
    ///
    /// The receiver task runs on the GPUI foreground executor and processes
    /// credential requests. Drop the returned `Task` to stop it.
    pub fn new(provider: Arc<dyn CredentialsProvider>, cx: AsyncApp) -> (Self, gpui::Task<()>) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<CredentialRequest>();

        // Spawn a GPUI task that owns the AsyncApp and processes requests.
        let task = cx.spawn(async move |cx| {
            while let Some(req) = rx.recv().await {
                match req {
                    CredentialRequest::Read { key, reply } => {
                        let url = format!("{KASK_CREDENTIAL_NAMESPACE}/{key}");
                        let result = provider
                            .read_credentials(&url, &cx)
                            .await
                            .map(|opt| {
                                opt.map(|(_user, pass)| String::from_utf8_lossy(&pass).to_string())
                            })
                            .map_err(|e| e.to_string());
                        let _ = reply.send(result);
                    }
                    CredentialRequest::Write { key, value, reply } => {
                        let url = format!("{KASK_CREDENTIAL_NAMESPACE}/{key}");
                        let result = provider
                            .write_credentials(&url, "kask", value.as_bytes(), &cx)
                            .await
                            .map_err(|e| e.to_string());
                        let _ = reply.send(result);
                    }
                    CredentialRequest::Delete { key, reply } => {
                        let url = format!("{KASK_CREDENTIAL_NAMESPACE}/{key}");
                        let result = provider
                            .delete_credentials(&url, &cx)
                            .await
                            .map_err(|e| e.to_string());
                        let _ = reply.send(result);
                    }
                }
            }
        });

        (Self { tx }, task)
    }
}

impl SecretsPort for CredentialsSecretsPort {
    fn read<'a>(
        &'a self,
        key: &'a str,
    ) -> hkask_types::SecretsFuture<'a, Result<Option<String>, hkask_types::SecretsError>> {
        let (tx_reply, rx_reply) = oneshot::channel();
        let key = key.to_string();
        async move {
            self.tx
                .send(CredentialRequest::Read {
                    key,
                    reply: tx_reply,
                })
                .map_err(|e| hkask_types::SecretsError::Store(e.to_string()))?;
            rx_reply
                .await
                .map_err(|e| hkask_types::SecretsError::Store(e.to_string()))?
                .map_err(|e| hkask_types::SecretsError::Store(e))
        }
        .boxed()
    }

    fn write<'a>(
        &'a self,
        key: &'a str,
        value: &'a str,
    ) -> hkask_types::SecretsFuture<'a, Result<(), hkask_types::SecretsError>> {
        let (tx_reply, rx_reply) = oneshot::channel();
        let key = key.to_string();
        let value = value.to_string();
        async move {
            self.tx
                .send(CredentialRequest::Write {
                    key,
                    value,
                    reply: tx_reply,
                })
                .map_err(|e| hkask_types::SecretsError::Store(e.to_string()))?;
            rx_reply
                .await
                .map_err(|e| hkask_types::SecretsError::Store(e.to_string()))?
                .map_err(|e| hkask_types::SecretsError::Store(e))
        }
        .boxed()
    }

    fn delete<'a>(
        &'a self,
        key: &'a str,
    ) -> hkask_types::SecretsFuture<'a, Result<(), hkask_types::SecretsError>> {
        let (tx_reply, rx_reply) = oneshot::channel();
        let key = key.to_string();
        async move {
            self.tx
                .send(CredentialRequest::Delete {
                    key,
                    reply: tx_reply,
                })
                .map_err(|e| hkask_types::SecretsError::Store(e.to_string()))?;
            rx_reply
                .await
                .map_err(|e| hkask_types::SecretsError::Store(e.to_string()))?
                .map_err(|e| hkask_types::SecretsError::Store(e))
        }
        .boxed()
    }
}
