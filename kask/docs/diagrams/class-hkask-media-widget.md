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
        <<enumeration>>
        Image
        Svg
        Audio
        Video
    }
    class MediaRef {
        <<enum>>
        +Asset{src: SharedString, kind: MediaKind}
        +Error(SharedString)
        +new(src, kind) MediaRef
        +src() &str
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
        +resolve(reference: &MediaRef) Result~ResolvedMedia~
    }
    class PathMediaStorage {
        +resolve(reference) Result~ResolvedMedia~
    }
    class MediaWidget {
        +reference: MediaRef
        +storage: dyn MediaStorage
        +focus_handle: FocusHandle
        +audio_player
        +video_player
        +transport
        +current_frame
        +playback_task
        +error: Option~String~
        +_subscriptions
        +new(reference, cx) MediaWidget
        +with_storage(storage) MediaWidget
        +load()
    }
    class create_media_widget {
        +create_media_widget(body, window, cx) Option~Entity~MediaWidget~~
    }

    MediaRef --> MediaKind
    ResolvedMedia --> MediaKind
    PathMediaStorage ..|> MediaStorage : implements
    MediaWidget --> MediaRef
    MediaWidget --> MediaStorage
    MediaWidget ..|> gpui_Focusable [Focusable]
    MediaWidget ..|> gpui_Render [Render]
    MediaWidget ..|> EventEmitter [EventEmitter~TransportEvent~]
    create_media_widget ..> MediaWidget : parses JSON {kind, src}
```

**Block shape:** a JSON body with a `kind` (`"image"|"svg"|"audio"|"video"`)
and a `src` (filesystem path / `data:` URI / URL). The media renderer
self-selects on the `kind` field and is tried first by `hkask-viz-core` so
`kind`-bearing bodies never reach the graph/kanban/portfolio/scenarios
renderers.

**Video decode** is gated behind the `video` / `vendored` cargo features
(system FFmpeg or vendored compile); without them, video blocks return an
error message instead of decoding.

<!-- DIAGRAM_ALIGNMENT
id: DIAG-VIZ-MEDIA
verified_date: 2026-08-03
verified_against: crates/hkask-media-widget/src/media_ref.rs; crates/hkask-media-widget/src/media_widget.rs; crates/hkask-media-widget/src/audio_player.rs; crates/hkask-media-widget/src/transport.rs; crates/hkask-media-widget/src/video_decoder.rs
status: VERIFIED 2026-08-03
-->