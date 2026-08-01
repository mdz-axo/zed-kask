use crate::rpc::Principal;
use crate::{AppState, Error, Result};
use anyhow::Context as _;
use aws_sdk_s3::presigning::PresigningConfig;
use axum::{
    Extension, Json, Router,
    extract::{Path, Query},
    http::StatusCode,
    response::Redirect,
    routing::{get, post},
};
use cloud_api_types::{GetKaskSkillsResponse, KaskSkillMetadata, KaskSkillVoteRequest};
use std::sync::Arc;
use std::time::Duration;
use util::ResultExt;

pub fn router() -> Router {
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
) -> Result<Redirect> {
    let Some((blob_store_client, bucket)) = app
        .blob_store_client
        .clone()
        .zip(app.config.blob_store_bucket.clone())
    else {
        Err(Error::Http(
            StatusCode::NOT_IMPLEMENTED,
            "not supported".into(),
            Default::default(),
        ))?
    };

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

    Ok(Redirect::temporary(url.uri()))
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
    let key_source_user = params.key.split('/').nth(1).unwrap_or("");
    if key_source_user != user.username {
        Err(Error::Http(
            StatusCode::FORBIDDEN,
            format!(
                "cannot upload to namespace '{}': authenticated user is '{}'",
                key_source_user, user.username
            ),
            Default::default(),
        ))?
    }

    let Some((blob_store_client, bucket)) = app
        .blob_store_client
        .clone()
        .zip(app.config.blob_store_bucket.clone())
    else {
        Err(Error::Http(
            StatusCode::NOT_IMPLEMENTED,
            "blob store not configured".into(),
            Default::default(),
        ))?
    };

    // zed-kask: Only clone the body for the (small) manifest upload, which
    // the immediate-index path re-parses below. Tarball bodies are passed
    // through without a copy.
    let is_manifest_upload = params.key.ends_with("/manifest.json");
    let manifest_body = is_manifest_upload.then(|| body.clone());

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
    if let Some(manifest_body) = manifest_body {
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
                let manifest: cloud_api_types::KaskSkillManifest =
                    serde_json::from_slice(&manifest_body).context("invalid manifest.json body")?;

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
    if user.username != source_user {
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
        published_at,
    })
}
