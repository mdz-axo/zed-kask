use sea_orm::entity::prelude::*;

/// zed-kask: D30 — Local-only fallback blob store for kask skill tarballs.
///
/// Used when `AppState.blob_store_client` is `None` (local dev / self-hosted
/// without S3; e.g. `cargo run -p collab serve all` with only `DATABASE_URL` /
/// `HTTP_PORT` / `ZED_ENVIRONMENT` set). The uploaded `archive.tar.gz` bytes
/// are stored inline so the upload/download handlers work end-to-end without
/// a blob store. Production deployments with S3 configured never read or write
/// this table — the handlers take the S3 branch and the periodic poll
/// reconciles from S3.
///
/// Keyed by the publish triple `(source_user, skill_name, version)`, which
/// matches the S3 key layout
/// `kask-skills/{source_user}/{skill_name}/{version}/archive.tar.gz`.
///
/// The signed-manifest verification gate (`verify_manifest_signature` + the
/// 120-day window in `api/kask_skills.rs`) is unchanged by this table: the
/// local path stores the same bytes the client already hashes+signs, and the
/// catalog row still carries `public_key`/`signature`/`expires_at`, so
/// `install_skill`'s SHA256 + signature checks are intact.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "kask_skill_tarballs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub source_user: String,
    #[sea_orm(primary_key)]
    pub skill_name: String,
    #[sea_orm(primary_key)]
    pub version: String,
    /// The raw `archive.tar.gz` bytes uploaded by the publisher.
    pub tarball: Vec<u8>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
