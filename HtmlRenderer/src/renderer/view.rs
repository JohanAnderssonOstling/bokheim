use super::*;
use floem::context::{ComputeLayoutCx, EventCx, PaintCx};
use floem::event::{Event, EventPropagation};
use floem::keyboard::{Key, NamedKey};
use floem::pointer::PointerButton;
use floem::style::{CursorStyle, Style};
use floem::{View, ViewId};
use kurbo::Point;

impl View for HTMLRenderer {
    fn id(&self) -> ViewId {
        self.adapter.id()
    }

    fn view_style(&self) -> Option<Style> {
        if self.renderer.link_cursor_active() { Some(Style::new().cursor(CursorStyle::Pointer)) } else { None }
    }

    fn compute_layout(&mut self, cx: &mut ComputeLayoutCx) -> Option<Rect> {
        self.renderer.set_origin(cx.window_origin());
        None
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        for command in self.adapter.take_commands() {
            self.apply(command);
        }

        let scale = self.renderer.scale();
        let origin = self.renderer.origin();
        let frame = self.renderer.prepare_frame(self.adapter.id().get_size().unwrap_or_default());
        cx.save();
        cx.set_scale(scale);
        cx.offset((-origin.x * (scale - 1.0) / scale, -origin.y * (scale - 1.0) / scale));

        let (glyph_cache, frame) = frame.into_parts();
        let mut painter = super::painter::FloemPainter::new(cx, glyph_cache);
        frame.paint(&mut painter);
        cx.restore();

        cx.set_scale(1.0);
    }

    fn event_before_children(&mut self, _cx: &mut EventCx, event: &Event) -> EventPropagation {
        match event {
            Event::PointerDown(pointer) => {
                if pointer.button != PointerButton::Primary {
                    return EventPropagation::Continue;
                }
                let position = self.to_layout(pointer.pos);
                let control = pointer.modifiers.control();
                let options = html_view_core::PointerDownOptions { select_whole_lines: control, copy_image: control };
                if self.renderer.pointer_down(position, options) { EventPropagation::Stop } else { EventPropagation::Continue }
            }
            Event::PointerMove(pointer) => {
                let position = self.to_layout(pointer.pos);
                if self.renderer.pointer_move(position, pointer.modifiers.control()).handled { EventPropagation::Stop } else { EventPropagation::Continue }
            }
            Event::PointerLeave => {
                self.renderer.pointer_leave();
                EventPropagation::Continue
            }
            Event::PointerWheel(wheel) => {
                if self.renderer.scroll_vertical(wheel.delta.y) {
                    EventPropagation::Stop
                } else {
                    EventPropagation::Continue
                }
            }
            Event::PointerUp(pointer) => {
                if pointer.button != PointerButton::Primary {
                    return EventPropagation::Continue;
                }
                self.renderer.pointer_up();
                EventPropagation::Stop
            }
            Event::KeyDown(key_event) => {
                if key_event.modifiers.alt() && !key_event.modifiers.control() {
                    match &key_event.key.logical_key {
                        Key::Named(NamedKey::ArrowLeft) => {
                            self.renderer.navigate_history_back();
                            return EventPropagation::Stop;
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            self.renderer.navigate_history_forward();
                            return EventPropagation::Stop;
                        }
                        _ => {}
                    }
                }
                if key_event.modifiers.control() {
                    match &key_event.key.logical_key {
                        Key::Named(NamedKey::ArrowRight) => {
                            self.renderer.next_document();
                            return EventPropagation::Stop;
                        }
                        Key::Named(NamedKey::ArrowLeft) => {
                            self.renderer.previous_document();
                            return EventPropagation::Stop;
                        }
                        Key::Character(character) if character.eq("c") => {
                            return if self.renderer.copy_selection() { EventPropagation::Stop } else { EventPropagation::Continue };
                        }
                        Key::Character(character) if character.eq("f") => {
                            self.renderer.toggle_search();
                            return EventPropagation::Stop;
                        }
                        Key::Character(character) if key_event.modifiers.alt() && character.eq("+") => {
                            self.renderer.change_column_width(20.0);
                            return EventPropagation::Stop;
                        }
                        Key::Character(character) if key_event.modifiers.alt() && character.eq("-") => {
                            self.renderer.change_column_width(-20.0);
                            return EventPropagation::Stop;
                        }
                        Key::Character(character) if character.eq("+") => {
                            self.renderer.change_scale(0.1);
                            return EventPropagation::Stop;
                        }
                        Key::Character(character) if character.eq("-") => {
                            self.renderer.change_scale(-0.1);
                            return EventPropagation::Stop;
                        }
                        _ => {}
                    }
                }
                if key_event.modifiers.alt() && !key_event.modifiers.control() {
                    match &key_event.key.logical_key {
                        Key::Named(NamedKey::ArrowUp) => {
                            self.renderer.change_root_font_size(1.0);
                            return EventPropagation::Stop;
                        }
                        Key::Named(NamedKey::ArrowDown) => {
                            self.renderer.change_root_font_size(-1.0);
                            return EventPropagation::Stop;
                        }
                        _ => {}
                    }
                }
                match &key_event.key.logical_key {
                    Key::Character(character) if character.eq_ignore_ascii_case("l") => {
                        self.renderer.toggle_line_numbers();
                        EventPropagation::Stop
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        self.renderer.previous_line();
                        EventPropagation::Stop
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        self.renderer.next_line();
                        EventPropagation::Stop
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        self.renderer.next_page();
                        EventPropagation::Stop
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        self.renderer.previous_page();
                        EventPropagation::Stop
                    }
                    Key::Named(NamedKey::Escape) if self.renderer.search_active() => {
                        self.renderer.set_search_active(false);
                        EventPropagation::Stop
                    }
                    _ => EventPropagation::Continue,
                }
            }
            _ => EventPropagation::Continue,
        }
    }
}

impl HTMLRenderer {
    fn to_layout(&self, position: Point) -> Point {
        self.renderer.to_layout_point(position)
    }
}
