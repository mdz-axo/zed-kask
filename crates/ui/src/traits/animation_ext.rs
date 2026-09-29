use std::time::Duration;

use gpui::{Animation, AnimationElement, AnimationExt, Transformation, percentage};

use crate::{prelude::*, traits::transformable::Transformable};

/// Maximum redraw rate for rotate animations (loading spinners), in frames per
/// second. See `use_keyed_rotate_animation`.
const ROTATE_ANIMATION_MAX_FPS: f32 = 20.0;

/// An extension trait for adding common animations to animatable components.
pub trait CommonAnimationExt: AnimationExt {
    /// Render this component as rotating over the given duration.
    ///
    /// NOTE: This method uses the location of the caller to generate an ID for this state.
    ///       If this is not sufficient to identify your state (e.g. you're rendering a list item),
    ///       you can provide a custom ElementID using the `use_keyed_rotate_animation` method.
    #[track_caller]
    fn with_rotate_animation(self, duration: u64) -> AnimationElement<Self>
    where
        Self: Transformable + Sized,
    {
        self.with_keyed_rotate_animation(
            ElementId::CodeLocation(*std::panic::Location::caller()),
            duration,
        )
    }

    /// Render this component as rotating with the given element ID over the given duration.
    fn with_keyed_rotate_animation(
        self,
        id: impl Into<ElementId>,
        duration: u64,
    ) -> AnimationElement<Self>
    where
        Self: Transformable + Sized,
    {
        self.with_animation(
            id,
            Animation::new(Duration::from_secs(duration))
                .repeat_synced()
                // zed-kask: cap spinner redraw rate (P7e). Uncapped, every visible
                // rotate animation requests a full window redraw on every display
                // frame; while agent threads run this pinned the GPUI main thread
                // (measured 2026-09-28). 20 fps is visually indistinguishable for a
                // rotating loading glyph. Needs a DIVERGENCE.md D-seam entry and a
                // pinning test at commit time.
                .with_max_fps(ROTATE_ANIMATION_MAX_FPS),
            |component, delta| component.transform(Transformation::rotate(percentage(delta))),
        )
    }
}

impl<T: AnimationExt> CommonAnimationExt for T {}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    use super::*;
    use crate::{Icon, IconName, IconSize};
    use gpui::{TestAppContext, WindowHandle, size};

    struct CappedSpinnerTestView {
        render_count: Rc<RefCell<usize>>,
    }

    impl Render for CappedSpinnerTestView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            *self.render_count.borrow_mut() += 1;
            Icon::new(IconName::LoadCircle)
                .size(IconSize::Small)
                .use_keyed_rotate_animation("capped-spinner-test", 2)
        }
    }

    /// zed-kask pin: `use_keyed_rotate_animation` must cap its redraw rate at
    /// `ROTATE_ANIMATION_MAX_FPS`. Uncapped, every visible spinner requests a
    /// full window redraw on every display frame, which pinned the GPUI main
    /// thread while agent threads ran (measured 2026-09-28). Capped, no
    /// per-frame callback is scheduled — the next render is timer-driven
    /// after the cap interval.
    #[gpui::test]
    fn rotate_animation_redraw_rate_is_capped(cx: &mut TestAppContext) {
        let render_count = Rc::new(RefCell::new(0));
        let window: WindowHandle<CappedSpinnerTestView> =
            cx.open_window(size(px(100.), px(100.)), {
                let render_count = render_count.clone();
                move |_, _| CappedSpinnerTestView { render_count }
            });
        cx.run_until_parked();
        assert_eq!(*render_count.borrow(), 1, "initial render");

        // Capped: no animation-frame callback is scheduled for the next
        // display frame, so stepping the frame does not re-render.
        let callbacks = window
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .unwrap();
        cx.run_until_parked();
        assert_eq!(
            callbacks, 0,
            "capped animation must not schedule per-frame redraws"
        );
        assert_eq!(*render_count.borrow(), 1);

        // The next render is timer-driven: advance the test clock past
        // 1/ROTATE_ANIMATION_MAX_FPS and the scheduled timer re-renders.
        let interval = Duration::from_secs_f32(1.0 / ROTATE_ANIMATION_MAX_FPS);
        cx.executor()
            .advance_clock(interval + Duration::from_millis(5));
        cx.run_until_parked();
        assert_eq!(
            *render_count.borrow(),
            2,
            "timer-driven re-render after the cap interval"
        );
    }
}
