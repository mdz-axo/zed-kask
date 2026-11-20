//! Integration tests — the GalleryStore + GalleryState pipeline and collage
//! composition logic. Inference-dependent tools require a running LLM backend
//! and are tested via the MCP protocol in live sessions.
use crate::gallery::GalleryState;
use crate::gallery::{GalleryMode, GalleryStore};
use image::{Rgb, RgbImage};
use std::sync::Arc;
use tempfile::TempDir;

fn setup_store() -> (Arc<GalleryStore>, TempDir) {
    let temp = TempDir::new().expect("tempdir");
    let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
    let store = Arc::new(GalleryStore::from_driver(driver).expect("gallery store init"));
    (store, temp)
}

fn create_test_image(dir: &std::path::Path, name: &str, r: u8, g: u8, b: u8) {
    let img: RgbImage = RgbImage::from_pixel(64, 64, Rgb([r, g, b]));
    img.save(dir.join(name)).expect("save test image");
}

#[test]
fn gallery_lifecycle_init_to_search() {
    let (store, temp) = setup_store();

    create_test_image(temp.path(), "sunset.jpg", 255, 100, 50);
    create_test_image(temp.path(), "ocean.jpg", 50, 100, 255);
    create_test_image(temp.path(), "forest.png", 34, 139, 34);

    let gallery = store
        .open(
            &temp.path().to_string_lossy(),
            crate::gallery::GalleryMode::ReadOnly,
        )
        .expect("create gallery");
    assert_eq!(gallery.image_count, 0);

    let state = GalleryState::new(temp.path().to_path_buf(), GalleryMode::ReadOnly);
    let scan = state.scan(false, None);
    assert_eq!(scan.entries.len(), 3);

    for entry in &scan.entries {
        store
            .add_image(
                &gallery.id,
                &temp.path().join(&entry.absolute_path).to_string_lossy(),
                &entry.hash,
                entry.width,
                entry.height,
                &entry.format,
                entry.size_bytes,
            )
            .expect("add image");
    }

    let img = store
        .get_image(&gallery.id, Some(0), None)
        .expect("get image");
    assert_eq!(img.width, 64);

    store
        .tag_image(&img.id, "object", "sunset", 0.95, "test")
        .expect("tag");

    let tags = store.get_tags(&img.id).expect("get tags");
    assert_eq!(tags.len(), 1);

    let all_tags = store.get_all_tags(&gallery.id).expect("get all tags");
    assert!(!all_tags.is_empty());
    assert!(all_tags.iter().any(|(t, _)| t.value == "sunset"));
}

#[test]
fn collage_compose_grid_layout() {
    let temp = TempDir::new().expect("tempdir");

    let images: Vec<image::DynamicImage> = vec![
        image::DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([255u8, 0, 0]))),
        image::DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([0, 255u8, 0]))),
        image::DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([0, 0, 255u8]))),
        image::DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([255u8, 255u8, 0]))),
    ];

    let spacing: u32 = 8;
    let canvas_w: u32 = 800;
    let canvas_h: u32 = 600;
    let cols = (images.len() as f64).sqrt().ceil() as u32;
    let rows = (images.len() as u32).div_ceil(cols);
    assert_eq!(cols, 2);
    assert_eq!(rows, 2);

    let canvas = crate::tools::processing::compose_collage_grid(
        &images, cols, rows, canvas_w, canvas_h, spacing,
    );

    let output_path = temp.path().join("collage_test.png");
    canvas.save(&output_path).expect("save collage");
    let collage = image::open(&output_path).expect("reopen");
    assert_eq!(collage.width(), 800);
    assert_eq!(collage.height(), 600);

    let non_bg = collage
        .to_rgba8()
        .pixels()
        .filter(|p| p.0 != [30, 30, 30, 255])
        .count();
    assert!(
        non_bg > 100,
        "collage should have non-bg pixels (got {})",
        non_bg
    );
}

#[test]
fn gallery_store_image_not_found() {
    let (store, temp) = setup_store();
    let gallery = store
        .open(
            &temp.path().to_string_lossy(),
            crate::gallery::GalleryMode::ReadOnly,
        )
        .expect("create gallery");

    assert!(store.get_image(&gallery.id, Some(999), None).is_err());
    assert!(
        store
            .get_image(&gallery.id, None, Some("nonexistent"))
            .is_err()
    );
}

#[test]
fn gallery_three_state_policy() {
    use crate::gallery::GalleryMode;
    assert_eq!(GalleryMode::ReadOnly.as_str(), "read-only");
    assert_eq!(GalleryMode::CopyOnWrite.as_str(), "copy-on-write");
    assert_eq!(GalleryMode::Destructive.as_str(), "destructive");
    assert_ne!(
        GalleryMode::ReadOnly.as_str(),
        GalleryMode::Destructive.as_str()
    );
}

// ── Face recognition tests ─────────────────────────────────────────────

#[test]
fn face_validation_deserialize_pass() {
    let json = r#"{
            "valid": true,
            "face_count": 1,
            "face_coverage_pct": 45,
            "pose": "frontal",
            "lighting": "good",
            "occlusion": "none",
            "clarity": "sharp",
            "issues": []
        }"#;
    let result: crate::gallery::vision::FaceValidationResult =
        serde_json::from_str(json).expect("deserialize");
    assert!(result.valid);
    assert_eq!(result.face_count, 1);
    assert_eq!(result.face_coverage_pct, 45);
    assert_eq!(result.pose, "frontal");
    assert!(result.issues.is_empty());
}

#[test]
fn face_validation_deserialize_reject() {
    let json = r#"{
            "valid": false,
            "face_count": 2,
            "face_coverage_pct": 10,
            "pose": "profile",
            "lighting": "poor",
            "occlusion": "significant",
            "clarity": "blurry",
            "issues": [
                "Multiple faces detected (2) — reference must contain exactly 1 face",
                "Face coverage too low (10%) — minimum 15% required",
                "Profile pose — frontal or near-frontal required"
            ]
        }"#;
    let result: crate::gallery::vision::FaceValidationResult =
        serde_json::from_str(json).expect("deserialize");
    assert!(!result.valid);
    assert_eq!(result.face_count, 2);
    assert_eq!(result.issues.len(), 3);
    assert!(result.issues[0].contains("Multiple faces"));
}

#[test]
fn face_match_deserialize_match() {
    let json = r#"{
            "match": true,
            "confidence": 0.94,
            "reasoning": "Same bone structure, identical eye spacing, matching nose shape."
        }"#;
    let result: crate::gallery::vision::FaceMatchResult =
        serde_json::from_str(json).expect("deserialize");
    assert!(result.is_match);
    assert!((result.confidence - 0.94).abs() < 0.001);
    assert!(result.reasoning.contains("bone structure"));
}

#[test]
fn face_match_deserialize_no_match() {
    let json = r#"{
            "match": false,
            "confidence": 0.85,
            "reasoning": "Different jawline structure and eye shape — likely different people."
        }"#;
    let result: crate::gallery::vision::FaceMatchResult =
        serde_json::from_str(json).expect("deserialize");
    assert!(!result.is_match);
    assert!((result.confidence - 0.85).abs() < 0.001);
    assert!(result.reasoning.contains("Different"));
}

#[test]
fn face_registry_lifecycle() {
    let (store, _temp) = setup_store();

    // Create a gallery and image for the face reference
    let gallery = store
        .open(
            tempfile::tempdir()
                .expect("root")
                .path()
                .to_str()
                .expect("UTF-8 root"),
            GalleryMode::ReadOnly,
        )
        .expect("create gallery");
    let img = store
        .add_image(
            &gallery.id,
            "/tmp/test-gallery/alice.jpg",
            "hash1",
            400,
            600,
            "jpg",
            50000,
        )
        .expect("add image");

    // Register a face
    let face = store
        .register_face("Alice", "Chen", &img.id, "valid", "Frontal portrait")
        .expect("register face");
    assert_eq!(face.first_name, "Alice");
    assert_eq!(face.status, "valid");

    // List faces
    let faces = store.list_faces(None).expect("list faces");
    assert_eq!(faces.len(), 1);

    // Get by ID
    let retrieved = store.get_face(&face.id).expect("get face");
    assert_eq!(retrieved.last_name, "Chen");

    // Remove
    store.remove_face(&face.id).expect("remove face");
    let faces = store.list_faces(None).expect("list after remove");
    assert_eq!(faces.len(), 0);
}

#[test]
fn face_registry_status_filter() {
    let (store, _temp) = setup_store();
    let gallery = store
        .open(
            tempfile::tempdir()
                .expect("root")
                .path()
                .to_str()
                .expect("UTF-8 root"),
            GalleryMode::ReadOnly,
        )
        .expect("create gallery");
    let img1 = store
        .add_image(&gallery.id, "/tmp/a.jpg", "h1", 100, 100, "jpg", 1000)
        .expect("add img1");
    let img2 = store
        .add_image(&gallery.id, "/tmp/b.jpg", "h2", 100, 100, "jpg", 1000)
        .expect("add img2");

    store
        .register_face("Alice", "A", &img1.id, "valid", "")
        .unwrap();
    store
        .register_face("Bob", "B", &img2.id, "rejected", "Too dark")
        .unwrap();

    let valid = store.list_faces(Some("valid")).unwrap();
    assert_eq!(valid.len(), 1);
    assert_eq!(valid[0].first_name, "Alice");

    let rejected = store.list_faces(Some("rejected")).unwrap();
    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0].first_name, "Bob");
}

#[test]
fn historical_gallery_lineage_round_trip() {
    let (store, temp) = setup_store();
    let gallery = store
        .open(&temp.path().to_string_lossy(), GalleryMode::ReadOnly)
        .unwrap();
    create_test_image(temp.path(), "gen.png", 10, 20, 30);
    let img = store
        .add_image(
            &gallery.id,
            temp.path().join("gen.png").to_str().unwrap(),
            "hash-gen",
            64,
            64,
            "png",
            1024,
        )
        .unwrap();

    // Historical generation metadata stays readable even though the
    // creative handlers that originally produced it have been retired.
    let wf = store
        .record_workflow("{\"nodes\":[],\"parallel\":false}")
        .unwrap();
    let params_json = serde_json::json!({ "size": "1024x1024" }).to_string();
    let record = store
        .record_generation(
            &img.id,
            "generate_image",
            Some("a serene mountain landscape"),
            Some("DeepInfra/black-forest-labs/FLUX-2-klein-4b"),
            Some("deepinfra"),
            Some(12345),
            Some(&params_json),
            Some(&wf.id),
            None,
        )
        .unwrap();
    assert_eq!(record.op, "generate_image");

    // Re-read the lineage (what gallery_lineage returns).
    let lineage = store
        .get_generation(&img.id)
        .unwrap()
        .expect("lineage should be recorded");
    assert_eq!(lineage.op, "generate_image");
    assert_eq!(
        lineage.prompt.as_deref(),
        Some("a serene mountain landscape")
    );
    assert_eq!(
        lineage.model.as_deref(),
        Some("DeepInfra/black-forest-labs/FLUX-2-klein-4b")
    );
    assert_eq!(lineage.provider.as_deref(), Some("deepinfra"));
    assert_eq!(lineage.seed, Some(12345));
    assert_eq!(lineage.workflow_id.as_deref(), Some(wf.id.as_str()));
    // Historical parameters round-trip without re-executing their retired operation.
    let replay: serde_json::Value =
        serde_json::from_str(lineage.params.as_deref().unwrap()).unwrap();
    assert_eq!(replay["size"], "1024x1024");

    // No lineage for an unrelated image.
    create_test_image(temp.path(), "other.png", 1, 2, 3);
    let other = store
        .add_image(
            &gallery.id,
            temp.path().join("other.png").to_str().unwrap(),
            "hash-other",
            64,
            64,
            "png",
            1024,
        )
        .unwrap();
    assert!(store.get_generation(&other.id).unwrap().is_none());
}
