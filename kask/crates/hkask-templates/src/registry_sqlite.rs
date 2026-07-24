//! SQLite registry adapter — persistent template registry backed by SQLite.
//!
//! Connection pooling via r2d2 for thread-safe shared access.
//! Use `new_with_pool()` when opening through `hkask_storage::Database`.

use crate::bundle::BundleManifest;
use crate::bundle::BundleRegistryIndex;
use crate::ports::{Result, TemplateError};
use hkask_types::SkillPolarity;
use hkask_types::template_type::TemplateType;
use hkask_types::{InfrastructureError, NotFound, Visibility};
use hkask_types::{
    RegistryEntry, RegistryError, RegistryIndex, Skill, SkillRegistryIndex, SkillZone,
};
use rusqlite::{Connection, params};
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

fn query_column(conn: &Connection, sql: &str, id: &str) -> Result<Vec<String>> {
    let db_err = |ctx: &str, e| {
        TemplateError::Database(InfrastructureError::database(format!("{ctx}: {e}")))
    };
    let results: Vec<String> = conn
        .prepare(sql)
        .map_err(|e| db_err("Prepare", e))?
        .query_map(params![id], |row| row.get::<_, String>(0))
        .map_err(|e| db_err("Query", e))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(results)
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
            "CREATE TABLE IF NOT EXISTS lexicon_terms(template_id TEXT NOT NULL, term TEXT NOT NULL, PRIMARY KEY(template_id, term), FOREIGN KEY(template_id) REFERENCES templates(id));",
            "CREATE TABLE IF NOT EXISTS template_capabilities(template_id TEXT NOT NULL, capability TEXT NOT NULL, PRIMARY KEY(template_id, capability), FOREIGN KEY(template_id) REFERENCES templates(id));",
            "CREATE TABLE IF NOT EXISTS provenance(id INTEGER PRIMARY KEY AUTOINCREMENT, template_id TEXT NOT NULL, git_sha TEXT NOT NULL, modified_by TEXT NOT NULL, modified_at DATETIME DEFAULT CURRENT_TIMESTAMP, branch TEXT, commit_message TEXT, FOREIGN KEY(template_id) REFERENCES templates(id));",
            "CREATE INDEX IF NOT EXISTS idx_templates_type ON templates(template_type);",
            "CREATE INDEX IF NOT EXISTS idx_lexicon_terms ON lexicon_terms(term);",
            "CREATE INDEX IF NOT EXISTS idx_provenance_template ON provenance(template_id);",
            "CREATE INDEX IF NOT EXISTS idx_template_capabilities ON template_capabilities(capability);",
            "CREATE TABLE IF NOT EXISTS skills(id TEXT PRIMARY KEY, domain TEXT NOT NULL, word_act TEXT, flow_def TEXT, know_act TEXT, polarity TEXT, content_hash TEXT, visibility TEXT NOT NULL DEFAULT 'private', zone TEXT NOT NULL DEFAULT 'private', namespace TEXT, created_at DATETIME DEFAULT CURRENT_TIMESTAMP);",
            "CREATE INDEX IF NOT EXISTS idx_skills_domain ON skills(domain);",
            "CREATE INDEX IF NOT EXISTS idx_skills_visibility ON skills(visibility);",
            "CREATE TABLE IF NOT EXISTS bundles(id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL, version TEXT NOT NULL, editor TEXT NOT NULL DEFAULT 'curator-or-human-admin', visibility TEXT NOT NULL DEFAULT 'Private', manifest_json TEXT NOT NULL, created_at DATETIME DEFAULT CURRENT_TIMESTAMP, updated_at DATETIME DEFAULT CURRENT_TIMESTAMP);",
            "CREATE TABLE IF NOT EXISTS bundle_skills(bundle_id TEXT NOT NULL, skill_id TEXT NOT NULL, polarity TEXT, manifest_ref TEXT, content_hash TEXT, position INTEGER NOT NULL, PRIMARY KEY(bundle_id, skill_id), FOREIGN KEY(bundle_id) REFERENCES bundles(id));",
            "CREATE INDEX IF NOT EXISTS idx_bundles_visibility ON bundles(visibility);",
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
    /// post: lexicon_terms and capabilities synced
    pub fn register(&mut self, entry: RegistryEntry) -> Result<()> {
        for warning in &entry.validate() {
            tracing::warn!(target: "hkask.templates", "{}", warning);
        }
        // F-07 fix: unknown lexicon terms are now errors, not warnings.
        let vocab_warnings = crate::vocabulary::validate_entry(&entry);
        if !vocab_warnings.is_empty() {
            return Err(TemplateError::Validation(format!(
                "lexicon validation failed: {}",
                vocab_warnings.join("; ")
            )));
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
        for (table, col, items) in [
            ("lexicon_terms", "term", &entry.lexicon_terms),
            (
                "template_capabilities",
                "capability",
                &entry.required_capabilities,
            ),
        ] {
            tx.execute(
                &format!("DELETE FROM {} WHERE template_id = ?1", table),
                params![entry.id],
            )
            .map_err(|e| TemplateError::Manifest(format!("Delete {col}: {}", e)))?;
            for item in items {
                tx.execute(
                    &format!(
                        "INSERT INTO {} (template_id, {}) VALUES (?1, ?2)",
                        table, col
                    ),
                    params![entry.id, item],
                )
                .map_err(|e| TemplateError::Manifest(format!("Insert {col}: {}", e)))?;
            }
        }
        tx.commit()
            .map_err(|e| TemplateError::Manifest(format!("Commit: {}", e)))?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn row_to_entry(
        conn: &Connection,
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
            lexicon_terms: query_column(
                conn,
                "SELECT term FROM lexicon_terms WHERE template_id = ?1",
                id,
            )?,
            required_capabilities: query_column(
                conn,
                "SELECT capability FROM template_capabilities WHERE template_id = ?1",
                id,
            )?,
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
        Self::row_to_entry(&conn, &row.0, row.1, row.2, row.3, row.4, row.5, row.6)
    }

    /// Delete a template and all associated data (lexicon terms, capabilities, provenance).
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
        for table in &["lexicon_terms", "template_capabilities", "provenance"] {
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

    /// Search templates by lexicon term.
    ///
    /// expect: "The system persists template registrations to SQLite"
    /// \[P3\] Motivating: Generative Space — vocabulary-aware template search
    /// \[P8\] Constraining: Semantic Grounding — search uses lexicon terms
    /// pre:  term is non-empty
    /// post: returns `Vec<RegistryEntry>` for templates declaring this term
    pub fn search_by_lexicon(&self, term: &str) -> Result<Vec<RegistryEntry>> {
        let conn = self
            .pool
            .get()
            .map_err(|e| TemplateError::Database(InfrastructureError::database(e.to_string())))?;
        let rows: Vec<TemplateRow> = conn
    			.prepare("SELECT t.id, t.template_type, t.name, t.description, t.source_path, t.cascade_level, t.matroshka_limit FROM templates t JOIN lexicon_terms l ON t.id = l.template_id WHERE l.term = ?1")
    			.map_err(|e| TemplateError::Database(InfrastructureError::database(format!("Prepare: {}", e))))?
    			.query_map(params![term], parse_template_row)
    			.map_err(|e| TemplateError::Database(InfrastructureError::database(format!("Query: {}", e))))?
    			.filter_map(|r| r.ok()).collect();
        let mut results = Vec::new();
        for (id, tt, name, desc, sp, cl, ml) in rows {
            results.push(Self::row_to_entry(&conn, &id, tt, name, desc, sp, cl, ml)?);
        }
        Ok(results)
    }

    /// Count registered templates.
    ///
    /// expect: "The system persists template registrations to SQLite"
    /// \[P3\] Motivating: Generative Space — reports persisted registry size
    /// post: returns count of templates in registry
    /// post: returns 0 on lock error (graceful degradation)
    pub fn count(&self) -> usize {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(target: "hkask.templates", error = %e, "count: pool get failed, returning 0");
                return 0;
            }
        };
        conn.query_row("SELECT COUNT(*) FROM templates", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0) as usize
    }

    const _T_SELECT: &str = "SELECT id, template_type, name, description, source_path, cascade_level, matroshka_limit FROM templates WHERE id = ?1";
}

// ── RegistryIndex ──────────────────────────────────────────────────────────

impl RegistryIndex for SqliteRegistry {
    fn list(&self, domain_hint: Option<TemplateType>) -> Vec<RegistryEntry> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
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
            Err(_) => return Vec::new(),
        };
        let rows: Vec<TemplateRow> = stmt
            .query_map(
                rusqlite::params_from_iter(
                    query_params
                        .iter()
                        .map(|v| v as &dyn rusqlite::types::ToSql),
                ),
                parse_template_row,
            )
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        rows.into_iter()
            .filter_map(|(id, tt, name, desc, sp, cl, ml)| {
                Self::row_to_entry(&conn, &id, tt, name, desc, sp, cl, ml).ok()
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
        if let Err(e) = conn.execute(
            "INSERT OR REPLACE INTO skills (id, domain, word_act, flow_def, know_act, polarity, content_hash, visibility, zone, namespace) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![skill.id, skill.domain.as_str(), skill.word_act, skill.flow_def, skill.know_act,
                skill.polarity.as_ref().map(|p| p.as_str()), skill.content_hash,
                skill.visibility.as_str(), skill.zone.as_str(), skill.namespace],
        ) {
            tracing::error!(target: "hkask.templates", error = %e, skill_id = %skill.id, "register_skill: INSERT failed");
        }
        Ok(())
    }

    fn get_skill(&self, id: &str) -> Option<Skill> {
        self.get_skill_owned(id)
    }
    fn list_skills(&self) -> Vec<Skill> {
        self.list_skills_owned()
    }
    fn list_skills_by_visibility(&self, v: Visibility) -> Vec<Skill> {
        self.list_skills_owned()
            .into_iter()
            .filter(|s| s.visibility == v)
            .collect()
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
        if let Err(e) = conn.execute("DELETE FROM skills WHERE id = ?1", params![id]) {
            tracing::error!(target: "hkask.templates", error = %e, id = %id, "remove_skill: DELETE failed");
        }
        Ok(skill)
    }
}

// ── BundleRegistryIndex ────────────────────────────────────────────────────

impl BundleRegistryIndex for SqliteRegistry {
    fn register_bundle(
        &mut self,
        bundle: BundleManifest,
    ) -> std::result::Result<(), crate::ports::TemplateError> {
        let manifest_json = match serde_json::to_string(&bundle) {
            Ok(j) => j,
            Err(e) => {
                tracing::error!(target: "hkask.templates", error = %e, bundle_id = %bundle.id, "register_bundle: serialize failed");
                return Ok(());
            }
        };
        let conn = self.pool.get().map_err(|e| {
            crate::ports::TemplateError::Database(hkask_types::InfrastructureError::Io(format!(
                "pool connection failed: {e}"
            )))
        })?;
        if let Err(e) = conn.execute("INSERT OR REPLACE INTO bundles (id, name, description, version, editor, visibility, manifest_json, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CURRENT_TIMESTAMP)", params![bundle.id, bundle.name, bundle.description, bundle.version, bundle.editor, bundle.visibility.as_str(), manifest_json]) {
            tracing::error!(target: "hkask.templates", error = %e, bundle_id = %bundle.id, "register_bundle: INSERT failed");
            return Ok(());
        }
        if let Err(e) = conn.execute(
            "DELETE FROM bundle_skills WHERE bundle_id = ?1",
            params![bundle.id],
        ) {
            tracing::error!(target: "hkask.templates", error = %e, bundle_id = %bundle.id, "register_bundle: DELETE bundle_skills failed");
            return Ok(());
        }
        for (position, skill) in bundle.skills.iter().enumerate() {
            if let Err(e) = conn.execute("INSERT INTO bundle_skills (bundle_id, skill_id, polarity, manifest_ref, content_hash, position) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![bundle.id, skill.id, Some(skill.polarity.as_str()), skill.manifest_ref, skill.content_hash, position as i64]) {
                tracing::error!(target: "hkask.templates", error = %e, bundle_id = %bundle.id, skill_id = %skill.id, "register_bundle: INSERT bundle_skills failed");
            }
        }
        Ok(())
    }

    fn get_bundle(&self, id: &str) -> Option<BundleManifest> {
        self.pool
            .get()
            .ok()
            .and_then(|conn| {
                conn.query_row(
                    "SELECT manifest_json FROM bundles WHERE id = ?1",
                    params![id],
                    |row| row.get::<_, String>(0),
                )
                .ok()
            })
            .and_then(|json| serde_json::from_str(&json).ok())
    }

    fn list_bundles(&self) -> Vec<BundleManifest> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut stmt = match conn.prepare("SELECT manifest_json FROM bundles") {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |row| row.get::<_, String>(0))
            .ok()
            .map(|rows| {
                rows.filter_map(|r| r.ok())
                    .filter_map(|json| serde_json::from_str(&json).ok())
                    .collect()
            })
            .unwrap_or_default()
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
        if let Err(e) = conn.execute(
            "DELETE FROM bundle_skills WHERE bundle_id = ?1",
            params![id],
        ) {
            tracing::error!(target: "hkask.templates", error = %e, id = %id, "remove_bundle: DELETE bundle_skills failed");
        }
        if let Err(e) = conn.execute("DELETE FROM bundles WHERE id = ?1", params![id]) {
            tracing::error!(target: "hkask.templates", error = %e, id = %id, "remove_bundle: DELETE bundles failed");
        }
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
    #[allow(clippy::too_many_arguments)]
    fn row_to_skill(
        id: String,
        domain_str: String,
        word_act: Option<String>,
        flow_def: Option<String>,
        know_act: Option<String>,
        polarity_str: Option<String>,
        content_hash: Option<String>,
        visibility_str: String,
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
        skill = skill
            .with_visibility(Visibility::parse_str(&visibility_str).unwrap_or(Visibility::Private));
        skill = skill.with_zone(SkillZone::parse_str(&zone_str).unwrap_or(SkillZone::Private));
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
    pub fn get_skill_owned(&self, id: &str) -> Option<Skill> {
        let conn = self.pool.get().ok()?;
        conn.query_row(
            "SELECT id, domain, word_act, flow_def, know_act, polarity, content_hash, visibility, zone, namespace FROM skills WHERE id = ?1", params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?)),
        ).ok().and_then(|(id, ds, wa, fd, ka, ps, ch, vs, zs, ns)| Self::row_to_skill(id, ds, wa, fd, ka, ps, ch, vs, zs, ns))
    }

    fn query_skills(&self, sql: &str, params: &[rusqlite::types::Value]) -> Vec<Skill> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut stmt = match conn.prepare(sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
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
                    row.get(9)?,
                ))
            },
        ) {
            Ok(m) => m.filter_map(|r| r.ok()).collect(),
            Err(_) => return Vec::new(),
        };
        let mut skills = Vec::with_capacity(rows.len());
        for (id, ds, wa, fd, ka, ps, ch, vs, zs, ns) in rows {
            if let Some(s) = Self::row_to_skill(id, ds, wa, fd, ka, ps, ch, vs, zs, ns) {
                skills.push(s);
            }
        }
        skills
    }

    const _SKILLS_SELECT: &str = "SELECT id, domain, word_act, flow_def, know_act, polarity, content_hash, visibility, zone, namespace FROM skills";

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
