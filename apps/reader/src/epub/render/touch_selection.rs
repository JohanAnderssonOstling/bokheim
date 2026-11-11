//! Shared selection commands and touch selection handles.

use dictionary::can_lookup;
use gpui::prelude::*;
use gpui::{AnyElement, Context, IntoElement, LongPressEvent, MouseDownEvent, MouseUpEvent, Pixels, Point, TouchEvent, TouchId, TouchPhase, Window, anchored, deferred, div, point, px};
use ui_components as components;
use reader_lookups::WikipediaArticle;

use crate::epub::{DocumentNavigation, LoadState, ReaderView};
use crate::shell::dictionary::{Closed, DictionaryPopup, SelectElement, WordClick};

#[derive(Default)]
pub(in crate::epub) struct TouchSelectionState {
    pub(in crate::epub) active: bool,
    drag: Option<SelectionDrag>,
    word_click: WordClick,
}

struct SelectionDrag {
    id: TouchId,
    offset: Point<Pixels>,
}

/// Keep both the knob and its touch target clear of the system gesture area.
fn handle_center(endpoint: Point<Pixels>, window: &Window) -> Point<Pixels> {
    let viewport = window.viewport_size();
    let bottom = (f32::from(viewport.height - window.insets().safe_area.bottom) - 22.0).max(22.0);
    point(px(f32::from(endpoint.x).clamp(22.0, f32::from(viewport.width).max(44.0) - 22.0)), px((f32::from(endpoint.y) + 12.0).clamp(22.0, bottom)))
}

impl ReaderView {
    pub(super) fn begin_word_click(&mut self, event: &MouseDownEvent) {
        self.touch_selection.word_click.begin(event);
    }

    pub(super) fn move_word_click(&mut self, position: Point<Pixels>) {
        self.touch_selection.word_click.moved(position);
    }

    pub(super) fn finish_word_click(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.touch_selection.word_click.finish(event) || components::touch_ui() {
            return;
        }
        let position = event.position;
        let whole_element = event.click_count == 4;
        // Let the renderer finish its pointer sequence before selecting the
        // word; it consumes mouse-up while completing ordinary drag selection.
        cx.defer_in(window, move |reader, _, cx| {
            let LoadState::Ready(renderer) = &reader.load_state else { return };
            renderer.update(cx, |view, cx| if whole_element { view.select_element_at(position, cx) } else { view.select_word_at(position, cx) });
        });
    }

    pub(super) fn lookup_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_selection_popup(false, window, cx);
    }

    pub(super) fn translate_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_selection_popup(true, window, cx);
    }

    fn open_selection_popup(&mut self, translate: bool, window: &mut Window, cx: &mut Context<Self>) {
        let LoadState::Ready(renderer) = &self.load_state else { return };
        let Some((cfi_range, text)) = renderer.read(cx).selected_passage() else { return };
        if !translate && !can_lookup(&text) {
            return;
        }
        let anchor = renderer.read(cx).selection_bounds();
        let mut languages = self.book.languages();
        if let Some(language) = renderer.read(cx).selection_language().filter(|language| reader_lookups::parse_language_tag(language).is_some()) {
            languages.retain(|tag| !tag.eq_ignore_ascii_case(language));
            languages.insert(0, language.to_owned());
        }
        self.selection = Some(cfi_range);
        self.close_annotation_popup(cx);
        let dictionary = cx.new(|cx| {
            let popup = if translate { DictionaryPopup::translate(self.locator.book_id(), text, languages, anchor, window, cx) } else { DictionaryPopup::new(text, languages, anchor, window, cx) };
            popup.with_reader(self.library.clone(), self.locator.clone(), self.document_history.is_some())
        });
        cx.subscribe_in(&dictionary, window, |this, _, _: &Closed, window, cx| {
            this.dictionary = None;
            this.dismiss_selection(cx);
            this.focus_document(window, cx);
            cx.notify();
        })
        .detach();
        if self.document_history.is_some() {
            cx.subscribe_in(&dictionary, window, |this, _, article: &WikipediaArticle, window, cx| {
                this.dictionary = None;
                this.dismiss_selection(cx);
                this.focus_document(window, cx);
                cx.emit(DocumentNavigation::Link(article.url.clone()));
                cx.notify();
            })
            .detach();
        }
        cx.subscribe_in(&dictionary, window, |this, popup, event: &SelectElement, window, cx| {
            let LoadState::Ready(renderer) = &this.load_state else { return };
            if !renderer.update(cx, |view, cx| view.select_element_at(event.position, cx)) {
                return;
            }
            let Some((cfi_range, text)) = renderer.read(cx).selected_passage() else { return };
            let anchor = renderer.read(cx).selection_bounds();
            this.selection = Some(cfi_range);
            if !can_lookup(&text) {
                this.dictionary = None;
                this.focus_document(window, cx);
                cx.notify();
                return;
            }
            popup.update(cx, |popup, cx| popup.show_selection(text, anchor, cx));
        })
        .detach();
        self.dictionary = Some(dictionary);
        cx.notify();
    }
    pub(super) fn begin_touch_selection(&mut self, event: &LongPressEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let LoadState::Ready(renderer) = &self.load_state else { return false };
        let selected = renderer.update(cx, |view, cx| view.select_word_at(event.position, cx));
        if !selected {
            return false;
        }
        self.selection = None;
        self.touch_selection.active = true;
        self.touch_selection.drag = Some(SelectionDrag { id: event.id, offset: Point::default() });
        self.context_sheet = None;
        self.close_annotation_popup(cx);
        self.chrome.hide(cx);
        self.touch_swipe = None;
        self.focus_document(window, cx);
        window.prevent_default();
        cx.notify();
        true
    }

    pub(super) fn handle_selection_touch(&mut self, event: &TouchEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.dictionary.is_some() || self.context_sheet.is_some() || !self.touch_selection.active {
            return;
        }
        let LoadState::Ready(renderer) = &self.load_state else { return };
        let renderer = renderer.clone();
        if let Some(drag) = &self.touch_selection.drag {
            if event.id != drag.id {
                return;
            }
            let position = event.position - drag.offset;
            match event.phase {
                TouchPhase::Moved => renderer.update(cx, |view, cx| view.update_touch_selection(position, cx)),
                TouchPhase::Ended | TouchPhase::Cancelled => {
                    renderer.update(cx, |view, cx| {
                        if matches!(event.phase, TouchPhase::Ended) {
                            view.update_touch_selection(position, cx);
                        }
                        view.end_touch_selection(cx);
                    });
                    self.touch_selection.drag = None;
                    cx.notify();
                }
                TouchPhase::Started => {}
            }
            window.prevent_default();
            cx.stop_propagation();
            return;
        }
        if event.phase != TouchPhase::Started {
            return;
        }
        let (start, end) = renderer.read(cx).selection_endpoints();
        let handle = [(true, start), (false, end)]
            .into_iter()
            .filter_map(|(start, endpoint)| {
                let endpoint = endpoint?;
                let center = handle_center(endpoint, window);
                let distance = f32::from(event.position.x - center.x).hypot(f32::from(event.position.y - center.y));
                (distance <= 24.0).then_some((start, endpoint, distance))
            })
            .min_by(|left, right| left.2.total_cmp(&right.2));
        if let Some((start, endpoint, _)) = handle {
            if renderer.update(cx, |view, cx| view.begin_selection_handle(start, cx)) {
                // Touches land on the knob below the text. Preserve the grab
                // offset, and hit-test slightly above the line's lower edge.
                self.touch_selection.drag = Some(SelectionDrag { id: event.id, offset: event.position - endpoint + point(px(0.0), px(3.0)) });
                cx.notify();
            }
        } else if renderer.read(cx).selection_contains(event.position) {
            self.open_context_sheet(event.position, cx);
        } else {
            self.dismiss_selection(cx);
        }
        window.prevent_default();
        cx.stop_propagation();
    }

    pub(in crate::epub) fn dismiss_selection(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.touch_selection.active && self.selection.is_none() {
            return false;
        }
        self.touch_selection.active = false;
        self.touch_selection.drag = None;
        self.selection = None;
        if let LoadState::Ready(renderer) = &self.load_state {
            renderer.update(cx, |view, cx| view.clear_text_selection(cx));
        }
        cx.notify();
        true
    }

    pub(super) fn copy_selection(&mut self, cx: &mut Context<Self>) {
        let LoadState::Ready(renderer) = &self.load_state else {
            return;
        };
        if renderer.update(cx, |view, cx| view.copy_selection_as_markdown(cx)) {
            self.dismiss_selection(cx);
        }
    }

    pub(super) fn copy_selection_with_source(&mut self, cx: &mut Context<Self>) {
        if self.book.is_transient() {
            return;
        }
        let LoadState::Ready(renderer) = &self.load_state else { return };
        let Some((cfi, _)) = renderer.read(cx).selected_passage() else { return };
        let section = self.toc.ordinal_for_cfi(&cfi).and_then(|ordinal| self.toc.title_for_ordinal(ordinal)).or_else(|| self.toc.active_title());
        crate::shell::annotation_copy::copy_link(&self.locator, &self.book_title, section.as_deref(), self.book.annotation_anchor(cfi), cx);
        self.dismiss_selection(cx);
    }

    pub(super) fn render_selection_handles(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.dictionary.is_some() || self.context_sheet.is_some() || !self.touch_selection.active {
            return None;
        }
        let LoadState::Ready(renderer) = &self.load_state else { return None };
        let view = renderer.read(cx);
        let (start, end) = view.selection_endpoints();
        let theme = components::browser_theme(cx);
        let mut overlay = div().absolute().inset_0();
        for (name, endpoint) in [("start", start), ("end", end)] {
            let Some(endpoint) = endpoint else { continue };
            let center = handle_center(endpoint, window);
            let knob = div()
                .id(format!("epub-selection-{name}-handle"))
                .debug_selector(move || format!("epub-selection-{name}-handle"))
                .size(px(44.0))
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .occlude()
                .child(div().absolute().left(px(21.0)).top(px(6.0)).w(px(2.0)).h(px(16.0)).bg(theme.accent))
                .child(div().size(px(18.0)).rounded_full().bg(theme.accent));
            overlay = overlay.child(deferred(anchored().position(center - point(px(22.0), px(22.0))).child(knob)).with_priority(2));
        }
        Some(overlay.into_any_element())
    }
}
