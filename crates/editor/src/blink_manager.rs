use gpui::{Context, Task};
use settings::SettingsStore;
use std::time::Duration;
use ui::App;

pub struct BlinkManager {
    blink_interval: Duration,
    blink_epoch: usize,
    /// Whether the blinking is paused.
    blinking_paused: bool,
    /// Whether the cursor should be visibly rendered or not.
    visible: bool,
    /// Whether the blinking is currently enabled.
    enabled: bool,
    /// Whether the blinking is enabled in the settings.
    blink_enabled_in_settings: fn(&App) -> bool,
    // zed-kask: replacing this task cancels the previous idle deadline, so rapid
    // selection changes keep exactly one resume callback alive. See DIVERGENCE.md D15.
    resume_task: Option<Task<()>>,
}

impl BlinkManager {
    pub fn new(
        blink_interval: Duration,
        blink_enabled_in_settings: fn(&App) -> bool,
        cx: &mut Context<Self>,
    ) -> Self {
        // zed-kask: only (re)start blinking when the setting transitions to enabled.
        // Upstream calls `blink_cursors(this.blink_epoch, cx)` on every SettingsStore
        // update. `blink_cursors` passes the epoch guard (epoch == self.blink_epoch
        // is trivially true for the current epoch) and spawns a fresh 500ms timer on
        // top of any already running. Each unrelated settings change (profile
        // switch, model selection, favorite toggle, settings.json save) therefore
        // accumulates a new overlapping blink cycle. After N settings changes the
        // cursor strobes at (N+1)x the normal rate — the "frenetic, omnipresent"
        // symptom. See DIVERGENCE.md D15.
        cx.observe_global::<SettingsStore>(move |this, cx| {
            let now_enabled = (this.blink_enabled_in_settings)(cx);
            if now_enabled && !this.enabled {
                this.enable(cx);
            } else if !now_enabled && this.enabled {
                this.disable(cx);
            }
        })
        .detach();

        Self {
            blink_interval,
            blink_epoch: 0,
            blinking_paused: false,
            visible: true,
            enabled: false,
            blink_enabled_in_settings,
            resume_task: None,
        }
    }

    fn next_blink_epoch(&mut self) -> usize {
        self.blink_epoch += 1;
        self.blink_epoch
    }

    pub fn pause_blinking(&mut self, cx: &mut Context<Self>) {
        self.show_cursor(cx);
        self.blinking_paused = true;

        let epoch = self.next_blink_epoch();
        let interval = self.blink_interval;
        self.resume_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(interval).await;
            this.update(cx, |this, cx| this.resume_cursor_blinking(epoch, cx))
                .ok();
        }));
    }

    fn resume_cursor_blinking(&mut self, epoch: usize, cx: &mut Context<Self>) {
        if epoch == self.blink_epoch {
            self.resume_task = None;
            self.blinking_paused = false;
            self.blink_cursors(epoch, cx);
        }
    }

    fn blink_cursors(&mut self, epoch: usize, cx: &mut Context<Self>) {
        if (self.blink_enabled_in_settings)(cx) {
            if epoch == self.blink_epoch && self.enabled && !self.blinking_paused {
                self.visible = !self.visible;
                cx.notify();

                let epoch = self.next_blink_epoch();
                let interval = self.blink_interval;
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(interval).await;
                    if let Some(this) = this.upgrade() {
                        this.update(cx, |this, cx| this.blink_cursors(epoch, cx));
                    }
                })
                .detach();
            }
        } else {
            self.show_cursor(cx);
        }
    }

    pub fn show_cursor(&mut self, cx: &mut Context<BlinkManager>) {
        if !self.visible {
            self.visible = true;
            cx.notify();
        }
    }

    /// Enable the blinking of the cursor.
    pub fn enable(&mut self, cx: &mut Context<Self>) {
        if self.enabled {
            return;
        }

        self.enabled = true;
        // Set cursors as invisible and start blinking: this causes cursors
        // to be visible during the next render.
        self.visible = false;
        self.blink_cursors(self.blink_epoch, cx);
    }

    /// Disable the blinking of the cursor.
    pub fn disable(&mut self, _cx: &mut Context<Self>) {
        self.resume_task = None;
        self.blinking_paused = false;
        self.next_blink_epoch();
        self.visible = false;
        self.enabled = false;
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    #[cfg(test)]
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext, TestAppContext};

    #[gpui::test]
    fn test_pause_blinking_restarts_single_resume_deadline(cx: &mut TestAppContext) {
        let blink_manager =
            cx.new(|cx| BlinkManager::new(Duration::from_millis(500), |_| true, cx));

        blink_manager.update(cx, |blink_manager, cx| {
            blink_manager.enable(cx);
            blink_manager.pause_blinking(cx);
            assert!(blink_manager.blinking_paused);
            assert!(blink_manager.visible);
            assert!(blink_manager.resume_task.is_some());
        });

        cx.executor().advance_clock(Duration::from_millis(400));
        blink_manager.update(cx, |blink_manager, cx| blink_manager.pause_blinking(cx));

        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        blink_manager.read_with(cx, |blink_manager, _| {
            assert!(blink_manager.blinking_paused);
            assert!(blink_manager.visible);
            assert!(blink_manager.resume_task.is_some());
        });

        cx.executor().advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
        blink_manager.read_with(cx, |blink_manager, _| {
            assert!(!blink_manager.blinking_paused);
            assert!(!blink_manager.visible);
            assert!(blink_manager.resume_task.is_none());
        });
    }

    #[gpui::test]
    fn test_disable_cancels_pending_resume(cx: &mut TestAppContext) {
        let blink_manager =
            cx.new(|cx| BlinkManager::new(Duration::from_millis(500), |_| true, cx));

        blink_manager.update(cx, |blink_manager, cx| {
            blink_manager.enable(cx);
            blink_manager.pause_blinking(cx);
            blink_manager.disable(cx);
            assert!(!blink_manager.blinking_paused);
            assert!(blink_manager.resume_task.is_none());
        });

        cx.executor().advance_clock(Duration::from_millis(500));
        cx.run_until_parked();
        blink_manager.read_with(cx, |blink_manager, _| {
            assert!(!blink_manager.enabled);
            assert!(!blink_manager.visible);
            assert!(blink_manager.resume_task.is_none());
        });
    }

    /// D15: repeated SettingsStore updates must NOT accumulate overlapping blink
    /// timers. Upstream's observer unconditionally calls `blink_cursors` on every
    /// settings change, and `blink_cursors` spawns a fresh 500ms timer that runs
    /// alongside the existing one. After N settings changes the cursor strobes at
    /// (N+1)x the normal rate. The kask observer only (re)starts blinking on the
    /// disabled→enabled transition, so unrelated settings changes (profile
    /// switch, model selection, settings.json save) do not duplicate the timer.
    ///
    /// This test pins the fix: after enabling blink and firing 5 spurious
    /// SettingsStore updates, advancing one interval toggles `visible` exactly
    /// once (one timer), not 6 times (6 overlapping timers).
    #[gpui::test]
    fn test_settings_updates_do_not_accumulate_blink_timers(cx: &mut TestAppContext) {
        use gpui::UpdateGlobal;
        let store = cx.update(|cx| settings::SettingsStore::test(cx));
        cx.update(|cx| cx.set_global(store));

        let blink_manager =
            cx.new(|cx| BlinkManager::new(Duration::from_millis(500), |_| true, cx));

        // Start blinking.
        blink_manager.update(cx, |blink_manager, cx| {
            blink_manager.enable(cx);
        });
        // `enable` sets visible=false then blink_cursors toggles to visible=true
        // and spawns the first 500ms timer.
        cx.run_until_parked();
        blink_manager.read_with(cx, |b, _| assert!(b.enabled));

        // Simulate 5 unrelated settings changes (profile switch, model
        // selection, settings.json save, etc.). Each fires the SettingsStore
        // observer on the BlinkManager.
        for _ in 0..5 {
            cx.update(|cx| {
                settings::SettingsStore::update_global(cx, |_, _| {});
            });
            cx.run_until_parked();
        }

        // Snapshot visible, advance exactly one interval, and assert exactly
        // one toggle. If overlapping timers had accumulated, visible would
        // toggle multiple times within the 500ms window.
        let visible_before = blink_manager.read_with(cx, |b, _| b.visible);
        cx.executor().advance_clock(Duration::from_millis(500));
        cx.run_until_parked();
        let visible_after_one_interval = blink_manager.read_with(cx, |b, _| b.visible);
        assert_ne!(
            visible_before, visible_after_one_interval,
            "D15: visible must toggle exactly once per interval. \
             If it did not toggle, the blink timer was cancelled by a \
             settings update. If it toggled multiple times, overlapping \
             timers accumulated — the upstream bug."
        );

        // Advance another interval and assert a second single toggle back.
        cx.executor().advance_clock(Duration::from_millis(500));
        cx.run_until_parked();
        let visible_after_two_intervals = blink_manager.read_with(cx, |b, _| b.visible);
        assert_eq!(
            visible_before, visible_after_two_intervals,
            "D15: after two intervals the cursor must return to the starting \
             state (one toggle per interval). Overlapping timers would \
             produce an out-of-phase result."
        );
    }
}
