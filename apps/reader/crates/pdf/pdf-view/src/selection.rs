use super::*;

impl PdfView {
    pub fn copy_selection(&self, cx: &mut Context<Self>) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(self.selection_text()));
        true
    }

    fn selection_text(&self) -> String {
        self.selection.iter().map(|page| page.selection.text()).collect::<Vec<_>>().join("\n")
    }

    pub fn selection_snapshot(&self) -> Option<PdfDocumentSelectionSnapshot> {
        if self.selection.is_empty() {
            return None;
        }
        let pages =
            self.selection.iter().map(|page| PdfSelectionSnapshot { page_index: page.page_index, text: page.selection.text().to_owned(), rects: page.selection.rects().iter().map(|rect| uncrop_rect(page.crop, *rect)).collect() }).collect();
        Some(PdfDocumentSelectionSnapshot { text: self.selection_text(), pages })
    }

    pub(super) fn selection_for_page(&self, page_index: usize) -> Option<&Arc<PdfTextSelection>> {
        self.selection.iter().find(|page| page.page_index == page_index).map(|page| &page.selection)
    }

    pub(super) fn retain_selection_text(&mut self) {
        for (page_index, page) in &self.pages {
            self.selection_text.entry(*page_index).or_insert_with(|| SelectionTextPage { layout: page.text_layout.clone(), crop: page.page_crop });
        }
    }

    /// PDF has no DOM elements; expand the selected word through its text geometry.
    pub fn select_containing_paragraph(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(selected) = self.selection.first() else { return false };
        let page_index = selected.page_index;
        let source_index = selected.selection.first_source_index();
        let Some(page) = self.selection_text.get(&page_index) else { return false };
        let options = SelectionOptions::default().with_granularity(SelectionGranularity::Paragraph);
        let Some(selection) = page.layout.geometry().select_source_range_with_options(source_index, source_index, options) else { return false };
        self.selection = vec![SelectedPdfPage { page_index, selection: Arc::new(selection), crop: page.crop }];
        self.selection_page = Some(page_index);
        self.selection_granularity = SelectionGranularity::Paragraph;
        cx.notify();
        true
    }

    pub(super) fn clear_selection(&mut self) {
        self.touch_selection_id = None;
        self.selection_anchor = None;
        self.selection_anchor_source_index = None;
        self.selection_active = None;
        self.selection_active_page = None;
        self.selection_drag_position = None;
        self.selection_scroll_tick = None;
        self.selection_page = None;
        self.selection_page_column = None;
        self.selection_granularity = SelectionGranularity::Word;
        self.selection.clear();
        self.selection_text.clear();
    }

    pub(super) fn begin_selection(&mut self, page_index: usize, page_column_index: u32, position: (f32, f32), anchor_source_index: usize, granularity: SelectionGranularity, cx: &mut Context<Self>) {
        self.clear_selection();
        self.retain_selection_text();
        self.selection_page = Some(page_index);
        self.selection_page_column = Some(page_column_index);
        self.selection_anchor = Some(position);
        self.selection_anchor_source_index = Some(anchor_source_index);
        self.selection_active = Some(position);
        self.selection_active_page = Some(page_index);
        self.selection_granularity = granularity;
        self.update_selection();
        cx.notify();
    }

    pub(super) fn update_selection_to(&mut self, page_index: usize, _page_column_index: u32, position: (f32, f32), cx: &mut Context<Self>) -> bool {
        if self.selection_anchor.is_none() {
            return false;
        }
        self.retain_selection_text();
        self.selection_active = Some(position);
        self.selection_active_page = Some(page_index);
        self.update_selection();
        cx.notify();
        true
    }

    pub(super) fn update_selection(&mut self) {
        let (Some(anchor_page), Some(anchor_source), Some(active_page), Some(active)) = (self.selection_page, self.selection_anchor_source_index, self.selection_active_page, self.selection_active) else { return };
        let Some(focus) = self.selection_text.get(&active_page).and_then(|page| page.layout.geometry().hit_test(active, None)) else { return };
        let anchor = (anchor_page, anchor_source);
        let focus = (active_page, focus.source_index());
        let (start, end) = if anchor <= focus { (anchor, focus) } else { (focus, anchor) };
        let options = SelectionOptions::default().with_granularity(self.selection_granularity);
        let mut selected = Vec::new();
        for page_index in start.0..=end.0 {
            // Never silently omit an unloaded page in the middle of a passage.
            let Some(page) = self.selection_text.get(&page_index) else { return };
            let mut characters = page.layout.geometry().characters().iter().filter(|character| !character.character().is_control() && !character.character().is_whitespace());
            let Some(first) = characters.next() else { continue };
            let last = characters.next_back().unwrap_or(first);
            let first = if page_index == start.0 { start.1 } else { first.source_index() };
            let last = if page_index == end.0 { end.1 } else { last.source_index() };
            if let Some(selection) = page.layout.geometry().select_source_range_with_options(first, last, options) {
                selected.push(SelectedPdfPage { page_index, selection: Arc::new(selection), crop: page.crop });
            }
        }
        self.selection = selected;
    }

    /// Keep the endpoint on the nearest painted fragment while crossing page
    /// gaps or dragging outside the viewport to scroll.
    fn selection_target(&self, position: Point<Pixels>) -> Option<PdfInteractionTarget> {
        let mut nearest = None;
        for (slot, (left, width)) in self.screen_slots(self.available_width).into_iter().enumerate() {
            for display in screen_column_stream(self.flow_base_index(), slot, self.display_column_count().unwrap_or(0)).take(if self.zoom_mode == PdfZoomMode::FitHeight { 1 } else { 2 }) {
                for column in columns_for_display(self.fit_width_plan.columns(), display) {
                    let column = self.painting_column(*column);
                    let inset = if self.zoom_mode == PdfZoomMode::FitHeight { ((width - column.display_width()) * 0.5).max(0.0) } else { 0.0 };
                    let top = (display - self.flow_base_index() - slot) as f32 * self.flow_column_height() + self.column_offset_within_display(column) - self.flow_remainder();
                    let x = f32::from(position.x - self.surface_bounds.origin.x);
                    let y = f32::from(position.y - self.surface_bounds.origin.y);
                    let from = top.max(0.0);
                    let to = (top + column.display_height()).min(self.available_height);
                    let from_x = (left + inset).max(0.0);
                    let to_x = (left + inset + column.display_width()).min(self.available_width);
                    if from >= to || from_x >= to_x {
                        continue;
                    }
                    let clamped_x = x.clamp(from_x, to_x);
                    let clamped_y = y.clamp(from, to);
                    let distance = (x - clamped_x).powi(2) + (y - clamped_y).powi(2);
                    if nearest.as_ref().is_none_or(|(best, _)| distance < *best) {
                        let page_point = column.column_point_to_page(((clamped_x - left - inset) / column.display_width(), (clamped_y - top) / column.display_height()));
                        let target =
                            PdfInteractionTarget { page_index: column.page_index(), column, page_point, page_display_size: (column.display_width() / column.page_rect().width(), column.display_height() / column.page_rect().height()) };
                        nearest = Some((distance, target));
                    }
                }
            }
        }
        nearest.map(|(_, target)| target)
    }

    pub(super) fn drag_selection_to(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        if self.selection_anchor.is_none() {
            return false;
        }
        self.selection_drag_position = Some(position);
        if let Some(target) = self.selection_target(position) {
            self.update_selection_to(target.page_index, target.column.page_column_index(), target.page_point, cx);
        } else {
            cx.notify();
        }
        true
    }

    pub(super) fn advance_selection_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(position) = self.selection_drag_position.filter(|_| self.selection_anchor.is_some()) else { return };
        let y = f32::from(position.y - self.surface_bounds.origin.y);
        let edge = 28.0_f32.min(self.available_height * 0.25);
        let direction = if y < edge {
            -1.0
        } else if y > self.available_height - edge {
            1.0
        } else {
            0.0
        };
        if direction == 0.0 {
            self.selection_scroll_tick = None;
            return;
        }
        if self.viewport_pager.is_none() && ((direction < 0.0 && self.flow_offset <= 0.0) || (direction > 0.0 && self.flow_offset >= self.max_flow_offset())) {
            self.selection_scroll_tick = None;
            return;
        }
        // Finish loading each viewport so selections cannot skip its text.
        if self.geometry_loading || self.rendering {
            self.selection_scroll_tick = None;
            window.request_animation_frame();
            return;
        }
        let first_tick = self.selection_scroll_tick.is_none();
        let elapsed = self.selection_scroll_tick.get_or_insert_with(Instant::now).elapsed();
        let before = (self.source_position, self.flow_offset);
        if self.zoom_mode == PdfZoomMode::FitHeight {
            if elapsed < Duration::from_millis(450) {
                window.request_animation_frame();
                return;
            }
            self.set_flow_offset(self.flow_offset + direction * self.flow_column_height(), cx);
        } else {
            self.sync_flow_scroll_target(-direction * 600.0 * elapsed.as_secs_f32().min(0.05), cx);
        }
        self.selection_scroll_tick = Some(Instant::now());
        if before != (self.source_position, self.flow_offset) {
            self.drag_selection_to(position, cx);
            window.request_animation_frame();
        } else if first_tick {
            window.request_animation_frame();
        }
    }

    pub(super) fn finish_selection_at(&mut self, target: Option<PdfInteractionTarget>, cx: &mut Context<Self>) -> bool {
        if self.selection_anchor.is_none() {
            return false;
        }
        if let Some(target) = target {
            self.update_selection_to(target.page_index, target.column.page_column_index(), target.page_point, cx);
        }
        self.selection_anchor = None;
        self.selection_anchor_source_index = None;
        self.selection_active = None;
        self.selection_active_page = None;
        self.selection_drag_position = None;
        self.selection_scroll_tick = None;
        self.selection_text.retain(|page_index, _| self.selection.iter().any(|page| page.page_index == *page_index));
        true
    }
}
