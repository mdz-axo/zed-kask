//! Test relays for this server's network tests.
//!
//! Compiled only with the `test-fixture` feature; the installed server never
//! contains it. Every relay here listens on the loopback address at a port
//! the operating system or the relay engine picks, and contacts nothing.
//!
//! Three kinds, one per outcome the server must tell apart:
//!
//! - [`TestRelay`] — a working relay (the SDK's own `LocalRelay`), which can
//!   also be started refusing every published event with a stated message;
//! - [`stopped_relay_url`] — an address nothing listens on;
//! - [`HandshakeFailingRelay`] — a listener that accepts the TCP connection
//!   and then fails the websocket handshake.

use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;

use nostr_sdk::prelude::*;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Why a test relay could not be set up.
#[derive(Debug)]
pub enum TestRelayError {
    /// The relay engine or listener did not start.
    Start(String),
    /// The event was not stored.
    Store(String),
}

impl std::fmt::Display for TestRelayError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Start(reason) => write!(formatter, "the test relay did not start: {reason}"),
            Self::Store(reason) => write!(formatter, "the test relay did not store: {reason}"),
        }
    }
}

impl std::error::Error for TestRelayError {}

/// Refuses every published event with one fixed message.
#[derive(Debug)]
struct RejectEverything {
    message: &'static str,
}

impl WritePolicy for RejectEverything {
    fn admit_event<'a>(
        &'a self,
        _event: &'a Event,
        _addr: &'a SocketAddr,
    ) -> Pin<Box<dyn Future<Output = WritePolicyResult> + Send + 'a>> {
        Box::pin(
            async move { WritePolicyResult::reject(MachineReadablePrefix::Blocked, self.message) },
        )
    }
}

/// A working relay on loopback. It stops when dropped.
#[derive(Debug)]
pub struct TestRelay {
    relay: LocalRelay,
    url: RelayUrl,
}

impl TestRelay {
    /// Start a relay that accepts every event.
    pub async fn start() -> Result<Self, TestRelayError> {
        Self::run(LocalRelay::builder()).await
    }

    /// Start a relay that refuses every published event with `message`.
    pub async fn start_rejecting(message: &'static str) -> Result<Self, TestRelayError> {
        Self::run(LocalRelay::builder().write_policy(RejectEverything { message })).await
    }

    async fn run(builder: LocalRelayBuilder) -> Result<Self, TestRelayError> {
        // Loopback, and no port named: the engine picks a free one.
        let relay = builder.addr(Ipv4Addr::LOCALHOST.into()).build();
        relay
            .run()
            .await
            .map_err(|error| TestRelayError::Start(error.to_string()))?;
        let url = relay.url().await;
        Ok(Self { relay, url })
    }

    /// The relay's `ws://` address.
    pub fn url(&self) -> RelayUrl {
        self.url.clone()
    }

    /// Put an event in the relay's store, as if it had been published. The
    /// engine verifies the event's id and signature first; the reject policy
    /// of [`TestRelay::start_rejecting`] does not apply to this path.
    pub async fn store(&self, event: Event) -> Result<(), TestRelayError> {
        let status = self
            .relay
            .add_event(event)
            .await
            .map_err(|error| TestRelayError::Store(error.to_string()))?;
        if status.is_success() {
            Ok(())
        } else {
            Err(TestRelayError::Store(format!("{status:?}")))
        }
    }
}

/// A listener that accepts connections and fails the websocket handshake:
/// it closes each connection as soon as it is accepted, so the client's
/// upgrade request gets no answer. It stops when dropped.
#[derive(Debug)]
pub struct HandshakeFailingRelay {
    url: RelayUrl,
    accept_loop: JoinHandle<()>,
}

impl HandshakeFailingRelay {
    /// Start the listener.
    pub async fn start() -> Result<Self, TestRelayError> {
        let listener = bind_loopback().await?;
        let url = loopback_url(&listener)?;
        let accept_loop = tokio::spawn(async move {
            while let Ok((stream, _peer)) = listener.accept().await {
                drop(stream);
            }
        });
        Ok(Self { url, accept_loop })
    }

    /// The listener's `ws://` address.
    pub fn url(&self) -> RelayUrl {
        self.url.clone()
    }
}

impl Drop for HandshakeFailingRelay {
    fn drop(&mut self) {
        self.accept_loop.abort();
    }
}

/// The address of a relay that is not running: a loopback port the operating
/// system just handed out and that nothing listens on any more.
pub async fn stopped_relay_url() -> Result<RelayUrl, TestRelayError> {
    let listener = bind_loopback().await?;
    loopback_url(&listener)
}

/// Bind loopback at a port the operating system picks.
async fn bind_loopback() -> Result<TcpListener, TestRelayError> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|error| TestRelayError::Start(error.to_string()))
}

fn loopback_url(listener: &TcpListener) -> Result<RelayUrl, TestRelayError> {
    let address = listener
        .local_addr()
        .map_err(|error| TestRelayError::Start(error.to_string()))?;
    RelayUrl::parse(&format!("ws://{address}"))
        .map_err(|error| TestRelayError::Start(error.to_string()))
}
