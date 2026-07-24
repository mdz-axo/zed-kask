-- Human users (contact info for recovery only)
CREATE TABLE IF NOT EXISTS human_users (
    user_id TEXT PRIMARY KEY,

    -- Human's contact info (encrypted)
    email_enc BLOB NOT NULL,
    phone_enc BLOB,

    -- Auth credentials
    passphrase_hash TEXT NOT NULL,
    salt TEXT NOT NULL,
    master_salt TEXT NOT NULL,

    -- Metadata
    created_at INTEGER NOT NULL,
    last_active INTEGER,
    passphrase_set_at INTEGER,
    -- Multi-user role (defaults to Member)
    role TEXT NOT NULL DEFAULT 'member',
    -- OAuth identity provider
    oauth_provider TEXT,
    oauth_provider_user_id TEXT,
    oauth_display_name TEXT
);

-- UserPod identities (user logs in AS a userpod)
-- Note: email/phone stored ONLY in human_users to avoid duplication
CREATE TABLE IF NOT EXISTS userpod_identities (
    userpod_name TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    webid TEXT UNIQUE NOT NULL,
    wallet_id TEXT,
    first_name_enc BLOB NOT NULL,
    last_name_enc BLOB NOT NULL,
    capabilities TEXT NOT NULL DEFAULT '[]',
    created_at INTEGER NOT NULL,
    last_login INTEGER,
    FOREIGN KEY (user_id) REFERENCES human_users(user_id)
);

-- Active sessions
CREATE TABLE IF NOT EXISTS user_sessions (
    session_id TEXT PRIMARY KEY,
    userpod_name TEXT NOT NULL,
    webid TEXT NOT NULL,
    user_id TEXT NOT NULL,
    session_key_salt TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    last_active INTEGER NOT NULL,
    FOREIGN KEY (userpod_name) REFERENCES userpod_identities(userpod_name),
    FOREIGN KEY (user_id) REFERENCES human_users(user_id)
);

-- Indexes for performance
CREATE INDEX IF NOT EXISTS idx_userpod_identities_user ON userpod_identities(user_id);
CREATE INDEX IF NOT EXISTS idx_userpod_identities_webid ON userpod_identities(webid);
CREATE INDEX IF NOT EXISTS idx_user_sessions_user ON user_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_user_sessions_userpod ON user_sessions(userpod_name);
CREATE INDEX IF NOT EXISTS idx_user_sessions_expiry ON user_sessions(expires_at);

-- Multi-user invitations
CREATE TABLE IF NOT EXISTS invites (
    invite_id TEXT PRIMARY KEY,
    created_by TEXT NOT NULL,
    code TEXT UNIQUE NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    accepted_at INTEGER,
    accepted_user_id TEXT,
    FOREIGN KEY (created_by) REFERENCES human_users(user_id),
    FOREIGN KEY (accepted_user_id) REFERENCES human_users(user_id)
);

CREATE INDEX IF NOT EXISTS idx_invites_code ON invites(code);
CREATE INDEX IF NOT EXISTS idx_invites_created_by ON invites(created_by);
