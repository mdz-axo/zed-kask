CREATE TABLE IF NOT EXISTS hmems (id TEXT PRIMARY KEY, entity TEXT NOT NULL, attribute TEXT NOT NULL, value TEXT NOT NULL, valid_from TEXT NOT NULL, valid_to TEXT, recalled_at TEXT NOT NULL DEFAULT (datetime('now')), confidence REAL NOT NULL DEFAULT 1.0, perspective TEXT, visibility TEXT NOT NULL DEFAULT 'private', owner_webid TEXT NOT NULL, ontology TEXT);
CREATE INDEX IF NOT EXISTS idx_hmems_entity ON hmems(entity);
CREATE INDEX IF NOT EXISTS idx_hmems_attribute ON hmems(attribute);
CREATE INDEX IF NOT EXISTS idx_hmems_entity_attribute ON hmems(entity, attribute);
CREATE TABLE IF NOT EXISTS embeddings (id TEXT PRIMARY KEY, entity_ref TEXT NOT NULL, vector BLOB NOT NULL, dimensions INTEGER NOT NULL, model TEXT NOT NULL, created_at TEXT DEFAULT (datetime('now')));
CREATE INDEX IF NOT EXISTS idx_embeddings_entity_ref ON embeddings(entity_ref);
CREATE VIRTUAL TABLE IF NOT EXISTS vec_embeddings USING vec0(embedding float[$DIM] distance_metric=cosine);
CREATE TABLE IF NOT EXISTS audit_log (id TEXT PRIMARY KEY, timestamp TEXT NOT NULL, actor_webid TEXT NOT NULL, action TEXT NOT NULL, resource TEXT NOT NULL, outcome TEXT NOT NULL, details TEXT, ip_address TEXT, created_at TEXT DEFAULT (datetime('now')));
CREATE INDEX IF NOT EXISTS idx_audit_log_timestamp ON audit_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_log_actor ON audit_log(actor_webid);
CREATE TABLE IF NOT EXISTS reg_variety_checkpoint (domain TEXT PRIMARY KEY, variety_count INTEGER NOT NULL, last_updated TEXT NOT NULL, threshold INTEGER NOT NULL DEFAULT 10);
CREATE TABLE IF NOT EXISTS reg_alerts (id TEXT PRIMARY KEY, timestamp TEXT NOT NULL, alert_type TEXT NOT NULL, severity TEXT NOT NULL, domain TEXT, message TEXT NOT NULL, resolved INTEGER NOT NULL DEFAULT 0, resolved_at TEXT);
CREATE TABLE IF NOT EXISTS agent_registry (name TEXT PRIMARY KEY, agent_kind TEXT, definition_json TEXT NOT NULL, token_hash TEXT NOT NULL, registered_at TEXT NOT NULL, source_yaml TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_agent_registry_kind ON agent_registry(agent_kind);
CREATE TABLE IF NOT EXISTS loop_cursors (key TEXT PRIMARY KEY, value INTEGER NOT NULL, updated_at TEXT NOT NULL);
-- Pod metadata — webid, pod_kind, created_at for passphrase derivation and discovery
CREATE TABLE IF NOT EXISTS pod_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
