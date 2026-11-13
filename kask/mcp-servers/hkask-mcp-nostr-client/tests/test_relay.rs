//! The local test relay, driven the way the server's later network code will
//! drive a relay: through the SDK's own client, over a real websocket on
//! loopback.
//!
//! Compiled only with the `test-fixture` feature. Nothing here leaves the
//! machine, and every key is generated inside the test.

#![cfg(feature = "test-fixture")]

use std::time::Duration;

use hkask_mcp_nostr_client::test_support::{HandshakeFailingRelay, TestRelay, stopped_relay_url};
use nostr_sdk::prelude::*;

/// How long any one step may take before the test fails instead of hanging.
const STEP: Duration = Duration::from_secs(10);

fn note(keys: &Keys, kind: Kind, text: &str) -> Event {
    EventBuilder::new(kind, text)
        .finalize(keys)
        .expect("a throwaway key signs")
}

async fn connected_client(url: &RelayUrl) -> Client {
    let client = Client::default();
    client.add_relay(url.clone()).await.expect("relay added");
    let connected = client.try_connect().timeout(STEP).await;
    assert!(
        connected.success.contains_key(url),
        "the test relay accepts a connection: {:?}",
        connected.failed
    );
    client
}

/// Everything the relay sent for one subscription, in arrival order, up to
/// and including end-of-stored-events.
#[derive(Debug, PartialEq, Eq)]
enum Frame {
    Event(EventId),
    EndOfStoredEvents,
}

async fn frames_until_end_of_stored_events(client: &Client, filter: Filter) -> Vec<Frame> {
    // Listen before subscribing: the stream only carries what arrives after
    // it was opened.
    let mut notifications = client.notifications();
    let subscription = client
        .subscribe(filter)
        .await
        .expect("the subscription is sent");
    let id = subscription.id().clone();

    let mut frames = Vec::new();
    let collected = tokio::time::timeout(STEP, async {
        while let Some(notification) = notifications.next().await {
            let ClientNotification::Message { message, .. } = notification else {
                continue;
            };
            match *message {
                RelayMessage::Event {
                    subscription_id,
                    event,
                } if *subscription_id == id => frames.push(Frame::Event(event.id)),
                RelayMessage::EndOfStoredEvents(subscription_id) if *subscription_id == id => {
                    frames.push(Frame::EndOfStoredEvents);
                    return;
                }
                _ => {}
            }
        }
    })
    .await;
    assert!(
        collected.is_ok(),
        "no end-of-stored-events within {STEP:?}; got {frames:?}"
    );
    frames
}

/// expect: a subscription is answered with the stored events that match the
/// filter, then end-of-stored-events.
/// pre: three stored events: two notes by one author, one note by another.
/// post: a filter on the first author yields exactly that author's two event
/// ids and then end-of-stored-events as the last frame; the other author's
/// event is not sent.
#[tokio::test]
async fn test_relay_answers_a_subscription_with_matching_events_then_eose() {
    let relay = TestRelay::start().await.expect("the test relay starts");
    let author = Keys::generate();
    let other = Keys::generate();
    let first = note(&author, Kind::TextNote, "first");
    let second = note(&author, Kind::TextNote, "second");
    let unrelated = note(&other, Kind::TextNote, "unrelated");
    for event in [&first, &second, &unrelated] {
        relay.store(event.clone()).await.expect("the event stores");
    }

    let client = connected_client(&relay.url()).await;
    let frames =
        frames_until_end_of_stored_events(&client, Filter::new().author(author.public_key())).await;
    client.shutdown().await;

    assert_eq!(
        frames.last(),
        Some(&Frame::EndOfStoredEvents),
        "end-of-stored-events closes the stored events: {frames:?}"
    );
    let mut sent: Vec<EventId> = frames
        .iter()
        .filter_map(|frame| match frame {
            Frame::Event(id) => Some(*id),
            Frame::EndOfStoredEvents => None,
        })
        .collect();
    sent.sort();
    let mut expected = vec![first.id, second.id];
    expected.sort();
    assert_eq!(sent, expected);
}

/// expect: a published event is answered with an OK message.
/// pre: a running test relay and one signed note.
/// post: the relay accepts it, and a second client can read it back.
#[tokio::test]
async fn test_relay_answers_a_published_event_with_ok() {
    let relay = TestRelay::start().await.expect("the test relay starts");
    let event = note(&Keys::generate(), Kind::TextNote, "published");

    let client = connected_client(&relay.url()).await;
    let output = tokio::time::timeout(STEP, client.send_event(&event))
        .await
        .expect("an answer within the step limit")
        .expect("the event is sent");
    client.shutdown().await;
    assert!(
        output.success.contains_key(&relay.url()),
        "the relay answered OK true: {:?}",
        output.failed
    );
    assert!(output.failed.is_empty(), "{:?}", output.failed);

    let reader = connected_client(&relay.url()).await;
    let stored = reader
        .fetch_events(Filter::new().id(event.id))
        .timeout(STEP)
        .await
        .expect("the fetch completes");
    reader.shutdown().await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored.first().map(|stored| stored.id), Some(event.id));
}

/// expect: the relay can be told to reject, with a stated message, and it
/// stores nothing it rejected.
/// pre: a test relay told to reject every event with a message.
/// post: the publish fails and the failure carries that message; the event
/// cannot be read back.
#[tokio::test]
async fn test_relay_rejects_with_the_stated_message() {
    const MESSAGE: &str = "this relay accepts nothing today";
    let relay = TestRelay::start_rejecting(MESSAGE)
        .await
        .expect("the test relay starts");
    let event = note(&Keys::generate(), Kind::TextNote, "refused");

    let client = connected_client(&relay.url()).await;
    let outcome = tokio::time::timeout(STEP, client.send_event(&event))
        .await
        .expect("an answer within the step limit");
    let failures: Vec<String> = match outcome {
        Ok(output) => {
            assert!(
                output.success.is_empty(),
                "a rejecting relay accepted the event"
            );
            output.failed.values().map(ToString::to_string).collect()
        }
        Err(error) => vec![error.to_string()],
    };
    assert!(
        failures.iter().any(|failure| failure.contains(MESSAGE)),
        "the rejection carries the stated message: {failures:?}"
    );

    let stored = client
        .fetch_events(Filter::new().id(event.id))
        .timeout(STEP)
        .await
        .expect("the fetch completes");
    client.shutdown().await;
    assert!(stored.is_empty(), "a rejected event was stored");
}

/// expect: a stopped relay is an address nothing listens on.
/// pre: the address of a relay that was started and then stopped.
/// post: a plain TCP connection is refused, and the SDK client reports the
/// relay as failed with a reason.
#[tokio::test]
async fn test_relay_can_be_stopped() {
    let url = stopped_relay_url().await.expect("a free port is found");
    let address = socket_address(&url);
    let refused = tokio::net::TcpStream::connect(address).await;
    assert!(
        refused.is_err(),
        "nothing listens at a stopped relay's address"
    );

    let client = Client::default();
    client.add_relay(url.clone()).await.expect("relay added");
    let connected = client.try_connect().timeout(STEP).await;
    client.shutdown().await;
    assert!(connected.success.is_empty());
    let reason = connected.failed.get(&url).expect("the failure is reported");
    assert!(!reason.is_empty(), "the failure has a reason");
}

/// expect: a relay can be started failing its handshake, and that is told
/// apart from a stopped one: something listens, and the upgrade fails.
/// pre: a relay started in handshake-failing mode.
/// post: a plain TCP connection succeeds; the SDK client reports the relay as
/// failed with a reason.
#[tokio::test]
async fn test_relay_can_fail_its_handshake() {
    let relay = HandshakeFailingRelay::start()
        .await
        .expect("the listener starts");
    let url = relay.url();
    let accepted = tokio::net::TcpStream::connect(socket_address(&url)).await;
    assert!(
        accepted.is_ok(),
        "a handshake-failing relay still accepts the TCP connection"
    );
    drop(accepted);

    let client = Client::default();
    client.add_relay(url.clone()).await.expect("relay added");
    let connected = client.try_connect().timeout(STEP).await;
    client.shutdown().await;
    assert!(connected.success.is_empty());
    let reason = connected.failed.get(&url).expect("the failure is reported");
    assert!(!reason.is_empty(), "the failure has a reason");
}

/// expect: no fixed port, and nothing outside the machine.
/// pre: several test relays of each kind, started together.
/// post: every address is `ws://` on the loopback address, and all ports
/// differ, so tests can run side by side.
#[tokio::test]
async fn test_relays_bind_loopback_on_distinct_ports() {
    let first = TestRelay::start().await.expect("starts");
    let second = TestRelay::start().await.expect("starts");
    let rejecting = TestRelay::start_rejecting("no").await.expect("starts");
    let failing = HandshakeFailingRelay::start().await.expect("starts");
    let stopped = stopped_relay_url().await.expect("a free port is found");

    let urls = [
        first.url(),
        second.url(),
        rejecting.url(),
        failing.url(),
        stopped,
    ];
    let mut ports = Vec::new();
    for url in &urls {
        assert_eq!(url.scheme(), RelayUrlScheme::Ws, "{url}");
        let address = socket_address(url);
        assert!(address.ip().is_loopback(), "{url} is not loopback");
        ports.push(address.port());
    }
    ports.sort_unstable();
    ports.dedup();
    assert_eq!(ports.len(), urls.len(), "two test relays share a port");
}

fn socket_address(url: &RelayUrl) -> std::net::SocketAddr {
    let text = url.as_str_without_trailing_slash();
    let authority = text
        .strip_prefix("ws://")
        .unwrap_or_else(|| panic!("{text} is a ws:// address"));
    authority
        .parse()
        .unwrap_or_else(|error| panic!("{authority} is an ip:port: {error}"))
}
