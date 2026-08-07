-- Postgres-compatible schema for hkask-storage.
-- Mirrors sql/schema.sql with Postgres-specific syntax:
--   datetime('now') → now()
--   INTEGER PRIMARY KEY AUTOINCREMENT → BIGSERIAL PRIMARY KEY
--   CREATE VIRTUAL TABLE ... vec0(...) → CREATE TABLE ... vector(N) + ivfflat index
--   INSERT OR IGNORE → INSERT ... ON CONFLICT DO NOTHING
--   BLOB → BYTEA
-- Run via open_postgres() after CREATE EXTENSION vector.

CREATE TABLE IF NOT EXISTS hmems (id TEXT PRIMARY KEY, entity TEXT NOT NULL, attribute TEXT NOT NULL, value TEXT NOT NULL, valid_from TEXT NOT NULL, valid_to TEXT, recalled_at TEXT NOT NULL DEFAULT (now()::text), transaction_at TEXT DEFAULT (now()::text), confidence REAL NOT NULL DEFAULT 1.0, perspective TEXT, visibility TEXT NOT NULL DEFAULT 'private', owner_webid TEXT NOT NULL, dimension TEXT, swarm_id TEXT);
CREATE INDEX IF NOT EXISTS idx_hmems_swarm_id ON hmems(swarm_id);
CREATE TABLE IF NOT EXISTS embeddings (id TEXT PRIMARY KEY, entity_ref TEXT NOT NULL, embedding vector($DIM) NOT NULL, dimensions INTEGER NOT NULL, model TEXT NOT NULL, created_at TEXT DEFAULT (now()::text));
CREATE INDEX IF NOT EXISTS idx_embeddings_entity_ref ON embeddings(entity_ref);
CREATE INDEX IF NOT EXISTS idx_embeddings_embedding ON embeddings USING ivfflat (embedding vector_cosine_ops);
CREATE TABLE IF NOT EXISTS nu_events (id TEXT PRIMARY KEY, timestamp TEXT NOT NULL, observer_webid TEXT NOT NULL, span_category TEXT NOT NULL, span_path TEXT NOT NULL, phase TEXT NOT NULL, observation TEXT NOT NULL, regulation TEXT, outcome TEXT, recursion_depth INTEGER NOT NULL, parent_event TEXT, visibility TEXT NOT NULL DEFAULT 'private');
CREATE INDEX IF NOT EXISTS idx_nu_events_timestamp_category ON nu_events(timestamp, span_category);
CREATE INDEX IF NOT EXISTS idx_nu_events_category_phase ON nu_events(span_category, phase);
CREATE TABLE IF NOT EXISTS audit_log (id TEXT PRIMARY KEY, timestamp TEXT NOT NULL, actor_webid TEXT NOT NULL, action TEXT NOT NULL, resource TEXT NOT NULL, outcome TEXT NOT NULL, details TEXT, ip_address TEXT, created_at TEXT DEFAULT (now()::text));
CREATE INDEX IF NOT EXISTS idx_audit_log_timestamp ON audit_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_log_actor ON audit_log(actor_webid);
CREATE TABLE IF NOT EXISTS reg_variety_checkpoint (domain TEXT PRIMARY KEY, variety_count INTEGER NOT NULL, last_updated TEXT NOT NULL, threshold INTEGER NOT NULL DEFAULT 10);
CREATE TABLE IF NOT EXISTS reg_alerts (id TEXT PRIMARY KEY, timestamp TEXT NOT NULL, alert_type TEXT NOT NULL, severity TEXT NOT NULL, domain TEXT, message TEXT NOT NULL, resolved INTEGER NOT NULL DEFAULT 0, resolved_at TEXT);
CREATE TABLE IF NOT EXISTS agent_registry (name TEXT PRIMARY KEY, agent_kind TEXT, definition_json TEXT NOT NULL, token_hash TEXT NOT NULL, registered_at TEXT NOT NULL, source_yaml TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_agent_registry_kind ON agent_registry(agent_kind);
CREATE TABLE IF NOT EXISTS loop_cursors (key TEXT PRIMARY KEY, value INTEGER NOT NULL, updated_at TEXT NOT NULL);
-- Kata practice history — tracks practice frequency, streaks, and automaticity across sessions
CREATE TABLE IF NOT EXISTS kata_history (id BIGSERIAL PRIMARY KEY, agent_name TEXT NOT NULL, date TEXT NOT NULL, kata_type TEXT NOT NULL, practice_name TEXT NOT NULL, steps_completed INTEGER NOT NULL DEFAULT 0, gas_consumed INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL DEFAULT (now()::text));
CREATE INDEX IF NOT EXISTS idx_kata_history_agent ON kata_history(agent_name);
CREATE INDEX IF NOT EXISTS idx_kata_history_date ON kata_history(date);
CREATE INDEX IF NOT EXISTS idx_kata_history_type ON kata_history(kata_type);
-- Pod metadata — webid, pod_kind, created_at for passphrase derivation and discovery
CREATE TABLE IF NOT EXISTS pod_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
INSERT INTO pod_meta (key, value) VALUES ('schema_version', '2') ON CONFLICT DO NOTHING;
