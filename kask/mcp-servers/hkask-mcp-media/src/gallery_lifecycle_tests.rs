//! Gallery lifecycle tests — organize, refresh, asset addition and deletion
//! across the gallery policy modes, end to end against a real gallery store.
use super::*;
use crate::test_support::read_only_organize_request;
use hkask_storage::database::sqlite::SqliteDriver;
use rmcp::handler::server::wrapper::Parameters;
use std::path::Path;
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn server(database: &Path, inference: Arc<dyn InferencePort>) -> MediaServer {
    unsafe { std::env::set_var(crate::models::VISION_ENV, "OpenRouter/test-vision") };
    let pool = SqliteDriver::file_pool(database.to_str().expect("UTF-8 fixture path"))
        .expect("unencrypted SQLite");
    let store =
        GalleryStore::from_driver(Arc::new(SqliteDriver::new(pool))).expect("gallery schema");
    MediaServer::new(
        hkask_types::WebID::new(),
        inference,
        Arc::new(Mutex::new(None)),
        Arc::new(store),
        templates::create_env().expect("templates"),
        FfmpegRunner::detect(),
        YtDlpRunner::detect(),
        jobs::new_job_store(),
        None,
        Default::default(),
    )
}

/// A real one-second video or tone (ffmpeg lavfi), chosen by extension:
/// gallery_add_media checks the declared kind against the file's streams.
async fn media_fixture(path: &Path) -> TestResult {
    crate::test_support::media_fixture(path, &[]).await
}

fn png(path: &Path, color: u8) {
    image::RgbImage::from_pixel(3, 2, image::Rgb([color, 0, 0]))
        .save(path)
        .expect("valid PNG fixture");
}

async fn organize(
    server: &MediaServer,
    root: &Path,
    recursive: bool,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let result = server
        .gallery_organize(Parameters(GalleryOrganizeRequest {
            path: root.to_string_lossy().into_owned(),
            mode: "read-only".into(),
            recursive,
            auto_analyze: false,
        }))
        .await?;
    let value: serde_json::Value = serde_json::from_str(&result)?;
    assert!(!result.contains("\"error\":"), "{result}");
    assert_eq!(value["content"]["gallery_changed"], true);
    Ok(value)
}

/// The transcript-linked audio-asset fixture shared by the deletion
/// tests: a temp gallery organized in `mode`, one source.wav, one
/// gallery asset row, and one stored transcript linked to the asset.
/// Both deletion tests each carried this setup before the helper
/// (2026-10-09 ratchet pass).
struct TranscriptLinkedAsset {
    _fixture: tempfile::TempDir,
    source: std::path::PathBuf,
    server: MediaServer,
    asset: crate::gallery::ImageRecord,
    transcript: crate::transcript_store::TranscriptSummary,
}

async fn transcript_linked_audio_asset(
    mode: &str,
) -> Result<TranscriptLinkedAsset, Box<dyn std::error::Error>> {
    let fixture = crate::test_support::contained_tempdir()?;
    let root = fixture.path().join("root");
    std::fs::create_dir(&root)?;
    let source = root.join("source.wav");
    std::fs::write(&source, b"source audio")?;
    let server = server(
        &fixture.path().join("gallery.sqlite"),
        Arc::new(BarrierVision::barrier(analysis_vision_text)),
    );
    server
        .gallery_organize(Parameters(GalleryOrganizeRequest {
            path: root.to_string_lossy().into_owned(),
            mode: mode.into(),
            recursive: true,
            auto_analyze: false,
        }))
        .await?;
    let gallery = server.access_gallery()?;
    let asset = server.gallery_store.add_media(
        &gallery.gallery_id,
        source.to_str().ok_or("UTF-8 source path")?,
        "source-hash",
        0,
        0,
        "wav",
        12,
        "audio",
    )?;
    let bundle = crate::transcript::TranscriptBundle::new(
        source.to_string_lossy().into_owned(),
        1.0,
        "source".to_string(),
    );
    let transcript = crate::transcript_store::store_transcript(
        &**server.gallery_store.driver(),
        &bundle,
        Some(&asset.id),
    )?;
    Ok(TranscriptLinkedAsset {
        _fixture: fixture,
        source,
        server,
        asset,
        transcript,
    })
}

/// dcterms:identifier: `MediaServer::gallery_delete_image`
/// expect: Gallery policy preserves originals unless destructive mode permits deletion; transcripts survive success, refusal, and partial failure.
/// [P1] Motivating: Catalog cleanup respects the user's preservation policy.
#[tokio::test]
async fn gallery_deletion_mode_matrix() -> TestResult {
    for (mode, delete_file, fail_index) in [
        ("read-only", false, false),
        ("read-only", true, false),
        ("copy-on-write", false, false),
        ("copy-on-write", true, false),
        ("destructive", false, false),
        ("destructive", true, false),
        ("destructive", true, true),
    ] {
        let TranscriptLinkedAsset {
            _fixture,
            source,
            server,
            asset,
            transcript,
        } = transcript_linked_audio_asset(mode).await?;

        if fail_index {
            server.gallery_store.driver().execute_batch(
                "CREATE TRIGGER refuse_delete BEFORE DELETE ON gallery_images
                 BEGIN SELECT RAISE(ABORT, 'injected index failure'); END;",
            )?;
        }
        let result = server
            .gallery_delete_image(Parameters(GalleryDeleteImageRequest {
                image_index: None,
                image_id: Some(asset.id.clone()),
                delete_file,
            }))
            .await;

        let denied = delete_file && mode != "destructive";
        let index_preserved = denied || fail_index;
        if denied {
            let error = result.expect_err("preserving mode must refuse file deletion");
            assert!(error.message.contains("destructive"), "{error}");
        } else if fail_index {
            let error = result.expect_err("injected index failure");
            assert!(error.message.contains("source file was deleted"), "{error}");
            assert!(error.message.contains("injected index failure"), "{error}");
        } else {
            let value: serde_json::Value = serde_json::from_str(&result?)?;
            assert_eq!(value["content"]["transcripts_detached"], 1);
            assert_eq!(value["content"]["file_deleted"], delete_file);
        }
        let source_preserved = denied || !delete_file;
        assert_eq!(source.exists(), source_preserved, "{mode}, {delete_file}");
        if source_preserved {
            assert_eq!(std::fs::read(&source)?, b"source audio");
        }
        assert_eq!(
            server.gallery_store.get_by_id(&asset.id).is_ok(),
            index_preserved
        );
        let (summary, _) = crate::transcript_store::load_transcript(
            &**server.gallery_store.driver(),
            &transcript.id,
        )?
        .ok_or("transcript preserved")?;
        assert_eq!(
            summary.gallery_asset_id.as_deref(),
            index_preserved.then_some(asset.id.as_str())
        );
        assert_eq!(
            summary.source_link,
            if index_preserved {
                crate::transcript_store::TranscriptSourceLink::Linked
            } else {
                crate::transcript_store::TranscriptSourceLink::Detached
            }
        );
        assert_eq!(
            summary.source_availability,
            if source_preserved {
                crate::transcript_store::TranscriptSourceAvailability::Available
            } else {
                crate::transcript_store::TranscriptSourceAvailability::Missing
            }
        );
    }
    Ok(())
}

/// dcterms:identifier: `MediaServer::gallery_delete_image`
/// expect: If source-file deletion fails, I see the filesystem cause and no durable identity is falsely removed.
/// [P1] Motivating: Destructive operations fail closed and never report cleanup they did not perform.
#[tokio::test]
async fn asset_file_deletion_failure_preserves_gallery_and_transcript_identity() -> TestResult {
    let TranscriptLinkedAsset {
        _fixture,
        source,
        server,
        asset,
        transcript,
    } = transcript_linked_audio_asset("destructive").await?;
    std::fs::remove_file(&source)?;
    std::fs::create_dir(&source)?;

    let error = server
        .gallery_delete_image(Parameters(GalleryDeleteImageRequest {
            image_index: None,
            image_id: Some(asset.id.clone()),
            delete_file: true,
        }))
        .await
        .expect_err("directory cannot be removed as a source file");

    assert!(error.message.contains("delete source file"), "{error}");
    assert!(error.message.contains("source.wav"), "{error}");
    assert!(
        source.is_dir(),
        "failed unlink must leave the source path intact"
    );
    assert!(server.gallery_store.get_by_id(&asset.id).is_ok());
    let (summary, _) =
        crate::transcript_store::load_transcript(&**server.gallery_store.driver(), &transcript.id)?
            .ok_or("transcript preserved")?;
    assert_eq!(summary.gallery_asset_id.as_deref(), Some(asset.id.as_str()));
    assert_eq!(
        summary.source_link,
        crate::transcript_store::TranscriptSourceLink::Linked
    );
    assert_eq!(
        summary.source_availability,
        crate::transcript_store::TranscriptSourceAvailability::Missing
    );
    Ok(())
}

/// A fresh fixture with two created subdirectories `first` and `second` —
/// the shared setup of the two-root gallery tests, which vary in what
/// they put inside the roots and assert after. The fixture is returned
/// (not dropped) so its registrations outlive the test body.
fn two_created_dir_roots()
-> std::io::Result<(tempfile::TempDir, std::path::PathBuf, std::path::PathBuf)> {
    let fixture = crate::test_support::contained_tempdir()?;
    let first = fixture.path().join("first");
    let second = fixture.path().join("second");
    std::fs::create_dir(&first)?;
    std::fs::create_dir(&second)?;
    Ok((fixture, first, second))
}

/// expect: My gallery and mode survive restart, aliases, and switches; invalid roots never activate. [P1]
#[tokio::test]
async fn reopen_two_roots_aliases_and_failure_preserves_activation() -> TestResult {
    let (fixture, first, second) = two_created_dir_roots()?;
    png(&first.join("first.png"), 1);
    png(&second.join("second.png"), 2);
    let database = fixture.path().join("gallery.sqlite");
    let original;
    {
        let server = server(
            &database,
            Arc::new(BarrierVision::barrier(analysis_vision_text)),
        );
        organize(&server, &first, true).await?;
        original = server.access_gallery()?.gallery_id;
        organize(&server, &second, true).await?;
        assert_ne!(server.access_gallery()?.gallery_id, original);
    }
    let server = server(
        &database,
        Arc::new(BarrierVision::barrier(analysis_vision_text)),
    );
    assert!(
        server.access_gallery().is_err(),
        "no automatic startup activation"
    );
    organize(&server, &first.join("../first/."), true).await?;
    assert_eq!(server.access_gallery()?.gallery_id, original);
    let result = server
        .gallery_organize(Parameters(GalleryOrganizeRequest {
            path: first.to_string_lossy().into_owned(),
            mode: "destructive".into(),
            recursive: true,
            auto_analyze: false,
        }))
        .await?;
    assert!(result.contains("\"mode_preserved\":true"), "{result}");
    assert_eq!(server.access_gallery()?.mode, "read-only");
    let invalid = fixture.path().join("absent");
    assert!(
        server
            .gallery_organize(read_only_organize_request(&invalid))
            .await
            .is_err()
    );
    assert_eq!(server.access_gallery()?.gallery_id, original);
    let rows = server
        .gallery_store
        .driver()
        .query("SELECT root_path FROM galleries", &[])?;
    assert_eq!(rows.len(), 2);
    #[cfg(unix)]
    {
        let alias = fixture.path().join("alias");
        std::os::unix::fs::symlink(&first, &alias)?;
        organize(&server, &alias, true).await?;
        assert_eq!(server.access_gallery()?.gallery_id, original);
    }
    Ok(())
}

/// expect: Missing files retain tags, albums and lineage; reappearance restores the same ID. [P1]
#[tokio::test]
async fn missing_return_changed_and_partial_coverage() -> TestResult {
    let fixture = crate::test_support::contained_tempdir()?;
    let root = fixture.path().join("root");
    std::fs::create_dir(&root)?;
    let path = root.join("one.png");
    png(&path, 1);
    let original_bytes = std::fs::read(&path)?;
    let server = server(
        &fixture.path().join("gallery.sqlite"),
        Arc::new(BarrierVision::barrier(analysis_vision_text)),
    );
    organize(&server, &root, true).await?;
    let gallery = server.access_gallery()?;
    let original = server
        .gallery_store
        .get_image(&gallery.gallery_id, Some(0), None)?;
    let store = &server.gallery_store;
    store.tag_image(&original.id, "caption", "My photo", 1.0, "user")?;
    let album = store.create_album(&gallery.gallery_id, "Keep", None)?;
    store.add_to_album(&album.id, &original.id)?;
    store.record_generation(
        &original.id,
        "generate_image",
        Some("original"),
        None,
        None,
        None,
        None,
        None,
        None,
    )?;
    organize(&server, &root, true).await?;
    assert_eq!(std::fs::read(&path)?, original_bytes);
    assert_eq!(store.count_assets(&gallery.gallery_id)?, 1);
    std::fs::remove_file(&path)?;
    organize(&server, &root, true).await?;
    assert_eq!(store.count_assets(&gallery.gallery_id)?, 0);
    assert!(store.list_assets(&gallery.gallery_id, 0, 10)?.is_empty());
    assert!(store.get_all_tags(&gallery.gallery_id)?.is_empty());
    assert!(store.list_album_members(&album.id)?.is_empty());
    assert!(store.get_by_id(&original.id)?.missing);
    let detail = server
        .gallery_asset_detail(Parameters(GalleryAssetDetailRequest {
            image_index: None,
            image_id: Some(original.id.clone()),
        }))
        .await?;
    assert!(detail.contains("\"missing\":true"), "{detail}");
    assert_eq!(store.get_tags(&original.id)?.len(), 1);
    assert!(store.get_generation(&original.id)?.is_some());
    std::fs::write(&path, &original_bytes)?;
    organize(&server, &root, true).await?;
    let returned = store.get_image(&gallery.gallery_id, Some(0), None)?;
    assert_eq!(returned.id, original.id);
    assert_eq!(returned.added_at, original.added_at);
    assert!(!returned.missing);
    assert_eq!(returned.metadata_stale, original.metadata_stale);
    assert_eq!(
        store.list_album_members(&album.id)?,
        vec![original.id.clone()]
    );
    png(&path, 2);
    organize(&server, &root, true).await?;
    let changed = store.get_by_id(&original.id)?;
    assert!(changed.metadata_stale);
    assert_ne!(changed.hash, original.hash);
    assert_eq!(store.get_tags(&original.id)?.len(), 1);
    assert!(!store.persist_analysis_for_tag_types(
        &original,
        &[("caption".into(), "wrong revision".into(), 1.0)],
        &["caption".to_string()],
        "fake",
        true
    )?);
    assert!(store.get_by_id(&original.id)?.metadata_stale);
    assert!(store.persist_analysis_for_tag_types(
        &changed,
        &[("caption".into(), "new revision".into(), 1.0)],
        &["caption".to_string()],
        "fake",
        true
    )?);
    assert!(!store.get_by_id(&original.id)?.metadata_stale);
    // Bad decode makes the entire scan conservative, even for another absent file.
    std::fs::remove_file(&path)?;
    std::fs::write(root.join("broken.png"), b"broken")?;
    let result = organize(&server, &root, true).await?;
    assert!(result.to_string().contains("degraded"));
    assert!(!store.get_by_id(&original.id)?.missing);
    // An unreadable/nonexistent root on refresh reports coverage, never absence.
    std::fs::rename(&root, fixture.path().join("offline"))?;
    let (scan, result) = server.rescan_gallery(&gallery, true)?;
    assert!(!scan.errors.is_empty());
    assert_eq!(result.missing, 0);
    Ok(())
}

/// expect: Nonrecursive scans do not hide nested images, external imports, or audio/video. [P1]
#[tokio::test]
async fn scope_and_identical_copies_keep_distinct_path_identities() -> TestResult {
    let fixture = crate::test_support::contained_tempdir()?;
    let root = fixture.path().join("root");
    std::fs::create_dir_all(root.join("nested"))?;
    png(&root.join("a.png"), 1);
    png(&root.join("b.png"), 1);
    png(&root.join("nested/c.png"), 1);
    let server = server(
        &fixture.path().join("gallery.sqlite"),
        Arc::new(BarrierVision::barrier(analysis_vision_text)),
    );
    organize(&server, &root, true).await?;
    let gallery = server.access_gallery()?;
    let store = &server.gallery_store;
    let copies = store.list_assets(&gallery.gallery_id, 0, 10)?;
    assert_eq!(copies.len(), 3);
    assert_ne!(copies[0].id, copies[1].id);
    assert_eq!(copies[0].hash, copies[1].hash);
    let external = fixture.path().join("external.png");
    png(&external, 2);
    let (external_id, _) = server.import_reference_image(&external, fixture.path())?;
    for (name, kind) in [("film.mp4", "video"), ("sound.wav", "audio")] {
        let path = root.join(name);
        media_fixture(&path).await?;
        let response = server
            .gallery_add_media(Parameters(GalleryAddMediaRequest {
                path: path.to_string_lossy().into_owned(),
                media_type: kind.into(),
                width: None,
                height: None,
            }))
            .await?;
        let response: serde_json::Value = serde_json::from_str(&response)?;
        let payload = hkask_types::tool_response::unwrap_tool_envelope(response);
        let asset_id = payload["id"]
            .as_str()
            .ok_or("gallery_add_media response omitted Asset id")?;
        let hint = payload["display_hint"]
            .as_str()
            .ok_or("gallery_add_media response omitted display_hint")?;
        let body = hint
            .trim()
            .strip_prefix("```media")
            .and_then(|body| body.trim().strip_suffix("```"))
            .ok_or("gallery_add_media display_hint was not a media block")?;
        let body: serde_json::Value = serde_json::from_str(body)?;
        assert_eq!(body["gallery_asset_id"], asset_id);
    }
    std::fs::remove_file(root.join("nested/c.png"))?;
    organize(&server, &root, false).await?;
    assert_eq!(store.count_assets(&gallery.gallery_id)?, 5);
    organize(&server, &root, true).await?;
    assert_eq!(store.count_assets(&gallery.gallery_id)?, 4);
    // A file outside the active gallery belongs to its own folder's gallery.
    let home = store
        .containing(&external.canonicalize()?)?
        .ok_or("external home gallery")?;
    assert_ne!(home.id, gallery.gallery_id);
    assert!(!store.get_by_id(&external_id)?.missing);
    let active = store.list_assets(&gallery.gallery_id, 0, 10)?;
    for (index, asset) in active.iter().enumerate() {
        assert_eq!(
            store.get_image(&gallery.gallery_id, Some(index), None)?.id,
            asset.id
        );
        assert_eq!(
            store.list_assets(&gallery.gallery_id, index, 1)?[0].id,
            asset.id
        );
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&external, root.join("escape.png"))?;
        std::fs::remove_file(root.join("a.png"))?;
        let (scan, result) = server.rescan_gallery(&gallery, true)?;
        assert!(scan.errors.iter().any(|error| error.contains("Symlink")));
        assert_eq!(result.missing, 0);
    }
    Ok(())
}

/// expect: Importing a file files it under the gallery whose folder
/// contains it, whichever gallery happens to be active. [P1]
#[tokio::test]
async fn add_media_files_under_the_containing_gallery() -> TestResult {
    let fixture = crate::test_support::contained_tempdir()?;
    let owner = fixture.path().join("owner");
    let active = fixture.path().join("active");
    std::fs::create_dir_all(&owner)?;
    std::fs::create_dir_all(&active)?;
    let server = server(
        &fixture.path().join("gallery.sqlite"),
        Arc::new(BarrierVision::barrier(analysis_vision_text)),
    );
    organize(&server, &owner, false).await?;
    let owner_id = server.access_gallery()?.gallery_id;
    organize(&server, &active, false).await?;
    let active_id = server.access_gallery()?.gallery_id;
    let film = owner.join("film.mp4");
    media_fixture(&film).await?;
    server
        .gallery_add_media(Parameters(GalleryAddMediaRequest {
            path: film.to_string_lossy().into_owned(),
            media_type: "video".into(),
            width: None,
            height: None,
        }))
        .await?;
    assert_eq!(server.gallery_store.count_assets(&owner_id)?, 1);
    assert_eq!(server.gallery_store.count_assets(&active_id)?, 0);
    Ok(())
}

/// expect: A failed reconciliation rolls back physical changes and missing transitions together. [P1]
#[tokio::test]
async fn reconciliation_rolls_back_on_injected_insert_failure() -> TestResult {
    let fixture = crate::test_support::contained_tempdir()?;
    let root = fixture.path().join("root");
    std::fs::create_dir(&root)?;
    png(&root.join("a.png"), 1);
    png(&root.join("b.png"), 2);
    let server = server(
        &fixture.path().join("gallery.sqlite"),
        Arc::new(BarrierVision::barrier(analysis_vision_text)),
    );
    organize(&server, &root, true).await?;
    let gallery = server.access_gallery()?;
    let before = server
        .gallery_store
        .list_assets(&gallery.gallery_id, 0, 10)?;
    png(&root.join("a.png"), 3);
    std::fs::remove_file(root.join("b.png"))?;
    png(&root.join("z.png"), 4);
    server.gallery_store.driver().execute_batch("CREATE TRIGGER fail_gallery_insert BEFORE INSERT ON gallery_images WHEN NEW.relative_path = 'z.png' BEGIN SELECT RAISE(ABORT, 'injected failure'); END;")?;
    assert!(server.rescan_gallery(&gallery, true).is_err());
    let after = server
        .gallery_store
        .list_assets(&gallery.gallery_id, 0, 10)?;
    assert_eq!(serde_json::to_value(before)?, serde_json::to_value(after)?);
    Ok(())
}

/// The shared vision test double: `vision_response` supplies the
/// per-test `generate_vision` text. The `barrier` variant parks the
/// first call on `entered`/`resume` so a test can switch gallery state
/// mid-analysis.
struct StubVisionPort {
    entered: tokio::sync::Notify,
    resume: tokio::sync::Notify,
    calls: std::sync::atomic::AtomicUsize,
    barrier: bool,
    vision_response: fn(&str) -> String,
}

impl StubVisionPort {
    fn new(vision_response: fn(&str) -> String) -> Self {
        Self {
            entered: tokio::sync::Notify::new(),
            resume: tokio::sync::Notify::new(),
            calls: std::sync::atomic::AtomicUsize::new(0),
            barrier: false,
            vision_response,
        }
    }

    /// The first-call barrier variant — `BarrierVision` at the call sites.
    fn barrier(vision_response: fn(&str) -> String) -> Self {
        Self {
            barrier: true,
            ..Self::new(vision_response)
        }
    }
}

/// The barrier vision port: parks the first analysis call so a test can
/// switch gallery state mid-inference.
type BarrierVision = StubVisionPort;

/// Well-formed analysis responses: empty detections, a neutral palette,
/// a centered composition, and a plain caption.
fn analysis_vision_text(prompt: &str) -> String {
    if prompt.contains("Return ONLY a JSON array") {
        "[]".to_string()
    } else if prompt.contains("color palette") && prompt.contains("JSON object") {
        r#"{"colors":[],"palette_style":"neutral","temperature":"balanced","saturation":"muted"}"#
            .to_string()
    } else if prompt.contains("JSON object") {
        r#"{"focal_point":"center","rule_of_thirds":"centered","leading_lines":"none","depth_of_field":"deep","perspective":"eye level","framing":"none","symmetry":"balanced","negative_space":"none"}"#.to_string()
    } else {
        "Test caption".to_string()
    }
}

/// Structurally invalid analysis responses: a missing colors field, an
/// empty composition object, and a blank caption — while the empty
/// object list is a legitimate detection result.
fn invalid_analysis_vision_text(prompt: &str) -> String {
    if prompt.contains("Return ONLY a JSON array") {
        "[]".to_string()
    } else if prompt.contains("color palette") && prompt.contains("JSON object") {
        "{}".to_string()
    } else if prompt.contains("JSON object") {
        "{}".to_string()
    } else {
        "   ".to_string()
    }
}

crate::test_support::media_inference_stub! {
    StubVisionPort,
    generate(self, _prompt): {
        Box::pin(async {
            Err(hkask_types::InferenceError::NotConfigured(
                "test only supports vision".into(),
            ))
        })
    },
    generate_vision(self, prompt, images): {
        assert!(!images.is_empty());
        let text = (self.vision_response)(prompt);
        Box::pin(async move {
            if self.barrier && self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0
            {
                self.entered.notify_one();
                self.resume.notified().await;
            }
            Ok(hkask_types::InferenceResult {
                text,
                model: "test-vision".into(),
                usage: Default::default(),
                finish_reason: "stop".into(),
                tool_calls: vec![],
                reasoning: None,
            })
        })
    }
}

/// expect: Switching galleries during vision never tags the new root or retargets later indices. [P1]
#[tokio::test]
async fn analysis_is_bound_before_await_and_rejects_changed_revision() -> TestResult {
    let (fixture, first, second) = two_created_dir_roots()?;
    for name in ["a.png", "b.png"] {
        png(&first.join(name), 1);
        png(&second.join(name), 2);
    }
    let vision = Arc::new(BarrierVision::barrier(analysis_vision_text));
    let server = Arc::new(server(
        &fixture.path().join("gallery.sqlite"),
        vision.clone(),
    ));
    organize(&server, &first, true).await?;
    let first_gallery = server.access_gallery()?;
    let records = server
        .gallery_store
        .list_assets(&first_gallery.gallery_id, 0, 10)?;
    let task = {
        let server = server.clone();
        tokio::spawn(async move {
            server
                .run_analysis_on_indices(&first_gallery, &[0, 1], &["scene".into()])
                .await
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), vision.entered.notified()).await?;
    organize(&server, &second, true).await?;
    let second_gallery = server.access_gallery()?;
    vision.resume.notify_one();
    let (count, errors) = task.await?;
    assert_eq!(count, 2, "{errors:?}");
    assert!(errors.is_empty());
    for record in &records {
        assert_eq!(server.gallery_store.get_tags(&record.id)?.len(), 1);
    }
    assert!(
        server
            .gallery_store
            .get_all_tags(&second_gallery.gallery_id)?
            .is_empty()
    );

    let vision = Arc::new(BarrierVision::barrier(analysis_vision_text));
    let changed_server = Arc::new(self::server(
        &fixture.path().join("gallery.sqlite"),
        vision.clone(),
    ));
    organize(&changed_server, &first, true).await?;
    let first_gallery = changed_server.access_gallery()?;
    let task = {
        let server = changed_server.clone();
        tokio::spawn(async move {
            server
                .run_analysis_on_indices(&first_gallery, &[0], &["scene".into()])
                .await
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), vision.entered.notified()).await?;
    png(Path::new(&records[0].absolute_path), 9);
    organize(&changed_server, &first, true).await?;
    vision.resume.notify_one();
    let (count, errors) = task.await?;
    assert_eq!(count, 0);
    assert!(!errors.is_empty());
    assert!(
        server
            .gallery_store
            .get_by_id(&records[0].id)?
            .metadata_stale
    );
    Ok(())
}
/// The stale-image analysis fixture shared by the reanalysis tests:
/// one png organized, replaced (making its metadata stale), and
/// re-organized — then a full four-pipeline analysis on index 0,
/// returning the server, the stale image record, and the analysis
/// (count, errors). Both tests carried this setup before the helper
/// (2026-10-09 ratchet pass); the vision port is the varying input.
async fn stale_image_analysis(
    vision: Arc<dyn InferencePort>,
) -> Result<
    (
        tempfile::TempDir,
        MediaServer,
        crate::gallery::ImageRecord,
        (u32, Vec<String>),
    ),
    Box<dyn std::error::Error>,
> {
    let fixture = crate::test_support::contained_tempdir()?;
    let root = fixture.path().join("root");
    std::fs::create_dir(&root)?;
    png(&root.join("a.png"), 1);
    let server = server(&fixture.path().join("gallery.sqlite"), vision);
    organize(&server, &root, true).await?;
    png(&root.join("a.png"), 2);
    organize(&server, &root, true).await?;
    let gallery = server.access_gallery()?;
    let image = server
        .gallery_store
        .get_image(&gallery.gallery_id, Some(0), None)?;
    assert!(image.metadata_stale);
    let (count, errors) = server
        .run_analysis_on_indices(
            &gallery,
            &[0],
            &[
                "objects".into(),
                "colors".into(),
                "composition".into(),
                "scene".into(),
            ],
        )
        .await;
    Ok((fixture, server, image, (count, errors)))
}

/// expect: Successful complete reanalysis clears staleness only for the analyzed revision. [P1]
#[tokio::test]
async fn complete_reanalysis_refreshes_matching_revision() -> TestResult {
    let vision = Arc::new(BarrierVision::barrier(analysis_vision_text));
    vision.resume.notify_one();
    let (_fixture, server, image, (count, errors)) = stale_image_analysis(vision).await?;
    assert_eq!(count, 1, "{errors:?}");
    assert!(errors.is_empty());
    assert!(!server.gallery_store.get_by_id(&image.id)?.metadata_stale);
    assert!(
        server
            .gallery_store
            .get_tags(&image.id)?
            .iter()
            .any(|tag| tag.value == "Test caption")
    );
    Ok(())
}

/// expect: Structurally invalid analysis output never certifies freshness. [P1]
#[tokio::test]
async fn invalid_analysis_outputs_retain_staleness() -> TestResult {
    let (_fixture, server, image, (count, errors)) =
        stale_image_analysis(Arc::new(StubVisionPort::new(invalid_analysis_vision_text))).await?;
    assert_eq!(count, 0, "nothing was certified");
    assert!(!errors.is_empty(), "invalid shapes must surface as errors");
    assert!(
        errors.iter().any(|error| error.contains("no colors array")),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("none of the recognized fields")),
        "{errors:?}"
    );
    assert!(
        errors.iter().any(|error| error.contains("blank")),
        "{errors:?}"
    );
    // Empty face/object detections are legitimate — they must not error.
    assert!(
        !errors
            .iter()
            .any(|error| error.contains("object detection")),
        "{errors:?}"
    );
    assert!(server.gallery_store.get_by_id(&image.id)?.metadata_stale);
    Ok(())
}
