//! Driver-backed registry adapter — persistent template registry backed by a
//! `StorageDriver` (the storage port in `hkask_types::storage`).
//!
//! hKask crates depend on the `StorageDriver` port, not on `rusqlite`/`r2d2`
//! (which conflict with zed's `libsqlite3-sys` 0.30.1). The `kask_bridge`
//! implements `StorageDriver` over zed's `sqlez`. Use `from_driver()` to
//! construct from a driver provided by the host.

use crate::bundle::BundleManifest;
use crate::bundle::BundleRegistryIndex;
use crate::ports::{Result, TemplateError};
use hkask_types::storage::{query_map, query_row, DbRow, DbValue, StorageDriver};
use hkask_types::SkillPolarity;
use hkask_types::template_type::TemplateType;
use hkask_types::{InfrastructureError, NotFound, Visibility};
use hkask_types::{
    RegistryEntry, RegistryError, RegistryIndex, Skill, SkillRegistryIndex, SkillZone,
};
use std::sync::Arc;
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

/// Map a `DbError` into a `TemplateError::Database` with context.
fn db_err(ctx: &str, e: hkask_types::DbError) -> TemplateError {
    TemplateError::Database(InfrastructureError::database(format!("{ctx}: {e}")))
}

/// Read an optional text column (NULL → None). Returns `DbError` so it can be
/// used inside `query_map`/`query_row` closures (which are bound to `DbError`).
fn opt_text(row: &DbRow, idx: usize) -> std::result::Result<Option<String>, hkask_types::DbError> {
    match row.get(idx) {
        Ok(DbValue::Null) => Ok(None),
        Ok(DbValue::Text(s)) => Ok(Some(s.clone())),
        Ok(other) => Err(hkask_types::DbError::Database(format!(
            "expected text or null, got {:?}",
            other
        ))),
        Err(e) => Err(e),
    }
}

/// Parse a `DbRow` (templates table order) into a `TemplateRow`.
fn parse_template_row(row: &DbRow) -> std::result::Result<TemplateRow, hkask_types::DbError> {
    let id = row.get_str(0)?.to_string();
    let tt_str = row.get_str(1)?.to_string();
    let tt = TemplateType::parse_str(&tt_str).ok_or_else(|| {
        hkask_types::DbError::Database(format!("Unknown template type: {}", tt_str))
    })?;
    Ok((
        id,
        tt,
        row.get_str(2)?.to_string(),
        row.get_str(3)?.to_string(),
        row.get_str(4)?.to_string(),
        row.get_int(5)? as u32,
        row.get_int(6)? as u32,
    ))
}

/// Query a single text column for a list of strings (lexicon_terms / capabilities).
fn query_column(driver: &dyn StorageDriver, sql: &str, id: &str) -> Result<Vec<String>> {
    let rows = driver
        .query(sql, &[DbValue::Text(id.to_string())])
        .map_err(|e| db_err("Query", e))?;
    rows.iter()
        .map(|r| {
            r.get_str(0)
                .map(String::from)
                .map_err(|e| db_err("Column", e))
        })
        .collect()
}

// ── SqliteRegistry ─────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SqliteRegistry {
    driver: Arc<dyn StorageDriver>,
}

impl SqliteRegistry {
    /// Create a new registry from a `StorageDriver`.
    ///
    /// expect: "The system persists template registrations to SQLite"
    /// \[P3\] Motivating: Generative Space — driver-backed template registry
    /// pre:  driver is a valid `StorageDriver` (provided by `kask_bridge` in
    ///       production, or a test driver in tests)
    /// post: returns SqliteRegistry with schema initialized
    pub fn from_driver(driver: Arc<dyn StorageDriver>) -> Result<Self> {
        Self::init_schema(&driver);
        Ok(Self { driver })
    }

    fn init_schema(driver: &Arc<dyn StorageDriver>) {
        let _ = driver.execute_batch(concat!(
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
        ));
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
        let driver = &self.driver;
        let res: std::result::Result<(), TemplateError> = (|| {
            driver
                .execute_batch("BEGIN")
                .map_err(|e| TemplateError::Manifest(format!("Begin: {}", e)))?;
            driver
                .execute(
                    "INSERT OR REPLACE INTO templates (id, template_type, name, description, source_path, cascade_level, matroshka_limit, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CURRENT_TIMESTAMP)",
                    &[
                        DbValue::Text(entry.id.clone()),
                        DbValue::Text(entry.template_type.as_str().to_string()),
                        DbValue::Text(entry.name.clone()),
                        DbValue::Text(entry.description.clone()),
                        DbValue::Text(entry.source_path.clone()),
                        DbValue::Integer(entry.cascade_level as i64),
                        DbValue::Integer(entry.matroshka_limit as i64),
                    ],
                )
                .map_err(|e| TemplateError::Manifest(format!("Insert: {}", e)))?;
            for (table, col, items) in [
                ("lexicon_terms", "term", &entry.lexicon_terms),
                ("template_capabilities", "capability", &entry.required_capabilities),
            ] {
                driver
                    .execute(
                        &format!("DELETE FROM {} WHERE template_id = ?1", table),
                        &[DbValue::Text(entry.id.clone())],
                    )
                    .map_err(|e| TemplateError::Manifest(format!("Delete {col}: {}", e)))?;
                for item in items {
                    driver
                        .execute(
                            &format!(
                                "INSERT INTO {} (template_id, {}) VALUES (?1, ?2)",
                                table, col
                            ),
                            &[DbValue::Text(entry.id.clone()), DbValue::Text(item.clone())],
                        )
                        .map_err(|e| TemplateError::Manifest(format!("Insert {col}: {}", e)))?;
                }
            }
            driver
                .commit_tx()
                .map_err(|e| TemplateError::Manifest(format!("Commit: {}", e)))?;
            Ok(())
        })();
        if let Err(e) = res {
            let _ = driver.rollback_tx();
            return Err(e);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn row_to_entry(
        driver: &dyn StorageDriver,
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
                driver,
                "SELECT term FROM lexicon_terms WHERE template_id = ?1",
                id,
            )?,
            required_capabilities: query_column(
                driver,
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
        let row = query_row(&*self.driver, Self::_T_SELECT, &[DbValue::Text(id.to_string())], parse_template_row)
            .map_err(|e| db_err("Prepare/Query", e))?
            .ok_or_else(|| TemplateError::NotFound(NotFound {
                entity_type: "template".to_string(),
                id: format!("Template '{}'", id),
            }))?;
        Self::row_to_entry(
            &*self.driver,
            &row.0,
            row.1,
            row.2,
            row.3,
            row.4,
            row.5,
            row.6,
        )
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
        for table in &["lexicon_terms", "template_capabilities", "provenance"] {
            if let Err(e) = self.driver.execute(
                &format!("DELETE FROM {} WHERE template_id = ?1", table),
                &[DbValue::Text(id.to_string())],
            ) {
                tracing::error!(target: "hkask.templates", error = %e, id = %id, table = table, "delete_entry: DELETE failed");
            }
        }
        if let Err(e) = self.driver.execute(
            "DELETE FROM templates WHERE id = ?1",
            &[DbValue::Text(id.to_string())],
        ) {
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
        let rows: Vec<TemplateRow> = query_map(
            &*self.driver,
            "SELECT t.id, t.template_type, t.name, t.description, t.source_path, t.cascade_level, t.matroshka_limit FROM templates t JOIN lexicon_terms l ON t.id = l.template_id WHERE l.term = ?1",
            &[DbValue::Text(term.to_string())],
            parse_template_row,
        )
        .map_err(|e| db_err("Prepare/Query", e))?;
        let mut results = Vec::new();
        for (id, tt, name, desc, sp, cl, ml) in rows {
            results.push(Self::row_to_entry(
                &*self.driver,
                &id,
                tt,
                name,
                desc,
                sp,
                cl,
                ml,
            )?);
        }
        Ok(results)
    }

    /// Count registered templates.
    ///
    /// expect: "The system persists template registrations to SQLite"
    /// \[P3\] Motivating: Generative Space — reports persisted registry size
    /// post: returns count of templates in registry
    /// post: returns 0 on error (graceful degradation)
    pub fn count(&self) -> usize {
        query_row(
            &*self.driver,
            "SELECT COUNT(*) FROM templates",
            &[],
            |row| row.get_int(0),
        )
        .ok()
        .flatten()
        .unwrap_or(0) as usize
    }

    const _T_SELECT: &str = "SELECT id, template_type, name, description, source_path, cascade_level, matroshka_limit FROM templates WHERE id = ?1";
}

// ── RegistryIndex ──────────────────────────────────────────────────────────

impl RegistryIndex for SqliteRegistry {
    fn list(&self, domain_hint: Option<TemplateType>) -> Vec<RegistryEntry> {
        let base_sql = "SELECT id, template_type, name, description, source_path, cascade_level, matroshka_limit FROM templates";
        let (sql, params): (&str, Vec<DbValue>) = match &domain_hint {
            None => (base_sql, vec![]),
            Some(tt) => (
                &format!("{base_sql} WHERE template_type = ?1"),
                vec![DbValue::Text(tt.as_str().to_string())],
            ),
        };
        let rows: Vec<TemplateRow> = match query_map(&*self.driver, sql, &params, parse_template_row) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        let mut results = Vec::new();
        for (id, tt, name, desc, sp, cl, ml) in rows {
            if let Ok(entry) = Self::row_to_entry(&*self.driver, &id, tt, name, desc, sp, cl, ml) {
                results.push(entry);
            }
        }
        results
    }

    fn get(&self, id: &str) -> std::result::Result<RegistryEntry, RegistryError> {
        self.get_entry(id).map_err(|e| {
            RegistryError::NotFound(NotFound {
                entity_type: "template".to_string(),
                id: format!("Template '{}': {}", id, e),
            })
        })
    }
}

// ── SkillRegistryIndex ─────────────────────────────────────────────────────

impl SkillRegistryIndex for SqliteRegistry {
    fn register_skill(&mut self, skill: Skill) -> std::result::Result<(), RegistryError> {
        if let Err(e) = self.driver.execute(
            "INSERT OR REPLACE INTO skills (id, domain, word_act, flow_def, know_act, polarity, content_hash, visibility, zone, namespace) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            &[
                DbValue::Text(skill.id.clone()),
                DbValue::Text(skill.domain.as_str().to_string()),
                skill.word_act.clone().map(DbValue::Text).unwrap_or(DbValue::Null),
                skill.flow_def.clone().map(DbValue::Text).unwrap_or(DbValue::Null),
                skill.know_act.clone().map(DbValue::Text).unwrap_or(DbValue::Null),
                skill.polarity.as_ref().map(|p| DbValue::Text(p.as_str().to_string())).unwrap_or(DbValue::Null),
                skill.content_hash.clone().map(DbValue::Text).unwrap_or(DbValue::Null),
                DbValue::Text(skill.visibility.as_str().to_string()),
                DbValue::Text(skill.zone.as_str().to_string()),
                skill.namespace.clone().map(DbValue::Text).unwrap_or(DbValue::Null),
            ],
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
        if let Err(e) = self.driver.execute(
            "DELETE FROM skills WHERE id = ?1",
            &[DbValue::Text(id.to_string())],
        ) {
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
        if let Err(e) = self.driver.execute(
            "INSERT OR REPLACE INTO bundles (id, name, description, version, editor, visibility, manifest_json, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CURRENT_TIMESTAMP)",
            &[
                DbValue::Text(bundle.id.clone()),
                DbValue::Text(bundle.name.clone()),
                DbValue::Text(bundle.description.clone()),
                DbValue::Text(bundle.version.clone()),
                DbValue::Text(bundle.editor.clone()),
                DbValue::Text(bundle.visibility.as_str().to_string()),
                DbValue::Text(manifest_json),
            ],
        ) {
            tracing::error!(target: "hkask.templates", error = %e, bundle_id = %bundle.id, "register_bundle: INSERT failed");
            return Ok(());
        }
        if let Err(e) = self.driver.execute(
            "DELETE FROM bundle_skills WHERE bundle_id = ?1",
            &[DbValue::Text(bundle.id.clone())],
        ) {
            tracing::error!(target: "hkask.templates", error = %e, bundle_id = %bundle.id, "register_bundle: DELETE bundle_skills failed");
            return Ok(());
        }
        for (position, skill) in bundle.skills.iter().enumerate() {
            if let Err(e) = self.driver.execute(
                "INSERT INTO bundle_skills (bundle_id, skill_id, polarity, manifest_ref, content_hash, position) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                &[
                    DbValue::Text(bundle.id.clone()),
                    DbValue::Text(skill.id.clone()),
                    DbValue::Text(skill.polarity.as_str().to_string()),
                    DbValue::Text(skill.manifest_ref.clone()),
                    DbValue::Text(skill.content_hash.clone()),
                    DbValue::Integer(position as i64),
                ],
            ) {
                tracing::error!(target: "hkask.templates", error = %e, bundle_id = %bundle.id, skill_id = %skill.id, "register_bundle: INSERT bundle_skills failed");
            }
        }
        Ok(())
    }

    fn get_bundle(&self, id: &str) -> Option<BundleManifest> {
        query_row(
            &*self.driver,
            "SELECT manifest_json FROM bundles WHERE id = ?1",
            &[DbValue::Text(id.to_string())],
            |row| row.get_str(0).map(String::from),
        )
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
    }

    fn list_bundles(&self) -> Vec<BundleManifest> {
        query_map(
            &*self.driver,
            "SELECT manifest_json FROM bundles",
            &[],
            |row| row.get_str(0).map(String::from),
        )
        .map(|rows| {
            rows.into_iter()
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
        if let Err(e) = self.driver.execute(
            "DELETE FROM bundle_skills WHERE bundle_id = ?1",
            &[DbValue::Text(id.to_string())],
        ) {
            tracing::error!(target: "hkask.templates", error = %e, id = %id, "remove_bundle: DELETE bundle_skills failed");
        }
        if let Err(e) = self.driver.execute(
            "DELETE FROM bundles WHERE id = ?1",
            &[DbValue::Text(id.to_string())],
        ) {
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
        let row = query_row(
            &*self.driver,
            "SELECT id, domain, word_act, flow_def, know_act, polarity, content_hash, visibility, zone, namespace FROM skills WHERE id = ?1",
            &[DbValue::Text(id.to_string())],
            |row| {
                Ok((
                    row.get_str(0)?.to_string(),
                    row.get_str(1)?.to_string(),
                    opt_text(row, 2)?,
                    opt_text(row, 3)?,
                    opt_text(row, 4)?,
                    opt_text(row, 5)?,
                    opt_text(row, 6)?,
                    row.get_str(7)?.to_string(),
                    row.get_str(8)?.to_string(),
                    opt_text(row, 9)?,
                ))
            },
        )
        .ok()
        .and_then(std::convert::identity)?;
        let (id, ds, wa, fd, ka, ps, ch, vs, zs, ns) = row;
        Self::row_to_skill(id, ds, wa, fd, ka, ps, ch, vs, zs, ns)
    }

    fn query_skills(&self, sql: &str, params: &[DbValue]) -> Vec<Skill> {
        let rows: Vec<SkillRow> = match query_map(&*self.driver, sql, params, |row| {
            Ok((
                row.get_str(0)?.to_string(),
                row.get_str(1)?.to_string(),
                opt_text(row, 2)?,
                opt_text(row, 3)?,
                opt_text(row, 4)?,
                opt_text(row, 5)?,
                opt_text(row, 6)?,
                row.get_str(7)?.to_string(),
                row.get_str(8)?.to_string(),
                opt_text(row, 9)?,
            ))
        }) {
            Ok(m) => m,
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
            &[DbValue::Text(domain.as_str().to_string())],
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
            &[DbValue::Text(tid.to_string())],
        )
    }
}
