use std::fmt;
use std::time::{Duration, Instant};

use gpui::{BenchAppContext, Context, Entity, IntoElement, Render, Task, Window};
use hkask_media_widget::{MediaWidget, PlaybackBenchmarkSnapshot};
use ui::prelude::*;

const PLAYBACK_TIMEOUT: Duration = Duration::from_secs(5);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(8);
const MIN_PRESENTED_FRAMES_AT_30_FPS: u64 = 89;
const SOURCE_FRAME_COUNT: u64 = 90;

#[derive(Clone, Copy, Debug)]
enum PlaybackMode {
    /// Visible players must reach completed playback.
    Visible,
    /// Loaded players are removed from the render tree after playback starts;
    /// they must reach suspended/no-polling rather than completing offscreen.
    Cached,
}

#[derive(Clone, Copy, Debug)]
struct PlaybackInput {
    mode: PlaybackMode,
    videos: usize,
}

impl fmt::Display for PlaybackInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mode = match self.mode {
            PlaybackMode::Visible => "visible",
            PlaybackMode::Cached => "cached",
        };
        write!(formatter, "{mode}-{}", self.videos)
    }
}

fn playback_inputs() -> [PlaybackInput; 6] {
    [
        PlaybackInput {
            mode: PlaybackMode::Visible,
            videos: 1,
        },
        PlaybackInput {
            mode: PlaybackMode::Visible,
            videos: 8,
        },
        PlaybackInput {
            mode: PlaybackMode::Visible,
            videos: 32,
        },
        PlaybackInput {
            mode: PlaybackMode::Cached,
            videos: 1,
        },
        PlaybackInput {
            mode: PlaybackMode::Cached,
            videos: 8,
        },
        PlaybackInput {
            mode: PlaybackMode::Cached,
            videos: 32,
        },
    ]
}

#[gpui::bench(
    fps = 120,
    inputs = playback_inputs(),
    input_name = "players",
    group = "Media playback",
    sample_size = 10
)]
fn media_playback(input: &PlaybackInput, cx: &mut BenchAppContext) {
    init_context(cx);
    cx.bench_batched_task(
        |cx| PlaybackFixture::prepare(*input, cx),
        |fixture, cx| fixture.run(cx),
    );
}

struct PlaybackFixture {
    mode: PlaybackMode,
    root: Entity<PlaybackBenchView>,
    widgets: Vec<Entity<MediaWidget>>,
}

impl PlaybackFixture {
    fn prepare(input: PlaybackInput, cx: &mut BenchAppContext) -> Self {
        hkask_viz_core::clear_widget_cache();
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../hkask-media-widget/test_data/playback-30fps.mp4");
        assert!(
            fixture_path.exists(),
            "playback benchmark fixture is missing"
        );
        hkask_media_widget::approve_gallery_media_path(&fixture_path)
            .expect("benchmark fixture receives gallery-result path authority");

        let mut window = cx.add_empty_window();
        let (root, widgets) = window.update(|window, cx| {
            let renderer = hkask_viz_core::block_renderer();
            let mut widgets = Vec::with_capacity(input.videos);
            for instance in 0..input.videos {
                let body = format!(
                    r#"{{"kind":"video","src":{},"benchmark_instance":{instance}}}"#,
                    serde_json::to_string(&fixture_path.to_string_lossy())
                        .expect("fixture path serializes")
                );
                assert!(
                    renderer(&body, window, cx).is_some(),
                    "production block renderer accepts the media body"
                );
                let widget = hkask_viz_core::shared_media_widget(&body, window, cx)
                    .expect("cached media widget exists");
                widgets.push(widget);
            }

            let root = window.replace_root(cx, |_window, _cx| PlaybackBenchView {
                widgets: widgets.clone(),
                progress_tick: 0,
            });
            (root, widgets)
        });

        wait_until_ready(&widgets, cx);
        Self {
            mode: input.mode,
            root,
            widgets,
        }
    }

    fn run(
        &mut self,
        cx: &mut BenchAppContext,
    ) -> Task<anyhow::Result<Vec<PlaybackBenchmarkSnapshot>>> {
        for widget in &self.widgets {
            widget.update(cx, |widget, cx| widget.benchmark_start_playback(cx));
        }
        if matches!(self.mode, PlaybackMode::Cached) {
            self.root.update(cx, |view, cx| {
                view.widgets.clear();
                cx.notify();
            });
        }

        let mode = self.mode;
        let widgets = self.widgets.clone();
        self.root.update(cx, |_, cx| {
            cx.spawn(async move |root, cx| {
                let deadline = Instant::now() + PLAYBACK_TIMEOUT;
                loop {
                    cx.background_executor().timer(PROGRESS_INTERVAL).await;
                    root.update(cx, |view, cx| {
                        view.progress_tick = view.progress_tick.wrapping_add(1);
                        cx.notify();
                    })?;

                    let snapshots = widgets
                        .iter()
                        .map(|widget| {
                            widget.read_with(cx, |widget, _cx| widget.benchmark_snapshot())
                        })
                        .collect::<Option<Vec<_>>>();
                    if let Some(snapshots) = snapshots {
                        let reached_terminal = match mode {
                            PlaybackMode::Visible => snapshots.iter().all(|snapshot| {
                                snapshot.state == hkask_media_widget::PlaybackState::Finished
                            }),
                            PlaybackMode::Cached => snapshots
                                .iter()
                                .all(|snapshot| snapshot.suspended && !snapshot.polling),
                        };
                        if reached_terminal {
                            match mode {
                                PlaybackMode::Visible => {
                                    validate_completion(&snapshots, widgets.len())
                                }
                                PlaybackMode::Cached => validate_suspension(&snapshots),
                            }
                            return Ok(snapshots);
                        }
                    }
                    if Instant::now() >= deadline {
                        anyhow::bail!(
                            "media playback did not complete within {PLAYBACK_TIMEOUT:?}"
                        );
                    }
                }
            })
        })
    }
}

struct PlaybackBenchView {
    widgets: Vec<Entity<MediaWidget>>,
    progress_tick: u64,
}

impl Render for PlaybackBenchView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_wrap()
            .gap_1()
            .children(self.widgets.iter().cloned())
            .child(
                div()
                    .id("playback-progress")
                    .child(self.progress_tick.to_string()),
            )
    }
}

fn init_context(cx: &mut BenchAppContext) {
    cx.update(|cx| {
        if !cx.has_global::<settings::SettingsStore>() {
            settings::init(cx);
        }
        if !cx.has_global::<theme::GlobalTheme>() {
            theme_settings::init(theme::LoadThemes::JustBase, cx);
        }
    });
}

fn wait_until_ready(widgets: &[Entity<MediaWidget>], cx: &mut BenchAppContext) {
    let deadline = Instant::now() + PLAYBACK_TIMEOUT;
    loop {
        cx.run_until_idle();
        let ready = widgets.iter().all(|widget| {
            widget.read_with(cx, |widget, _cx| {
                widget
                    .benchmark_snapshot()
                    .is_some_and(|snapshot| snapshot.has_frame && snapshot.error.is_none())
            })
        });
        if ready {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "media widgets did not become ready within {PLAYBACK_TIMEOUT:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
        cx.update(|_| ());
    }
}

fn validate_completion(snapshots: &[PlaybackBenchmarkSnapshot], player_count: usize) {
    for snapshot in snapshots {
        assert!(
            snapshot.error.is_none(),
            "playback failed: {:?}",
            snapshot.error
        );
        assert!(
            snapshot.has_frame,
            "completed playback retains its final frame"
        );
        assert_eq!(snapshot.delivery.max_pending_frames, 1);
        assert!(snapshot.delivery.pending_frames <= 1);
        assert_eq!(snapshot.delivery.out_of_order_frames, 0);
        let final_pts_ms = snapshot
            .delivery
            .last_consumed_pts_ms
            .expect("completed playback presents a final frame");
        assert!(
            final_pts_ms.saturating_add(50) >= snapshot.duration.as_millis() as u64,
            "the final source frame must reach GPUI: {snapshot:?}"
        );
        if player_count == 1 {
            static REPORT_ONCE: std::sync::Once = std::sync::Once::new();
            REPORT_ONCE.call_once(|| {
                eprintln!(
                    "30fps delivery: presented={}/{} published={} replaced={}",
                    snapshot.delivery.consumed_frames,
                    SOURCE_FRAME_COUNT,
                    snapshot.delivery.published_frames,
                    snapshot.delivery.replaced_frames,
                );
            });
            assert!(
                snapshot.delivery.consumed_frames >= MIN_PRESENTED_FRAMES_AT_30_FPS,
                "single visible 30fps playback must present at least 89/90 source frames: {snapshot:?}"
            );
        }
        assert!(snapshot.delivery.consumed_frames <= SOURCE_FRAME_COUNT);
        assert!(snapshot.position >= snapshot.duration);
    }
    std::hint::black_box(snapshots);
}

fn validate_suspension(snapshots: &[PlaybackBenchmarkSnapshot]) {
    for snapshot in snapshots {
        assert!(
            snapshot.error.is_none(),
            "hidden playback failed before suspension: {:?}",
            snapshot.error
        );
        assert!(snapshot.suspended, "hidden media must be suspended");
        assert!(!snapshot.polling, "hidden media must own no polling task");
        assert_ne!(
            snapshot.state,
            hkask_media_widget::PlaybackState::Playing,
            "hidden media must not continue decode/audio playback"
        );
        assert_eq!(snapshot.delivery.max_pending_frames, 1);
        assert!(snapshot.delivery.pending_frames <= 1);
    }
    std::hint::black_box(snapshots);
}

gpui::bench_group!(benches, media_playback);
gpui::bench_main!(benches);
