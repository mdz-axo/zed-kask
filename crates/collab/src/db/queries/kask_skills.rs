use anyhow::Context;
use cloud_api_types::{KaskSkillManifest, KaskSkillMetadata};
use collections::HashMap;
use sea_orm::sea_query::{Expr, Index, IntoCondition, Table};

use super::*;
use crate::Error;
use crate::db::queries::extensions::convert_time_to_chrono;
use crate::db::{KaskSkillId, UserId};

/// A new kask skill version discovered in the blob store by the periodic poll.
pub struct NewKaskSkillVersion {
    pub source_user: String,
    pub skill_name: String,
    pub version: String,
    pub description: String,
    pub dependencies: Vec<String>,
    pub tarball_sha256: String,
    pub public_key: String,
    pub signature: String,
    pub expires_at: String,
    pub published_at: time::PrimitiveDateTime,
}

impl Database {
    /// zed-kask: Builds the CREATE TABLE IF NOT EXISTS statements for the
    /// kask skill marketplace. Pure statement construction, extracted for
    /// testability (a live-Database test requires the test-support harness).
    fn kask_skill_table_statements() -> (
        Vec<sea_orm::sea_query::TableCreateStatement>,
        Vec<sea_orm::sea_query::IndexCreateStatement>,
    ) {
        let tables = vec![
            Table::create()
                .table(kask_skill::Entity)
                .if_not_exists()
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill::Column::Id)
                        .integer()
                        .not_null()
                        .auto_increment()
                        .primary_key(),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill::Column::SourceUser)
                        .text()
                        .not_null(),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill::Column::SkillName)
                        .text()
                        .not_null(),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill::Column::Description)
                        .text()
                        .not_null(),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill::Column::LatestVersion)
                        .text()
                        .not_null(),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill::Column::TotalDownloadCount)
                        .big_integer()
                        .not_null()
                        .default(0),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill::Column::UpvoteCount)
                        .big_integer()
                        .not_null()
                        .default(0),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill::Column::DownvoteCount)
                        .big_integer()
                        .not_null()
                        .default(0),
                )
                .to_owned(),
            Table::create()
                .table(kask_skill_version::Entity)
                .if_not_exists()
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill_version::Column::KaskSkillId)
                        .integer()
                        .not_null(),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill_version::Column::Version)
                        .text()
                        .not_null(),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill_version::Column::PublishedAt)
                        .timestamp()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill_version::Column::Dependencies)
                        .text()
                        .not_null()
                        .default(""),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill_version::Column::TarballSha256)
                        .text()
                        .not_null(),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill_version::Column::PublicKey)
                        .text()
                        .not_null(),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill_version::Column::Signature)
                        .text()
                        .not_null(),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill_version::Column::ExpiresAt)
                        .text()
                        .not_null(),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill_version::Column::DownloadCount)
                        .big_integer()
                        .not_null()
                        .default(0),
                )
                .primary_key(
                    Index::create()
                        .col(kask_skill_version::Column::KaskSkillId)
                        .col(kask_skill_version::Column::Version),
                )
                .foreign_key(
                    sea_orm::sea_query::ForeignKey::create()
                        .from(
                            kask_skill_version::Entity,
                            kask_skill_version::Column::KaskSkillId,
                        )
                        .to(kask_skill::Entity, kask_skill::Column::Id)
                        .on_delete(sea_orm::sea_query::ForeignKeyAction::Cascade),
                )
                .to_owned(),
            Table::create()
                .table(kask_skill_vote::Entity)
                .if_not_exists()
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill_vote::Column::KaskSkillId)
                        .integer()
                        .not_null(),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill_vote::Column::UserId)
                        .integer()
                        .not_null(),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill_vote::Column::Vote)
                        .small_integer()
                        .not_null(),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill_vote::Column::VotedAt)
                        .timestamp()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .primary_key(
                    Index::create()
                        .col(kask_skill_vote::Column::KaskSkillId)
                        .col(kask_skill_vote::Column::UserId),
                )
                .foreign_key(
                    sea_orm::sea_query::ForeignKey::create()
                        .from(
                            kask_skill_vote::Entity,
                            kask_skill_vote::Column::KaskSkillId,
                        )
                        .to(kask_skill::Entity, kask_skill::Column::Id)
                        .on_delete(sea_orm::sea_query::ForeignKeyAction::Cascade),
                )
                .to_owned(),
            // zed-kask: D30 — local fallback blob store for kask skill tarballs.
            // Used when `AppState.blob_store_client` is `None` (local dev /
            // self-hosted without S3). Production with S3 never touches this
            // table — the upload/download/delete handlers take the S3 branch
            // and the periodic poll reconciles from S3. Composite primary key
            // on the publish triple matches the S3 key layout
            // `kask-skills/{source_user}/{skill_name}/{version}/archive.tar.gz`.
            Table::create()
                .table(kask_skill_tarball::Entity)
                .if_not_exists()
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill_tarball::Column::SourceUser)
                        .text()
                        .not_null(),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill_tarball::Column::SkillName)
                        .text()
                        .not_null(),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill_tarball::Column::Version)
                        .text()
                        .not_null(),
                )
                .col(
                    sea_orm::sea_query::ColumnDef::new(kask_skill_tarball::Column::Tarball)
                        .binary()
                        .not_null(),
                )
                .primary_key(
                    Index::create()
                        .col(kask_skill_tarball::Column::SourceUser)
                        .col(kask_skill_tarball::Column::SkillName)
                        .col(kask_skill_tarball::Column::Version),
                )
                .to_owned(),
        ];

        let indexes = vec![
            Index::create()
                .name("index_kask_skills_source_user_skill_name")
                .table(kask_skill::Entity)
                .col(kask_skill::Column::SourceUser)
                .col(kask_skill::Column::SkillName)
                .unique()
                .if_not_exists()
                .to_owned(),
            Index::create()
                .name("index_kask_skills_total_download_count")
                .table(kask_skill::Entity)
                .col(kask_skill::Column::TotalDownloadCount)
                .if_not_exists()
                .to_owned(),
        ];

        (tables, indexes)
    }

    /// zed-kask: Creates the kask skill marketplace tables/indexes if they
    /// do not exist. Idempotent boot-time self-heal for self-hosted
    /// deployments — upstream Zed applies schema out-of-band, which leaves
    /// a fresh self-hosted collab server 500ing on `/api/kask-skills` with
    /// no signal. Mirrors the `initialize_notification_kinds` pattern:
    /// called from `setup_app_database` at server startup (any serve mode,
    /// so a split collab+API deployment sharing one database self-heals
    /// regardless of which process starts first).
    pub async fn ensure_kask_skill_tables(&self) -> Result<()> {
        let backend = self.pool.get_database_backend();
        let (tables, indexes) = Self::kask_skill_table_statements();
        for statement in &tables {
            self.pool.execute(backend.build(statement)).await?;
        }
        for statement in &indexes {
            self.pool.execute(backend.build(statement)).await?;
        }
        Ok(())
    }

    /// Returns all kask skills (latest version of each), ordered by download count.
    pub async fn get_kask_skills(&self) -> Result<Vec<KaskSkillMetadata>> {
        self.transaction(|tx| async move { self.get_kask_skills_where(None, None, &tx).await })
            .await
    }

    /// Returns a single kask skill by its `"{source_user}/{skill_name}"` id.
    pub async fn get_kask_skill(&self, id: &str) -> Result<Option<KaskSkillMetadata>> {
        let (source_user, skill_name) = id
            .split_once('/')
            .context("kask skill id must be \"{source_user}/{skill_name}\"")?;
        self.transaction(|tx| async move {
            let condition = kask_skill::Column::SourceUser
                .eq(source_user)
                .and(kask_skill::Column::SkillName.eq(skill_name))
                .into_condition();
            let mut results = self
                .get_kask_skills_where(Some(condition), Some(1), &tx)
                .await?;
            Ok(results.pop())
        })
        .await
    }

    async fn get_kask_skills_where(
        &self,
        condition: Option<Condition>,
        limit: Option<u64>,
        tx: &DatabaseTransaction,
    ) -> Result<Vec<KaskSkillMetadata>> {
        let mut query = kask_skill::Entity::find()
            .inner_join(kask_skill_version::Entity)
            .select_also(kask_skill_version::Entity)
            .filter(
                kask_skill::Column::LatestVersion
                    .into_expr()
                    .eq(kask_skill_version::Column::Version.into_expr()),
            )
            .order_by_desc(kask_skill::Column::TotalDownloadCount)
            .order_by_asc(kask_skill::Column::SkillName);
        if let Some(condition) = condition {
            query = query.filter(condition);
        }
        if let Some(limit) = limit {
            query = query.limit(limit);
        }
        let rows = query.all(tx).await?;

        // zed-kask: expiry enforcement (plan Phase 3 / D2). A skill version is
        // listed only while its signed `expires_at` is in the future. The
        // column is TEXT (RFC 3339), so the comparison happens here in Rust
        // rather than in SQL (lexicographic ordering is not reliable for
        // variable-precision timestamps). The sweep deletes the underlying
        // rows; this filter is the enforcement point that keeps expired
        // skills out of the catalog between sweep runs.
        let now = time::OffsetDateTime::now_utc();
        Ok(rows
            .into_iter()
            .filter_map(|(skill, version)| {
                let version = version?;
                let expires_at = time::OffsetDateTime::parse(
                    &version.expires_at,
                    &time::format_description::well_known::Rfc3339,
                )
                .ok()?;
                if expires_at <= now {
                    return None;
                }
                Some(metadata_from_skill_and_version(skill, version))
            })
            .collect())
    }

    /// Returns the set of (source_user, skill_name, version) tuples already
    /// known to Postgres, so the periodic poll can skip them.
    pub async fn get_known_kask_skill_versions(&self) -> Result<HashMap<String, Vec<String>>> {
        self.transaction(|tx| async move {
            let mut skill_ids_by_row = HashMap::default();
            let mut rows = kask_skill::Entity::find().stream(&*tx).await?;
            while let Some(row) = rows.next().await {
                let row = row?;
                let id = format!("{}/{}", row.source_user, row.skill_name);
                skill_ids_by_row.insert(row.id, id);
            }
            drop(rows);

            let mut known: HashMap<String, Vec<String>> = HashMap::default();
            let mut rows = kask_skill_version::Entity::find().stream(&*tx).await?;
            while let Some(row) = rows.next().await {
                let row = row?;
                let Some(id) = skill_ids_by_row.get(&row.kask_skill_id) else {
                    continue;
                };
                let versions = known.entry(id.clone()).or_default();
                if let Err(ix) = versions.binary_search(&row.version) {
                    versions.insert(ix, row.version);
                }
            }
            Ok(known)
        })
        .await
    }

    /// Upserts kask skill versions discovered by the periodic poll. Mirrors
    /// `insert_extension_versions` — inserts the skill row if new, updates
    /// `latest_version` if the new version is newer, inserts the version row.
    pub async fn insert_kask_skill_versions(&self, versions: &[NewKaskSkillVersion]) -> Result<()> {
        self.transaction(|tx| async move {
            for new_version in versions {
                let insert = kask_skill::Entity::insert(kask_skill::ActiveModel {
                    id: ActiveValue::NotSet,
                    source_user: ActiveValue::Set(new_version.source_user.clone()),
                    skill_name: ActiveValue::Set(new_version.skill_name.clone()),
                    description: ActiveValue::Set(new_version.description.clone()),
                    latest_version: ActiveValue::Set(new_version.version.clone()),
                    total_download_count: ActiveValue::NotSet,
                    upvote_count: ActiveValue::NotSet,
                    downvote_count: ActiveValue::NotSet,
                })
                .on_conflict(
                    OnConflict::columns([
                        kask_skill::Column::SourceUser,
                        kask_skill::Column::SkillName,
                    ])
                    .update_columns([
                        kask_skill::Column::Description,
                        kask_skill::Column::LatestVersion,
                    ])
                    .to_owned(),
                );

                let skill = if tx.support_returning() {
                    insert.exec_with_returning(&*tx).await?
                } else {
                    insert.exec_without_returning(&*tx).await?;
                    kask_skill::Entity::find()
                        .filter(
                            kask_skill::Column::SourceUser
                                .eq(new_version.source_user.as_str())
                                .and(
                                    kask_skill::Column::SkillName
                                        .eq(new_version.skill_name.as_str()),
                                ),
                        )
                        .one(&*tx)
                        .await?
                        .context("failed to insert kask skill")?
                };

                kask_skill_version::Entity::insert(kask_skill_version::ActiveModel {
                    kask_skill_id: ActiveValue::Set(skill.id),
                    version: ActiveValue::Set(new_version.version.clone()),
                    published_at: ActiveValue::Set(new_version.published_at),
                    dependencies: ActiveValue::Set(new_version.dependencies.join(",")),
                    tarball_sha256: ActiveValue::Set(new_version.tarball_sha256.clone()),
                    public_key: ActiveValue::Set(new_version.public_key.clone()),
                    signature: ActiveValue::Set(new_version.signature.clone()),
                    expires_at: ActiveValue::Set(new_version.expires_at.clone()),
                    download_count: ActiveValue::NotSet,
                })
                .on_conflict(
                    OnConflict::columns([
                        kask_skill_version::Column::KaskSkillId,
                        kask_skill_version::Column::Version,
                    ])
                    .update_columns([
                        kask_skill_version::Column::Dependencies,
                        kask_skill_version::Column::TarballSha256,
                        kask_skill_version::Column::PublicKey,
                        kask_skill_version::Column::Signature,
                        kask_skill_version::Column::ExpiresAt,
                    ])
                    .to_owned(),
                )
                .exec_without_returning(&*tx)
                .await?;
            }
            Ok(())
        })
        .await
    }

    /// Increments the download count for a kask skill version. Returns `false`
    /// if the skill or version doesn't exist.
    pub async fn record_kask_skill_download(
        &self,
        source_user: &str,
        skill_name: &str,
        version: &str,
    ) -> Result<bool> {
        self.transaction(|tx| async move {
            let skill_id: Option<KaskSkillId> = kask_skill::Entity::find()
                .filter(
                    kask_skill::Column::SourceUser
                        .eq(source_user)
                        .and(kask_skill::Column::SkillName.eq(skill_name)),
                )
                .select_only()
                .column(kask_skill::Column::Id)
                .into_tuple()
                .one(&*tx)
                .await?;
            let Some(skill_id) = skill_id else {
                return Ok(false);
            };

            let version_exists: Option<String> = kask_skill_version::Entity::find()
                .filter(
                    kask_skill_version::Column::KaskSkillId
                        .eq(skill_id)
                        .and(kask_skill_version::Column::Version.eq(version)),
                )
                .select_only()
                .column(kask_skill_version::Column::Version)
                .into_tuple()
                .one(&*tx)
                .await?;
            if version_exists.is_none() {
                return Ok(false);
            }

            kask_skill_version::Entity::update_many()
                .col_expr(
                    kask_skill_version::Column::DownloadCount,
                    kask_skill_version::Column::DownloadCount.into_expr().add(1),
                )
                .filter(
                    kask_skill_version::Column::KaskSkillId
                        .eq(skill_id)
                        .and(kask_skill_version::Column::Version.eq(version)),
                )
                .exec(&*tx)
                .await?;

            kask_skill::Entity::update_many()
                .col_expr(
                    kask_skill::Column::TotalDownloadCount,
                    kask_skill::Column::TotalDownloadCount.into_expr().add(1),
                )
                .filter(kask_skill::Column::Id.eq(skill_id))
                .exec(&*tx)
                .await?;

            Ok(true)
        })
        .await
    }

    /// Casts or updates a user's vote on a kask skill. Returns the new
    /// aggregate `(upvote_count, downvote_count)` for the skill.
    pub async fn vote_kask_skill(
        &self,
        source_user: &str,
        skill_name: &str,
        user_id: UserId,
        vote: i16,
    ) -> Result<(i64, i64)> {
        self.transaction(|tx| async move {
            let skill_id: Option<KaskSkillId> = kask_skill::Entity::find()
                .filter(
                    kask_skill::Column::SourceUser
                        .eq(source_user)
                        .and(kask_skill::Column::SkillName.eq(skill_name)),
                )
                .select_only()
                .column(kask_skill::Column::Id)
                .into_tuple()
                .one(&*tx)
                .await?;
            let Some(skill_id) = skill_id else {
                return Err(Error::Internal(anyhow::anyhow!("kask skill not found")));
            };

            // Upsert the vote row.
            let existing = kask_skill_vote::Entity::find()
                .filter(
                    kask_skill_vote::Column::KaskSkillId
                        .eq(skill_id)
                        .and(kask_skill_vote::Column::UserId.eq(user_id)),
                )
                .one(&*tx)
                .await?;

            let old_vote: i16 = existing.as_ref().map(|v| v.vote).unwrap_or(0);
            let delta = vote - old_vote;

            if existing.is_some() {
                kask_skill_vote::Entity::update_many()
                    .col_expr(
                        kask_skill_vote::Column::Vote,
                        sea_orm::sea_query::Expr::value(vote),
                    )
                    .col_expr(
                        kask_skill_vote::Column::VotedAt,
                        Expr::current_time().into(),
                    )
                    .filter(
                        kask_skill_vote::Column::KaskSkillId
                            .eq(skill_id)
                            .and(kask_skill_vote::Column::UserId.eq(user_id)),
                    )
                    .exec(&*tx)
                    .await?;
            } else {
                kask_skill_vote::Entity::insert(kask_skill_vote::ActiveModel {
                    kask_skill_id: ActiveValue::Set(skill_id),
                    user_id: ActiveValue::Set(user_id),
                    vote: ActiveValue::Set(vote),
                    voted_at: ActiveValue::NotSet,
                })
                .exec_without_returning(&*tx)
                .await?;
            }

            // Update aggregate counts on the skill row.
            if delta != 0 {
                if old_vote > 0 {
                    kask_skill::Entity::update_many()
                        .col_expr(
                            kask_skill::Column::UpvoteCount,
                            kask_skill::Column::UpvoteCount.into_expr().sub(old_vote),
                        )
                        .filter(kask_skill::Column::Id.eq(skill_id))
                        .exec(&*tx)
                        .await?;
                } else if old_vote < 0 {
                    kask_skill::Entity::update_many()
                        .col_expr(
                            kask_skill::Column::DownvoteCount,
                            kask_skill::Column::DownvoteCount.into_expr().sub(-old_vote),
                        )
                        .filter(kask_skill::Column::Id.eq(skill_id))
                        .exec(&*tx)
                        .await?;
                }
                if vote > 0 {
                    kask_skill::Entity::update_many()
                        .col_expr(
                            kask_skill::Column::UpvoteCount,
                            kask_skill::Column::UpvoteCount.into_expr().add(vote),
                        )
                        .filter(kask_skill::Column::Id.eq(skill_id))
                        .exec(&*tx)
                        .await?;
                } else if vote < 0 {
                    kask_skill::Entity::update_many()
                        .col_expr(
                            kask_skill::Column::DownvoteCount,
                            kask_skill::Column::DownvoteCount.into_expr().add(-vote),
                        )
                        .filter(kask_skill::Column::Id.eq(skill_id))
                        .exec(&*tx)
                        .await?;
                }
            }

            let skill = kask_skill::Entity::find_by_id(skill_id)
                .one(&*tx)
                .await?
                .context("kask skill disappeared during vote")?;
            Ok((skill.upvote_count, skill.downvote_count))
        })
        .await
    }

    /// zed-kask: Delete a kask skill and all its versions/votes from Postgres.
    /// Called when a user unpublishes their skill. The ON DELETE CASCADE
    /// on the versions and votes tables handles the cleanup.
    pub async fn delete_kask_skill(&self, source_user: &str, skill_name: &str) -> Result<bool> {
        self.transaction(|tx| async move {
            let result = kask_skill::Entity::delete_many()
                .filter(
                    kask_skill::Column::SourceUser
                        .eq(source_user)
                        .and(kask_skill::Column::SkillName.eq(skill_name)),
                )
                .exec(&*tx)
                .await?;
            Ok(result.rows_affected > 0)
        })
        .await
    }

    // zed-kask: D30 — local fallback blob store for kask skill tarballs. See
    // `tables/kask_skill_tarball.rs` for the rationale (no-S3 publish path).
    // These three methods are the DB side of the local upload/download/delete
    // branches in `api/kask_skills.rs`; they are only reached when
    // `AppState.blob_store_client` is `None`.

    /// Store a kask skill tarball in the local fallback blob store. Upsert on
    /// the `(source_user, skill_name, version)` primary key so a re-upload of
    /// the same version replaces the bytes.
    pub async fn put_kask_skill_tarball(
        &self,
        source_user: &str,
        skill_name: &str,
        version: &str,
        tarball: Vec<u8>,
    ) -> Result<()> {
        use kask_skill_tarball::{ActiveModel, Column};
        let source_user = source_user.to_string();
        let skill_name = skill_name.to_string();
        let version = version.to_string();
        self.transaction(|tx| {
            let source_user = source_user.clone();
            let skill_name = skill_name.clone();
            let version = version.clone();
            let tarball = tarball.clone();
            async move {
                let active = ActiveModel {
                    source_user: sea_orm::ActiveValue::Set(source_user),
                    skill_name: sea_orm::ActiveValue::Set(skill_name),
                    version: sea_orm::ActiveValue::Set(version),
                    tarball: sea_orm::ActiveValue::Set(tarball),
                };
                kask_skill_tarball::Entity::insert(active)
                    .on_conflict(
                        sea_orm::sea_query::OnConflict::columns([
                            Column::SourceUser,
                            Column::SkillName,
                            Column::Version,
                        ])
                        .update_column(kask_skill_tarball::Column::Tarball)
                        .to_owned(),
                    )
                    .exec_without_returning(&*tx)
                    .await?;
                Ok::<_, Error>(())
            }
        })
        .await
    }

    /// Fetch a kask skill tarball from the local fallback blob store.
    /// Returns `None` when no tarball is stored for the triple (the upload
    /// never happened, or was deleted).
    pub async fn get_kask_skill_tarball(
        &self,
        source_user: &str,
        skill_name: &str,
        version: &str,
    ) -> Result<Option<Vec<u8>>> {
        use kask_skill_tarball::Column;
        self.transaction(|tx| async move {
            let row = kask_skill_tarball::Entity::find()
                .filter(
                    Column::SourceUser
                        .eq(source_user)
                        .and(Column::SkillName.eq(skill_name))
                        .and(Column::Version.eq(version)),
                )
                .one(&*tx)
                .await?;
            Ok::<_, Error>(row.map(|model| model.tarball))
        })
        .await
    }

    /// Delete all locally-stored tarballs for a `(source_user, skill_name)`
    /// publish namespace. Returns the number of deleted rows.
    pub async fn delete_kask_skill_tarballs(
        &self,
        source_user: &str,
        skill_name: &str,
    ) -> Result<u64> {
        use kask_skill_tarball::Column;
        self.transaction(|tx| async move {
            let result = kask_skill_tarball::Entity::delete_many()
                .filter(
                    Column::SourceUser
                        .eq(source_user)
                        .and(Column::SkillName.eq(skill_name)),
                )
                .exec(&*tx)
                .await?;
            Ok::<_, Error>(result.rows_affected)
        })
        .await
    }

    /// zed-kask: Purge expired kask skill versions and the skills orphaned by
    /// them (plan Phase 3 / D2).
    ///
    /// A version's `expires_at` (TEXT, RFC 3339) is compared here in Rust —
    /// lexicographic SQL comparison is unreliable for variable-precision
    /// timestamps. Versions whose `expires_at` has passed are deleted; skill
    /// rows with no remaining versions are deleted too (the catalog filter in
    /// `get_kask_skills_where` is the enforcement point between sweeps; this
    /// is the cleanup that removes the dead rows).
    ///
    /// Returns the number of expired versions purged. The caller logs a
    /// signal (`.rules` "signal, not silence" trap) — a sweep finding expired
    /// skills is an operator-visible event, not a silent no-op.
    pub async fn purge_expired_kask_skill_versions(&self) -> Result<usize> {
        self.transaction(|tx| async move {
            let now = time::OffsetDateTime::now_utc();
            // zed-kask: join versions → skills so we capture the
            // `(source_user, skill_name, version)` triple for each expired
            // version. The tarball fallback table (D30) is keyed by that
            // triple, not by `kask_skill_id`, so we need the namespace to
            // clean up orphaned tarball rows (the `kask_skill_tarballs`
            // table has no FK to `kask_skills`).
            let versions = kask_skill_version::Entity::find()
                .inner_join(kask_skill::Entity)
                .select_also(kask_skill::Entity)
                .all(&*tx)
                .await?;

            let mut expired_versions = Vec::new();
            for (version, skill) in &versions {
                let Some(skill) = skill else { continue };
                let expires_at = time::OffsetDateTime::parse(
                    &version.expires_at,
                    &time::format_description::well_known::Rfc3339,
                );
                if expires_at.map(|t| t <= now).unwrap_or(true) {
                    // Unparseable `expires_at` counts as expired: it can never
                    // satisfy the catalog filter, and keeping the row forever
                    // would let dead skills accumulate (plan D5 fail-closed).
                    expired_versions.push((
                        version.kask_skill_id,
                        version.version.clone(),
                        skill.source_user.clone(),
                        skill.skill_name.clone(),
                    ));
                }
            }

            for (skill_id, version, source_user, skill_name) in &expired_versions {
                kask_skill_version::Entity::delete_many()
                    .filter(
                        kask_skill_version::Column::KaskSkillId
                            .eq(*skill_id)
                            .and(kask_skill_version::Column::Version.eq(version)),
                    )
                    .exec(&*tx)
                    .await?;
                // zed-kask: D30 — also delete the locally-stored tarball for
                // this expired version so the local fallback store doesn't
                // leak orphaned rows (the table has no FK cascade). No-op in
                // production (S3 path); the local store is dev/self-hosted only.
                kask_skill_tarball::Entity::delete_many()
                    .filter(
                        kask_skill_tarball::Column::SourceUser
                            .eq(source_user)
                            .and(kask_skill_tarball::Column::SkillName.eq(skill_name))
                            .and(kask_skill_tarball::Column::Version.eq(version)),
                    )
                    .exec(&*tx)
                    .await?;
            }

            // Orphaned skills: no remaining versions (the expired one was the
            // only/latest one). ON DELETE CASCADE on versions/votes already
            // cleaned those up; this removes the skill row itself.
            if !expired_versions.is_empty() {
                let remaining: Vec<KaskSkillId> = kask_skill_version::Entity::find()
                    .select_only()
                    .column(kask_skill_version::Column::KaskSkillId)
                    .distinct()
                    .into_tuple()
                    .all(&*tx)
                    .await?;
                if remaining.is_empty() {
                    kask_skill::Entity::delete_many().exec(&*tx).await?;
                } else {
                    kask_skill::Entity::delete_many()
                        .filter(kask_skill::Column::Id.is_not_in(remaining.iter().map(|id| *id)))
                        .exec(&*tx)
                        .await?;
                }
            }

            Ok(expired_versions.len())
        })
        .await
    }
}

fn metadata_from_skill_and_version(
    skill: kask_skill::Model,
    version: kask_skill_version::Model,
) -> KaskSkillMetadata {
    KaskSkillMetadata {
        id: format!("{}/{}", skill.source_user, skill.skill_name).into(),
        manifest: KaskSkillManifest {
            source_user: skill.source_user,
            skill_name: skill.skill_name,
            version: version.version,
            description: skill.description,
            dependencies: if version.dependencies.is_empty() {
                Vec::new()
            } else {
                version
                    .dependencies
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect()
            },
            tarball_sha256: version.tarball_sha256,
            public_key: version.public_key,
            signature: version.signature,
            expires_at: version.expires_at,
        },
        published_at: convert_time_to_chrono(version.published_at),
        download_count: skill.total_download_count as u64,
        upvote_count: skill.upvote_count,
        downvote_count: skill.downvote_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::sea_query::{PostgresQueryBuilder, SqliteQueryBuilder};

    /// zed-kask: pin the self-heal schema — all statements must be
    /// idempotent (IF NOT EXISTS) and cover the three marketplace tables.
    /// A live-Database test requires the test-support harness; rendering
    /// the statements pins the contract without one.
    #[test]
    fn kask_skill_table_statements_are_idempotent() {
        let (tables, indexes) = Database::kask_skill_table_statements();

        let table_names: Vec<String> = tables
            .iter()
            .map(|t| {
                let sql = t.to_string(PostgresQueryBuilder);
                assert!(
                    sql.contains("IF NOT EXISTS"),
                    "CREATE TABLE must be idempotent: {sql}"
                );
                sql
            })
            .collect();
        assert!(table_names.iter().any(|s| s.contains("\"kask_skills\"")));
        assert!(
            table_names
                .iter()
                .any(|s| s.contains("\"kask_skill_versions\""))
        );
        assert!(
            table_names
                .iter()
                .any(|s| s.contains("\"kask_skill_votes\""))
        );

        // SQLite must render too (collab supports it via the `sqlite` feature).
        for t in &tables {
            t.to_string(SqliteQueryBuilder);
        }

        let index_sql: Vec<String> = indexes
            .iter()
            .map(|i| i.to_string(PostgresQueryBuilder))
            .collect();
        assert!(
            index_sql.iter().all(|s| s.contains("IF NOT EXISTS")),
            "CREATE INDEX must be idempotent: {index_sql:?}"
        );
        assert!(index_sql.iter().any(
            |s| s.contains("index_kask_skills_source_user_skill_name") && s.contains("UNIQUE")
        ));
    }
}
