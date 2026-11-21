use super::*;
use crate::test_fixtures::scheduled_job;

/// Invoker-global tests serialize: mock invokers install into a
/// process-global, and two installers racing under parallel test
/// execution cross-answer each other's calls (observed live
/// 2026-10-25: the pagination test recorded a job_list call). One
/// installer at a time — the same pattern as the widget crate's
/// audio-device GLOBAL_TEST_LOCK.
static INVOKER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn hint(body: &str) -> String {
    format!("```media\n{body}\n```")
}

/// dcterms:identifier: `MediaViewer::activate_tab`
/// expect: Leaving the workspace suspends the shared player before the hidden mode renders.
/// [P1] Motivating: Panel navigation cannot leave ghost audio or playback polling behind.
#[gpui::test]
fn leaving_workspace_suspends_retained_player(cx: &mut gpui::TestAppContext) {
    // The lock: entering Library dispatches `gallery_list_assets`, and
    // an unlocked dispatch can land in a concurrent lock-holder's
    // scripted invoker (the cross-talk the lock exists to prevent).
    let _invoker_guard = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let widget = cx.new(|cx| {
        hkask_media_widget::MediaWidget::new(
            hkask_media_widget::MediaRef::new(
                "/definitely/missing.png".into(),
                hkask_media_widget::MediaKind::Image,
            ),
            cx,
        )
    });
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, _cx| {
        viewer.media_widget = Some(widget.clone());
        viewer.media_widget_body = Some("asset-body".to_string());
        viewer.active_tab = ViewerTab::Workspace;
    });

    viewer.update(cx, |viewer, cx| viewer.activate_tab(ViewerTab::Library, cx));

    assert!(widget.read_with(cx, |widget, _cx| widget.is_suspended()));
    assert_eq!(
        viewer.read_with(cx, |viewer, _cx| viewer.active_tab),
        ViewerTab::Library
    );
}

/// dcterms:identifier: `MediaViewer::toggle_inspector`
/// expect: Toggling the inspector pane never clears the selection, the
/// retained player, or its body, and never suspends the player — the
/// pane only changes what renders (T1 criterion 2).
/// [P1] Motivating: the workspace keeps the object being worked on.
#[gpui::test]
fn inspector_toggle_preserves_selection_and_player(cx: &mut gpui::TestAppContext) {
    let widget = cx.new(|cx| {
        hkask_media_widget::MediaWidget::new(
            hkask_media_widget::MediaRef::new(
                "/definitely/missing.png".into(),
                hkask_media_widget::MediaKind::Image,
            ),
            cx,
        )
    });
    let viewer = cx.new(|_| {
        let mut viewer = MediaViewer::new();
        // A surfaced (unindexed) asset: the toggle pin must not dispatch —
        // this test installs no invoker and holds no lock, so any
        // inspector read would cross-wire into the process-global
        // invoker of whichever lock-holding test is running.
        viewer.assets = vec![surfaced_asset("/a.png")];
        viewer.selected = Some(0);
        viewer.active_tab = ViewerTab::Workspace;
        viewer
    });
    viewer.update(cx, |viewer, _cx| {
        viewer.media_widget = Some(widget.clone());
        viewer.media_widget_body = Some("asset-0-body".to_string());
    });

    viewer.update(cx, |viewer, cx| viewer.toggle_inspector(cx));
    viewer.read_with(cx, |viewer, _cx| {
        assert!(!viewer.inspector_open, "the first toggle closes the pane");
        assert_eq!(viewer.selected, Some(0));
        assert!(viewer.media_widget.is_some(), "the player is retained");
        assert_eq!(viewer.media_widget_body.as_deref(), Some("asset-0-body"));
    });

    viewer.update(cx, |viewer, cx| viewer.toggle_inspector(cx));
    viewer.read_with(cx, |viewer, _cx| {
        assert!(viewer.inspector_open, "the second toggle reopens the pane");
        assert_eq!(viewer.selected, Some(0));
        assert!(viewer.media_widget.is_some());
        assert_eq!(viewer.media_widget_body.as_deref(), Some("asset-0-body"));
    });
    assert!(
        !widget.read_with(cx, |widget, _cx| widget.is_suspended()),
        "the pane toggle never suspends the player"
    );
}

/// dcterms:identifier: `MediaViewer::load_inspector`
/// expect: The inspector's reads (detail, transcript links, annotations)
/// fire on workspace entry with the pane visible and on a selection change
/// under the open pane, never with the pane closed or from another mode —
/// the old Detail-tab activation contract, now the pane's.
/// [P1] Motivating: the inspector follows the selection without a
/// separate destination to visit.
#[gpui::test]
fn inspector_loads_follow_selection_and_pane_visibility(cx: &mut gpui::TestAppContext) {
    let _invoker_guard = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (calls, _restore) = install_scripted_invoker(vec![
        r#"{"content":{"image":{"src":"/a.png"},"tags":[]}}"#.to_string(),
        r#"{"content":[]}"#.to_string(),
        r#"{"content":[]}"#.to_string(),
    ]);
    let viewer = cx.new(|_| {
        let mut viewer = MediaViewer::new();
        viewer.assets = vec![indexed_asset("/a.png", 0)];
        viewer.active_tab = ViewerTab::Library;
        viewer
    });
    // Selecting from the Library loads nothing while the Library is the
    // active mode; entering the workspace with the pane open fires the
    // inspector's three reads for the selected asset.
    viewer.update(cx, |viewer, cx| {
        viewer.set_selection(Some(0), cx);
    });
    assert_eq!(
        calls.lock().expect("calls lock").len(),
        0,
        "no inspector reads from the Library mode"
    );
    viewer.update(cx, |viewer, cx| {
        viewer.activate_tab(ViewerTab::Workspace, cx);
    });
    let recorded = calls.lock().expect("calls lock").clone();
    assert_eq!(
        recorded
            .iter()
            .map(|(tool, _)| tool.as_str())
            .collect::<Vec<_>>(),
        vec![
            "gallery_asset_detail",
            "educt_list_transcripts",
            "gallery_annotation_list"
        ],
        "the workspace entry fires the inspector's trio"
    );
    assert_eq!(recorded[0].1, serde_json::json!({ "image_id": "asset-0" }));

    // A selection change under the closed pane loads nothing; reopening
    // the pane loads the reads for the NEW selection.
    viewer.update(cx, |viewer, cx| viewer.toggle_inspector(cx));
    viewer.update(cx, |viewer, _cx| {
        viewer.assets.push(indexed_asset("/b.png", 1));
    });
    viewer.update(cx, |viewer, cx| viewer.set_selection(Some(1), cx));
    assert_eq!(
        calls.lock().expect("calls lock").len(),
        3,
        "a closed pane loads nothing on selection change"
    );
    viewer.update(cx, |viewer, cx| viewer.toggle_inspector(cx));
    let reopened = calls.lock().expect("calls lock").clone();
    assert_eq!(
        reopened[3..]
            .iter()
            .map(|(tool, _)| tool.as_str())
            .collect::<Vec<_>>(),
        vec![
            "gallery_asset_detail",
            "educt_list_transcripts",
            "gallery_annotation_list"
        ],
        "reopening the pane refreshes the reads for the new selection"
    );
    assert_eq!(reopened[3].1, serde_json::json!({ "image_id": "asset-1" }));
}

/// expect: [P7] MediaViewer::ingest_tool_result makes new artifacts visible
/// without an unrelated render; replaying an existing artifact stays quiet.
#[gpui::test]
fn ingest_tool_result_notifies_observers_only_for_new_assets(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| MediaViewer::new());
    let notifications = std::rc::Rc::new(std::cell::Cell::new(0));
    let _subscription = cx.update(|cx| {
        let notifications = notifications.clone();
        cx.observe(&viewer, move |_, _| {
            notifications.set(notifications.get() + 1);
        })
    });
    cx.run_until_parked();
    assert_eq!(notifications.get(), 0);

    for (index, source) in ["/tmp/structured.png", "/tmp/text.png"]
        .into_iter()
        .enumerate()
    {
        let output = hkask_media::media_block::enrich_with_omc_and_provenance(
            serde_json::json!({"output": source}),
            "image_create_collage",
            "image",
            serde_json::json!({}),
            None,
        );
        let output = if index == 0 {
            output
        } else {
            Value::String(output.to_string())
        };
        viewer.update(cx, |viewer, cx| {
            viewer.ingest_tool_result(&output, "image_create_collage", cx)
        });
        cx.run_until_parked();
        assert_eq!(
            notifications.get(),
            index + 1,
            "new artifact must notify GPUI observers"
        );

        viewer.update(cx, |viewer, cx| {
            viewer.ingest_tool_result(&output, "image_create_collage", cx)
        });
        cx.run_until_parked();
        assert_eq!(
            notifications.get(),
            index + 1,
            "duplicate must not request another redraw"
        );
    }
    viewer.update(cx, |viewer, cx| {
        viewer.ingest_tool_result(&serde_json::json!({"status": "ok"}), "gallery_status", cx);
    });
    cx.run_until_parked();
    assert_eq!(
        notifications.get(),
        2,
        "hint-free output does not change the view"
    );
}

/// expect: [P7] The public viewer ingress accepts both live transports,
/// retains metadata, selects new assets, and does not duplicate replays.
#[gpui::test]
fn ingest_tool_result_accepts_structured_and_text_transports(cx: &mut gpui::TestAppContext) {
    let output = hkask_media::media_block::enrich_with_omc_and_provenance(
        serde_json::json!({"output": "/tmp/雪/quote\"-back\\slash\nimage.png"}),
        "image_create_collage",
        "image",
        serde_json::json!({"prompt": "snow"}),
        None,
    );
    let wrapped = serde_json::json!({"content": output});
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.detail = Some(serde_json::json!({"stale": true}));
        viewer.ingest_tool_result(&wrapped, "image_create_collage", cx);
        assert_eq!(viewer.assets.len(), 1);
        assert_eq!(viewer.selected, Some(0));
        assert!(viewer.detail.is_none());
        viewer.ingest_tool_result(
            &Value::String(wrapped.to_string()),
            "image_create_collage",
            cx,
        );
        viewer.ingest_tool_result(&output, "image_create_collage", cx);
        assert_eq!(viewer.assets.len(), 1);
        let second = hkask_media::media_block::media_block("video", "/tmp/second.mp4");
        viewer.ingest_tool_result(
            &serde_json::json!({"display_hints": [second]}),
            "video_clip",
            cx,
        );
        assert_eq!(viewer.selected, Some(1));
        assert_eq!(viewer.assets[1].src, "/tmp/second.mp4");
    });
}

struct JobListInvoker(String);

impl hkask_tool_invoker::ToolInvoker for JobListInvoker {
    fn invoke_tool(
        &self,
        server: &str,
        tool: &str,
        args: Value,
    ) -> gpui::Task<Result<String, hkask_tool_invoker::InvokeError>> {
        assert_eq!(server, MEDIA_SERVER);
        assert_eq!(tool, "job_list");
        assert_eq!(
            args,
            serde_json::json!({
                "limit": hkask_media::media_limits::DEFAULT_JOB_LIST_LIMIT
            })
        );
        gpui::Task::ready(Ok(self.0.clone()))
    }
}

/// expect: [P7] Queue refresh distinguishes an empty queue from a broken
/// response, and preserves the last good rows when refresh fails.
#[gpui::test]
fn load_jobs_surfaces_strict_rows_and_response_failures(cx: &mut gpui::TestAppContext) {
    let _invoker_guard = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _restore = InvokerRestore(hkask_tool_invoker::shared_tool_invoker());
    let record = hkask_media::jobs_contract::JobRecord {
        id: "job-1".into(),
        op: "transcribe".into(),
        status: "running".into(),
        created_at: "2026-09-05T00:00:00Z".into(),
        completed_at: None,
        result: None,
        error: None,
        source: None,
    };
    let viewer = cx.new(|_| MediaViewer::new());
    let response = serde_json::json!({"content": {
        "jobs": [record],
        "total": 1,
        "limit": hkask_media::media_limits::DEFAULT_JOB_LIST_LIMIT,
        "has_more": false,
        "history_scope": hkask_media::jobs_contract::JOB_HISTORY_SCOPE,
        "restart_behavior": hkask_media::jobs_contract::JOB_RESTART_BEHAVIOR
    }})
    .to_string();
    hkask_tool_invoker::set_tool_invoker(Some(std::sync::Arc::new(JobListInvoker(response))));
    viewer.update(cx, |viewer, cx| viewer.load_jobs(cx));
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert_eq!(viewer.jobs.len(), 1);
        assert_eq!(viewer.jobs[0].id, "job-1");
        assert_eq!(viewer.jobs_request.state, ResourceState::Ready);
    });
    for response in [
        "not json",
        "{}",
        r#"{"content":[{"id":"broken"}]}"#,
        r#"{"content":{"jobs":[]}}"#,
        r#"{"content":[]}"#,
        r#"{"error":"job store unavailable","kind":"unavailable"}"#,
    ] {
        hkask_tool_invoker::set_tool_invoker(Some(std::sync::Arc::new(JobListInvoker(
            response.into(),
        ))));
        viewer.update(cx, |viewer, cx| viewer.load_jobs(cx));
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert_eq!(viewer.jobs.len(), 1, "failed refresh preserves known jobs");
            let ResourceState::Failed(status) = &viewer.jobs_request.state else {
                panic!("queue failure must be visible");
            };
            assert!(status.contains("job_list"));
            if response.contains("job store unavailable") {
                assert!(status.contains("job store unavailable"));
            }
        });
    }
}

#[test]
fn asset_from_hint_parses_kind_and_src() {
    let asset = asset_from_hint(
        &hint(r#"{"kind":"video","src":"/tmp/out.mp4"}"#),
        &"video_fetch".into(),
    )
    .expect("parses");
    assert_eq!(asset.kind, "video");
    assert_eq!(asset.src, "/tmp/out.mp4");
    assert_eq!(asset.tool, "video_fetch");
    assert!(asset.body.starts_with('{'));
    assert!(asset.gallery_index.is_none());
}

/// expect: [P7] Retiring a producer must not erase historical media provenance.
#[test]
fn historical_creative_hint_remains_readable() {
    let body = r#"{"kind":"image","src":"/tmp/historical.png","gallery_asset_id":"old-asset","ontology":"omc:CreativeWork","provenance":{"tool":"generate_image","args":{"prompt":"historical"}}}"#;
    let asset = asset_from_hint(&hint(body), &"generate_image".into())
        .expect("historical producer is not an execution request");
    assert_eq!(asset.gallery_asset_id.as_deref(), Some("old-asset"));
    assert_eq!(asset.body, body);
    assert_eq!(asset.tool, "generate_image");
}

/// expect: [P7] Invalid media never becomes a selectable phantom asset.
#[test]
fn asset_from_hint_rejects_missing_src_and_unsupported_kind() {
    for body in [
        r#"{"kind":"image"}"#,
        r#"{"kind":"hologram","src":"/tmp/x"}"#,
    ] {
        assert!(asset_from_hint(&hint(body), &"image_create_collage".into()).is_none());
    }
}

/// expect: [P7] Server-authored hints preserve paths and provenance in both consumers.
#[test]
fn server_hint_round_trips_through_viewer_and_widget() -> anyhow::Result<()> {
    let source = "/tmp/雪/quote\"-back\\slash\nimage.png";
    for kind in ["image", "svg", "video", "audio"] {
        let output = hkask_media::media_block::enrich_with_omc_and_provenance(
            serde_json::json!({"output": source, "gallery_asset_id": "asset-1"}),
            "image_create_collage",
            kind,
            serde_json::json!({"prompt": "雪\n\""}),
            Some("span-1".into()),
        );
        let hint = output["display_hint"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing hint"))?;
        let asset = asset_from_hint(hint, &"image_create_collage".into())
            .ok_or_else(|| anyhow::anyhow!("missing asset"))?;
        let body = hkask_media_widget::MediaBlockBody::parse(&asset.body)?;
        assert_eq!(asset.src, source);
        assert_eq!(body.to_media_ref()?.src(), source);
        assert_eq!(body.kind, kind);
        assert_eq!(body.ontology.as_deref(), Some("omc:CreativeWork"));
        assert_eq!(body.gallery_asset_id.as_deref(), Some("asset-1"));
        assert_eq!(asset.gallery_asset_id.as_deref(), Some("asset-1"));
        assert_eq!(
            body.provenance.tool.as_deref(),
            Some("image_create_collage")
        );
        assert_eq!(body.provenance.span_id.as_deref(), Some("span-1"));
        assert_eq!(
            body.provenance.args,
            serde_json::json!({"prompt": "雪\n\""})
        );
        // Additional metadata stays in the original body handed to the renderer.
        let mut extended: Value = serde_json::from_str(&asset.body)?;
        extended["width"] = serde_json::json!(640);
        extended["height"] = serde_json::json!(480);
        extended["caption"] = serde_json::json!("雪\n\"");
        let extended_hint = self::hint(&extended.to_string());
        let asset = asset_from_hint(&extended_hint, &"image_create_collage".into())
            .ok_or_else(|| anyhow::anyhow!("extended body rejected"))?;
        assert_eq!(serde_json::from_str::<Value>(&asset.body)?, extended);
    }
    Ok(())
}

#[test]
fn asset_from_hint_rejects_non_media_fences_and_bad_json() {
    assert!(asset_from_hint("```rust\nfn main() {}\n```", &"t".into()).is_none());
    assert!(asset_from_hint("```media\nnot json\n```", &"t".into()).is_none());
    assert!(asset_from_hint("plain text", &"t".into()).is_none());
}

fn indexed_asset(src: &str, index: usize) -> MediaAsset {
    MediaAsset {
        body: format!(r#"{{"kind":"image","src":"{src}"}}"#),
        src: src.into(),
        kind: "image".into(),
        tool: "gallery".into(),
        gallery_index: Some(index),
        gallery_asset_id: Some(format!("asset-{index}")),
    }
}

fn surfaced_asset(src: &str) -> MediaAsset {
    surfaced_asset_of_kind(src, "image")
}

fn surfaced_asset_of_kind(src: &str, kind: &str) -> MediaAsset {
    MediaAsset {
        body: serde_json::json!({"kind": kind, "src": src}).to_string(),
        src: src.into(),
        kind: kind.into(),
        tool: "image_create_collage".into(),
        gallery_index: None,
        gallery_asset_id: None,
    }
}

fn job(status: &str) -> JobRecord {
    JobRecord {
        id: format!("job-{status}"),
        op: "video_clip".into(),
        status: status.into(),
        created_at: "2026-09-14T00:00:00Z".into(),
        completed_at: None,
        result: None,
        error: None,
        source: None,
    }
}

/// A strict `job_list` response envelope for the scripted invoker —
/// the same contract shape `parse_job_list_response` validates.
fn jobs_listing_response(jobs: &[JobRecord], total: usize, has_more: bool) -> String {
    serde_json::json!({
        "content": {
            "jobs": jobs,
            "total": total,
            "limit": hkask_media::media_limits::DEFAULT_JOB_LIST_LIMIT,
            "has_more": has_more,
            "history_scope": hkask_media::jobs_contract::JOB_HISTORY_SCOPE,
            "restart_behavior": hkask_media::jobs_contract::JOB_RESTART_BEHAVIOR
        }
    })
    .to_string()
}

/// The confirmed-delete bug this pins: deleting in the Library reloads
/// the listing, but the merge used to only add and update — the deleted
/// asset's row never left the list, so a confirmed removal appeared to
/// do nothing. The merge must reconcile: in-page absences are
/// deletions; unindexed (conversation-surfaced) and out-of-page assets
/// stay; survivors' indexes follow the positional shift; a
/// conversation-surfaced asset for an indexed file gains its index
/// (listing indices are the gallery image_index contract — server:
/// `index = offset + i`).
#[test]
fn merge_gallery_records_merges_by_src_and_reconciles_deletions() {
    let mut assets = vec![
        surfaced_asset("/gallery/keep-a.png"),
        indexed_asset("/gallery/deleted.png", 1),
        indexed_asset("/gallery/keep-b.png", 2),
        surfaced_asset("/tmp/chat-only.png"),
        indexed_asset("/gallery/out-of-page.png", 150),
    ];
    // The fresh page (offset 0): deleted.png is gone; keep-b shifted
    // from index 2 to 1.
    let records = serde_json::json!([
        {"id": "asset-a", "index": 0, "path": "/gallery/keep-a.png", "media_type": "image"},
        {"id": "asset-b", "index": 1, "path": "/gallery/keep-b.png", "media_type": "image"},
    ]);
    merge_gallery_records(&mut assets, records.as_array().unwrap(), 0, None);

    let srcs: Vec<&str> = assets.iter().map(|a| a.src.as_str()).collect();
    assert_eq!(
        srcs,
        vec![
            "/gallery/keep-a.png",
            "/gallery/keep-b.png",
            "/tmp/chat-only.png",
            "/gallery/out-of-page.png",
        ],
        "the deleted in-page asset must drop; unindexed and out-of-page assets stay"
    );
    let keep_a = assets.first().expect("keep-a survives");
    assert_eq!(
        keep_a.gallery_index,
        Some(0),
        "a conversation-surfaced asset for an indexed file gains its index"
    );
    assert_eq!(keep_a.gallery_asset_id.as_deref(), Some("asset-a"));
    assert_eq!(
        hkask_media_widget::MediaBlockBody::parse(&keep_a.body)
            .expect("reconciled body stays renderable")
            .gallery_asset_id
            .as_deref(),
        Some("asset-a"),
        "the renderer body must carry the same stable Asset identity as the panel row"
    );
    assert_eq!(
        assets
            .iter()
            .find(|a| a.src == "/gallery/keep-b.png")
            .expect("keep-b survives")
            .gallery_index,
        Some(1),
        "a survivor's index follows the positional shift"
    );
}

/// A page fetched at a non-zero offset vouches only for its own window:
/// an asset on an earlier page absent from THIS page must survive —
/// only an in-window absence is a deletion.
#[test]
fn merge_gallery_records_reconciles_only_the_fetched_window() {
    let mut assets = vec![
        indexed_asset("/gallery/early.png", 5),
        indexed_asset("/gallery/in-window-deleted.png", 100),
        indexed_asset("/gallery/in-window-kept.png", 101),
    ];
    // Page at offset 100: one record — in-window-kept shifted down to
    // 100 because in-window-deleted was removed.
    let records = serde_json::json!([
        {"index": 100, "path": "/gallery/in-window-kept.png", "media_type": "image"},
    ]);
    merge_gallery_records(&mut assets, records.as_array().unwrap(), 100, None);

    let srcs: Vec<&str> = assets.iter().map(|a| a.src.as_str()).collect();
    assert_eq!(
        srcs,
        vec!["/gallery/early.png", "/gallery/in-window-kept.png"],
        "only in-window absences are deletions; earlier-page assets survive"
    );
}

/// expect: [P1] Empty and shortened terminal pages remove obsolete gallery
/// rows without erasing conversation artifacts.
#[gpui::test]
fn gallery_terminal_pages_remove_exhausted_tail(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
            viewer.assets = vec![
                indexed_asset("/gallery/keep.png", 0),
                indexed_asset("/gallery/tail.png", 1),
                surfaced_asset("/tmp/history.png"),
            ];
            viewer.merge_gallery_listing(Some(serde_json::json!({
                "gallery_id": "gallery-a", "total": 1, "offset": 0, "limit": 100,
                "assets": [{"id": "/gallery/keep.png", "index": 0, "path": "/gallery/keep.png", "media_type": "image", "missing": false, "metadata_stale": false}]
            })), cx);
            assert_eq!(viewer.assets.len(), 2, "exhausted tail must disappear");
            viewer.merge_gallery_listing(Some(serde_json::json!({
                "gallery_id": "gallery-a", "total": 0, "offset": 0, "limit": 100, "assets": []
            })), cx);
            assert_eq!(viewer.assets.len(), 1);
            assert_eq!(viewer.assets[0].src, "/tmp/history.png");
        });
}

/// expect: [P1] A different gallery invalidates old selection and pending
/// deletion, even when the next root uses the same positional index.
#[gpui::test]
fn gallery_root_change_clears_selection_and_confirmation(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
            for gallery in ["a", "b"] {
                viewer.merge_gallery_listing(Some(serde_json::json!({
                    "gallery_id": gallery, "total": 1, "offset": 0, "limit": 100,
                    "assets": [{"id": gallery, "index": 0, "path": format!("/{gallery}.png"), "media_type": "image", "missing": false, "metadata_stale": false}]
                })), cx);
                if gallery == "a" {
                    viewer.selected = Some(0);
                    viewer.confirm_delete.confirm(&0);
                    viewer.detail = Some(serde_json::json!({"old": true}));
                }
            }
            assert_eq!(viewer.assets.len(), 1, "old gallery rows are invalid");
            assert!(viewer.selected.is_none());
            assert!(viewer.confirm_delete.armed.is_none());
            assert!(viewer.detail.is_none());
        });
}

/// expect: [P1] A delayed response for a superseded listing request —
/// e.g. gallery A's slow response arriving after the panel already
/// holds gallery B — is dropped, never merged over the current view.
#[gpui::test]
fn delayed_listing_response_is_dropped(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
            // Two listing requests were issued; the second owns the resource.
            let stale_epoch = viewer
                .gallery_request
                .begin(RequestOwner::GalleryPage(0))
                .expect("first request starts");
            let current_epoch = viewer
                .gallery_request
                .begin(RequestOwner::GalleryPage(GALLERY_PAGE_LIMIT))
                .expect("different page supersedes it");
            viewer.apply_gallery_listing(
                current_epoch,
                parse_gallery_listing(
                    Some(serde_json::json!({
                        "gallery_id": "gallery-b", "total": 101, "offset": 100, "limit": 100,
                        "assets": [{"id": "b100", "index": 100, "path": "/b.png", "media_type": "image", "missing": false, "metadata_stale": false}]
                    })),
                    GALLERY_PAGE_LIMIT,
                ),
                cx,
            );
            assert_eq!(viewer.assets.len(), 1);
            assert_eq!(viewer.assets[0].src, "/b.png");
            assert_eq!(viewer.gallery_id.as_deref(), Some("gallery-b"));
            // The superseded request's delayed response (gallery A, epoch 1)
            // must not overwrite the view or retarget the gallery.
            viewer.apply_gallery_listing(
                stale_epoch,
                parse_gallery_listing(
                    Some(serde_json::json!({
                        "gallery_id": "gallery-a", "total": 1, "offset": 0, "limit": 100,
                        "assets": [{"id": "a0", "index": 0, "path": "/a.png", "media_type": "image", "missing": false, "metadata_stale": false}]
                    })),
                    0,
                ),
                cx,
            );
            assert_eq!(viewer.assets.len(), 1, "delayed response must be dropped");
            assert_eq!(viewer.assets[0].src, "/b.png");
            assert_eq!(viewer.gallery_id.as_deref(), Some("gallery-b"));
        });
}

/// expect: [P1] After a root switch, no stale A-gallery row survives to
/// act on B: the confirmed deletion is invalidated and no row carries
/// A's stable asset id — a delete addressed by that id cannot hit B.
#[gpui::test]
fn stale_gallery_rows_cannot_act_after_root_change(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
            viewer.merge_gallery_listing(
                Some(serde_json::json!({
                    "gallery_id": "gallery-a", "total": 1, "offset": 0, "limit": 100,
                    "assets": [{"id": "a0", "index": 0, "path": "/a.png", "media_type": "image", "missing": false, "metadata_stale": false}]
                })),
                cx,
            );
            viewer.confirm_delete.confirm(&0); // pending deletion of A's row
            viewer.merge_gallery_listing(
                Some(serde_json::json!({
                    "gallery_id": "gallery-b", "total": 1, "offset": 0, "limit": 100,
                    "assets": [{"id": "b0", "index": 0, "path": "/b.png", "media_type": "image", "missing": false, "metadata_stale": false}]
                })),
                cx,
            );
            // The switch invalidated the pending action and every A row.
            assert!(viewer.confirm_delete.armed.is_none(),
                "a confirmation captured in A must not act on B");
            assert!(viewer.assets.iter().all(|asset| asset.src != "/a.png"),
                "A's rows are gone");
            // B's row carries B's stable id — a delete addressed at A's id
            // finds no target in the panel, and the server resolves ids
            // against the active gallery, so it cannot hit B either.
            assert!(viewer
                .assets
                .iter()
                .all(|asset| asset.gallery_asset_id.as_deref() != Some("a0")));
            assert_eq!(
                viewer.assets[0].gallery_asset_id.as_deref(),
                Some("b0"),
                "actions address B's rows by B's stable id"
            );
        });
}

/// dcterms:identifier: `tool_result_changed_gallery`
/// expect: Every publisher family reports mutation from its completed result, while reads do not.
/// [P7] Motivating: Library freshness follows observed state changes rather than tool-name guesses.
#[test]
fn gallery_freshness_is_result_derived_across_publisher_families() {
    for tool in [
        "image_create_collage",
        "video_clip",
        "audio_capture",
        "educt_render_edl",
        "video_fetch",
        "gallery_add_media",
        "gallery_delete_image",
    ] {
        let output = serde_json::json!({
            "content": {
                "gallery_changed": true,
                "gallery_asset_id": format!("asset-{tool}")
            }
        });
        assert!(
            tool_result_changed_gallery(&output),
            "publisher missed: {tool}"
        );
    }

    for output in [
        serde_json::json!({"content":{"gallery_changed":false}}),
        serde_json::json!({"content":{"jobs":[],"total":0}}),
        serde_json::json!({"content":{"display_hints":["```media\n{\"kind\":\"image\",\"src\":\"/existing.png\",\"gallery_asset_id\":\"existing\"}\n```"]}}),
        serde_json::json!({"error":{"code":"unavailable","message":"read failed"}}),
    ] {
        assert!(!tool_result_changed_gallery(&output));
    }
}

/// Selection and delete-confirmation repair across a reconciling
/// merge: gallery indexes are positional, so after a deletion the old
/// indices point at different assets. The selection must follow its
/// asset (or clear with its detail when the asset left), and a pending
/// confirmation must follow its asset — a stale confirm index would
/// delete the WRONG asset on confirm.
#[gpui::test]
fn merge_gallery_listing_repairs_selection_and_confirm(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, _| {
        viewer.assets = vec![
            indexed_asset("/gallery/doomed.png", 0),
            indexed_asset("/gallery/survivor.png", 1),
        ];
        viewer.selected = Some(0); // the asset about to be deleted
        viewer.confirm_delete.confirm(&1); // the survivor, pre-shift
    });
    let payload = serde_json::json!({
        "gallery_id": "gallery-a",
        "total": 1,
        "offset": 0,
        "limit": GALLERY_PAGE_LIMIT,
        "assets": [
            {"id": "survivor", "index": 0, "path": "/gallery/survivor.png", "media_type": "image"},
        ]
    });
    viewer.update(cx, |viewer, cx| {
        viewer.merge_gallery_listing(Some(payload), cx)
    });
    viewer.update(cx, |viewer, _| {
        assert_eq!(
            viewer.selected,
            Some(0),
            "the deleted selection falls back to the first asset"
        );
        assert!(
            viewer.detail.is_none(),
            "the deleted asset's detail is cleared"
        );
        assert_eq!(
            viewer.confirm_delete.armed,
            Some(0),
            "the pending confirmation follows its asset to the shifted index"
        );
    });
}

/// dcterms:identifier: `MediaViewer::apply_gallery_listing`
/// expect: A malformed latest page and a stale earlier failure cannot alter a last-good Library snapshot.
/// [P1] Motivating: Library pagination is an atomic latest-owner transition.
#[gpui::test]
fn gallery_failure_is_atomic_and_stale_failure_is_ignored(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
            viewer.assets = vec![indexed_asset("/good.png", 0)];
            viewer.gallery_total = Some(1);
            viewer.gallery_offset = 0;
            viewer.gallery_limit = GALLERY_PAGE_LIMIT;
            viewer.gallery_id = Some("gallery-good".into());
            viewer.selected = Some(0);
            let before = (
                viewer.assets.clone(),
                viewer.gallery_total,
                viewer.gallery_offset,
                viewer.gallery_limit,
                viewer.gallery_id.clone(),
                viewer.selected,
            );
            let malformed_epoch = viewer
                .gallery_request
                .begin(RequestOwner::GalleryPage(GALLERY_PAGE_LIMIT))
                .expect("page request starts");
            let malformed = parse_gallery_listing(
                Some(serde_json::json!({
                    "gallery_id": "gallery-good", "total": 2, "offset": 100, "limit": 100,
                    "assets": [{"id": "broken", "index": 100, "media_type": "image"}]
                })),
                GALLERY_PAGE_LIMIT,
            );
            viewer.apply_gallery_listing(malformed_epoch, malformed, cx);
            assert_eq!(
                (
                    viewer.assets.clone(),
                    viewer.gallery_total,
                    viewer.gallery_offset,
                    viewer.gallery_limit,
                    viewer.gallery_id.clone(),
                    viewer.selected,
                ),
                before,
                "every last-good field survives malformed refresh"
            );
            assert!(matches!(
                viewer.gallery_request.state,
                ResourceState::Failed(_)
            ));

            let stale_epoch = viewer
                .gallery_request
                .begin(RequestOwner::GalleryPage(0))
                .expect("refresh starts");
            let current_epoch = viewer
                .gallery_request
                .begin(RequestOwner::GalleryPage(GALLERY_PAGE_LIMIT))
                .expect("next page supersedes refresh");
            viewer.apply_gallery_listing(
                current_epoch,
                parse_gallery_listing(
                    Some(serde_json::json!({
                        "gallery_id": "gallery-good", "total": 101, "offset": 100, "limit": 100,
                        "assets": [{"id": "latest", "index": 100, "path": "/latest.png", "media_type": "image"}]
                    })),
                    GALLERY_PAGE_LIMIT,
                ),
                cx,
            );
            viewer.apply_gallery_listing(stale_epoch, Err("stale failure".into()), cx);
            assert_eq!(viewer.gallery_request.state, ResourceState::Ready);
            assert!(viewer.assets.iter().any(|asset| asset.src == "/latest.png"));
        });
}

/// dcterms:identifier: `RequestLifecycle::begin`
/// expect: Repeated clicks for the same loading resource remain single-flight.
/// [P1] Motivating: refresh controls cannot fan out duplicate work.
#[test]
fn repeated_loading_owner_is_single_flight() {
    let mut request = RequestLifecycle::new();
    assert!(request.begin(RequestOwner::Jobs).is_some());
    assert!(request.begin(RequestOwner::Jobs).is_none());
    assert!(
        request
            .begin(RequestOwner::GalleryPage(GALLERY_PAGE_LIMIT))
            .is_some()
    );
}

/// dcterms:identifier: `MediaViewer::should_poll_jobs`
/// expect: The activity poll arms while any nonterminal job is observed — on any mode, drawer closed — and stops only at complete quiescence or a listing failure; a superseded epoch never mutates state.
/// [P1] Motivating: background queue work is bounded by observed demand, never by page visibility (T2).
#[gpui::test]
fn activity_polling_arms_on_any_live_job_and_stops_only_at_quiescence(
    cx: &mut gpui::TestAppContext,
) {
    // The lock: the tab switches below dispatch the modes' loads
    // (`gallery_list_assets`, `educt_list_transcripts`) through the
    // process-global invoker — unlocked they cross-wire into a
    // concurrent lock-holder's scripted invoker.
    let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        // Ambient from any mode: Library is active and the drawer is
        // closed — no jobs surface is visible (visiting the retired
        // Queue tab was the old visibility condition).
        viewer.active_tab = ViewerTab::Library;
        let epoch = viewer
            .jobs_request
            .begin(RequestOwner::Jobs)
            .expect("starts");
        viewer.apply_jobs_listing(
            epoch,
            Ok(JobListing {
                jobs: vec![job("running")],
                total: 1,
                limit: hkask_media::media_limits::DEFAULT_JOB_LIST_LIMIT,
                has_more: false,
                history_scope: hkask_media::jobs_contract::JOB_HISTORY_SCOPE.into(),
                restart_behavior: hkask_media::jobs_contract::JOB_RESTART_BEHAVIOR.into(),
            }),
            cx,
        );
        assert!(
            viewer.queue_poll_task.is_some(),
            "a live job arms the poll with no jobs surface visible"
        );
        // Mode switches never cancel observation — the poll condition
        // is the observed live set, not the visible page.
        viewer.activate_tab(ViewerTab::Workspace, cx);
        assert!(
            viewer.queue_poll_task.is_some(),
            "leaving a mode does not cancel the timer"
        );
        viewer.activate_tab(ViewerTab::Library, cx);
        assert!(
            viewer.queue_poll_task.is_some(),
            "entering a mode does not cancel the timer"
        );
        // Closing the drawer never cancels observation either.
        viewer.activity_open = true;
        viewer.toggle_activity(cx);
        assert!(!viewer.activity_open);
        assert!(
            viewer.queue_poll_task.is_some(),
            "closing the drawer does not cancel the timer"
        );

        // Complete quiescence stops the poll: the page covers the
        // whole bounded store (has_more false) and holds no live job.
        let quiescent_epoch = viewer
            .jobs_request
            .begin(RequestOwner::Jobs)
            .expect("starts");
        viewer.apply_jobs_listing(
            quiescent_epoch,
            Ok(JobListing {
                jobs: vec![job("completed")],
                total: 1,
                limit: hkask_media::media_limits::DEFAULT_JOB_LIST_LIMIT,
                has_more: false,
                history_scope: hkask_media::jobs_contract::JOB_HISTORY_SCOPE.into(),
                restart_behavior: hkask_media::jobs_contract::JOB_RESTART_BEHAVIOR.into(),
            }),
            cx,
        );
        assert!(
            viewer.queue_poll_task.is_none(),
            "complete quiescence stops the poll"
        );
        assert!(viewer.live_jobs.is_empty());

        // A superseded epoch's outcome is dropped; the owned error
        // stops the poll and preserves the last-good rows.
        let running_epoch = viewer
            .jobs_request
            .begin(RequestOwner::Jobs)
            .expect("starts");
        viewer.apply_jobs_listing(
            running_epoch,
            Ok(JobListing {
                jobs: vec![job("running")],
                total: 1,
                limit: hkask_media::media_limits::DEFAULT_JOB_LIST_LIMIT,
                has_more: false,
                history_scope: hkask_media::jobs_contract::JOB_HISTORY_SCOPE.into(),
                restart_behavior: hkask_media::jobs_contract::JOB_RESTART_BEHAVIOR.into(),
            }),
            cx,
        );
        assert!(viewer.queue_poll_task.is_some());
        let superseded_epoch = viewer.jobs_request.epoch;
        let error_epoch = viewer
            .jobs_request
            .begin(RequestOwner::Jobs)
            .expect("supersedes");
        assert_ne!(error_epoch, superseded_epoch);
        viewer.apply_jobs_listing(superseded_epoch, Err("stale queue error".into()), cx);
        assert_eq!(
            viewer.jobs.len(),
            1,
            "a superseded epoch's outcome is dropped"
        );
        viewer.apply_jobs_listing(error_epoch, Err("queue unavailable".into()), cx);
        assert!(viewer.queue_poll_task.is_none(), "error cancels timer");
        assert_eq!(viewer.jobs.len(), 1, "error preserves last-good jobs");
    });
}

/// dcterms:identifier: `MediaViewer::apply_jobs_listing`
/// expect: A completed background publisher becomes one visible asset exactly once.
/// [P7] Motivating: job completion and foreground publication share one result ingress.
#[gpui::test]
fn completed_job_result_is_ingested_once(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
            // Library mode suppresses the inspector trio the ingest's
            // auto-select would otherwise fire (the retired Queue tab's old
            // role in this setup) — this test holds no invoker lock, so any
            // dispatch would cross-wire into a concurrent lock-holder's
            // scripted invoker.
            viewer.active_tab = ViewerTab::Library;
            let result = serde_json::json!({
                "gallery_changed": true,
                "gallery_asset_id": "job-asset",
                "display_hint": "```media\n{\"kind\":\"image\",\"src\":\"/missing-job.png\",\"gallery_asset_id\":\"job-asset\"}\n```"
            });
            let mut completed = job("completed");
            completed.id = "job-1".into();
            completed.result = Some(result);
            for _ in 0..2 {
                let epoch = viewer
                    .jobs_request
                    .begin(RequestOwner::Jobs)
                    .expect("listing starts");
                viewer.apply_jobs_listing(
                    epoch,
                    Ok(JobListing {
                        jobs: vec![completed.clone()],
                        total: 1,
                        limit: hkask_media::media_limits::DEFAULT_JOB_LIST_LIMIT,
                        has_more: false,
                        history_scope: hkask_media::jobs_contract::JOB_HISTORY_SCOPE.into(),
                        restart_behavior: hkask_media::jobs_contract::JOB_RESTART_BEHAVIOR.into(),
                    }),
                    cx,
                );
            }
            assert_eq!(
                viewer
                    .assets
                    .iter()
                    .filter(|asset| asset.gallery_asset_id.as_deref() == Some("job-asset"))
                    .count(),
                1
            );
        });
}

/// dcterms:identifier: `MediaViewer::observe_live_jobs`
/// expect: A nonterminal job deeper than the newest-first page — behind 20 newer completed jobs — stays observed (the poll stays armed) and its landing updates the live set (loop-register L11, the falsifier T2 settles).
/// [P1] Motivating: page visibility never bounds job observation.
#[gpui::test]
fn deep_running_job_behind_a_full_page_stays_observed_and_updated(cx: &mut gpui::TestAppContext) {
    let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let limit = hkask_media::media_limits::DEFAULT_JOB_LIST_LIMIT;
    // The deep job: older than everything on the page (the L11 shape).
    let mut deep = scheduled_job("job-deep", "transcribe", "running", "2026-09-01T00:00:00Z");
    deep.source = Some("/talk.wav".into());
    // Twenty newer completed jobs fill the page.
    let newer: Vec<JobRecord> = (0..limit)
        .map(|index| {
            scheduled_job(
                &format!("job-new-{index}"),
                "video_clip",
                "completed",
                "2026-09-02T00:00:00Z",
            )
        })
        .collect();
    let page = jobs_listing_response(&newer, limit + 1, true);
    let probe_empty = jobs_listing_response(&[], 0, false);
    let probe_deep = jobs_listing_response(&[deep], 1, false);
    // Tick 1: the unfiltered page plus the three status probes
    // (queued, running, cancelling — the dispatch order); the running
    // probe finds the deep job. Tick 2 (the poll's next link): the
    // deep job landed — every probe is empty.
    let (calls, _restore) = install_scripted_invoker(vec![
        page.clone(),
        probe_empty.clone(),
        probe_deep,
        probe_empty.clone(),
        page,
        probe_empty.clone(),
        probe_empty.clone(),
        probe_empty,
    ]);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        // No jobs surface is visible: the Workspace mode with no open
        // transcript, drawer closed — the observation must be ambient,
        // not visited. (The Library is the recordings section's home
        // since T3 — a transcribe landing THERE would also reload the
        // stored-transcript listing, a dispatch this test's scripted
        // queue does not carry; the workspace shows no such surface.)
        viewer.active_tab = ViewerTab::Workspace;
        viewer.load_jobs(cx);
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert_eq!(
            viewer.jobs.len(),
            limit,
            "the page holds only the newest jobs"
        );
        assert!(
            !viewer.jobs.iter().any(|job| job.id == "job-deep"),
            "the deep job is beyond the page — that is the L11 shape"
        );
        assert!(
            viewer.live_jobs.iter().any(|job| job.id == "job-deep"),
            "the status probes observe the deep running job"
        );
        assert_eq!(viewer.live_jobs.len(), 1);
        assert!(
            viewer.queue_poll_task.is_some(),
            "the poll stays armed while a live job is observed anywhere"
        );
        assert_eq!(
            viewer.transcribing_ids,
            ["job-deep".to_string()].into_iter().collect::<HashSet<_>>(),
            "deep transcribe jobs feed the R5 landing tracking"
        );
        assert_eq!(
            viewer.jobs_restart_behavior.as_deref(),
            Some(hkask_media::jobs_contract::JOB_RESTART_BEHAVIOR),
            "the drawer's disclosure copy is sourced from the validated listing fields"
        );
        assert_eq!(
            viewer.jobs_history_scope.as_deref(),
            Some(hkask_media::jobs_contract::JOB_HISTORY_SCOPE)
        );
    });
    // The dispatch shape: the page asks unfiltered, then one
    // status-filtered `job_list` per nonterminal status — the server
    // filters BEFORE the newest-first sort and limit, which is exactly
    // what observes beyond the page.
    {
        let calls = calls.lock().expect("calls lock");
        let status_args: Vec<Value> = calls
            .iter()
            .map(|(_, args)| args.get("status").cloned().unwrap_or(Value::Null))
            .collect();
        assert_eq!(
            status_args,
            vec![
                Value::Null,
                Value::String("queued".into()),
                Value::String("running".into()),
                Value::String("cancelling".into()),
            ],
            "one tick: the page, then one status probe per nonterminal status"
        );
    }
    // Tick 2 — the poll's next link observes the landing: the deep job
    // is gone from every probe, so the live set empties, the R5
    // tracking updates, and the poll stops.
    viewer.update(cx, |viewer, cx| viewer.load_jobs(cx));
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert!(
            viewer.live_jobs.is_empty(),
            "the landing of a job beyond the page is observed and updated"
        );
        assert!(viewer.transcribing_ids.is_empty());
        assert!(
            viewer.queue_poll_task.is_none(),
            "the poll stops when no live job remains anywhere"
        );
    });
    // The second tick repeated the same dispatch shape.
    {
        let calls = calls.lock().expect("calls lock");
        assert_eq!(
            calls.len(),
            8,
            "two ticks of the page plus three probes each"
        );
    }
}

/// dcterms:identifier: `MediaViewer::settle_activity_tick`
/// expect: A transcribe job landing anywhere in the store reloads the
/// stored-transcript listing exactly once while the Library — the
/// recordings browser's home since T3 — is showing, so the finished
/// transcript appears under its recording (R5 continuity through the
/// T2 poll restructure) — and not while another mode is showing.
/// [P7] Motivating: the finished transcript appears under its recording.
#[gpui::test]
fn transcribe_landing_reloads_the_recordings_listing(cx: &mut gpui::TestAppContext) {
    let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let transcript_listing = r#"{"content": {"transcripts": [], "count": 0}}"#.to_string();
    let (calls, _restore) = install_scripted_invoker(vec![transcript_listing]);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript_listing = Some(vec![]);
        let running_listing = JobListing {
            jobs: vec![scheduled_job(
                "job-r5",
                "transcribe",
                "running",
                "2026-09-03T00:00:00Z",
            )],
            total: 1,
            limit: hkask_media::media_limits::DEFAULT_JOB_LIST_LIMIT,
            has_more: false,
            history_scope: hkask_media::jobs_contract::JOB_HISTORY_SCOPE.into(),
            restart_behavior: hkask_media::jobs_contract::JOB_RESTART_BEHAVIOR.into(),
        };
        // Landing away from the Library (the Workspace mode is showing)
        // does not reload the recordings section's listing.
        viewer.active_tab = ViewerTab::Workspace;
        let epoch = viewer
            .jobs_request
            .begin(RequestOwner::Jobs)
            .expect("starts");
        viewer.apply_jobs_listing(epoch, Ok(running_listing.clone()), cx);
        assert_eq!(
            viewer.transcribing_ids,
            ["job-r5".to_string()].into_iter().collect::<HashSet<_>>()
        );
        let landed_epoch = viewer
            .jobs_request
            .begin(RequestOwner::Jobs)
            .expect("starts");
        let mut landed_listing = running_listing.clone();
        landed_listing.jobs = vec![scheduled_job(
            "job-r5",
            "transcribe",
            "completed",
            "2026-09-03T00:00:00Z",
        )];
        viewer.apply_jobs_listing(landed_epoch, Ok(landed_listing), cx);
        assert!(
            viewer.transcribing_ids.is_empty(),
            "the landing is observed from the live set"
        );
        assert_eq!(
            viewer.transcript_listing_request.state,
            ResourceState::Idle,
            "no reload while the Workspace mode is showing"
        );

        // Landing while the Library is showing reloads its recordings
        // section once — the finished transcript appears under its
        // recording (R5).
        viewer.active_tab = ViewerTab::Library;
        let epoch = viewer
            .jobs_request
            .begin(RequestOwner::Jobs)
            .expect("restarts");
        viewer.apply_jobs_listing(epoch, Ok(running_listing), cx);
        let landed_epoch = viewer
            .jobs_request
            .begin(RequestOwner::Jobs)
            .expect("starts");
        let landed_listing = JobListing {
            jobs: vec![scheduled_job(
                "job-r5",
                "transcribe",
                "completed",
                "2026-09-03T00:00:00Z",
            )],
            total: 1,
            limit: hkask_media::media_limits::DEFAULT_JOB_LIST_LIMIT,
            has_more: false,
            history_scope: hkask_media::jobs_contract::JOB_HISTORY_SCOPE.into(),
            restart_behavior: hkask_media::jobs_contract::JOB_RESTART_BEHAVIOR.into(),
        };
        viewer.apply_jobs_listing(landed_epoch, Ok(landed_listing), cx);
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert_eq!(
            viewer.transcript_listing_request.state,
            ResourceState::Ready,
            "the landing reloaded the recordings listing (R5)"
        );
        assert_eq!(
            viewer.transcript_listing.as_ref().map(Vec::len),
            Some(0),
            "the reloaded listing committed"
        );
        let calls = calls.lock().expect("calls lock");
        assert_eq!(calls.len(), 1, "the reload dispatched exactly once");
        assert_eq!(calls[0].0, "educt_list_transcripts");
    });
}

/// dcterms:identifier: `MediaViewer::toggle_activity`
/// expect: Opening the activity drawer refreshes the job listing (observed on demand, never stale); closing dispatches nothing.
/// [P1] Motivating: the drawer's opening state is current, and closing is free.
#[gpui::test]
fn opening_the_activity_drawer_refreshes_the_listing(cx: &mut gpui::TestAppContext) {
    let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let page = jobs_listing_response(&[job("completed")], 1, false);
    let (calls, _restore) = install_scripted_invoker(vec![page]);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        assert!(!viewer.activity_open);
        viewer.toggle_activity(cx);
        assert!(viewer.activity_open);
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert_eq!(
            viewer.jobs_request.state,
            ResourceState::Ready,
            "opening the drawer dispatched a listing refresh"
        );
        assert_eq!(viewer.jobs.len(), 1);
    });
    let calls_before_close = calls.lock().expect("calls lock").len();
    viewer.update(cx, |viewer, cx| viewer.toggle_activity(cx));
    cx.run_until_parked();
    assert!(!viewer.read_with(cx, |viewer, _| viewer.activity_open));
    assert_eq!(
        calls.lock().expect("calls lock").len(),
        calls_before_close,
        "closing the drawer dispatches nothing"
    );
}

/// dcterms:identifier: `MediaViewer::asset_activity_verb`
/// expect: The Library badge names the live job's verb from the record's primary source, and only for the asset it targets.
/// [P1] Motivating: ambient status is accurate to the affected asset.
#[test]
fn asset_activity_verb_follows_the_primary_source() {
    let mut viewer = MediaViewer::new();
    viewer.assets = vec![surfaced_asset("/talk.wav")];
    let mut live = scheduled_job("job-live", "transcribe", "running", "2026-09-05T00:00:00Z");
    live.source = Some("/talk.wav".into());
    viewer.live_jobs = vec![live];
    assert_eq!(
        viewer.asset_activity_verb(&viewer.assets[0]),
        Some("Transcribing…")
    );
    let mut elsewhere = scheduled_job("job-live", "transcribe", "running", "2026-09-05T00:00:00Z");
    elsewhere.source = Some("/elsewhere.wav".into());
    viewer.live_jobs = vec![elsewhere];
    assert_eq!(
        viewer.asset_activity_verb(&viewer.assets[0]),
        None,
        "a job against another asset does not badge this one"
    );
    viewer.live_jobs.clear();
    assert_eq!(viewer.asset_activity_verb(&viewer.assets[0]), None);
    // The verb vocabulary covers every research op.
    for (op, verb) in [
        ("transcribe", "Transcribing…"),
        ("audio_trim", "Trimming…"),
        ("audio_concat", "Joining…"),
        ("video_concat", "Joining…"),
        ("video_clip", "Clipping…"),
        ("render_video", "Working…"),
    ] {
        assert_eq!(job_op_verb(op), verb);
    }
}

/// dcterms:identifier: `MediaViewer::apply_live_observation`
/// expect: A filter-violating probe response never grafts a terminal row onto the live set, and a failed battery retains the previously observed live entries.
/// [P1] Motivating: the live set is nonterminal by construction — retention must not immortalize a bad row.
#[gpui::test]
fn live_observation_rejects_terminal_rows_and_retains_through_failure(
    cx: &mut gpui::TestAppContext,
) {
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        // A live entry observed by a previous tick, not on the page.
        let mut deep = scheduled_job("job-deep", "transcribe", "running", "2026-09-01T00:00:00Z");
        deep.source = Some("/talk.wav".into());
        viewer.live_jobs = vec![deep.clone()];
        let epoch = viewer
            .job_observation_request
            .begin(RequestOwner::JobObservation)
            .expect("starts");
        // The probe response violates its own filter: a completed row.
        let mut terminal =
            scheduled_job("job-bad", "video_clip", "completed", "2026-09-02T00:00:00Z");
        terminal.source = Some("/clip.mp4".into());
        viewer.apply_live_observation(epoch, Ok(vec![terminal, deep]), cx);
        assert_eq!(viewer.live_jobs.len(), 1);
        assert!(
            viewer.live_jobs.iter().all(|job| job_is_nonterminal(job)),
            "the live set stays nonterminal by construction"
        );
        assert!(viewer.live_jobs.iter().any(|job| job.id == "job-deep"));

        // A failed battery retains previously observed live entries —
        // it proves nothing about deeper jobs.
        let epoch = viewer
            .job_observation_request
            .begin(RequestOwner::JobObservation)
            .expect("restarts");
        viewer.apply_live_observation(epoch, Err("probe failed".into()), cx);
        assert_eq!(
            viewer.live_jobs.len(),
            1,
            "a failed battery retains the observed live set"
        );
        assert!(
            viewer
                .status
                .as_deref()
                .is_some_and(|status| status.contains("observation failed")),
            "the battery failure is surfaced"
        );
    });
}

/// dcterms:identifier: `MediaViewer::apply_detail_result`
/// expect: A newer asset detail owns the view even if an older failure arrives later.
/// [P1] Motivating: overlapping inspector work cannot regress visible state.
#[gpui::test]
fn overlapping_detail_requests_drop_stale_errors(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        let old_detail = viewer
            .detail_request
            .begin(RequestOwner::Detail("asset-old".into()))
            .expect("old detail starts");
        let new_detail = viewer
            .detail_request
            .begin(RequestOwner::Detail("asset-new".into()))
            .expect("new detail supersedes");
        viewer.apply_detail_result(
            new_detail,
            Ok(serde_json::json!({"image": {"id": "asset-new"}})),
            cx,
        );
        viewer.apply_detail_result(old_detail, Err("stale detail error".into()), cx);
        assert_eq!(viewer.detail_request.state, ResourceState::Ready);
        assert_eq!(
            viewer
                .detail
                .as_ref()
                .and_then(|d| d.pointer("/image/id"))
                .and_then(Value::as_str),
            Some("asset-new")
        );
    });
}

/// dcterms:identifier: `MediaViewer::begin_edit`
/// expect: Trim and concat cannot overlap and hide an admitted side effect.
/// [P1] Motivating: every direct edit remains visible until completion or failure.
#[test]
fn direct_edits_are_serialized_until_terminal() {
    let mut viewer = MediaViewer::new();
    let trim_epoch = viewer.begin_edit("video_clip").expect("trim admitted");
    let error = viewer
        .begin_edit("video_concat")
        .expect_err("concat must wait for trim");
    assert!(error.contains("already in progress"));
    assert!(viewer.edit_request.owns(trim_epoch));
    assert!(viewer.edit_request.complete(trim_epoch, Ok(())));
    assert!(viewer.begin_edit("video_concat").is_ok());
}

/// dcterms:identifier: `MediaViewer::confirm_label_edit`
/// expect: Confirming a create closes the editor and surfaces the
/// dispatch outcome visibly (the unwired test invoker names itself);
/// an in-flight edit refuses a second confirm and keeps the typed
/// buffer; a blank rename refuses without dispatching; Escape cancels
/// without dispatching.
/// [P1] Motivating: the label editor never loses typed text to a silent
/// refusal, and every refusal surfaces, never swallows.
#[gpui::test]
fn label_editor_confirm_and_cancel_lifecycle(cx: &mut gpui::TestAppContext) {
    let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
    let viewer = cx.new(|_| MediaViewer::new());
    let blank_modifiers = || gpui::Modifiers {
        control: false,
        alt: false,
        shift: false,
        platform: false,
        function: false,
    };
    let key = |key: &str| Keystroke {
        modifiers: blank_modifiers(),
        key: key.into(),
        key_char: None,
    };
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(TranscriptPaneState {
            transcript_id: "t-1".into(),
            media_path: "/m".into(),
            gallery_asset_id: None,
            alignment: "original".into(),
            alignment_note: None,
            words: None,
            working_text: String::new(),
            paragraph_breaks: None,
            speaker_spans: None,
            review: None,
            layers: Vec::new(),
            highlights: Vec::new(),
            strikes: Vec::new(),
            correction_edits: Vec::new(),
            source_words: Vec::new(),
        });

        // Create confirm: the editor closes and the unwired invoker
        // refusal surfaces (the edit request completes into Failed).
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::Create {
                start_word: 3,
                end_word: 9,
            },
            buffer: LabelBuffer::from_str("climax"),
        });
        viewer.confirm_label_edit(cx);
        assert!(
            viewer.highlight_edit.is_none(),
            "confirm consumed the editor"
        );
        assert!(
            viewer
                .status
                .as_deref()
                .unwrap()
                .contains("Tool invoker not wired"),
            "status: {:?}",
            viewer.status
        );
        assert!(matches!(
            viewer.edit_request.state,
            ResourceState::Failed(_)
        ));

        // An in-flight edit refuses a second confirm and keeps the
        // typed buffer for retry.
        viewer.edit_request.state = ResourceState::Loading;
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::Create {
                start_word: 3,
                end_word: 9,
            },
            buffer: LabelBuffer::from_str("thesis"),
        });
        viewer.confirm_label_edit(cx);
        assert!(
            viewer.highlight_edit.is_some(),
            "a refused confirm keeps the typed buffer"
        );
        assert!(
            viewer
                .status
                .as_deref()
                .unwrap()
                .contains("already in progress")
        );
        assert_eq!(viewer.edit_request.state, ResourceState::Loading);
        viewer.edit_request.state = ResourceState::Idle;

        // A blank rename refuses without dispatching; the editor stays.
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::Rename {
                annotation_id: "layer-1".into(),
                start_word: 3,
                end_word: 9,
                note: "kept note".into(),
            },
            buffer: LabelBuffer::from_str("   "),
        });
        viewer.confirm_label_edit(cx);
        assert!(
            viewer.highlight_edit.is_some(),
            "the blank rename stays open"
        );
        assert!(
            viewer.status.as_deref().unwrap().contains("nonblank label"),
            "status: {:?}",
            viewer.status
        );
        assert_eq!(viewer.edit_request.state, ResourceState::Idle);

        // S5: the correction editor rides the same admission contract
        // — an in-flight edit refuses a correction confirm and keeps
        // the typed buffer for retry.
        viewer.edit_request.state = ResourceState::Loading;
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::Correction {
                start_word: 3,
                end_word: 9,
            },
            buffer: LabelBuffer::from_str("fixed words"),
        });
        viewer.confirm_label_edit(cx);
        assert!(
            viewer.highlight_edit.is_some(),
            "a refused correction keeps the typed buffer"
        );
        assert!(
            viewer
                .status
                .as_deref()
                .unwrap()
                .contains("already in progress")
        );
        assert_eq!(viewer.edit_request.state, ResourceState::Loading);
        viewer.edit_request.state = ResourceState::Idle;

        // A blank replacement refuses without dispatching — a
        // correction replaces text; deletions belong to Cut.
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::Correction {
                start_word: 3,
                end_word: 9,
            },
            buffer: LabelBuffer::from_str("   "),
        });
        viewer.confirm_label_edit(cx);
        assert!(
            viewer.highlight_edit.is_some(),
            "the blank correction stays open"
        );
        assert!(
            viewer
                .status
                .as_deref()
                .unwrap()
                .contains("needs replacement text"),
            "status: {:?}",
            viewer.status
        );
        assert_eq!(viewer.edit_request.state, ResourceState::Idle);

        // A valid rename dispatches with the note preserved (the
        // unwired invoker names the rename path).
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::Rename {
                annotation_id: "layer-1".into(),
                start_word: 3,
                end_word: 9,
                note: "kept note".into(),
            },
            buffer: LabelBuffer::from_str("climax"),
        });
        viewer.confirm_label_edit(cx);
        assert!(viewer.highlight_edit.is_none());
        assert!(
            viewer
                .status
                .as_deref()
                .unwrap()
                .contains("cannot rename highlights"),
            "status: {:?}",
            viewer.status
        );

        // Escape cancels without dispatching anything.
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::Create {
                start_word: 1,
                end_word: 2,
            },
            buffer: LabelBuffer::default(),
        });
        viewer.handle_label_key(&key("escape"), cx);
        assert!(viewer.highlight_edit.is_none());
        assert!(viewer.status.as_deref().unwrap().contains("cancelled"));
        // Escape cancels without dispatching: the rename's completed
        // edit state is untouched (no new admission, no Loading).
        assert!(
            matches!(viewer.edit_request.state, ResourceState::Failed(_)),
            "cancel must not admit or complete an edit: {:?}",
            viewer.edit_request.state
        );
    });
}

/// A minimal workspace state for the interaction tests below.
fn pane_state(transcript_id: &str) -> TranscriptPaneState {
    TranscriptPaneState {
        transcript_id: transcript_id.into(),
        media_path: "/m".into(),
        gallery_asset_id: None,
        alignment: "original".into(),
        alignment_note: None,
        words: None,
        working_text: String::new(),
        paragraph_breaks: None,
        speaker_spans: None,
        review: None,
        layers: Vec::new(),
        highlights: Vec::new(),
        strikes: Vec::new(),
        correction_edits: Vec::new(),
        source_words: Vec::new(),
    }
}

/// dcterms:identifier: `parse_transcript_payload`
/// expect: The compose flow's gather layer (model "user", template
/// workspace_reel_gather) never re-enters the highlight list — its
/// entries are bookkeeping copies of researcher annotations, and the
/// rename/delete affordances keyed to them would always be refused by
/// the server's researcher-annotation guard (2026-10-26 review).
#[test]
fn gather_layers_do_not_reenter_the_highlight_list() {
    let payload = serde_json::json!({
        "summary": {"id": "t-1", "media_path": "/m", "words_count": 2, "has_word_timings": true},
        "working_transcript": {"text": "a b", "alignment": "original", "words": [
            {"word": "a", "start_ms": 0, "end_ms": 500},
            {"word": "b", "start_ms": 500, "end_ms": 900}
        ]},
        "layers": [
            {"id": "layer-researcher", "created_at": "c1", "layer": {
                "kind": "highlight",
                "provenance": {"model": "user", "prompt_template": "researcher_annotation", "created_at": "c1"},
                "highlights": [{"start_word": 0, "end_word": 0, "label": "thesis", "note": "n"}]
            }},
            {"id": "layer-gather", "created_at": "c2", "layer": {
                "kind": "highlight",
                "provenance": {"model": "user", "prompt_template": "workspace_reel_gather", "created_at": "c2"},
                "highlights": [{"start_word": 0, "end_word": 0, "label": "thesis", "note": "gathered"}]
            }},
            {"id": "layer-model", "created_at": "c3", "layer": {
                "kind": "highlight",
                "provenance": {"model": "pass-model", "prompt_template": "semantic", "created_at": "c3"},
                "highlights": [{"start_word": 1, "end_word": 1, "label": "model-theme", "note": "n"}]
            }}
        ],
        "exports": [], "renders": []
    });
    let state = parse_transcript_payload(&payload).expect("parses");
    assert_eq!(state.highlights.len(), 2, "researcher + model rows only");
    assert!(state.highlights[0].researcher_supplied);
    assert_eq!(state.highlights[0].annotation_id, "layer-researcher");
    assert!(!state.highlights[1].researcher_supplied);
    assert_eq!(state.highlights[1].annotation_id, "layer-model");
    assert!(
        !state
            .highlights
            .iter()
            .any(|highlight| highlight.annotation_id == "layer-gather"),
        "the gather layer never re-enters the list"
    );
    // The gather layer still shows in the Layers strip — it is real
    // stored state, just not an annotation.
    assert_eq!(state.layers.len(), 3);
}

/// dcterms:identifier: `parse_transcript_payload`
/// expect: Strike layers parse under the server's strike identity only
/// (model "user", template researcher_annotation, exactly one Cut op) —
/// model EDLs, multi-op researcher EDLs, and single-Keep researcher EDLs
/// stay in the Layers strip but never render as strikes. [P1]
#[test]
fn strike_layers_parse_with_the_server_identity() {
    let payload = serde_json::json!({
        "summary": {"id": "t-1", "media_path": "/m", "words_count": 3, "has_word_timings": true},
        "working_transcript": {"text": "a b c", "alignment": "original", "words": [
            {"word": "a", "start_ms": 0, "end_ms": 300},
            {"word": "b", "start_ms": 400, "end_ms": 700},
            {"word": "c", "start_ms": 800, "end_ms": 900}
        ]},
        "layers": [
            {"id": "layer-strike", "created_at": "c1", "layer": {
                "kind": "edl",
                "provenance": {"model": "user", "prompt_template": "researcher_annotation", "created_at": "c1"},
                "ops": [{"op": "cut", "range": {"start_word": 1, "end_word": 1}}]
            }},
            {"id": "layer-model-edl", "created_at": "c2", "layer": {
                "kind": "edl",
                "provenance": {"model": "pass-model", "prompt_template": "semantic", "created_at": "c2"},
                "ops": [{"op": "cut", "range": {"start_word": 0, "end_word": 0}}]
            }},
            {"id": "layer-multi-op", "created_at": "c3", "layer": {
                "kind": "edl",
                "provenance": {"model": "user", "prompt_template": "researcher_annotation", "created_at": "c3"},
                "ops": [
                    {"op": "cut", "range": {"start_word": 0, "end_word": 0}},
                    {"op": "cut", "range": {"start_word": 2, "end_word": 2}}
                ]
            }},
            {"id": "layer-keep", "created_at": "c4", "layer": {
                "kind": "edl",
                "provenance": {"model": "user", "prompt_template": "researcher_annotation", "created_at": "c4"},
                "ops": [{"op": "keep", "range": {"start_word": 0, "end_word": 2}}]
            }}
        ],
        "exports": [], "renders": []
    });
    let state = parse_transcript_payload(&payload).expect("parses");
    assert_eq!(state.strikes.len(), 1, "only the strike identity renders");
    assert_eq!(state.strikes[0].start_word, 1);
    assert_eq!(state.strikes[0].end_word, 1);
    assert_eq!(state.strikes[0].annotation_id, "layer-strike");
    assert!(
        !state
            .strikes
            .iter()
            .any(|strike| strike.annotation_id == "layer-model-edl"
                || strike.annotation_id == "layer-multi-op"
                || strike.annotation_id == "layer-keep"),
        "non-strike EDLs never render as strikes"
    );
    assert_eq!(state.layers.len(), 4, "every EDL stays in the Layers strip");
    assert!(state.highlights.is_empty());
}

/// dcterms:identifier: `MediaViewer::strike_selected_range`
/// expect: Striking from the open label editor closes the editor and
/// dispatches the cut (kind strike) — the unwired invoker refusal
/// surfaces visibly, progress clears, and the edit request completes
/// into Failed, the same admission contract the highlight create path
/// carries. [P1]
#[gpui::test]
fn strike_from_the_label_editor_closes_and_dispatches(cx: &mut gpui::TestAppContext) {
    let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(pane_state("t-1"));
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::Create {
                start_word: 3,
                end_word: 9,
            },
            buffer: LabelBuffer::from_str("typed but struck"),
        });
        viewer.strike_selected_range(3, 9, cx);
        assert!(
            viewer.highlight_edit.is_none(),
            "the strike consumed the editor"
        );
        assert!(
            viewer
                .status
                .as_deref()
                .unwrap()
                .contains("Tool invoker not wired"),
            "status: {:?}",
            viewer.status
        );
        assert!(
            viewer.edit_progress.is_none(),
            "the refusal cleared the edit progress: {:?}",
            viewer.edit_progress
        );
        assert!(matches!(
            viewer.edit_request.state,
            ResourceState::Failed(_)
        ));
    });
}

/// dcterms:identifier: `strike_create_params`
/// expect: The strike create request carries the exact server contract
/// — kind strike with blank label and note over the selected range.
#[test]
fn strike_create_params_carry_the_exact_contract() {
    let params = strike_create_params("t-1", 34, 51);
    let object = params.as_object().expect("object");
    assert_eq!(object.len(), 6);
    assert_eq!(params["transcript_id"], serde_json::json!("t-1"));
    assert_eq!(params["start_word"], serde_json::json!(34));
    assert_eq!(params["end_word"], serde_json::json!(51));
    assert_eq!(params["label"], serde_json::json!(""));
    assert_eq!(params["note"], serde_json::json!(""));
    assert_eq!(params["kind"], serde_json::json!("strike"));
}

/// dcterms:identifier: `MediaViewer::presented_status`
/// expect: While an edit is in flight on the workspace (the transcript
/// sub-surface's compose/highlight edits share the edit lifecycle), the
/// edit progress surfaces (S5's progress-visible contract); once the
/// edit completes, the specific status message falls through as
/// before (2026-10-26 review: compose progress previously rendered
/// nowhere).
#[gpui::test]
fn workspace_surfaces_edit_progress(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.active_tab = ViewerTab::Workspace;
        viewer.edit_request.state = ResourceState::Loading;
        viewer.edit_progress = Some("composing reel from pane highlights…".into());
        let status = viewer.presented_status().map(|(text, _)| text.to_string());
        assert_eq!(
            status.as_deref(),
            Some("composing reel from pane highlights…"),
            "the compose progress is visible on the workspace"
        );
        // The edit completes: the lifecycle returns to Ready and the
        // completion message is the specific status that shows.
        viewer.edit_request.state = ResourceState::Ready;
        viewer.edit_progress = None;
        viewer.status = Some("Highlight created — the transcript reloads with it.".into());
        let status = viewer.presented_status().map(|(text, _)| text.to_string());
        assert_eq!(
            status.as_deref(),
            Some("Highlight created — the transcript reloads with it."),
            "outside an edit, the specific status message shows"
        );
        let _ = cx;
    });
}

/// dcterms:identifier: `MediaViewer::paste_into_label_edit`
/// expect: Clipboard text lands in the open editor's buffer with
/// control characters dropped; with no editor open the paste is inert
/// (2026-10-26 review: labels could not be pasted at all).
#[gpui::test]
fn label_editor_pastes_clipboard_text(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::Create {
                start_word: 0,
                end_word: 1,
            },
            buffer: LabelBuffer::from_str("climax"),
        });
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(" pasted\u{7}label ".into()));
        viewer.paste_into_label_edit(cx);
        assert_eq!(
            viewer
                .highlight_edit
                .as_ref()
                .expect("editor open")
                .buffer
                .text(),
            "climax pastedlabel ",
            "control characters drop on paste"
        );
        viewer.highlight_edit = None;
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("x".into()));
        viewer.paste_into_label_edit(cx);
        assert!(
            viewer.highlight_edit.is_none(),
            "paste is inert with no editor"
        );
    });
}

/// dcterms:identifier: `MediaViewer::close_transcript_workspace`
/// expect: Closing clears the workspace and pending interaction state
/// and returns to the plain asset workspace — the same mode (T3: the
/// recordings browser is the Library's section now, reachable by its
/// mode at any time, so the 2026-10-26 reachability concern is
/// structural). Library entry reloads the recordings section's listing
/// — the browser's data is one mode away, never unreachable; the
/// unwired-invoker refusal surfaces because the entry dispatch is
/// visible (D5). The invoker lock is required: a parallel installer
/// test's mock in the process global would answer the dispatch
/// instead (observed flaky 2026-10-26).
#[gpui::test]
fn closing_the_transcript_returns_to_the_asset_workspace(cx: &mut gpui::TestAppContext) {
    let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(pane_state("t-1"));
        viewer.selection_mode = true;
        viewer.selection_anchor = Some(2);
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::Create {
                start_word: 0,
                end_word: 1,
            },
            buffer: LabelBuffer::default(),
        });
        viewer.active_tab = ViewerTab::Workspace;
        viewer.close_transcript_workspace(cx);
        assert!(viewer.transcript.is_none(), "the workspace closed");
        assert!(viewer.selection_anchor.is_none());
        assert!(viewer.highlight_edit.is_none());
        assert!(viewer.transcript_follow_task.is_none());
        assert!(
            !viewer.selection_mode,
            "close tears down the Select-text mode with the rest of the sub-surface state \
                 (open_transcript resets it; the close path must match)"
        );
        assert_eq!(
            viewer.active_tab,
            ViewerTab::Workspace,
            "closing stays with the asset's plain workspace"
        );
        // The recordings browser is the Library's section: entering the
        // mode reloads the stored-transcript listing — the entry
        // dispatch surfaces its own unavailability when no invoker is
        // wired, proving the section's data path is live, never
        // dead-ended by the close.
        viewer.activate_tab(ViewerTab::Library, cx);
        assert!(
            matches!(
                viewer.transcript_listing_request.state,
                ResourceState::Failed(_)
            ),
            "the Library entry dispatched the recordings listing load: {:?}",
            viewer.transcript_listing_request.state
        );
    });
}

/// dcterms:identifier: `MediaViewer::load_transcript_with`
/// expect: A reload keeps the last-good workspace visible while the
/// request is in flight (no idle-browser flash after every highlight
/// edit — 2026-10-26 review); an explicit open still clears
/// immediately (2026-10-26 review). The invoker lock is required: this
/// test asserts the unwired-invoker refusal, and a parallel installer
/// test's mock in the process global would answer the dispatch
/// instead.
#[gpui::test]
fn reload_keeps_the_last_good_workspace(cx: &mut gpui::TestAppContext) {
    let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(pane_state("t-old"));
        viewer.load_transcript_with("t-old".into(), cx);
        assert!(
            viewer.transcript.is_some(),
            "the last-good workspace stays visible during the reload"
        );
        assert!(
            matches!(viewer.transcript_request.state, ResourceState::Failed(_)),
            "the unwired invoker's refusal surfaced: {:?}",
            viewer.transcript_request.state
        );
        viewer.open_transcript("t-new".into(), cx);
        assert!(
            viewer.transcript.is_none(),
            "an explicit open clears the old workspace immediately"
        );
    });
}

/// dcterms:identifier: `parse_annotation_listing`
/// expect: Annotation listings parse strictly or reject — never
/// degrade to an empty success; the kind is the closed tag/note set.
#[test]
fn annotation_listing_is_strict() {
    let valid = serde_json::json!({
        "annotations": [
            {"id": "ann-1", "tag_type": "tag", "value": "thesis", "created_at": "c1", "confidence": 1.0, "model_used": "user"},
            {"id": "ann-2", "tag_type": "note", "value": "check audio at 0:30", "created_at": "c2", "confidence": 1.0, "model_used": "user"}
        ]
    });
    let rows = parse_annotation_listing(&valid).expect("valid listing parses");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].kind, "tag");
    assert_eq!(rows[0].value, "thesis");
    assert_eq!(rows[1].kind, "note");
    for invalid in [
        r#"{"annotations": [{}]}"#,
        r#"{"annotations": [{"id": ""}]}"#,
        r#"{"annotations": [{"id": "a", "tag_type": "vibe"}]}"#,
        r#"{"annotations": [{"id": "a", "tag_type": "tag"}]}"#,
        r#"{"annotations": [{"id": "a", "tag_type": "tag", "value": ""}]}"#,
        r#"{"annotations": [{"id": "a", "tag_type": "tag", "value": "v"}]}"#,
        r#"{"annotations": ["a"]}"#,
        r#"{}"#,
    ] {
        let payload: Value = serde_json::from_str(invalid).expect("fixture json");
        assert!(
            parse_annotation_listing(&payload).is_err(),
            "malformed listing must reject: {invalid}"
        );
    }
}

/// dcterms:identifier: `annotation_create_params` / `annotation_update_params` / `annotation_delete_params`
/// expect: The three annotation dispatches carry their exact request
/// contracts.
#[test]
fn annotation_params_carry_the_exact_contract() {
    let create = annotation_create_params("asset-1", "tag", "thesis");
    let create = create.as_object().expect("object");
    assert_eq!(create.len(), 3);
    assert_eq!(create["gallery_asset_id"], serde_json::json!("asset-1"));
    assert_eq!(create["kind"], serde_json::json!("tag"));
    assert_eq!(create["value"], serde_json::json!("thesis"));

    let update = annotation_update_params("asset-1", "ann-9", "revised");
    let update = update.as_object().expect("object");
    assert_eq!(update.len(), 3);
    assert_eq!(update["gallery_asset_id"], serde_json::json!("asset-1"));
    assert_eq!(update["annotation_id"], serde_json::json!("ann-9"));
    assert_eq!(update["value"], serde_json::json!("revised"));

    let delete = annotation_delete_params("asset-1", "ann-9");
    let delete = delete.as_object().expect("object");
    assert_eq!(delete.len(), 2);
    assert_eq!(delete["annotation_id"], serde_json::json!("ann-9"));
}

/// dcterms:identifier: `MediaViewer::confirm_annotation_edit`
/// expect: Confirming a new tag closes the editor and surfaces the
/// dispatch outcome visibly (the unwired test invoker names itself);
/// a blank value refuses without dispatching; an unindexed asset
/// refuses and keeps the buffer; the replace arm dispatches over the
/// row's identity; Escape cancels without dispatching.
/// [P1] Motivating: the annotation editor never loses typed text to a
/// silent refusal, and every refusal surfaces, never swallows.
#[gpui::test]
fn annotation_editor_confirm_and_cancel_lifecycle(cx: &mut gpui::TestAppContext) {
    let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.selected = Some(0);
        viewer.assets = vec![indexed_asset("/annotation-a.png", 1)];

        // Create confirm: the editor closes and the unwired invoker
        // refusal surfaces.
        viewer.annotation_edit = Some(AnnotationEdit {
            kind: "tag",
            annotation_id: None,
            buffer: LabelBuffer::from_str("thesis"),
        });
        viewer.confirm_annotation_edit(cx);
        assert!(
            viewer.annotation_edit.is_none(),
            "confirm consumed the editor"
        );
        assert!(
            viewer
                .status
                .as_deref()
                .unwrap()
                .contains("cannot edit annotations"),
            "status: {:?}",
            viewer.status
        );
        assert!(matches!(
            viewer.edit_request.state,
            ResourceState::Failed(_)
        ));

        // A blank value refuses without dispatching; the editor stays.
        viewer.edit_request.state = ResourceState::Idle;
        viewer.annotation_edit = Some(AnnotationEdit {
            kind: "note",
            annotation_id: None,
            buffer: LabelBuffer::from_str("   "),
        });
        viewer.confirm_annotation_edit(cx);
        assert!(
            viewer.annotation_edit.is_some(),
            "the blank value stays open"
        );
        assert!(
            viewer.status.as_deref().unwrap().contains("needs text"),
            "status: {:?}",
            viewer.status
        );
        assert_eq!(viewer.edit_request.state, ResourceState::Idle);

        // The replace arm dispatches over the row's identity (the
        // unwired invoker names the annotation path again).
        viewer.annotation_edit = Some(AnnotationEdit {
            kind: "note",
            annotation_id: Some("ann-9".into()),
            buffer: LabelBuffer::from_str("revised"),
        });
        viewer.confirm_annotation_edit(cx);
        assert!(viewer.annotation_edit.is_none());
        assert!(
            viewer
                .status
                .as_deref()
                .unwrap()
                .contains("cannot edit annotations"),
            "status: {:?}",
            viewer.status
        );

        // An unindexed asset refuses and keeps the buffer.
        viewer.edit_request.state = ResourceState::Idle;
        viewer.assets = vec![surfaced_asset("/annotation-b.png")];
        viewer.annotation_edit = Some(AnnotationEdit {
            kind: "tag",
            annotation_id: None,
            buffer: LabelBuffer::from_str("orphan"),
        });
        viewer.confirm_annotation_edit(cx);
        assert!(
            viewer.annotation_edit.is_some(),
            "the unindexed refusal keeps the buffer"
        );
        assert!(
            viewer
                .status
                .as_deref()
                .unwrap()
                .contains("not in the gallery index"),
            "status: {:?}",
            viewer.status
        );

        // Escape cancels without dispatching anything.
        viewer.handle_annotation_key(
            &Keystroke {
                modifiers: gpui::Modifiers {
                    control: false,
                    alt: false,
                    shift: false,
                    platform: false,
                    function: false,
                },
                key: "escape".into(),
                key_char: None,
            },
            cx,
        );
        assert!(viewer.annotation_edit.is_none());
        assert!(viewer.status.as_deref().unwrap().contains("cancelled"));
        assert_eq!(viewer.edit_request.state, ResourceState::Idle);
    });
}

/// dcterms:identifier: `MediaViewer::queue_selected_for_concat`
/// expect: Non-video and over-cap concat additions fail visibly without changing the queue.
/// [P1] Motivating: concat admission never silently drops or coerces selected work.
#[test]
fn concat_queue_rejects_type_and_cap_without_truncation() {
    let mut viewer = MediaViewer::new();
    viewer.assets.push(surfaced_asset("/image.png"));
    viewer.selected = Some(0);
    let error = viewer
        .queue_selected_for_concat()
        .expect_err("image must be rejected");
    assert!(error.contains("Only video"));
    assert!(viewer.concat_queue.is_empty());

    viewer
        .assets
        .push(surfaced_asset_of_kind("/overflow.mp4", "video"));
    viewer.selected = Some(1);
    viewer.concat_queue = (0..hkask_media::media_limits::MAX_CONCAT_ITEMS)
        .map(|index| format!("/{index}.mp4"))
        .collect();
    let before = viewer.concat_queue.clone();
    let error = viewer
        .queue_selected_for_concat()
        .expect_err("over-cap addition must be rejected");
    assert!(error.contains(&hkask_media::media_limits::MAX_CONCAT_ITEMS.to_string()));
    assert_eq!(viewer.concat_queue, before);
}

/// dcterms:identifier: `parse_delete_acknowledgement`
/// expect: Deletion refreshes the Library only after an exact successful acknowledgement.
/// [P1] Motivating: malformed deletion responses cannot falsely remove visible work.
#[test]
fn delete_acknowledgement_is_strict() {
    let valid = serde_json::json!({"content": {
        "deleted": true,
        "image_id": "asset-1",
        "file_deleted": false,
        "transcripts_detached": 2,
        "gallery_changed": true
    }})
    .to_string();
    assert_eq!(parse_delete_acknowledgement(&valid, "asset-1"), Ok(2));
    for invalid in [
        r#"{"content":{"deleted":false,"image_id":"asset-1","file_deleted":false,"transcripts_detached":0,"gallery_changed":true}}"#,
        r#"{"content":{"deleted":true,"image_id":"other","file_deleted":false,"transcripts_detached":0,"gallery_changed":true}}"#,
        r#"{"content":{"deleted":true,"image_id":"asset-1","file_deleted":false,"transcripts_detached":0}}"#,
    ] {
        assert!(parse_delete_acknowledgement(invalid, "asset-1").is_err());
    }
}

/// dcterms:identifier: `parse_job_listing`
/// expect: The panel preserves every current queue field and rejects compatibility shapes.
/// [P7] Motivating: the producer and consumer share one strict contract.
#[test]
fn job_listing_preserves_strict_contract_and_rejects_legacy_shapes() {
    let record = job("running");
    let listing = parse_job_listing(
        &serde_json::json!({
            "content": {
                "jobs": [record],
                "total": 3,
                "limit": 1,
                "has_more": true,
                "history_scope": hkask_media::jobs_contract::JOB_HISTORY_SCOPE,
                "restart_behavior": hkask_media::jobs_contract::JOB_RESTART_BEHAVIOR
            }
        })
        .to_string(),
    )
    .expect("current response parses");
    assert_eq!(listing.jobs.len(), 1);
    assert_eq!(listing.total, 3);
    assert_eq!(listing.limit, 1);
    assert!(listing.has_more);
    assert_eq!(
        listing.history_scope,
        hkask_media::jobs_contract::JOB_HISTORY_SCOPE
    );
    for response in [
        r#"{"content":[]}"#,
        r#"{"content":{"jobs":[]}}"#,
        r#"{"content":{"jobs":[],"total":0,"limit":20,"has_more":false,"history_scope":"ephemeral_process_local"}}"#,
    ] {
        assert!(
            parse_job_listing(response).is_err(),
            "legacy/incomplete shape decoded"
        );
    }
}

struct GalleryPageInvoker {
    calls: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
}

impl hkask_tool_invoker::ToolInvoker for GalleryPageInvoker {
    fn invoke_tool(
        &self,
        server: &str,
        tool: &str,
        args: Value,
    ) -> gpui::Task<Result<String, hkask_tool_invoker::InvokeError>> {
        assert_eq!(server, MEDIA_SERVER);
        assert_eq!(tool, "gallery_list_assets");
        self.calls.lock().expect("calls lock").push(args.clone());
        let offset = args.get("offset").and_then(Value::as_u64).expect("offset");
        let limit = args.get("limit").and_then(Value::as_u64).expect("limit");
        gpui::Task::ready(Ok(serde_json::json!({
            "content": {
                "gallery_id": "gallery-pages",
                "total": 250,
                "offset": offset,
                "limit": limit,
                "assets": [{
                    "id": format!("asset-{offset}"),
                    "index": offset,
                    "path": format!("/{offset}.mp4"),
                    "media_type": "video"
                }]
            }
        })
        .to_string()))
    }
}

/// dcterms:identifier: `MediaViewer::next_gallery_page`
/// expect: Previous and next actions dispatch reachable adjacent pages and commit their offsets.
/// [P1] Motivating: every gallery page is reachable from Library controls.
#[gpui::test]
fn gallery_previous_and_next_reach_adjacent_pages(cx: &mut gpui::TestAppContext) {
    let _invoker_guard = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _restore = InvokerRestore(hkask_tool_invoker::shared_tool_invoker());
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    hkask_tool_invoker::set_tool_invoker(Some(std::sync::Arc::new(GalleryPageInvoker {
        calls: calls.clone(),
    })));
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, _| {
        viewer.gallery_total = Some(250);
        viewer.gallery_id = Some("gallery-pages".into());
        viewer.gallery_limit = GALLERY_PAGE_LIMIT;
    });
    viewer.update(cx, |viewer, cx| viewer.next_gallery_page(cx));
    cx.run_until_parked();
    assert_eq!(viewer.read_with(cx, |viewer, _| viewer.gallery_offset), 100);
    viewer.update(cx, |viewer, cx| viewer.previous_gallery_page(cx));
    cx.run_until_parked();
    assert_eq!(viewer.read_with(cx, |viewer, _| viewer.gallery_offset), 0);
    let calls = calls.lock().expect("calls lock");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["offset"], 100);
    assert_eq!(calls[1]["offset"], 0);
}

/// dcterms:identifier: `resource_status`
/// expect: In-progress work uses neutral presentation, never error presentation.
/// [P1] Motivating: users can distinguish progress from failure.
#[test]
fn loading_status_is_not_error_colored() {
    let (_, color) = resource_status("Queue", &ResourceState::Loading, false, None)
        .expect("loading is presented");
    assert_eq!(color, ui::Color::Muted);
}

/// dcterms:identifier: `parse_transcript_listing`
/// expect: Transcript listings parse strictly or reject, never degrade to an empty success.
/// [P1] Motivating: a malformed listing must not masquerade as "no transcripts".
#[test]
fn transcript_listing_is_strict() {
    let valid = serde_json::json!({
        "transcripts": [
            {"id": "caa6566e", "media_path": "/generated/vonnegut.mp4", "words_count": 558, "audio_duration_secs": 276.392, "has_word_timings": true, "alignment": "re_anchored"},
            {"id": "t-untimed", "media_path": "/generated/plain.mp3", "words_count": 12, "audio_duration_secs": 11, "has_word_timings": false, "alignment": "untimed"}
        ],
        "count": 2
    });
    assert_eq!(
        parse_transcript_listing(&valid),
        Ok(vec![
            TranscriptSummaryRow {
                id: "caa6566e".into(),
                media_path: "/generated/vonnegut.mp4".into(),
                words_count: 558,
                duration_secs: 276,
                has_word_timings: true,
                alignment: "re_anchored".into(),
                gallery_asset_id: None,
            },
            TranscriptSummaryRow {
                id: "t-untimed".into(),
                media_path: "/generated/plain.mp3".into(),
                words_count: 12,
                duration_secs: 11,
                has_word_timings: false,
                alignment: "untimed".into(),
                gallery_asset_id: None,
            }
        ])
    );
    for invalid in [
        r#"{"transcripts": [], "count": 1}"#,
        r#"{"count": 0}"#,
        r#"{"transcripts": []}"#,
        r#"{"transcripts": [{}], "count": 1}"#,
        r#"{"transcripts": [{"id": ""}], "count": 1}"#,
        r#"{"transcripts": [{"id": "t", "words_count": 3}], "count": 1}"#,
        r#"{"transcripts": [{"id": "t", "media_path": "", "words_count": 3}], "count": 1}"#,
        r#"{"transcripts": [{"id": "t", "has_word_timings": true}], "count": 1}"#,
        r#"{"transcripts": [{"id": "t", "media_path": "/m", "words_count": 3, "has_word_timings": true}], "count": 1}"#,
        r#"{"transcripts": [{"id": "t", "media_path": "/m", "words_count": 3, "has_word_timings": true, "alignment": "sideways"}], "count": 1}"#,
        r#"{"transcripts": ["t"], "count": 1}"#,
        r#"{}"#,
    ] {
        let payload: Value = serde_json::from_str(invalid).expect("fixture parses");
        assert!(
            parse_transcript_listing(&payload).is_err(),
            "expected rejection for {invalid}"
        );
    }
}

/// dcterms:identifier: `transcript_alignment_note`
/// expect: The hand-annotatable states carry no note (the default row IS
/// the editable bundle); the gated states name their gate, and untimed
/// rows rely on the row's existing no-timings text. [P1]
#[test]
fn transcript_alignment_note_marks_gated_recordings_only() {
    assert_eq!(transcript_alignment_note("original"), None);
    assert_eq!(transcript_alignment_note("aligned"), None);
    assert_eq!(transcript_alignment_note("untimed"), None);
    assert_eq!(
        transcript_alignment_note("re_anchored"),
        Some("Estimated timings · review only")
    );
    assert_eq!(
        transcript_alignment_note("unaligned"),
        Some("Timed editing unavailable")
    );
}

#[test]
fn recording_keyed_workspace_bodies_prefer_the_recording() {
    let mut viewer = MediaViewer::new();
    // A video-keyed transcript: the gallery recording is the identity,
    // so the workspace renders the RECORDING's asset body — the video
    // plays, the same body the asset workspace's player renders.
    viewer.assets = vec![MediaAsset {
        body: r#"{"kind":"video","src":"/talk.mp4"}"#.into(),
        src: "/talk.mp4".into(),
        kind: "video".into(),
        tool: "gallery".into(),
        gallery_index: Some(0),
        gallery_asset_id: Some("asset-talk".into()),
    }];
    let mut state = pane_state("t-video");
    state.gallery_asset_id = Some("asset-talk".into());
    assert_eq!(
        viewer.workspace_media_body(&state),
        r#"{"kind":"video","src":"/talk.mp4"}"#,
        "a linked transcript renders the recording's asset body"
    );
    // An unlinked transcript falls to its stored media_path — the
    // recording left the gallery index; the transcript and its durable
    // exports stay reachable.
    let unlinked = pane_state("t-unlinked");
    assert_eq!(
        viewer.workspace_media_body(&unlinked),
        transcript_media_body(&unlinked.media_path),
        "an unlinked transcript falls to its stored media body"
    );
}

#[test]
fn human_recording_labels_avoid_internal_identifiers() {
    assert_eq!(
        recording_display_name("/generated/dunning-2017-audio.mp3"),
        "dunning 2017 audio"
    );
    assert_eq!(
        recording_display_name("/generated/2d014afb-07ca-418b-9d53-8cb1f7329fec.mp4"),
        "Untitled recording"
    );
    assert_eq!(format_recording_duration(11), "11s");
    assert_eq!(format_recording_duration(276), "4m 36s");
    assert_eq!(format_recording_duration(4963), "1h 22m");

    let mut state = pane_state("t");
    state.words = Some(vec![
        PaneWord {
            word: "first".into(),
            start_ms: 0,
            end_ms: 100,
        },
        PaneWord {
            word: "second".into(),
            start_ms: 100,
            end_ms: 200,
        },
        PaneWord {
            word: "third".into(),
            start_ms: 200,
            end_ms: 300,
        },
    ]);
    assert_eq!(transcript_range_excerpt(&state, 1, 2), "second third");
}

/// dcterms:identifier: `parse_reel_provenance`
/// expect: A render record names its source transcript; absence renders nothing.
/// [P1] Motivating: reel provenance shows when an asset was cut from a transcript.
#[test]
fn reel_provenance_names_source_and_absence_is_none() {
    let rendered = serde_json::json!({
        "transcript_render": {
            "gallery_asset_id": "reel-1",
            "source_transcript_id": "caa6566e",
            "created_at": "2026-10-25T18:45:57Z"
        }
    });
    assert_eq!(
        parse_reel_provenance(&rendered).as_deref(),
        Some("caa6566e (created 2026-10-25T18:45:57Z)")
    );
    assert_eq!(parse_reel_provenance(&serde_json::json!({})), None);
    assert_eq!(
        parse_reel_provenance(&serde_json::json!({"transcript_render": null})),
        None
    );
    assert_eq!(
        parse_reel_provenance(&serde_json::json!({
            "transcript_render": {"gallery_asset_id": "reel-1"}
        })),
        None
    );
}

/// dcterms:identifier: `MediaViewer::apply_detail_transcripts_result`
/// expect: A newer transcript request owns the rows even if an older outcome arrives later.
/// [P1] Motivating: overlapping transcript loads cannot regress visible detail state.
#[gpui::test]
fn overlapping_detail_transcripts_requests_drop_stale_errors(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        let old_request = viewer
            .detail_transcripts_request
            .begin(RequestOwner::DetailTranscripts("asset-old".into()))
            .expect("old transcript request starts");
        let new_request = viewer
            .detail_transcripts_request
            .begin(RequestOwner::DetailTranscripts("asset-new".into()))
            .expect("new transcript request supersedes");
        viewer.apply_detail_transcripts_result(
            new_request,
            Ok(vec![TranscriptSummaryRow {
                id: "t-new".into(),
                media_path: "/generated/new.mp4".into(),
                words_count: 4,
                duration_secs: 2,
                has_word_timings: true,
                alignment: "original".into(),
                gallery_asset_id: None,
            }]),
            cx,
        );
        viewer.apply_detail_transcripts_result(old_request, Err("stale listing error".into()), cx);
        assert_eq!(
            viewer.detail_transcripts_request.state,
            ResourceState::Ready
        );
        assert_eq!(
            viewer.detail_transcripts.as_deref(),
            Some(
                &[TranscriptSummaryRow {
                    id: "t-new".into(),
                    media_path: "/generated/new.mp4".into(),
                    words_count: 4,
                    duration_secs: 2,
                    has_word_timings: true,
                    alignment: "original".into(),
                    gallery_asset_id: None,
                }][..]
            )
        );

        let failure = viewer
            .detail_transcripts_request
            .begin(RequestOwner::DetailTranscripts("asset-new".into()))
            .expect("failure request starts");
        viewer.apply_detail_transcripts_result(
            failure,
            Err("educt_list_transcripts failed: unreachable".into()),
            cx,
        );
        assert_eq!(
            viewer.detail_transcripts_request.state,
            ResourceState::Failed("educt_list_transcripts failed: unreachable".into())
        );
    });
}

/// dcterms:identifier: `parse_transcript_payload`
/// expect: A get_transcript payload parses strictly into the pane state
/// or rejects — never a half view.
/// [P1] Motivating: the workspace renders the real working view, and a
/// malformed payload must surface as a visible error.
#[test]
fn transcript_payload_parses_strictly() {
    let valid = serde_json::json!({
        "summary": {
            "id": "caa6566e",
            "media_path": "/generated/vonnegut.mp4",
            "words_count": 558,
            "has_word_timings": true
        },
        "working_transcript": {
            "text": "There's no reason why the simple shapes",
            "alignment": "original",
            "correction_layer_id": null,
            "realignment_layer_id": null,
            "alignment_error": null,
            "words": [
                {"word": "There's", "start_ms": 21700, "end_ms": 21999, "confidence": null},
                {"word": "no", "start_ms": 21999, "end_ms": 22119, "confidence": null},
                {"word": "reason", "start_ms": 22119, "end_ms": 22420, "confidence": null}
            ]
        },
        "layers": [
            {
                "id": "layer-paragraph",
                "transcript_id": "caa6566e",
                "created_at": "2026-10-25T18:45:03Z",
                "layer": {
                    "kind": "paragraph",
                    "provenance": {"model": "test", "prompt_template": "t", "created_at": "2026-10-25T18:45:03Z"},
                    "breaks_after": [1]
                }
            },
            {
                "id": "layer-speaker",
                "transcript_id": "caa6566e",
                "created_at": "2026-10-25T18:45:04Z",
                "layer": {
                    "kind": "speaker",
                    "provenance": {"model": "test", "prompt_template": "t", "created_at": "2026-10-25T18:45:04Z"},
                    "spans": [
                        {"start_word": 0, "end_word": 2, "speaker": "Vonnegut", "confidence": 0.9}
                    ]
                }
            },
            {
                "id": "layer-highlight",
                "transcript_id": "caa6566e",
                "created_at": "2026-10-25T18:45:05Z",
                "layer": {
                    "kind": "highlight",
                    "provenance": {"model": "user", "prompt_template": "researcher_annotation", "created_at": "2026-10-25T18:45:05Z"},
                    "highlights": [
                        {"start_word": 1, "end_word": 2, "label": "thesis", "note": "n"}
                    ]
                }
            }
        ],
        "exports": [],
        "renders": []
    });
    let state = parse_transcript_payload(&valid).expect("valid payload parses");
    // Tolerant absence (task 1a): a payload without a review view
    // parses — the pane renders no flags, never a fabricated green.
    assert_eq!(state.review, None);
    assert_eq!(state.transcript_id, "caa6566e");
    assert_eq!(state.media_path, "/generated/vonnegut.mp4");
    assert_eq!(state.alignment, "original");
    assert_eq!(
        state.words.as_deref(),
        Some(
            &[
                PaneWord {
                    word: "There's".into(),
                    start_ms: 21700,
                    end_ms: 21999
                },
                PaneWord {
                    word: "no".into(),
                    start_ms: 21999,
                    end_ms: 22119
                },
                PaneWord {
                    word: "reason".into(),
                    start_ms: 22119,
                    end_ms: 22420
                }
            ][..]
        )
    );
    assert_eq!(state.paragraph_breaks.as_deref(), Some(&[1usize][..]));
    assert_eq!(
        state.speaker_spans.as_deref(),
        Some(
            &[PaneSpeakerSpan {
                start_word: 0,
                end_word: 2,
                speaker: "Vonnegut".into()
            }][..]
        )
    );
    assert_eq!(state.layers.len(), 3);
    assert_eq!(state.layers[0].kind, "paragraph");
    assert_eq!(
        state.highlights,
        vec![PaneHighlight {
            start_word: 1,
            end_word: 2,
            label: "thesis".into(),
            note: "n".into(),
            annotation_id: "layer-highlight".into(),
            researcher_supplied: true
        }]
    );

    for invalid in [
        serde_json::json!({}),
        serde_json::json!({"summary": {}}),
        serde_json::json!({"summary": {"id": ""}}),
        serde_json::json!({
            "summary": {"id": "t", "media_path": ""}
        }),
        serde_json::json!({
            "summary": {"id": "t", "media_path": "/m"}
        }),
        serde_json::json!({
            "summary": {"id": "t", "media_path": "/m"},
            "working_transcript": {}
        }),
        serde_json::json!({
            "summary": {"id": "t", "media_path": "/m"},
            "working_transcript": {"text": "hi", "alignment": "sideways"}
        }),
        serde_json::json!({
            "summary": {"id": "t", "media_path": "/m"},
            "working_transcript": {"text": "hi", "alignment": "original"}
        }),
        serde_json::json!({
            "summary": {"id": "t", "media_path": "/m"},
            "working_transcript": {
                "text": "hi", "alignment": "original",
                "words": [{"word": "hi"}]
            }
        }),
        serde_json::json!({
            "summary": {"id": "t", "media_path": "/m"},
            "working_transcript": {
                "text": "hi", "alignment": "original",
                "words": [{"word": "hi", "start_ms": 0, "end_ms": 1}]
            }
        }),
        serde_json::json!({
            "summary": {"id": "t", "media_path": "/m"},
            "working_transcript": {
                "text": "hi", "alignment": "untimed"
            }
        }),
        serde_json::json!({
            "summary": {"id": "t", "media_path": "/m"},
            "working_transcript": {
                "text": "hi", "alignment": "original",
                "words": [{"word": "hi", "start_ms": 0, "end_ms": 1}]
            },
            "layers": [{}]
        }),
        serde_json::json!({
            "summary": {"id": "t", "media_path": "/m"},
            "working_transcript": {
                "text": "hi", "alignment": "original",
                "words": [{"word": "hi", "start_ms": 0, "end_ms": 1}]
            },
            "layers": [{
                "id": "l", "created_at": "2026-10-25T00:00:00Z",
                "layer": {"kind": "paragraph", "breaks_after": ["two"]}
            }]
        }),
        serde_json::json!({
            "summary": {"id": "t", "media_path": "/m"},
            "working_transcript": {
                "text": "hi", "alignment": "original",
                "words": [{"word": "hi", "start_ms": 0, "end_ms": 1}]
            },
            "layers": [{
                "id": "l", "created_at": "2026-10-25T00:00:00Z",
                "layer": {"kind": "speaker", "spans": [{"start_word": 0, "end_word": 0}]}
            }]
        }),
        serde_json::json!({
            "summary": {"id": "t", "media_path": "/m"},
            "working_transcript": {
                "text": "hi", "alignment": "original",
                "words": [{"word": "hi", "start_ms": 0, "end_ms": 1}]
            },
            "layers": [{
                "id": "l", "created_at": "2026-10-25T00:00:00Z",
                "layer": {"kind": "highlight", "highlights": [{"start_word": 0, "end_word": 0}]}
            }]
        }),
        serde_json::json!({
            "summary": {"id": "t", "media_path": "/m"},
            "working_transcript": {
                "text": "hi", "alignment": "original",
                "words": [{"word": "hi", "start_ms": 0, "end_ms": 1}]
            },
            "layers": [{
                "id": "l", "created_at": "2026-10-25T00:00:00Z",
                "layer": {"kind": "highlight", "highlights": ["no"]}
            }]
        }),
    ] {
        assert!(
            parse_transcript_payload(&invalid).is_err(),
            "expected rejection for {invalid}"
        );
    }

    // Text-only views parse with words None — the pane renders the text
    // with a visible degradation note.
    let untimed = serde_json::json!({
        "summary": {"id": "t2", "media_path": "/m2"},
        "working_transcript": {
            "text": "plain text only",
            "alignment": "untimed",
            "alignment_error": "source transcript has no word-level timings"
        },
        "layers": []
    });
    let state = parse_transcript_payload(&untimed).expect("untimed payload parses");
    assert_eq!(state.review, None);
    assert_eq!(state.words, None);
    assert_eq!(state.working_text, "plain text only");
    assert_eq!(
        state.alignment_note.as_deref(),
        Some("source transcript has no word-level timings")
    );
}

/// dcterms:identifier: `parse_transcript_payload`
/// expect: A derived review view parses strictly when present —
/// flagged blocks carry the stored minimum confidence and gaps surface
/// no-confidence-recorded — and a malformed one rejects the payload.
#[test]
fn transcript_payload_parses_review_flags() {
    let payload = serde_json::json!({
        "summary": {
            "id": "t-review",
            "media_path": "/generated/review.wav",
            "words_count": 4,
            "has_word_timings": true
        },
        "working_transcript": {
            "text": "a b c d",
            "alignment": "original",
            "correction_layer_id": null,
            "realignment_layer_id": null,
            "alignment_error": null,
            "words": [
                {"word": "a", "start_ms": 0, "end_ms": 500, "confidence": null},
                {"word": "b", "start_ms": 500, "end_ms": 1000, "confidence": null},
                {"word": "c", "start_ms": 1000, "end_ms": 1500, "confidence": null},
                {"word": "d", "start_ms": 1500, "end_ms": 2000, "confidence": null}
            ]
        },
        "review": {
            "threshold": 0.75,
            "paragraph_layer_id": "p-1",
            "speaker_layer_id": "s-1",
            "blocks": [
                {"start_word": 0, "end_word": 1, "min_confidence": 0.9, "status": "ok"},
                {"start_word": 2, "end_word": 3, "min_confidence": 0.5, "status": "flagged"},
                {"start_word": 4, "end_word": 5, "min_confidence": null, "status": "no-confidence-recorded"}
            ],
            "note": null
        },
        "layers": [],
        "exports": [],
        "renders": []
    });
    let state = parse_transcript_payload(&payload).expect("payload with review parses");
    let review = state.review.expect("review view present");
    assert_eq!(review.threshold, 0.75);
    assert_eq!(review.note, None);
    assert_eq!(
        review.blocks,
        vec![
            PaneReviewFlag {
                start_word: 0,
                end_word: 1,
                min_confidence: Some(0.9),
                status: PaneReviewStatus::Ok
            },
            PaneReviewFlag {
                start_word: 2,
                end_word: 3,
                min_confidence: Some(0.5),
                status: PaneReviewStatus::Flagged
            },
            PaneReviewFlag {
                start_word: 4,
                end_word: 5,
                min_confidence: None,
                status: PaneReviewStatus::NoConfidenceRecorded
            },
        ]
    );

    // Malformed review views reject the whole payload — never a half
    // view: an unknown status, a non-numeric confidence, a missing
    // threshold.
    let mut unknown_status = payload.clone();
    unknown_status["review"]["blocks"][0]["status"] = serde_json::json!("green");
    assert!(parse_transcript_payload(&unknown_status).is_err());

    let mut bad_confidence = payload.clone();
    bad_confidence["review"]["blocks"][0]["min_confidence"] = serde_json::json!("high");
    assert!(parse_transcript_payload(&bad_confidence).is_err());

    let mut no_threshold = payload;
    no_threshold["review"]
        .as_object_mut()
        .expect("review object")
        .remove("threshold");
    assert!(parse_transcript_payload(&no_threshold).is_err());
}

/// dcterms:identifier: `latest_layer_payload`
/// expect: The newest layer of a kind wins, deterministically.
#[test]
fn latest_paragraph_layer_wins_deterministically() {
    let older = serde_json::json!({
        "id": "a-paragraph",
        "created_at": "2026-10-25T00:00:00Z",
        "layer": {"kind": "paragraph", "breaks_after": [9]}
    });
    let newer_same_timestamp = serde_json::json!({
        "id": "b-paragraph",
        "created_at": "2026-10-25T00:00:00Z",
        "layer": {"kind": "paragraph", "breaks_after": [1]}
    });
    let layers = [older, newer_same_timestamp];
    let latest = latest_layer_payload(&layers, "paragraph").expect("found");
    assert_eq!(latest.get("breaks_after"), Some(&serde_json::json!([1])));
}

/// dcterms:identifier: `MediaViewer::handle_word_selection`
/// expect: Seek outside select mode; anchor → clear on repeat; anchor →
/// ordered range on the second click — never an inverted range. The
/// completed range opens the selection popover; Highlight on the
/// popover opens the label editor.
/// [P1] Motivating: creation must be deliberate and range-ordered.
#[test]
fn word_selection_state_machine() {
    let mut viewer = MediaViewer::new();
    viewer.selection_mode = false;
    assert_eq!(viewer.handle_word_selection(5), WordClickOutcome::SeekOnly);

    viewer.selection_mode = true;
    viewer.selection_anchor = None;
    assert_eq!(
        viewer.handle_word_selection(5),
        WordClickOutcome::AnchorSet(5)
    );

    viewer.selection_anchor = Some(5);
    assert_eq!(
        viewer.handle_word_selection(5),
        WordClickOutcome::AnchorCleared
    );

    viewer.selection_anchor = Some(9);
    assert_eq!(
        viewer.handle_word_selection(3),
        WordClickOutcome::CompleteRange {
            start_word: 3,
            end_word: 9
        }
    );

    viewer.selection_anchor = Some(3);
    assert_eq!(
        viewer.handle_word_selection(9),
        WordClickOutcome::CompleteRange {
            start_word: 3,
            end_word: 9
        }
    );

    // The label editor is modal (S4-3): while open, clicks neither seek
    // nor build a new selection.
    viewer.selection_mode = false;
    viewer.selection_anchor = Some(2);
    viewer.highlight_edit = Some(HighlightEdit {
        kind: HighlightEditKind::Create {
            start_word: 3,
            end_word: 9,
        },
        buffer: LabelBuffer::default(),
    });
    assert_eq!(
        viewer.handle_word_selection(5),
        WordClickOutcome::EditorOpen
    );
}

/// dcterms:identifier: `MediaViewer::handle_word_drag`
/// expect: press-and-release across words completes the ordered union
/// range in EITHER interaction mode (drag is the one-gesture selection
/// path — seek mode keeps click-to-seek); same-word release stays a
/// plain click; the open label editor makes drags inert. The completed
/// range opens the selection popover; Highlight on the popover opens
/// the label editor.
#[test]
fn word_drag_state_machine() {
    let mut viewer = MediaViewer::new();
    viewer.selection_mode = false;
    assert_eq!(
        viewer.handle_word_drag(2, 5),
        WordDragOutcome::CompleteRange {
            start_word: 2,
            end_word: 5
        },
        "drag-select completes a range outside Select text mode"
    );
    assert_eq!(
        viewer.handle_word_drag(9, 3),
        WordDragOutcome::CompleteRange {
            start_word: 3,
            end_word: 9
        },
        "the range is the ordered union, never inverted"
    );
    assert_eq!(
        viewer.handle_word_drag(4, 4),
        WordDragOutcome::Click,
        "same-word press/release is a plain click (the click path seeks)"
    );
    viewer.highlight_edit = Some(HighlightEdit {
        kind: HighlightEditKind::Create {
            start_word: 0,
            end_word: 1,
        },
        buffer: LabelBuffer::default(),
    });
    assert_eq!(
        viewer.handle_word_drag(2, 5),
        WordDragOutcome::Inert,
        "drags are inert while the label editor is open"
    );
}

/// dcterms:identifier: `MediaViewer::popover_cut`
/// expect: The tray's Cut consumes the popover and dispatches the
/// existing strike path; an in-flight edit surfaces the admission
/// refusal instead of dispatching.
/// [P1] Motivating: Delete/Cut on a pending selection must reach the
/// strike dispatcher, never a silent no-op.
#[gpui::test]
fn popover_cut_dispatches_the_strike_path(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(TranscriptPaneState {
            transcript_id: "t-popover-cut".into(),
            media_path: "/m.wav".into(),
            gallery_asset_id: None,
            alignment: "original".into(),
            alignment_note: None,
            words: None,
            working_text: String::new(),
            paragraph_breaks: None,
            speaker_spans: None,
            review: None,
            layers: Vec::new(),
            highlights: Vec::new(),
            strikes: Vec::new(),
            correction_edits: Vec::new(),
            source_words: Vec::new(),
        });
        viewer.selection_popover = Some((3, 9));
        viewer.edit_request.state = ResourceState::Loading;
        viewer.popover_cut(cx);
        assert!(
            viewer.selection_popover.is_none(),
            "the cut consumes the tray"
        );
        assert!(
            viewer
                .status
                .as_deref()
                .is_some_and(|status| status.contains("Another media edit")),
            "the strike arm surfaced the admission refusal: {:?}",
            viewer.status
        );
    });
}

/// dcterms:identifier: `MediaViewer::popover_include_in_reel`
/// expect: Over a composed clip's range the tray flips that clip's
/// include flag (the highlights row's toggle path, no dispatch — the
/// clip's flag is the one membership source of truth); over a fresh
/// range it dispatches the existing create path — an in-flight edit
/// surfaces the admission refusal and the range never enters the
/// composer directly (a clip with no highlight behind it has no reload
/// identity to survive on, so the fresh arm creates).
/// [P1] Motivating: selection → reel is one click (the low-effort path),
/// and membership state stays honest.
#[gpui::test]
fn popover_include_toggles_reel_membership(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(TranscriptPaneState {
            transcript_id: "t-popover-include".into(),
            media_path: "/m.wav".into(),
            gallery_asset_id: None,
            alignment: "original".into(),
            alignment_note: None,
            words: None,
            working_text: String::new(),
            paragraph_breaks: None,
            speaker_spans: None,
            review: None,
            layers: Vec::new(),
            highlights: vec![PaneHighlight {
                start_word: 3,
                end_word: 9,
                label: "thesis".into(),
                note: String::new(),
                annotation_id: "ann-1".into(),
                researcher_supplied: true,
            }],
            strikes: Vec::new(),
            correction_edits: Vec::new(),
            source_words: Vec::new(),
        });
        viewer.reel_clips = vec![reel_clip("ann-1", 3, 9, true, &[])];

        // Included → the tray flips the clip's include flag off.
        viewer.selection_popover = Some((3, 9));
        viewer.popover_toggle_reel_include(cx);
        assert_eq!(
            reel_clip_identities(viewer),
            vec![("ann-1".into(), false, vec![])],
            "the include toggle flips the included clip off"
        );
        assert!(viewer.selection_popover.is_none());

        // Excluded → the tray flips it back on.
        viewer.selection_popover = Some((3, 9));
        viewer.popover_toggle_reel_include(cx);
        assert_eq!(
            reel_clip_identities(viewer),
            vec![("ann-1".into(), true, vec![])],
            "the include toggle re-adds the highlight's clip"
        );
        assert!(viewer.selection_popover.is_none());

        // A fresh range dispatches the create path — an in-flight edit
        // surfaces the refusal, and no clip materializes.
        viewer.edit_request.state = ResourceState::Loading;
        viewer.selection_popover = Some((20, 22));
        viewer.popover_toggle_reel_include(cx);
        assert_eq!(
            reel_clip_identities(viewer),
            vec![("ann-1".into(), true, vec![])],
            "a fresh range rides the create dispatch, never a bare clip insert"
        );
        assert!(
            viewer
                .status
                .as_deref()
                .is_some_and(|status| status.contains("Another media edit")),
            "the create arm surfaced the admission refusal: {:?}",
            viewer.status
        );
    });
}

/// dcterms:identifier: `MediaViewer::popover_copy_time`
/// expect: Copy time consumes the tray and writes the selection's
/// time span to the clipboard in the panel's marks vocabulary
/// ("in 0.3s out 0.5s" — the same readout the trim marks label
/// uses); a view without word timings degrades honestly instead of
/// copying a made-up span.
/// [P1] Motivating: the copied span must be the exact selection, and
/// degradation must surface, never fabricate.
#[gpui::test]
fn popover_copy_time_writes_the_span(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(TranscriptPaneState {
            transcript_id: "t-popover-copy".into(),
            media_path: "/m.wav".into(),
            gallery_asset_id: None,
            alignment: "original".into(),
            alignment_note: None,
            words: Some(vec![
                PaneWord {
                    word: "the".into(),
                    start_ms: 200,
                    end_ms: 290,
                },
                PaneWord {
                    word: "span".into(),
                    start_ms: 300,
                    end_ms: 390,
                },
                PaneWord {
                    word: "to".into(),
                    start_ms: 400,
                    end_ms: 470,
                },
                PaneWord {
                    word: "copy".into(),
                    start_ms: 480,
                    end_ms: 500,
                },
            ]),
            working_text: String::new(),
            paragraph_breaks: None,
            speaker_spans: None,
            review: None,
            layers: Vec::new(),
            highlights: Vec::new(),
            strikes: Vec::new(),
            correction_edits: Vec::new(),
            source_words: Vec::new(),
        });
        viewer.selection_popover = Some((1, 3));
        viewer.popover_copy_time(cx);
        let copied = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .expect("the span is on the clipboard");
        assert_eq!(copied, "in 0.3s out 0.5s");
        assert!(
            viewer.selection_popover.is_none(),
            "the copy consumes the tray"
        );
    });

    // No word timings on the view: honest degradation, nothing new
    // copied (the clipboard keeps the earlier span — the degraded arm
    // writes nothing rather than a made-up span).
    viewer.update(cx, |viewer, cx| {
        viewer.transcript.as_mut().unwrap().words = None;
        viewer.selection_popover = Some((1, 3));
        viewer.popover_copy_time(cx);
        assert!(
            viewer
                .status
                .as_deref()
                .is_some_and(|status| status.contains("No word timings")),
            "the degraded copy surfaces honestly: {:?}",
            viewer.status
        );
        let copied = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .expect("the clipboard still holds the earlier span");
        assert_eq!(
            copied, "in 0.3s out 0.5s",
            "the degraded arm writes nothing new"
        );
    });
}

/// dcterms:identifier: `MediaViewer::close_transcript_workspace`
/// expect: The selection popover tears down with the surface — the
/// close path, a fresh open, and a surface switch all reset it
/// (teardown symmetry: the tray belongs to the word surface it was
/// opened on).
/// [P1] Motivating: teardown asymmetries are latent traps for later
/// surfaces (the S1 hygiene rule).
#[gpui::test]
fn selection_popover_resets_with_the_surface(cx: &mut gpui::TestAppContext) {
    let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(TranscriptPaneState {
            transcript_id: "t-popover-teardown".into(),
            media_path: "/m.wav".into(),
            gallery_asset_id: None,
            alignment: "original".into(),
            alignment_note: None,
            words: None,
            working_text: String::new(),
            paragraph_breaks: None,
            speaker_spans: None,
            review: None,
            layers: Vec::new(),
            highlights: Vec::new(),
            strikes: Vec::new(),
            correction_edits: Vec::new(),
            source_words: Vec::new(),
        });
        viewer.selection_popover = Some((3, 9));
        viewer.close_transcript_workspace(cx);
        assert!(
            viewer.selection_popover.is_none(),
            "the close path resets the tray"
        );

        viewer.selection_popover = Some((3, 9));
        viewer.open_transcript("t-popover-reopen".into(), cx);
        assert!(
            viewer.selection_popover.is_none(),
            "the open path resets the tray"
        );

        // Re-establish the word surface's state (close/open cleared
        // it) — the surface switch must reset the tray too.
        viewer.transcript = Some(TranscriptPaneState {
            transcript_id: "t-popover-teardown".into(),
            media_path: "/m.wav".into(),
            gallery_asset_id: None,
            alignment: "original".into(),
            alignment_note: None,
            words: None,
            working_text: String::new(),
            paragraph_breaks: None,
            speaker_spans: None,
            review: None,
            layers: Vec::new(),
            highlights: Vec::new(),
            strikes: Vec::new(),
            correction_edits: Vec::new(),
            source_words: Vec::new(),
        });
        viewer.selection_popover = Some((3, 9));
        viewer.toggle_transcript_surface(TranscriptSurface::Events, cx);
        assert!(
            viewer.selection_popover.is_none(),
            "leaving the word surface resets the tray"
        );
    });
}

/// dcterms:identifier: `transcript_search_matches`
/// expect: The search matches the trimmed query case-insensitively over
/// the working words joined by single spaces (the same join the blocks
/// render), maps every occurrence to the inclusive word range it covers
/// (phrases span words), never overlaps, matches nothing on an empty
/// query or wordless view, and treats non-ASCII exactly (the server's
/// `educt_search_transcripts` lexical rule, so the panel and the server
/// agree on what a match is).
/// [P1] Motivating: the match list is the search's ground truth — wrong
/// ranges mark and jump to the wrong words.
#[test]
fn transcript_search_matches_orders_maps_and_edges() {
    let words = |texts: &[&str]| {
        texts
            .iter()
            .map(|text| PaneWord {
                word: (*text).into(),
                start_ms: 0,
                end_ms: 0,
            })
            .collect::<Vec<_>>()
    };
    let transcript = words(&["We", "see", "the", "Dunning", "effect", "in", "dunning"]);
    assert_eq!(
        transcript_search_matches(&transcript, "dunning"),
        vec![(3, 3), (6, 6)],
        "case-insensitive, ordered, word-mapped"
    );
    assert_eq!(
        transcript_search_matches(&transcript, "the dunning effect"),
        vec![(2, 4)],
        "a phrase maps to the covering word range"
    );
    assert_eq!(
        transcript_search_matches(&transcript, "we see"),
        vec![(0, 1)],
        "a lowercase query matches capitalized words"
    );
    assert!(
        transcript_search_matches(&transcript, "absent").is_empty(),
        "no occurrence is an empty list, never silence downstream"
    );
    assert!(
        transcript_search_matches(&transcript, "   ").is_empty(),
        "a whitespace-only query matches nothing"
    );
    assert!(
        transcript_search_matches(&[], "dunning").is_empty(),
        "a wordless view matches nothing"
    );
    assert_eq!(
        transcript_search_matches(&words(&["aaaa"]), "aa"),
        vec![(0, 0), (0, 0)],
        "adjacent occurrences count separately and never overlap"
    );
    // Non-ASCII is exact (the server's rule): the accented word does
    // not answer to the plain query and vice versa.
    let accented = words(&["Café", "cafe"]);
    assert_eq!(transcript_search_matches(&accented, "café"), vec![(0, 0)]);
    assert_eq!(transcript_search_matches(&accented, "CAFE"), vec![(1, 1)]);
    assert_eq!(transcript_search_matches(&accented, "cafe"), vec![(1, 1)]);
}

/// dcterms:identifier: `transcript_search_status`
/// expect: The search status is honest at every state — no label for a
/// blank query, the word-timing degradation note on text-only views, "No
/// matches" for zero results (never silence), and the current-position
/// count while matches exist.
/// [P1] Motivating: zero matches must surface, and a text-only view must
/// say why it cannot search rather than claim absence.
#[test]
fn transcript_search_status_counts_zero_and_degrades() {
    let mut search = TranscriptSearch::default();
    assert_eq!(transcript_search_status(&search, true), None);
    search.query = LabelBuffer::from_str("dunning");
    assert_eq!(
        transcript_search_status(&search, false).as_deref(),
        Some("Search needs a word-timed view"),
        "a text-only view names why it cannot search, not 'no matches'"
    );
    assert_eq!(
        transcript_search_status(&search, true).as_deref(),
        Some("No matches"),
        "zero matches surface, never silence"
    );
    search.matches = vec![(3, 3), (9, 9), (12, 12)];
    search.current = 1;
    assert_eq!(
        transcript_search_status(&search, true).as_deref(),
        Some("2/3"),
        "the count names the current match's position"
    );
}

/// dcterms:identifier: `reset_transcript_search`
/// expect: The search tears down with the surface — the close path, a
/// fresh open, and a fresh workspace reload all clear the query, the
/// matches, the pending rescan, and the follow custody (stale matches
/// would mark words that no longer exist in the reloaded view).
/// [P1] Motivating: teardown asymmetries are latent traps (the S1
/// hygiene rule).
#[gpui::test]
fn transcript_search_resets_with_the_surface(cx: &mut gpui::TestAppContext) {
    let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(pane_state("t-search-teardown"));
        let populate = |viewer: &mut MediaViewer, cx: &mut Context<MediaViewer>| {
            viewer.transcript_search.query = LabelBuffer::from_str("dunning");
            viewer.transcript_search.matches = vec![(3, 3)];
            viewer.transcript_search.current = 0;
            viewer.schedule_transcript_search_rescan(cx);
        };
        let assert_cleared = |viewer: &MediaViewer| {
            assert_eq!(viewer.transcript_search.query.text(), "");
            assert!(viewer.transcript_search.matches.is_empty());
            assert_eq!(viewer.transcript_search.current, 0);
            assert!(
                viewer.transcript_search_task.is_none(),
                "the pending rescan cancels with the surface"
            );
            assert!(!viewer.transcript_search_suspended_follow);
        };
        populate(viewer, cx);
        viewer.close_transcript_workspace(cx);
        assert_cleared(viewer);

        populate(viewer, cx);
        viewer.open_transcript("t-search-open".into(), cx);
        assert_cleared(viewer);

        // The reload path: a fresh workspace state invalidates the
        // pending search the same way.
        populate(viewer, cx);
        let epoch = viewer
            .transcript_request
            .begin(RequestOwner::Transcript("t-search-open".into()))
            .expect("the reload owns its request");
        viewer.apply_transcript_result(epoch, Ok(pane_state("t-search-open")), cx);
        assert_cleared(viewer);
    });
}

/// dcterms:identifier: `highlight_create_params`
/// expect: The create dispatch carries the exact request contract with
/// the label editor's typed label.
#[test]
fn highlight_create_params_carry_the_exact_contract() {
    let params = highlight_create_params("t-1", 34, 51, "climax");
    let object = params.as_object().expect("object");
    assert_eq!(object.len(), 5);
    assert_eq!(object["transcript_id"], serde_json::json!("t-1"));
    assert_eq!(object["start_word"], serde_json::json!(34));
    assert_eq!(object["end_word"], serde_json::json!(51));
    assert_eq!(object["label"], serde_json::json!("climax"));
    assert!(object["note"].as_str().is_some());
}

/// dcterms:identifier: `resolve_create_label`
/// expect: Confirming an empty create editor falls back to the default
/// label; typed text trims through verbatim.
#[test]
fn create_label_resolves_empty_to_the_default() {
    assert_eq!(resolve_create_label(""), "Highlight");
    assert_eq!(resolve_create_label("   "), "Highlight");
    assert_eq!(resolve_create_label("  thesis  "), "thesis");
}

/// dcterms:identifier: `highlight_update_params`
/// expect: The rename dispatch carries the exact request contract —
/// identity plus the replacement label over the unchanged range/note.
#[test]
fn highlight_update_params_carry_the_exact_contract() {
    let params = highlight_update_params("t-1", "ann-9", 34, 51, "climax", "kept note");
    let object = params.as_object().expect("object");
    assert_eq!(object.len(), 6);
    assert_eq!(object["transcript_id"], serde_json::json!("t-1"));
    assert_eq!(object["annotation_id"], serde_json::json!("ann-9"));
    assert_eq!(object["start_word"], serde_json::json!(34));
    assert_eq!(object["end_word"], serde_json::json!(51));
    assert_eq!(object["label"], serde_json::json!("climax"));
    assert_eq!(object["note"], serde_json::json!("kept note"));
}

// ── S5: in-panel layered transcription correction ─────────────────────
//
// The correction flow rides S2's selection tray, the label-editor
// machinery, and the edit admission; the pins below cover the tray's
// mode contract (the re_anchored correction path, D5 v2), the wire
// schema (built from the server's own layer types), the strict parse
// of the live correction layer's edits, and the full scripted dispatch
// chain on both view kinds.

/// dcterms:identifier: `selection_tray_mode`
/// expect: Every alignment answers the tray gate — original/aligned
/// views open the full action tray, a re_anchored view opens the
/// correction tray (the locate-translated path, D5 v2), and the
/// unselectable states keep the visible refusal. [P1] Motivating: the
/// D5 v1 blanket refusal hid the one action that can translate
/// selections on corrected transcripts; the unaligned/untimed
/// refusals stay honest (no timed words to select over).
#[test]
fn selection_tray_mode_covers_every_alignment() {
    assert!(matches!(
        selection_tray_mode("original"),
        Ok(SelectionTrayMode::Full)
    ));
    assert!(matches!(
        selection_tray_mode("aligned"),
        Ok(SelectionTrayMode::Full)
    ));
    assert!(matches!(
        selection_tray_mode("re_anchored"),
        Ok(SelectionTrayMode::CorrectOnly)
    ));
    for alignment in ["unaligned", "untimed", "unexpected"] {
        let reason = selection_tray_mode(alignment).expect_err("blocked");
        assert!(
            reason.starts_with("Direct selection"),
            "{alignment}: {reason}"
        );
        assert!(!reason.contains("index-identical"), "{alignment}: {reason}");
    }
}

/// dcterms:identifier: `correction_store_params`
/// expect: The correction store request carries the server's layer
/// contract verbatim — kind correction, researcher provenance (model
/// "user", the researcher_correction authoring path), and the merged
/// edits over SOURCE word ranges; the realign request names the stored
/// layer; the locate request declares basis "source". [P1]
/// Motivating: a hand-rolled layer JSON drifts from the server's serde
/// types silently — building it from the server's own types and
/// pinning the wire shape keeps the schema pinned byte-for-byte.
#[test]
fn correction_params_carry_the_exact_contract() {
    let edits = vec![PaneCorrectionEdit {
        start_word: 3,
        end_word: 5,
        replacement: "fixed words".into(),
        reason: CORRECTION_REASON.to_string(),
    }];
    let store = correction_store_params("t-1", &edits, "2026-11-06T00:00:00Z")
        .expect("the server layer types serialize");
    assert_eq!(store["transcript_id"], "t-1");
    assert_eq!(store["layer"]["kind"], "correction");
    assert_eq!(store["layer"]["provenance"]["model"], "user");
    assert_eq!(
        store["layer"]["provenance"]["prompt_template"],
        "researcher_correction"
    );
    assert_eq!(
        store["layer"]["provenance"]["created_at"],
        "2026-11-06T00:00:00Z"
    );
    assert_eq!(store["layer"]["edits"][0]["start_word"], 3);
    assert_eq!(store["layer"]["edits"][0]["end_word"], 5);
    assert_eq!(store["layer"]["edits"][0]["replacement"], "fixed words");
    assert_eq!(store["layer"]["edits"][0]["reason"], CORRECTION_REASON);
    let realign = correction_realign_params("t-1", "layer-9");
    assert_eq!(realign["transcript_id"], "t-1");
    assert_eq!(realign["layer_id"], "layer-9");
    let locate = correction_locate_params("t-1", "misheard words");
    assert_eq!(locate["transcript_id"], "t-1");
    assert_eq!(locate["text"], "misheard words");
    assert_eq!(locate["basis"], "source");
}

/// dcterms:identifier: `parse_transcript_payload`
/// expect: The live correction layer's edits parse strictly — the
/// working view names it by correction_layer_id; an absent id parses as
/// no prior edits; a named layer missing from the records rejects the
/// payload rather than silently dropping prior corrections. [P1]
/// Motivating: a dropped prior edit would silently revert an earlier
/// correction at the next merge (the silent-revert guard).
#[test]
fn correction_layer_edits_parse_strictly() {
    let payload = serde_json::json!({
        "summary": {"id": "t-1", "media_path": "/m", "words_count": 3, "has_word_timings": true},
        "working_transcript": {"text": "a b c", "alignment": "aligned", "correction_layer_id": "layer-c1", "words": [
            {"word": "a", "start_ms": 0, "end_ms": 100},
            {"word": "b", "start_ms": 100, "end_ms": 200},
            {"word": "c", "start_ms": 200, "end_ms": 300}
        ]},
        "layers": [
            {"id": "layer-c1", "created_at": "c1", "layer": {
                "kind": "correction",
                "provenance": {"model": "user", "prompt_template": "researcher_correction", "created_at": "c1"},
                "edits": [{"start_word": 1, "end_word": 1, "replacement": "bee", "reason": "fix"}]
            }}
        ],
        "exports": [], "renders": []
    });
    let state = parse_transcript_payload(&payload).expect("parses");
    assert_eq!(
        state.correction_edits,
        vec![PaneCorrectionEdit {
            start_word: 1,
            end_word: 1,
            replacement: "bee".into(),
            reason: "fix".into(),
        }]
    );

    // No live correction → no prior edits.
    let uncorrected = serde_json::json!({
        "summary": {"id": "t-2", "media_path": "/m", "words_count": 1, "has_word_timings": true},
        "working_transcript": {"text": "a", "alignment": "original", "words": [
            {"word": "a", "start_ms": 0, "end_ms": 100}
        ]},
        "layers": [],
        "exports": [], "renders": []
    });
    assert!(
        parse_transcript_payload(&uncorrected)
            .expect("parses")
            .correction_edits
            .is_empty()
    );

    // A named layer missing from the records rejects the payload —
    // never a silent drop of prior corrections.
    let mut orphan = payload;
    orphan["working_transcript"]["correction_layer_id"] = serde_json::json!("layer-gone");
    assert!(parse_transcript_payload(&orphan).is_err());
}

/// The reloaded view Pin D's chain answers `educt_get_transcript` with:
/// the original transcript corrected one-for-one over words [0..=1].
const CORRECTED_VIEW_PAYLOAD: &str = r#"{"summary": {"id": "t-correct", "media_path": "/m", "words_count": 3, "has_word_timings": true}, "working_transcript": {"text": "measured words here", "alignment": "aligned", "correction_layer_id": "layer-new", "words": [{"word": "measured", "start_ms": 0, "end_ms": 500}, {"word": "words", "start_ms": 500, "end_ms": 900}, {"word": "here", "start_ms": 900, "end_ms": 1300}]}, "layers": [{"id": "layer-new", "created_at": "c1", "layer": {"kind": "correction", "provenance": {"model": "user", "prompt_template": "researcher_correction", "created_at": "c1"}, "edits": [{"start_word": 0, "end_word": 1, "replacement": "measured words", "reason": "researcher correction from the transcript workspace selection"}]}}], "exports": [], "renders": []}"#;

/// dcterms:identifier: `MediaViewer::dispatch_workspace_correction`
/// expect: Confirming a correction over working words [a..b] stores a
/// merged correction layer through educt_store_layer (working indices
/// equal source indices on an original view), chains
/// educt_realign_transcript naming the stored layer, and the reloaded
/// working view shows the replacement at [a..b] — the request touches
/// only the layer, never the source bundle. [P1] Motivating: the
/// acceptance is the correction landing in the working view (the
/// layered discipline: source words are never mutated).
#[gpui::test]
fn correction_appears_in_the_working_view(cx: &mut gpui::TestAppContext) {
    let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
    let (calls, _restore) = install_scripted_invoker(vec![
            r#"{"stored": {"id": "layer-new"}}"#.to_string(),
            r#"{"alignment": "aligned", "note": "correction is already one-for-one aligned; no re-anchoring needed", "applied_layer": {"id": "layer-new"}}"#.to_string(),
            CORRECTED_VIEW_PAYLOAD.to_string(),
        ]);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        let mut state = pane_state("t-correct");
        state.words = Some(vec![
            PaneWord {
                word: "misheard".into(),
                start_ms: 0,
                end_ms: 400,
            },
            PaneWord {
                word: "wrods".into(),
                start_ms: 400,
                end_ms: 800,
            },
            PaneWord {
                word: "here".into(),
                start_ms: 800,
                end_ms: 1200,
            },
        ]);
        viewer.transcript = Some(state);
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::Correction {
                start_word: 0,
                end_word: 1,
            },
            buffer: LabelBuffer::from_str("measured words"),
        });
        viewer.confirm_label_edit(cx);
    });
    cx.run_until_parked();

    let recorded = calls.lock().unwrap().clone();
    assert_eq!(
        recorded
            .iter()
            .map(|(tool, _)| tool.as_str())
            .collect::<Vec<_>>(),
        [
            "educt_store_layer",
            "educt_realign_transcript",
            "educt_get_transcript"
        ],
        "store → realign → reload, in order"
    );
    // The store request: the layer anchors the selection's range (the
    // source-identical indices of an original view) with the typed
    // replacement, under the researcher authoring path.
    assert_eq!(recorded[0].1["transcript_id"], "t-correct");
    assert_eq!(recorded[0].1["layer"]["kind"], "correction");
    assert_eq!(recorded[0].1["layer"]["edits"][0]["start_word"], 0);
    assert_eq!(recorded[0].1["layer"]["edits"][0]["end_word"], 1);
    assert_eq!(
        recorded[0].1["layer"]["edits"][0]["replacement"],
        "measured words"
    );
    // The realign names the stored layer.
    assert_eq!(recorded[1].1["layer_id"], "layer-new");
    // The reload landed: the working view carries the correction.
    viewer.update(cx, |viewer, _| {
        let state = viewer.transcript.as_ref().expect("reloaded");
        let words = state.words.as_deref().expect("timed view");
        assert_eq!(words[0].word, "measured");
        assert_eq!(words[1].word, "words");
        assert_eq!(words[2].word, "here");
        assert_eq!(state.alignment, "aligned");
        assert!(
            viewer
                .status
                .as_deref()
                .is_some_and(|status| status.contains("Correction applied")),
            "the completion surfaces: {:?}",
            viewer.status
        );
    });
}

/// The reloaded view Pin E's chain answers `educt_get_transcript` with:
/// the re_anchored working view after the second correction.
const REANCHORED_VIEW_PAYLOAD: &str = r#"{"summary": {"id": "t-reanchored", "media_path": "/m", "words_count": 9, "has_word_timings": true}, "working_transcript": {"text": "kept words here", "alignment": "re_anchored", "correction_layer_id": "layer-second", "words": [{"word": "kept", "start_ms": 0, "end_ms": 100}, {"word": "words", "start_ms": 100, "end_ms": 200}, {"word": "here", "start_ms": 200, "end_ms": 300}]}, "layers": [{"id": "layer-second", "created_at": "c2", "layer": {"kind": "correction", "provenance": {"model": "user", "prompt_template": "researcher_correction", "created_at": "c2"}, "edits": [{"start_word": 40, "end_word": 41, "replacement": "words here", "reason": "researcher correction from the transcript workspace selection"}]}}], "exports": [], "renders": []}"#;

/// dcterms:identifier: `MediaViewer::dispatch_workspace_correction`
/// expect: On a re_anchored view the dispatch translates the working
/// selection to SOURCE word indices through educt_locate basis
/// "source" (disambiguated by the candidate's working range — the
/// `.rules` retranscribe trap), carries disjoint prior corrections
/// verbatim, supersedes the overlapping one visibly, and chains
/// educt_realign_transcript; the reloaded view reads re_anchored. [P1]
/// Motivating: a correction anchored to working indices on a
/// cardinality-changed view lands shifted by the cumulative delta
/// (observed 2026-10-25) — the source range must come from the
/// server's own index map.
#[gpui::test]
fn reanchored_correction_translates_and_chains_realign(cx: &mut gpui::TestAppContext) {
    let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
    let (calls, _restore) = install_scripted_invoker(vec![
            r#"{"status": "located", "ranges": [{"start_word": 40, "end_word": 41, "start_ms": 100, "end_ms": 300, "working_start_word": 1, "working_end_word": 2}], "count": 1, "index_basis": "source", "working_view": "realigned_working", "source_word_count": 9, "working_word_count": 9, "cardinality_changed": false}"#.to_string(),
            r#"{"stored": {"id": "layer-second"}}"#.to_string(),
            r#"{"alignment": "re_anchored", "reanchored": {"correction_layer_id": "layer-second"}}"#.to_string(),
            REANCHORED_VIEW_PAYLOAD.to_string(),
        ]);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        let mut state = pane_state("t-reanchored");
        state.alignment = "re_anchored".into();
        state.words = Some(vec![
            PaneWord {
                word: "kept".into(),
                start_ms: 0,
                end_ms: 100,
            },
            PaneWord {
                word: "wrods".into(),
                start_ms: 100,
                end_ms: 200,
            },
            PaneWord {
                word: "heer".into(),
                start_ms: 200,
                end_ms: 300,
            },
        ]);
        state.correction_edits = vec![
            // A disjoint prior correction rides verbatim.
            PaneCorrectionEdit {
                start_word: 7,
                end_word: 7,
                replacement: "prior-kept".into(),
                reason: "prior".into(),
            },
            // A prior correction overlapping the translated source
            // range is superseded visibly.
            PaneCorrectionEdit {
                start_word: 41,
                end_word: 43,
                replacement: "prior-overlap".into(),
                reason: "prior".into(),
            },
        ];
        viewer.transcript = Some(state);
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::Correction {
                start_word: 1,
                end_word: 2,
            },
            buffer: LabelBuffer::from_str("words here"),
        });
        viewer.confirm_label_edit(cx);
    });
    cx.run_until_parked();

    let recorded = calls.lock().unwrap().clone();
    assert_eq!(
        recorded
            .iter()
            .map(|(tool, _)| tool.as_str())
            .collect::<Vec<_>>(),
        [
            "educt_locate",
            "educt_store_layer",
            "educt_realign_transcript",
            "educt_get_transcript"
        ],
        "locate → store → realign → reload, in order"
    );
    // The locate request quotes the selection's rendered form and
    // declares the source basis.
    assert_eq!(recorded[0].1["text"], "wrods heer");
    assert_eq!(recorded[0].1["basis"], "source");
    // THE TRAP PIN: the new edit anchors the TRANSLATED source range,
    // never the working range (1..=2) — and the disjoint prior edit
    // rides verbatim while the overlapping one is superseded.
    let edits = &recorded[1].1["layer"]["edits"];
    let edits = edits.as_array().expect("edits array");
    assert_eq!(edits.len(), 2, "kept prior + new edit: {edits:?}");
    assert_eq!(
        edits[0]["start_word"], 7,
        "the disjoint prior rides verbatim"
    );
    assert_eq!(edits[0]["replacement"], "prior-kept");
    assert_eq!(
        edits[1]["start_word"], 40,
        "the new edit anchors the SOURCE range"
    );
    assert_eq!(edits[1]["end_word"], 41);
    assert_eq!(edits[1]["replacement"], "words here");
    assert_eq!(recorded[2].1["layer_id"], "layer-second");
    viewer.update(cx, |viewer, _| {
        let state = viewer.transcript.as_ref().expect("reloaded");
        assert_eq!(state.alignment, "re_anchored");
        assert_eq!(
            transcript_alignment_note(&state.alignment),
            Some("Estimated timings · review only")
        );
        assert!(
            viewer
                .status
                .as_deref()
                .is_some_and(|status| status.contains("superseded 1")),
            "the overlapping prior's supersession is surfaced: {:?}",
            viewer.status
        );
    });
}

/// dcterms:identifier: `LabelBuffer`
/// expect: The label input's model edits multi-byte text as characters,
/// clamps cursor moves at both edges, and routes keystrokes (typed
/// characters insert; modifier chords never do).
#[test]
fn label_buffer_edits_and_moves_the_cursor() {
    let mut buffer = LabelBuffer::from_str("hi");
    assert_eq!(buffer.cursor(), 2);
    buffer.insert(' ');
    buffer.insert('é');
    assert_eq!(buffer.text(), "hi é");
    assert_eq!(buffer.cursor(), 4);
    buffer.backspace();
    assert_eq!(buffer.text(), "hi ");
    buffer.move_home();
    buffer.insert('X');
    assert_eq!(buffer.text(), "Xhi ");
    assert_eq!(buffer.cursor(), 1);
    buffer.delete_forward();
    assert_eq!(buffer.text(), "Xi ");
    buffer.move_left();
    buffer.backspace();
    buffer.backspace();
    assert_eq!(buffer.text(), "Xi ", "backspace clamps at the start");
    buffer.move_end();
    buffer.move_right();
    assert_eq!(buffer.cursor(), 3, "move_right clamps at the end");
    assert_eq!(buffer.label(), "Xi");

    let mut typed = LabelBuffer::default();
    assert!(typed.apply_keystroke(&Keystroke {
        modifiers: gpui::Modifiers {
            control: false,
            alt: false,
            shift: false,
            platform: false,
            function: false,
        },
        key: "space".into(),
        key_char: Some(" ".into()),
    }));
    assert_eq!(typed.text(), " ");
    assert!(
        !typed.apply_keystroke(&Keystroke {
            modifiers: gpui::Modifiers {
                control: true,
                alt: false,
                shift: false,
                platform: false,
                function: false,
            },
            key: "c".into(),
            key_char: None,
        }),
        "modifier chords are shortcuts, not typing"
    );
    assert_eq!(typed.text(), " ");
}

/// dcterms:identifier: `compose_reel_params`
/// expect: The three compose modes build the reel EDL directly from
/// the ordered clip list — assemble keeps only the included clips in
/// composer order (a later clip that overlaps an earlier one keeps only
/// its unplayed remainder; adjacent clips stay separate ops), reduce
/// cuts every strike, mixed carries both; empty op sets yield None, and
/// the pane_reel_compose provenance never matches the pane strike
/// identity. [P1]
#[test]
fn compose_reel_params_builds_each_mode() {
    let state = TranscriptPaneState {
        transcript_id: "t-1".into(),
        media_path: "/m".into(),
        gallery_asset_id: None,
        alignment: "original".into(),
        alignment_note: None,
        words: None,
        working_text: String::new(),
        paragraph_breaks: None,
        speaker_spans: None,
        review: None,
        layers: Vec::new(),
        highlights: vec![
            PaneHighlight {
                start_word: 0,
                end_word: 3,
                label: "a".into(),
                note: "n".into(),
                annotation_id: "layer-0".into(),
                researcher_supplied: true,
            },
            PaneHighlight {
                start_word: 2,
                end_word: 5,
                label: "b".into(),
                note: "n".into(),
                annotation_id: "layer-2".into(),
                researcher_supplied: true,
            },
            PaneHighlight {
                start_word: 6,
                end_word: 7,
                label: "c".into(),
                note: "n".into(),
                annotation_id: "layer-6".into(),
                researcher_supplied: true,
            },
        ],
        strikes: vec![PaneStrike {
            start_word: 4,
            end_word: 4,
            annotation_id: "strike-4".into(),
        }],
        correction_edits: Vec::new(),
        source_words: Vec::new(),
    };
    let clips = [
        reel_clip("layer-0", 0, 3, true, &[]),
        reel_clip("layer-2", 2, 5, true, &[]),
        reel_clip("layer-6", 6, 7, true, &[]),
    ];

    // Assemble: the ordered builder — the first clip keeps its range,
    // the overlapping second keeps only its unplayed remainder [4,5],
    // the adjacent third stays its own op.
    let assemble =
        compose_reel_params(&state, &clips, ReelComposeMode::Assemble).expect("assemble");
    assert_eq!(assemble["transcript_id"], serde_json::json!("t-1"));
    assert_eq!(assemble["layer"]["kind"], serde_json::json!("edl"));
    assert_eq!(
        assemble["layer"]["provenance"]["prompt_template"],
        serde_json::json!("pane_reel_compose")
    );
    let ops = assemble["layer"]["ops"].as_array().expect("ops");
    assert_eq!(ops.len(), 3, "ordered keeps, the overlap trimmed");
    assert_eq!(ops[0]["op"], serde_json::json!("keep"));
    assert_eq!(ops[0]["range"]["start_word"], serde_json::json!(0));
    assert_eq!(ops[0]["range"]["end_word"], serde_json::json!(3));
    assert_eq!(ops[1]["range"]["start_word"], serde_json::json!(4));
    assert_eq!(ops[1]["range"]["end_word"], serde_json::json!(5));
    assert_eq!(ops[2]["range"]["start_word"], serde_json::json!(6));
    assert_eq!(ops[2]["range"]["end_word"], serde_json::json!(7));

    // Reduce: one Cut op per strike, no keeps.
    let reduce = compose_reel_params(&state, &clips, ReelComposeMode::Reduce).expect("reduce");
    let ops = reduce["layer"]["ops"].as_array().expect("ops");
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0]["op"], serde_json::json!("cut"));
    assert_eq!(ops[0]["range"]["start_word"], serde_json::json!(4));

    // Mixed: the ordered keeps plus the cut.
    let mixed = compose_reel_params(&state, &clips, ReelComposeMode::Mixed).expect("mixed");
    let ops = mixed["layer"]["ops"].as_array().expect("ops");
    assert_eq!(ops.len(), 4);
    assert_eq!(ops[0]["op"], serde_json::json!("keep"));
    assert_eq!(ops[3]["op"], serde_json::json!("cut"));

    // Empty op sets yield None — and Mixed with nothing included
    // refuses rather than silently composing the reduce reel.
    let none_included = [reel_clip("layer-0", 0, 3, false, &[])];
    assert!(compose_reel_params(&state, &none_included, ReelComposeMode::Assemble).is_none());
    assert!(compose_reel_params(&state, &none_included, ReelComposeMode::Mixed).is_none());
    let no_strikes = TranscriptPaneState {
        strikes: Vec::new(),
        correction_edits: Vec::new(),
        ..state
    };
    assert!(compose_reel_params(&no_strikes, &clips, ReelComposeMode::Reduce).is_none());

    // The pane_reel_compose provenance never matches the pane strike
    // identity: a single-cut reduce EDL re-parsed as a transcript layer
    // renders no strike row — the compose flow's own output cannot
    // masquerade as a researcher strike.
    let payload = serde_json::json!({
        "summary": {"id": "t-1", "media_path": "/m", "words_count": 8, "has_word_timings": true},
        "working_transcript": {"text": "a b c d e f g h", "alignment": "original", "words": [
            {"word": "a", "start_ms": 0, "end_ms": 100}
        ]},
        "layers": [
            {"id": "compose-1", "created_at": "c1", "layer": reduce["layer"].clone()}
        ],
        "exports": [], "renders": []
    });
    let parsed = parse_transcript_payload(&payload).expect("parses");
    assert!(
        parsed.strikes.is_empty(),
        "a composed single-cut EDL is not a researcher strike"
    );
    assert_eq!(parsed.layers.len(), 1, "it stays in the Layers strip");
}

// ── S6 reel composer pins ─────────────────────────────────────────────

fn reel_highlight(annotation_id: &str, start_word: usize, end_word: usize) -> PaneHighlight {
    PaneHighlight {
        start_word,
        end_word,
        label: format!("{annotation_id} label"),
        note: "n".into(),
        annotation_id: annotation_id.into(),
        researcher_supplied: true,
    }
}

fn reel_clip(
    annotation_id: &str,
    start_word: usize,
    end_word: usize,
    include: bool,
    strikes: &[(usize, usize)],
) -> ReelClip {
    ReelClip {
        annotation_id: annotation_id.into(),
        start_word,
        end_word,
        label: format!("{annotation_id} label"),
        include,
        strikes: strikes.to_vec(),
    }
}

/// The composer's state as comparable identities: (annotation id,
/// include, strikes) per clip, in composer order.
fn reel_clip_identities(viewer: &MediaViewer) -> Vec<(String, bool, Vec<(usize, usize)>)> {
    viewer
        .reel_clips
        .iter()
        .map(|clip| {
            (
                clip.annotation_id.clone(),
                clip.include,
                clip.strikes.clone(),
            )
        })
        .collect()
}

/// dcterms:identifier: `MediaViewer::rematch_reel_clips`
/// expect: Composer state — order, inclusion, and per-clip in-reel
/// strikes — survives every workspace reload, keyed to highlight
/// identity (annotation id + source range): a fresh load composes every
/// researcher highlight included in transcript order, an unrelated
/// annotation edit reloads the workspace and the deltas survive the
/// re-match, new researcher highlights join included at the tail,
/// deleted highlights drop their clips, and a rename refreshes the
/// clip's label. [P1]
#[gpui::test]
fn reel_composer_rebuilds_reorders_and_survives_reload(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| MediaViewer::new());

    // Load 1: three researcher highlights compose [a, b, c], all
    // included (the landed fresh-workspace semantics).
    viewer.update(cx, |viewer, cx| {
        let epoch = viewer
            .transcript_request
            .begin(RequestOwner::Transcript("t-reel".into()))
            .expect("the first load owns its request");
        let mut state = pane_state("t-reel");
        state.highlights = vec![
            reel_highlight("layer-a", 0, 3),
            reel_highlight("layer-b", 5, 8),
            reel_highlight("layer-c", 10, 12),
        ];
        viewer.apply_transcript_result(epoch, Ok(state), cx);
    });
    viewer.update(cx, |viewer, cx| {
        assert_eq!(
            reel_clip_identities(viewer),
            vec![
                ("layer-a".into(), true, vec![]),
                ("layer-b".into(), true, vec![]),
                ("layer-c".into(), true, vec![]),
            ]
        );
        // Composer deltas: b before a, c excluded, b's tail struck in-reel.
        viewer.move_reel_clip(1, true, cx);
        viewer.toggle_reel_clip_include("layer-c", 10, 12, cx);
        viewer.selection_popover = Some((5, 7));
        viewer.popover_strike_in_reel(cx);
    });

    // Reload 2 — an unrelated annotation edit mid-compose: the same
    // highlights with a renamed label, an unrelated strike elsewhere, a
    // model-pass row, and a NEW researcher highlight.
    viewer.update(cx, |viewer, cx| {
        let epoch = viewer
            .transcript_request
            .begin(RequestOwner::Transcript("t-reel".into()))
            .expect("the reload owns its request");
        let mut state = pane_state("t-reel");
        state.highlights = vec![
            PaneHighlight {
                label: "renamed a".into(),
                ..reel_highlight("layer-a", 0, 3)
            },
            reel_highlight("layer-b", 5, 8),
            reel_highlight("layer-c", 10, 12),
            reel_highlight("layer-d", 30, 32),
            PaneHighlight {
                researcher_supplied: false,
                ..reel_highlight("layer-model", 100, 110)
            },
        ];
        state.strikes = vec![PaneStrike {
            start_word: 40,
            end_word: 41,
            annotation_id: "unrelated-strike".into(),
        }];
        viewer.apply_transcript_result(epoch, Ok(state), cx);
    });
    viewer.update(cx, |viewer, _| {
        // The composer deltas survived the reload; d joined included at
        // the tail; the model row never composes.
        assert_eq!(
            reel_clip_identities(viewer),
            vec![
                ("layer-b".into(), true, vec![(5, 7)]),
                ("layer-a".into(), true, vec![]),
                ("layer-c".into(), false, vec![]),
                ("layer-d".into(), true, vec![]),
            ],
            "order, inclusion, and strikes survive the reload"
        );
        assert_eq!(
            viewer.reel_clips[1].label, "renamed a",
            "the re-match refreshes labels from the reloaded highlights"
        );
    });

    // Reload 3 — deleting a highlight drops its clip; the rest survives.
    viewer.update(cx, |viewer, cx| {
        let epoch = viewer
            .transcript_request
            .begin(RequestOwner::Transcript("t-reel".into()))
            .expect("the reload owns its request");
        let mut state = pane_state("t-reel");
        state.highlights = vec![
            reel_highlight("layer-a", 0, 3),
            reel_highlight("layer-b", 5, 8),
            reel_highlight("layer-d", 30, 32),
        ];
        viewer.apply_transcript_result(epoch, Ok(state), cx);
    });
    viewer.update(cx, |viewer, _| {
        assert_eq!(
            reel_clip_identities(viewer),
            vec![
                ("layer-b".into(), true, vec![(5, 7)]),
                ("layer-a".into(), true, vec![]),
                ("layer-d".into(), true, vec![]),
            ],
            "the deleted highlight's clip drops; the deltas survive"
        );
    });
}

/// dcterms:identifier: `composed_keep_ranges`
/// expect: Reorder changes the composed op order — the composed clip
/// list emits its Keep ops in composer order, and a later clip that
/// overlaps an earlier one repeats only its unplayed remainder (the
/// selection algebra admits each source span once — overlapping Keep
/// ops are rejected — so [b(5,15), a(0,10)] composes keep(5,15) then
/// keep(0,4), and the reverse order composes keep(0,10) then
/// keep(11,15)). [P1]
#[test]
fn reorder_changes_composed_op_order() {
    let state = pane_state("t-order");
    let b_first = [
        reel_clip("layer-b", 5, 15, true, &[]),
        reel_clip("layer-a", 0, 10, true, &[]),
    ];
    let params = compose_reel_params(&state, &b_first, ReelComposeMode::Assemble).expect("compose");
    let ops = params["layer"]["ops"].as_array().expect("ops");
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[0]["op"], serde_json::json!("keep"));
    assert_eq!(ops[0]["range"]["start_word"], serde_json::json!(5));
    assert_eq!(ops[0]["range"]["end_word"], serde_json::json!(15));
    assert_eq!(ops[1]["range"]["start_word"], serde_json::json!(0));
    assert_eq!(
        ops[1]["range"]["end_word"],
        serde_json::json!(4),
        "the later clip repeats only its unplayed remainder"
    );

    let a_first = [
        reel_clip("layer-a", 0, 10, true, &[]),
        reel_clip("layer-b", 5, 15, true, &[]),
    ];
    let params = compose_reel_params(&state, &a_first, ReelComposeMode::Assemble).expect("compose");
    let ops = params["layer"]["ops"].as_array().expect("ops");
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[0]["range"]["start_word"], serde_json::json!(0));
    assert_eq!(ops[0]["range"]["end_word"], serde_json::json!(10));
    assert_eq!(ops[1]["range"]["start_word"], serde_json::json!(11));
    assert_eq!(ops[1]["range"]["end_word"], serde_json::json!(15));
}

/// dcterms:identifier: `compose_reel_params` (S6 ordered form)
/// expect: An in-reel strike bakes into the clip's Keep op through the
/// server's published difference helper — the composer EDL carries
/// ONLY Keep ops (never a Cut op: the algebra unions ALL cuts globally,
/// so a per-clip strike as a Cut would strike every clip overlapping
/// the range), the struck sub-range splits the clip into its flanking
/// pieces, excluded clips compose nothing, and the pane_reel_compose
/// provenance keeps the ordered form outside the pane strike identity.
/// [P1]
#[test]
fn in_reel_strikes_bake_into_keeps_not_cuts() {
    let state = pane_state("t-bake");
    let clips = [
        reel_clip("layer-a", 0, 10, true, &[(3, 4)]),
        reel_clip("layer-b", 20, 24, false, &[]),
    ];
    let params = compose_reel_params(&state, &clips, ReelComposeMode::Assemble).expect("compose");
    assert_eq!(
        params["layer"]["provenance"]["prompt_template"],
        serde_json::json!("pane_reel_compose")
    );
    let ops = params["layer"]["ops"].as_array().expect("ops");
    assert_eq!(ops.len(), 2, "the strike splits the one clip's Keep op");
    assert!(
        ops.iter().all(|op| op["op"] == serde_json::json!("keep")),
        "a composer EDL carries no Cut ops — strikes are baked keeps"
    );
    assert_eq!(
        ops[0]["range"],
        serde_json::json!({"start_word": 0, "end_word": 2})
    );
    assert_eq!(
        ops[1]["range"],
        serde_json::json!({"start_word": 5, "end_word": 10})
    );

    // The ordered identity extension: the composed layer re-parsed as
    // a transcript layer renders no strike row (the pane strike
    // identity needs researcher_annotation + exactly one Cut op).
    let payload = serde_json::json!({
        "summary": {"id": "t-bake", "media_path": "/m", "words_count": 11, "has_word_timings": true},
        "working_transcript": {"text": "a b c", "alignment": "original", "words": [
            {"word": "a", "start_ms": 0, "end_ms": 100}
        ]},
        "layers": [
            {"id": "compose-ordered", "created_at": "c1", "layer": params["layer"].clone()}
        ],
        "exports": [], "renders": []
    });
    let parsed = parse_transcript_payload(&payload).expect("parses");
    assert!(
        parsed.strikes.is_empty(),
        "an ordered multi-Keep composer EDL is not a researcher strike"
    );
    assert_eq!(parsed.layers.len(), 1, "it stays in the Layers strip");
}

/// dcterms:identifier: `reel_preview_plan`
/// expect: The preview's word→time mapping reads the stored SOURCE
/// bundle words — the render path's own input — never the working
/// view's timings: over a re_anchored fixture whose working timings are
/// interpolated estimates over different indices, every preview span
/// equals `edl_to_clip_plan`'s output for the same composed EDL over
/// the source words, and differs from the working-view mapping. [P1]
#[test]
fn reel_preview_plan_reads_source_bundle_words() {
    let mut state = pane_state("t-preview");
    state.alignment = "re_anchored".into();
    // The source bundle words: six words, 0..=5, timings 0..1800ms.
    state.source_words = (0..6)
        .map(|index| PaneWord {
            word: format!("w{index}"),
            start_ms: index * 300,
            end_ms: index * 300 + 300,
        })
        .collect();
    // The re_anchored working view: interpolated estimates over
    // DIFFERENT indices — ten times the source timings.
    state.words = Some(
        (0..6)
            .map(|index| PaneWord {
                word: format!("w{index}"),
                start_ms: index * 3000,
                end_ms: index * 3000 + 3000,
            })
            .collect(),
    );
    let clips = [
        reel_clip("layer-b", 2, 5, true, &[]),
        reel_clip("layer-a", 0, 1, true, &[]),
    ];
    let spans = reel_preview_plan(&state, &clips).expect("plan");

    // The render path's own plan over the same composed EDL and the
    // SOURCE bundle words — the equality the preview must hold.
    let keep_ranges = composed_keep_ranges(&clips);
    let edl = hkask_media::transcript_select::Edl {
        ops: keep_ranges
            .iter()
            .map(|range| hkask_media::transcript_select::EdlEntry {
                range: *range,
                op: hkask_media::transcript_select::EdlOp::Keep,
            })
            .collect(),
    };
    let timed = |words: &[PaneWord]| -> Vec<hkask_media::transcript::TimedWord> {
        words
            .iter()
            .map(|word| hkask_media::transcript::TimedWord {
                word: word.word.clone(),
                start_ms: word.start_ms,
                end_ms: word.end_ms,
                confidence: None,
            })
            .collect()
    };
    let source_words = timed(&state.source_words);
    let expected = hkask_media::transcript_select::edl_to_clip_plan(&source_words, &edl)
        .expect("the render plan maps");
    assert_eq!(spans, expected, "preview spans equal the render plan");
    assert_eq!(
        spans,
        vec![(600, 1800), (0, 600)],
        "the spans follow the composed order over the SOURCE timings"
    );
    let working_words = timed(
        state
            .words
            .as_deref()
            .expect("the fixture carries working words"),
    );
    let working_plan = hkask_media::transcript_select::edl_to_clip_plan(&working_words, &edl)
        .expect("the working-view plan maps");
    assert_ne!(
        spans, working_plan,
        "the preview never reads the working view's timings"
    );
}

/// The committed preview fixture (a 3-second WAV, the widget crate's
/// playback-seek fixture copied in) — the real audio the scripted-widget
/// preview test loads, seeks, and reads positions from.
const REEL_PREVIEW_FIXTURE: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/test_data/reel-preview.wav");

fn assert_player_position_near(
    cx: &gpui::TestAppContext,
    widget: &gpui::Entity<hkask_media_widget::MediaWidget>,
    expected_ms: f64,
) {
    let position = widget
        .read_with(cx, |widget, _| widget.transport_position_secs())
        .expect("the player reports a position");
    let expected_secs = expected_ms / 1000.0;
    assert!(
        (position - expected_secs).abs() < 0.05,
        "the player position {position}s must sit at the seek target {expected_secs}s"
    );
}

/// dcterms:identifier: `MediaViewer::toggle_reel_preview`
/// expect: The preview plays the composed order on the shared player
/// without rendering: starting primes a seek to the first span's start
/// (observable as the real player's position landing off 0), the walk
/// holds while playback is paused (the playing gate), crossing a
/// span's end seeks the next span's start through the bound widget (a
/// backward jump natural playback never makes — the landing is the
/// seek's proof), and the last span's end finishes the preview. [P1]
#[gpui::test]
fn reel_preview_plays_spans_in_composed_order(cx: &mut gpui::TestAppContext) {
    hkask_media_widget::approve_gallery_media_path(std::path::Path::new(REEL_PREVIEW_FIXTURE))
        .expect("the committed preview fixture must be admittable");
    let widget = cx.new(|cx| {
        hkask_media_widget::MediaWidget::new(
            hkask_media_widget::MediaRef::new(
                REEL_PREVIEW_FIXTURE.into(),
                hkask_media_widget::MediaKind::Audio,
            ),
            cx,
        )
    });
    // Drive the widget's own pub load path (the production entry after
    // construction), then let the async file read + decode complete —
    // the error gate below is only meaningful once a load has run.
    widget.update(cx, |widget, cx| widget.load(cx));
    cx.run_until_parked();
    assert!(
        widget.read_with(cx, |widget, _| widget.error_message().is_none()),
        "the fixture must load — a seek on a failed (or never-run) load is a vacuous green"
    );

    // The workspace: source words over the 3-second fixture, the clips
    // composed [b(900..1500ms), a(0..600ms)].
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, _| {
        let mut state = pane_state("t-preview-order");
        state.source_words = (0..6)
            .map(|index| PaneWord {
                word: format!("w{index}"),
                start_ms: index * 300,
                end_ms: index * 300 + 300,
            })
            .collect();
        viewer.transcript = Some(state);
        viewer.reel_clips = vec![
            reel_clip("layer-b", 3, 4, true, &[]),
            reel_clip("layer-a", 0, 1, true, &[]),
        ];
        viewer.media_widget = Some(widget.clone());
        viewer.media_widget_body = Some("fixture".into());
        viewer.active_tab = ViewerTab::Workspace;
    });
    viewer.update(cx, |viewer, cx| viewer.toggle_reel_preview(cx));
    viewer.update(cx, |viewer, _| {
        let preview = viewer.reel_preview.as_ref().expect("the preview armed");
        assert_eq!(
            preview.spans,
            vec![(900, 1500), (0, 600)],
            "the span plan follows the composed order"
        );
    });
    // The prime seek landed on the real player: the position reads the
    // first span's start, off the 0.0 the fixture loads paused at.
    assert_player_position_near(cx, &widget, 900.);

    // Paused: the walk holds (the playing gate — the tick keeps polling
    // without advancing).
    viewer.update(cx, |viewer, cx| {
        assert!(
            viewer.reel_preview_tick(cx),
            "the tick keeps polling while paused"
        );
        assert_eq!(
            viewer.reel_preview.as_ref().expect("held").index,
            0,
            "the walk never advances a paused player"
        );
    });

    // The walk, scripted through the production step: crossing span 1's
    // end seeks span 2's start through the bound widget — the position
    // lands at 0, a backward jump natural playback never makes.
    viewer.update(cx, |viewer, cx| viewer.step_reel_preview(1_500, cx));
    assert_player_position_near(cx, &widget, 0.);

    // Crossing the last span's end finishes the preview.
    viewer.update(cx, |viewer, cx| {
        viewer.step_reel_preview(600, cx);
        assert!(viewer.reel_preview.is_none(), "the preview finished");
        assert!(
            viewer
                .status
                .as_deref()
                .is_some_and(|status| status.contains("finished")),
            "the finish surfaces: {:?}",
            viewer.status
        );
    });
}

/// dcterms:identifier: `MediaViewer::stop_reel_preview`
/// expect: The preview joins the sub-surface's teardown sites — leaving
/// the workspace, a fresh open, closing the transcript, and every reload
/// stop it and clear the span plan (the composer state itself survives
/// reloads; only the preview tears down), and the tick exits on
/// widget-None (hidden media clears the widget mid-preview). [P1]
#[gpui::test]
fn reel_preview_teardown_joins_the_surface_lifecycle(cx: &mut gpui::TestAppContext) {
    // Tab switches and opens dispatch reads through the shared
    // process-global invoker — the lock prevents cross-talk into a
    // concurrent lock-holder's scripted invoker.
    let _invoker_lock = INVOKER_TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let viewer = cx.new(|_| MediaViewer::new());
    let arm_preview = |viewer: &mut MediaViewer, cx: &mut gpui::Context<MediaViewer>| {
        let mut state = pane_state("t-preview-teardown");
        state.source_words = vec![PaneWord {
            word: "a".into(),
            start_ms: 0,
            end_ms: 300,
        }];
        viewer.transcript = Some(state);
        viewer.reel_clips = vec![reel_clip("layer-a", 0, 0, true, &[])];
        viewer.active_tab = ViewerTab::Workspace;
        viewer.toggle_reel_preview(cx);
        assert!(viewer.reel_preview.is_some(), "the preview armed");
        assert!(viewer.reel_preview_task.is_some());
    };

    // Leaving the workspace tears the preview down.
    viewer.update(cx, |viewer, cx| {
        arm_preview(viewer, cx);
        viewer.activate_tab(ViewerTab::Library, cx);
    });
    viewer.update(cx, |viewer, _| {
        assert!(
            viewer.reel_preview.is_none(),
            "leaving the workspace stops the preview"
        );
        assert!(viewer.reel_preview_task.is_none());
    });

    // A fresh open tears it down (open_transcript resets the surface).
    viewer.update(cx, |viewer, cx| {
        arm_preview(viewer, cx);
        viewer.open_transcript("t-other".into(), cx);
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert!(
            viewer.reel_preview.is_none(),
            "a fresh open stops the preview"
        );
        assert!(viewer.reel_preview_task.is_none());
    });

    // The reload path tears it down — while the composer state re-matches.
    viewer.update(cx, |viewer, cx| {
        arm_preview(viewer, cx);
        let epoch = viewer
            .transcript_request
            .begin(RequestOwner::Transcript("t-preview-teardown".into()))
            .expect("the reload owns its request");
        let mut state = pane_state("t-preview-teardown");
        state.source_words = vec![PaneWord {
            word: "a".into(),
            start_ms: 0,
            end_ms: 300,
        }];
        state.highlights = vec![reel_highlight("layer-a", 0, 0)];
        viewer.apply_transcript_result(epoch, Ok(state), cx);
        assert!(
            viewer.reel_preview.is_none(),
            "the reload stops the preview"
        );
        assert!(viewer.reel_preview_task.is_none());
        assert_eq!(
            reel_clip_identities(viewer),
            vec![("layer-a".into(), true, vec![])],
            "the composer state re-matched across the same reload"
        );
    });

    // widget-None: the tick exits and clears (hidden media suspends or
    // the selection changes mid-preview).
    viewer.update(cx, |viewer, cx| {
        arm_preview(viewer, cx);
        viewer.clear_media_widget(cx);
        assert!(
            !viewer.reel_preview_tick(cx),
            "the tick exits on widget-None"
        );
        assert!(viewer.reel_preview.is_none());
        assert!(viewer.reel_preview_task.is_none());
    });

    // Closing the transcript tears it down.
    viewer.update(cx, |viewer, cx| {
        arm_preview(viewer, cx);
        viewer.close_transcript_workspace(cx);
    });
    viewer.update(cx, |viewer, _| {
        assert!(
            viewer.reel_preview.is_none(),
            "closing the transcript stops the preview"
        );
    });
}

/// dcterms:identifier: `resolved_highlight_runs`
/// expect: Overlapping and out-of-order marks compose into the sorted,
/// non-overlapping run list gpui consumes — a struck word inside a
/// highlight keeps BOTH styles (its run carries the background and
/// the line-through), so the follow-along mark no longer shifts
/// styling past its range. [P1]
#[test]
fn resolved_highlight_runs_sorts_merges_and_keeps_both_styles() {
    let background = HighlightStyle {
        background_color: Some(gpui::black()),
        ..Default::default()
    };
    let strike = HighlightStyle {
        strikethrough: Some(StrikethroughStyle {
            thickness: px(1.),
            color: None,
        }),
        ..Default::default()
    };
    // Out of order, with the strike overlapping the highlight.
    let runs = resolved_highlight_runs(vec![(10..12, strike), (0..6, background), (4..8, strike)]);
    assert_eq!(
        runs.len(),
        4,
        "0..4 background, 4..6 both, 6..8 strike, 10..12 strike"
    );
    assert_eq!(runs[0].0, 0..4);
    assert!(runs[0].1.background_color.is_some());
    assert!(runs[0].1.strikethrough.is_none());
    assert_eq!(runs[1].0, 4..6);
    assert!(
        runs[1].1.background_color.is_some() && runs[1].1.strikethrough.is_some(),
        "the overlap keeps both styles"
    );
    assert_eq!(runs[2].0, 6..8);
    assert!(runs[2].1.strikethrough.is_some());
    assert!(runs[2].1.background_color.is_none());
    assert_eq!(runs[3].0, 10..12);
    // No marks, no runs — the plain text path.
    assert!(resolved_highlight_runs(Vec::new()).is_empty());
}

/// dcterms:identifier: `resolved_highlight_runs` (search composition)
/// expect: Search marks compose with the researcher overlays field-wise
/// — a match pushed after a highlight keeps its own background (the
/// match stays visible inside the researcher's range), a struck match
/// keeps the line-through, and the current match paints over the plain
/// match (the render's push order: highlight, strike, match, current).
/// [P1] Motivating: search must find words inside highlights and cuts —
/// an overlay-erased match is a false negative on screen (S3 AC3).
#[test]
fn search_marks_compose_with_highlight_and_strike_overlays() {
    let range = 2..6;
    let highlight = HighlightStyle {
        background_color: Some(highlight_label_color("thesis")),
        ..Default::default()
    };
    let strike = HighlightStyle {
        strikethrough: Some(StrikethroughStyle {
            thickness: px(1.),
            color: None,
        }),
        ..Default::default()
    };
    let match_style = HighlightStyle {
        background_color: Some(gpui::white()),
        ..Default::default()
    };
    let current_style = HighlightStyle {
        background_color: Some(gpui::black()),
        ..Default::default()
    };
    let runs = resolved_highlight_runs(vec![
        (range.clone(), highlight),
        (range.clone(), strike),
        (range.clone(), match_style),
    ]);
    let merged = runs.first().expect("the marked range resolves");
    assert_eq!(merged.0, range, "the composed run keeps the word range");
    assert_eq!(
        merged.1.background_color,
        Some(gpui::white()),
        "the match's background wins over the highlight's — a match inside a highlight stays visible"
    );
    assert!(
        merged.1.strikethrough.is_some(),
        "a struck match keeps its line-through"
    );

    let runs = resolved_highlight_runs(vec![(range.clone(), match_style), (range, current_style)]);
    let merged = runs.first().expect("the marked range resolves");
    assert_eq!(
        merged.1.background_color,
        Some(gpui::black()),
        "the current match paints over the plain match"
    );
}

/// dcterms:identifier: `block_word_char_range`
/// expect: The block-owned word-to-char mapping gates ranges that
/// miss the block, clamps partial overlaps to the block bounds, and
/// returns None when the clamped indices fall outside the word map
/// (the R2 refactor's behavior-equivalence pin). [P1]
#[test]
fn block_word_char_range_gates_clamps_and_bounds() {
    let block = PaneBlock {
        speaker: None,
        text: "a b c".into(),
        word_char_ranges: vec![0..1, 2..3, 4..5],
        word_starts: vec![(0, 0), (1, 100), (2, 200)],
    };
    assert_eq!(block_word_char_range(&block, 0, 2), Some(0..5));
    assert_eq!(block_word_char_range(&block, 1, 1), Some(2..3));
    assert_eq!(
        block_word_char_range(&block, 5, 9),
        None,
        "a range missing the block maps nothing"
    );
    // A text-only block has no word map: nothing maps, even word 0.
    assert_eq!(block_word_char_range(&PaneBlock::default(), 0, 0), None);
}

/// dcterms:identifier: `MediaViewer::strike_selected_range`
/// expect: An in-flight edit refuses the strike and keeps the editor
/// open for retry — the confirm path's admission contract, pinned on
/// the strike arm. [P1]
#[gpui::test]
fn strike_admission_refusal_keeps_the_editor(cx: &mut gpui::TestAppContext) {
    let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(pane_state("t-1"));
        viewer.edit_request.state = ResourceState::Loading;
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::Create {
                start_word: 3,
                end_word: 9,
            },
            buffer: LabelBuffer::from_str("typed but refused"),
        });
        viewer.strike_selected_range(3, 9, cx);
        assert!(
            viewer.highlight_edit.is_some(),
            "a refused strike keeps the editor for retry"
        );
        assert!(
            viewer
                .status
                .as_deref()
                .unwrap()
                .contains("already in progress"),
            "status: {:?}",
            viewer.status
        );
        assert_eq!(viewer.edit_request.state, ResourceState::Loading);
    });
}

/// dcterms:identifier: `MediaViewer::delete_workspace_annotation`
/// expect: The kind-dispatched delete refusal names its subject — the
/// strike arm says strikes, the highlight arm stays byte-identical to
/// the pre-R2 message — both surfacing the unwired invoker and
/// completing the edit request into Failed. [P1]
#[gpui::test]
fn annotation_delete_refusals_name_the_subject(cx: &mut gpui::TestAppContext) {
    let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(pane_state("t-1"));
        viewer.delete_workspace_annotation("layer-strike".into(), "Strike", cx);
        assert!(
            viewer
                .status
                .as_deref()
                .unwrap()
                .contains("cannot delete strikes"),
            "status: {:?}",
            viewer.status
        );
        assert!(matches!(
            viewer.edit_request.state,
            ResourceState::Failed(_)
        ));
        viewer.delete_workspace_annotation("layer-highlight".into(), "Highlight", cx);
        assert!(
            viewer
                .status
                .as_deref()
                .unwrap()
                .contains("cannot delete highlights"),
            "the highlight message is unchanged: {:?}",
            viewer.status
        );
    });
}

/// dcterms:identifier: `MediaViewer::compose_workspace_reel`
/// expect: A compose dispatch runs under the edit admission — the
/// unwired invoker completes the request into Failed with progress
/// cleared — and a mode with nothing to compose surfaces its per-mode
/// empty status without admitting an edit. [P1]
#[gpui::test]
fn compose_dispatch_refusal_and_empty_status_surface(cx: &mut gpui::TestAppContext) {
    let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        let mut state = pane_state("t-1");
        state.highlights = vec![PaneHighlight {
            start_word: 0,
            end_word: 2,
            label: "thesis".into(),
            note: String::new(),
            annotation_id: "layer-0".into(),
            researcher_supplied: true,
        }];
        state.strikes = vec![PaneStrike {
            start_word: 4,
            end_word: 5,
            annotation_id: "strike-4".into(),
        }];
        viewer.transcript = Some(state);
        viewer.reel_clips = vec![reel_clip("layer-0", 0, 2, true, &[])];
        viewer.compose_workspace_reel(ReelComposeMode::Assemble, cx);
        assert!(
            matches!(viewer.edit_request.state, ResourceState::Failed(_)),
            "the unwired compose refusal fails the edit request"
        );
        assert!(
            viewer.edit_progress.is_none(),
            "the refusal cleared the compose progress"
        );
        // Nothing included: the assemble empty status surfaces
        // without admitting an edit.
        viewer.edit_request.state = ResourceState::Idle;
        viewer.reel_clips = vec![reel_clip("layer-0", 0, 2, false, &[])];
        viewer.compose_workspace_reel(ReelComposeMode::Assemble, cx);
        assert!(
            viewer
                .status
                .as_deref()
                .unwrap()
                .contains("No included highlights"),
            "status: {:?}",
            viewer.status
        );
        assert_eq!(viewer.edit_request.state, ResourceState::Idle);
    });
}

/// dcterms:identifier: `transcript_media_body`
/// expect: The workspace media body infers kind by extension, escapes
/// paths, and parses as the media block the widget registry consumes.
/// [P1] Motivating: the shared-player binding is only as good as the
/// body it keys by.
#[test]
fn transcript_media_body_infers_kind_and_parses() {
    let video = transcript_media_body("/generated/vonnegut.mp4");
    assert_eq!(video, r#"{"kind":"video","src":"/generated/vonnegut.mp4"}"#);
    let audio = transcript_media_body("/generated/dunning-2017-audio.mp3");
    assert_eq!(
        audio,
        r#"{"kind":"audio","src":"/generated/dunning-2017-audio.mp3"}"#
    );
    // A path with JSON-hostile characters still parses and round-trips
    // the src through the block parser.
    let hostile = transcript_media_body("/generated/quote\"path.wav");
    let parsed =
        hkask_media_widget::MediaBlockBody::parse(&hostile).expect("escaped path still parses");
    assert_eq!(parsed.kind, "audio");
    assert_eq!(parsed.src, "/generated/quote\"path.wav");

    let parsed = hkask_media_widget::MediaBlockBody::parse(&video).expect("parses");
    assert_eq!(parsed.kind, "video");
    assert_eq!(parsed.src, "/generated/vonnegut.mp4");
}

/// dcterms:identifier: `current_word_index`
/// expect: The marked word is the last whose start is at or before the
/// position — never a word that starts after it.
/// [P1] Motivating: follow-along must mark the word being heard.
#[test]
fn current_word_index_marks_the_containing_word() {
    let words = vec![
        PaneWord {
            word: "a".into(),
            start_ms: 1000,
            end_ms: 1500,
        },
        PaneWord {
            word: "b".into(),
            start_ms: 2000,
            end_ms: 2500,
        },
        PaneWord {
            word: "c".into(),
            start_ms: 3000,
            end_ms: 3500,
        },
    ];
    assert_eq!(current_word_index(&words, 0), None);
    assert_eq!(current_word_index(&words, 1000), Some(0));
    assert_eq!(current_word_index(&words, 2100), Some(1));
    assert_eq!(current_word_index(&words, 3000), Some(2));
    assert_eq!(current_word_index(&words, 9999), Some(2));
    assert_eq!(current_word_index(&[], 100), None);
}

/// dcterms:identifier: `block_index_for_word`
/// expect: The mark-to-item mapping mirrors the block builder's
/// boundaries exactly — every word maps to the block that contains it,
/// so the follow-along scrolls the block the operator is hearing.
/// [P1] Motivating: a mapping drift would scroll the wrong block.
#[test]
fn block_index_for_word_matches_block_boundaries() {
    let state = TranscriptPaneState {
        transcript_id: "t-parity".into(),
        media_path: "/generated/parity.wav".into(),
        gallery_asset_id: None,
        alignment: "original".into(),
        alignment_note: None,
        words: Some(
            (0..50)
                .map(|index| PaneWord {
                    word: format!("parity-word-{index}"),
                    start_ms: index as u64 * 100,
                    end_ms: index as u64 * 100 + 90,
                })
                .collect(),
        ),
        working_text: String::new(),
        paragraph_breaks: Some(vec![9, 19, 29, 39]),
        speaker_spans: None,
        review: None,
        layers: Vec::new(),
        highlights: Vec::new(),
        strikes: Vec::new(),
        correction_edits: Vec::new(),
        source_words: Vec::new(),
    };
    let blocks = build_pane_blocks(&state);
    assert_eq!(blocks.len(), 5, "four breaks split 50 words into 5 blocks");
    for (block_index, block) in blocks.iter().enumerate() {
        for (word, _) in &block.word_starts {
            assert_eq!(
                block_index_for_word(&state, *word),
                Some(block_index),
                "word {word} must map to its own block"
            );
        }
    }
    assert_eq!(
        block_index_for_word(&state, 50),
        None,
        "a past-the-end word has no block"
    );
    let no_breaks = TranscriptPaneState {
        paragraph_breaks: None,
        ..state
    };
    assert_eq!(
        block_index_for_word(&no_breaks, 49),
        Some(0),
        "without breaks every word maps to the single block"
    );
}

/// dcterms:identifier: `ConfirmArm`
/// expect: The shared two-click confirm: the first click arms, the same
/// row's next click confirms and consumes the arm, a different row's
/// click re-aims at it (a confirm never acts on the row beside the
/// armed one), and disarm leaves no aim. One pattern serves every row
/// family — Library indexes and transcript ids alike (R4).
/// [P1] Motivating: two hand-rolled confirm state machines drifted
/// apart in the disarm paths; one implementation cannot.
#[test]
fn confirm_arm_arms_confirms_and_disarms() {
    let mut arm = ConfirmArm::default();
    assert!(
        !arm.confirm(&"row-a"),
        "the first click arms, never confirms"
    );
    assert!(arm.is_armed(&"row-a"), "the clicked row is armed");
    assert!(
        !arm.confirm(&"row-b") && arm.is_armed(&"row-b"),
        "clicking another row re-aims at it"
    );
    assert!(arm.confirm(&"row-b"), "the armed row's next click confirms");
    assert!(arm.armed.is_none(), "confirm consumes the arm");
    arm.confirm(&"row-c");
    arm.disarm();
    assert!(!arm.is_armed(&"row-c"), "disarm leaves no aim");
}

/// dcterms:identifier: `build_pane_blocks`
/// expect: Blocks honor breaks, align char ranges with the word text,
/// and carry the global indices and start times the click map consumes.
/// [P1] Motivating: click-to-seek and follow-along are only as good as
/// the mapping tables.
#[test]
fn pane_blocks_align_char_ranges_and_word_starts() {
    let state = TranscriptPaneState {
        transcript_id: "t".into(),
        media_path: "/m".into(),
        gallery_asset_id: None,
        alignment: "original".into(),
        alignment_note: None,
        words: Some(vec![
            PaneWord {
                word: "hello".into(),
                start_ms: 100,
                end_ms: 200,
            },
            PaneWord {
                word: "world".into(),
                start_ms: 300,
                end_ms: 400,
            },
            PaneWord {
                word: "again".into(),
                start_ms: 500,
                end_ms: 600,
            },
        ]),
        working_text: "hello world again".into(),
        paragraph_breaks: Some(vec![1]),
        review: None,
        speaker_spans: Some(vec![PaneSpeakerSpan {
            start_word: 0,
            end_word: 2,
            speaker: "Vonnegut".into(),
        }]),
        layers: Vec::new(),
        highlights: Vec::new(),
        strikes: Vec::new(),
        correction_edits: Vec::new(),
        source_words: Vec::new(),
    };
    let blocks = build_pane_blocks(&state);
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].text, "hello world");
    assert_eq!(blocks[0].word_char_ranges, vec![0..5, 6..11]);
    assert_eq!(blocks[0].word_starts, vec![(0, 100), (1, 300)]);
    assert_eq!(blocks[1].text, "again");
    assert_eq!(blocks[1].word_char_ranges, vec![0..5]);
    assert_eq!(blocks[1].word_starts, vec![(2, 500)]);
    assert_eq!(blocks[0].speaker.as_deref(), Some("Vonnegut"));

    // Text-only views yield one unlabeled block with no word map.
    let mut text_only = state;
    text_only.words = None;
    let blocks = build_pane_blocks(&text_only);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].text, "hello world again");
    assert!(blocks[0].word_char_ranges.is_empty());
    assert!(blocks[0].word_starts.is_empty());
}

/// dcterms:identifier: `MediaViewer::apply_transcript_result`
/// expect: A newer transcript request owns the workspace even if an older outcome arrives later.
/// [P1] Motivating: overlapping transcript loads cannot regress the visible workspace.
#[gpui::test]
fn overlapping_transcript_requests_drop_stale_errors(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        let old_request = viewer
            .transcript_request
            .begin(RequestOwner::Transcript("t-old".into()))
            .expect("old transcript request starts");
        let new_request = viewer
            .transcript_request
            .begin(RequestOwner::Transcript("t-new".into()))
            .expect("new transcript request supersedes");
        let state = TranscriptPaneState {
            transcript_id: "t-new".into(),
            media_path: "/m".into(),
            gallery_asset_id: None,
            alignment: "original".into(),
            alignment_note: None,
            words: Some(vec![PaneWord {
                word: "hi".into(),
                start_ms: 0,
                end_ms: 1,
            }]),
            working_text: "hi".into(),
            paragraph_breaks: None,
            speaker_spans: None,
            review: None,
            layers: Vec::new(),
            highlights: Vec::new(),
            strikes: Vec::new(),
            correction_edits: Vec::new(),
            source_words: Vec::new(),
        };
        viewer.apply_transcript_result(new_request, Ok(state.clone()), cx);
        viewer.apply_transcript_result(old_request, Err("stale error".into()), cx);
        assert_eq!(viewer.transcript_request.state, ResourceState::Ready);
        assert_eq!(viewer.transcript.as_ref(), Some(&state));

        let failure = viewer
            .transcript_request
            .begin(RequestOwner::Transcript("t-new".into()))
            .expect("failure request starts");
        viewer.apply_transcript_result(
            failure,
            Err("educt_get_transcript failed: unreachable".into()),
            cx,
        );
        assert_eq!(
            viewer.transcript_request.state,
            ResourceState::Failed("educt_get_transcript failed: unreachable".into())
        );
        // Retention semantics (mirrors apply_detail_result): the apply
        // path never clears on failure — the real load path clears at
        // dispatch start, so a failed reload leaves the pane empty while
        // a lifecycle-level failure retains the last-good view.
        assert_eq!(viewer.transcript.as_ref(), Some(&state));
    });
}

// ── Visual-transcript surface (S5) ───────────────────────────────────

/// A canned `layers_transcript` response over two events: a shot row
/// (no box) and an object row carrying the box that arms a redaction
/// target.
fn visual_events_response() -> serde_json::Value {
    serde_json::json!({
        "status": "completed",
        "media_path": "/rec/one.mp4",
        "events": [
            {
                "start_sec": 0.0, "end_sec": 4.2, "kind": "shot",
                "layer_id": "layer_shot", "item_id": "item_s0",
                "label": null, "searchable_text": "shot 0",
                "payload": {"shot_index": 0},
                "frame_asset_ids": [],
                "provenance": {"model": "ffmpeg:scdet", "prompt_template": "ffmpeg:scdet", "created_at": "2026-10-30T00:00:00Z"},
                "evidence": ["seen"]
            },
            {
                "start_sec": 3.5, "end_sec": 9.0, "kind": "object",
                "layer_id": "layer_obj", "item_id": "item_a",
                "label": "Grounded pass", "searchable_text": "object: person",
                "payload": {"label": "person", "confidence": 0.9, "box": {"x": 0.1, "y": 0.2, "w": 0.3, "h": 0.4}, "frame_index": 2},
                "frame_asset_ids": ["frame-1"],
                "provenance": {"model": "qwen3-vl", "prompt_template": "layers_semantic_pass_grounded", "created_at": "2026-10-30T00:00:00Z"},
                "evidence": ["heard", "seen"]
            }
        ],
        "events_total": 2, "events_page": 2,
        "kinds_present": ["shot", "object"],
        "layers_included": ["layer_shot", "layer_obj"],
        "resume_from": 0, "next_from": null,
        "time_basis": "source_pts_seconds"
    })
}

/// A viewer with an open transcript workspace over `/rec/one.mp4` —
/// the events-surface tests' shared fixture.
fn events_workspace_viewer() -> MediaViewer {
    let mut viewer = MediaViewer::new();
    viewer.transcript = Some(TranscriptPaneState {
        transcript_id: "t-1".into(),
        media_path: "/rec/one.mp4".into(),
        gallery_asset_id: Some("ga-1".into()),
        alignment: "original".into(),
        alignment_note: None,
        words: None,
        working_text: String::new(),
        paragraph_breaks: None,
        speaker_spans: None,
        review: None,
        layers: Vec::new(),
        highlights: Vec::new(),
        strikes: Vec::new(),
        correction_edits: Vec::new(),
        source_words: Vec::new(),
    });
    viewer
}

/// A scripted invoker for the S5 dispatches: records every (tool, args)
/// call and answers from a queue of canned response texts.
struct ScriptedInvoker {
    calls: std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>,
    responses: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<String>>>,
}

impl hkask_tool_invoker::ToolInvoker for ScriptedInvoker {
    fn invoke_tool(
        &self,
        server: &str,
        tool: &str,
        args: Value,
    ) -> gpui::Task<Result<String, hkask_tool_invoker::InvokeError>> {
        assert_eq!(server, MEDIA_SERVER);
        self.calls
            .lock()
            .expect("calls lock")
            .push((tool.to_string(), args));
        let response = self
            .responses
            .lock()
            .expect("responses lock")
            .pop_front()
            .unwrap_or_else(|| "{}".to_string());
        gpui::Task::ready(Ok(response))
    }
}

/// Install a scripted invoker, returning (recorded-calls, restore). The
/// caller holds INVOKER_TEST_LOCK for the test's lifetime — the same
/// serialization discipline as the direct-install invoker tests.
struct InvokerRestore(Option<std::sync::Arc<dyn hkask_tool_invoker::ToolInvoker>>);
impl Drop for InvokerRestore {
    fn drop(&mut self) {
        hkask_tool_invoker::set_tool_invoker(self.0.take());
    }
}

fn install_scripted_invoker(
    responses: Vec<String>,
) -> (
    std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>,
    InvokerRestore,
) {
    let restore = InvokerRestore(hkask_tool_invoker::shared_tool_invoker());
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    hkask_tool_invoker::set_tool_invoker(Some(std::sync::Arc::new(ScriptedInvoker {
        calls: calls.clone(),
        responses: std::sync::Arc::new(std::sync::Mutex::new(responses.into_iter().collect())),
    })));
    (calls, restore)
}

/// dcterms:identifier: `parse_visual_events`
/// expect: The event log parses strictly or rejects, never degrading to
/// a partial success; the no_layers state is its own named status.
#[test]
fn parse_visual_events_is_strict() {
    let parsed =
        parse_visual_events(&visual_events_response()).expect("the completed response parses");
    assert_eq!(parsed.status, "completed");
    assert_eq!(parsed.events_total, 2);
    assert_eq!(
        parsed.kinds_present,
        vec!["shot".to_string(), "object".to_string()]
    );
    assert_eq!(parsed.events.len(), 2);
    let object = &parsed.events[1];
    assert_eq!(object.box_norm, Some([0.1, 0.2, 0.3, 0.4]));
    assert_eq!(object.object_label.as_deref(), Some("person"));
    assert_eq!(object.source_ref(), "layer_obj/item_a");
    assert_eq!(object.model, "qwen3-vl");
    let shot = &parsed.events[0];
    assert_eq!(shot.box_norm, None);
    assert_eq!(shot.end_or_start(), 4.2);

    let no_layers = serde_json::json!({
        "status": "no_layers", "media_path": "/rec/one.mp4", "events": [],
        "note": "no stored layers for this recording"
    });
    let parsed = parse_visual_events(&no_layers).expect("no_layers parses");
    assert_eq!(parsed.status, "no_layers");
    assert!(parsed.events.is_empty());
    assert_eq!(
        parsed.note.as_deref(),
        Some("no stored layers for this recording")
    );

    let object_event = &visual_events_response()["events"][1];
    for invalid in [
        serde_json::json!({}),
        serde_json::json!({"status": "pending", "events": []}),
        serde_json::json!({"status": "completed"}),
        serde_json::json!({"status": "no_layers", "events": [object_event.clone()]}),
        serde_json::json!({"status": "completed", "events": [serde_json::json!({
            "end_sec": 4.0, "kind": "shot", "layer_id": "l", "item_id": "i",
            "searchable_text": "x", "provenance": {"model": "m"}
        })]}),
        serde_json::json!({"status": "completed", "events": [serde_json::json!({
            "start_sec": -1.0, "end_sec": 4.0, "kind": "shot", "layer_id": "l", "item_id": "i",
            "searchable_text": "x", "provenance": {"model": "m"}
        })]}),
        serde_json::json!({"status": "completed", "events": [serde_json::json!({
            "start_sec": 5.0, "end_sec": 4.0, "kind": "shot", "layer_id": "l", "item_id": "i",
            "searchable_text": "x", "provenance": {"model": "m"}
        })]}),
        serde_json::json!({"status": "completed", "events": [serde_json::json!({
            "start_sec": 0.0, "end_sec": 4.0, "kind": "  ", "layer_id": "l", "item_id": "i",
            "searchable_text": "x", "provenance": {"model": "m"}
        })]}),
        serde_json::json!({"status": "completed", "events": [serde_json::json!({
            "start_sec": 0.0, "end_sec": 4.0, "kind": "shot", "layer_id": "l", "item_id": "i",
            "searchable_text": "  ", "provenance": {"model": "m"}
        })]}),
        serde_json::json!({"status": "completed", "events": [serde_json::json!({
            "start_sec": 0.0, "end_sec": 4.0, "kind": "shot", "layer_id": "l", "item_id": "i",
            "searchable_text": "x"
        })]}),
        serde_json::json!({"status": "completed", "events": [object_event.clone()],
                "events_total": 1}),
    ] {
        let err = parse_visual_events(&invalid)
            .expect_err("malformed payloads must reject, never partially parse");
        assert!(!err.is_empty(), "rejection names the failure");
    }
}

/// dcterms:identifier: `VisualEventsState::truncation_note`
/// expect: A truncated page's note names the page-vs-total gap — the
/// count the operator reads never silently claims completeness; a
/// full page is not truncated and renders no note.
#[test]
fn truncation_note_names_page_and_total() {
    let mut state = parse_visual_events(&visual_events_response()).expect("fixture parses");
    assert!(!state.truncated(), "a full page is not truncated");
    state.events_total = 1200;
    assert!(state.truncated());
    assert_eq!(
        state.truncation_note(),
        "showing 2 of 1200 events — refine your search or window"
    );
}

/// dcterms:identifier: `parse_redaction_render`
/// expect: The S3 render result parses into the review/lineage summary
/// or rejects — segments and applied items carry their full detail.
#[test]
fn parse_redaction_render_is_strict() {
    let render = serde_json::json!({
        "status": "rendered",
        "result": {"output": "/published/redacted.mp4", "kind": "video"},
        "decision_layer_id": "layer-decision",
        "media_path": "/rec/one.mp4",
        "segments": [
            {"mode": "stream_copy", "start_sec": 0.0, "end_sec": 12.0, "items": []},
            {"mode": "reencode", "start_sec": 12.0, "end_sec": 18.0, "items": ["item_d1"]},
            {"mode": "stream_copy", "start_sec": 18.0, "end_sec": 20.0, "items": []}
        ],
        "reencoded_segments": 1,
        "stream_copied_segments": 2,
        "decisions_rendered": 1,
        "applied_items": [{
            "item_id": "item_d1", "effect": "blur",
            "pixel_x": 16, "pixel_y": 12, "pixel_w": 32, "pixel_h": 24,
            "local_start_sec": 0.4, "local_end_sec": 5.6, "delogo_edge_inset": false
        }]
    });
    let parsed = parse_redaction_render(&render).expect("the rendered result parses");
    assert_eq!(parsed.decision_layer_id, "layer-decision");
    assert_eq!(parsed.decisions_rendered, 1);
    assert_eq!(parsed.reencoded_segments, 1);
    assert_eq!(parsed.stream_copied_segments, 2);
    assert_eq!(parsed.segments.len(), 3);
    assert_eq!(
        parsed.segments[1],
        PaneRenderSegment {
            mode: "reencode".into(),
            start_sec: 12.0,
            end_sec: 18.0,
            items: 1,
        }
    );
    assert_eq!(parsed.applied.len(), 1);
    assert_eq!(parsed.applied[0].pixel_w, 32);
    assert_eq!(parsed.applied[0].local_end_sec, 5.6);

    for invalid in [
        serde_json::json!({}),
        serde_json::json!({"status": "failed"}),
        serde_json::json!({"status": "rendered"}),
        serde_json::json!({"status": "rendered", "decision_layer_id": "l"}),
        serde_json::json!({"status": "rendered", "decision_layer_id": "l", "decisions_rendered": 1,
                "reencoded_segments": 1, "stream_copied_segments": 1}),
        serde_json::json!({"status": "rendered", "decision_layer_id": "l", "decisions_rendered": 1,
                "reencoded_segments": 1, "stream_copied_segments": 1,
                "segments": [{"mode": "teleport", "start_sec": 0.0, "end_sec": 1.0, "items": []}]}),
        serde_json::json!({"status": "rendered", "decision_layer_id": "l", "decisions_rendered": 1,
                "reencoded_segments": 1, "stream_copied_segments": 1,
                "segments": [{"mode": "reencode", "start_sec": 0.0, "end_sec": 1.0, "items": []}],
                "applied_items": [{"item_id": "i", "effect": "blur", "pixel_x": 1}]}),
    ] {
        assert!(
            parse_redaction_render(&invalid).is_err(),
            "malformed render results must reject"
        );
    }
}

/// dcterms:identifier: `MediaViewer::handle_event_click`
/// expect: The event-click state machine mirrors the word pane's —
/// seek outside select mode, anchor → span inside it, inert while the
/// editor is open — with span bounds over source-PTS ranges and a
/// named refusal for degenerate spans.
#[gpui::test]
fn event_click_state_machine_and_span_bounds(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| {
        let mut viewer = events_workspace_viewer();
        viewer.visual_events = Some(VisualEventsState {
            status: "completed".into(),
            events: vec![
                PaneEventRow {
                    start_sec: 3.5,
                    end_sec: Some(9.0),
                    kind: "object".into(),
                    layer_id: "layer_obj".into(),
                    item_id: "item_a".into(),
                    searchable_text: "object: person".into(),
                    model: "qwen3-vl".into(),
                    box_norm: Some([0.1, 0.2, 0.3, 0.4]),
                    object_label: Some("person".into()),
                    evidence: vec!["seen".into()],
                },
                PaneEventRow {
                    start_sec: 12.4,
                    end_sec: Some(17.6),
                    kind: "researcher".into(),
                    layer_id: "layer_dec".into(),
                    item_id: "item_r1".into(),
                    searchable_text: "redact: person (blur)".into(),
                    model: "user".into(),
                    box_norm: None,
                    object_label: None,
                    evidence: vec!["authored".into()],
                },
                PaneEventRow {
                    start_sec: 5.0,
                    end_sec: None,
                    kind: "audio_event".into(),
                    layer_id: "layer_a".into(),
                    item_id: "item_p".into(),
                    searchable_text: "sound: clap".into(),
                    model: "user".into(),
                    box_norm: None,
                    object_label: None,
                    evidence: vec!["heard".into()],
                },
                PaneEventRow {
                    start_sec: 5.0,
                    end_sec: None,
                    kind: "audio_event".into(),
                    layer_id: "layer_a".into(),
                    item_id: "item_q".into(),
                    searchable_text: "sound: clap".into(),
                    model: "user".into(),
                    box_norm: None,
                    object_label: None,
                    evidence: vec!["heard".into()],
                },
            ],
            events_total: 4,
            kinds_present: vec!["object".into(), "researcher".into(), "audio_event".into()],
            note: None,
        });
        viewer
    });
    // Outside select mode every click seeks (criterion 1's routing).
    viewer.update(cx, |viewer, cx| {
        assert_eq!(viewer.handle_event_click(0), EventClickOutcome::SeekOnly);
        viewer.selection_mode = true;
        assert_eq!(
            viewer.handle_event_click(0),
            EventClickOutcome::AnchorSet(0)
        );
        viewer.event_selection_anchor = Some(0);
        assert_eq!(
            viewer.handle_event_click(0),
            EventClickOutcome::AnchorCleared
        );
        // The span covers from the earlier start to the later end,
        // regardless of click order.
        viewer.event_selection_anchor = Some(0);
        assert_eq!(
            viewer.handle_event_click(1),
            EventClickOutcome::SpanSelected {
                start_sec: 3.5,
                end_sec: 17.6
            }
        );
        viewer.event_selection_anchor = Some(1);
        assert_eq!(
            viewer.handle_event_click(0),
            EventClickOutcome::SpanSelected {
                start_sec: 3.5,
                end_sec: 17.6
            }
        );
        // A point event's end falls back to its start.
        viewer.event_selection_anchor = Some(2);
        assert_eq!(
            viewer.handle_event_click(0),
            EventClickOutcome::SpanSelected {
                start_sec: 3.5,
                end_sec: 9.0
            }
        );
        // Two same-instant point events cover no time — the named refusal.
        viewer.event_selection_anchor = Some(2);
        assert_eq!(
            viewer.handle_event_click(3),
            EventClickOutcome::SpanRejected
        );
        // The label editor is modal to event clicks.
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::EventTag {
                start_sec: 1.0,
                end_sec: 2.0,
            },
            buffer: LabelBuffer::default(),
        });
        assert_eq!(viewer.handle_event_click(0), EventClickOutcome::EditorOpen);
        // The degenerate-span refusal clears the anchor with a named status.
        viewer.highlight_edit = None;
        viewer.event_selection_anchor = Some(2);
        viewer.event_selection_span = None;
        viewer.event_clicked(3, 5.0, cx);
        assert_eq!(viewer.event_selection_anchor, None);
        assert_eq!(viewer.event_selection_span, None);
        let status = viewer.status.clone().expect("refusal surfaces a status");
        assert!(status.contains("span is empty"), "got: {status}");
        // The completed span routes through with its own status.
        viewer.event_selection_anchor = Some(0);
        viewer.event_clicked(1, 12.4, cx);
        assert_eq!(viewer.event_selection_span, Some((3.5, 17.6)));
        assert_eq!(viewer.event_selection_anchor, None);
    });
}

/// dcterms:identifier: `PaneRedactionEffect::next`
/// expect: The effect chip cycles the card's tier-1 vocabulary.
#[test]
fn redaction_effect_cycles_the_card_vocabulary() {
    assert_eq!(
        PaneRedactionEffect::Blur.next(),
        PaneRedactionEffect::Pixelate
    );
    assert_eq!(
        PaneRedactionEffect::Pixelate.next(),
        PaneRedactionEffect::Black
    );
    assert_eq!(PaneRedactionEffect::Black.next(), PaneRedactionEffect::Blur);
    assert_eq!(PaneRedactionEffect::Blur.as_str(), "blur");
    assert_eq!(PaneRedactionEffect::Pixelate.as_str(), "pixelate");
    assert_eq!(PaneRedactionEffect::Black.as_str(), "black");
}

/// dcterms:identifier: `MediaViewer::toggle_transcript_surface`
/// expect: Switching to Events dispatches exactly one pinned
/// `layers_transcript` call for the open recording; retained state means
/// switching back and forth re-dispatches nothing; the switch clears
/// pending surface-local selections.
#[gpui::test]
fn surface_toggle_switches_and_loads_events_once(cx: &mut gpui::TestAppContext) {
    let _invoker_guard = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (calls, _restore) = install_scripted_invoker(vec![
        serde_json::json!({
            "content": visual_events_response()
        })
        .to_string(),
    ]);
    let viewer = cx.new(|_| {
        let mut viewer = events_workspace_viewer();
        viewer.selection_mode = true;
        viewer.event_selection_anchor = Some(0);
        viewer.event_selection_span = Some((1.0, 2.0));
        viewer
    });
    assert_eq!(
        viewer.read_with(cx, |viewer, _| viewer.transcript_surface),
        TranscriptSurface::Words
    );
    viewer.update(cx, |viewer, cx| {
        viewer.toggle_transcript_surface(TranscriptSurface::Events, cx)
    });
    assert_eq!(
        viewer.read_with(cx, |viewer, _| viewer.transcript_surface),
        TranscriptSurface::Events
    );
    // The switch cleared the pending selections that belonged to the
    // words surface's select mode.
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(viewer.selection_anchor, None);
        assert_eq!(viewer.event_selection_anchor, None);
        assert_eq!(viewer.event_selection_span, None);
    });
    cx.run_until_parked();
    assert_eq!(
        viewer.read_with(cx, |viewer, _| viewer.visual_events_request.state.clone()),
        ResourceState::Ready
    );
    {
        let recorded = calls.lock().expect("calls lock");
        assert_eq!(recorded.len(), 1, "exactly one layers_transcript dispatch");
        assert_eq!(recorded[0].0, "layers_transcript");
        assert_eq!(
            recorded[0].1,
            serde_json::json!({ "media_path": "/rec/one.mp4", "limit": 5000 }),
            "the reading surface dispatches the pinned payload — the \
                 5000-event maximum page, never the 500-event default prefix"
        );
    }
    // Retained state: back to words and again dispatches nothing.
    viewer.update(cx, |viewer, cx| {
        viewer.toggle_transcript_surface(TranscriptSurface::Words, cx)
    });
    viewer.update(cx, |viewer, cx| {
        viewer.toggle_transcript_surface(TranscriptSurface::Events, cx)
    });
    cx.run_until_parked();
    assert_eq!(calls.lock().expect("calls lock").len(), 1);
}

/// dcterms:identifier: `MediaViewer::create_event_tag`
/// expect: Selecting a span and confirming the label editor authors a
/// researcher tag through `layers_store` with the exact human-act
/// payload (kind researcher, model "user", one item over the span),
/// then reloads the event log.
#[gpui::test]
fn select_to_author_tag_dispatches_strict_researcher_payload(cx: &mut gpui::TestAppContext) {
    let _invoker_guard = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let store_response = serde_json::json!({
        "content": {
            "id": "layer-tag-1", "media_path": "/rec/one.mp4",
            "gallery_asset_id": null, "detached_at": null,
            "kind": "researcher", "label": "hot moment",
            "provenance": {"model": "user", "prompt_template": "media-panel:select-to-author",
                "created_at": "2026-10-30T00:00:00Z", "parameters": null},
            "items_count": 1, "created_at": "2026-10-30T00:00:00Z",
            "probed_duration_secs": 20.0, "time_basis": "source_pts_seconds"
        }
    })
    .to_string();
    let (calls, _restore) = install_scripted_invoker(vec![
        store_response,
        serde_json::json!({"content": visual_events_response()}).to_string(),
    ]);
    let viewer = cx.new(|_| {
        let mut viewer = events_workspace_viewer();
        viewer.event_selection_span = Some((3.5, 9.0));
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::EventTag {
                start_sec: 3.5,
                end_sec: 9.0,
            },
            buffer: LabelBuffer::from_str("hot moment"),
        });
        viewer
    });
    viewer.update(cx, |viewer, cx| viewer.confirm_label_edit(cx));
    cx.run_until_parked();
    let calls = calls.lock().expect("calls lock");
    assert_eq!(calls.len(), 2, "store then reload");
    assert_eq!(calls[0].0, "layers_store");
    assert_eq!(
        calls[0].1,
        serde_json::json!({
            "media_path": "/rec/one.mp4",
            "gallery_asset_id": "ga-1",
            "kind": "researcher",
            "label": "hot moment",
            "provenance": {"model": "user", "prompt_template": "media-panel:select-to-author"},
            "items": [{
                "start_sec": 3.5,
                "end_sec": 9.0,
                "searchable_text": "tag: hot moment",
                "payload": {"label": "hot moment"},
            }],
        }),
        "select-to-author dispatches the exact researcher payload"
    );
    assert_eq!(calls[1].0, "layers_transcript");
    drop(calls);
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(
            viewer.event_selection_span, None,
            "the consumed span clears"
        );
        assert!(viewer.highlight_edit.is_none(), "the editor closed");
        let status = viewer.status.as_deref().expect("success surfaces a status");
        assert!(status.contains("tag stored"), "got: {status}");
    });
}

/// dcterms:identifier: `MediaViewer::arm_redaction_target`
/// expect: Only box-carrying events arm the redaction target, and the
/// armed target carries the OMC lineage source ref.
#[gpui::test]
fn target_arming_requires_a_box(cx: &mut gpui::TestAppContext) {
    let viewer = cx.new(|_| {
        let mut viewer = events_workspace_viewer();
        viewer.visual_events =
            Some(parse_visual_events(&visual_events_response()).expect("fixture parses"));
        viewer
    });
    viewer.update(cx, |viewer, cx| {
        viewer.arm_redaction_target(0, cx);
        assert_eq!(viewer.redaction_target, None, "the shot row has no box");
        let status = viewer.status.as_deref().expect("the refusal surfaces");
        assert!(status.contains("box"), "got: {status}");
        viewer.status = None;
        viewer.arm_redaction_target(1, cx);
        assert_eq!(
            viewer.redaction_target,
            Some(RedactionTarget {
                label: "person".into(),
                box_norm: [0.1, 0.2, 0.3, 0.4],
                source_ref: "layer_obj/item_a".into(),
            })
        );
    });
}

/// dcterms:identifier: `MediaViewer::dispatch_redaction_render`
/// expect: With target and span armed, the dispatch runs the pinned
/// two-invoke chain — an approved decision layer through `layers_store`,
/// then `layers_render_redactions` with the STORED layer id — commits
/// the segments/applied-items summary, and reloads the event log.
#[gpui::test]
fn redaction_chain_pins_store_and_render_payloads(cx: &mut gpui::TestAppContext) {
    let _invoker_guard = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let decision_store_response = serde_json::json!({
        "content": {
            "id": "layer-decision", "media_path": "/rec/one.mp4",
            "gallery_asset_id": null, "detached_at": null,
            "kind": "researcher", "label": "redaction decisions",
            "provenance": {"model": "user", "prompt_template": "media-panel:select-to-author",
                "created_at": "2026-10-30T00:00:00Z", "parameters": null},
            "items_count": 1, "created_at": "2026-10-30T00:00:00Z",
            "probed_duration_secs": 20.0, "time_basis": "source_pts_seconds"
        }
    })
    .to_string();
    let render_response = serde_json::json!({
        "content": {
            "status": "rendered",
            "result": {"output": "/published/redacted.mp4", "kind": "video"},
            "decision_layer_id": "layer-decision",
            "media_path": "/rec/one.mp4",
            "segments": [
                {"mode": "stream_copy", "start_sec": 0.0, "end_sec": 12.0, "items": []},
                {"mode": "reencode", "start_sec": 12.0, "end_sec": 18.0, "items": ["item-1"]},
                {"mode": "stream_copy", "start_sec": 18.0, "end_sec": 20.0, "items": []}
            ],
            "reencoded_segments": 1, "stream_copied_segments": 2, "decisions_rendered": 1,
            "applied_items": [{
                "item_id": "item-1", "effect": "blur",
                "pixel_x": 16, "pixel_y": 12, "pixel_w": 32, "pixel_h": 24,
                "local_start_sec": 0.4, "local_end_sec": 5.6, "delogo_edge_inset": false
            }]
        }
    })
    .to_string();
    let (calls, _restore) = install_scripted_invoker(vec![
        decision_store_response,
        render_response,
        serde_json::json!({"content": visual_events_response()}).to_string(),
    ]);
    let viewer = cx.new(|_| {
        let mut viewer = events_workspace_viewer();
        viewer.visual_events =
            Some(parse_visual_events(&visual_events_response()).expect("fixture parses"));
        viewer.event_selection_span = Some((3.5, 9.0));
        viewer
    });
    viewer.update(cx, |viewer, cx| {
        viewer.arm_redaction_target(1, cx);
        viewer.dispatch_redaction_render(cx);
    });
    cx.run_until_parked();
    let calls = calls.lock().expect("calls lock");
    assert_eq!(calls.len(), 3, "store, render, then the event-log reload");
    assert_eq!(calls[0].0, "layers_store");
    assert_eq!(
        calls[0].1,
        serde_json::json!({
            "media_path": "/rec/one.mp4",
            "gallery_asset_id": "ga-1",
            "kind": "researcher",
            "label": "redaction decisions",
            "provenance": {"model": "user", "prompt_template": "media-panel:select-to-author"},
            "items": [{
                "start_sec": 3.5,
                "end_sec": 9.0,
                "searchable_text": "redact: person (blur)",
                "payload": {
                    "op": "redact",
                    "status": "approved",
                    "effect": "blur",
                    "target": {"kind": "box", "box": {"x": 0.1, "y": 0.2, "w": 0.3, "h": 0.4}},
                    "source_tag_refs": ["layer_obj/item_a"],
                },
            }],
        }),
        "the decision layer carries the exact S3 redaction vocabulary"
    );
    assert_eq!(calls[1].0, "layers_render_redactions");
    assert_eq!(
        calls[1].1,
        serde_json::json!({
            "media_path": "/rec/one.mp4",
            "layer_id": "layer-decision",
        }),
        "the render dispatches the STORED decision layer id (required)"
    );
    assert_eq!(calls[2].0, "layers_transcript");
    drop(calls);
    viewer.read_with(cx, |viewer, _| {
        let summary = viewer
            .redaction_render
            .as_ref()
            .expect("the render summary commits for review/lineage");
        assert_eq!(summary.decision_layer_id, "layer-decision");
        assert_eq!(summary.decisions_rendered, 1);
        assert_eq!(summary.reencoded_segments, 1);
        assert_eq!(summary.stream_copied_segments, 2);
        assert_eq!(summary.segments.len(), 3);
        assert_eq!(summary.applied[0].pixel_w, 32);
        assert_eq!(
            viewer.event_selection_span, None,
            "the consumed span clears"
        );
        let status = viewer.status.as_deref().expect("success surfaces a status");
        assert!(status.contains("Redaction rendered"), "got: {status}");
    });
}

/// dcterms:identifier: `MediaViewer::dispatch_redaction_render`
/// expect: Missing target or span refuses with a named status and
/// dispatches nothing — the authoring gate runs before any tool call.
#[gpui::test]
fn redaction_refuses_without_target_or_span(cx: &mut gpui::TestAppContext) {
    let _invoker_guard = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (calls, _restore) = install_scripted_invoker(vec![]);
    let viewer = cx.new(|_| {
        let mut viewer = events_workspace_viewer();
        viewer.visual_events =
            Some(parse_visual_events(&visual_events_response()).expect("fixture parses"));
        viewer.event_selection_span = Some((3.5, 9.0));
        viewer
    });
    viewer.update(cx, |viewer, cx| {
        viewer.dispatch_redaction_render(cx);
        let status = viewer.status.as_deref().expect("the refusal surfaces");
        assert!(status.contains("Arm a target"), "got: {status}");
        viewer.arm_redaction_target(1, cx);
        viewer.event_selection_span = None;
        viewer.dispatch_redaction_render(cx);
        let status = viewer.status.as_deref().expect("the refusal surfaces");
        assert!(status.contains("Select a span"), "got: {status}");
    });
    assert_eq!(
        calls.lock().expect("calls lock").len(),
        0,
        "refusals dispatch nothing"
    );
}

/// expect: every argument object a panel builder produces decodes into
/// the media server's own request type for the tool it is sent to, and
/// a layer it carries parses as the server's layer type.
/// [P1] Constraining: the builders' tests asserted JSON shape against a
/// recording invoker, never against the server. The reel, correction,
/// tag and redaction builders send their layer and payload as objects;
/// the server's fields decoded JSON text only and refused all four
/// (measured 2026-11-12 on the installed server: "invalid type: map,
/// expected a string").
#[test]
fn every_argument_builder_decodes_into_the_servers_request_type() {
    use hkask_media::transcript_layers::TranscriptLayer;
    use hkask_media::wire::{
        EductAnnotationCreateRequest, EductAnnotationUpdateRequest, EductLocateRequest,
        EductRealignTranscriptRequest, EductStoreLayerRequest, LayersStoreRequest,
    };
    // The crate has no direct serde dependency; a macro needs no bound.
    macro_rules! decoded {
        ($request:ty, $tool:literal, $arguments:expr) => {{
            let arguments: serde_json::Value = $arguments;
            serde_json::from_value::<$request>(arguments.clone()).unwrap_or_else(|error| {
                panic!(
                    "{} refuses the panel's arguments: {error}\n{arguments}",
                    $tool
                )
            })
        }};
    }
    fn stores_a_layer(what: &str, arguments: serde_json::Value) {
        let request = decoded!(EductStoreLayerRequest, "educt_store_layer", arguments);
        if let Err(error) = serde_json::from_value::<TranscriptLayer>(request.layer.0) {
            panic!("{what}: the layer is not a server layer: {error}");
        }
    }

    decoded!(
        EductAnnotationCreateRequest,
        "educt_annotation_create",
        highlight_create_params("t-1", 0, 3, "label")
    );
    decoded!(
        EductAnnotationCreateRequest,
        "educt_annotation_create",
        strike_create_params("t-1", 4, 4)
    );
    decoded!(
        EductAnnotationUpdateRequest,
        "educt_annotation_update",
        highlight_update_params("t-1", "layer-0", 0, 3, "label", "note")
    );
    decoded!(
        EductRealignTranscriptRequest,
        "educt_realign_transcript",
        correction_realign_params("t-1", "layer-0")
    );
    decoded!(
        EductLocateRequest,
        "educt_locate",
        correction_locate_params("t-1", "a quote")
    );

    let edits = vec![PaneCorrectionEdit {
        start_word: 3,
        end_word: 5,
        replacement: "fixed words".into(),
        reason: CORRECTION_REASON.to_string(),
    }];
    stores_a_layer(
        "correction",
        correction_store_params("t-1", &edits, "2026-11-12T00:00:00Z").expect("params"),
    );

    let state = TranscriptPaneState {
        transcript_id: "t-1".into(),
        media_path: "/m".into(),
        gallery_asset_id: None,
        alignment: "original".into(),
        alignment_note: None,
        words: None,
        working_text: String::new(),
        paragraph_breaks: None,
        speaker_spans: None,
        review: None,
        layers: Vec::new(),
        highlights: vec![PaneHighlight {
            start_word: 0,
            end_word: 3,
            label: "a".into(),
            note: "n".into(),
            annotation_id: "layer-0".into(),
            researcher_supplied: true,
        }],
        strikes: vec![PaneStrike {
            start_word: 5,
            end_word: 5,
            annotation_id: "strike-5".into(),
        }],
        correction_edits: Vec::new(),
        source_words: Vec::new(),
    };
    let clips = [reel_clip("layer-0", 0, 3, true, &[])];
    for mode in [
        ReelComposeMode::Assemble,
        ReelComposeMode::Reduce,
        ReelComposeMode::Mixed,
    ] {
        stores_a_layer(
            "reel",
            compose_reel_params(&state, &clips, mode).expect("a reel to compose"),
        );
    }

    decoded!(
        LayersStoreRequest,
        "layers_store",
        researcher_tag_params("/m.mp4", Some("asset-1"), 1.0, 2.0, "label")
    );
    let target = RedactionTarget {
        label: "person".into(),
        box_norm: [0.1, 0.2, 0.3, 0.4],
        source_ref: "layer_obj/item_a".into(),
    };
    decoded!(
        LayersStoreRequest,
        "layers_store",
        redaction_decision_params(
            "/m.mp4",
            None,
            &target,
            (1.0, 2.0),
            PaneRedactionEffect::Blur,
        )
    );
}

/// S099-F04: truncating a job id for the activity drawer must slice on
/// Unicode scalar boundaries, never raw UTF-8 byte offsets.
#[test]
fn short_job_id_truncates_by_unicode_scalar_count() {
    // 8 Unicode scalars, 12 UTF-8 bytes — raw `&id[..8]` panics mid-character.
    assert_eq!(short_job_id("job-αβγδ").as_ref(), "job-αβγδ");
    assert_eq!(short_job_id("job-αβγδ-extra").as_ref(), "job-αβγδ");
    assert_eq!(short_job_id("abcdefghi").as_ref(), "abcdefgh");
    assert_eq!(short_job_id("short").as_ref(), "short");
}

/// S099-F11: closing or reopening a transcript workspace clears stale
/// reel clips and word-drag anchors; leaving the Workspace tab clears
/// transient selection popover, context menu, highlight editor, and
/// word-drag state.
#[gpui::test]
fn transcript_lifecycle_and_tab_switch_clear_transient_state(cx: &mut gpui::TestAppContext) {
    let _guard = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.active_tab = ViewerTab::Workspace;
        viewer.reel_clips = vec![reel_clip("ann-1", 0, 2, true, &[])];
        viewer.word_drag_anchor = Some(1);
        viewer.drag_last_word = Some(3);
        viewer.selection_popover = Some((1, 3));
        viewer.context_menu = Some((1, 3));
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::Create {
                start_word: 1,
                end_word: 3,
            },
            buffer: LabelBuffer::from_str("draft"),
        });

        viewer.activate_tab(ViewerTab::Library, cx);
        assert!(viewer.selection_popover.is_none());
        assert!(viewer.context_menu.is_none());
        assert!(viewer.highlight_edit.is_none());
        assert!(viewer.word_drag_anchor.is_none());
        assert!(viewer.drag_last_word.is_none());

        viewer.active_tab = ViewerTab::Workspace;
        viewer.reel_clips = vec![reel_clip("ann-1", 0, 2, true, &[])];
        viewer.word_drag_anchor = Some(0);
        viewer.drag_last_word = Some(2);
        viewer.close_transcript_workspace(cx);
        assert!(viewer.reel_clips.is_empty());
        assert!(viewer.word_drag_anchor.is_none());
        assert!(viewer.drag_last_word.is_none());

        viewer.reel_clips = vec![reel_clip("ann-2", 0, 1, true, &[])];
        viewer.word_drag_anchor = Some(0);
        viewer.drag_last_word = Some(1);
        viewer.open_transcript("tr-next".into(), cx);
        assert!(viewer.reel_clips.is_empty());
        assert!(viewer.word_drag_anchor.is_none());
        assert!(viewer.drag_last_word.is_none());
    });
}

#[cfg(test)]
mod edit_tests {
    use super::*;

    /// The edit-dispatch ingestion path: a `video_clip` result (the exact
    /// envelope shape the server emits — content-wrapped, display_hint
    /// inside) must surface as a new selected asset, the same way thread
    /// tool results do. Ground truth for the shape: the server's
    /// `enrich_with_omc_and_provenance` (hkask-mcp-media/src/media_block.rs).
    #[gpui::test]
    fn merge_tool_result_surfaces_clip_as_new_selected_asset(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            let output = serde_json::json!({
                "content": {
                    "status": "clipped",
                    "source": "/tmp/source.mp4",
                    "start_sec": 10.0,
                    "end_sec": 40.0,
                    "duration": 30.0,
                    "output": "/tmp/clip.mp4",
                    "display_hint": "```media\n{\"kind\":\"video\",\"src\":\"/tmp/clip.mp4\"}\n```"
                }
            })
            .to_string();

            viewer.ingest_tool_result(&Value::String(output), "video_clip", cx);

            assert_eq!(viewer.assets.len(), 1, "the clip must surface as an asset");
            let selected = viewer
                .selected
                .expect("the new asset must be auto-selected");
            assert_eq!(viewer.assets[selected].src, "/tmp/clip.mp4");
            assert_eq!(viewer.assets[selected].kind, "video");
            assert_eq!(viewer.assets[selected].tool, "video_clip");
        });
    }

    /// Dedup: re-ingesting the same result must not duplicate the asset
    /// (the same discipline `ingest_thread` applies to thread results).
    #[gpui::test]
    fn merge_tool_result_deduplicates_by_body(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            let output = serde_json::json!({
                "content": {
                    "status": "clipped",
                    "output": "/tmp/clip.mp4",
                    "display_hint": "```media\n{\"kind\":\"video\",\"src\":\"/tmp/clip.mp4\"}\n```"
                }
            })
            .to_string();

            let output = Value::String(output);
            viewer.ingest_tool_result(&output, "video_clip", cx);
            viewer.ingest_tool_result(&output, "video_clip", cx);

            assert_eq!(
                viewer.assets.len(),
                1,
                "identical results must not duplicate"
            );
        });
    }

    /// A result without a display_hint (or unparseable output) must not
    /// crash or add phantom assets — it is a no-op the status line already
    /// covers by clearing on success.
    #[gpui::test]
    fn merge_tool_result_ignores_output_without_display_hint(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.ingest_tool_result(
                &serde_json::json!({"content": {"status": "clipped"}}),
                "video_clip",
                cx,
            );
            assert!(viewer.assets.is_empty());
            viewer.ingest_tool_result(&Value::String("not json at all".into()), "video_clip", cx);
            assert!(viewer.assets.is_empty());
        });
    }

    /// expect: `export_transcript` dispatches `educt_export` for `web_annotation`,
    /// `webvtt`, and `otio` (with optional `layer_id`) on its own read lifecycle
    /// (independent of `edit_request`), surfaces the durable `output` path, and
    /// preserves last-good `export_result` when a subsequent export fails. [P1]
    #[gpui::test]
    fn export_transcript_dispatches_standards_formats_and_surfaces_path(
        cx: &mut gpui::TestAppContext,
    ) {
        let _invoker_guard = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (calls, _restore) = install_scripted_invoker(vec![
            serde_json::json!({
                "content": {
                    "export_id": "exp-wa",
                    "format": "web_annotation",
                    "output": "/tmp/exports/t1-highlights.annotations.json",
                    "status": "exported",
                }
            })
            .to_string(),
            serde_json::json!({
                "content": {
                    "export_id": "exp-vtt",
                    "format": "webvtt",
                    "output": "/tmp/exports/t1.vtt",
                    "status": "exported",
                }
            })
            .to_string(),
            serde_json::json!({
                "content": {
                    "export_id": "exp-otio",
                    "format": "otio",
                    "output": "/tmp/exports/t1-reel.otio",
                    "status": "exported",
                }
            })
            .to_string(),
            // Malformed payload -> failure must preserve last-good export_result.
            "not json".to_string(),
        ]);

        let viewer = cx.new(|_| MediaViewer::new());
        // Simulate an in-flight edit: exports ride their own read lifecycle (axis 8)
        // and must not be blocked by `edit_request`.
        viewer.update(cx, |viewer, cx| {
            let _ = viewer.edit_request.begin(RequestOwner::Edit("busy"));
            viewer.export_transcript("tr-1".to_string(), "web_annotation", None, cx);
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, cx| {
            let exported = viewer
                .export_result
                .as_ref()
                .expect("web_annotation export must land");
            assert_eq!(exported.format, "web_annotation");
            assert_eq!(
                exported.output_path,
                "/tmp/exports/t1-highlights.annotations.json"
            );
            assert!(
                viewer
                    .status
                    .as_deref()
                    .unwrap_or("")
                    .contains("/tmp/exports/t1-highlights.annotations.json")
            );
            viewer.export_transcript("tr-1".to_string(), "webvtt", None, cx);
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, cx| {
            let exported = viewer
                .export_result
                .as_ref()
                .expect("webvtt export must land");
            assert_eq!(exported.format, "webvtt");
            assert_eq!(exported.output_path, "/tmp/exports/t1.vtt");
            viewer.export_transcript("tr-1".to_string(), "otio", Some("edl-42".to_string()), cx);
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, cx| {
            let exported = viewer
                .export_result
                .as_ref()
                .expect("otio export must land");
            assert_eq!(exported.format, "otio");
            assert_eq!(exported.output_path, "/tmp/exports/t1-reel.otio");
            // Fourth call returns malformed payload -> Failed state, last-good preserved.
            viewer.export_transcript("tr-1".to_string(), "otio", None, cx);
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert!(matches!(
                viewer.export_request.state,
                ResourceState::Failed(_)
            ));
            let exported = viewer
                .export_result
                .as_ref()
                .expect("failed export must preserve last-good result");
            assert_eq!(exported.output_path, "/tmp/exports/t1-reel.otio");
        });

        let recorded = calls.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(recorded.len(), 4);
        assert_eq!(recorded[0].0, "educt_export");
        assert_eq!(recorded[0].1["format"], "web_annotation");
        assert_eq!(recorded[1].1["format"], "webvtt");
        assert_eq!(recorded[2].1["format"], "otio");
        assert_eq!(recorded[2].1["layer_id"], "edl-42");

        // Also pin `parse_reel_render_target` for the inspector Reel tab.
        let detail = serde_json::json!({
            "transcript_render": {
                "transcript_id": "tr-1",
                "source_transcript_id": "tr-1",
                "edl_layer_id": "edl-42",
                "created_at": "2026-11-20T00:00:00Z",
            }
        });
        assert_eq!(
            parse_reel_render_target(&detail),
            Some(("tr-1".to_string(), Some("edl-42".to_string())))
        );
        let detached = serde_json::json!({
            "transcript_render": {
                "transcript_id": null,
                "source_transcript_id": "tr-1",
                "edl_layer_id": null,
                "created_at": "2026-11-20T00:00:00Z",
            }
        });
        assert_eq!(parse_reel_render_target(&detached), None);
    }
}
