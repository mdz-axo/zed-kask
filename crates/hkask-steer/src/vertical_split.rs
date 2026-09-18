//! Shared vertical split state for Steer panels with a viewer above a director.

use gpui::{Bounds, DragMoveEvent, Pixels, px};

/// Drag payload used by the divider handle and panel-root drag surface.
pub struct VerticalSplitDrag;

/// Height of the divider's invisible grab area. The visible rule remains 1px.
pub const SPLIT_HANDLE_HIT_HEIGHT: f32 = 8.0;

const DEFAULT_BOTTOM_FRACTION: f32 = 0.5;
const MIN_BOTTOM_FRACTION: f32 = 0.2;
const MAX_BOTTOM_FRACTION: f32 = 0.8;

/// In-memory split state. The bottom pane keeps between 20% and 80% of the
/// available height so neither the viewer nor the Steer editor can be starved.
#[derive(Debug, Clone)]
pub struct VerticalSplitState {
    bottom_fraction: f32,
}

impl Default for VerticalSplitState {
    fn default() -> Self {
        Self {
            bottom_fraction: DEFAULT_BOTTOM_FRACTION,
        }
    }
}

impl VerticalSplitState {
    pub fn bottom_fraction(&self) -> f32 {
        self.bottom_fraction
    }

    pub fn update_from_drag(&mut self, event: &DragMoveEvent<VerticalSplitDrag>) {
        if let Some(fraction) = bottom_fraction_from_pointer(event.event.position.y, event.bounds) {
            self.bottom_fraction = fraction;
        }
    }

    pub fn reset(&mut self) {
        self.bottom_fraction = DEFAULT_BOTTOM_FRACTION;
    }
}

fn bottom_fraction_from_pointer(pointer_y: Pixels, panel: Bounds<Pixels>) -> Option<f32> {
    let panel_height = panel.bottom() - panel.top();
    if panel_height <= px(0.) {
        return None;
    }
    let bottom_height = panel.bottom() - pointer_y;
    Some((bottom_height / panel_height).clamp(MIN_BOTTOM_FRACTION, MAX_BOTTOM_FRACTION))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Bounds, point, size};

    #[test]
    fn pointer_fraction_clamps_without_starving_either_pane() {
        let bounds = Bounds::new(point(px(0.), px(100.)), size(px(200.), px(100.)));
        assert_eq!(bottom_fraction_from_pointer(px(100.), bounds), Some(0.8));
        assert_eq!(bottom_fraction_from_pointer(px(150.), bounds), Some(0.5));
        assert_eq!(bottom_fraction_from_pointer(px(200.), bounds), Some(0.2));
    }

    #[test]
    fn zero_height_has_no_fraction() {
        let bounds = Bounds::new(point(px(0.), px(100.)), size(px(200.), px(0.)));
        assert_eq!(bottom_fraction_from_pointer(px(100.), bounds), None);
    }
}
