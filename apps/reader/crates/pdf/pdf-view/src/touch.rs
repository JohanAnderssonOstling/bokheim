use super::*;

impl PdfView {
    /// Hide selection handles while another reader overlay owns interaction.
    pub fn set_selection_handles_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.touch_selection_handles_visible != visible {
            self.touch_selection_handles_visible = visible;
            cx.notify();
        }
    }

    pub(super) fn selection_endpoints(&self) -> [Option<Point<Pixels>>; 2] {
        let Some(selection) = &self.selection else { return [None, None] };
        let rects = selection.rects();
        [rects.first().map(|r| (r.left(), r.bottom())), rects.last().map(|r| (r.right(), r.bottom()))].map(|endpoint| {
            let (x, y) = endpoint?;
            for (slot, (left, width)) in self.screen_slots(self.available_width).into_iter().enumerate() {
                for stream in 0..if self.zoom_mode == PdfZoomMode::FitHeight { 1 } else { 2 } {
                    for column in columns_for_display(self.fit_width_plan.columns(), self.flow_base_index() + slot + stream) {
                        let column = self.painting_column(*column);
                        let rect = column.page_rect();
                        if Some(column.page_index()) != self.selection_page || x < rect.left() || x > rect.right() || y < rect.top() || y > rect.bottom() {
                            continue;
                        }
                        let inset = if self.zoom_mode == PdfZoomMode::FitHeight { ((width - column.display_width()) * 0.5).max(0.0) } else { 0.0 };
                        let point = self.surface_bounds.origin
                            + point(
                                px(left + inset + (x - rect.left()) / rect.width() * column.display_width()),
                                px(stream as f32 * self.flow_column_height() + self.column_offset_within_display(column) - self.flow_remainder() + (y - rect.top()) / rect.height() * column.display_height()),
                            );
                        if self.surface_bounds.contains(&point) {
                            return Some(point);
                        }
                    }
                }
            }
            None
        })
    }

    pub(super) fn begin_selection_handle(&mut self, event: &TouchEvent, window: &Window, cx: &mut Context<Self>) {
        let endpoints = self.selection_endpoints();
        let handle = endpoints
            .into_iter()
            .enumerate()
            .filter_map(|(index, endpoint)| {
                let endpoint = endpoint?;
                let center = components::reader_selection_handle_center(endpoint, window);
                let distance = f32::from(event.position.x - center.x).hypot(f32::from(event.position.y - center.y));
                (distance <= 24.0).then_some((index, endpoint, distance))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2));
        let Some((index, endpoint, _)) = handle else { return };
        let Some(selection) = &self.selection else { return };
        self.selection_anchor_source_index = Some(if index == 0 { selection.last_source_index() } else { selection.first_source_index() });
        self.selection_anchor = Some((0.0, 0.0));
        self.touch_selection_id = Some(event.id);
        self.touch_selection_offset = event.position - endpoint + point(px(0.0), px(3.0));
        self.tap_start = None;
        self.touch_momentum.cancel();
        cx.emit(PdfViewEvent::SelectionAdjustmentStarted);
        cx.notify();
    }

    pub(super) fn scroll_touch_content(&mut self, delta: Pixels, cx: &mut Context<Self>) -> Pixels {
        let before = self.source_position;
        let old_offset = self.flow_offset;
        self.sync_flow_scroll_target(f32::from(delta), cx);
        let applied = if let Some(pager) = &self.viewport_pager { pager.distance_to(FitWidthAnchor::new(before.page_index(), before.page_y())).unwrap_or(0.0) } else { old_offset - self.flow_offset };
        let applied = px(applied);
        self.touch_scroll_offset.y += applied;
        applied
    }

    pub(super) fn handle_touch_scroll(&mut self, travel: Point<f32>, phase: TouchPhase, from_edge: bool, window: &Window, cx: &mut Context<Self>) {
        if phase == TouchPhase::Started {
            self.touch_momentum.cancel();
            self.scroll_animation = None;
            self.touch_scroll_axis = Some(if travel.x.abs() > travel.y.abs() { Axis::Horizontal } else { Axis::Vertical });
            self.tap_start = None;
            self.clear_selection();
        }
        if self.touch_scroll_axis == Some(Axis::Horizontal) {
            self.handle_page_scroll(point(travel.x, 0.0), phase, from_edge, window, cx);
        } else {
            let applied = if phase == TouchPhase::Cancelled { Pixels::ZERO } else { self.scroll_touch_content(px(travel.y), cx) };
            self.touch_momentum.observe_surface(phase, point(Pixels::ZERO, applied), self.touch_scroll_offset, window, cx);
            cx.notify();
        }
        if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            self.touch_scroll_axis = None;
        }
    }
}
