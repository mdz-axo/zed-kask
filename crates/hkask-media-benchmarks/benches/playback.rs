use std::fmt;
use std::time::{Duration, Instant};

use gpui::{AppContext as _, BenchAppContext, Context, Entity, IntoElement, Render, Task, Window};
use hkask_media_widget::{MediaWidget, PlaybackBenchmarkSnapshot};
use ui::prelude::*;

const PLAYBACK_TIMEOUT: Duration = Duration::from_secs(5);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(8);
const EXPECTED_FINAL_PTS_MS: u64 = 500;

#[derive(Clone, Copy, Debug)]
enum PlaybackMode {
    Visible,
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
    root: Entity<PlaybackBenchView>,
    widgets: Vec<Entity<MediaWidget>>,
}

impl PlaybackFixture {
    fn prepare(input: PlaybackInput, cx: &mut BenchAppContext) -> Self {
        hkask_viz_core::clear_widget_cache();
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../hkask-media-widget/test_data/playback-lifecycle.mp4");
        assert!(
            fixture_path.exists(),
            "playback benchmark fixture is missing"
        );

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

            let visible_widgets = match input.mode {
                PlaybackMode::Visible => widgets.clone(),
                PlaybackMode::Cached => Vec::new(),
            };
            let root = window.replace_root(cx, |_window, _cx| PlaybackBenchView {
                widgets: visible_widgets,
                progress_tick: 0,
            });
            (root, widgets)
        });

        wait_until_ready(&widgets, cx);
        Self { root, widgets }
    }

    fn run(
        &mut self,
        cx: &mut BenchAppContext,
    ) -> Task<anyhow::Result<Vec<PlaybackBenchmarkSnapshot>>> {
        for widget in &self.widgets {
            widget.update(cx, |widget, cx| widget.benchmark_start_playback(cx));
        }

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
                        .collect::<Result<Vec<_>, _>>()?
                        .into_iter()
                        .collect::<Option<Vec<_>>>();
                    if let Some(snapshots) = snapshots
                        && snapshots.iter().all(|snapshot| {
                            snapshot.state
                                == hkask_media_widget::video_decoder::PlaybackState::Finished
                        })
                    {
                        validate_completion(&snapshots);
                        return Ok(snapshots);
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

fn validate_completion(snapshots: &[PlaybackBenchmarkSnapshot]) {
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
        assert_eq!(
            snapshot.delivery.last_consumed_pts_ms,
            Some(EXPECTED_FINAL_PTS_MS),
            "the final source frame must reach GPUI"
        );
        assert!(snapshot.delivery.consumed_frames <= 6);
        assert!(snapshot.delivery.consumed_frames >= 2);
        assert!(snapshot.position >= snapshot.duration);
    }
    std::hint::black_box(snapshots);
}
