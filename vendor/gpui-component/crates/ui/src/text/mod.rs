mod document;
mod inline;
pub(crate) mod selection;
mod state;
mod text_view;
mod window_selection;

use gpui::{App, ElementId, IntoElement, RenderOnce, SharedString, Window};
pub use state::*;
pub use text_view::*;
pub(crate) use window_selection::TextSelectionController;
pub(crate) use window_selection::WindowTextSelection;
pub(crate) use window_selection::{SelectionScope, SelectionScopeElement};

pub(crate) fn init(cx: &mut App) {
    state::init(cx);
}

/// Create literal text with its source location as the element id.
#[track_caller]
pub fn plain(source: impl Into<SharedString>) -> TextView {
    let id: ElementId = ElementId::CodeLocation(*std::panic::Location::caller());
    TextView::plain(id, source)
}

#[derive(IntoElement, Clone)]
pub enum Text {
    String(SharedString),
    TextView(Box<TextView>),
}

impl From<SharedString> for Text {
    fn from(s: SharedString) -> Self {
        Self::String(s)
    }
}

impl From<&str> for Text {
    fn from(s: &str) -> Self {
        Self::String(SharedString::from(s.to_string()))
    }
}

impl From<String> for Text {
    fn from(s: String) -> Self {
        Self::String(s.into())
    }
}

impl From<TextView> for Text {
    fn from(e: TextView) -> Self {
        Self::TextView(Box::new(e))
    }
}

impl Text {
    /// Get the text content.
    pub(crate) fn get_text(&self, cx: &App) -> SharedString {
        match self {
            Self::String(s) => s.clone(),
            Self::TextView(view) => {
                if let Some(state) = &view.state {
                    state.read(cx).source()
                } else {
                    view.text.clone().unwrap_or_default()
                }
            }
        }
    }
}

impl RenderOnce for Text {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        match self {
            Self::String(s) => s.into_any_element(),
            Self::TextView(e) => e.into_any_element(),
        }
    }
}
