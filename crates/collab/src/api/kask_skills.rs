use crate::rpc::Principal;
use crate::{AppState, Error, Result};
use anyhow::Context as _;
use aws_sdk_s3::presigning::PresigningConfig;
use axum::{
    Extension, Json, Router,
    extract::{Path, Query},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use cloud_api_types::{
    GetKaskSkillsResponse, KaskSkillManifest, KaskSkillMetadata, KaskSkillVoteRequest,
};
use ed25519_dalek::{Signature, VerifyingKey};
use std::sync::Arc;
use std::time::Duration;
use util::ResultExt;

/// Maximum accepted lifetime of a signed kask skill manifest, in days.
///
/// Mirrors `hkask_keystore::KEY_MAX_AGE_DAYS` (the client's default for
/// `expires_at` at signing time). The server enforces the same cap against
/// its own clock: a manifest whose `expires_at` is more than this many days
/// in the future is rejected (`OverCap`), and one whose `expires_at` has
/// passed is filtered from the catalog and purged (plan D2).
const KASK_SKILL_MAX_AGE_DAYS: u64 = 120;

/// Why a signed kask skill manifest failed verification.
///
/// The manifest fields are required (they fail to deserialize if missing),
/// so `verify_manifest_signature` only reports failures that survive
/// parsing. The upload path rejects with the variant's reason (fail closed,
/// plan D5); the periodic poll skips and warns with the same reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestVerificationError {
    InvalidPublicKey,
    InvalidSignature,
    SignatureMismatch,
    InvalidExpiresAt(String),
    ExpiredAtSigning(String),
    OverCap(String),
}

impl std::fmt::Display for ManifestVerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPublicKey => write!(f, "public key is not a valid Ed25519 key"),
            Self::InvalidSignature => write!(f, "signature is not valid Ed25519"),
            Self::SignatureMismatch => {
                write!(
                    f,
                    "signature does not verify against the manifest's public key"
                )
            }
            Self::InvalidExpiresAt(expires_at) => {
                write!(
                    f,
                    "expires_at {expires_at} is not a valid RFC 3339 timestamp"
                )
            }
            Self::ExpiredAtSigning(expires_at) => {
                write!(
                    f,
                    "expires_at {expires_at} is already in the past (expired at signing)"
                )
            }
            Self::OverCap(expires_at) => write!(
                f,
                "expires_at {expires_at} exceeds the {KASK_SKILL_MAX_AGE_DAYS}-day cap"
            ),
        }
    }
}

/// Verify a signed kask skill manifest (plan D2/D3/D5).
///
/// Checks, against the **server clock**:
/// 1. `public_key` parses as an Ed25519 key and `signature` as Ed25519.
/// 2. `signature` verifies over `manifest.canonical_signing_bytes()` — the
///    manifest's own shared canonical serialization (plan D4), so it
///    commits to `expires_at` and transitively to the tarball hash.
/// 3. `expires_at` is inside `now < expires_at <= now + 120 days`.
///
/// The upload path fails closed (400); the poll path skips + warns.
pub fn verify_manifest_signature(
    manifest: &KaskSkillManifest,
) -> Result<(), ManifestVerificationError> {
    let public_key = hex::decode(&manifest.public_key)
        .map_err(|_| ManifestVerificationError::InvalidPublicKey)?;
    let public_key: [u8; 32] = public_key
        .try_into()
        .map_err(|_| ManifestVerificationError::InvalidPublicKey)?;
    let verifying_key = VerifyingKey::from_bytes(&public_key)
        .map_err(|_| ManifestVerificationError::InvalidPublicKey)?;

    let signature = hex::decode(&manifest.signature)
        .map_err(|_| ManifestVerificationError::InvalidSignature)?;
    let signature: [u8; 64] = signature
        .try_into()
        .map_err(|_| ManifestVerificationError::InvalidSignature)?;
    let signature = Signature::from_bytes(&signature);

    let canonical = manifest
        .canonical_signing_bytes()
        .map_err(|_| ManifestVerificationError::InvalidSignature)?;
    verifying_key
        .verify_strict(&canonical, &signature)
        .map_err(|_| ManifestVerificationError::SignatureMismatch)?;

    let now = chrono::Utc::now();
    let expires_at = chrono::DateTime::parse_from_rfc3339(&manifest.expires_at)
        .map_err(|_| ManifestVerificationError::InvalidExpiresAt(manifest.expires_at.clone()))?;
    if expires_at.with_timezone(&chrono::Utc) <= now {
        return Err(ManifestVerificationError::ExpiredAtSigning(
            manifest.expires_at.clone(),
        ));
    }
    let cap = now + chrono::Duration::days(KASK_SKILL_MAX_AGE_DAYS as i64);
    if expires_at.with_timezone(&chrono::Utc) > cap {
        return Err(ManifestVerificationError::OverCap(
            manifest.expires_at.clone(),
        ));
    }

    Ok(())
}

/// zed-kask: D30 — Build the kask-skills API router.
///
/// `app_state` selects the auth middleware: in development the dev bypass
/// (`auth::dev_validate_header`) inserts a `Principal` without a Zed Cloud
/// round-trip (local collab is often run without `cd ../cloud; cargo make
/// dev`, so `validate_header` cannot reach `/client/users/me`); in
/// production the real `auth::validate_header` validates the token against Zed
/// Cloud. The `AppState` extension is layered on by the caller in `main.rs`.
///
/// The dispatch is a single `from_fn` closure (not an `if/else` over two
/// `from_fn` instantiations) so both branches share one `ServiceBuilder`
/// type — `from_fn` is monomorphized per fn item, so an `if/else` over two
/// fn items yields incompatible types.
pub fn router(app_state: &Arc<AppState>) -> Router {
    use axum::middleware;
    use tower::ServiceBuilder;
    let is_dev = app_state.config.is_development();
    let auth_layer = ServiceBuilder::new().layer(middleware::from_fn(move |req, next| {
        // `into_response()` each branch so both futures resolve to the same
        // concrete `Response<UnsyncBoxBody>` type — the validators return
        // distinct `impl IntoResponse` opaque types.
        async move {
            if is_dev {
                crate::auth::dev_validate_header(req, next)
                    .await
                    .into_response()
            } else {
                crate::auth::validate_header(req, next)
                    .await
                    .into_response()
            }
        }
    }));
    Router::new()
        .route("/api/kask-skills", get(get_kask_skills))
        .route("/api/kask-skills/:id", get(get_kask_skill))
        .route("/api/kask-skills/:id/download", get(download_kask_skill))
        .route("/api/kask-skills/:id/vote", post(vote_kask_skill))
        .route("/api/kask-skills/upload", post(upload_kask_skill))
        .route(
            "/api/kask-skills/:id",
            axum::routing::delete(delete_kask_skill),
        )
        .layer(auth_layer)
}

async fn get_kask_skills(
    Extension(app): Extension<Arc<AppState>>,
) -> Result<Json<GetKaskSkillsResponse>> {
    let skills = app.db.get_kask_skills().await?;
    Ok(Json(GetKaskSkillsResponse { data: skills }))
}

#[derive(Debug, serde::Deserialize)]
struct GetKaskSkillParams {
    id: String,
}

async fn get_kask_skill(
    Extension(app): Extension<Arc<AppState>>,
    Path(params): Path<GetKaskSkillParams>,
) -> Result<Json<Option<KaskSkillMetadata>>> {
    let skill = app.db.get_kask_skill(&params.id).await?;
    Ok(Json(skill))
}

async fn download_kask_skill(
    Extension(app): Extension<Arc<AppState>>,
    Path(params): Path<GetKaskSkillParams>,
) -> Result<Response> {
    let (source_user, skill_name) = params
        .id
        .split_once('/')
        .context("kask skill id must be \"{source_user}/{skill_name}\"")?;

    let skill = app
        .db
        .get_kask_skill(&params.id)
        .await?
        .context("unknown kask skill")?;

    let version_exists = app
        .db
        .record_kask_skill_download(source_user, skill_name, &skill.manifest.version)
        .await?;

    if !version_exists {
        Err(Error::Http(
            StatusCode::NOT_FOUND,
            "unknown kask skill version".into(),
            Default::default(),
        ))?;
    }

    // zed-kask: D30 — local (no-blob-store) path. Serve the tarball bytes
    // stored in the `kask_skill_tarballs` fallback table directly. Used by
    // local dev / self-hosted deployments without S3 (`cargo run -p collab
    // serve all` with only `DATABASE_URL`/`HTTP_PORT`/`ZED_ENVIRONMENT`). The
    // client's install path (`install_skill`) reads the raw `archive.tar.gz`
    // bytes from the response body, so returning the bytes inline matches
    // what it expects; the signed-manifest gate is unchanged (the catalog
    // row still carries `public_key`/`signature`/`expires_at`).
    if app.blob_store_client.is_none() {
        let tarball = app
            .db
            .get_kask_skill_tarball(source_user, skill_name, &skill.manifest.version)
            .await?
            .context("kask skill tarball not found in local store")?;
        let body = axum::body::Body::from(tarball);
        let mut response = axum::response::Response::new(body);
        *response.status_mut() = StatusCode::OK;
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/gzip"),
        );
        return Ok(response.into_response());
    }

    let (blob_store_client, bucket) = (
        app.blob_store_client.clone().unwrap(),
        app.config.blob_store_bucket.clone().unwrap(),
    );
    let url = blob_store_client
        .get_object()
        .bucket(bucket)
        .key(format!(
            "kask-skills/{source_user}/{skill_name}/{}/archive.tar.gz",
            skill.manifest.version
        ))
        .presigned(PresigningConfig::expires_in(KASK_SKILL_DOWNLOAD_URL_LIFETIME).unwrap())
        .await
        .context("creating presigned kask skill download url")?;

    Ok(Redirect::temporary(url.uri()).into_response())
}

async fn vote_kask_skill(
    Extension(app): Extension<Arc<AppState>>,
    Extension(principal): Extension<Principal>,
    Path(params): Path<GetKaskSkillParams>,
    Json(body): Json<KaskSkillVoteRequest>,
) -> Result<Json<serde_json::Value>> {
    let Principal::User(user) = principal;

    let (source_user, skill_name) = params
        .id
        .split_once('/')
        .context("kask skill id must be \"{source_user}/{skill_name}\"")?;

    let vote = body.vote as i16;
    if vote != 1 && vote != -1 {
        Err(Error::Http(
            StatusCode::BAD_REQUEST,
            "vote must be +1 or -1".into(),
            Default::default(),
        ))?
    }

    let (upvote_count, downvote_count) = app
        .db
        .vote_kask_skill(source_user, skill_name, user.id, vote)
        .await?;

    Ok(Json(serde_json::json!({
        "upvote_count": upvote_count,
        "downvote_count": downvote_count,
    })))
}

/// zed-kask: Upload a kask skill tarball or manifest to S3. The client
/// publishes by uploading `archive.tar.gz` and `manifest.json` to this
/// endpoint with a `?key=` query param specifying the S3 key. The server
/// proxies to S3 because the client doesn't have direct S3 credentials.
#[derive(Debug, serde::Deserialize)]
struct UploadKaskSkillParams {
    key: String,
}

async fn upload_kask_skill(
    Extension(app): Extension<Arc<AppState>>,
    Extension(principal): Extension<Principal>,
    Query(params): Query<UploadKaskSkillParams>,
    body: axum::body::Bytes,
) -> Result<StatusCode> {
    let Principal::User(user) = principal;

    // zed-kask: Verify the S3 key's source_user matches the authenticated
    // user's github_login. This prevents user bob from publishing under
    // alice's namespace.
    // Expected key format: kask-skills/{source_user}/{skill_name}/{version}/...
    //
    // zed-kask: D30 — the namespace check is relaxed in development. The
    // dev bypass principal (`auth::dev_validate_header`) synthesizes a
    // `User` whose `username` is `local-dev`, which will not match the
    // publisher's S3 key, and the local collab has no Zed Cloud to resolve
    // the real username. Production keeps the strict check.
    let key_source_user = params.key.split('/').nth(1).unwrap_or("");
    if !app.config.is_development() && key_source_user != user.username {
        Err(Error::Http(
            StatusCode::FORBIDDEN,
            format!(
                "cannot upload to namespace '{}': authenticated user is '{}'",
                key_source_user, user.username
            ),
            Default::default(),
        ))?
    }

    // zed-kask: Only clone the body for the (small) manifest upload, which
    // the immediate-index path re-parses below. Tarball bodies are passed
    // through without a copy.
    let is_manifest_upload = params.key.ends_with("/manifest.json");

    // zed-kask: verify a manifest upload's signature and expiry **before** it
    // reaches storage (fail closed, plan D2/D5). An unsigned, tampered, or
    // expired manifest must not enter the catalog or the blob store — the
    // poll would otherwise reconcile it into Postgres.
    let verified_manifest = if is_manifest_upload {
        let manifest: KaskSkillManifest = serde_json::from_slice(&body).map_err(|e| {
            Error::Http(
                StatusCode::BAD_REQUEST,
                format!("invalid manifest.json body: {e}"),
                Default::default(),
            )
        })?;
        verify_manifest_signature(&manifest).map_err(|e| {
            Error::Http(
                StatusCode::BAD_REQUEST,
                format!("manifest verification failed: {e}"),
                Default::default(),
            )
        })?;
        Some(manifest)
    } else {
        None
    };

    // zed-kask: D30 — local (no-blob-store) path. Store the uploaded bytes in
    // the `kask_skill_tarballs` fallback table. The tarball upload stores the
    // `archive.tar.gz` bytes keyed by the publish triple; the manifest upload
    // triggers the immediate catalog index (verifying the tarball row exists in
    // lieu of the S3 `head_object` check). Production with S3 takes the branch
    // below. The signed-manifest gate above is unchanged.
    if app.blob_store_client.is_none() {
        let parts: Vec<&str> = params.key.split('/').collect();
        if let ["kask-skills", source_user, skill_name, version, filename] = parts.as_slice() {
            if *filename == "archive.tar.gz" {
                app.db
                    .put_kask_skill_tarball(source_user, skill_name, version, body.to_vec())
                    .await
                    .map_err(|e| {
                        Error::Internal(anyhow::anyhow!(
                            "uploading kask skill tarball to local store: {e}"
                        ))
                    })?;
            } else if let Some(manifest) = verified_manifest {
                // Verify the tarball for this version was uploaded before
                // indexing — mirrors the S3 `head_object` check so a
                // manifest-only upload doesn't create a catalog entry whose
                // download 404s.
                let tarball_present = app
                    .db
                    .get_kask_skill_tarball(source_user, skill_name, version)
                    .await
                    .map_err(|e| {
                        Error::Internal(anyhow::anyhow!(
                            "checking local tarball for manifest upload: {e}"
                        ))
                    })?;
                if tarball_present.is_none() {
                    Err(Error::Http(
                        StatusCode::BAD_REQUEST,
                        "tarball for this version has not been uploaded".into(),
                        Default::default(),
                    ))?;
                }
                let now = time::OffsetDateTime::now_utc();
                if let Err(err) = app
                    .db
                    .insert_kask_skill_versions(&[
                        crate::db::queries::kask_skills::NewKaskSkillVersion {
                            source_user: source_user.to_string(),
                            skill_name: skill_name.to_string(),
                            version: version.to_string(),
                            description: manifest.description,
                            dependencies: manifest.dependencies,
                            tarball_sha256: manifest.tarball_sha256,
                            public_key: manifest.public_key,
                            signature: manifest.signature,
                            expires_at: manifest.expires_at,
                            published_at: time::PrimitiveDateTime::new(now.date(), now.time()),
                        },
                    ])
                    .await
                {
                    log::warn!(
                        "failed to index kask skill '{}' version '{}' immediately (local store): {err:#}",
                        params.key,
                        version
                    );
                }
            }
        }
        return Ok(StatusCode::CREATED);
    }

    let (blob_store_client, bucket) = (
        app.blob_store_client.clone().unwrap(),
        app.config.blob_store_bucket.clone().unwrap(),
    );

    blob_store_client
        .put_object()
        .bucket(&bucket)
        .key(&params.key)
        .body(body.into())
        .send()
        .await
        .map_err(|e| Error::Internal(anyhow::anyhow!("uploading kask skill to S3: {e}")))?;

    // zed-kask: A manifest.json upload means a complete publish (tarball +
    // manifest). Upsert the catalog row immediately so the skill is visible
    // in the marketplace within seconds instead of waiting up to
    // KASK_SKILL_FETCH_INTERVAL for the periodic poll. The poll remains as
    // reconciliation for out-of-band S3 writes.
    if let Some(manifest) = verified_manifest {
        let parts: Vec<&str> = params.key.split('/').collect();
        if let [
            "kask-skills",
            source_user,
            skill_name,
            version,
            "manifest.json",
        ] = parts.as_slice()
        {
            let index_result = async {
                // Verify the tarball for this version actually exists before
                // indexing — otherwise a manifest-only upload (client bug or
                // reordering) creates a catalog entry whose install 404s.
                blob_store_client
                    .head_object()
                    .bucket(&bucket)
                    .key(format!(
                        "kask-skills/{source_user}/{skill_name}/{version}/archive.tar.gz"
                    ))
                    .send()
                    .await
                    .context("tarball for this version has not been uploaded")?;

                let now = time::OffsetDateTime::now_utc();
                app.db
                    .insert_kask_skill_versions(&[
                        crate::db::queries::kask_skills::NewKaskSkillVersion {
                            source_user: source_user.to_string(),
                            skill_name: skill_name.to_string(),
                            version: version.to_string(),
                            description: manifest.description,
                            dependencies: manifest.dependencies,
                            tarball_sha256: manifest.tarball_sha256,
                            public_key: manifest.public_key,
                            signature: manifest.signature,
                            expires_at: manifest.expires_at,
                            published_at: time::PrimitiveDateTime::new(now.date(), now.time()),
                        },
                    ])
                    .await
            }
            .await;
            if let Err(err) = index_result {
                // The upload itself succeeded — the next poll will reconcile.
                // Degrading to a warn keeps publish usable if the immediate
                // index fails transiently (e.g. DB reconnect).
                log::warn!(
                    "failed to index kask skill '{}' version '{}' immediately: {err:#}. \
                     The periodic poll will pick it up within {:?}.",
                    params.key,
                    version,
                    KASK_SKILL_FETCH_INTERVAL
                );
            }
        }
    }

    Ok(StatusCode::CREATED)
}

/// zed-kask: Delete a kask skill from S3 and Postgres. Called when a user
/// toggles a skill back to Private (unpublish). The skill is deleted from
/// S3 (prefix delete) and the Postgres row is removed.
async fn delete_kask_skill(
    Extension(app): Extension<Arc<AppState>>,
    Extension(principal): Extension<Principal>,
    Path(params): Path<GetKaskSkillParams>,
) -> Result<StatusCode> {
    let Principal::User(user) = principal;

    let (source_user, skill_name) = params
        .id
        .split_once('/')
        .context("kask skill id must be \"{source_user}/{skill_name}\"")?;

    // zed-kask: Only the publisher can unpublish their own skill.
    // The user's username must match the source_user in the id.
    // (The client uses user.username as the source_user namespace.)
    // zed-kask: D30 — relaxed in development for the same reason as
    // `upload_kask_skill` (the dev bypass principal's username is `local-dev`).
    if !app.config.is_development() && user.username != source_user {
        Err(Error::Http(
            StatusCode::FORBIDDEN,
            "only the publisher can unpublish their skill".into(),
            Default::default(),
        ))?
    }

    // Delete from S3 (prefix delete all versions).
    if let Some((blob_store_client, bucket)) = app
        .blob_store_client
        .clone()
        .zip(app.config.blob_store_bucket.clone())
    {
        let prefix = format!("kask-skills/{}/{}/", source_user, skill_name);
        let list = blob_store_client
            .list_objects()
            .bucket(&bucket)
            .prefix(&prefix)
            .send()
            .await
            .map_err(|e| Error::Internal(e.into()))?;
        for object in list.contents.unwrap_or_default() {
            if let Some(key) = object.key {
                blob_store_client
                    .delete_object()
                    .bucket(&bucket)
                    .key(&key)
                    .send()
                    .await
                    .map_err(|e| Error::Internal(e.into()))?;
            }
        }
    } else {
        // zed-kask: D30 — local (no-blob-store) path. Delete the locally-stored
        // tarballs for this publish namespace. The catalog rows are removed by
        // `delete_kask_skill` below (cascades to versions/votes).
        if let Err(err) = app
            .db
            .delete_kask_skill_tarballs(source_user, skill_name)
            .await
        {
            log::warn!("failed to delete local tarballs for '{source_user}/{skill_name}': {err:#}",);
        }
    }

    // Delete from Postgres (cascades to versions and votes).
    let deleted = app.db.delete_kask_skill(source_user, skill_name).await?;
    if !deleted {
        log::info!(
            "kask-extensions: skill '{}/{}' was not in Postgres (may have been already removed)",
            source_user,
            skill_name
        );
    }

    Ok(StatusCode::NO_CONTENT)
}

const KASK_SKILL_DOWNLOAD_URL_LIFETIME: Duration = Duration::from_secs(3 * 60);
const KASK_SKILL_FETCH_INTERVAL: Duration = Duration::from_secs(5 * 60);

pub fn fetch_kask_skills_from_blob_store_periodically(app_state: Arc<AppState>) {
    let Some(blob_store_client) = app_state.blob_store_client.clone() else {
        // zed-kask: warn (not info) — an unconfigured blob store disables
        // the entire skill marketplace, and an operator must be able to
        // distinguish "not configured" from "configured but broken".
        log::warn!(
            "kask skill marketplace disabled: blob store not configured. \
             Set BLOB_STORE_URL, BLOB_STORE_REGION, BLOB_STORE_ACCESS_KEY, \
             BLOB_STORE_SECRET_KEY, and BLOB_STORE_BUCKET to enable \
             /api/kask-skills upload/download and catalog indexing."
        );
        return;
    };
    let Some(blob_store_bucket) = app_state.config.blob_store_bucket.clone() else {
        log::warn!(
            "kask skill marketplace disabled: BLOB_STORE_BUCKET not set. \
             /api/kask-skills upload/download and catalog indexing will not work."
        );
        return;
    };

    let executor = app_state.executor.clone();
    executor.spawn_detached({
        let executor = executor.clone();
        async move {
            loop {
                fetch_kask_skills_from_blob_store(
                    &blob_store_client,
                    &blob_store_bucket,
                    &app_state,
                )
                .await
                .log_err();
                executor.sleep(KASK_SKILL_FETCH_INTERVAL).await;
            }
        }
    });
}

async fn fetch_kask_skills_from_blob_store(
    blob_store_client: &aws_sdk_s3::Client,
    blob_store_bucket: &String,
    app_state: &Arc<AppState>,
) -> anyhow::Result<()> {
    log::info!("fetching kask skills from blob store");

    let mut next_marker = None;
    let mut published_versions: collections::HashMap<String, Vec<String>> = Default::default();

    loop {
        let list = blob_store_client
            .list_objects()
            .bucket(blob_store_bucket)
            .prefix("kask-skills/")
            .set_marker(next_marker.clone())
            .send()
            .await?;
        let objects = list.contents.unwrap_or_default();
        log::info!("fetched {} kask object(s) from blob store", objects.len());

        for object in &objects {
            let Some(key) = object.key.as_ref() else {
                continue;
            };
            let mut parts = key.split('/');
            let Some(_) = parts.next().filter(|part| *part == "kask-skills") else {
                continue;
            };
            let Some(source_user) = parts.next() else {
                continue;
            };
            let Some(skill_name) = parts.next() else {
                continue;
            };
            let Some(version) = parts.next() else {
                continue;
            };
            if parts.next() == Some("manifest.json") {
                let id = format!("{source_user}/{skill_name}");
                published_versions
                    .entry(id)
                    .or_default()
                    .push(version.to_owned());
            }
        }

        if let (Some(true), Some(last_object)) = (list.is_truncated, objects.last()) {
            next_marker.clone_from(&last_object.key);
        } else {
            break;
        }
    }

    log::info!("found {} published kask skills", published_versions.len());

    let known_versions = app_state.db.get_known_kask_skill_versions().await?;

    let mut new_versions = Vec::new();
    let empty = Vec::new();
    for (id, versions) in &published_versions {
        let known = known_versions.get(id).unwrap_or(&empty);
        for version in versions {
            if known.binary_search(version).is_err()
                && let Some(new_version) =
                    fetch_kask_skill_manifest(blob_store_client, blob_store_bucket, id, version)
                        .await
                        .log_err()
            {
                new_versions.push(new_version);
            }
        }
    }

    app_state
        .db
        .insert_kask_skill_versions(&new_versions)
        .await?;

    // zed-kask: expiry sweep (plan Phase 3 / D2). Runs on the same cadence as
    // the poll. A nonzero purge count is `log::warn!`ed — the `.rules`
    // "signal, not silence" trap: an operator must be able to distinguish
    // "catalog healthy, nothing expired" from "catalog purged dead skills".
    let purged = app_state.db.purge_expired_kask_skill_versions().await?;
    if purged > 0 {
        log::warn!(
            "kask skill expiry sweep purged {purged} expired version(s). \
             Their publishers must re-sign and re-publish to relist them."
        );
    }

    log::info!(
        "fetched {} new kask skills from blob store",
        new_versions.len()
    );

    Ok(())
}

async fn fetch_kask_skill_manifest(
    blob_store_client: &aws_sdk_s3::Client,
    blob_store_bucket: &String,
    id: &str,
    version: &str,
) -> anyhow::Result<crate::db::queries::kask_skills::NewKaskSkillVersion> {
    let (source_user, skill_name) = id
        .split_once('/')
        .context("kask skill id must be \"{source_user}/{skill_name}\"")?;

    let object = blob_store_client
        .get_object()
        .bucket(blob_store_bucket)
        .key(format!(
            "kask-skills/{source_user}/{skill_name}/{version}/manifest.json"
        ))
        .send()
        .await?;
    let manifest_bytes = object
        .body
        .collect()
        .await
        .map(|data| data.into_bytes())
        .with_context(|| {
            format!("failed to download manifest for kask skill {id} version {version}")
        })?
        .to_vec();
    let manifest = serde_json::from_slice::<cloud_api_types::KaskSkillManifest>(&manifest_bytes)
        .with_context(|| {
            format!(
                "invalid manifest for kask skill {id} version {version}: {}",
                String::from_utf8_lossy(&manifest_bytes)
            )
        })?;

    // zed-kask: skip + warn on unverifiable manifests (fail closed, plan
    // D5). The upload path rejects the same way with a 400; the poll is the
    // reconciliation path for out-of-band S3 writes, so it must not index a
    // manifest that would have been rejected at upload. The error propagates
    // to the caller's `.log_err()`, which logs the S3 key + reason.
    if let Err(error) = verify_manifest_signature(&manifest) {
        anyhow::bail!(
            "kask skill {id} version {version} failed signature verification; \
             not indexed. The publisher must re-sign and re-publish. Reason: {error}"
        );
    }
    let published_at = object.last_modified.with_context(|| {
        format!("missing last modified timestamp for kask skill {id} version {version}")
    })?;
    let published_at = time::OffsetDateTime::from_unix_timestamp_nanos(published_at.as_nanos())?;
    let published_at = time::PrimitiveDateTime::new(published_at.date(), published_at.time());

    Ok(crate::db::queries::kask_skills::NewKaskSkillVersion {
        source_user: source_user.to_owned(),
        skill_name: skill_name.to_owned(),
        version: version.to_owned(),
        description: manifest.description,
        dependencies: manifest.dependencies,
        tarball_sha256: manifest.tarball_sha256,
        public_key: manifest.public_key,
        signature: manifest.signature,
        expires_at: manifest.expires_at,
        published_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn signed_manifest(expires_at: &str) -> KaskSkillManifest {
        let mut secret = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::rng(), &mut secret);
        let signing_key = SigningKey::from_bytes(&secret);
        let public_key: ed25519_dalek::VerifyingKey = signing_key.verifying_key();

        let mut manifest = KaskSkillManifest {
            source_user: "alice".to_string(),
            skill_name: "essentialist".to_string(),
            version: "2026-08-02.1".to_string(),
            description: "test".to_string(),
            dependencies: vec![],
            tarball_sha256: "abc123".to_string(),
            public_key: hex::encode(public_key.as_bytes()),
            signature: String::new(),
            expires_at: expires_at.to_string(),
        };
        let canonical = manifest.canonical_signing_bytes().unwrap();
        let signature = ed25519_dalek::Signer::sign(&signing_key, &canonical);
        manifest.signature = hex::encode(signature.to_bytes());
        manifest
    }

    fn future_expiry() -> String {
        (chrono::Utc::now() + chrono::Duration::days(KASK_SKILL_MAX_AGE_DAYS as i64)).to_rfc3339()
    }

    // zed-kask: pin the cap constant — the server's `KASK_SKILL_MAX_AGE_DAYS`
    // mirrors `hkask_keystore::KEY_MAX_AGE_DAYS` (the client's default at
    // signing time). The keystore pins its own side with
    // `key_max_age_days_is_120`; this pins the server side so a one-sided
    // change (client raising the default, server not enforcing, or vice
    // versa) fails loudly instead of silently accepting mismatched windows
    // (`.rules` "Model-name constants must not be duplicated across crates"
    // — same drift class for a policy constant).
    #[test]
    fn kask_skill_max_age_days_is_120() {
        assert_eq!(KASK_SKILL_MAX_AGE_DAYS, 120);
    }

    // zed-kask: pin the deny-by-default deviation (plan Phase 5 / D5) — the
    // upstream extension store accepts any manifest; kask requires a valid
    // signature inside the 120-day window.
    #[test]
    fn verification_accepts_valid_signed_manifest() {
        let manifest = signed_manifest(&future_expiry());
        verify_manifest_signature(&manifest).expect("valid manifest must verify");
    }

    #[test]
    fn verification_rejects_tampered_manifest() {
        let mut manifest = signed_manifest(&future_expiry());
        manifest.description = "tampered".to_string();
        assert_eq!(
            verify_manifest_signature(&manifest),
            Err(ManifestVerificationError::SignatureMismatch)
        );
    }

    #[test]
    fn verification_rejects_expired_at_signing() {
        let manifest = signed_manifest("2020-01-01T00:00:00Z");
        assert_eq!(
            verify_manifest_signature(&manifest),
            Err(ManifestVerificationError::ExpiredAtSigning(
                "2020-01-01T00:00:00Z".to_string()
            ))
        );
    }

    #[test]
    fn verification_rejects_over_cap_expiry() {
        // 121 days out — beyond the server's 120-day cap (plan D2: the cap is
        // judged by the server clock, not the publisher's).
        let far_future = (chrono::Utc::now()
            + chrono::Duration::days(KASK_SKILL_MAX_AGE_DAYS as i64 + 1))
        .to_rfc3339();
        let manifest = signed_manifest(&far_future);
        assert!(matches!(
            verify_manifest_signature(&manifest),
            Err(ManifestVerificationError::OverCap(_))
        ));
    }

    #[test]
    fn verification_rejects_invalid_public_key() {
        let mut manifest = signed_manifest(&future_expiry());
        manifest.public_key = "zz".repeat(32);
        assert_eq!(
            verify_manifest_signature(&manifest),
            Err(ManifestVerificationError::InvalidPublicKey)
        );
    }
}
