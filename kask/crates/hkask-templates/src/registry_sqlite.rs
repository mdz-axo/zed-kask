//! SQLite registry adapter — persistent template registry backed by SQLite.
//!
//! Connection pooling via r2d2 for thread-safe shared access.
//! Use `new_with_pool()` when opening through `hkask_storage::Database`.

use crate::bundle::BundleManifest;
use crate::bundle::BundleRegistryIndex;
use crate::ports::{Result, TemplateError};
use hkask_types::SkillPolarity;
use hkask_types::template_type::TemplateType;
use hkask_types::{InfrastructureError, NotFound};
use hkask_types::{
    RegistryEntry, RegistryError, RegistryIndex, Skill, SkillRegistryIndex, SkillZone,
};
use rusqlite::params;
use tracing;

type SkillRow = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
);
type TemplateRow = (String, TemplateType, String, String, String, u32, u32);

fn parse_template_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TemplateRow> {
    let tt_str: String = row.get(1)?;
    let tt = TemplateType::parse_str(&tt_str).ok_or_else(|| {
        rusqlite::Error::ToSqlConversionFailure(format!("Unknown template type: {}", tt_str).into())
    })?;
    Ok((
        row.get(0)?,
        tt,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
    ))
}

// ── SqliteRegistry ─────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SqliteRegistry {
    pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
}

impl SqliteRegistry {
    /// Create a new SQLite-backed registry.
    ///
    /// expect: "The system persists template registrations to SQLite"
    /// \[P3\] Motivating: Generative Space — SQLite-backed template registry
    /// pre:  path is None (in-memory) or a valid filesystem path
    /// post: returns SqliteRegistry with schema initialized
    pub fn new(path: Option<&str>) -> Result<Self> {
        let manager = match path {
            Some(p) => {
                if let Some(parent) = std::path::Path::new(p).parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        TemplateError::Manifest(format!(
                            "Failed to create registry directory {}: {}",
                            parent.display(),
                            e
                        ))
                    })?;
                }
                r2d2_sqlite::SqliteConnectionManager::file(p)
            }
            None => {
                tracing::warn!(target: "hkask.templates",
                    "No database path — template registry is in-memory and will be lost on restart.");
                r2d2_sqlite::SqliteConnectionManager::memory()
            }
        };
        let pool = r2d2::Pool::builder()
            .max_size(4)
            .build(manager)
            .map_err(|e| TemplateError::Manifest(format!("Failed to create pool: {}", e)))?;
        let mut registry = Self { pool };
        registry.init_schema()?;
        Ok(registry)
    }

    /// Create a registry from an existing r2d2 connection pool.
    ///
    /// expect: "The system persists template registrations to SQLite"
    /// \[P3\] Motivating: Generative Space — SQLite registry from existing pool
    /// pre:  pool is a valid SQLite connection pool
    /// post: returns SqliteRegistry with schema initialized on the given pool
    pub fn new_with_pool(pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>) -> Result<Self> {
        let mut registry = Self { pool };
        registry.init_schema()?;
        Ok(registry)
    }

    fn init_schema(&mut self) -> Result<()> {
        self.pool
            .get()
            .map_err(|e| TemplateError::Database(InfrastructureError::database(e.to_string())))?
            .execute_batch(concat!(
            "CREATE TABLE IF NOT EXISTS templates(id TEXT PRIMARY KEY, template_type TEXT NOT NULL, name TEXT NOT NULL DEFAULT '', description TEXT, source_path TEXT NOT NULL, cascade_level INTEGER NOT NULL DEFAULT 0, matroshka_limit INTEGER NOT NULL DEFAULT 7, created_at DATETIME DEFAULT CURRENT_TIMESTAMP, updated_at DATETIME DEFAULT CURRENT_TIMESTAMP);",
            "CREATE TABLE IF NOT EXISTS provenance(id INTEGER PRIMARY KEY AUTOINCREMENT, template_id TEXT NOT NULL, git_sha TEXT NOT NULL, modified_by TEXT NOT NULL, modified_at DATETIME DEFAULT CURRENT_TIMESTAMP, branch TEXT, commit_message TEXT, FOREIGN KEY(template_id) REFERENCES templates(id));",
            "CREATE INDEX IF NOT EXISTS idx_templates_type ON templates(template_type);",
            "CREATE INDEX IF NOT EXISTS idx_provenance_template ON provenance(template_id);",
            "CREATE TABLE IF NOT EXISTS skills(id TEXT PRIMARY KEY, domain TEXT NOT NULL, word_act TEXT, flow_def TEXT, know_act TEXT, polarity TEXT, content_hash TEXT, zone TEXT NOT NULL DEFAULT 'private', namespace TEXT, created_at DATETIME DEFAULT CURRENT_TIMESTAMP);",
            "CREATE INDEX IF NOT EXISTS idx_skills_domain ON skills(domain);",
            "CREATE TABLE IF NOT EXISTS bundles(id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL, version TEXT NOT NULL, editor TEXT NOT NULL DEFAULT 'curator-or-human-admin', manifest_json TEXT NOT NULL, created_at DATETIME DEFAULT CURRENT_TIMESTAMP, updated_at DATETIME DEFAULT CURRENT_TIMESTAMP);",
            "CREATE TABLE IF NOT EXISTS bundle_skills(bundle_id TEXT NOT NULL, skill_id TEXT NOT NULL, polarity TEXT, manifest_ref TEXT, content_hash TEXT, position INTEGER NOT NULL, PRIMARY KEY(bundle_id, skill_id), FOREIGN KEY(bundle_id) REFERENCES bundles(id));",
            "CREATE INDEX IF NOT EXISTS idx_bundle_skills_bundle ON bundle_skills(bundle_id);",
            "CREATE INDEX IF NOT EXISTS idx_bundle_skills_skill ON bundle_skills(skill_id);",
        )).map_err(|e| TemplateError::Manifest(format!("Schema init: {}", e)))?;
        Ok(())
    }

    /// Register a template entry in the registry.
    ///
    /// expect: "The system persists template registrations to SQLite"
    /// \[P3\] Motivating: Generative Space — persists template registration
    /// pre:  entry.id is non-empty, entry.template_type is valid
    /// post: entry inserted or replaced in templates table
    /// post: capabilities synced
    pub fn register(&mut self, entry: RegistryEntry) -> Result<()> {
        for warning in &entry.validate() {
            tracing::warn!(target: "hkask.templates", "{}", warning);
        }
        let mut conn = self
            .pool
            .get()
            .map_err(|e| TemplateError::Database(InfrastructureError::database(e.to_string())))?;
        let tx = conn
            .transaction()
            .map_err(|e| TemplateError::Manifest(format!("Transaction: {}", e)))?;
        tx.execute(
            "INSERT OR REPLACE INTO templates (id, template_type, name, description, source_path, cascade_level, matroshka_limit, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CURRENT_TIMESTAMP)",
            params![entry.id, entry.template_type.as_str(), entry.name, entry.description, entry.source_path, entry.cascade_level, entry.matroshka_limit],
        ).map_err(|e| TemplateError::Manifest(format!("Insert: {}", e)))?;
        tx.commit()
            .map_err(|e| TemplateError::Manifest(format!("Commit: {}", e)))?;
        Ok(())
    }

    fn row_to_entry(
        id: &str,
        tt: TemplateType,
        name: String,
        desc: String,
        sp: String,
        cl: u32,
        ml: u32,
    ) -> Result<RegistryEntry> {
        Ok(RegistryEntry {
            id: id.to_string(),
            template_type: tt,
            name,
            description: desc,
            source_path: sp,
            cascade_level: cl,
            matroshka_limit: ml,
        })
    }

    /// Get a template entry by ID.
    ///
    /// expect: "The system persists template registrations to SQLite"
    /// \[P3\] Motivating: Generative Space — retrieves persisted template entry
    /// pre:  id is non-empty
    /// post: returns RegistryEntry if found
    /// post: returns Err(NotFound) if not found
    pub fn get_entry(&self, id: &str) -> Result<RegistryEntry> {
        let conn = self
            .pool
            .get()
            .map_err(|e| TemplateError::Database(InfrastructureError::database(e.to_string())))?;
        let row = conn
            .prepare(Self::_T_SELECT)
            .map_err(|e| {
                TemplateError::Database(InfrastructureError::database(format!("Prepare: {}", e)))
            })?
            .query_row(params![id], parse_template_row)
            .map_err(|e| {
                TemplateError::NotFound(NotFound {
                    entity_type: "template".to_string(),
                    id: format!("Template '{}': {}", id, e),
                })
            })?;
        Self::row_to_entry(&row.0, row.1, row.2, row.3, row.4, row.5, row.6)
    }

    /// Delete a template and all associated data (capabilities, provenance).
    /// Returns the entry if it existed, None otherwise.
    ///
    /// expect: "The system persists template registrations to SQLite"
    /// \[P3\] Motivating: Generative Space — removes persisted template entry
    /// pre:  id is non-empty
    /// post: template and associated data deleted
    /// post: returns Some(entry) if existed, None otherwise
    pub fn delete_entry(&mut self, id: &str) -> Option<RegistryEntry> {
        let entry = self.get_entry(id).ok();
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(target: "hkask.templates", error = %e, id = %id, "delete_entry: pool connection failed");
                return entry;
            }
        };
        for table in &["provenance"] {
            if let Err(e) = conn.execute(
                &format!("DELETE FROM {} WHERE template_id = ?1", table),
                params![id],
            ) {
                tracing::error!(target: "hkask.templates", error = %e, id = %id, table = table, "delete_entry: DELETE failed");
            }
        }
        if let Err(e) = conn.execute("DELETE FROM templates WHERE id = ?1", params![id]) {
            tracing::error!(target: "hkask.templates", error = %e, id = %id, "delete_entry: DELETE templates failed");
        }
        entry
    }

    /// Count registered templates.
    ///
    /// expect: "The system persists template registrations to SQLite"
    /// \[P3\] Motivating: Generative Space — reports persisted registry size
    /// post: returns count of templates in registry
    /// post: returns 0 on lock/query error (graceful degradation, with a `warn!`)
    pub fn count(&self) -> usize {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(target: "hkask.templates", error = %e, "count: pool get failed, returning 0");
                return 0;
            }
        };
        match conn.query_row("SELECT COUNT(*) FROM templates", [], |row| {
            row.get::<_, i64>(0)
        }) {
            Ok(count) => count as usize,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.templates",
                    error = %e,
                    "count: SELECT COUNT(*) failed, returning 0 — a locked or corrupt templates table reads as zero"
                );
                0
            }
        }
    }

    const _T_SELECT: &str = "SELECT id, template_type, name, description, source_path, cascade_level, matroshka_limit FROM templates WHERE id = ?1";
}

// ── RegistryIndex ──────────────────────────────────────────────────────────

impl RegistryIndex for SqliteRegistry {
    fn list(&self, domain_hint: Option<TemplateType>) -> Vec<RegistryEntry> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.templates",
                    error = %e,
                    "list: pool get failed, returning empty"
                );
                return Vec::new();
            }
        };
        let sql = "SELECT id, template_type, name, description, source_path, cascade_level, matroshka_limit FROM templates";
        let (query_sql, query_params): (&str, &[rusqlite::types::Value]) = match &domain_hint {
            Some(tt) => (
                &format!("{sql} WHERE template_type = ?1"),
                &[rusqlite::types::Value::Text(tt.as_str().to_string())][..],
            ),
            None => (sql, &[]),
        };
        let mut stmt = match conn.prepare(query_sql) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.templates",
                    error = %e,
                    "list: prepare failed, returning empty"
                );
                return Vec::new();
            }
        };
        let rows: Vec<TemplateRow> = match stmt.query_map(
            rusqlite::params_from_iter(
                query_params
                    .iter()
                    .map(|v| v as &dyn rusqlite::types::ToSql),
            ),
            parse_template_row,
        ) {
            Ok(m) => m.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                tracing::warn!(
                    target: "hkask.templates",
                    error = %e,
                    "list: query_map failed, returning empty"
                );
                return Vec::new();
            }
        };
        rows.into_iter()
            .filter_map(|(id, tt, name, desc, sp, cl, ml)| {
                Self::row_to_entry(&id, tt, name, desc, sp, cl, ml).ok()
            })
            .collect()
    }

    fn get(&self, id: &str) -> std::result::Result<RegistryEntry, hkask_types::RegistryError> {
        self.get_entry(id).map_err(|e| {
            hkask_types::RegistryError::NotFound(NotFound {
                entity_type: "template".to_string(),
                id: format!("Template '{}': {}", id, e),
            })
        })
    }
}

// ── SkillRegistryIndex ─────────────────────────────────────────────────────

impl SkillRegistryIndex for SqliteRegistry {
    fn register_skill(&mut self, skill: Skill) -> std::result::Result<(), RegistryError> {
        let conn = self
            .pool
            .get()
            .map_err(|e| RegistryError::Other(format!("pool connection failed: {e}")))?;
        conn.execute(
            "INSERT OR REPLACE INTO skills (id, domain, word_act, flow_def, know_act, polarity, content_hash, zone, namespace) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![skill.id, skill.domain.as_str(), skill.word_act, skill.flow_def, skill.know_act,
                skill.polarity.as_ref().map(|p| p.as_str()), skill.content_hash,
                skill.zone.as_str(), skill.namespace],
        ).map_err(|e| {
            tracing::error!(target: "hkask.templates", error = %e, skill_id = %skill.id, "register_skill: INSERT failed");
            RegistryError::Other(format!("register_skill INSERT failed: {e}"))
        })?;
        Ok(())
    }

    fn get_skill(&self, id: &str) -> Option<Skill> {
        self.get_skill_owned(id)
    }
    fn list_skills(&self) -> Vec<Skill> {
        self.list_skills_owned()
    }
    fn skills_by_domain(&self, domain: TemplateType) -> Vec<Skill> {
        self.skills_by_domain_owned(domain)
    }
    fn skills_referencing_template(&self, tid: &str) -> Vec<Skill> {
        self.skills_referencing_template_owned(tid)
    }

    fn remove_skill(&mut self, id: &str) -> std::result::Result<Option<Skill>, RegistryError> {
        let skill = self.get_skill_owned(id);
        let conn = self
            .pool
            .get()
            .map_err(|e| RegistryError::Other(format!("pool connection failed: {e}")))?;
        conn.execute("DELETE FROM skills WHERE id = ?1", params![id]).map_err(|e| {
            tracing::error!(target: "hkask.templates", error = %e, id = %id, "remove_skill: DELETE failed");
            RegistryError::Other(format!("remove_skill DELETE failed: {e}"))
        })?;
        Ok(skill)
    }
}

// ── BundleRegistryIndex ────────────────────────────────────────────────────

impl BundleRegistryIndex for SqliteRegistry {
    fn register_bundle(
        &mut self,
        bundle: BundleManifest,
    ) -> std::result::Result<(), crate::ports::TemplateError> {
        let manifest_json = serde_json::to_string(&bundle).map_err(|e| {
            tracing::error!(target: "hkask.templates", error = %e, bundle_id = %bundle.id, "register_bundle: serialize failed");
            crate::ports::TemplateError::Manifest(format!("register_bundle serialize failed for '{bundle_id}': {e}", bundle_id = bundle.id))
        })?;
        let conn = self.pool.get().map_err(|e| {
            crate::ports::TemplateError::Database(hkask_types::InfrastructureError::Io(format!(
                "pool connection failed: {e}"
            )))
        })?;
        conn.execute("INSERT OR REPLACE INTO bundles (id, name, description, version, editor, manifest_json, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, CURRENT_TIMESTAMP)", params![bundle.id, bundle.name, bundle.description, bundle.version, bundle.editor, manifest_json]).map_err(|e| {
            tracing::error!(target: "hkask.templates", error = %e, bundle_id = %bundle.id, "register_bundle: INSERT failed");
            crate::ports::TemplateError::Manifest(format!("register_bundle INSERT failed for '{bundle_id}': {e}", bundle_id = bundle.id))
        })?;
        conn.execute(
            "DELETE FROM bundle_skills WHERE bundle_id = ?1",
            params![bundle.id],
        ).map_err(|e| {
            tracing::error!(target: "hkask.templates", error = %e, bundle_id = %bundle.id, "register_bundle: DELETE bundle_skills failed");
            crate::ports::TemplateError::Manifest(format!("register_bundle DELETE bundle_skills failed for '{bundle_id}': {e}", bundle_id = bundle.id))
        })?;
        for (position, skill) in bundle.skills.iter().enumerate() {
            conn.execute("INSERT INTO bundle_skills (bundle_id, skill_id, polarity, manifest_ref, content_hash, position) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![bundle.id, skill.id, Some(skill.polarity.as_str()), skill.manifest_ref, skill.content_hash, position as i64]).map_err(|e| {
                tracing::error!(target: "hkask.templates", error = %e, bundle_id = %bundle.id, skill_id = %skill.id, "register_bundle: INSERT bundle_skills failed");
                crate::ports::TemplateError::Manifest(format!("register_bundle INSERT bundle_skills failed for '{bundle_id}' skill '{skill_id}': {e}", bundle_id = bundle.id, skill_id = skill.id))
            })?;
        }
        Ok(())
    }

    fn get_bundle(&self, id: &str) -> Option<BundleManifest> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.templates",
                    bundle_id = %id,
                    error = %e,
                    "get_bundle: pool get failed, returning None"
                );
                return None;
            }
        };
        match conn.query_row(
            "SELECT manifest_json FROM bundles WHERE id = ?1",
            params![id],
            |row| row.get::<_, String>(0),
        ) {
            Ok(json) => serde_json::from_str(&json).ok(),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.templates",
                    bundle_id = %id,
                    error = %e,
                    "get_bundle: query failed (not NotFound), returning None"
                );
                None
            }
        }
    }

    fn list_bundles(&self) -> Vec<BundleManifest> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.templates",
                    error = %e,
                    "list_bundles: pool get failed, returning empty"
                );
                return Vec::new();
            }
        };
        let mut stmt = match conn.prepare("SELECT manifest_json FROM bundles") {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.templates",
                    error = %e,
                    "list_bundles: prepare failed, returning empty"
                );
                return Vec::new();
            }
        };
        match stmt.query_map([], |row| row.get::<_, String>(0)) {
            Ok(rows) => rows
                .filter_map(|r| r.ok())
                .filter_map(|json| serde_json::from_str(&json).ok())
                .collect(),
            Err(e) => {
                tracing::warn!(
                    target: "hkask.templates",
                    error = %e,
                    "list_bundles: query_map failed, returning empty"
                );
                Vec::new()
            }
        }
    }

    fn remove_bundle(
        &mut self,
        id: &str,
    ) -> std::result::Result<Option<BundleManifest>, crate::ports::TemplateError> {
        let bundle = self.get_bundle(id);
        let conn = self.pool.get().map_err(|e| {
            crate::ports::TemplateError::Database(hkask_types::InfrastructureError::Io(format!(
                "pool connection failed: {e}"
            )))
        })?;
        conn.execute(
            "DELETE FROM bundle_skills WHERE bundle_id = ?1",
            params![id],
        ).map_err(|e| {
            tracing::error!(target: "hkask.templates", error = %e, id = %id, "remove_bundle: DELETE bundle_skills failed");
            crate::ports::TemplateError::Manifest(format!("remove_bundle DELETE bundle_skills failed for '{id}': {e}"))
        })?;
        conn.execute("DELETE FROM bundles WHERE id = ?1", params![id]).map_err(|e| {
            tracing::error!(target: "hkask.templates", error = %e, id = %id, "remove_bundle: DELETE bundles failed");
            crate::ports::TemplateError::Manifest(format!("remove_bundle DELETE bundles failed for '{id}': {e}"))
        })?;
        Ok(bundle)
    }

    fn find_bundle_by_skills(&self, skill_ids: &[String]) -> Option<BundleManifest> {
        let target: std::collections::HashSet<&str> =
            skill_ids.iter().map(|s| s.as_str()).collect();
        self.list_bundles().into_iter().find(|b| {
            b.skills
                .iter()
                .map(|s| s.id.as_str())
                .collect::<std::collections::HashSet<_>>()
                == target
        })
    }
}

// ── Owned-skill retrieval ──────────────────────────────────────────────────

impl SqliteRegistry {
    fn row_to_skill(
        id: String,
        domain_str: String,
        word_act: Option<String>,
        flow_def: Option<String>,
        know_act: Option<String>,
        polarity_str: Option<String>,
        content_hash: Option<String>,
        zone_str: String,
        namespace: Option<String>,
    ) -> Option<Skill> {
        let domain = TemplateType::parse_str(&domain_str).unwrap_or(TemplateType::FlowDef);
        let mut skill = Skill::new(&id, domain);
        if let Some(wa) = word_act {
            skill = skill.with_word_act(&wa);
        }
        if let Some(fd) = flow_def {
            skill = skill.with_flow_def(&fd);
        }
        if let Some(ka) = know_act {
            skill = skill.with_know_act(&ka);
        }
        if let Some(p) = polarity_str.and_then(|s| SkillPolarity::parse_str(&s)) {
            skill = skill.with_polarity(p);
        }
        if let Some(ch) = content_hash {
            skill = skill.with_content_hash(ch);
        }
        let zone = SkillZone::parse_str(&zone_str).unwrap_or_else(|| {
            tracing::warn!(
                target: "hkask.templates",
                skill_id = %id,
                zone_str = %zone_str,
                "row_to_skill: unknown zone string — defaulting to Private."
            );
            SkillZone::Private
        });
        skill = skill.with_zone(zone);
        if let Some(ns) = namespace {
            skill = skill.with_namespace(ns);
        }
        Some(skill)
    }

    /// Get a skill by ID (owned query, no OCAP check).
    ///
    /// expect: "The system persists template registrations to SQLite"
    /// \[P3\] Motivating: Generative Space — retrieves owned skill record
    /// pre:  id is non-empty
    /// post: returns Some(Skill) if found, None otherwise
    ///
    /// `NotFound` (no row for `id`) returns `None` with no warn — that is the
    /// expected "no such skill" case. Every other failure (pool unavailable,
    /// query error, schema mismatch) returns `None` *with* a `warn!` so an
    /// operator can distinguish "no such skill" from "the DB is broken" —
    /// collapsing the two made a locked table read as "no skills" (F7).
    pub fn get_skill_owned(&self, id: &str) -> Option<Skill> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.templates",
                    skill_id = %id,
                    error = %e,
                    "get_skill_owned: pool get failed, returning None"
                );
                return None;
            }
        };
        match conn.query_row(
            "SELECT id, domain, word_act, flow_def, know_act, polarity, content_hash, zone, namespace FROM skills WHERE id = ?1", params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?)),
        ) {
            Ok((id, ds, wa, fd, ka, ps, ch, zs, ns)) => Self::row_to_skill(id, ds, wa, fd, ka, ps, ch, zs, ns),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.templates",
                    skill_id = %id,
                    error = %e,
                    "get_skill_owned: query failed (not NotFound), returning None"
                );
                None
            }
        }
    }

    fn query_skills(&self, sql: &str, params: &[rusqlite::types::Value]) -> Vec<Skill> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.templates",
                    error = %e,
                    "query_skills: pool get failed, returning empty"
                );
                return Vec::new();
            }
        };
        let mut stmt = match conn.prepare(sql) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.templates",
                    error = %e,
                    "query_skills: prepare failed, returning empty"
                );
                return Vec::new();
            }
        };
        let rows: Vec<SkillRow> = match stmt.query_map(
            rusqlite::params_from_iter(params.iter().map(|v| v as &dyn rusqlite::types::ToSql)),
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            },
        ) {
            Ok(m) => m.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                tracing::warn!(
                    target: "hkask.templates",
                    error = %e,
                    "query_skills: query_map failed, returning empty"
                );
                return Vec::new();
            }
        };
        let mut skills = Vec::with_capacity(rows.len());
        for (id, ds, wa, fd, ka, ps, ch, zs, ns) in rows {
            if let Some(s) = Self::row_to_skill(id, ds, wa, fd, ka, ps, ch, zs, ns) {
                skills.push(s);
            }
        }
        skills
    }

    const _SKILLS_SELECT: &str = "SELECT id, domain, word_act, flow_def, know_act, polarity, content_hash, zone, namespace FROM skills";

    /// List all skills (owned query, no OCAP check).
    ///
    /// expect: "The system persists template registrations to SQLite"
    /// \[P3\] Motivating: Generative Space — lists owned skill records
    /// post: returns `Vec<Skill>` with all registered skills
    pub fn list_skills_owned(&self) -> Vec<Skill> {
        self.query_skills(Self::_SKILLS_SELECT, &[])
    }

    /// List skills by domain (owned query, no OCAP check).
    ///
    /// expect: "The system persists template registrations to SQLite"
    /// \[P3\] Motivating: Generative Space — domain-filtered owned skill listing
    /// pre:  domain is a valid TemplateType
    /// post: returns `Vec<Skill>` filtered by domain
    pub fn skills_by_domain_owned(&self, domain: TemplateType) -> Vec<Skill> {
        self.query_skills(
            &format!("{} WHERE domain = ?1", Self::_SKILLS_SELECT),
            &[rusqlite::types::Value::Text(domain.as_str().to_string())],
        )
    }

    /// List skills referencing a template (owned query, no OCAP check).
    ///
    /// expect: "The system persists template registrations to SQLite"
    /// \[P3\] Motivating: Generative Space — reverse owned skill lookup
    /// pre:  tid is non-empty
    /// post: returns `Vec<Skill>` referencing the given template ID
    pub fn skills_referencing_template_owned(&self, tid: &str) -> Vec<Skill> {
        self.query_skills(
            &format!(
                "{} WHERE word_act = ?1 OR flow_def = ?1 OR know_act = ?1",
                Self::_SKILLS_SELECT
            ),
            &[rusqlite::types::Value::Text(tid.to_string())],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Construct an in-memory registry with the schema initialized.
    fn fresh_registry() -> SqliteRegistry {
        SqliteRegistry::new(None).expect("in-memory registry")
    }

    /// F5/F6/F7 — a registry whose `templates`/`skills`/`bundles` tables have
    /// been dropped (simulating a corrupt or migrated schema) must not panic.
    /// `count` returns 0, `list`/`list_skills_owned`/`list_bundles` return empty,
    /// and `get_skill_owned`/`get_bundle` return `None`. The `warn!`s are the
    /// operator's signal that the table is broken (previously these collapsed
    /// silently — a locked table read as "0 templates" with no signal).
    #[test]
    fn methods_degrade_gracefully_on_missing_tables() {
        let registry = fresh_registry();
        // Drop the tables to simulate a corrupt/migrated schema.
        {
            let conn = registry.pool.get().expect("pool");
            conn.execute_batch("DROP TABLE templates; DROP TABLE skills; DROP TABLE bundles;")
                .expect("drop tables");
        }
        // `count` — query_row fails (no table), must return 0 not panic.
        assert_eq!(
            registry.count(),
            0,
            "count must return 0 on missing templates table"
        );
        // `list` — prepare fails, must return empty not panic.
        assert!(
            RegistryIndex::list(&registry, None).is_empty(),
            "list must return empty on missing templates table"
        );
        // `list_skills_owned` — prepare fails, must return empty not panic.
        assert!(
            registry.list_skills_owned().is_empty(),
            "list_skills_owned must return empty on missing skills table"
        );
        // `list_bundles` — prepare fails, must return empty not panic.
        assert!(
            registry.list_bundles().is_empty(),
            "list_bundles must return empty on missing bundles table"
        );
        // `get_skill_owned` — query fails (not NotFound), must return None not panic.
        assert!(
            registry.get_skill_owned("any").is_none(),
            "get_skill_owned must return None on missing skills table"
        );
        // `get_bundle` — query fails (not NotFound), must return None not panic.
        assert!(
            registry.get_bundle("any").is_none(),
            "get_bundle must return None on missing bundles table"
        );
    }

    /// F7 — `get_skill_owned` distinguishes `NotFound` (no row) from a query
    /// error. Both return `None`, but only the latter `warn!`s. This test pins
    /// the behavioral contract: a missing skill returns `None` without panic,
    /// and a skill that exists returns `Some`.
    #[test]
    fn get_skill_owned_returns_none_for_missing_skill() {
        let registry = fresh_registry();
        assert!(
            registry.get_skill_owned("does-not-exist").is_none(),
            "get_skill_owned must return None for a missing skill (NotFound, no warn)"
        );
    }

    /// Sanity: `count` returns the actual template count when the table is healthy.
    #[test]
    fn count_returns_actual_count_on_healthy_table() {
        let registry = fresh_registry();
        // Insert two templates directly.
        {
            let conn = registry.pool.get().expect("pool");
            conn.execute(
                "INSERT INTO templates (id, template_type, name, source_path) VALUES (?1, ?2, ?3, ?4)",
                params!["t1", "flowdef", "Test 1", "/path/t1"],
            )
            .expect("insert t1");
            conn.execute(
                "INSERT INTO templates (id, template_type, name, source_path) VALUES (?1, ?2, ?3, ?4)",
                params!["t2", "flowdef", "Test 2", "/path/t2"],
            )
            .expect("insert t2");
        }
        assert_eq!(
            registry.count(),
            2,
            "count must return the actual template count"
        );
    }
}
