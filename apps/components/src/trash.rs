//! The Trash page: one list of deleted books and folders, newest first, that
//! the reader selects from and restores or deletes for good.

use gpui::prelude::*;
use gpui::{Div, ElementId, SharedString, Stateful, div, px};
use gpui_component::{Icon, IconName};

use crate::{BOOK_COVER_ASPECT_RATIO, BrowserTheme, SPACE_MD, SPACE_SM, SPACE_XS, TEXT_MD, TEXT_XS};

/// Width of the cover or folder glyph that leads a row.
const TRASH_THUMBNAIL_WIDTH: f32 = 40.0;
const SELECTION_CHECKBOX_SIZE: f32 = 18.0;
const SELECTION_BAR_HEIGHT_REM: f32 = 3.5;

/// One deleted book or folder: its checkbox, its cover or glyph, what it is,
/// and what can be done with it. A selected row wears the selection wash.
pub fn trash_row(checkbox: impl IntoElement, leading: impl IntoElement, title: impl Into<SharedString>, description: impl Into<SharedString>, actions: impl IntoElement, selected: bool, theme: BrowserTheme) -> Div {
    div()
        .w_full()
        .min_h(px(76.0))
        .py(px(SPACE_SM))
        .pr(px(SPACE_SM))
        .flex()
        .items_center()
        .gap(px(SPACE_MD))
        .border_t_1()
        .border_color(theme.rule)
        .when(selected, |row| row.bg(theme.accent_wash))
        .child(checkbox)
        .child(leading)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(SPACE_XS))
                .child(div().truncate().text_size(gpui::rems(TEXT_MD)).child(title.into()))
                .child(div().truncate().text_size(gpui::rems(TEXT_XS)).text_color(theme.text_muted).child(description.into())),
        )
        .child(div().flex_none().flex().items_center().gap(px(SPACE_SM)).child(actions))
}

/// The square ticked to select a row, centred in a hit area the size of an
/// icon button so it is not a fiddly target.
pub fn selection_checkbox(id: impl Into<ElementId>, checked: bool, theme: BrowserTheme) -> Stateful<Div> {
    let square = div()
        .size(px(SELECTION_CHECKBOX_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(if checked { theme.accent } else { theme.border })
        .bg(if checked { theme.accent } else { theme.page_bg })
        .when(checked, |square| square.child(Icon::new(IconName::Check).size(px(12.0)).text_color(theme.accent_text)));
    div().id(id).flex_none().size(gpui::rems(crate::topbar_action_height_rem())).flex().items_center().justify_center().cursor_pointer().child(square)
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
