//! Visual feedback shared by paginated touch surfaces.

use std::time::Duration;
use web_time::Instant;

use gpui::{App, Axis, Pixels, Point, Window, point, px, touch_page_settle_duration, touch_page_settle_easing};

/// A finger-controlled translation which can ease back after a rejected turn.
#[derive(Default)]
pub struct SwipePreview {
    offset: Point<Pixels>,
    returning_at: Option<Instant>,
    axis: Option<Axis>,
    extent: Pixels,
    return_duration: Duration,
    redraw_pending: bool,
}

impl SwipePreview {
    pub fn drag(&mut self, travel: Point<f32>, window: &Window) -> bool {
        let viewport = window.viewport_size();
        if self.returning_at.is_some() {
            self.axis = None;
        }
        if self.axis.is_none() && travel.x.abs().max(travel.y.abs()) >= 12.0 {
            if travel.x.abs() >= travel.y.abs() * 1.25 {
                self.axis = Some(Axis::Horizontal);
            } else if travel.y.abs() >= travel.x.abs() * 1.25 {
                self.axis = Some(Axis::Vertical);
            }
        }
        // Keep the other axis stationary once a deliberate direction emerges.
        let offset = if self.axis.map_or(travel.x.abs() > travel.y.abs(), |axis| axis == Axis::Horizontal) {
            point(px(travel.x).clamp(-viewport.width * 0.85, viewport.width * 0.85), Pixels::ZERO)
        } else {
            point(Pixels::ZERO, px(travel.y).clamp(-viewport.height * 0.85, viewport.height * 0.85))
        };
        self.extent = if offset.x != Pixels::ZERO { viewport.width } else { viewport.height };
        let changed = self.offset != offset;
        self.offset = offset;
        self.returning_at = None;
        // State follows every event, but observers only need one invalidation
        // until the frame consumes the latest displacement.
        let notify = changed && !self.redraw_pending;
        self.redraw_pending |= changed;
        notify
    }

    pub fn settle(&mut self) {
        self.return_duration = touch_page_settle_duration(self.offset.x.abs().max(self.offset.y.abs()), self.extent);
        self.returning_at = (self.offset != Point::default()).then(Instant::now);
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn offset(&mut self, window: &mut Window, cx: &App) -> Point<Pixels> {
        self.redraw_pending = false;
        let Some(started_at) = self.returning_at else { return self.offset };
        let progress = (started_at.elapsed().as_secs_f32() / self.return_duration.as_secs_f32()).clamp(0.0, 1.0);
        if progress >= 1.0 || cx.reduce_motion() {
            self.clear();
            return Point::default();
        }
        window.request_animation_frame();
        self.offset * (1.0 - touch_page_settle_easing(progress))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn a_burst_of_moves_invalidates_once_but_keeps_the_latest_position(cx: &mut TestAppContext) {
        let (_, visual) = cx.add_window_view(|_, _| gpui::Empty);
        visual.update(|window, cx| {
            let mut preview = SwipePreview::default();
            assert!(preview.drag(point(-20.0, 0.0), window));
            for distance in 21..100 {
                assert!(!preview.drag(point(-(distance as f32), 0.0), window));
            }
            assert_eq!(preview.offset(window, cx), point(px(-99.0), Pixels::ZERO));
            assert!(preview.drag(point(-100.0, 0.0), window), "the next frame accepts another invalidation");
        });
    }
}
