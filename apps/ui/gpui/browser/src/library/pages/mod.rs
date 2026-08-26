//! Pages backed by the currently selected library.

use gpui::{AnyElement, IntoElement};
use ui_components::BrowserTheme;

pub(crate) mod authors;
pub(crate) mod browse;
pub(crate) mod home;
pub(crate) mod subjects;
pub(crate) mod trash;

/// Common empty-state presentation for library-backed pages.
///
/// The surrounding page remains owned by each page, but the empty content is
/// identical across Home, Authors, hierarchy pages, and Trash.
pub(crate) fn library_empty_state(theme: BrowserTheme) -> AnyElement {
    ui_components::browser_actionable_empty("Nothing here", "There is nothing to display in this section.", gpui::div(), theme).into_any_element()
}
