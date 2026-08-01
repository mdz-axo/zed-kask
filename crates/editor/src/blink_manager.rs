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
        // Make sure we blink the cursors if the setting is re-enabled
        cx.observe_global::<SettingsStore>(move |this, cx| {
            this.blink_cursors(this.blink_epoch, cx)
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
}
