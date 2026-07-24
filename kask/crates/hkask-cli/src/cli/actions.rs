//! CLI action enums — subcommand types for each top-level command.
//!
//! Phase 1 trim: runtime/side-door commands removed. CLI is now admin, config,
//! startup, shutdown, and the single `tui` runtime launch. Runtime operations
//! (skills, bundles, templates, kata, kanban, goals, adapters, Regulation queries,
//! curator escalations, consolidation, style, web search) live in the TUI's
//! REPL slash commands or are invoked via MCP tools from within the runtime.

use clap::Subcommand;
use std::path::PathBuf;

/// Pod admin actions — only deployment artifact generation. Lifecycle ops
/// (create/activate/deactivate/assign/mode) are runtime operations available
/// from the TUI REPL or the HTTP API.
#[derive(Debug, Subcommand)]
pub enum PodAction {
    /// Generate a Containerfile + pod files for Docker builds
    ExportContainer {
        #[arg()]
        pod_id: String,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Copy canonical K8s manifests from `deploy/k8s/` into an output directory
    ExportK8s {
        #[arg()]
        pod_id: String,
        #[arg(long, default_value = "10")]
        volume_size_gb: u32,
        #[arg(long, default_value = "1")]
        max_replicas: u32,
        #[arg(short, long)]
        output: PathBuf,
    },
}

/// MCP server inventory — read-only. Tool invocation is runtime-only.
/// Use the TUI REPL's `/invoke` slash command or the agent's autonomous
/// tool dispatch.
#[derive(Debug, Subcommand)]
pub enum McpAction {
    /// List registered MCP servers
    ListServers,
    /// List all tools across all servers
    ListTools,
    /// Get a single tool's definition
    GetTool {
        #[arg()]
        name: String,
    },
}

/// Sovereignty admin — structural verification only. Live consent grants/revokes
/// and status checks are runtime operations available from the TUI REPL or API.
#[derive(Debug, Subcommand)]
pub enum SovereigntyAction {
    /// Run a Magna Carta structural audit against the codebase
    Verify {
        #[arg(long)]
        principle: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

/// Git archival and CAS actions
#[derive(Debug, Subcommand)]
pub enum GitAction {
    Archive {
        #[arg(short, long)]
        owner: String,
        #[arg(short, long)]
        repo: String,
        #[arg(short, long, default_value = "main")]
        branch: String,
        #[arg(short, long, default_value = "registry")]
        path: String,
        #[arg(short, long)]
        content: Option<String>,
        #[arg(short, long)]
        file: Option<PathBuf>,
    },
    Restore {
        #[arg(short, long)]
        owner: String,
        #[arg(short, long)]
        repo: String,
        #[arg(short, long)]
        r#ref: String,
        #[arg(short, long, default_value = ".")]
        target: String,
    },
    List {
        #[arg(short, long)]
        owner: String,
        #[arg(short, long)]
        repo: String,
    },
    Snapshot {
        #[arg(short, long)]
        owner: String,
        #[arg(short, long)]
        repo: String,
        #[arg(short, long)]
        message: String,
    },
    CasVerify {
        #[arg(short, long, default_value = "registry")]
        repo: String,
    },
    CasDiff {
        #[arg(short, long, default_value = "registry")]
        repo: String,
        #[arg(short, long)]
        from: String,
        #[arg(short, long)]
        to: String,
    },
    CasLog {
        #[arg(short, long, default_value = "registry")]
        repo: String,
        #[arg(short, long, default_value = "20")]
        max_count: usize,
    },
    CasSnapshot {
        #[arg(short, long, default_value = "registry")]
        repo: String,
        #[arg(short, long)]
        message: String,
    },
    CasRestore {
        #[arg(short, long, default_value = "registry")]
        repo: String,
        #[arg(short, long)]
        r#ref: Option<String>,
        #[arg(short, long)]
        prefix: Option<String>,
    },
}

/// Backup actions — snapshot, restore, list, prune, verify, config
#[derive(Debug, Subcommand)]
pub enum BackupAction {
    /// Create a backup snapshot of all pod directories
    Snapshot {
        /// Scope: "full", or artifact type label (e.g., "template", "goal")
        #[arg(short, long, default_value = "full")]
        scope: String,
    },
    /// Restore a pod from a backup snapshot
    Restore {
        /// Pod name to restore
        #[arg()]
        pod: String,
        /// Date to restore from (YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS)
        #[arg(short, long)]
        date: Option<String>,
        /// Commit hash to restore from (alternative to --date)
        #[arg(short, long)]
        commit: Option<String>,
    },
    /// List backup snapshots for all pods
    List {
        /// Maximum snapshots per pod
        #[arg(short, long, default_value = "5")]
        limit: usize,
    },
    /// Verify backup integrity (old CAS repos)
    Verify,
    /// Show per-pod snapshot status
    Status,
}

/// OCAP token issuance — admin credential provisioning for MCP gateways.
#[derive(Debug, Subcommand)]
pub enum TokenAction {
    /// Issue a new DelegationToken for a userpod
    Issue {
        #[arg(long)]
        userpod: String,
        #[arg(long, num_args = 1..)]
        capabilities: Vec<String>,
        #[arg(long, default_value = "24h")]
        ttl: String,
    },
    /// List issued tokens (by userpod filter)
    List {
        #[arg(long)]
        userpod: Option<String>,
    },
    /// Revoke a token by ID
    Revoke {
        #[arg()]
        token_id: String,
    },
}

/// UserPod identity actions
#[derive(Debug, Subcommand)]
pub enum UserPodAction {
    Register {
        #[arg()]
        userpod_name: String,
        #[arg(long)]
        first_name: String,
        #[arg(long)]
        last_name: String,
        #[arg(long)]
        email: String,
        #[arg(long)]
        phone: Option<String>,
    },
    Login {
        #[arg()]
        userpod_name: String,
    },

    /// Change a userpod's passphrase
    Passphrase {
        #[arg()]
        userpod_name: String,
    },
    Logout {
        #[arg()]
        session_id: String,
    },
    Sessions {
        #[arg()]
        userpod_name: String,
    },
    List {
        #[arg(long)]
        user_id: Option<String>,
    },
    Show {
        #[arg()]
        userpod_name: String,
    },
    /// Rename a userpod
    Rename {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
    },
    /// Delete a userpod and all its data
    Delete {
        #[arg()]
        name: String,
    },
    /// Create an invite code for a new member (admin only)
    Invite {
        /// Admin userpod name issuing the invite
        #[arg(long)]
        by: String,
        /// Single invitee: `Name <email>` format
        #[arg(long)]
        invitee: Option<String>,
        /// Batch invitees: comma-separated `Name <email>` pairs
        /// Example: `Alice Smith <alice@x.com>, Bob Jones <bob@y.com>`
        #[arg(long, value_delimiter = ',')]
        invitees: Vec<String>,
        /// Send invites via email (requires MXroute config)
        #[arg(long)]
        send: bool,
    },
    /// Revoke a pending invite code (admin only)
    RevokeInvite {
        /// Invite code to revoke
        #[arg()]
        code: String,
        /// Admin userpod name
        #[arg(long)]
        by: String,
    },
}

/// Sovereignty export actions
#[derive(Debug, Subcommand)]
pub enum ExportAction {
    /// Create an encrypted sovereignty backup archive
    Create {
        #[arg(short, long)]
        passphrase: String,
    },
    /// Upload a sovereignty archive to a server (migration)
    Upload {
        /// Path to the archive file
        #[arg(short, long)]
        archive: PathBuf,
        /// Passphrase to decrypt the archive
        #[arg(short, long)]
        passphrase: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum KeystoreAction {
    Load {
        #[arg(short, long, default_value = "key_load_template.env")]
        path: PathBuf,
        #[arg(short = 'x', long, default_value = "HKASK_")]
        prefix: String,
        #[arg(long)]
        overwrite: bool,
        /// Securely delete the file after loading keys into the keychain.
        /// Requires explicit confirmation — the system will warn that the
        /// file will be permanently destroyed and ask for consent.
        #[arg(long)]
        shred: bool,
    },
    List,
    Get {
        #[arg()]
        key: String,
    },
    Set {
        #[arg()]
        key: String,
        #[arg()]
        value: String,
    },
    Delete {
        #[arg()]
        key: String,
    },
    /// Rotate the master key version — increments the key version,
    /// derives new secrets, and stores them in the keychain.
    /// Old-version secrets remain derivable for data access.
    /// Requires the current master passphrase.
    Rotate {
        /// New master passphrase (if changing). If not provided,
        /// the current passphrase is used with the incremented version.
        #[arg(short, long)]
        passphrase: Option<String>,
    },
}

/// REPL settings actions — get, set, list, reset inference parameters.
/// Same settings as the `/repl` slash command in interactive mode.
#[derive(Debug, Subcommand)]
pub enum SettingsAction {
    /// Show all settings (or a single setting if name is given)
    Show {
        #[arg()]
        name: Option<String>,
    },
    /// Set a setting value
    Set { name: String, value: String },
    /// Reset all settings to defaults
    Reset,
}

#[derive(Debug, Subcommand)]
pub enum WalletAction {
    /// Show rJoule balance with USDC and gas equivalents
    Balance {
        /// Wallet ID (UUID). Defaults to system wallet if omitted.
        #[arg(short, long)]
        wallet: Option<String>,
    },
    /// Show or derive a deposit address for receiving USDC
    DepositAddress {
        /// Blockchain network (hinkal or hedera). Defaults to hinkal.
        #[arg(short, long)]
        chain: Option<String>,
        /// Use shielded/privacy mode (default behavior).
        #[arg(short, long, conflicts_with = "transparent")]
        private: bool,
        /// Opt out to transparent mode (public on-chain visibility).
        #[arg(long, conflicts_with = "private")]
        transparent: bool,
        /// Wallet ID (UUID). Defaults to system wallet if omitted.
        #[arg(short, long)]
        wallet: Option<String>,
    },
    /// Generate a one-time deposit reference for shielded deposits
    DepositReference {
        /// Blockchain network (hinkal or hedera)
        #[arg(short, long)]
        chain: String,
        /// Wallet ID (UUID). Defaults to system wallet if omitted.
        #[arg(short, long)]
        wallet: Option<String>,
    },
    /// Show paginated transaction history
    History {
        /// Maximum transactions to show
        #[arg(short, long)]
        limit: Option<u32>,
        /// Wallet ID (UUID). Defaults to system wallet if omitted.
        #[arg(short, long)]
        wallet: Option<String>,
    },
    /// API key management
    Key {
        #[command(subcommand)]
        action: KeyAction,
    },
    /// Estimate current network withdrawal fee for a chain
    Fee {
        /// Blockchain network (hinkal or hedera). Defaults to hinkal.
        #[arg(short, long)]
        chain: Option<String>,
    },
    /// Withdraw rJoules as USDC to an external address
    Withdraw {
        /// Amount in rJoules to withdraw
        amount_rj: u64,
        /// Destination address (Hedera 0.0.XXXXX)
        #[arg(short, long)]
        to: String,
        /// Blockchain network (hinkal or hedera). Defaults to hinkal.
        #[arg(short, long)]
        chain: Option<String>,
        /// Use shielded/privacy mode (default behavior).
        #[arg(short, long, conflicts_with = "transparent")]
        private: bool,
        /// Opt out to transparent mode (public on-chain visibility).
        #[arg(long, conflicts_with = "private")]
        transparent: bool,
        /// Wallet ID (UUID). Defaults to system wallet if omitted.
        #[arg(short, long)]
        wallet: Option<String>,
    },
    /// Allocate rJoules to an API key for spending
    Encumber {
        /// API key ID to allocate rJoules to
        #[arg(short, long)]
        key_id: String,
        /// Amount in rJoules to allocate
        #[arg(short, long)]
        amount: u64,
        /// Wallet ID (UUID). Defaults to system wallet if omitted.
        #[arg(short, long)]
        wallet: Option<String>,
    },
    /// Release an API key's encumbrance (returns unspent rJoules to wallet)
    ReleaseEncumbrance {
        /// API key ID to release
        #[arg(short, long)]
        key_id: String,
    },
    /// Show spending report for an API key
    Report {
        /// API key ID to report on
        #[arg(short, long)]
        key_id: String,
        /// Wallet ID (UUID). Defaults to system wallet if omitted.
        #[arg(short, long)]
        wallet: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum KeyAction {
    /// Create a new API key with spending limit
    Create {
        /// Spending limit in rJoules
        #[arg(short, long)]
        limit: u64,
        /// Expiry in days (no expiry if omitted)
        #[arg(short, long)]
        expiry: Option<u32>,
        /// Restrict to shielded/privacy mode (default behavior).
        #[arg(short, long, conflicts_with = "transparent")]
        private: bool,
        /// Opt out to transparent mode (public on-chain visibility).
        #[arg(long, conflicts_with = "private")]
        transparent: bool,
        /// Restrict to a specific chain (defaults to hinkal)
        #[arg(short, long)]
        chain: Option<String>,
        /// Wallet ID (UUID). Defaults to system wallet if omitted.
        #[arg(short, long)]
        wallet: Option<String>,
    },
    /// List active API keys
    List {
        /// Wallet ID (UUID). Defaults to system wallet if omitted.
        #[arg(short, long)]
        wallet: Option<String>,
    },
    /// Revoke an API key (returns unspent rJoules to wallet)
    Revoke {
        /// Key ID to revoke
        key_id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum MatrixAction {
    /// Generate Docker sidecar files (docker-compose.yml, Caddyfile, conduit.toml)
    DeploySidecar {
        /// Domain name for the Matrix homeserver (e.g., matrix.example.com)
        #[arg(short, long)]
        domain: String,
        /// Also generate Hydrogen web client config
        #[arg(long)]
        with_web_client: bool,
        /// Output directory (default: ~/.config/hkask/sidecar)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Register a hKask agent on the Matrix homeserver
    RegisterAgent {
        /// UserPod name (e.g., "Alice-Smith")
        agent: String,
        /// Homeserver URL (default: http://localhost:8008)
        #[arg(short, long, default_value = "http://localhost:8008")]
        homeserver: String,
    },
    /// Create a human Matrix user account on the homeserver
    RegisterUser {
        /// Desired username (e.g., "Bob-Jones")
        user: String,
        /// Homeserver URL (default: http://localhost:8008)
        #[arg(short, long, default_value = "http://localhost:8008")]
        homeserver: String,
    },
    /// Check sidecar health (Docker containers, API, database)
    StatusSidecar,
}

#[derive(Debug, Subcommand)]
pub enum DaemonAction {
    /// Start the daemon (binds Unix socket, runs Regulation loops, serves until shutdown)
    Start,

    /// Check daemon status (socket existence + health)
    Status,

    /// Stop the daemon (sends shutdown signal via socket)
    Stop,
}

/// Remote cluster deployment — K3s/Hetzner bootstrap.
/// Extracted from the former `curator init` command (which was misnamed —
/// it deploys the cluster, not the Curator daemon).
#[derive(Debug, Subcommand)]
pub enum DeployAction {
    /// Initialize the hKask system on a Hetzner K3s cluster:
    /// validates env, deploys shared Conduit, creates org tokens, deploys hKask pod.
    Init {
        /// Domain for the hKask installation (e.g., hkask.example.com)
        #[arg(long, default_value = "localhost")]
        domain: String,
    },
}

/// Skill corpus audit — health-score verification. Discovery, status,
/// publishing, and derivation remain REPL-only (`/skill`) since they
/// operate on the live registry; the CLI exposes only the structural
/// audit, which CI runs as a gate.
#[derive(Debug, Subcommand)]
pub enum SkillAction {
    /// Audit all skills; exit non-zero if any skill's health score is below `--fail-below`.
    Audit {
        /// Minimum health score required to exit 0 (default: 0.8).
        #[arg(long, default_value = "0.8")]
        fail_below: f64,
        /// Emit the full report as JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
    },
}
