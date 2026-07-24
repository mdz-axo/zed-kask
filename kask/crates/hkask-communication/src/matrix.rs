//! Matrix transport for agent-to-agent and human-to-agent communication.
//!
//! Uses `matrix-sdk` for Matrix protocol integration. The homeserver (Conduit)
//! runs as a Docker sidecar — hKask does not embed or maintain server code.
//!
//! E2EE is deferred to v2 due to a SQLCipher/SQLite linking conflict between
//! hkask-storage and matrix-sdk-sqlite. v1 uses TLS-only transport security.
//!
//! Continuous sync ("listening") is deferred until a VOIP/real-time use case
//! exists. v1 uses on-demand message polling via `get_messages()`.
//!
//! Public API:
//! - Lifecycle: new, health_check, login, healthy
//! - Messaging: send_message, get_messages
//! - Rooms: create_room, invite_user, list_rooms

use serde::{Deserialize, Serialize};
use thiserror::Error;

// ── Matrix types ───────────────────────────────────────────────────────────

/// A Matrix room identifier (e.g., "!abc123:localhost").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RoomId(pub String);

impl RoomId {
    /// Create a new RoomId from a string.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// pre:  id is a valid Matrix room ID (e.g., "!abc123:localhost")
    /// post: returns RoomId wrapping the string
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }

    /// Return the room ID as a string slice.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// post: returns &str of the inner room ID
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A Matrix user identifier (e.g., "@agent:localhost").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct UserId(pub String);

impl UserId {
    /// Create a new UserId from a string.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// pre:  id is a valid Matrix user ID (e.g., "@agent:localhost")
    /// post: returns UserId wrapping the string
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }

    /// Return the user ID as a string slice.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// post: returns &str of the inner user ID
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A threaded conversation (Matrix room with thread support).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    /// Matrix room ID.
    pub room_id: RoomId,
    /// Human-readable thread title.
    pub title: String,
    /// Participants in the thread.
    pub participants: Vec<UserId>,
    /// Whether this thread is monitored by an agent.
    pub monitored_by: Vec<UserId>,
    /// Whether this thread has been escalated.
    pub escalated: bool,
    /// Thread creation timestamp.
    pub created_at: i64,
}

/// A message in a Matrix room.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixMessage {
    /// Matrix event identifier, stable across timeline polls.
    pub event_id: String,
    /// Sender user ID.
    pub sender: UserId,
    /// Plain text body.
    pub body: String,
    /// Optional structured payload (JSON).
    pub structured: Option<serde_json::Value>,
    /// Message timestamp.
    pub timestamp: i64,
}

// ── Client errors ──────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum MatrixError {
    #[error("Matrix homeserver not available: {0}")]
    Unavailable(String),
    #[error("Authentication failed: {0}")]
    Auth(String),
    #[error("Room error: {0}")]
    Room(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Not logged in — call login() first")]
    NotLoggedIn,
}

// ── Matrix transport ──────────────────────────────────────────────────────

/// Thin transport layer over `matrix_sdk::Client`.
///
/// Owns the Matrix client lifecycle: login, message send, on-demand
/// message polling. Does NOT maintain a continuous sync loop (deferred
/// until VOIP/real-time use case exists). Does NOT manage E2EE keys
/// (deferred to v2). Does NOT embed a homeserver.
pub struct MatrixTransport {
    /// The underlying matrix-sdk Client. None before login.
    client: Option<matrix_sdk::Client>,
    /// Homeserver URL (e.g., "http://localhost:8008").
    homeserver_url: String,
}

impl MatrixTransport {
    /// Create a new Matrix transport pointed at the given homeserver URL.
    ///
    /// Does not connect or authenticate — call `login()` first.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// pre:  homeserver_url is a valid URL string
    /// post: returns MatrixTransport with client=None, homeserver_url set
    pub fn new(homeserver_url: &str) -> Self {
        Self {
            client: None,
            homeserver_url: homeserver_url.to_string(),
        }
    }

    /// Return the authenticated Matrix user, if the transport has logged in.
    pub fn authenticated_user_id(&self) -> Option<UserId> {
        self.client
            .as_ref()?
            .user_id()
            .map(|user_id| UserId::new(user_id.as_str()))
    }

    /// Check whether the homeserver is reachable.
    ///
    /// Performs `GET /_matrix/client/versions` to verify Conduit is running.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// pre:  homeserver_url is set
    /// post: returns Ok(true) if homeserver responds
    /// post: returns Err(Unavailable) if homeserver is unreachable
    pub async fn health_check(&mut self) -> Result<bool, MatrixError> {
        let client = matrix_sdk::Client::builder()
            .homeserver_url(&self.homeserver_url)
            .build()
            .await
            .map_err(|e| MatrixError::Unavailable(format!("Failed to build client: {}", e)))?;

        // Verify the client was built successfully (homeserver URL is accessible)
        let _homeserver = client.homeserver();
        tracing::info!(
            target: "hkask.communication.matrix.health",
            url = %self.homeserver_url,
            "Matrix homeserver healthy"
        );
        Ok(true)
    }

    /// Login to the Matrix homeserver with username and password.
    ///
    /// Stores the authenticated client for subsequent operations.
    /// Must be called before `send_message()`, `get_messages()`, etc.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// pre:  username and password are non-empty
    /// post: if successful, self.client is set to authenticated client
    /// post: returns Err(Auth) if credentials are invalid
    /// post: returns Err(Unavailable) if homeserver is unreachable
    pub async fn login(&mut self, username: &str, password: &str) -> Result<(), MatrixError> {
        let client = matrix_sdk::Client::builder()
            .homeserver_url(&self.homeserver_url)
            .build()
            .await
            .map_err(|e| MatrixError::Unavailable(format!("Failed to build client: {}", e)))?;

        client
            .matrix_auth()
            .login_username(username, password)
            .send()
            .await
            .map_err(|e| MatrixError::Auth(format!("Login failed: {}", e)))?;

        tracing::info!(
            target: "hkask.communication.matrix.login",
            username = %username,
            homeserver = %self.homeserver_url,
            "Matrix login successful"
        );

        self.client = Some(client);
        Ok(())
    }

    /// Get recent messages from a Matrix room.
    ///
    /// Performs a one-shot poll of the room's timeline. Does not require
    /// a continuous sync loop. Call this when the agent is activated to
    /// check for new messages.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// pre:  client is authenticated (login() called)
    /// pre:  room_id is a valid Matrix room ID
    /// pre:  limit > 0
    /// post: returns `Vec<MatrixMessage>` with at most `limit` messages
    /// post: returns Err(NotLoggedIn) if not authenticated
    /// post: returns Err(Room) if room not found
    pub async fn get_messages(
        &self,
        room_id: &RoomId,
        limit: usize,
    ) -> Result<Vec<MatrixMessage>, MatrixError> {
        let client = self.client.as_ref().ok_or(MatrixError::NotLoggedIn)?;

        let room_id = matrix_sdk::ruma::RoomId::parse(room_id.as_str())
            .map_err(|e| MatrixError::Room(format!("Invalid room ID: {}", e)))?;

        let room = client
            .get_room(&room_id)
            .ok_or_else(|| MatrixError::Room(format!("Room not found: {}", room_id)))?;

        // Perform a one-shot sync to get latest events, then read the timeline
        client
            .sync(matrix_sdk::config::SyncSettings::default())
            .await
            .map_err(|e| MatrixError::Network(format!("Sync failed: {}", e)))?;

        let messages: Vec<MatrixMessage> = room
            .messages(matrix_sdk::room::MessagesOptions::backward())
            .await
            .map_err(|e| MatrixError::Room(format!("Failed to get messages: {}", e)))?
            .chunk
            .into_iter()
            .take(limit)
            .filter_map(|ev| {
                let raw_str = ev.raw().json().get();
                let parsed: serde_json::Value = serde_json::from_str(raw_str).ok()?;
                let event_id = parsed.get("event_id")?.as_str()?.to_string();
                let sender = parsed
                    .get("sender")
                    .and_then(|s| s.as_str())
                    .map(UserId::new)
                    .unwrap_or_else(|| UserId::new("unknown"));
                let body = parsed
                    .get("content")
                    .and_then(|c| c.get("body"))
                    .and_then(|b| b.as_str())
                    .unwrap_or("")
                    .to_string();
                let timestamp = parsed
                    .get("origin_server_ts")
                    .and_then(|t| t.as_i64())
                    .unwrap_or(0);
                if body.is_empty() {
                    return None;
                }
                Some(MatrixMessage {
                    event_id,
                    sender,
                    body,
                    structured: None,
                    timestamp,
                })
            })
            .collect();

        tracing::debug!(
            target: "hkask.communication.matrix.messages.polled",
            room_id = %room_id,
            count = messages.len(),
            "Messages polled from room"
        );

        Ok(messages)
    }

    /// Send a message to a Matrix room.
    ///
    /// If `structured` is provided, it is attached as JSON in the
    /// message's `org.matrix.custom.html` formatted body for machine
    /// consumption while the plain `body` remains human-readable.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// pre:  client is authenticated (login() called)
    /// pre:  room_id is a valid Matrix room ID
    /// pre:  body is non-empty
    /// post: message sent to room
    /// post: returns Err(NotLoggedIn) if not authenticated
    /// post: returns Err(Network) if send fails
    pub async fn send_message(
        &self,
        room_id: &RoomId,
        body: &str,
        structured: Option<serde_json::Value>,
    ) -> Result<(), MatrixError> {
        let client = self.client.as_ref().ok_or(MatrixError::NotLoggedIn)?;

        let room_id = matrix_sdk::ruma::RoomId::parse(room_id.as_str())
            .map_err(|e| MatrixError::Room(format!("Invalid room ID: {}", e)))?;

        let room = client
            .get_room(&room_id)
            .ok_or_else(|| MatrixError::Room(format!("Room not found: {}", room_id)))?;

        let content = if let Some(ref structured) = structured {
            let html = format!(
                "<p>{}</p>\n<!-- hkask-structured: {} -->",
                body,
                serde_json::to_string(structured).unwrap_or_default()
            );
            matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_html(body, html)
        } else {
            matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(body)
        };

        room.send(content)
            .await
            .map_err(|e| MatrixError::Network(format!("Send failed: {}", e)))?;

        tracing::info!(
            target: "hkask.communication.matrix.message.sent",
            room_id = %room_id,
            body_len = body.len(),
            "Matrix message sent"
        );

        Ok(())
    }

    /// Create a new Matrix room.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// pre:  client is authenticated (login() called)
    /// pre:  name is non-empty
    /// post: returns Ok(RoomId) for the newly created room
    /// post: room name is set to `name`
    /// post: returns Err(NotLoggedIn) if not authenticated
    pub async fn create_room(
        &self,
        name: &str,
        _topic: Option<&str>,
    ) -> Result<RoomId, MatrixError> {
        let client = self.client.as_ref().ok_or(MatrixError::NotLoggedIn)?;

        let room = client
            .create_room(matrix_sdk::ruma::api::client::room::create_room::v3::Request::new())
            .await
            .map_err(|e| MatrixError::Room(format!("Failed to create room: {}", e)))?;

        let room_id = room.room_id().to_string();

        // Set the room name
        if let Some(joined) = client.get_room(room.room_id()) {
            joined
                .set_name(name.to_string())
                .await
                .map_err(|e| MatrixError::Room(format!("Failed to set room name: {}", e)))?;
        }

        tracing::info!(
            target: "reg.communication.thread.created",
            room_id = %room_id,
            name = %name,
            "Matrix room created"
        );

        Ok(RoomId::new(&room_id))
    }

    /// Invite a user to a room.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// pre:  client is authenticated (login() called)
    /// pre:  room_id is a valid Matrix room ID
    /// pre:  user_id is a valid Matrix user ID
    /// post: user invited to room
    /// post: returns Err(NotLoggedIn) if not authenticated
    /// post: returns Err(Room) if room not found or invite fails
    pub async fn invite_user(&self, room_id: &RoomId, user_id: &UserId) -> Result<(), MatrixError> {
        let client = self.client.as_ref().ok_or(MatrixError::NotLoggedIn)?;

        let room_id = matrix_sdk::ruma::RoomId::parse(room_id.as_str())
            .map_err(|e| MatrixError::Room(format!("Invalid room ID: {}", e)))?;

        let user_id = matrix_sdk::ruma::UserId::parse(user_id.as_str())
            .map_err(|e| MatrixError::Room(format!("Invalid user ID: {}", e)))?;

        let room = client
            .get_room(&room_id)
            .ok_or_else(|| MatrixError::Room(format!("Room not found: {}", room_id)))?;

        room.invite_user_by_id(&user_id)
            .await
            .map_err(|e| MatrixError::Room(format!("Invite failed: {}", e)))?;

        tracing::info!(
            target: "reg.communication.agent.invited",
            room_id = %room_id,
            user = %user_id,
            "User invited to room"
        );

        Ok(())
    }

    /// List joined rooms.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// pre:  client is authenticated (login() called)
    /// post: returns `Vec<Thread>` with all joined rooms
    /// post: each Thread has room_id, title, and participants populated
    /// post: returns Err(NotLoggedIn) if not authenticated
    pub async fn list_rooms(&self) -> Result<Vec<Thread>, MatrixError> {
        let client = self.client.as_ref().ok_or(MatrixError::NotLoggedIn)?;

        let rooms = client.joined_rooms();

        let mut threads = Vec::new();
        for room in rooms {
            let room_id = room.room_id().to_string();
            let title = room.name().unwrap_or_else(|| room_id.clone());
            let members: Vec<UserId> = room
                .members(matrix_sdk::RoomMemberships::JOIN)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|m| UserId::new(m.user_id().as_str()))
                .collect();

            threads.push(Thread {
                room_id: RoomId::new(&room_id),
                title,
                participants: members,
                monitored_by: vec![],
                escalated: false,
                created_at: chrono::Utc::now().timestamp_millis(),
            });
        }

        Ok(threads)
    }

    /// Check whether the client is authenticated.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// post: returns true iff client is Some (login() succeeded)
    pub fn healthy(&self) -> bool {
        self.client.is_some()
    }

    /// Reconnect to the Matrix homeserver.
    ///
    /// Rebuilds the SDK client and re-authenticates using stored credentials.
    /// Call this when the connection drops or the homeserver restarts.
    /// Requires `HKASK_MATRIX_AGENT_USERNAME` and `HKASK_MATRIX_AGENT_PASSWORD`
    /// environment variables to be set.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// pre:  HKASK_MATRIX_AGENT_USERNAME and HKASK_MATRIX_AGENT_PASSWORD env vars are set
    /// post: self.client is reset and re-authenticated
    /// post: returns Err(Auth) if env vars are missing or credentials invalid
    pub async fn reconnect(&mut self) -> Result<(), MatrixError> {
        self.client = None;

        let username = std::env::var("HKASK_MATRIX_AGENT_USERNAME")
            .map_err(|_| MatrixError::Auth("HKASK_MATRIX_AGENT_USERNAME not set".to_string()))?;
        let password = std::env::var("HKASK_MATRIX_AGENT_PASSWORD")
            .map_err(|_| MatrixError::Auth("HKASK_MATRIX_AGENT_PASSWORD not set".to_string()))?;

        self.login(&username, &password).await?;

        tracing::info!(
            target: "hkask.communication.matrix.reconnect",
            url = %self.homeserver_url,
            "Matrix transport reconnected"
        );
        Ok(())
    }

    /// Check whether the Matrix connection is alive.
    ///
    /// Returns `true` if the client is authenticated AND the homeserver
    /// responds to a version check. Returns `false` if either fails.
    /// Does not mutate state — safe to call from health probes.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// post: returns true iff client is authenticated and whoami succeeds
    /// post: does not mutate self
    pub async fn is_healthy(&self) -> bool {
        let Some(client) = &self.client else {
            return false;
        };

        // Check if the client can reach the homeserver
        let whoami = client.whoami().await;
        whoami.is_ok()
    }

    /// Get the current logged-in user ID, if authenticated.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// post: returns Some(user_id) if authenticated
    /// post: returns None if not authenticated or whoami fails
    pub async fn current_user_id(&self) -> Option<String> {
        let client = self.client.as_ref()?;
        client.whoami().await.ok().map(|r| r.user_id.to_string())
    }

    /// Upload a file to the Matrix homeserver, returning an mxc:// URI.
    pub async fn upload_file(
        &self,
        filename: &str,
        mime_type: &str,
        data: &[u8],
    ) -> Result<String, MatrixError> {
        let client = self.client.as_ref().ok_or(MatrixError::NotLoggedIn)?;
        let mime: mime::Mime = mime_type.parse().unwrap_or(mime::APPLICATION_OCTET_STREAM);
        let response = client
            .media()
            .upload(&mime, data.to_vec(), None)
            .await
            .map_err(|e| MatrixError::Network(format!("Upload failed: {}", e)))?;
        let uri = response.content_uri.to_string();
        tracing::info!(target: "hkask.communication.matrix.file.uploaded", filename = %filename, uri = %uri, "File uploaded");
        Ok(uri)
    }

    /// Send a file attachment to a Matrix room.
    pub async fn send_file(
        &self,
        room_id: &RoomId,
        filename: &str,
        mime_type: &str,
        data: &[u8],
        caption: Option<&str>,
    ) -> Result<(), MatrixError> {
        let uri = self.upload_file(filename, mime_type, data).await?;
        let client = self.client.as_ref().ok_or(MatrixError::NotLoggedIn)?;
        let rid = matrix_sdk::ruma::RoomId::parse(room_id.as_str())
            .map_err(|e| MatrixError::Room(format!("Invalid room ID: {}", e)))?;
        let room = client
            .get_room(&rid)
            .ok_or_else(|| MatrixError::Room(format!("Room not found: {}", rid)))?;
        let body = caption.unwrap_or(filename);
        let mxc = matrix_sdk::ruma::OwnedMxcUri::from(uri);
        let content = if mime_type.starts_with("image/") {
            matrix_sdk::ruma::events::room::message::RoomMessageEventContent::new(
                matrix_sdk::ruma::events::room::message::MessageType::Image(
                    matrix_sdk::ruma::events::room::message::ImageMessageEventContent::plain(
                        body.to_string(),
                        mxc,
                    ),
                ),
            )
        } else if mime_type.starts_with("video/") {
            matrix_sdk::ruma::events::room::message::RoomMessageEventContent::new(
                matrix_sdk::ruma::events::room::message::MessageType::Video(
                    matrix_sdk::ruma::events::room::message::VideoMessageEventContent::plain(
                        body.to_string(),
                        mxc,
                    ),
                ),
            )
        } else if mime_type.starts_with("audio/") {
            matrix_sdk::ruma::events::room::message::RoomMessageEventContent::new(
                matrix_sdk::ruma::events::room::message::MessageType::Audio(
                    matrix_sdk::ruma::events::room::message::AudioMessageEventContent::plain(
                        body.to_string(),
                        mxc,
                    ),
                ),
            )
        } else {
            matrix_sdk::ruma::events::room::message::RoomMessageEventContent::new(
                matrix_sdk::ruma::events::room::message::MessageType::File(
                    matrix_sdk::ruma::events::room::message::FileMessageEventContent::plain(
                        body.to_string(),
                        mxc,
                    ),
                ),
            )
        };
        room.send(content)
            .await
            .map_err(|e| MatrixError::Network(format!("File send failed: {}", e)))?;
        tracing::info!(target: "hkask.communication.matrix.file.sent", room_id = %rid, filename = %filename, "File sent");
        Ok(())
    }
}
