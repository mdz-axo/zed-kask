//! Media Panel — a Steer-only surface for the `hkask-mcp-media` MCP server.
//!
//! A cross-domain curator conversation supports ingesting, organizing,
//! annotating, and extracting existing research evidence. The viewer offers
//! two task modes — Library (indexed assets, recordings with their
//! transcripts) and the Asset workspace (the selected asset's player and
//! edit toolbar beside a toggleable inspector pane) — plus an ambient
//! activity layer: per-asset job badges and an on-demand drawer; chat drives
//! the server tools. The prompt highlights the server's generated
//! `TOOL_NAMES` without hiding other MCP tools. Source-linked media results
//! render inline (the D18 seam).

pub mod media_viewer;
#[cfg(test)]
mod test_fixtures;

use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, SharedString, Task, WeakEntity,
    Window, actions,
};
use ui::{Icon, IconName, prelude::*};
use util::ResultExt as _;
use workspace::{
    Workspace,
    item::{Item, ItemEvent, SerializableItem},
    register_serializable_item,
};

pub use media_viewer::MediaViewer;

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

/// The status bar button type that opens/focuses the Media panel.
pub type MediaPanelButton = hkask_panel_ui::PanelToggleButton<Toggle>;

/// Construct the Media panel status bar button — the panel's literals
/// over the shared `PanelToggleButton` implementation.
pub fn panel_button() -> MediaPanelButton {
    hkask_panel_ui::PanelToggleButton::new(
        "media-panel-button",
        IconName::Image,
        "Media",
        "Toggle Media",
        Toggle,
    )
}

/// Register the panel's actions on every new `Workspace`.
pub fn init(cx: &mut App) {
    register_serializable_item::<MediaPanel>(cx);
    hkask_panel_ui::register_center_panel_actions::<MediaPanel, Toggle, ToggleFocus>(
        cx,
        MediaPanel::new,
    );
}

/// The Media panel: a tabbed viewing pane above a Steer director pane,
/// split by a draggable horizontal divider.
///
/// The director (bottom) is a cross-domain curator conversation — media
/// operations (ingest, search, organize, annotate, transcribe, excerpt) are
/// driven through chat. The viewing pane (top) surfaces what the tools
/// produced: assets are extracted structurally from tool-result
/// `display_hint` fields, so the viewer updates on every tool
/// result regardless of whether the model echoes fenced blocks.
pub struct MediaPanel {
    focus_handle: FocusHandle,
    steer: hkask_steer::SteerSurface,
    project: Entity<project::Project>,
    fs: std::sync::Arc<dyn fs::Fs>,
    workspace_handle: WeakEntity<Workspace>,
    /// The viewing pane — the Library and Workspace modes plus the
    /// ambient activity drawer.
    viewer: Entity<MediaViewer>,
    /// The observed conversation thread + the observation subscription.
    /// Wired lazily in `render` once the Steer conversation exists.
    thread_observation: Option<gpui::Subscription>,
    /// The "Open Thread" picker — resumes a database thread in this panel's
    /// Steer surface.
    thread_picker: Entity<hkask_steer::ThreadPicker>,
    /// The steer pane's share of the panel height (the bottom of the
    /// split). Drag the divider to change it; double-click the divider to
    /// reset it. In-memory only — the split is not serialized.
    split: hkask_steer::VerticalSplitState,
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
                split: hkask_steer::VerticalSplitState::default(),
            }
        })
    }

    /// Resume a database thread in this panel's Steer surface, replacing the
    /// live conversation. The viewer's thread observation is dropped so the
    /// next render re-wires it against the resumed conversation's thread.
    fn open_thread(
        &mut self,
        session_id: agent_client_protocol::v2::schema::SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.steer.stage_resume(session_id);
        self.thread_observation = None;
        self.ensure_steer(window, cx);
        cx.notify();
    }

    /// Lazily construct the cross-domain Steer `ConversationView`. Media tool
    /// guidance is verified against the server's generated `TOOL_NAMES`.
    fn ensure_steer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        hkask_steer::ensure_steer(
            &mut self.steer,
            hkask_steer::SteerContext {
                system_prompt: steer_system_prompt(),
                fs: self.fs.clone(),
                project: self.project.clone(),
                workspace: self.workspace_handle.clone(),
            },
            hkask_media::wire::MEDIA_TOOL_NAMES,
            &[
                "gallery_",
                "image_",
                "video_",
                "audio_",
                "face_",
                "transcribe",
                "record_",
                "educt_",
                "model_",
                "job_",
                "describe_",
            ],
            window,
            cx,
        );
    }
}

/// The Steer prompt. Behavioral prose is written by hand; the tool list is
/// rendered from the server's generated `TOOL_NAMES` by
/// `render_grouped_tool_advertisement`, so a rename/merge/addition in the
/// server propagates here at the next build instead of degrading to
/// "tool not found" at dispatch. `verify_tool_advertisement` inside
/// `ensure_steer` still checks the prose's tool mentions.
fn steer_system_prompt() -> SharedString {
    // Labels and prefixes only — never tool names. First matching group wins,
    // so `kanban`-style overlaps resolve by declaration order; tools matching
    // no group land in "Other tools" and are still advertised.
    let tool_section = hkask_steer::render_grouped_tool_advertisement(
        hkask_media::wire::MEDIA_TOOL_NAMES,
        &[
            ("Gallery tools", &["gallery_"]),
            ("Face tools", &["face_"]),
            ("Image tools", &["image_", "describe_", "media_fetch"]),
            ("Video tools", &["video_"]),
            ("Audio tools", &["audio_", "transcribe", "record_"]),
            ("Transcript store tools (local)", &["educt_"]),
            ("Media layer tools", &["layers_"]),
            ("Model tools", &["model_"]),
            ("Job tools", &["job_"]),
        ],
    );
    let prompt = format!(
        "## Media Panel — Steer Mode\n\
         You are operating in the Media panel's cross-domain Steer mode. The \
         full MCP tool surface remains available. Use media tools to ingest, \
         organize, annotate, search, transcribe, and extract existing research evidence.\n\
         \n\
         {tool_section}\
         \n\
         Preserve source bytes and stable asset identities. Keep user-authored \
         annotations distinct from model-derived observations; neither is verified \
         source evidence by itself. Link excerpts and reels to their source and \
         transcript ranges, and retain historical provenance. Use gallery_annotation_ \
         tools with stable gallery_asset_id, never positional indices, for researcher \
         tags/notes; educt_annotation_ tools use immutable source-word ranges. \
         Search stored transcripts with educt_search_transcripts in lexical mode; \
         follow returned continuation offsets and retain source/timing citations. \
         A lexical match is not semantic retrieval or verified evidence. Use face identification \
         only when explicitly selected by the operator. Do not synthesize \
         research evidence. Media results render inline; the viewer's two \
         surfaces are the Library mode (indexed assets, recordings with their \
         transcripts) and the Asset workspace (the selected asset's player and \
         edit toolbar beside a toggleable inspector pane), and job state reaches \
         the operator as ambient activity — badges on affected assets and an \
         on-demand drawer, never a navigation destination."
    );
    prompt.into()
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
        if self.thread_observation.is_none() {
            let viewer = self.viewer.clone();
            self.thread_observation = self.steer.observe_active_thread(cx, move |thread, cx| {
                viewer.update(cx, |viewer, cx| viewer.ingest_thread(thread, cx));
            });
        }

        hkask_steer::render_vertical_split(
            "media-panel",
            self.split.bottom_fraction(),
            self.viewer.clone(),
            self.thread_picker.clone(),
            self.steer.conversation().cloned(),
            |this| &mut this.split,
            cx,
        )
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

    /// D98-PIN (batch-9 audit): the serializable-item registration is what
    /// restores the panel into its dock position after a workspace reload —
    /// the batch-9 audit found the registration unpinned (the DIVERGENCE row
    /// had named the production trait method as if it were a pin). Source
    /// pin on the registration call plus the kind identity, in the alacritty
    /// resize-log precedent style: deleting the registration fails here.
    #[test]
    fn serializable_item_registration_is_pinned() {
        assert_eq!(
            MediaPanel::serialized_item_kind(),
            "MediaPanel",
            "the serialized kind string is the dock persistence identity"
        );
        let source = include_str!("media_panel.rs");
        assert!(
            source.contains("register_serializable_item::<MediaPanel>(cx)"),
            "media_panel::init must register the serializable item — dock persistence depends on it"
        );
    }

    /// Every tool name the Steer prompt advertises in backticks must exist
    /// in the server's generated TOOL_NAMES. The list itself is rendered
    /// from TOOL_NAMES (`render_grouped_tool_advertisement`), so this passes
    /// by construction — it exists to catch a regression back to a
    /// hand-written list, which is how the 2026-09-03 media consolidation
    /// shipped stale names.
    #[test]
    fn steer_prompt_advertises_only_known_tools() {
        let prompt = steer_system_prompt();
        for name in hkask_steer::advertised_tool_names(
            &prompt,
            &[
                "gallery_",
                "image_",
                "media_fetch",
                "video_",
                "audio_",
                "face_",
                "transcribe",
                "record_",
                "educt_",
                "model_",
                "job_",
                "describe_",
                "voice_",
            ],
        ) {
            assert!(
                hkask_media::wire::MEDIA_TOOL_NAMES.contains(&name.as_str()),
                "steer prompt advertises `{name}`, not in hkask_media::wire::MEDIA_TOOL_NAMES"
            );
        }
    }

    /// expect: [P7] The approved research surface excludes creative operations
    /// while retaining source processing, annotations, retrieval, and history.
    #[test]
    fn steer_prompt_carries_research_scope_and_boundaries() {
        assert_eq!(hkask_media::wire::MEDIA_TOOL_NAMES.len(), 91);
        let prompt = steer_system_prompt();
        for required in [
            "gallery_annotation_create",
            "gallery_annotation_list",
            "gallery_annotation_update",
            "gallery_annotation_delete",
            "educt_annotation_create",
            "educt_annotation_list",
            "educt_annotation_update",
            "educt_annotation_delete",
            "educt_search_transcripts",
            "gallery_lineage",
            "gallery_asset_detail",
            "transcribe_bundle",
            "audio_trim",
            "audio_concat",
            "video_clip",
            "video_concat",
        ] {
            assert!(
                prompt.contains(required),
                "research prompt missing {required}"
            );
        }
        for retired in [
            "voice_design",
            "generate_speech",
            "gallery_record_generation",
            "gallery_reproduce",
            "generate_image",
            "transform_image",
            "upscale_image",
            "generate_video",
            "expand_prompt",
            "image_edit_region",
            "image_remove_background",
            "image_apply_style",
            "image_to_video",
            "video_remix",
            "video_from_images",
            "video_meme",
            "workflow_save",
            "workflow_list",
            "workflow_load",
            "workflow_delete",
        ] {
            assert!(
                !prompt.contains(retired),
                "retired operation advertised: {retired}"
            );
        }
        for boundary in [
            "full MCP tool surface remains available",
            "Preserve source bytes",
            "immutable source-word ranges",
            "lexical mode",
            "source/timing citations",
            "neither is verified",
            "explicitly selected by the operator",
            "historical provenance",
        ] {
            assert!(prompt.contains(boundary), "missing boundary: {boundary}");
        }
    }

    /// Every tool the server exposes should be advertised in the prompt — a
    /// missing name means the curator cannot discover it in Steer mode.
    /// Generated by construction; pins the renderer's completeness.
    #[test]
    fn server_tools_are_all_advertised() {
        let prompt = steer_system_prompt();
        for tool in hkask_media::wire::MEDIA_TOOL_NAMES {
            assert!(
                prompt.contains(tool),
                "hkask_media::wire::MEDIA_TOOL_NAMES lists `{tool}` but the Steer prompt never mentions it"
            );
        }
    }
}
