//! The shape of one book's page.
//!
//! Mobile pages keep book actions visible, with contents and description in
//! sheets. Desktop pages place details in a sidebar; contents and description
//! sit beside it when space allows, or stack in one reading column.
//!
//! These functions style and lay out. What a button does, which entry the reader
//! is inside, and whether the file is on the device are the page's business.

use ui_text::text;

use gpui::prelude::*;
use gpui::{Div, ElementId, SharedString, Stateful, div, px, rems};
use gpui_component::text::TextView;
use gpui_component::IconName;

use crate::{
    BOOK_DETAIL_COLUMN_GAP, BOOK_DETAIL_COLUMN_MAX_WIDTH_REM, BOOK_DETAIL_COVER_FRACTION, BOOK_DETAIL_COVER_WIDTH, BOOK_DETAIL_HERO_GAP, BOOK_DETAIL_PAGE_MARGIN,
    BOOK_DETAIL_PAGE_WIDTH_REM, BOOK_DETAIL_SECTION_GAP, BrowserTheme, SPACE_LG, SPACE_SM, SPACE_XS, TEXT_LG, TEXT_MD, TEXT_SM, TEXT_XS, TITLE_MD, topbar_action_height_rem,
};

gpui::actions!(book_detail, [ShowBookDescription, ShowBookContents, CloseBookPlayer]);

pub fn book_detail_close_player_button() -> crate::Button {
    crate::playback_secondary_button(crate::outlined_icon_button("book-close-player", text!(common::close_player), IconName::Close)).on_click(|_, window, cx| window.dispatch_action(Box::new(CloseBookPlayer), cx))
}

/// Secondary book information stays one tap away without extending the page.
pub fn book_detail_description_button(theme: BrowserTheme) -> crate::Button {
    crate::playback_secondary_button(crate::base_button("book-description").ghost().text_color(theme.text).icon(IconName::Info).tooltip(text!(common::description)))
        .debug_selector(|| "book-description".into())
        .on_click(|_, window, cx| window.dispatch_action(Box::new(ShowBookDescription), cx))
}

pub fn book_detail_contents_button(theme: BrowserTheme) -> crate::Button {
    crate::playback_secondary_button(crate::base_button("book-contents").ghost().text_color(theme.text).icon(IconName::Menu).tooltip(text!(common::contents)))
        .debug_selector(|| "book-contents".into())
        .on_click(|_, window, cx| window.dispatch_action(Box::new(ShowBookContents), cx))
}

pub fn book_detail_mobile_tools(theme: BrowserTheme) -> Div {
    crate::now_playing_tools().child(book_detail_contents_button(theme)).child(book_detail_description_button(theme))
}

/// How hard a desktop column grows against a margin or gap. The columns take
/// all the spare width until each reaches its cap; only then do the margins
/// and gaps share what is left. (Flex weights below 1 in total would leave
/// that remainder unused, so the spacers keep a weight of 1 and the columns
/// outweigh them instead.)
const COLUMN_GROWTH: f32 = 1000.0;

/// The page centres whichever responsive measure the caller selects.
///
/// The caller makes it scroll — `overflow_y_scrollbar` wraps an element that
/// already has its children, so it cannot be applied from in here.
pub fn book_detail_page() -> Div {
    div().size_full().min_h_0().min_w_0().flex().flex_col().items_center().p(px(SPACE_LG))
}

/// The compact page's single measure.
pub fn book_detail_measure() -> Div {
    div().w_full().max_w(rems(BOOK_DETAIL_PAGE_WIDTH_REM)).min_w_0().flex().flex_col().gap(px(BOOK_DETAIL_SECTION_GAP))
}

/// The desktop page, whose columns each scroll on their own: a long chapter
/// list never carries the cover and the player out of view. The page itself
/// does not scroll, and leaves the vertical padding to the columns, so their
/// content scrolls up to the window's edge rather than vanishing short of it.
///
/// It spans the whole width: the player column stays at the cover's width,
/// while contents and description grow to `BOOK_DETAIL_COLUMN_MAX_WIDTH_REM`.
/// Whatever is left past those caps is shared equally by the two margins and
/// the gaps between columns.
pub fn book_detail_columns_page() -> Div {
    div().size_full().min_h_0().min_w_0().flex().flex_row()
}

/// One of the page's two outer margins.
pub fn book_detail_page_margin() -> Div {
    div().flex_grow(1.0).flex_basis(px(0.0)).min_w(px(BOOK_DETAIL_PAGE_MARGIN))
}

/// The room between two columns, with a hairline down its middle.
pub fn book_detail_column_gap(theme: BrowserTheme) -> Div {
    div().relative().flex_grow(1.0).flex_basis(px(0.0)).min_w(px(BOOK_DETAIL_COLUMN_GAP)).child(div().absolute().top(px(SPACE_LG)).bottom(px(SPACE_LG)).left(gpui::relative(0.5)).w(px(1.0)).bg(theme.rule.opacity(0.6)))
}

/// A desktop column that grows with its fellows up to its cap.
pub fn book_detail_column() -> Div {
    div().flex_grow(COLUMN_GROWTH).flex_shrink(1.0).flex_basis(px(0.0)).min_w_0().max_w(rems(BOOK_DETAIL_COLUMN_MAX_WIDTH_REM)).min_h_0().flex().flex_col()
}

/// The player's column matches the cover's width, keeping its contents centred.
pub fn book_detail_sidebar_column() -> Div {
    book_detail_column().min_w(px(BOOK_DETAIL_COVER_WIDTH)).max_w(px(BOOK_DETAIL_COVER_WIDTH))
}

/// The cover and the headline, inside the sidebar's scroll area.
pub fn book_detail_sidebar() -> Div {
    div().w_full().min_w_0().flex().flex_col().gap(px(SPACE_LG)).py(px(SPACE_LG))
}

pub fn book_detail_sidebar_cover_slot() -> Div {
    div().relative().w(px(BOOK_DETAIL_COVER_WIDTH)).max_w_full().flex_none().self_center()
}

/// What a contents or description column holds, inside its scroll area. The
/// trailing padding keeps the scrollbar off the chapters' durations.
pub fn book_detail_column_body() -> Div {
    div().w_full().min_w_0().flex().flex_col().gap(px(BOOK_DETAIL_SECTION_GAP)).py(px(SPACE_LG)).pr(px(SPACE_SM))
}

/// Back to where the book was opened, beside the top of its cover.
pub fn book_detail_back(id: impl Into<ElementId>, theme: BrowserTheme) -> crate::Button {
    crate::outlined_icon_button(id, text!(common::return_to_library), IconName::ArrowLeft).text_color(theme.text).tooltip(crate::shortcut_tooltip(text!(common::return_to_library), "escape")).debug_selector(|| "book-detail-back".into())
}

/// A centred cover with navigation beside its top edge, without a header row.
pub fn book_detail_cover_row(back: Option<crate::Button>, cover: impl IntoElement) -> Div {
    div().relative().w_full().flex().justify_center().items_start().child(cover).children(back.map(|back| div().absolute().top_0().left_0().child(back)))
}

/// The cover and the words beside it, side by side down to a width where the
/// title would have too little room; below that the page stacks them, the
/// title centred under the cover.
pub fn book_detail_hero() -> Div {
    div().w_full().min_w_0().flex().flex_row().items_start().gap(px(BOOK_DETAIL_HERO_GAP))
}

/// The cover is 300 pixels wide, limited to half the hero on narrow screens.
///
/// Positioned, because the reading position hangs under it the way it does
/// under a card in the grid.
pub fn book_detail_cover_slot() -> Div {
    div().relative().flex_none().w(px(BOOK_DETAIL_COVER_WIDTH)).max_w(gpui::relative(BOOK_DETAIL_COVER_FRACTION))
}

pub fn book_detail_headline() -> Div {
    div().flex_1().min_w_0().flex().flex_col().gap(px(SPACE_SM))
}

/// The headline under the desktop cover, centred: the cover above it and the
/// player below it are both symmetric, and a left-aligned block between them
/// reads as lopsided.
pub fn book_detail_sidebar_headline() -> Div {
    book_detail_headline().items_center().text_center()
}

/// Literal metadata with selection and clipboard support, without markup parsing.
pub fn book_detail_text(id: impl Into<ElementId>, text: impl Into<SharedString>) -> TextView {
    TextView::plain(id, text).selectable(true)
}

pub fn book_detail_title(title: impl Into<SharedString>, subtitle: Option<&str>, large: bool, theme: BrowserTheme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE_XS))
        .child(div().when_some(theme.display_font_family(), |text, family| text.font_family(family)).text_size(rems(if large { TITLE_MD } else { TEXT_LG })).text_color(theme.text).child(book_detail_text("book-detail-title", title)))
        .children(subtitle.filter(|subtitle| !subtitle.trim().is_empty()).map(|subtitle| div().text_size(rems(TEXT_MD)).text_color(theme.text_muted).child(book_detail_text("book-detail-subtitle", subtitle.to_owned()))))
}

/// One credited line: "Narrated by", "Translated by", the authors themselves.
/// The role is stated rather than implied by position, because a page that lists
/// four names in a column says nothing about which of them wrote it.
pub fn book_detail_credit_line(role: Option<impl Into<SharedString>>, theme: BrowserTheme) -> Div {
    div().flex().flex_row().flex_wrap().items_baseline().gap(px(SPACE_XS)).text_size(rems(TEXT_SM)).text_color(theme.text_muted).children(role.map(|role| div().flex_none().child(book_detail_text("book-detail-credit-role", role))))
}

/// Author names wrap within the headline, with room for at most three lines.
/// The height cap also limits rows formed by separate clickable names.
pub fn book_detail_authors(theme: BrowserTheme) -> Div {
    const LINE_HEIGHT: f32 = 1.5;
    book_detail_credit_line(None::<SharedString>, theme).min_w_0().max_w_full().gap_y_0().line_height(rems(TEXT_SM * LINE_HEIGHT)).line_clamp(3).max_h(rems(TEXT_SM * LINE_HEIGHT * 3.0)).overflow_hidden()
}

/// A name that goes somewhere — an author's own page.
pub fn book_detail_name_link(id: impl Into<ElementId>, name: impl Into<SharedString>, theme: BrowserTheme) -> Stateful<Div> {
    div().id(id).cursor_pointer().text_color(theme.text_accent).hover(|style| style.underline()).child(book_detail_text("book-detail-name", name))
}

/// The facts a reader checks rather than reads — how long an audiobook runs,
/// how many chapters it has. Separated by a middot on one line, because each is
/// two words long and a list of them down the page would take more room than the
/// cover beside it.
pub fn book_detail_facts(facts: impl IntoIterator<Item = SharedString>, theme: BrowserTheme) -> Div {
    let mut row = div().flex().flex_row().flex_wrap().items_baseline().gap(px(SPACE_SM)).text_size(rems(TEXT_XS)).text_color(theme.text_muted);
    for (index, fact) in facts.into_iter().enumerate() {
        if index > 0 {
            row = row.child(div().flex_none().text_color(theme.rule).child(book_detail_text(format!("book-detail-fact-separator-{index}"), "·")));
        }
        row = row.child(div().flex_none().child(book_detail_text(format!("book-detail-fact-{index}"), fact)));
    }
    row
}

/// Where the reader is, in the book's own words rather than as a percentage:
/// "Against the Tide?" is a place, "34%" is a number about one. The percentage
/// stays beside it for the books whose navigation says nothing.
///
/// Said, not drawn. How far in they are is the bar under the cover, the same
/// one and in the same place as on a card in the grid — a book is one thing
/// wherever it is seen. Drawn here as well it ran the width of the hero, which
/// at nought per cent is a rule under the byline saying nothing.
pub fn book_detail_position(entry: Option<SharedString>, detail: Option<SharedString>, left_aligned: bool, theme: BrowserTheme) -> Div {
    let has_detail = detail.is_some();
    div()
        .w_full()
        .flex()
        .flex_row()
        .items_baseline()
        .justify_between()
        .gap(px(SPACE_SM))
        .text_size(rems(TEXT_XS))
        .text_color(theme.text_muted)
        .children((has_detail && !left_aligned).then(|| div().flex_none().w(rems(3.5))))
        .child(
            div()
                .debug_selector(|| "book-detail-position-entry-box".into())
                .flex_1()
                .min_w_0()
                .text_center()
                .when(left_aligned, |entry| entry.text_left())
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .children(entry.map(|entry| book_detail_text("book-detail-position-entry", entry))),
        )
        .children(detail.map(|detail| div().flex_none().w(rems(3.5)).text_right().child(book_detail_text("book-detail-position-detail", detail))))
}

pub fn book_detail_actions() -> Div {
    div().w_full().flex().flex_row().flex_wrap().items_center().justify_center().gap(px(SPACE_SM))
}

/// The compact page's sections, in reading order.
pub fn book_detail_sections() -> Div {
    div().w_full().min_w_0().flex().flex_col().gap(px(SPACE_LG))
}

pub fn book_detail_section() -> Div {
    div().w_full().min_w_0().flex().flex_col().gap(px(SPACE_SM))
}

pub fn book_detail_heading(heading: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().when_some(theme.display_font_family(), |text, family| text.font_family(family)).text_size(rems(TEXT_LG)).text_color(theme.text).child(book_detail_text("book-detail-heading", heading))
}

/// A section's heading, with what the section sums up to — "41 chapters ·
/// 23 h 37 min" — set small at the end of the same line.
///
/// `open` is `Some` where the section folds: the heading then ends in a caret
/// saying which way it is, and the caller makes the whole heading the toggle.
pub fn book_detail_section_heading(heading: impl Into<SharedString>, summary: Option<impl Into<SharedString>>, open: Option<bool>, theme: BrowserTheme) -> Div {
    div()
        .w_full()
        .min_h(rems(topbar_action_height_rem()))
        .min_w_0()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(SPACE_SM))
        .child(book_detail_heading(heading, theme).flex_1().min_w_0())
        .children(summary.map(|summary| div().flex_none().text_size(rems(TEXT_XS)).text_color(theme.text_muted).child(book_detail_text("book-detail-section-summary", summary))))
        .when_some(open, |heading, open| {
            heading.cursor_pointer().child(gpui_component::Icon::new(if open { gpui_component::IconName::ChevronDown } else { gpui_component::IconName::ChevronRight }).size(rems(1.0)).text_color(theme.text_muted))
        })
}

/// The cover and the title beside it, then the player at full
/// width: beside the cover it would have half the row.
pub fn book_detail_introduction() -> Div {
    div().w_full().min_w_0().flex().flex_col().gap(px(SPACE_SM))
}

/// Too narrow for a title beside the cover: the cover centred, then
/// the title block, player and all, centred under it.
pub fn book_detail_stacked_introduction() -> Div {
    div().w_full().min_w_0().flex().flex_col().gap(px(SPACE_LG))
}
