use super::document::TextDocument;
use crate::{
    ElementExt,
    input::{self, SelectAll},
    scroll::AutoScroll,
    v_flex,
};
use gpui::{App, Bounds, Context, FocusHandle, IntoElement, KeyBinding, ListState, ParentElement as _, Pixels, Point, Render, SharedString, Styled as _, Window, px};

const CONTEXT: &'static str = "TextView";
pub(crate) fn init(cx: &mut App) {
    cx.bind_keys(vec![
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-c", input::Copy, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-c", input::Copy, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-a", input::SelectAll, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-a", input::SelectAll, Some(CONTEXT)),
    ]);
}

/// Selectable literal text, with no markup parsing or background work.
pub struct TextViewState {
    pub(super) focus_handle: FocusHandle,
    pub(super) entity_id: gpui::EntityId,
    pub(super) list_state: ListState,
    bounds: Bounds<Pixels>,
    pub(super) selectable: bool,
    pub(super) scrollable: bool,
    pub(super) is_selecting: bool,
    multi_click_selection: Option<TextViewMultiClickSelection>,
    selected_text_override: Option<String>,
    select_all: bool,
    pub(super) auto_scroll: AutoScroll,
    document: TextDocument,
}

impl TextViewState {
    /// Create a text view state that displays its input literally.
    pub fn plain(text: &str, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            entity_id: cx.entity_id(),
            list_state: ListState::new(0, gpui::ListAlignment::Top, px(1000.)).measure_all(),
            bounds: Bounds::default(),
            selectable: false,
            scrollable: false,
            is_selecting: false,
            multi_click_selection: None,
            selected_text_override: None,
            select_all: false,
            auto_scroll: AutoScroll::default(),
            document: TextDocument::new(text.into()),
        }
    }

    /// Get the text content.
    pub(crate) fn source(&self) -> SharedString {
        self.document.source.clone()
    }

    /// Set whether the text is selectable, default false.
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    /// Set whether the text is selectable, default false.
    pub fn set_selectable(&mut self, selectable: bool, cx: &mut Context<Self>) {
        self.selectable = selectable;
        cx.notify();
    }

    /// Set whether the text is selectable, default false.
    pub fn scrollable(mut self, scrollable: bool) -> Self {
        self.scrollable = scrollable;
        self
    }

    /// Set whether the text is selectable, default false.
    pub fn set_scrollable(&mut self, scrollable: bool, cx: &mut Context<Self>) {
        if !scrollable {
            self.reset_selection();
        }
        self.scrollable = scrollable;
        cx.notify();
    }

    /// Replace literal text synchronously, including its selection state.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.document.source.as_ref() == text {
            return;
        }
        self.document = TextDocument::new(text.into());
        if !self.is_selecting {
            self.reset_selection();
        }
        cx.notify();
    }

    /// Append literal text while retaining active selection coordinates.
    pub fn push_str(&mut self, text: &str, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }
        let mut source = self.document.source.to_string();
        source.push_str(text);
        self.set_text(&source, cx);
    }

    /// Return the selected text.
    pub fn selected_text(&self) -> String {
        if self.select_all {
            return self.document.text();
        }

        if let Some(text) = &self.selected_text_override {
            return text.clone();
        }

        self.document.selected_text()
    }

    /// Save bounds and unselect if bounds changed.
    pub(super) fn update_bounds(&mut self, bounds: Bounds<Pixels>) {
        if self.bounds.size != bounds.size {
            self.reset_selection();
        }
        self.bounds = bounds;
    }

    pub(super) fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    /// Whether this view has a view-local selection (select-all, multi-click, or override),
    /// independent of the window-level selection.
    pub(super) fn has_view_selection(&self) -> bool {
        self.select_all || self.multi_click_selection.is_some() || self.selected_text_override.is_some()
    }

    pub(super) fn stop_auto_scroll(&mut self) {
        self.auto_scroll.stop();
    }

    fn reset_selection(&mut self) {
        self.multi_click_selection = None;
        self.selected_text_override = None;
        self.select_all = false;
        self.is_selecting = false;
        self.auto_scroll.stop();
        // Clear the inline selection state synchronously, so offscreen
        // (virtualized) views that won't repaint don't leak stale selection
        // text into a new cross-view copy.
        self.document.clear_selection();
    }

    /// Clear the current text selection.
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.reset_selection();
        cx.notify();
    }

    pub(super) fn scroll_offset(&self) -> Point<Pixels> {
        if self.scrollable { self.list_state.scroll_px_offset_for_scrollbar() } else { Point::default() }
    }

    /// Select all rendered text in this view.
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.multi_click_selection = None;
        self.selected_text_override = None;
        self.select_all = true;
        self.is_selecting = false;
        self.auto_scroll.stop();
        cx.notify();
    }

    pub(crate) fn set_multi_click_selection(&mut self, pos: Point<Pixels>, kind: TextViewMultiClickKind, selected_text: String) {
        let scroll_offset = self.scroll_offset();
        let pos = pos - self.bounds.origin - scroll_offset;
        self.multi_click_selection = Some(TextViewMultiClickSelection { pos, kind });
        self.selected_text_override = Some(selected_text);
        self.select_all = false;
        self.is_selecting = false;
        self.auto_scroll.stop();
    }

    pub(super) fn set_auto_scroll(&mut self, delta: Option<Pixels>, cx: &mut Context<Self>) {
        self.auto_scroll.set(delta, cx, |delta, state, cx| {
            state.list_state.scroll_by(delta);
            cx.notify();
        });
    }

    /// Return the window selection (anchor, cursor) in window coordinates if
    /// this view participates in it.
    ///
    /// Single-view fast path: when both endpoints are anchored inside one
    /// TextView, only that view participates (identical to the previous
    /// per-view behavior).
    pub(crate) fn selection_points(&self, window: &Window, cx: &App) -> Option<(Point<Pixels>, Point<Pixels>)> {
        if !self.selectable {
            return None;
        }
        let root = window.root::<crate::Root>().flatten()?;
        let selection = &root.read(cx).text_selection;
        if let Some(view_id) = selection.single_view() {
            if view_id != self.entity_id {
                return None;
            }
        }
        selection.resolved_points(cx)
    }

    pub(super) fn on_action_select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selectable {
            cx.propagate();
            return;
        }

        self.select_all(cx);
    }

    pub(crate) fn is_selectable(&self) -> bool {
        self.selectable
    }

    pub(crate) fn is_all_selected(&self) -> bool {
        self.select_all
    }

    pub(crate) fn multi_click_selection(&self) -> Option<TextViewMultiClickSelection> {
        let scroll_offset = self.scroll_offset();
        self.multi_click_selection.map(|selection| {
            let pos = selection.pos + scroll_offset + self.bounds.origin;
            TextViewMultiClickSelection { pos, ..selection }
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextViewMultiClickSelection {
    pub(crate) pos: Point<Pixels>,
    pub(crate) kind: TextViewMultiClickKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextViewMultiClickKind {
    Word,
    Paragraph,
}

impl Render for TextViewState {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = cx.entity();
        let list_state = self.scrollable.then(|| self.list_state.clone());
        v_flex().w_full().child(self.document.render_root(list_state)).on_prepaint(move |bounds, window, cx| {
            let size_changed = state.read(cx).bounds().size != bounds.size;
            let id = state.entity_id();
            state.update(cx, |state, _| {
                state.update_bounds(bounds);
            });
            if size_changed {
                if let Some(root) = window.root::<crate::Root>().flatten() {
                    root.update(cx, |root, cx| {
                        root.clear_text_selection_for_resized_view(id, cx);
                    });
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, TestAppContext};

    #[gpui::test]
    fn set_text_then_push_str_appends_to_replaced_content(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let state = cx.update(|cx| cx.new(|cx| TextViewState::plain("old", cx)));
        cx.run_until_parked();

        state.update(cx, |state, cx| {
            state.set_text("", cx);
            state.push_str("new", cx);
            state.push_str(" text", cx);
        });
        cx.run_until_parked();

        state.read_with(cx, |state, _| {
            assert_eq!(state.source().as_str(), "new text");
        });

        state.update(cx, |state, cx| {
            state.set_text("", cx);
        });
        cx.run_until_parked();

        state.read_with(cx, |state, _| {
            assert_eq!(state.source().as_str(), "");
        });
    }

    #[gpui::test]
    fn select_all_preserves_literal_text(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let state = cx.update(|cx| cx.new(|cx| TextViewState::plain("<quick> &amp; **value**\nnext line", cx)));
        cx.run_until_parked();

        state.update(cx, |state, cx| {
            state.select_all(cx);
        });

        state.read_with(cx, |state, _| {
            assert!(state.has_view_selection());
            assert_eq!(state.selected_text(), "<quick> &amp; **value**\nnext line");
        });

        state.update(cx, |state, cx| {
            state.clear_selection(cx);
        });

        state.read_with(cx, |state, _| {
            assert!(!state.has_view_selection());
            assert_eq!(state.selected_text(), "");
        });
    }
}
