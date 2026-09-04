//! Media Panel — a Steer-only surface for the `hkask-mcp-media` MCP server.
//!
//! Like the portfolio panel, this panel deliberately has **no browse forms** —
//! the media server exposes 60+ tools spanning gallery management, image/video
//! generation, voice synthesis, transcription, and face recognition. A
//! hand-written management UI for all of these would be impractical and would
//! duplicate the Steer conversation's chat-driven workflow. The panel's sole
//! affordance is a scoped curator `ConversationView` (via `hkask_steer::SteerSurface`)
//! whose prompt advertises the media server's generated `TOOL_NAMES`.
//!
//! Generated media (images, videos) renders inline in the conversation via the
//! media block renderer (the D18 seam). The operator asks the curator to
//! generate, search, organize, or transform media; the curator dispatches via
//! the media MCP tools and results appear inline.

pub mod media_viewer;
pub mod panel_button;

use gpui::{
    App, Bounds, ClickEvent, Context, DefiniteLength, DragMoveEvent, Entity, EventEmitter,
    FocusHandle, Focusable, Pixels, SharedString, Task, WeakEntity, Window, actions, px,
};
use ui::{Icon, IconName, prelude::*};
use util::ResultExt as _;
use workspace::{
    Workspace,
    item::{Item, ItemEvent, SerializableItem},
    register_serializable_item,
};

pub use media_viewer::MediaViewer;
pub use panel_button::MediaPanelButton;

/// The MCP server id this panel's Steer conversation is scoped to.
/// Matches `kask_bridge::mcp_servers::BUILT_IN_MCP_SERVERS` (id: "media").
const MEDIA_SERVER: &str = "media";

/// The drag value carried by GPUI's drag system: the split handle's
/// `on_drag` starts the drag, the panel root's `on_drag_move` consumes it
/// for the drag's duration (the git-graph split's pattern).
struct DraggedSplitHandle;

/// Height of the divider's invisible grab area. The visible rule is 1px;
/// 8px is grabbable without covering either pane's content.
const SPLIT_HANDLE_HIT_HEIGHT: f32 = 8.0;

/// The steer pane's share of the panel height: the default split, and the
/// drag clamp. The floor keeps the director's header + prompt editor
/// usable; the ceiling keeps the viewer's tab bar + player visible.
const DEFAULT_STEER_FRACTION: f32 = 0.5;
const MIN_STEER_FRACTION: f32 = 0.2;
const MAX_STEER_FRACTION: f32 = 0.8;

actions!(
    media_panel,
    [
        /// Deploys a new Media Panel if none is open, else focuses the
        /// existing one. Used by the View menu entry and the status bar button.
        Toggle,
        /// Focuses an existing Media Panel (no-op if none is open).
        ToggleFocus,
    ]
);

/// Register the panel's actions on every new `Workspace`.
pub fn init(cx: &mut App) {
    register_serializable_item::<MediaPanel>(cx);
    cx.observe_new(move |workspace: &mut Workspace, window, _cx| {
        let Some(_window) = window else {
            return;
        };
        // Per the `.rules` trap "Center-pane Item Toggle vs ToggleFocus", the
        // View menu entry uses `Toggle` (deploys a new item if none exists),
        // not `ToggleFocus` (silent no-op when absent).
        workspace
            .register_action(move |workspace, _: &Toggle, window, cx| {
                let existing = workspace
                    .active_pane()
                    .read(cx)
                    .items()
                    .find_map(|item| item.downcast::<MediaPanel>());

                if let Some(existing) = existing {
                    workspace.activate_item(&existing, true, true, window, cx);
                } else {
                    let panel = MediaPanel::new(workspace, window, cx);
                    workspace.add_item_to_active_pane(
                        Box::new(panel.clone()),
                        None,
                        true,
                        window,
                        cx,
                    );
                    panel.focus_handle(cx).focus(window, cx);
                }
            })
            .register_action(move |workspace, _: &ToggleFocus, window, cx| {
                let existing = workspace
                    .active_pane()
                    .read(cx)
                    .items()
                    .find_map(|item| item.downcast::<MediaPanel>());
                if let Some(existing) = existing {
                    workspace.activate_item(&existing, true, true, window, cx);
                }
            });
    })
    .detach();
}

/// The Media panel: a tabbed viewing pane above a Steer director pane,
/// split by a draggable horizontal divider.
///
/// The director (bottom) is the scoped curator conversation — all media
/// operations (generate, search, organize, transform, transcribe) are
/// driven through chat. The viewing pane (top) surfaces what the tools
/// produced: assets are extracted structurally from tool-result
/// `display_hint` fields (T-V2), so the viewer updates on every tool
/// result regardless of whether the model echoes fenced blocks.
pub struct MediaPanel {
    focus_handle: FocusHandle,
    steer: hkask_steer::SteerSurface,
    project: Entity<project::Project>,
    fs: std::sync::Arc<dyn fs::Fs>,
    workspace_handle: WeakEntity<Workspace>,
    /// The tabbed viewing pane (Media / Library).
    viewer: Entity<MediaViewer>,
    /// The observed conversation thread + the observation subscription.
    /// Wired lazily in `render` once the Steer conversation exists.
    thread_observation: Option<gpui::Subscription>,
    /// The "Open Thread" picker — resumes a database thread in this panel's
    /// Steer surface.
    thread_picker: Entity<hkask_steer::ThreadPicker>,
    /// The session id to resume on the next `ensure_steer` — set by
    /// `open_thread`, consumed by `ensure_steer`.
    pending_resume: Option<agent_client_protocol::schema::v1::SessionId>,
    /// The steer pane's share of the panel height (the bottom of the
    /// split). Drag the divider to change it; double-click the divider to
    /// reset it. In-memory only — the split is not serialized.
    steer_split_fraction: f32,
}

impl MediaPanel {
    pub fn new(
        workspace: &Workspace,
        _window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        let workspace_handle = workspace.weak_handle();
        let project = workspace.project().clone();
        let fs = workspace.app_state().fs.clone();
        cx.new(|cx| {
            let panel_handle: gpui::WeakEntity<MediaPanel> = cx.weak_entity();
            let thread_picker = cx.new(|cx| {
                hkask_steer::ThreadPicker::new(
                    std::rc::Rc::new(move |session_id, window, cx: &mut gpui::App| {
                        panel_handle
                            .update(cx, |panel, cx| panel.open_thread(session_id, window, cx))
                            .log_err();
                    }),
                    cx,
                )
            });
            Self {
                focus_handle: cx.focus_handle(),
                steer: hkask_steer::SteerSurface::new(),
                project,
                fs,
                workspace_handle: workspace_handle.clone(),
                viewer: cx.new(|_| MediaViewer::new()),
                thread_observation: None,
                thread_picker,
                pending_resume: None,
                steer_split_fraction: DEFAULT_STEER_FRACTION,
            }
        })
    }

    /// Resume a database thread in this panel's Steer surface, replacing the
    /// live conversation. The viewer's thread observation is dropped so the
    /// next render re-wires it against the resumed conversation's thread.
    fn open_thread(
        &mut self,
        session_id: agent_client_protocol::schema::v1::SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_resume = Some(session_id);
        self.steer.invalidate();
        self.thread_observation = None;
        self.ensure_steer(window, cx);
        cx.notify();
    }

    /// Apply a divider drag to the split. `event.bounds` is the panel root
    /// (the element carrying `on_drag_move`) — the reference frame the
    /// fraction is taken against.
    fn update_split_from_drag(&mut self, event: &DragMoveEvent<DraggedSplitHandle>) {
        if let Some(fraction) = steer_fraction_from_drag(event.event.position.y, event.bounds) {
            self.steer_split_fraction = fraction;
        }
    }

    /// The split divider: a 1px rule with an invisible grab area. Drag to
    /// resize the panes; double-click to reset the split. The same handle
    /// pattern as the git graph's split (`git_ui/src/git_graph.rs`).
    fn render_split_handle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("media-panel-split-divider")
            .relative()
            .w_full()
            .flex_shrink_0()
            .h(px(1.))
            .bg(cx.theme().colors().border_variant)
            .child(
                div()
                    .id("media-panel-split-handle")
                    .absolute()
                    .top(px(-SPLIT_HANDLE_HIT_HEIGHT / 2.0))
                    .h(px(SPLIT_HANDLE_HIT_HEIGHT))
                    .w_full()
                    .cursor_row_resize()
                    .block_mouse_except_scroll()
                    .on_click(cx.listener(|this, event: &ClickEvent, _window, cx| {
                        if event.click_count() >= 2 {
                            this.steer_split_fraction = DEFAULT_STEER_FRACTION;
                            cx.notify();
                        }
                        cx.stop_propagation();
                    }))
                    .on_drag(DraggedSplitHandle, |_, _, _, cx| cx.new(|_| gpui::Empty)),
            )
    }

    /// Lazily construct the Steer `ConversationView`. Scoped to the media
    /// MCP server; verified against its generated `TOOL_NAMES`.
    fn ensure_steer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        hkask_steer::ensure_steer(
            &mut self.steer,
            hkask_steer::SteerContext {
                server_scope: MEDIA_SERVER.into(),
                system_prompt: steer_system_prompt(),
                fs: self.fs.clone(),
                project: self.project.clone(),
                workspace: self.workspace_handle.clone(),
                resume_session_id: self.pending_resume.take(),
            },
            hkask_mcp_media::TOOL_NAMES,
            &[
                "gallery_",
                "image_",
                "video_",
                "audio_",
                "face_",
                "generate_",
                "voice_",
                "transcribe",
                "record_",
                "educt_",
                "model_",
                "job_",
                "workflow_",
                "describe_",
                "expand_",
                "upscale_",
                "transform_",
            ],
            window,
            cx,
        );
    }
}

/// The Steer prompt. Text-only; verified against the server's generated
/// TOOL_NAMES by `verify_tool_advertisement` inside `ensure_steer`.
fn steer_system_prompt() -> SharedString {
    let prompt = "## Media Panel — Steer Mode\n\
         You are operating in the Media panel's Steer mode, scoped to the \
         `hkask-mcp-media` MCP server. All media operations are driven \
         through chat — there are no management forms in this panel.\n\
         \n\
         **Gallery tools**: `gallery_organize`, `gallery_status`, \
         `gallery_search`, `gallery_find_similar`, `gallery_refresh`, \
         `gallery_timeline`, `gallery_analyze`, `gallery_record_generation`, \
         `gallery_lineage`, `gallery_asset_detail`, `gallery_reproduce`, \
         `gallery_delete_image`, `gallery_add_video`, `gallery_add_audio`, \
         `gallery_list_assets`, \
         `gallery_create_album`, `gallery_list_albums`, \
         `gallery_move_to_album`, `gallery_remove_from_album`, \
         `gallery_delete_album`, `gallery_list_album_members`.\n\
         **Image tools**: `describe_image`, `image_remove_background`, \
         `image_apply_style`, `image_create_collage`, `image_edit_region`.\n\
         **Video tools**: `video_clip`, `video_to_gif`, `image_to_video`, \
         `video_add_caption`, `video_remix`, `video_concat`, \
         `video_from_images`, `video_caption`, `video_meme`, \
         `video_extract_frames`, `video_fetch`, `video_info`.\n\
         **Generation tools**: `generate_image`, `transform_image`, \
         `upscale_image`, `generate_video`, `generate_variants`, \
         `expand_prompt`.\n\
         **Voice tools**: `voice_design`, `generate_speech`.\n\
         **Audio tools**: `transcribe`, `transcribe_bundle`, \
         `audio_capture`, `record_and_transcribe`, `audio_trim`, \
         `audio_concat`.\n\
         **Transcript store tools**: `educt_store_transcript`, \
         `educt_list_transcripts`, `educt_get_transcript`, \
         `educt_delete_transcript`, `educt_store_layer`, \
         `educt_list_layers`.\n\
         **Face tools**: `face_validate`, `face_register`, \
         `face_scan_folder`, `face_list`, `face_remove`, \
         `gallery_name_face`.\n\
         **Model tools**: `model_info`, `model_list`.\n\
         **Job tools**: `job_submit`, `job_status`, `job_list`, `job_cancel`.\n\
         **Workflow tools**: `workflow_save`, `workflow_load`, \
         `workflow_list`, `workflow_delete`.\n\
         \n\
         Generated media (images, videos) renders inline in the conversation \
         via the media block renderer. Use `generate_image` for image \
         creation, `gallery_search` to find existing images, and \
         `gallery_organize` to manage the gallery structure.";
    prompt.into()
}

/// The steer pane's height fraction implied by a divider drag: the distance
/// from the pointer to the panel's bottom edge over the panel height,
/// clamped so neither pane starves. `None` on a zero-height panel — the
/// divide would produce a NaN fraction that poisons every later layout.
fn steer_fraction_from_drag(pointer_y: Pixels, panel: Bounds<Pixels>) -> Option<f32> {
    let panel_height = panel.bottom() - panel.top();
    if panel_height <= px(0.) {
        return None;
    }
    let steer_height = panel.bottom() - pointer_y;
    Some((steer_height / panel_height).clamp(MIN_STEER_FRACTION, MAX_STEER_FRACTION))
}

impl gpui::Render for MediaPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Lazily ensure the Steer surface the first time the panel draws —
        // `ensure_steer` needs `&mut Window`.
        self.ensure_steer(window, cx);

        // Wire the thread observation as soon as the conversation exists:
        // every thread update re-ingests tool-result display hints into the
        // viewer. This is the structural path — the viewer reflects what
        // the tools produced, not what the model echoed.
        if self.thread_observation.is_none()
            && let Some(conversation) = self.steer.conversation()
            && let Some(thread_view) = conversation.read(cx).active_thread()
        {
            let thread = thread_view.read(cx).thread.clone();
            let viewer = self.viewer.clone();
            self.thread_observation = Some(cx.observe(&thread, move |_, thread, cx| {
                viewer.update(cx, |viewer, cx| viewer.ingest_thread(thread.clone(), cx));
            }));
            // Ingest whatever the (possibly resumed) thread already holds.
            let thread_for_ingest = thread.clone();
            self.viewer
                .update(cx, |viewer, cx| viewer.ingest_thread(thread_for_ingest, cx));
        }

        let conversation = self.steer.conversation().cloned();
        // The director (bottom): the Steer conversation drives all media
        // operations through the scoped media MCP tools. Definite fractional
        // height — the viewer above flexes to fill whatever the director
        // leaves.
        let director = div()
            .w_full()
            .h(DefiniteLength::Fraction(self.steer_split_fraction))
            // The director is a flex-column child: without min_h_0 its
            // content's min-content height would override the dragged
            // fraction (the conversation would hold the pane open).
            .min_h_0()
            .flex()
            .flex_col()
            // The Open Thread affordance sits above the conversation so the
            // operator can resume a previous steer session at any time.
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().colors().border_variant)
                    .child(self.thread_picker.clone()),
            )
            .when_some(conversation, |el, conversation| el.child(conversation));

        v_flex()
            .size_full()
            // The drag surface for the split divider: the handle's `on_drag`
            // starts a `DraggedSplitHandle` drag, and this root receives
            // every move event for the drag's duration with `event.bounds`
            // = the whole panel — the reference frame the split math uses.
            .on_drag_move::<DraggedSplitHandle>(cx.listener(|this, event, _window, cx| {
                this.update_split_from_drag(event);
                cx.notify();
            }))
            // The viewing pane (top): what the tools produced, structurally.
            // flex_1 + min_h_0 — the pane is a flex-column child and must
            // shrink below its content's min-content height as the divider
            // drags (the vertical counterpart of the viewer's own min_h_0
            // tab content). min_w_0 keeps the horizontal-fit invariant:
            // without it the pane cannot shrink below its content's
            // min-content width, so a long untruncated header src or a wide
            // toolbar inflated the pane past the dock and the video rendered
            // clipped (the recurring horizontal-overflow bug — see
            // viewer_layout_tests::viewer_content_fits_narrow_pane).
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(self.viewer.clone()),
            )
            // The movable divider between the panes.
            .child(self.render_split_handle(cx))
            .child(director)
    }
}

impl EventEmitter<ItemEvent> for MediaPanel {}

impl Focusable for MediaPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Item for MediaPanel {
    type Event = ItemEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        "Media".into()
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<Icon> {
        Some(Icon::new(IconName::Image).color(Color::Muted))
    }

    fn show_toolbar(&self) -> bool {
        false
    }

    fn to_item_events(event: &Self::Event, function: &mut dyn FnMut(ItemEvent)) {
        function(*event)
    }
}

impl SerializableItem for MediaPanel {
    fn serialized_item_kind() -> &'static str {
        "MediaPanel"
    }

    fn cleanup(
        _workspace_id: workspace::WorkspaceId,
        _alive_items: Vec<workspace::ItemId>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<anyhow::Result<()>> {
        Task::ready(Ok(()))
    }

    fn serialize(
        &mut self,
        _workspace: &mut Workspace,
        _item_id: workspace::ItemId,
        _closing: bool,
        _cx: &mut Context<Self>,
    ) -> Option<Task<anyhow::Result<()>>> {
        None
    }

    fn should_serialize(&self, _event: &Self::Event) -> bool {
        false
    }

    fn deserialize(
        _project: Entity<project::Project>,
        workspace: WeakEntity<Workspace>,
        _workspace_id: workspace::WorkspaceId,
        _item_id: workspace::ItemId,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Entity<Self>>> {
        cx.spawn(async move |cx| {
            workspace.update_in(cx, |workspace, window, cx| {
                MediaPanel::new(workspace, window, cx)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every tool name the Steer prompt advertises in backticks must exist
    /// in the server's generated TOOL_NAMES — a rename fails here, not at
    /// dispatch.
    #[test]
    fn steer_prompt_advertises_only_known_tools() {
        let prompt = steer_system_prompt();
        for name in hkask_steer::advertised_tool_names(
            &prompt,
            &[
                "gallery_",
                "image_",
                "video_",
                "audio_",
                "face_",
                "generate_",
                "voice_",
                "transcribe",
                "record_",
                "educt_",
                "model_",
                "job_",
                "workflow_",
                "describe_",
                "expand_",
                "upscale_",
                "transform_",
            ],
        ) {
            assert!(
                hkask_mcp_media::TOOL_NAMES.contains(&name.as_str()),
                "steer prompt advertises `{name}`, not in hkask_mcp_media::TOOL_NAMES"
            );
        }
    }

    /// Every tool the server exposes should be advertised in the prompt — a
    /// missing name means the curator cannot discover it in Steer mode.
    #[test]
    fn server_tools_are_all_advertised() {
        let prompt = steer_system_prompt();
        for tool in hkask_mcp_media::TOOL_NAMES {
            assert!(
                prompt.contains(tool),
                "hkask_mcp_media::TOOL_NAMES lists `{tool}` but the Steer prompt never mentions it"
            );
        }
    }

    /// The divider drag math: the steer pane's height is the distance from
    /// the pointer to the panel's bottom, clamped so neither pane starves.
    /// Orientation is the likely bug — this pins top-vs-bottom.
    #[test]
    fn split_fraction_follows_pointer_and_clamps() {
        let panel = Bounds::new(gpui::point(px(0.), px(0.)), gpui::size(px(800.), px(600.)));
        // Pointer mid-panel → the default equal split.
        assert_eq!(steer_fraction_from_drag(px(300.), panel), Some(0.5));
        // Pointer at the bottom edge → the steer pane collapses to its floor.
        assert_eq!(
            steer_fraction_from_drag(px(600.), panel),
            Some(MIN_STEER_FRACTION)
        );
        // Pointer at the top edge → the steer pane takes its ceiling.
        assert_eq!(
            steer_fraction_from_drag(px(0.), panel),
            Some(MAX_STEER_FRACTION)
        );
        // Overshoots beyond either edge clamp, never extrapolate.
        assert_eq!(
            steer_fraction_from_drag(px(900.), panel),
            Some(MIN_STEER_FRACTION)
        );
        assert_eq!(
            steer_fraction_from_drag(px(-50.), panel),
            Some(MAX_STEER_FRACTION)
        );
    }

    /// A zero-height panel must not produce a fraction — 0/0 is NaN, and a
    /// NaN fraction would poison every layout after it.
    #[test]
    fn split_fraction_guards_zero_height_panel() {
        let flat = Bounds::new(gpui::point(px(0.), px(0.)), gpui::size(px(800.), px(0.)));
        assert_eq!(steer_fraction_from_drag(px(0.), flat), None);
    }
}
