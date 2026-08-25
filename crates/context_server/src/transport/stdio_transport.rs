use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use futures::io::{BufReader, BufWriter};
use futures::{
    AsyncBufReadExt as _, AsyncRead, AsyncWrite, AsyncWriteExt as _, Stream, StreamExt as _,
};
use gpui::AsyncApp;

use util::TryFutureExt as _;
use util::process::Child;
use util::shell::Shell;
use util::shell_builder::ShellBuilder;

use crate::client::ModelContextServerBinary;
use crate::transport::Transport;

pub struct StdioTransport {
    stdout_sender: async_channel::Sender<String>,
    stdin_receiver: async_channel::Receiver<String>,
    stderr_receiver: async_channel::Receiver<String>,
    server: Child,
    /// Server identity for attributed logging. Without this, a child exiting
    /// surfaces as an unattributed `Broken pipe (os error 32)` ERROR — the
    /// operator cannot tell which server died.
    server_id: Arc<str>,
}

impl StdioTransport {
    pub fn new(
        binary: ModelContextServerBinary,
        working_directory: &Option<PathBuf>,
        server_id: Arc<str>,
        cx: &AsyncApp,
    ) -> Result<Self> {
        let builder = ShellBuilder::new(&Shell::System, cfg!(windows)).non_interactive();
        let mut command =
            builder.build_std_command(Some(binary.executable.display().to_string()), &binary.args);

        command.envs(binary.env.unwrap_or_default());

        if let Some(working_directory) = working_directory {
            command.current_dir(working_directory);
        }

        let mut server = Child::spawn(
            command,
            std::process::Stdio::piped(),
            std::process::Stdio::piped(),
            std::process::Stdio::piped(),
        )?;

        let stdin = server.stdin.take().unwrap();
        let stdout = server.stdout.take().unwrap();
        let stderr = server.stderr.take().unwrap();

        let (stdin_sender, stdin_receiver) = async_channel::unbounded::<String>();
        let (stdout_sender, stdout_receiver) = async_channel::unbounded::<String>();
        let (stderr_sender, stderr_receiver) = async_channel::unbounded::<String>();

        let log_server_id = server_id.clone();
        cx.spawn(async move |_| {
            Self::handle_output(stdin, stdout_receiver, log_server_id)
                .log_err()
                .await
        })
        .detach();

        let log_server_id = server_id.clone();
        cx.spawn(async move |_| Self::handle_input(stdout, stdin_sender, log_server_id).await)
            .detach();

        let log_server_id = server_id.clone();
        cx.spawn(async move |_| Self::handle_err(stderr, stderr_sender, log_server_id).await)
            .detach();

        Ok(Self {
            stdout_sender,
            stdin_receiver,
            stderr_receiver,
            server,
            server_id,
        })
    }

    async fn handle_input<Stdout>(
        stdin: Stdout,
        inbound_rx: async_channel::Sender<String>,
        server_id: Arc<str>,
    ) where
        Stdout: AsyncRead + Unpin + Send + 'static,
    {
        let mut stdin = BufReader::new(stdin);
        let mut line = String::new();
        while let Ok(n) = stdin.read_line(&mut line).await {
            if n == 0 {
                log::debug!("context server {server_id} stdout closed (EOF)");
                break;
            }
            if inbound_rx.send(line.clone()).await.is_err() {
                log::debug!("context server {server_id} stdout receiver dropped — stopping reader");
                break;
            }
            line.clear();
        }
    }

    async fn handle_output<Stdin>(
        stdin: Stdin,
        outbound_rx: async_channel::Receiver<String>,
        server_id: Arc<str>,
    ) -> Result<()>
    where
        Stdin: AsyncWrite + Unpin + Send + 'static,
    {
        let mut stdin = BufWriter::new(stdin);
        let mut pinned_rx = Box::pin(outbound_rx);
        while let Some(message) = pinned_rx.next().await {
            log::trace!("context server {server_id} outgoing message: {}", message);

            if let Err(err) = stdin.write_all(message.as_bytes()).await {
                // A child exiting before we finish writing surfaces as
                // `Broken pipe (os error 32)`. This is expected when the server
                // shuts down (clean exit, credential failure, crash) — log at
                // debug with the server_id so the operator can attribute it,
                // and propagate the error so the task ends.
                log::debug!("context server {server_id} stdin write failed: {err}");
                return Err(err.into());
            }
            stdin.write_all(b"\n").await?;
            stdin.flush().await?;
        }
        Ok(())
    }

    async fn handle_err<Stderr>(
        stderr: Stderr,
        stderr_tx: async_channel::Sender<String>,
        server_id: Arc<str>,
    ) where
        Stderr: AsyncRead + Unpin + Send + 'static,
    {
        let mut stderr = BufReader::new(stderr);
        let mut line = String::new();
        while let Ok(n) = stderr.read_line(&mut line).await {
            if n == 0 {
                log::debug!("context server {server_id} stderr closed (EOF)");
                break;
            }
            if stderr_tx.send(line.clone()).await.is_err() {
                log::debug!("context server {server_id} stderr receiver dropped — stopping reader");
                break;
            }
            line.clear();
        }
    }
}

#[async_trait]
impl Transport for StdioTransport {
    async fn send(&self, message: String) -> Result<()> {
        Ok(self.stdout_sender.send(message).await?)
    }

    fn receive(&self) -> Pin<Box<dyn Stream<Item = String> + Send>> {
        Box::pin(self.stdin_receiver.clone())
    }

    fn receive_err(&self) -> Pin<Box<dyn Stream<Item = String> + Send>> {
        Box::pin(self.stderr_receiver.clone())
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        // Distinguish a clean kill (child already exited) from a forced kill
        // (child still running) so the operator can attribute an unexpected
        // transport teardown. The `server_id` field exists for exactly this
        // attribution — without it, `Drop` was silently killing the child.
        let server_id = &self.server_id;
        match self.server.try_status() {
            Ok(Some(status)) => {
                log::debug!(
                    "context server {server_id} already exited with status {status} — no kill needed"
                );
            }
            Ok(None) => {
                log::debug!("context server {server_id} still running — killing on transport drop");
                let _ = self.server.kill();
            }
            Err(err) => {
                log::warn!("context server {server_id} failed to poll status before kill: {err}");
                let _ = self.server.kill();
            }
        }
    }
}
