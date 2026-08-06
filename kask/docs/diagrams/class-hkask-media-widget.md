---
title: "hKask Media Widget — Class Diagram"
audience: [architects, developers]
last_updated: 2026-08-04
version: "1.0.0"
status: "Active"
domain: "Composition"
mds_categories: [composition]
---

# hKask Media Widget — Class Diagram

`hkask-media-widget` renders ```` ```media ```` fenced blocks (image / SVG /
audio / video) inline in agent markdown. It resolves the asset reference via
`MediaStorage` (default `PathMediaStorage`, which handles filesystem paths,
`data:` URIs, and `http(s)` URLs directly — gallery images arrive as filesystem
paths) and drives playback through the `audio_player` / `video_decoder` /
`transport` collaborators.

```mermaid
classDiagram
    class MediaKind {
        <<enum>>
        Image
        Svg
        Audio
        Video
    }
    class MediaRef {
        <<enum>>
        Asset
        Error
        +new(src, kind) MediaRef
        +src() str
        +kind() Option~MediaKind~
        +is_error() bool
    }
    class ResolvedMedia {
        +kind: MediaKind
        +path: Option~PathBuf~
        +bytes: Option~Vec~u8~~
        +url: Option~SharedString~
    }
    class MediaStorage {
        <<interface>>
        +resolve(reference) Result~ResolvedMedia~
    }
    class PathMediaStorage {
        +resolve(reference) Result~ResolvedMedia~
    }
    class MediaBlockBody {
        +kind: String
        +src: String
        +ontology: Option~String~
        +provenance: BlockProvenance
        +parse(body) Result~MediaBlockBody~
        +to_media_ref() Result~MediaRef~
    }
    class MediaWidget {
        +reference: MediaRef
        +storage: MediaStorage
        +focus_handle: FocusHandle
        +audio_player
        +video_player
        +transport
        +current_frame
        +playback_task
        +error: Option~String~
        +ontology: Option~String~
        +provenance: BlockProvenance
        +disagree_draft: Option~String~
        +explain_result: Option~String~
        +explain_error: Option~String~
        +new(reference, cx) MediaWidget
        +new_with_block(reference, block, cx) MediaWidget
        +with_storage(storage) MediaWidget
        +load()
        +on_explain_click(cx)
        +on_disagree_click(window, cx)
        +compose_disagree_body() String
    }
    class create_media_widget {
        +create_media_widget(body, window, cx) Option~MediaWidget~
    }
    class explain_tool_for {
        +explain_tool_for(ontology) str
    }

    MediaRef --> MediaKind
    ResolvedMedia --> MediaKind
    PathMediaStorage ..|> MediaStorage : implements
    MediaBlockBody --> MediaRef : to_media_ref
    MediaWidget --> MediaRef
    MediaWidget --> MediaStorage
    MediaWidget --> MediaBlockBody : parses
    MediaWidget ..|> gpui_Focusable : Focusable
    MediaWidget ..|> gpui_Render : Render
    MediaWidget ..|> EventEmitter : emits TransportEvent
    MediaWidget ..> explain_tool_for : I pattern dispatch
    create_media_widget ..> MediaBlockBody : parses
    create_media_widget ..> MediaWidget : creates
```

**Block shape:** a JSON body with a `kind` (`"image"|"svg"|"audio"|"video"`),
a `src` (filesystem path / `data:` URI / URL), an optional `ontology` concept
URI (e.g. `omc:CreativeWork`), and optional `provenance`. The media renderer
self-selects on the `kind` field and is tried first by `hkask-viz-core` so
`kind`-bearing bodies never reach the graph/kanban/portfolio/scenarios
renderers.

**Ontology-bounded affordances (the "I" pattern):** when the block carries
`ontology` + dispatchable `provenance`, the widget renders two affordances:
- **Explain** — dispatches the explain tool selected by
  `hkask_bridge_ontology::explain_tool_for(ontology)` via `shared_tool_invoker()`.
  `omc:Scene`/`omc:Asset` → `gallery_analyze`; other OMC concepts → `describe_image`.
- **I disagree** — composes a provenance-scoped revision request and injects it
  via `shared_injector()` (D21 seam). Falls back to a copyable draft when no
  injector is active.

**Video decode** is gated behind the `video` / `vendored` cargo features
(system FFmpeg or vendored compile); without them, video blocks return an
error message instead of decoding.

<!-- DIAGRAM_ALIGNMENT
id: DIAG-VIZ-MEDIA
verified_date: 2026-08-04
verified_against: crates/hkask-media-widget/src/media_ref.rs; crates/hkask-media-widget/src/media_widget.rs; crates/hkask-media-widget/src/audio_player.rs; crates/hkask-media-widget/src/transport.rs; crates/hkask-media-widget/src/video_decoder.rs
status: VERIFIED
-->