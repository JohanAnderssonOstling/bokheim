//! The Trash page: one list of deleted books and folders, newest first, that
//! the reader selects from and restores or deletes for good.

use gpui::prelude::*;
use gpui::{Div, ElementId, SharedString, div, px};
use gpui_component::checkbox::Checkbox;
use gpui_component::{Icon, IconName, Sizable, Size};

use crate::{BOOK_COVER_ASPECT_RATIO, BrowserTheme, SPACE_MD, SPACE_SM, TEXT_MD};

/// Width of the cover or folder glyph that leads a row.
const TRASH_THUMBNAIL_WIDTH: f32 = 40.0;
const SELECTION_BAR_HEIGHT_REM: f32 = 3.5;

/// The square ticked to select a row, centred in a hit area the size of an
/// icon button so it is not a fiddly target.
pub fn selection_checkbox(id: impl Into<ElementId>, label: impl Into<SharedString>, checked: bool) -> Checkbox {
    Checkbox::new(id).checked(checked).aria_label(label).with_size(Size::Large).flex_none().size(gpui::rems(crate::topbar_action_height_rem())).items_center().justify_center().cursor_pointer()
}

/// A book's cover at row size. `cover` is the cover image or its generated
/// stand-in.
pub fn trash_book_thumbnail(cover: impl IntoElement) -> Div {
    div().flex_none().w(px(TRASH_THUMBNAIL_WIDTH)).child(cover)
}

/// A folder's glyph, in the box a book's cover takes, so titles line up.
pub fn trash_folder_thumbnail(theme: BrowserTheme) -> Div {
    div().flex_none().w(px(TRASH_THUMBNAIL_WIDTH)).h(px(TRASH_THUMBNAIL_WIDTH / BOOK_COVER_ASPECT_RATIO)).flex().items_center().justify_center().text_color(theme.text_muted).child(Icon::new(IconName::Folder).size(px(28.0)))
}

/// The bar that acts on the selection, below the list. `label` says what is
/// selected.
pub fn selection_bar(label: impl Into<SharedString>, actions: impl IntoElement, theme: BrowserTheme) -> Div {
    div()
        .w_full()
        .h(gpui::rems(SELECTION_BAR_HEIGHT_REM))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(SPACE_SM))
        .px(px(SPACE_MD))
        .border_t_1()
        .border_color(theme.rule)
        .bg(theme.page_bg)
        .child(div().flex_1().min_w_0().truncate().text_size(gpui::rems(TEXT_MD)).child(label.into()))
        .child(actions)
}
