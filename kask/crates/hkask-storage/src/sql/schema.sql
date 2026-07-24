CREATE TABLE IF NOT EXISTS hmems (id TEXT PRIMARY KEY, entity TEXT NOT NULL, attribute TEXT NOT NULL, value TEXT NOT NULL, valid_from TEXT NOT NULL, valid_to TEXT, recalled_at TEXT NOT NULL DEFAULT (datetime('now')), transaction_at TEXT DEFAULT (datetime('now')), confidence REAL NOT NULL DEFAULT 1.0, perspective TEXT, visibility TEXT NOT NULL DEFAULT 'private', owner_webid TEXT NOT NULL, dimension TEXT);
CREATE TABLE IF NOT EXISTS embeddings (id TEXT PRIMARY KEY, entity_ref TEXT NOT NULL, vector BLOB NOT NULL, dimensions INTEGER NOT NULL, model TEXT NOT NULL, created_at TEXT DEFAULT (datetime('now')));
CREATE INDEX IF NOT EXISTS idx_embeddings_entity_ref ON embeddings(entity_ref);
CREATE VIRTUAL TABLE IF NOT EXISTS vec_embeddings USING vec0(embedding float[$DIM] distance_metric=cosine);
CREATE TABLE IF NOT EXISTS nu_events (id TEXT PRIMARY KEY, timestamp TEXT NOT NULL, observer_webid TEXT NOT NULL, span_category TEXT NOT NULL, span_path TEXT NOT NULL, phase TEXT NOT NULL, observation TEXT NOT NULL, regulation TEXT, outcome TEXT, recursion_depth INTEGER NOT NULL, parent_event TEXT, visibility TEXT NOT NULL DEFAULT 'private');
CREATE INDEX IF NOT EXISTS idx_nu_events_timestamp_category ON nu_events(timestamp, span_category);
CREATE INDEX IF NOT EXISTS idx_nu_events_category_phase ON nu_events(span_category, phase);
CREATE TABLE IF NOT EXISTS audit_log (id TEXT PRIMARY KEY, timestamp TEXT NOT NULL, actor_webid TEXT NOT NULL, action TEXT NOT NULL, resource TEXT NOT NULL, outcome TEXT NOT NULL, details TEXT, ip_address TEXT, created_at TEXT DEFAULT (datetime('now')));
CREATE INDEX IF NOT EXISTS idx_audit_log_timestamp ON audit_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_log_actor ON audit_log(actor_webid);
CREATE TABLE IF NOT EXISTS reg_variety_checkpoint (domain TEXT PRIMARY KEY, variety_count INTEGER NOT NULL, last_updated TEXT NOT NULL, threshold INTEGER NOT NULL DEFAULT 10);
CREATE TABLE IF NOT EXISTS reg_alerts (id TEXT PRIMARY KEY, timestamp TEXT NOT NULL, alert_type TEXT NOT NULL, severity TEXT NOT NULL, domain TEXT, message TEXT NOT NULL, resolved INTEGER NOT NULL DEFAULT 0, resolved_at TEXT);
CREATE TABLE IF NOT EXISTS agent_registry (name TEXT PRIMARY KEY, agent_kind TEXT, definition_json TEXT NOT NULL, token_hash TEXT NOT NULL, registered_at TEXT NOT NULL, source_yaml TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_agent_registry_kind ON agent_registry(agent_kind);
CREATE TABLE IF NOT EXISTS goals (id TEXT PRIMARY KEY, webid TEXT NOT NULL, text TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'pending', visibility TEXT NOT NULL DEFAULT 'private', created_at TEXT DEFAULT (datetime('now')), completed_at TEXT, parent_goal_id TEXT, depth INTEGER NOT NULL DEFAULT 0, display_name TEXT);
CREATE TABLE IF NOT EXISTS goal_criteria (id TEXT PRIMARY KEY, goal_id TEXT REFERENCES goals(id), type TEXT NOT NULL, description TEXT NOT NULL, satisfied INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS goal_artifacts (id TEXT PRIMARY KEY, goal_id TEXT REFERENCES goals(id), artifact_ref TEXT NOT NULL, artifact_type TEXT NOT NULL, created_at TEXT DEFAULT (datetime('now')));
CREATE TABLE IF NOT EXISTS consent_records (id TEXT PRIMARY KEY, webid TEXT NOT NULL UNIQUE, granted_categories TEXT NOT NULL, granted_at INTEGER NOT NULL, revoked_at INTEGER, active INTEGER NOT NULL DEFAULT 1);
CREATE INDEX IF NOT EXISTS idx_consent_active ON consent_records(active);
CREATE TABLE IF NOT EXISTS quarantined_goals (id TEXT PRIMARY KEY, original_data TEXT NOT NULL DEFAULT '', quarantine_reason TEXT NOT NULL, quarantined_at TEXT NOT NULL, repair_attempts INTEGER NOT NULL DEFAULT 0, repaired INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS loop_cursors (key TEXT PRIMARY KEY, value INTEGER NOT NULL, updated_at TEXT NOT NULL);
-- Wallet tables — rJoule payments, multi-chain deposits, API key lifecycle
CREATE TABLE IF NOT EXISTS wallet_balances (wallet_id TEXT PRIMARY KEY, balance_rj INTEGER NOT NULL DEFAULT 0, usdc_equivalent_micro INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now')));
CREATE TABLE IF NOT EXISTS wallet_transactions (id INTEGER PRIMARY KEY AUTOINCREMENT, wallet_id TEXT NOT NULL REFERENCES wallet_balances(wallet_id), tx_type TEXT NOT NULL, tx_subtype TEXT, chain TEXT, on_chain_tx_hash TEXT, amount_rj INTEGER NOT NULL, balance_after_rj INTEGER NOT NULL, key_id TEXT, tool_name TEXT, gas_units INTEGER, created_at TEXT NOT NULL DEFAULT (datetime('now')));
CREATE INDEX IF NOT EXISTS idx_wallet_tx_wallet_id ON wallet_transactions(wallet_id);
CREATE INDEX IF NOT EXISTS idx_wallet_tx_created_at ON wallet_transactions(created_at);
CREATE TABLE IF NOT EXISTS api_keys (key_id TEXT PRIMARY KEY, wallet_id TEXT NOT NULL REFERENCES wallet_balances(wallet_id), public_key BLOB NOT NULL, spending_limit_rj INTEGER NOT NULL, spent_rj INTEGER NOT NULL DEFAULT 0, scope TEXT NOT NULL DEFAULT '[]', purpose TEXT NOT NULL DEFAULT '', rate_limit_json TEXT, privacy_mode TEXT NOT NULL DEFAULT 'transparent', preferred_chain TEXT, expires_at TEXT, issued_at TEXT NOT NULL, revoked_at TEXT, created_at TEXT NOT NULL DEFAULT (datetime('now')));
CREATE INDEX IF NOT EXISTS idx_api_keys_wallet_id ON api_keys(wallet_id);
CREATE INDEX IF NOT EXISTS idx_api_keys_public_key ON api_keys(public_key);
CREATE TABLE IF NOT EXISTS deposit_addresses (wallet_id TEXT NOT NULL, chain TEXT NOT NULL, address TEXT NOT NULL, derivation_index INTEGER NOT NULL, privacy_mode TEXT NOT NULL DEFAULT 'transparent', created_at TEXT NOT NULL DEFAULT (datetime('now')), PRIMARY KEY (wallet_id, chain, derivation_index));
CREATE UNIQUE INDEX IF NOT EXISTS deposit_addresses_unique_address ON deposit_addresses(chain, privacy_mode, address);
CREATE TABLE IF NOT EXISTS deposit_references (reference TEXT PRIMARY KEY, wallet_id TEXT NOT NULL REFERENCES wallet_balances(wallet_id), chain TEXT NOT NULL, expires_at TEXT NOT NULL, spent INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL DEFAULT (datetime('now')));
CREATE INDEX IF NOT EXISTS idx_deposit_refs_wallet_id ON deposit_references(wallet_id);
CREATE INDEX IF NOT EXISTS idx_deposit_refs_expires ON deposit_references(expires_at);
-- Encumbrance table — rJoule locks for API key allocations
CREATE TABLE IF NOT EXISTS encumbrances (key_id TEXT PRIMARY KEY REFERENCES api_keys(key_id), wallet_id TEXT NOT NULL REFERENCES wallet_balances(wallet_id), amount_rj INTEGER NOT NULL, consumed_rj INTEGER NOT NULL DEFAULT 0, status TEXT NOT NULL DEFAULT 'active', created_at TEXT NOT NULL DEFAULT (datetime('now')), released_at TEXT);
CREATE INDEX IF NOT EXISTS idx_encumbrances_wallet_id ON encumbrances(wallet_id);
-- Kata practice history — tracks practice frequency, streaks, and automaticity across sessions
CREATE TABLE IF NOT EXISTS kata_history (id INTEGER PRIMARY KEY AUTOINCREMENT, agent_name TEXT NOT NULL, date TEXT NOT NULL, kata_type TEXT NOT NULL, practice_name TEXT NOT NULL, steps_completed INTEGER NOT NULL DEFAULT 0, gas_consumed INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL DEFAULT (datetime('now')));
CREATE INDEX IF NOT EXISTS idx_kata_history_agent ON kata_history(agent_name);
CREATE INDEX IF NOT EXISTS idx_kata_history_date ON kata_history(date);
CREATE INDEX IF NOT EXISTS idx_kata_history_type ON kata_history(kata_type);
-- Pod metadata — webid, pod_kind, created_at for passphrase derivation and discovery
CREATE TABLE IF NOT EXISTS pod_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
INSERT OR IGNORE INTO pod_meta (key, value) VALUES ('schema_version', '2');
