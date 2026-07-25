//! Server configuration — loaded from ~/.config/hkask/config.json.
//!
//! Defines the server's registration mode (open/closed), domain, and
//! other administrative settings. Shared between CLI (init) and API (admin config).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Server registration mode — controls whether new users can self-register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerRegistration {
    /// Anyone with an OAuth account can register and create agents.
    Open,
    /// Only users with an admin-issued invite code can register.
    Closed,
}

impl std::fmt::Display for ServerRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::Closed => write!(f, "closed"),
        }
    }
}

impl std::str::FromStr for ServerRegistration {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "open" => Ok(Self::Open),
            "closed" => Ok(Self::Closed),
            other => Err(format!(
                "invalid registration mode '{other}': expected 'open' or 'closed'"
            )),
        }
    }
}

/// Server configuration persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub version: String,
    pub profile: String,
    pub registration: ServerRegistration,
    pub data_dir: String,
    pub domain: String,
    #[serde(default)]
    pub conduit_room_id: Option<String>,
}

impl ServerConfig {
    /// Default config path: ~/.config/hkask/config.json
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("hkask")
            .join("config.json")
    }

    /// Load server config from the default path.
    ///
    /// Blocking — reads from disk synchronously. For async contexts, callers
    /// should wrap in `tokio::task::spawn_blocking`.
    ///
    /// expect: "As an admin I can view the server configuration"
    /// pre:  config.json exists at default_path()
    /// post: returns ServerConfig or an error
    pub fn load() -> Result<Self, ServerConfigError> {
        let path = Self::default_path();
        let bytes = std::fs::read_to_string(&path).map_err(|e| ServerConfigError::Io {
            path: path.display().to_string(),
            error: e.to_string(),
        })?;
        serde_json::from_str(&bytes).map_err(ServerConfigError::Parse)
    }

    /// Load config, returning a safe default if the file does not exist.
    ///
    /// Distinguishes "config not found" (dev/first-run — use default) from
    /// "config corrupted" (production — return error for fail-closed safety).
    ///
    /// expect: "The server gracefully handles missing config during development"
    /// pre:  none
    /// post: returns Ok(default) if file missing; Ok(parsed) if valid; Err if corrupted
    pub fn load_or_default() -> Result<Self, ServerConfigError> {
        match Self::load() {
            Ok(config) => Ok(config),
            Err(ServerConfigError::Io { .. }) => {
                // File not found — first run or dev mode. Default to closed for safety.
                Ok(Self {
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    profile: "server".to_string(),
                    registration: ServerRegistration::Closed,
                    data_dir: "/var/lib/hkask".to_string(),
                    domain: "localhost".to_string(),
                    conduit_room_id: None,
                })
            }
            Err(e) => Err(e), // Parse error — config exists but is corrupted
        }
    }

    /// Save server config to the default path.
    ///
    /// Blocking — writes to disk synchronously. For async contexts, callers
    /// should wrap in `tokio::task::spawn_blocking`.
    ///
    /// Note: No file locking. Concurrent writes (e.g., admin PATCH while
    /// OAuth callback stores conduit_room_id) may race. The room creation
    /// handler mitigates this by reloading config after write.
    ///
    /// expect: "As an admin I can modify the server configuration"
    /// pre:  config directory exists
    /// post: config.json written with updated values
    pub fn save(&self) -> Result<(), ServerConfigError> {
        let path = Self::default_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ServerConfigError::Io {
                path: parent.display().to_string(),
                error: e.to_string(),
            })?;
        }
        let json = serde_json::to_string_pretty(self).map_err(ServerConfigError::Serialize)?;
        std::fs::write(&path, json).map_err(|e| ServerConfigError::Io {
            path: path.display().to_string(),
            error: e.to_string(),
        })?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ServerConfigError {
    #[error("Failed to read {path}: {error}")]
    Io { path: String, error: String },
    #[error("Failed to parse config.json: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("Failed to serialize config: {0}")]
    Serialize(serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_or_default_returns_default_when_file_missing() {
        // Can't override default_path, but we can test the load logic directly
        let result = ServerConfig::load_or_default();
        // In test environment, config likely doesn't exist — should get default
        assert!(result.is_ok(), "load_or_default should not panic");
        let config = result.unwrap();
        assert_eq!(config.registration, ServerRegistration::Closed);
    }

    #[test]
    fn registration_display_and_parse_roundtrip() {
        assert_eq!(ServerRegistration::Open.to_string(), "open");
        assert_eq!(ServerRegistration::Closed.to_string(), "closed");
        assert_eq!(
            "open".parse::<ServerRegistration>().unwrap(),
            ServerRegistration::Open
        );
        assert_eq!(
            "closed".parse::<ServerRegistration>().unwrap(),
            ServerRegistration::Closed
        );
        assert!("invalid".parse::<ServerRegistration>().is_err());
    }

    #[test]
    fn serverconfig_serialization_roundtrip() {
        let config = ServerConfig {
            version: "0.31.0".into(),
            profile: "server".into(),
            registration: ServerRegistration::Open,
            data_dir: "/tmp/hkask".into(),
            domain: "hkask.example.com".into(),
            conduit_room_id: Some("!abc123:localhost".into()),
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: ServerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.registration, ServerRegistration::Open);
        assert_eq!(parsed.domain, "hkask.example.com");
        assert_eq!(parsed.conduit_room_id, Some("!abc123:localhost".into()));
    }

    #[test]
    fn serverconfig_deserializes_without_conduit_room_id() {
        let json = r#"{
            "version": "0.31.0",
            "profile": "server",
            "registration": "closed",
            "data_dir": "/var/lib/hkask",
            "domain": "localhost"
        }"#;
        let config: ServerConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.registration, ServerRegistration::Closed);
        assert_eq!(config.conduit_room_id, None);
    }
}
