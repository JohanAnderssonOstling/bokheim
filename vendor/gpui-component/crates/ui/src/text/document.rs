use super::inline::{Inline, InlineState};
use gpui::{Div, InteractiveElement as _, IntoElement as _, ListState, ParentElement as _, SharedString, Stateful, Styled as _, div};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(super) struct TextDocument {
    pub(super) source: SharedString,
    inline: Arc<Mutex<InlineState>>,
}

impl TextDocument {
    pub(super) fn new(source: SharedString) -> Self {
        let inline = Arc::new(Mutex::new(InlineState { text: source.clone(), selection: None }));
        Self { source, inline }
    }

    pub(super) fn text(&self) -> String {
        self.source.to_string()
    }

    pub(super) fn selected_text(&self) -> String {
        let Ok(state) = self.inline.lock() else { return String::new() };
        state.selection.as_ref().and_then(|selection| state.text.get(selection.start..selection.end)).unwrap_or_default().to_owned()
    }

    pub(super) fn clear_selection(&self) {
        if let Ok(mut state) = self.inline.lock() {
            state.selection = None;
        }
    }

    pub(super) fn render_root(&self, list_state: Option<ListState>) -> Stateful<Div> {
        let row = |inline| div().w_full().min_w_0().child(Inline::new("text", inline));
        let root = div().id("document").w_full();
        let Some(list_state) = list_state else { return root.child(row(self.inline.clone())) };
        if list_state.item_count() != 1 {
            list_state.reset(1);
        }
        let inline = self.inline.clone();
        root.size_full().child(gpui::list(list_state, move |_, _, _| row(inline.clone()).into_any_element()).size_full())
    }
}
