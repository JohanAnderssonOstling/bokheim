use super::*;
use floem::View;
use floem::context::{ComputeLayoutCx, EventCx, PaintCx};
use floem::event::{Event, EventPropagation};
use floem::keyboard::{Key, NamedKey};
use floem::kurbo::Point;
use floem::pointer::PointerButton;
use floem::reactive::{SignalGet, SignalUpdate};
use floem::style::{CursorStyle, Style};

impl View for HTMLRenderer {
    fn id(&self) -> ViewId {
        self.id
    }

    fn view_style(&self) -> Option<Style> {
        if self.selection.link_cursor_active { Some(Style::new().cursor(CursorStyle::Pointer)) } else { None }
    }

    fn compute_layout(&mut self, cx: &mut ComputeLayoutCx) -> Option<Rect> {
        self.origin = cx.window_origin();
        None
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let had_updates = self.image_pipeline.poll();
        self.apply_reader_settings_from_signals();

        if let Some(href) = self.nav.nav_request_signal.get_untracked() {
            self.nav.nav_request_signal.set(None);
            if !href.is_empty() {
                self.navigate_to_href(&href);
            }
        }
        if let Some(step) = self.nav.history_nav_signal.get_untracked() {
            self.nav.history_nav_signal.set(None);
            if step < 0 {
                self.navigate_history_back();
            } else if step > 0 {
                self.navigate_history_forward();
            }
        }
        if let Some(query) = self.highlight.search_query_pending.get_untracked() {
            self.highlight.search_query_pending.set(None);
            self.set_search_query(&query);
        }
        if let Some(step) = self.highlight.search_nav.get_untracked() {
            self.highlight.search_nav.set(None);
            if step > 0 {
                self.next_match();
            } else {
                self.prev_match();
            }
        }

        self.configure_layout_for_viewport(self.id.get_size().unwrap());
        cx.save();
        cx.set_scale(self.layout.scale);
        cx.offset((-self.origin.x * (self.layout.scale - 1.0) / self.layout.scale, -self.origin.y * (self.layout.scale - 1.0) / self.layout.scale));

        match self.viewport.direction {
            PaintDirection::Forward => self.paint_forward(cx),
            PaintDirection::Backward => self.paint_backward(cx),
        };
        cx.restore();

        self.prefetch_images_for_current_page();
        if had_updates {
            self.id.request_paint();
        }
        if self.nav.pending_nav_anchor_update {
            self.update_nav_anchor_from_layout();
            self.persist_nav_position();
            self.nav.pending_nav_anchor_update = false;
        }
        self.update_nav_signal();
        cx.set_scale(1.0);
    }

    fn event_before_children(&mut self, _cx: &mut EventCx, event: &Event) -> EventPropagation {
        match event {
            Event::PointerDown(pointer_event) => {
                if pointer_event.button != PointerButton::Primary {
                    return EventPropagation::Continue;
                }
                if pointer_event.modifiers.control() {
                    if let Some(image_idx) = self.hit_test_image(self.to_layout(pointer_event.pos)) {
                        self.copy_image_to_clipboard(image_idx);
                        return EventPropagation::Stop;
                    }
                }
                if let Some(idx) = self.hit_test_glyph(self.to_layout(pointer_event.pos)) {
                    self.selection.selection_anchor = Some(idx);
                    self.selection.selection_active = Some(idx);
                    self.selection.is_selecting = true;
                    self.selection.select_whole_lines = pointer_event.modifiers.control();
                    self.selection.pending_link_glyph = Some(idx);
                    self.update_selection_text();
                    self.id.request_paint();
                    return EventPropagation::Stop;
                }
                EventPropagation::Continue
            }
            Event::PointerMove(pointer_event) => {
                let over_link = self.pointer_is_over_link(self.to_layout(pointer_event.pos));
                self.update_link_cursor(over_link);
                if !self.selection.is_selecting {
                    return EventPropagation::Continue;
                }
                if pointer_event.modifiers.control() {
                    self.selection.select_whole_lines = true;
                }
                if let Some(idx) = self.hit_test_glyph(self.to_layout(pointer_event.pos)) {
                    self.selection.selection_active = Some(idx);
                    if self.selection.selection_anchor != self.selection.selection_active {
                        self.selection.pending_link_glyph = None;
                    }
                    self.update_selection_text();
                    self.id.request_paint();
                    return EventPropagation::Stop;
                }
                EventPropagation::Continue
            }

            Event::PointerLeave => {
                self.update_link_cursor(false);
                EventPropagation::Continue
            }
            Event::PointerWheel(wheel_event) => {
                if wheel_event.delta.y < 0.0 {
                    self.prev_line();
                    return EventPropagation::Stop;
                }
                if wheel_event.delta.y > 0.0 {
                    self.next_line();
                    return EventPropagation::Stop;
                }
                EventPropagation::Continue
            }
            Event::PointerUp(pointer_event) => {
                if pointer_event.button != PointerButton::Primary {
                    return EventPropagation::Continue;
                }
                self.selection.is_selecting = false;
                self.update_selection_text();
                if let (Some(anchor), Some(active)) = (self.selection.selection_anchor, self.selection.selection_active) {
                    if anchor == active {
                        if let Some(pending) = self.selection.pending_link_glyph {
                            if pending == anchor && self.handle_link_click(anchor) {
                                self.selection.pending_link_glyph = None;
                                return EventPropagation::Stop;
                            }
                        }
                    }
                }
                if let Some(text) = &self.selection.selected_text {
                    println!("{text}");
                }
                self.selection.pending_link_glyph = None;
                EventPropagation::Stop
            }
            Event::KeyDown(key_event) => {
                if key_event.modifiers.alt() && !key_event.modifiers.control() {
                    match &key_event.key.logical_key {
                        Key::Named(NamedKey::ArrowLeft) => {
                            self.navigate_history_back();
                            return EventPropagation::Stop;
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            self.navigate_history_forward();
                            return EventPropagation::Stop;
                        }
                        _ => {}
                    }
                }
                if key_event.modifiers.control() {
                    match &key_event.key.logical_key {
                        Key::Named(NamedKey::ArrowRight) => {
                            self.next_document();
                            return EventPropagation::Stop;
                        }
                        Key::Named(NamedKey::ArrowLeft) => {
                            self.prev_document();
                            return EventPropagation::Stop;
                        }
                        Key::Character(char) => {
                            if char.eq("c") {
                                self.copy_selection_to_clipboard();
                                return EventPropagation::Stop;
                            }
                            if char.eq("f") {
                                let current = self.highlight.search_active.get_untracked();
                                self.highlight.search_active.set(!current);
                                return EventPropagation::Stop;
                            }
                            if key_event.modifiers.alt() {
                                if char.eq("+") {
                                    self.set_col_width(self.layout.col_width + 20.0);
                                    return EventPropagation::Stop;
                                }
                                if char.eq("-") {
                                    self.set_col_width(self.layout.col_width - 20.0);
                                    return EventPropagation::Stop;
                                }
                            } else {
                                if char.eq("+") {
                                    self.set_scale(self.layout.scale + 0.1);
                                }
                                if char.eq("-") {
                                    self.set_scale(self.layout.scale - 0.1);
                                }
                            }
                        }

                        _ => {}
                    }
                }
                if key_event.modifiers.alt() && !key_event.modifiers.control() {
                    match &key_event.key.logical_key {
                        Key::Named(NamedKey::ArrowUp) => {
                            self.set_root_font_size(self.root_font_size + 1.0);
                            return EventPropagation::Stop;
                        }
                        Key::Named(NamedKey::ArrowDown) => {
                            self.set_root_font_size(self.root_font_size - 1.0);
                            return EventPropagation::Stop;
                        }
                        _ => {}
                    }
                }
                match &key_event.key.logical_key {
                    Key::Character(char) if char.eq_ignore_ascii_case("l") => {
                        self.set_show_line_numbers(!self.show_line_numbers);
                        EventPropagation::Stop
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        self.prev_line();
                        EventPropagation::Stop
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        self.next_line();
                        EventPropagation::Stop
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        self.next_page();
                        EventPropagation::Stop
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        self.prev_page();
                        EventPropagation::Stop
                    }
                    Key::Named(NamedKey::Escape) => {
                        if self.highlight.search_active.get_untracked() {
                            self.highlight.search_active.set(false);
                            self.set_search_query("");
                            return EventPropagation::Stop;
                        }
                        EventPropagation::Continue
                    }
                    _ => EventPropagation::Continue,
                }
            }
            _ => EventPropagation::Continue,
        }
    }
}

impl HTMLRenderer {
    fn apply_reader_settings_from_signals(&mut self) {
        let column_width = self.settings.current_column_width.get_untracked();
        if (column_width - self.layout.base_col_width).abs() >= 0.1 {
            self.set_col_width(column_width);
        }

        let font_size = self.settings.current_font_size.get_untracked();
        if (font_size - self.root_font_size).abs() >= f32::EPSILON {
            self.set_root_font_size(font_size);
        }

        let scale = self.settings.current_scale.get_untracked();
        if (scale - self.layout.scale).abs() >= f64::EPSILON {
            self.set_scale(scale);
        }

        let show_line_numbers = self.settings.current_line_numbers.get_untracked();
        if show_line_numbers != self.show_line_numbers {
            self.set_show_line_numbers(show_line_numbers);
        }
    }

    fn to_layout(&self, pos: Point) -> Point {
        let scale = self.layout.scale;
        let dx = self.origin.x * (scale - 1.0);
        let dy = self.origin.y * (scale - 1.0);
        Point::new((pos.x + dx) / scale, (pos.y + dy) / scale)
    }

    fn pointer_is_over_link(&self, pos: Point) -> bool {
        let glyph_idx = match self.hit_test_glyph(pos) {
            Some(idx) => idx,
            None => return false,
        };
        if self.document.link_href_for_glyph(glyph_idx).is_none() {
            return false;
        }
        let line_idx = match self.find_line_for_glyph(glyph_idx) {
            Some(idx) => idx,
            None => return false,
        };
        let line = &self.document.lines()[line_idx];
        let screen = match self.frame.last_line_positions[line_idx] {
            Some(screen) => screen,
            None => return false,
        };
        let glyph_start = screen.point.x + self.glyph_x_in_line(line, glyph_idx);
        let glyph_index = self.document.glyph_at(glyph_idx as usize).unwrap_or_default();
        let glyph_width = self.glyph_cache.get(glyph_index).size().width;
        pos.x >= glyph_start && pos.x <= glyph_start + glyph_width
    }

    fn update_link_cursor(&mut self, over_link: bool) {
        if self.selection.link_cursor_active == over_link {
            return;
        }
        self.selection.link_cursor_active = over_link;
        self.id.request_style();
    }
}
