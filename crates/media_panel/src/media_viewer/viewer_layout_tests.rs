use super::*;
use crate::test_fixtures::scheduled_job;
use gpui::{DefiniteLength, TestAppContext, px, size};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test_data/viewer-layout.mp4");

/// Admit the committed fixture through the widget's locator-validation
/// boundary (`approve_gallery_media_path`) — the same authority the
/// viewer's ingest path uses for gallery-published media. The widget's
/// cfg(test) approved root covers only its OWN crate's `test_data`; this
/// crate's fixture lives outside both it and the artifacts dir, so
/// without explicit admission the shared widget refuses to resolve the
/// path and the video area never renders (the layout pins then fail).
fn admit_fixture() {
    hkask_media_widget::approve_gallery_media_path(std::path::Path::new(FIXTURE))
        .expect("the committed layout fixture must be admittable");
}

/// Layout ground truth at the VIEWER level — the full production chain
/// (tab bar → tab content → toolbar → player), not the widget in
/// isolation. The widget-level test passed while the app was broken,
/// so the break is in this chain. The property: the video area's
/// laid-out height tracks window height exactly.
#[gpui::test]
fn viewer_video_area_scales_with_window_size(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    admit_fixture();

    let viewer = cx.new(|_| MediaViewer::new());
    let output = serde_json::json!({
        "content": {
            "status": "fetched",
            "display_hint": format!(
                "```media\n{{\"kind\":\"video\",\"src\":\"{FIXTURE}\"}}\n```"
            )
        }
    })
    .to_string();
    viewer.update(cx, |viewer, cx| {
        viewer.ingest_tool_result(&Value::String(output), "video_fetch", cx)
    });
    let asset_count = viewer.update(cx, |viewer, cx| {
        let count = viewer.assets.len();
        let _ = cx;
        count
    });
    assert_eq!(asset_count, 1, "fixture asset must merge");

    // This pin is the PLAYER's chain: the video area must track window
    // height with the inspector pane closed — the open pane is a
    // legitimate height share (the fraction contract), not a squash.
    // The fixture asset is surfaced (unindexed), so the toggle
    // dispatches nothing.
    viewer.update(cx, |viewer, cx| viewer.toggle_inspector(cx));

    struct Host {
        viewer: Entity<MediaViewer>,
    }
    impl gpui::Render for Host {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            // The production embedding: a pane-sized flex child, as the
            // media panel renders the viewer.
            div().size_full().child(self.viewer.clone())
        }
    }

    let (_, cx) = cx.add_window_view(|_window, _cx| Host {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(800.), px(600.)));
    cx.run_until_parked();
    let short_bounds = cx
        .debug_bounds("media-video-area")
        .expect("video area laid out in the viewer chain");

    // THE FIT assertion — the one this bug class evaded: the whole
    // widget (video area + transport) must fit INSIDE the window. The
    // broken layout (h_full on the tab content, a flex-column main-axis
    // child) made the widget extend past the window bottom by the tab
    // bar's height while every scaling assertion passed.
    let widget_bounds = cx
        .debug_bounds("media-widget")
        .expect("widget laid out with debug bounds");
    assert!(
        widget_bounds.bottom() <= px(600.) + px(1.),
        "the player must fit inside a 600px window: widget bottom {:?}",
        widget_bounds.bottom()
    );

    cx.simulate_resize(size(px(800.), px(900.)));
    cx.run_until_parked();
    let tall_bounds = cx
        .debug_bounds("media-video-area")
        .expect("video area laid out after resize");

    // 300px window delta minus sub-pixel rounding — the property is
    // "tracks the window", not exact pixel equality.
    let height_delta = tall_bounds.size.height - short_bounds.size.height;
    assert!(
        height_delta > px(295.) && height_delta < px(305.),
        "video area height must track window height through the full viewer chain: \
             {:?} at 600px vs {:?} at 900px (delta {height_delta:?})",
        short_bounds.size.height,
        tall_bounds.size.height
    );

    let tall_widget_bounds = cx
        .debug_bounds("media-widget")
        .expect("widget laid out after resize");
    assert!(
        tall_widget_bounds.bottom() <= px(900.) + px(1.),
        "the player must fit inside a 900px window: widget bottom {:?}",
        tall_widget_bounds.bottom()
    );
}

/// The production panel embedding for the narrow-pane probes: the
/// viewer pane on top (`flex_1`, `min_h_0`, `min_w_0`), the 1px split
/// divider, and the director below at a fractional height — exactly as
/// `media_panel.rs` renders them (minus the drag wiring, which needs a
/// real pointer drag the harness doesn't synthesize).
struct NarrowPaneHost {
    viewer: Entity<MediaViewer>,
}

impl gpui::Render for NarrowPaneHost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(self.viewer.clone()),
            )
            .child(div().w_full().flex_shrink_0().h(px(1.)))
            .child(
                div()
                    .w_full()
                    .h(DefiniteLength::Fraction(0.5))
                    .min_h_0()
                    .flex()
                    .flex_col(),
            )
    }
}

/// The theme/settings globals the viewer's render reads — shared by
/// this module's layout tests.
fn init_layout_test_globals(cx: &mut TestAppContext) {
    cx.update(|cx| {
        if !cx.has_global::<settings::SettingsStore>() {
            settings::init(cx);
        }
        if !cx.has_global::<theme::GlobalTheme>() {
            theme_settings::init(theme::LoadThemes::JustBase, cx);
        }
    });
}

/// THE transcript-pane wrap contract (operator report 2026-10-26:
/// paragraph text went off the pane edge). A single long paragraph's
/// max-content width is far wider than the dock; the block must wrap
/// at the definite pane width — the former flex-row container broke
/// the definite-width propagation and laid the text as one line.
#[gpui::test]
fn transcript_pane_wraps_within_narrow_dock(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(TranscriptPaneState {
            transcript_id: "t-long".into(),
            media_path: "/generated/long.mp4".into(),
            gallery_asset_id: None,
            alignment: "original".into(),
            alignment_note: None,
            words: Some(
                (0..200)
                    .map(|index| PaneWord {
                        word: format!("unwrappable-word-{index}"),
                        start_ms: index as u64 * 100,
                        end_ms: index as u64 * 100 + 90,
                    })
                    .collect(),
            ),
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
        // The open path's post-state (T3): open_transcript selects the
        // Workspace and yields the inspector's share to the reading
        // surface.
        viewer.active_tab = ViewerTab::Workspace;
        viewer.inspector_open = false;
        cx.notify();
    });

    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(600.)));
    cx.run_until_parked();

    // The sub-surface's chrome (purpose line + header row, T3) sits
    // above the list, so the 140px pinned player row and the header
    // rows fill the first frame — scroll the paragraph into view
    // before measuring. The wrap contract is width-containment at the
    // definite pane width, verifiable at any scroll offset.
    viewer.update(cx, |viewer, cx| {
        viewer.transcript_list.scroll_to(ListOffset {
            item_ix: WORKSPACE_HEADER_ITEMS,
            offset_in_item: px(0.),
        });
        cx.notify();
    });
    cx.run_until_parked();
    let bounds = cx
        .debug_bounds("media-transcript-paragraph")
        .expect("paragraph block laid out");
    assert!(
        bounds.right() <= px(480.),
        "the paragraph wraps within the dock: {bounds:?}"
    );
    assert!(
        bounds.size.height > px(20.),
        "the wrapped paragraph is taller than one line: {bounds:?}"
    );
}

/// The label editor row (S4-3) fits the narrow dock: its fixed-width
/// input and affordances lay out within the pane, never past its edge
/// — the same wrap contract the paragraph blocks carry.
#[gpui::test]
fn label_editor_fits_narrow_dock(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(TranscriptPaneState {
            transcript_id: "t-edit".into(),
            media_path: "/generated/edit.wav".into(),
            gallery_asset_id: None,
            alignment: "original".into(),
            alignment_note: None,
            words: None,
            working_text: "words in the workspace".into(),
            paragraph_breaks: None,
            speaker_spans: None,
            review: None,
            layers: Vec::new(),
            highlights: Vec::new(),
            strikes: Vec::new(),
            correction_edits: Vec::new(),
            source_words: Vec::new(),
        });
        // The open path's post-state (T3).
        viewer.active_tab = ViewerTab::Workspace;
        viewer.inspector_open = false;
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::Create {
                start_word: 0,
                end_word: 1,
            },
            buffer: LabelBuffer::from_str("climax"),
        });
        cx.notify();
    });

    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(600.)));
    cx.run_until_parked();

    let bounds = cx
        .debug_bounds("media-transcript-label-editor")
        .expect("label editor row laid out");
    assert!(
        bounds.right() <= px(480.),
        "the label editor fits the dock: {bounds:?}"
    );
}

/// THE horizontal-fit invariant — the one the scaling test above does
/// not encode. No part of the viewer — media content, header, toolbar,
/// inspector pane, or tab bar — may exceed the pane's available width
/// at any pane width, with the video's natural width (640px) larger
/// than the pane. Historically the pane div lacked `min_w_0`, so the
/// viewer's min-content width (untruncated header src + seven inline
/// toolbar buttons ≈ 685px) inflated the pane and the video rendered
/// clipped at the dock edge, unviewable. This test pins the fix in the
/// current production embedding (top/bottom split): the pane is a
/// flex-column child and still carries `min_w_0`. The final iteration
/// resizes back — the no-thrash probe.
#[gpui::test]
fn viewer_content_fits_narrow_pane(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    admit_fixture();

    let viewer = cx.new(|_| MediaViewer::new());
    let output = serde_json::json!({
        "content": {
            "status": "fetched",
            "display_hint": format!(
                "```media\n{{\"kind\":\"video\",\"src\":\"{FIXTURE}\"}}\n```"
            )
        }
    })
    .to_string();
    viewer.update(cx, |viewer, cx| {
        viewer.ingest_tool_result(&Value::String(output), "video_fetch", cx)
    });

    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });

    // (dock width, pane available) — the split is horizontal
    // (top/bottom panes), so the viewer spans the dock's full width;
    // the director no longer takes a horizontal share. 480px is the
    // adversarial dock: far narrower than the fixture's natural 640px
    // video.
    for (dock_width, pane_available) in [
        (px(700.), px(700.)),
        (px(480.), px(480.)),
        (px(700.), px(700.)),
    ] {
        cx.simulate_resize(size(dock_width, px(600.)));
        cx.run_until_parked();
        assert_viewer_fits_narrow_pane(cx, dock_width, pane_available);
    }
}

/// THE close-transcript overlap pin (S1, operator report 2026-11-04):
/// closing the transcript sub-surface must never leave the shared
/// player painting past the workspace's content area — the artifact
/// the operator saw as transport buttons superimposed over the Steer
/// director below. The transcript's 140px player row clips, which
/// masked the player's intrinsic usable-height floor; closing the
/// transcript was the first unclipped render of the same shared
/// widget. The invariant: the player's laid-out bounds stay inside
/// the content container at every pane height — the floor belongs to
/// the content-flow embedder (`block_renderer`), and this container
/// clips as the backstop.
#[gpui::test]
fn closing_transcript_leaves_widget_inside_pane(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    admit_fixture();

    let viewer = cx.new(|_| MediaViewer::new());
    let output = serde_json::json!({
        "content": {
            "status": "fetched",
            "display_hint": format!(
                "```media\n{{\"kind\":\"video\",\"src\":\"{FIXTURE}\"}}\n```"
            )
        }
    })
    .to_string();
    viewer.update(cx, |viewer, cx| {
        viewer.ingest_tool_result(&Value::String(output), "video_fetch", cx)
    });

    fn pinned_pane_state() -> TranscriptPaneState {
        TranscriptPaneState {
            transcript_id: "t-close-pin".into(),
            media_path: FIXTURE.into(),
            gallery_asset_id: None,
            alignment: "original".into(),
            alignment_note: None,
            words: Some(vec![PaneWord {
                word: "pin".into(),
                start_ms: 0,
                end_ms: 100,
            }]),
            working_text: "pin".into(),
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

    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });

    // A short pane (the host halves the window: the viewer area is
    // ~159px at 320px, far below the player's former 240px floor) and
    // a tall one — the invariant holds at every height, exercised over
    // the open→close transition each time.
    for window_height in [px(320.), px(900.)] {
        cx.simulate_resize(size(px(480.), window_height));
        cx.run_until_parked();

        // Open the sub-surface — the player rides the clipped 140px
        // header row — then close: the transition the operator
        // reported.
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(pinned_pane_state());
            viewer.active_tab = ViewerTab::Workspace;
            cx.notify();
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, cx| viewer.close_transcript_workspace(cx));
        cx.run_until_parked();

        let content = cx
            .debug_bounds("media-viewer-content")
            .expect("the workspace content container lays out after close");
        let widget = cx
            .debug_bounds("media-widget")
            .expect("the shared player lays out after close");
        assert!(
            widget.top() >= content.top() - px(1.),
            "the player starts inside the content area (window height {window_height:?}): \
                 widget {widget:?} vs content {content:?}"
        );
        assert!(
            widget.bottom() <= content.bottom() + px(1.),
            "the player must not paint past the content area after closing the transcript \
                 (window height {window_height:?}): widget {widget:?} vs content {content:?}"
        );
    }
}

/// dcterms:identifier: `render_transcript_workspace` — the pinned
/// dcterms:identifier: `render_transcript_workspace` — the pinned
/// interaction band
/// expect: The player, the surface chips, and the search input pin
/// ABOVE the scrolling transcript list: a deep follow-along reveal
/// (the operator's far-forward seek) scrolls the marked block into
/// view WITHOUT displacing the player — the synced-recording contract
/// keeps media and text addressing each other, both visible. The
/// shipped construction rendered the player as list item 0 (operator
/// report 2026-11-07: the text scroll "pushes or blocks the video from
/// the view"), so the reveal's own scroll virtualized the primary
/// media object out of the pane. The band caps at 55% of the
/// workspace (the player row is its flexible member), so the reading
/// area keeps the majority at every pane height — the composed
/// height-budget answer to the one-list design's rejected concern.
/// [P1] Motivating: the synced recording — watch while reading along.
/// pre: a word-timed pane state with 30 blocks (far taller than the
/// pane); the follow reveal armed by a late current word.
/// post: the media widget's bounds remain laid out INSIDE the band
/// after the reveal — a virtualized-away row lays out no bounds,
/// which is the shipped construction's red.
#[gpui::test]
fn follow_scroll_keeps_the_pinned_player_in_view(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    admit_fixture();

    let viewer = cx.new(|_| MediaViewer::new());
    let output = serde_json::json!({
        "content": {
            "status": "fetched",
            "display_hint": format!(
                "```media\n{{\"kind\":\"video\",\"src\":\"{FIXTURE}\"}}\n```"
            )
        }
    })
    .to_string();
    viewer.update(cx, |viewer, cx| {
        viewer.ingest_tool_result(&Value::String(output), "video_fetch", cx)
    });

    // 30 blocks over 60 words: far taller than the test pane, so the
    // reveal must scroll.
    let deep_state = TranscriptPaneState {
        transcript_id: "t-follow-pin".into(),
        media_path: FIXTURE.into(),
        gallery_asset_id: None,
        alignment: "original".into(),
        alignment_note: None,
        words: Some(
            (0..60)
                .map(|index| PaneWord {
                    word: format!("follow-word-{index}"),
                    start_ms: index * 100,
                    end_ms: index * 100 + 100,
                })
                .collect(),
        ),
        working_text: "deep transcript".into(),
        paragraph_breaks: Some((0..30).map(|index| index * 2 + 1).collect()),
        speaker_spans: None,
        review: None,
        layers: Vec::new(),
        highlights: Vec::new(),
        strikes: Vec::new(),
        correction_edits: Vec::new(),
        source_words: Vec::new(),
    };

    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(600.)));
    cx.run_until_parked();

    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(deep_state);
        viewer.active_tab = ViewerTab::Workspace;
        // The mark: word 50 — block 25, far past the first viewport.
        viewer.current_word = Some(50);
        viewer.follow_viewport = true;
        cx.notify();
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, cx| {
        viewer.follow_marked_block_into_view();
        cx.notify();
    });
    cx.run_until_parked();

    let band = cx
        .debug_bounds("media-transcript-band")
        .expect("the pinned interaction band lays out under the sub-surface");
    let widget = cx
        .debug_bounds("media-widget")
        .expect("the pinned player stays laid out after the deep reveal");
    assert!(
        widget.top() >= band.top() - px(1.),
        "the pinned player starts inside the band after the reveal: \
             widget {widget:?} vs band {band:?}"
    );
    assert!(
        widget.bottom() <= band.bottom() + px(1.),
        "the pinned player must not leave the band after the reveal: \
             widget {widget:?} vs band {band:?}"
    );
}

/// The selection popover (S2) opens for BOTH selection gestures on the
/// completed range — press-and-release drag and the two-click fallback —
/// pinned above the list like the label editor (never virtualized
/// away), carrying the plan's S2 acceptance 1-2. A fresh completion
/// replaces the tray's range; the completed range owns the pane (no
/// anchor lingers).
#[gpui::test]
fn selection_popover_opens_for_drag_and_two_click_ranges(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(TranscriptPaneState {
            transcript_id: "t-popover-gestures".into(),
            media_path: "/generated/popover.wav".into(),
            gallery_asset_id: None,
            alignment: "original".into(),
            alignment_note: None,
            words: Some(
                (0..30)
                    .map(|index| PaneWord {
                        word: format!("popover-word-{index}"),
                        start_ms: index as u64 * 100,
                        end_ms: index as u64 * 100 + 90,
                    })
                    .collect(),
            ),
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
        viewer.active_tab = ViewerTab::Workspace;
        viewer.inspector_open = false;
        cx.notify();
    });
    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(600.)));
    cx.run_until_parked();

    use gpui::VisualContext as _;
    // Drag completion (S2 core → tray): press on word 2, hover word 5,
    // release — the ordered union range, never inverted.
    cx.update_window_entity(&viewer, |viewer, window, cx| {
        viewer.word_drag_anchor = Some(2);
        viewer.drag_last_word = Some(5);
        viewer.word_drag_released(window, cx);
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert_eq!(
            viewer.selection_popover,
            Some((2, 5)),
            "the drag completion opens the tray on the ordered union range"
        );
    });
    assert!(
        cx.debug_bounds("media-transcript-selection-popover")
            .is_some(),
        "the selection popover renders pinned above the list"
    );

    // Two-click completion (the fallback gesture): anchor then last
    // word in select mode — the same range semantics, the same tray.
    cx.update_window_entity(&viewer, |viewer, window, cx| {
        viewer.selection_mode = true;
        viewer.word_clicked(10, 1_000, window, cx);
        viewer.word_clicked(14, 1_400, window, cx);
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert_eq!(
            viewer.selection_popover,
            Some((10, 14)),
            "the two-click completion opens the tray on the same union range"
        );
        assert!(
            viewer.selection_anchor.is_none(),
            "an open tray owns the completed range; no anchor lingers"
        );
    });
}

/// The tray's keyboard shortcuts (S2): Enter opens the label editor over
/// the tray's range, Delete cuts through the strike path, Escape
/// dismisses — the plan's S2 objective's Enter/Delete keys. The tray
/// holds focus while open, so the keys bind on it and never on the
/// label input (Delete there is text editing).
#[gpui::test]
fn selection_popover_keys_highlight_cut_and_dismiss(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| MediaViewer::new());
    let blank_modifiers = || gpui::Modifiers {
        control: false,
        alt: false,
        shift: false,
        platform: false,
        function: false,
    };
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(TranscriptPaneState {
            transcript_id: "t-popover-keys".into(),
            media_path: "/generated/popover-keys.wav".into(),
            gallery_asset_id: None,
            alignment: "original".into(),
            alignment_note: None,
            words: Some(
                (0..30)
                    .map(|index| PaneWord {
                        word: format!("keys-word-{index}"),
                        start_ms: index as u64 * 100,
                        end_ms: index as u64 * 100 + 90,
                    })
                    .collect(),
            ),
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
        viewer.active_tab = ViewerTab::Workspace;
        viewer.inspector_open = false;
        cx.notify();
    });
    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(600.)));
    cx.run_until_parked();

    use gpui::VisualContext as _;
    // Enter routes to Highlight: the label editor opens over the tray's
    // range and the tray closes (the editor supersedes it).
    cx.update_window_entity(&viewer, |viewer, window, cx| {
        viewer.selection_popover = Some((3, 9));
        viewer.handle_selection_popover_key(
            &Keystroke {
                modifiers: blank_modifiers(),
                key: "enter".into(),
                key_char: None,
            },
            window,
            cx,
        );
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert!(
            matches!(
                &viewer.highlight_edit,
                Some(HighlightEdit {
                    kind: HighlightEditKind::Create {
                        start_word: 3,
                        end_word: 9
                    },
                    ..
                })
            ),
            "Enter opens the label editor over the tray's range"
        );
        assert!(
            viewer.selection_popover.is_none(),
            "the editor supersedes the tray"
        );
    });

    // Delete routes to the strike path — the admission refusal surfaces
    // (an in-flight edit), proving the cut arm was taken.
    cx.update_window_entity(&viewer, |viewer, window, cx| {
        viewer.highlight_edit = None;
        viewer.selection_popover = Some((3, 9));
        viewer.edit_request.state = ResourceState::Loading;
        viewer.handle_selection_popover_key(
            &Keystroke {
                modifiers: blank_modifiers(),
                key: "delete".into(),
                key_char: None,
            },
            window,
            cx,
        );
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert!(
            viewer.selection_popover.is_none(),
            "the cut consumes the tray"
        );
        assert!(
            viewer
                .status
                .as_deref()
                .is_some_and(|status| status.contains("Another media edit")),
            "Delete reached the strike path and surfaced the admission refusal: {:?}",
            viewer.status
        );
    });

    // Escape dismisses without acting.
    cx.update_window_entity(&viewer, |viewer, window, cx| {
        viewer.edit_request.state = ResourceState::Idle;
        viewer.selection_popover = Some((3, 9));
        viewer.handle_selection_popover_key(
            &Keystroke {
                modifiers: blank_modifiers(),
                key: "escape".into(),
                key_char: None,
            },
            window,
            cx,
        );
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert!(
            viewer.selection_popover.is_none(),
            "Escape dismisses the tray"
        );
        assert!(
            viewer
                .status
                .as_deref()
                .is_some_and(|status| status.contains("dismissed")),
            "the dismissal surfaces: {:?}",
            viewer.status
        );
    });
}

/// dcterms:identifier: `MediaViewer::word_drag_released`
/// expect: A completed drag on a re_anchored view opens the CORRECTION
/// tray (S5, D5 v2) — Enter opens the correction editor, Delete stays
/// inert (cut needs an index-identical view), the index-identical-only
/// actions refuse with the honest reason even when reached directly,
/// and Escape dismisses. [P1] Motivating: the Dunning recording is
/// re_anchored — the in-panel correction path needs a selection
/// surface there, and working indices must never anchor source-word
/// tools (the 2026-10-25 overshoot trap).
#[gpui::test]
fn reanchored_selection_tray_corrects_only(cx: &mut gpui::TestAppContext) {
    init_layout_test_globals(cx);
    // No dispatch in this pin's flow: the tray opens, Enter opens the
    // editor, Delete is inert, and the index-identical-only actions
    // refuse at the view gate — before the edit admission and its
    // invoker read — so it needs neither the lock nor a scripted
    // invoker.
    let blank_modifiers = || gpui::Modifiers {
        control: false,
        alt: false,
        shift: false,
        platform: false,
        function: false,
    };
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(TranscriptPaneState {
            transcript_id: "t-reanchored-tray".into(),
            media_path: "/m.wav".into(),
            gallery_asset_id: None,
            alignment: "re_anchored".into(),
            alignment_note: None,
            words: Some(
                ["alpha", "bravo", "charlie"]
                    .into_iter()
                    .enumerate()
                    .map(|(index, word)| PaneWord {
                        word: word.into(),
                        start_ms: index as u64 * 100,
                        end_ms: index as u64 * 100 + 90,
                    })
                    .collect(),
            ),
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
        viewer.active_tab = ViewerTab::Workspace;
        viewer.inspector_open = false;
        cx.notify();
    });
    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(600.)));
    cx.run_until_parked();

    use gpui::VisualContext as _;
    // A completed drag opens the correction tray on a re_anchored view
    // (the S2 gate refused selection here).
    cx.update_window_entity(&viewer, |viewer, window, cx| {
        viewer.word_drag_anchor = Some(0);
        viewer.drag_last_word = Some(2);
        viewer.word_drag_released(window, cx);
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert_eq!(viewer.selection_popover, Some((0, 2)), "the tray opens");
        assert!(
            viewer
                .status
                .as_deref()
                .is_some_and(|status| status.contains("Suggest correction")),
            "the correction tray's hint: {:?}",
            viewer.status
        );
    });

    // Enter opens the correction editor over the tray's range.
    cx.update_window_entity(&viewer, |viewer, window, cx| {
        viewer.handle_selection_popover_key(
            &Keystroke {
                modifiers: blank_modifiers(),
                key: "enter".into(),
                key_char: None,
            },
            window,
            cx,
        );
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert!(
            matches!(
                &viewer.highlight_edit,
                Some(HighlightEdit {
                    kind: HighlightEditKind::Correction {
                        start_word: 0,
                        end_word: 2
                    },
                    ..
                })
            ),
            "Enter opens the correction editor: {:?}",
            viewer.highlight_edit
        );
        assert!(viewer.selection_popover.is_none());
        viewer.highlight_edit = None;
        viewer.selection_popover = Some((0, 2));
    });

    // Delete stays inert on the correction tray — cut needs an
    // index-identical view.
    cx.update_window_entity(&viewer, |viewer, window, cx| {
        viewer.handle_selection_popover_key(
            &Keystroke {
                modifiers: blank_modifiers(),
                key: "delete".into(),
                key_char: None,
            },
            window,
            cx,
        );
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert_eq!(
            viewer.selection_popover,
            Some((0, 2)),
            "Delete is inert on the correction tray"
        );
        assert_eq!(viewer.edit_request.state, ResourceState::Idle);
    });

    // The index-identical-only actions refuse with the honest reason
    // even when reached directly — a stale tray or a mis-routed key
    // must never anchor working indices to source-word tools.
    cx.update_window_entity(&viewer, |viewer, window, cx| {
        viewer.popover_highlight(window, cx);
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert!(
            viewer
                .status
                .as_deref()
                .is_some_and(|status| status.contains("index-identical")),
            "the highlight refusal names the view gate: {:?}",
            viewer.status
        );
        assert!(viewer.highlight_edit.is_none());
        viewer.selection_popover = Some((0, 2));
    });
    cx.update_window_entity(&viewer, |viewer, _window, cx| {
        viewer.popover_cut(cx);
        viewer.selection_popover = Some((0, 2));
        viewer.popover_toggle_reel_include(cx);
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert!(
            viewer
                .status
                .as_deref()
                .is_some_and(|status| status.contains("index-identical")),
            "the cut/include refusals name the view gate: {:?}",
            viewer.status
        );
        assert_eq!(viewer.edit_request.state, ResourceState::Idle);
    });

    // Escape dismisses.
    cx.update_window_entity(&viewer, |viewer, window, cx| {
        viewer.handle_selection_popover_key(
            &Keystroke {
                modifiers: blank_modifiers(),
                key: "escape".into(),
                key_char: None,
            },
            window,
            cx,
        );
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert!(viewer.selection_popover.is_none(), "Escape dismisses");
    });
}

/// A words pane state for the search tests: `total` words in 5-word
/// blocks, "dunning" at each index in `matches_at`.
fn search_pane_state(total: usize, dunning_at: &[usize]) -> TranscriptPaneState {
    TranscriptPaneState {
        transcript_id: "t-search".into(),
        media_path: "/generated/search.wav".into(),
        gallery_asset_id: None,
        alignment: "original".into(),
        alignment_note: None,
        words: Some(
            (0..total)
                .map(|index| PaneWord {
                    word: if dunning_at.contains(&index) {
                        "dunning".into()
                    } else {
                        format!("w{index}")
                    },
                    start_ms: index as u64 * 100,
                    end_ms: index as u64 * 100 + 90,
                })
                .collect(),
        ),
        working_text: String::new(),
        paragraph_breaks: Some((1..(total / 5)).map(|block| block * 5 - 1).collect()),
        speaker_spans: None,
        review: None,
        layers: Vec::new(),
        highlights: Vec::new(),
        strikes: Vec::new(),
        correction_edits: Vec::new(),
        source_words: Vec::new(),
    }
}

/// The blank-modifier keystroke helper for the search tests.
fn search_key(key: &str, shift: bool) -> Keystroke {
    Keystroke {
        modifiers: gpui::Modifiers {
            control: false,
            alt: false,
            shift,
            platform: false,
            function: false,
        },
        key: key.into(),
        key_char: None,
    }
}

/// Whether the block at `block_ix` is fully inside the list's viewport
/// — the same visibility read `follow_marked_block_into_view` performs.
fn search_block_in_view(
    viewer: &gpui::Entity<MediaViewer>,
    cx: &mut gpui::VisualTestContext,
    block_ix: usize,
) -> bool {
    viewer.update(cx, |viewer, _| {
        let item_ix = WORKSPACE_HEADER_ITEMS + block_ix;
        viewer
            .transcript_list
            .bounds_for_item(item_ix)
            .is_some_and(|item| {
                let viewport = viewer.transcript_list.viewport_bounds();
                item.top() >= viewport.top() && item.bottom() <= viewport.bottom()
            })
    })
}

/// The in-transcript search (S3): typing into the header input finds
/// every match of the query over the loaded transcript, the stabilized
/// query lands the first match in view, Enter cycles forward and
/// Shift-Enter back (wrapping), each cycled match's block scrolls into
/// view, and a query with no results shows the honest no-matches state
/// — the plan's S3 acceptance 1 and 2.
#[gpui::test]
fn transcript_search_finds_cycles_and_lands_the_current_match(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| MediaViewer::new());
    // 120 words in 5-word blocks, "dunning" at 25, 85, 115 — three
    // matches, the later two far below the first fold.
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(search_pane_state(120, &[25, 85, 115]));
        viewer.active_tab = ViewerTab::Workspace;
        viewer.inspector_open = false;
        cx.notify();
    });
    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    // A taller window than the other layout tests: the search row sits
    // BELOW the 140px player row, so the first fold must cover the whole
    // header for the input to paint — and its focus path to register
    // (focus events dispatch from the painted frame's focus path).
    cx.simulate_resize(size(px(480.), px(900.)));
    cx.run_until_parked();

    // The search input renders in the header from the first paint.
    assert!(
        cx.debug_bounds("media-transcript-search-input").is_some(),
        "the search input renders in the header"
    );

    // Type the query; the debounced rescan runs once the burst settles.
    // The paint before the rescan carries the typed query and the
    // honest interim status ("No matches" — the rescan has not run).
    viewer.update(cx, |viewer, cx| {
        for character in "dunning".chars() {
            viewer.handle_transcript_search_key(&search_key(&character.to_string(), false), cx);
        }
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("media-transcript-search-status").is_some(),
        "the match count renders beside the input"
    );
    cx.executor().advance_clock(TRANSCRIPT_SEARCH_DEBOUNCE);
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert_eq!(
            viewer.transcript_search.matches,
            vec![(25, 25), (85, 85), (115, 115)],
            "typing finds all the matches"
        );
        assert_eq!(viewer.transcript_search.current, 0);
        assert_eq!(
            transcript_search_status(&viewer.transcript_search, true).as_deref(),
            Some("1/3")
        );
    });
    // The rescan's auto-land scrolled the first match's block into view
    // — the header rows (input included) are above the viewport now, so
    // the input's rendering is pinned by the PRE-rescan bounds asserts
    // above; here the reveal is the observable.
    assert!(
        search_block_in_view(&viewer, cx, 5),
        "the stabilized query lands the first match (word 25, block 5) in view"
    );

    // Enter cycles forward (wrapping) and lands each cycled match in
    // view; Shift-Enter cycles back.
    for (expected_current, block) in [(1usize, 17usize), (2, 23), (0, 5)] {
        viewer.update(cx, |viewer, cx| {
            viewer.handle_transcript_search_key(&search_key("enter", false), cx);
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert_eq!(
                viewer.transcript_search.current, expected_current,
                "Enter cycles and wraps"
            );
        });
        assert!(
            search_block_in_view(&viewer, cx, block),
            "the cycled match's block scrolls into view"
        );
    }
    viewer.update(cx, |viewer, cx| {
        viewer.handle_transcript_search_key(&search_key("enter", true), cx);
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert_eq!(
            viewer.transcript_search.current, 2,
            "Shift-Enter cycles back"
        );
    });

    // A query with no results shows the honest no-matches state.
    viewer.update(cx, |viewer, cx| {
        viewer.handle_transcript_search_key(&search_key("escape", false), cx);
        for character in "absent".chars() {
            viewer.handle_transcript_search_key(&search_key(&character.to_string(), false), cx);
        }
    });
    cx.executor().advance_clock(TRANSCRIPT_SEARCH_DEBOUNCE);
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert!(viewer.transcript_search.matches.is_empty());
        assert_eq!(
            transcript_search_status(&viewer.transcript_search, true).as_deref(),
            Some("No matches"),
            "zero matches surface, never silence"
        );
    });
}

/// The search rescan is debounced (S3): a burst of keystrokes rescans
/// once, 150 ms after the LAST edit — never per keystroke, never before
/// the burst settles.
#[gpui::test]
fn transcript_search_rescan_debounces_the_burst(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(search_pane_state(30, &[5]));
        viewer.active_tab = ViewerTab::Workspace;
        viewer.inspector_open = false;
        cx.notify();
    });
    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(600.)));
    cx.run_until_parked();

    viewer.update(cx, |viewer, cx| {
        for character in "dun".chars() {
            viewer.handle_transcript_search_key(&search_key(&character.to_string(), false), cx);
        }
    });
    // 149 ms after the last keystroke: the rescan has not run.
    cx.executor()
        .advance_clock(TRANSCRIPT_SEARCH_DEBOUNCE - Duration::from_millis(1));
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert!(
            viewer.transcript_search.matches.is_empty(),
            "the rescan waits out the burst"
        );
    });
    // One millisecond later it fires, once, on the final query.
    cx.executor().advance_clock(Duration::from_millis(1));
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert_eq!(
            viewer.transcript_search.matches,
            vec![(5, 5)],
            "the settled burst rescans once, on the final query"
        );
        assert!(
            viewer.transcript_search_task.is_none(),
            "the rescan task completes"
        );
    });
}

/// Focusing the search input suspends viewport tracking (the plan's S3
/// design: typing and the auto-landed matches must not fight the
/// follow-along); blur restores tracking only when the search took it —
/// a manually suspended follow stays suspended.
#[gpui::test]
fn transcript_search_focus_suspends_follow_and_blur_restores(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(search_pane_state(30, &[5]));
        viewer.active_tab = ViewerTab::Workspace;
        viewer.inspector_open = false;
        cx.notify();
    });
    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    // The search row sits below the 140px player row — the fold must
    // cover the header for the input to paint and register its focus
    // path (focus events dispatch from the painted frame's focus path,
    // so an off-screen input never gains focus events).
    cx.simulate_resize(size(px(480.), px(900.)));
    cx.run_until_parked();

    use gpui::VisualContext as _;
    // The test platform's windows start INACTIVE: the focus-change event
    // carries empty focus paths for an inactive window
    // (`current_window_active` gates them), so the listeners would never
    // fire. Activate first — the production window is active the moment
    // the operator clicks anything.
    cx.update_window_entity(&viewer, |_, window, _| window.activate_window());
    cx.run_until_parked();
    // The initial paint registered the focus listeners (the workspace
    // render path); the drive only focuses. The dispatch needs an
    // active window (test windows start INACTIVE — the focus event's
    // paths stay empty until activate_window) and a following draw, so
    // the drive also notifies.
    cx.update_window_entity(&viewer, |viewer, window, cx| {
        let focus = viewer
            .transcript_search_focus
            .clone()
            .expect("rendering the header created the focus handle");
        window.focus(&focus, cx);
        cx.notify();
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert!(!viewer.follow_viewport, "focus suspends tracking");
        assert!(
            viewer.transcript_search_suspended_follow,
            "the search holds the follow custody it took"
        );
    });

    // Blur (focus the label input): custody restores the tracking.
    cx.update_window_entity(&viewer, |viewer, window, cx| {
        let focus = viewer
            .label_input_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        window.focus(&focus, cx);
        cx.notify();
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert!(viewer.follow_viewport, "blur restores the follow it took");
        assert!(!viewer.transcript_search_suspended_follow);
    });

    // A manually suspended follow is not the search's to restore.
    viewer.update(cx, |viewer, _| {
        viewer.follow_viewport = false;
    });
    cx.update_window_entity(&viewer, |viewer, window, cx| {
        let focus = viewer
            .transcript_search_focus
            .clone()
            .expect("the search focus handle exists");
        window.focus(&focus, cx);
        cx.notify();
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert!(
            !viewer.transcript_search_suspended_follow,
            "the search never takes custody of an already-suspended follow"
        );
    });
    cx.update_window_entity(&viewer, |viewer, window, cx| {
        let focus = viewer
            .label_input_focus
            .clone()
            .expect("the label focus handle exists");
        window.focus(&focus, cx);
        cx.notify();
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _| {
        assert!(
            !viewer.follow_viewport,
            "a manually suspended follow stays suspended after blur"
        );
    });
}

/// A failed player must not turn the transcript workspace into a dead
/// end: Back and range selection remain reachable while the error is
/// visible. The missing-file fixture fails synchronously and needs no
/// codec, device, network, or operator data.
#[gpui::test]
fn transcript_media_failure_keeps_navigation_and_curation_reachable(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| MediaViewer::new());
    let media_path = "/definitely-missing-transcript-audio.mp3";
    let widget = cx.new(|cx| {
        hkask_media_widget::MediaWidget::new(
            hkask_media_widget::MediaRef::new(
                media_path.into(),
                hkask_media_widget::MediaKind::Audio,
            ),
            cx,
        )
    });
    widget.update(cx, |widget, cx| widget.load(cx));
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(TranscriptPaneState {
            transcript_id: "t-media-failure".into(),
            media_path: media_path.into(),
            gallery_asset_id: None,
            alignment: "original".into(),
            alignment_note: None,
            words: Some(vec![PaneWord {
                word: "recover".into(),
                start_ms: 0,
                end_ms: 100,
            }]),
            working_text: "recover".into(),
            paragraph_breaks: None,
            speaker_spans: None,
            review: None,
            layers: Vec::new(),
            highlights: Vec::new(),
            strikes: Vec::new(),
            correction_edits: Vec::new(),
            source_words: Vec::new(),
        });
        // The open path's post-state (T3).
        viewer.active_tab = ViewerTab::Workspace;
        viewer.inspector_open = false;
        viewer.media_widget = Some(widget);
        viewer.media_widget_body = Some(transcript_media_body(media_path));
        cx.notify();
    });

    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    // 480x600, not the old 480x320: the composed two-pane fix (2026-11-07)
    // pins the interaction band (player + chips + search) above the list,
    // capped at 55% of the workspace so the reading area keeps the
    // majority — at the old 320px-window geometry the band leaves the
    // list a sub-padding sliver and the reachability contract is
    // exercised against the band instead (the pinned close row and
    // select chip are now reachable WITHOUT scrolling — strictly
    // stronger than the old scroll-to-reach). The harness halves the
    // window, so the viewer area here is ~300px.
    cx.simulate_resize(size(px(480.), px(600.)));
    cx.run_until_parked();

    // The close affordance rides in the sub-surface's header row above
    // the list — always visible, never virtualized away, reachable
    // without scrolling even on the tightest pane (the harness halves
    // the window, so the viewer area here is ~300px).
    let close = cx
        .debug_bounds("media-transcript-close")
        .expect("Close remains laid out during media failure");
    assert!(
        close.bottom() <= px(320.),
        "Close remains reachable during media failure: {close:?}"
    );
    // The select-mode chip is PINNED in the interaction band (the
    // composed two-pane fix): it is never a list item, so it must
    // render WITHOUT any scrolling — the strict form of the old
    // scroll-to-reach contract.
    let select_mode = cx
        .debug_bounds("media-transcript-select-mode")
        .expect("the pinned select chip renders without scrolling during media failure");
    assert!(
        select_mode.bottom() <= px(320.),
        "the pinned select chip remains reachable during media failure: {select_mode:?}"
    );
    // The paragraph (block 0) remains a list item under the band —
    // scrolling renders it, no dead end.
    let selector = "media-transcript-paragraph";
    viewer.update(cx, |viewer, cx| {
        viewer.transcript_list.scroll_to(ListOffset {
            item_ix: WORKSPACE_HEADER_ITEMS,
            offset_in_item: px(0.),
        });
        cx.notify();
    });
    cx.run_until_parked();
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} renders when scrolled to during media failure"));
    assert!(
        bounds.bottom() <= px(320.),
        "{selector} remains reachable during media failure: {bounds:?}"
    );
}

/// The follow-along mark must stay VISIBLE during playback (S3d, the
/// repair plan's R1 acceptance): with the mark on a late block and
/// viewport tracking on, the marked block is scrolled into the pane.
/// A virtualized off-screen block renders nothing at all, so its
/// bounds being present AND inside the pane is visibility proof —
/// before the fix the marked block sits far below the fold.
#[gpui::test]
fn follow_along_scrolls_the_marked_block_into_view(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(TranscriptPaneState {
            transcript_id: "t-follow".into(),
            media_path: "/generated/follow.wav".into(),
            gallery_asset_id: None,
            alignment: "original".into(),
            alignment_note: None,
            words: Some(
                (0..200)
                    .map(|index| PaneWord {
                        word: format!("follow-word-{index}"),
                        start_ms: index as u64 * 100,
                        end_ms: index as u64 * 100 + 90,
                    })
                    .collect(),
            ),
            working_text: String::new(),
            paragraph_breaks: Some((1..20).map(|block| block * 10 - 1).collect()),
            speaker_spans: None,
            review: None,
            layers: Vec::new(),
            highlights: Vec::new(),
            strikes: Vec::new(),
            correction_edits: Vec::new(),
            source_words: Vec::new(),
        });
        // The open path's post-state (T3).
        viewer.active_tab = ViewerTab::Workspace;
        viewer.inspector_open = false;
        cx.notify();
    });

    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(600.)));
    cx.run_until_parked();

    // The mark lands on a late block — the follow must scroll that
    // block into view (the loop calls the same method each tick;
    // the mark computation is separately pinned by
    // current_word_index_marks_the_containing_word).
    viewer.update(cx, |viewer, cx| {
        viewer.current_word = Some(190);
        viewer.follow_viewport = true;
        viewer.follow_marked_block_into_view();
        cx.notify();
    });
    cx.run_until_parked();

    let bounds = cx
        .debug_bounds("media-transcript-marked-block")
        .expect("the follow-along must scroll the marked block into view");
    assert!(
        bounds.top() >= px(0.),
        "the marked block is on-screen, not above the pane: {bounds:?}"
    );
    assert!(
        bounds.bottom() <= px(600.),
        "the marked block is within the pane: {bounds:?}"
    );
}

/// A manual scroll suspends follow-along viewport tracking, and the
/// Follow toggle re-enables it (S3d, R1 acceptance). The observable a
/// wheel scroll produces on the follow tick is the list-offset change;
/// the toggle then reveals the current mark again.
#[gpui::test]
fn manual_scroll_suspends_follow_and_the_toggle_re_enables(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript = Some(TranscriptPaneState {
            transcript_id: "t-suspend".into(),
            media_path: "/generated/suspend.wav".into(),
            gallery_asset_id: None,
            alignment: "original".into(),
            alignment_note: None,
            words: Some(
                (0..200)
                    .map(|index| PaneWord {
                        word: format!("suspend-word-{index}"),
                        start_ms: index as u64 * 100,
                        end_ms: index as u64 * 100 + 90,
                    })
                    .collect(),
            ),
            working_text: String::new(),
            paragraph_breaks: Some((1..20).map(|block| block * 10 - 1).collect()),
            speaker_spans: None,
            review: None,
            layers: Vec::new(),
            highlights: Vec::new(),
            strikes: Vec::new(),
            correction_edits: Vec::new(),
            source_words: Vec::new(),
        });
        // The open path's post-state (T3).
        viewer.active_tab = ViewerTab::Workspace;
        viewer.inspector_open = false;
        cx.notify();
    });

    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(600.)));
    cx.run_until_parked();

    // Baseline the tick, scroll by hand, and observe the next tick:
    // the offset changed with no follow reveal pending, so tracking
    // suspends.
    viewer.update(cx, |viewer, cx| {
        viewer.note_follow_scroll_tick(cx);
        viewer.transcript_list.scroll_by(px(200.));
        viewer.note_follow_scroll_tick(cx);
        assert!(
            !viewer.follow_viewport,
            "a manual scroll suspends viewport tracking"
        );
        cx.notify();
    });
    cx.run_until_parked();
    // Scrolling back to the top is the operator's way back to the Follow
    // chip — the whole workspace scrolls together now (S3d).
    viewer.update(cx, |viewer, cx| {
        viewer.transcript_list.scroll_by(px(-200.));
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("media-transcript-follow-toggle").is_some(),
        "the Follow toggle renders in the workspace header rows"
    );

    // The toggle re-enables tracking and reveals the current mark.
    viewer.update(cx, |viewer, cx| {
        viewer.current_word = Some(190);
        viewer.toggle_follow_viewport(cx);
        assert!(
            viewer.follow_viewport,
            "the toggle re-enables viewport tracking"
        );
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("media-transcript-marked-block").is_some(),
        "re-enabling follow reveals the marked block"
    );
}

/// The per-recording transcription status (R2/R5): a running transcribe
/// job whose source is the recording shows Transcribing; a stored
/// transcript linked to the recording shows Transcribed; otherwise
/// Not transcribed. [P1] Motivating: the recordings browser is the
/// recordings CRUD the reference model's capability 3 names.
#[test]
fn recordings_rows_carry_transcription_status() {
    let mut viewer = MediaViewer::new();
    viewer.assets = vec![
        MediaAsset {
            body: r#"{"kind":"video","src":"/talk.mp4"}"#.into(),
            src: "/talk.mp4".into(),
            kind: "video".into(),
            tool: "gallery".into(),
            gallery_index: Some(0),
            gallery_asset_id: Some("asset-talk".into()),
        },
        MediaAsset {
            body: r#"{"kind":"audio","src":"/note.wav"}"#.into(),
            src: "/note.wav".into(),
            kind: "audio".into(),
            tool: "gallery".into(),
            gallery_index: Some(1),
            gallery_asset_id: Some("asset-note".into()),
        },
    ];
    viewer.transcript_listing = Some(vec![TranscriptSummaryRow {
        id: "t-talk".into(),
        media_path: "/talk.mp4".into(),
        words_count: 42,
        duration_secs: 30,
        has_word_timings: true,
        alignment: "original".into(),
        gallery_asset_id: Some("asset-talk".into()),
    }]);
    viewer.live_jobs = vec![JobRecord {
        id: "job-transcribe".into(),
        op: "transcribe".into(),
        status: "running".into(),
        created_at: "2026-10-28T00:00:00Z".into(),
        completed_at: None,
        result: None,
        error: None,
        source: Some("/note.wav".into()),
    }];
    assert_eq!(
        viewer.recording_transcription_status(&viewer.assets[0]),
        RecordingTranscriptionStatus::Transcribed,
        "a linked transcript shows Transcribed"
    );
    assert_eq!(
        viewer.recording_transcription_status(&viewer.assets[1]),
        RecordingTranscriptionStatus::Transcribing,
        "a running transcribe job against the recording shows Transcribing"
    );
    // A gallery-backed recording with no transcript and no job shows
    // Not transcribed — the operator can take it to its transcript.
    viewer.assets.push(MediaAsset {
        body: r#"{"kind":"video","src":"/later.mp4"}"#.into(),
        src: "/later.mp4".into(),
        kind: "video".into(),
        tool: "gallery".into(),
        gallery_index: Some(2),
        gallery_asset_id: Some("asset-later".into()),
    });
    let later = viewer.assets.last().unwrap();
    assert_eq!(
        viewer.recording_transcription_status(later),
        RecordingTranscriptionStatus::NotTranscribed,
        "a recording with neither transcript nor job shows Not transcribed"
    );
}

/// The Transcribe affordance (R5, T3) renders on a not-transcribed
/// recording's Library row and dispatches the async transcribe job
/// against the recording's source; the unwired-invoker degradation
/// surfaces a status (tests run without a shared tool invoker).
#[gpui::test]
fn transcribe_affordance_renders_and_degrades_honestly(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.assets = vec![MediaAsset {
            body: r#"{"kind":"video","src":"/later.mp4"}"#.into(),
            src: "/later.mp4".into(),
            kind: "video".into(),
            tool: "gallery".into(),
            gallery_index: Some(0),
            gallery_asset_id: Some("asset-later".into()),
        }];
        viewer.gallery_total = Some(1);
        viewer.transcript_listing = Some(vec![]);
        viewer.active_tab = ViewerTab::Library;
        cx.notify();
    });

    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(320.)));
    cx.run_until_parked();

    // The Transcribe affordance renders on the not-transcribed
    // recording's Library row; the operator can take the recording to
    // its transcript without a chat turn.
    assert!(
        cx.debug_bounds("media-recording-transcribe").is_some(),
        "the Transcribe affordance must render on the recording's Library row"
    );
    assert!(
        cx.debug_bounds("media-transcript-delete").is_none(),
        "no transcript rows render for a not-transcribed recording"
    );

    // The dispatch surfaces a status even on the unwired degradation
    // path — never silence (D5).
    viewer.update(cx, |viewer, cx| {
        viewer.transcribe_recording("asset-later".into(), "/later.mp4".into(), cx);
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, _cx| {
        assert!(
            viewer
                .status
                .as_deref()
                .is_some_and(|status| status.contains("cannot transcribe")),
            "the unwired-invoker degradation must surface a status, got {:?}",
            viewer.status
        );
    });
}

/// The Library's transcript rows' two-click delete (T3): the Delete
/// affordance renders per row — nested under its recording, or in the
/// unlinked section — arming swaps it for Confirm delete, and the
/// dispatch closes the edit lifecycle through the unwired-invoker
/// degradation path (tests run without a shared tool invoker).
#[gpui::test]
fn library_transcript_rows_delete_confirms_and_closes_the_edit(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.transcript_listing = Some(vec![TranscriptSummaryRow {
            id: "t-delete".into(),
            media_path: "/generated/dunning.mp4".into(),
            words_count: 558,
            duration_secs: 276,
            has_word_timings: true,
            alignment: "original".into(),
            gallery_asset_id: None,
        }]);
        viewer.gallery_total = Some(0);
        viewer.active_tab = ViewerTab::Library;
        cx.notify();
    });

    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(320.)));
    cx.run_until_parked();

    // The Delete affordance is laid out on the row, alongside the open
    // affordance — the operator can always remove a stored transcript;
    // the row renders in the unlinked section below the (empty) asset
    // rows, reachable, never hidden.
    assert!(
        cx.debug_bounds("media-transcript-delete").is_some(),
        "the Delete affordance must render on the Library's transcript row"
    );

    // Arming the confirmation swaps the affordance.
    viewer.update(cx, |viewer, cx| {
        viewer
            .confirm_delete_transcript
            .confirm(&"t-delete".to_owned());
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("media-transcript-delete-confirm").is_some(),
        "the armed row must show Confirm delete"
    );
    assert!(
        cx.debug_bounds("media-transcript-delete").is_none(),
        "the plain Delete affordance must be replaced while confirming"
    );

    // The dispatch closes the edit lifecycle even on the unwired
    // degradation path — a second admission must not report an edit
    // already in progress (the lifecycle would be stuck Loading).
    viewer.update(cx, |viewer, cx| {
        viewer.delete_transcript_recording("t-delete".into(), cx);
    });
    cx.run_until_parked();
    viewer.update(cx, |viewer, cx| {
        assert!(
            viewer
                .status
                .as_deref()
                .is_some_and(|status| status.contains("cannot delete recordings")),
            "the unwired-invoker degradation must surface a status, got {:?}",
            viewer.status
        );
        assert_eq!(
            viewer.confirm_delete_transcript.armed, None,
            "dispatch disarms the confirmation"
        );
        viewer.delete_transcript_recording("t-delete".into(), cx);
        assert!(
            !viewer
                .status
                .as_deref()
                .is_some_and(|status| status.contains("already in progress")),
            "a completed edit must admit the next one"
        );
        cx.notify();
    });
}

/// The activity drawer (T2) renders on demand over any mode: the
/// observed live rows first with Cancel for queued/running jobs, the
/// recent history after them (terminal — no Cancel), and the
/// process-local history disclosure in the footer. The toolbar
/// toggle opens and closes it.
#[gpui::test]
fn activity_drawer_renders_rows_cancel_and_disclosure(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.active_tab = ViewerTab::Library;
        viewer.gallery_total = Some(0);
        viewer.jobs_total = Some(2);
        viewer.jobs_limit = hkask_media::media_limits::DEFAULT_JOB_LIST_LIMIT;
        viewer.jobs_has_more = false;
        let mut live = scheduled_job("job-live", "transcribe", "running", "2026-09-05T00:00:00Z");
        live.source = Some("/talk.wav".into());
        viewer.live_jobs = vec![live];
        viewer.jobs = vec![scheduled_job(
            "job-done",
            "video_clip",
            "completed",
            "2026-09-06T00:00:00Z",
        )];
        viewer.jobs_history_scope = Some(hkask_media::jobs_contract::JOB_HISTORY_SCOPE.into());
        viewer.jobs_restart_behavior =
            Some(hkask_media::jobs_contract::JOB_RESTART_BEHAVIOR.into());
        viewer.activity_open = true;
        cx.notify();
    });

    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(320.)));
    cx.run_until_parked();

    assert!(
        cx.debug_bounds("media-activity-toggle").is_some(),
        "the toolbar's Activity toggle renders"
    );
    assert!(
        cx.debug_bounds("media-activity-drawer").is_some(),
        "the drawer renders open over any mode"
    );
    assert!(
        cx.debug_bounds("media-activity-row-job-live").is_some(),
        "the observed live row renders"
    );
    assert!(
        cx.debug_bounds("media-activity-cancel-job-live").is_some(),
        "a queued/running row carries Cancel"
    );
    assert!(
        cx.debug_bounds("media-activity-row-job-done").is_some(),
        "the recent-history row renders"
    );
    assert!(
        cx.debug_bounds("media-activity-cancel-job-done").is_none(),
        "a terminal history row carries no Cancel"
    );
    assert!(
        cx.debug_bounds("media-activity-disclosure").is_some(),
        "the footer carries the process-local history disclosure"
    );

    // The toggle closes the drawer — ambient state, never a
    // destination.
    viewer.update(cx, |viewer, cx| viewer.toggle_activity(cx));
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("media-activity-drawer").is_none(),
        "the toggle closes the drawer"
    );
}

/// The ambient activity badge (T2): a live research job whose primary
/// source is the asset badges its Library row — transient (it clears
/// when the job is no longer live), never error-styled.
#[gpui::test]
fn library_rows_badge_live_jobs(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
        viewer.active_tab = ViewerTab::Library;
        viewer.gallery_total = Some(2);
        viewer.assets = vec![
            MediaAsset {
                body: r#"{"kind":"audio","src":"/talk.wav"}"#.into(),
                src: "/talk.wav".into(),
                kind: "audio".into(),
                tool: "gallery".into(),
                gallery_index: Some(0),
                gallery_asset_id: Some("asset-talk".into()),
            },
            MediaAsset {
                body: r#"{"kind":"video","src":"/other.mp4"}"#.into(),
                src: "/other.mp4".into(),
                kind: "video".into(),
                tool: "gallery".into(),
                gallery_index: Some(1),
                gallery_asset_id: Some("asset-other".into()),
            },
        ];
        let mut live = scheduled_job("job-live", "transcribe", "running", "2026-09-05T00:00:00Z");
        live.source = Some("/talk.wav".into());
        viewer.live_jobs = vec![live];
        cx.notify();
    });

    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(320.)));
    cx.run_until_parked();

    assert!(
        cx.debug_bounds("media-library-activity-0").is_some(),
        "the affected asset's Library row carries the badge"
    );
    assert!(
        cx.debug_bounds("media-library-activity-1").is_none(),
        "an unaffected asset carries no badge"
    );
    // The badge is transient: it clears when the job is no longer
    // live (HIG Progress indicators — present only while ongoing).
    viewer.update(cx, |viewer, cx| {
        viewer.live_jobs.clear();
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("media-library-activity-0").is_none(),
        "the badge clears when the job is no longer live"
    );
}

/// The shared fit assertions: the video area and the whole player
/// widget must stay within the dock's right edge, and the video area's
/// width must stay within the pane's available width (+1px sub-pixel
/// tolerance).
fn assert_viewer_fits_narrow_pane(
    cx: &mut gpui::VisualTestContext,
    dock_width: gpui::Pixels,
    pane_available: gpui::Pixels,
) {
    let widget_bounds = cx
        .debug_bounds("media-widget")
        .expect("widget laid out with debug bounds");
    let video_bounds = cx
        .debug_bounds("media-video-area")
        .expect("video area laid out with debug bounds");
    assert!(
        widget_bounds.right() <= dock_width + px(1.),
        "the player must fit inside the {dock_width:?} dock: widget right {:?}",
        widget_bounds.right()
    );
    assert!(
        video_bounds.right() <= dock_width + px(1.),
        "the video area must fit inside the {dock_width:?} dock: right {:?}",
        video_bounds.right()
    );
    assert!(
        video_bounds.size.width <= pane_available + px(1.),
        "the video area must stay within the pane's ~{pane_available:?} available \
             width, got {:?} — the pane inflated to its content's min-width",
        video_bounds.size.width
    );
    // The inspector pane (T1): the pane and all seven of its
    // pane-internal tabs render inside the same dock — the tab row
    // wraps instead of inflating the pane's min-content.
    let inspector_bounds = cx
        .debug_bounds("media-inspector")
        .expect("inspector laid out with debug bounds");
    assert!(
        inspector_bounds.right() <= dock_width + px(1.),
        "the inspector must fit inside the {dock_width:?} dock: right {:?}",
        inspector_bounds.right()
    );
    for tab in InspectorTab::ALL {
        cx.debug_bounds(tab.element_id())
            .unwrap_or_else(|| panic!("inspector tab {:?} laid out", tab));
    }
}

/// The events surface (S5) renders inside the same narrow-dock
/// workspace: the surface toggle chips, the time-ordered event rows,
/// and the authoring section all lay out within the pane — the toggle
/// is a reading-surface switch, not a new workspace.
#[gpui::test]
fn events_surface_renders_rows_and_authoring_section(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| {
        let mut viewer = MediaViewer::new();
        viewer.transcript = Some(TranscriptPaneState {
            transcript_id: "t-events".into(),
            media_path: "/generated/one.mp4".into(),
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
        viewer.visual_events = Some(VisualEventsState {
            status: "completed".into(),
            events: vec![
                PaneEventRow {
                    start_sec: 0.0,
                    end_sec: Some(4.2),
                    kind: "shot".into(),
                    layer_id: "layer_shot".into(),
                    item_id: "item_s0".into(),
                    searchable_text: "shot 0".into(),
                    model: "ffmpeg:scdet".into(),
                    box_norm: None,
                    object_label: None,
                    evidence: vec!["seen".into()],
                },
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
            ],
            events_total: 2,
            kinds_present: vec!["shot".into(), "object".into()],
            note: None,
        });
        viewer.transcript_surface = TranscriptSurface::Events;
        // The open path's post-state (T3).
        viewer.active_tab = ViewerTab::Workspace;
        viewer.inspector_open = false;
        viewer
    });

    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(600.)));
    cx.run_until_parked();

    // The list paints items only within its viewport (virtualized):
    // the toggle chips are what the first frame guarantees in view
    // (T3: the sub-surface's purpose line and header row sit above the
    // list, so the pinned 140px player row and the chips fill the
    // first frame); the event rows render when scrolled to — the
    // reachability contract under virtualization.
    let bounds = cx
        .debug_bounds("media-transcript-surface-events")
        .expect("the surface toggle chip must lay out");
    assert!(
        bounds.right() <= px(480.),
        "the surface toggle chip fits the dock: {bounds:?}"
    );
    viewer.update(cx, |viewer, cx| {
        viewer.transcript_list.scroll_to(gpui::ListOffset {
            item_ix: WORKSPACE_HEADER_ITEMS,
            offset_in_item: px(0.),
        });
        cx.notify();
    });
    cx.run_until_parked();
    let bounds = cx
        .debug_bounds("media-event-row-0")
        .expect("the first event row must lay out when scrolled to");
    assert!(
        bounds.right() <= px(480.),
        "the first event row fits the dock: {bounds:?}"
    );

    // Scrolling the virtualized list to the tail brings the authoring
    // section into view — the section renders as a real laid-out row,
    // not a phantom.
    viewer.update(cx, |viewer, cx| {
        viewer.transcript_list.scroll_to(gpui::ListOffset {
            item_ix: viewer.transcript_list_count.saturating_sub(1),
            offset_in_item: px(0.),
        });
        cx.notify();
    });
    cx.run_until_parked();
    let bounds = cx
        .debug_bounds("media-events-authoring-section")
        .expect("the authoring section must lay out");
    assert!(
        bounds.right() <= px(480.),
        "the authoring section fits the dock: {bounds:?}"
    );
}

/// The events-surface layout fixture: the two-row event log (a shot
/// row and a box-carrying object row) with a caller-set total — a
/// total above the row count makes the state a truncated page.
fn events_state(events_total: usize) -> VisualEventsState {
    VisualEventsState {
        status: "completed".into(),
        events: vec![
            PaneEventRow {
                start_sec: 0.0,
                end_sec: Some(4.2),
                kind: "shot".into(),
                layer_id: "layer_shot".into(),
                item_id: "item_s0".into(),
                searchable_text: "shot 0".into(),
                model: "ffmpeg:scdet".into(),
                box_norm: None,
                object_label: None,
                evidence: vec!["seen".into()],
            },
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
        ],
        events_total,
        kinds_present: vec!["shot".into(), "object".into()],
        note: None,
    }
}

/// An events-surface workspace over `/generated/one.mp4` with the
/// given event log — the truncation tests' shared viewer.
fn events_surface_viewer(state: VisualEventsState) -> MediaViewer {
    let mut viewer = MediaViewer::new();
    viewer.transcript = Some(TranscriptPaneState {
        transcript_id: "t-events".into(),
        media_path: "/generated/one.mp4".into(),
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
    viewer.visual_events = Some(state);
    viewer.transcript_surface = TranscriptSurface::Events;
    // The open path's post-state (T3).
    viewer.active_tab = ViewerTab::Workspace;
    viewer.inspector_open = false;
    viewer
}

/// A page shorter than the recording's total events surfaces the
/// truncation note at the bottom of the event list: the list carries
/// one extra tail item, the note lays out under the last event row,
/// and the authoring section still lays out below it.
#[gpui::test]
fn events_surface_truncation_note_renders_below_the_event_rows(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| events_surface_viewer(events_state(1200)));

    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(600.)));
    cx.run_until_parked();

    // The truncated page carries one extra tail item: the fixed
    // header, the two event rows, the truncation note, then the
    // authoring section.
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(
            viewer.transcript_list_count,
            WORKSPACE_HEADER_ITEMS + 2 + WORKSPACE_EVENTS_TAIL_ITEMS + 1
        );
    });

    viewer.update(cx, |viewer, cx| {
        viewer.transcript_list.scroll_to(gpui::ListOffset {
            item_ix: WORKSPACE_HEADER_ITEMS + 2,
            offset_in_item: px(0.),
        });
        cx.notify();
    });
    cx.run_until_parked();
    let bounds = cx
        .debug_bounds("media-events-truncation-note")
        .expect("the truncation note must lay out");
    assert!(
        bounds.right() <= px(480.),
        "the truncation note fits the dock: {bounds:?}"
    );

    // The authoring section still lays out below the note.
    viewer.update(cx, |viewer, cx| {
        viewer.transcript_list.scroll_to(gpui::ListOffset {
            item_ix: viewer.transcript_list_count.saturating_sub(1),
            offset_in_item: px(0.),
        });
        cx.notify();
    });
    cx.run_until_parked();
    let bounds = cx
        .debug_bounds("media-events-authoring-section")
        .expect("the authoring section must lay out");
    assert!(
        bounds.right() <= px(480.),
        "the authoring section fits the dock: {bounds:?}"
    );
}

/// A full page is not truncated: the tail stays exactly the authoring
/// section — no phantom truncation note — so the note's presence
/// itself is the truncation signal.
#[gpui::test]
fn events_surface_full_page_renders_no_truncation_note(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| events_surface_viewer(events_state(2)));

    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(600.)));
    cx.run_until_parked();

    viewer.read_with(cx, |viewer, _| {
        assert_eq!(
            viewer.transcript_list_count,
            WORKSPACE_HEADER_ITEMS + 2 + WORKSPACE_EVENTS_TAIL_ITEMS
        );
    });

    viewer.update(cx, |viewer, cx| {
        viewer.transcript_list.scroll_to(gpui::ListOffset {
            item_ix: WORKSPACE_HEADER_ITEMS + 2,
            offset_in_item: px(0.),
        });
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("media-events-truncation-note").is_none(),
        "a full page renders no truncation note"
    );
    cx.debug_bounds("media-events-authoring-section")
        .expect("the authoring section still lays out");
}

/// A real pointer two-click on the rendered words reaches the
/// selection path — the click hitboxes map window coordinates to the
/// word ranges, and the completed range opens the selection popover.
/// [P1] Motivating: the interaction must work through the painted
/// surface, not only through the handler methods the unit pins call.
#[gpui::test]
fn pointer_two_click_selection_opens_the_tray(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-diag-click".into(),
                media_path: "/generated/diag.wav".into(),
                gallery_asset_id: None,
                alignment: "original".into(),
                alignment_note: None,
                words: Some(
                    (0..12)
                        .map(|index| PaneWord {
                            word: format!("word{index:02}"),
                            start_ms: index as u64 * 100,
                            end_ms: index as u64 * 100 + 90,
                        })
                        .collect(),
                ),
                working_text: "word00 word01 word02 word03 word04 word05 word06 word07 word08 word09 word10 word11".into(),
                paragraph_breaks: Some(vec![5, 11]),
                speaker_spans: None,
                review: None,
                layers: Vec::new(),
                highlights: Vec::new(),
                strikes: Vec::new(),
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
            cx.notify();
        });
    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(600.)));
    cx.run_until_parked();

    // Test windows start inactive — activate before the pointer path
    // can dispatch (the production window is active on first click).
    cx.update_window_entity(&viewer, |_, window, _| window.activate_window());
    cx.run_until_parked();
    let paragraph = cx
        .debug_bounds("media-transcript-paragraph")
        .expect("block 0 laid out");
    // Select mode on, then two clicks: anchor word 0, complete on the
    // word under the second click.
    cx.update_window_entity(&viewer, |viewer, _, cx| {
        viewer.selection_mode = true;
        cx.notify();
    });
    cx.run_until_parked();
    let first = gpui::Point {
        x: paragraph.left() + gpui::px(4.),
        y: paragraph.center().y,
    };
    let second = gpui::Point {
        x: paragraph.left() + gpui::px(120.),
        y: paragraph.center().y,
    };
    cx.simulate_mouse_down(first, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_up(first, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_down(second, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_up(second, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();

    viewer.read_with(cx, |viewer, _| {
        assert!(
            viewer.selection_anchor.is_none(),
            "the completed range clears the anchor"
        );
        assert_eq!(
            viewer.selection_popover,
            Some((0, 1)),
            "the two pointer clicks complete the ordered range"
        );
    });
    assert!(
        cx.debug_bounds("media-transcript-selection-popover")
            .is_some(),
        "the completed range opens the selection popover"
    );
}

/// dcterms:identifier: `MediaViewer::context_menu_range`
/// expect: an open selection popover owns its range; otherwise a
/// pending two-click anchor completes to the hovered word; a bare
/// right click over a word selects that word alone; a right click
/// outside the words (no hovered word, no anchor) has no range.
#[test]
fn context_menu_range_prefers_the_open_tray() {
    let mut viewer = MediaViewer::new();
    viewer.selection_popover = Some((3, 9));
    viewer.selection_anchor = Some(1);
    viewer.context_menu_fallback = Some(7);
    assert_eq!(
        viewer.context_menu_range(),
        Some((3, 9)),
        "the open tray's range wins"
    );
    viewer.selection_popover = None;
    assert_eq!(
        viewer.context_menu_range(),
        Some((1, 7)),
        "a pending anchor completes to the right-click target"
    );
    viewer.selection_anchor = None;
    assert_eq!(
        viewer.context_menu_range(),
        Some((7, 7)),
        "a bare right click selects the right-click target word"
    );
    viewer.context_menu_fallback = None;
    assert_eq!(
        viewer.context_menu_range(),
        None,
        "a right click outside the words has no range"
    );
}

/// A right click over the rendered words opens the context menu with
/// the Highlight and Strike out entries — the pointer alternative to
/// the select-then-tray gesture. [P1] Motivating: the operator's
/// "select text, then right-click to choose highlight or strike" is
/// reachable through the painted surface.
#[gpui::test]
fn right_click_opens_context_menu_with_highlight_and_strike(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-context-menu".into(),
                media_path: "/generated/context-menu.wav".into(),
                gallery_asset_id: None,
                alignment: "original".into(),
                alignment_note: None,
                words: Some(
                    (0..12)
                        .map(|index| PaneWord {
                            word: format!("word{index:02}"),
                            start_ms: index as u64 * 100,
                            end_ms: index as u64 * 100 + 90,
                        })
                        .collect(),
                ),
                working_text: "word00 word01 word02 word03 word04 word05 word06 word07 word08 word09 word10 word11".into(),
                paragraph_breaks: Some(vec![5, 11]),
                speaker_spans: None,
                review: None,
                layers: Vec::new(),
                highlights: Vec::new(),
                strikes: Vec::new(),
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
            cx.notify();
        });
    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(600.)));
    cx.run_until_parked();
    cx.update_window_entity(&viewer, |_, window, _| window.activate_window());
    cx.run_until_parked();
    let paragraph = cx
        .debug_bounds("media-transcript-paragraph")
        .expect("block 0 laid out");
    let point = gpui::Point {
        x: paragraph.left() + gpui::px(4.),
        y: paragraph.center().y,
    };

    // The pointer rests on a word first (hover tracks it), then the
    // right click opens the menu for that word.
    cx.simulate_mouse_move(point, None, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_down(point, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.simulate_mouse_up(point, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("media-transcript-context-menu").is_some(),
        "the context menu renders"
    );
    assert!(
        cx.debug_bounds("media-transcript-context-menu-highlight")
            .is_some(),
        "the context menu offers Highlight"
    );
    assert!(
        cx.debug_bounds("media-transcript-context-menu-strike")
            .is_some(),
        "the context menu offers Strike out"
    );

    // Choosing Highlight opens the label editor over the range.
    let highlight = cx
        .debug_bounds("media-transcript-context-menu-highlight")
        .expect("Highlight entry laid out");
    cx.simulate_click(highlight.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    viewer.read_with(cx, |viewer, _| {
        assert!(
            matches!(
                &viewer.highlight_edit,
                Some(HighlightEdit {
                    kind: HighlightEditKind::Create {
                        start_word: 0,
                        end_word: 0
                    },
                    ..
                })
            ),
            "Highlight opens the label editor over the right-clicked word"
        );
    });
}

/// Escape dismisses the open context menu without acting — the menu's
/// dismissal key, bound on the menu while it holds focus.
#[gpui::test]
fn context_menu_escape_dismisses(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-context-escape".into(),
                media_path: "/generated/context-escape.wav".into(),
                gallery_asset_id: None,
                alignment: "original".into(),
                alignment_note: None,
                words: Some(
                    (0..12)
                        .map(|index| PaneWord {
                            word: format!("word{index:02}"),
                            start_ms: index as u64 * 100,
                            end_ms: index as u64 * 100 + 90,
                        })
                        .collect(),
                ),
                working_text: "word00 word01 word02 word03 word04 word05 word06 word07 word08 word09 word10 word11".into(),
                paragraph_breaks: Some(vec![5, 11]),
                speaker_spans: None,
                review: None,
                layers: Vec::new(),
                highlights: Vec::new(),
                strikes: Vec::new(),
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
            cx.notify();
        });
    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(600.)));
    cx.run_until_parked();
    cx.update_window_entity(&viewer, |_, window, _| window.activate_window());
    cx.run_until_parked();
    let paragraph = cx
        .debug_bounds("media-transcript-paragraph")
        .expect("block 0 laid out");
    let point = gpui::Point {
        x: paragraph.left() + gpui::px(4.),
        y: paragraph.center().y,
    };
    cx.simulate_mouse_move(point, None, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_down(point, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.simulate_mouse_up(point, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("media-transcript-context-menu").is_some(),
        "the context menu opens"
    );
    // Escape dismisses the open menu (the menu holds focus).
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    viewer.read_with(cx, |viewer, _| {
        assert!(
            viewer.context_menu.is_none(),
            "Escape dismisses the context menu"
        );
    });
}

/// A completed selection followed by a right click opens the context
/// menu over that selection, and its Strike out entry dispatches the
/// strike path — the operator's "select text, then right-click to
/// choose highlight or strike" for the cut half of the pair.
#[gpui::test]
fn right_click_after_selection_strikes_the_selection(cx: &mut TestAppContext) {
    init_layout_test_globals(cx);
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-context-strike".into(),
                media_path: "/generated/context-strike.wav".into(),
                gallery_asset_id: None,
                alignment: "original".into(),
                alignment_note: None,
                words: Some(
                    (0..12)
                        .map(|index| PaneWord {
                            word: format!("word{index:02}"),
                            start_ms: index as u64 * 100,
                            end_ms: index as u64 * 100 + 90,
                        })
                        .collect(),
                ),
                working_text: "word00 word01 word02 word03 word04 word05 word06 word07 word08 word09 word10 word11".into(),
                paragraph_breaks: Some(vec![5, 11]),
                speaker_spans: None,
                review: None,
                layers: Vec::new(),
                highlights: Vec::new(),
                strikes: Vec::new(),
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
            cx.notify();
        });
    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.simulate_resize(size(px(480.), px(600.)));
    cx.run_until_parked();
    cx.update_window_entity(&viewer, |_, window, _| window.activate_window());
    cx.run_until_parked();
    let paragraph = cx
        .debug_bounds("media-transcript-paragraph")
        .expect("block 0 laid out");
    // Select words 3–9 with the two-click gesture, completing the
    // selection popover, then right-click to reach the context menu.
    cx.update_window_entity(&viewer, |viewer, _, cx| {
        viewer.selection_mode = true;
        cx.notify();
    });
    cx.run_until_parked();
    let first = gpui::Point {
        x: paragraph.left() + gpui::px(4.),
        y: paragraph.center().y,
    };
    let second = gpui::Point {
        x: paragraph.left() + gpui::px(120.),
        y: paragraph.center().y,
    };
    cx.simulate_mouse_down(first, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_up(first, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_down(second, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_up(second, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(
        viewer.read_with(cx, |viewer, _| viewer.selection_popover.is_some()),
        "the two clicks complete the selection popover"
    );
    // The pointer rests on a word, then the right click opens the menu
    // over the completed selection.
    cx.simulate_mouse_move(first, None, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_down(first, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.simulate_mouse_up(first, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("media-transcript-context-menu").is_some(),
        "the context menu opens over the completed selection"
    );

    // Strike out dispatches the strike over the selection's range.
    let strike = cx
        .debug_bounds("media-transcript-context-menu-strike")
        .expect("Strike out entry laid out");
    cx.simulate_click(strike.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    viewer.read_with(cx, |viewer, _| {
        assert!(
            viewer
                .status
                .as_deref()
                .is_some_and(|status| { status.contains("Tool invoker not wired") }),
            "Strike out reached the strike dispatcher: {:?}",
            viewer.status
        );
        assert!(
            matches!(viewer.edit_request.state, ResourceState::Failed(_)),
            "the strike refusal failed the edit admission"
        );
    });
}

/// S099-F02: when the transcript's linked recording is selected in the
/// workspace but its media player fails to bind (e.g. unapproved or
/// missing path), rendering the transcript workspace must fall through to
/// the "Playback unavailable" UI without panicking in `.expect(...)`.
#[gpui::test]
fn transcript_workspace_does_not_panic_when_recording_widget_fails_to_bind(
    cx: &mut TestAppContext,
) {
    init_layout_test_globals(cx);
    let unapproved_path = "/nonexistent/unapproved-recording.mp4";
    let viewer = cx.new(|_| MediaViewer::new());
    viewer.update(cx, |viewer, _cx| {
        viewer.active_tab = ViewerTab::Workspace;
        viewer.assets = vec![MediaAsset {
            kind: "video".into(),
            src: unapproved_path.into(),
            tool: "gallery_list_assets".into(),
            body: "not-valid-media-json".into(),
            gallery_index: Some(0),
            gallery_asset_id: Some("asset-unapproved".into()),
        }];
        viewer.selected = Some(0);
        viewer.transcript = Some(TranscriptPaneState {
            transcript_id: "tr-unapproved".into(),
            media_path: unapproved_path.into(),
            gallery_asset_id: Some("asset-unapproved".into()),
            alignment: "original".into(),
            alignment_note: None,
            words: Some(vec![PaneWord {
                word: "hello".into(),
                start_ms: 0,
                end_ms: 500,
            }]),
            working_text: "hello".into(),
            paragraph_breaks: None,
            speaker_spans: None,
            review: None,
            layers: Vec::new(),
            highlights: Vec::new(),
            strikes: Vec::new(),
            correction_edits: Vec::new(),
            source_words: vec![PaneWord {
                word: "hello".into(),
                start_ms: 0,
                end_ms: 500,
            }],
        });
    });

    let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
        viewer: viewer.clone(),
    });
    cx.run_until_parked();
    viewer.read_with(cx, |viewer, _| {
        assert!(
            viewer.media_widget.is_none(),
            "unapproved media path must leave media_widget unbound"
        );
    });
}
