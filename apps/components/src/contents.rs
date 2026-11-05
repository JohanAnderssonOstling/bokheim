//! One contents list, for every surface that shows one.
//!
//! A book's navigation is drawn in three places — the reader's sidebar, the
//! audiobook player's chapter table, and the library's detail page — and each
//! grew its own list: a tree widget, a `uniform_list` with two amount columns,
//! and nothing at all. The rows meant the same thing and looked different, and
//! a fix to one never reached the others.
//!
//! What varies between them is data, not presentation: an entry has children
//! or an amount, and one row is where the reader currently is. So the row
//! takes those as fields and the differences stop being different components.
//!
//! Interaction stays with the caller. A row hands back two hit areas because
//! they do different things — the disclosure opens the entry, the title goes to
//! it — and which of those means "seek the running audiobook" rather than "open
//! the book" is a decision only the surface can make.

use ui_text::text;

use crate::Button;
use gpui::prelude::*;
use gpui::{Div, ElementId, MouseButton, SharedString, Stateful, div, px};
use gpui_component::{Icon, IconName, Sizable};
use std::rc::Rc;
use std::time::Duration;

use crate::{BrowserTheme, CONTENTS_AMOUNT_WIDTH_REM, CONTENTS_DISCLOSURE_SIZE_REM, CONTENTS_ROW_GAP, CONTENTS_ROW_INDENT, CONTENTS_ROW_MIN_HEIGHT_REM, SPACE_SM, TEXT_SM, TEXT_XS};

pub type ContentsReadReset = Rc<dyn Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App)>;

pub fn contents_read_reset_button(id: impl Into<ElementId>) -> Button {
    crate::outlined_icon_button(id, text!(common::reset_read_status), IconName::Undo2).flex_none().when(crate::touch_ui(), |button| button.large())
}

pub fn contents_read_checkmark(id: impl Into<ElementId>, on_reset: ContentsReadReset) -> Button {
    crate::base_button(id)
        .ghost()
        .icon(Icon::new(IconName::Check).size(gpui::rems(TEXT_XS)))
        .tooltip(text!(common::mark_chapter_as_unread))
        .debug_selector(|| "chapter-read-checkmark".into())
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |event, window, cx| {
            cx.stop_propagation();
            on_reset(event, window, cx);
        })
}

/// What one row of a contents list is.
///
/// Every field beyond the title answers a question the surface's data either
/// has or has not: an audiobook chapter knows how long it is, and a navigation
/// document's entry knows its children.
#[derive(Clone, Copy, Debug, Default)]
pub struct ContentsRow {
    /// Nesting, from zero. Indent is capped so that a deep entry stays legible
    /// rather than disappearing off the right of a narrow sidebar.
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
    /// The entry the reader is inside. Exactly one row in a list has this, and
    /// its ancestors do not: a navigation document never says that a parent is
    /// partly read.
    pub marked: bool,
    /// A completed leaf, or a parent whose children are all complete.
    pub read: bool,
    /// The keyboard cursor, which is not the same thing as where the reader is.
    pub focused: bool,
    /// Audiobook offsets and lengths, displayed in two columns on the right.
    pub timing: Option<ContentsTiming>,
}

/// Chapter times use the book-wide clock and the original playback duration.
#[derive(Clone, Copy, Debug)]
pub struct ContentsTiming {
    pub start: Duration,
    /// Missing when the final chapter's end is not known yet.
    pub duration: Option<Duration>,
}

fn format_chapter_time(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    if hours > 0 { format!("{hours}:{minutes:02}:{seconds:02}") } else { format!("{minutes}:{seconds:02}") }
}

/// An open contents list, beginning with a quiet rule. Rows draw their own
/// hairlines so the same treatment works in detail views and reader sidebars.
pub fn contents_panel(theme: BrowserTheme) -> Div {
    div().w_full().min_w_0().flex().flex_col().border_t_1().border_color(theme.rule)
}

fn contents_amount_cell(value: impl Into<SharedString>) -> Div {
    div().w(gpui::rems(CONTENTS_AMOUNT_WIDTH_REM)).flex_none().whitespace_nowrap().text_right().text_size(gpui::rems(TEXT_XS)).child(value.into())
}

/// Align audiobook timing headings with the two values in every chapter row.
pub fn contents_timing_header(theme: BrowserTheme) -> Div {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(CONTENTS_ROW_GAP))
        .py(px(ROW_PADDING_Y))
        .px(px(SPACE_SM))
        .text_color(theme.text_muted)
        .border_b_1()
        .border_color(theme.rule.opacity(0.45))
        .child(div().flex_1().min_w_0())
        .child(contents_amount_cell(text!(audiobook::chapter_start_time)))
        .child(contents_amount_cell(text!(audiobook::chapter_duration)))
        .child(div().w(gpui::rems(2.0)).flex_none())
}

/// The title, as the row's own clickable region.
///
/// Whoever owns the row may already use a click for something else — the
/// reader's sidebar is inside a tree that toggles an entry when its row is
/// clicked — so the title takes the mouse down and stops it, and navigation
/// stays separable from expansion. A surface with nothing else on the row can
/// pass a bare string instead. Contents titles wrap to two lines at most.
pub fn contents_title(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Stateful<Div> {
    div().id(id).flex_1().min_w_0().whitespace_normal().line_height_auto().line_clamp(2).text_ellipsis().cursor_pointer().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(title.into())
}

/// The disclosure, for a surface whose row already opens the entry: drawn, and
/// transparent to the click that opens it.
///
/// The reader's tree toggles on mouse down over the whole row, so the chevron
/// there holds nothing of its own and lets the event through to the row behind
/// it. A chevron that swallowed the event without handling it would be the one
/// spot in the list that did nothing at all.
///
/// Rows without children keep the empty column so that every title in the list
/// starts on the same line — a ragged left edge reads as damage.
/// Callers can attach their own toggle handler when `row.has_children` is true.
///
/// The button is as wide as a one-line row is tall, and always the row's full
/// height: it stretches, and negative margins give back the row's padding on
/// three sides, so a wrapped title's taller row gets a taller button while
/// every title still starts on one line. On the current row the hover is
/// `accent_text` at a low alpha, since `hover` is the accent itself and would
/// vanish over it.
pub fn contents_disclosure(id: impl Into<ElementId>, row: ContentsRow, theme: BrowserTheme) -> Stateful<Div> {
    div().id(id).w(gpui::rems(CONTENTS_ROW_MIN_HEIGHT_REM)).self_stretch().my(px(-ROW_PADDING_Y)).ml(px(-SPACE_SM)).flex_none().when(row.has_children, |disclosure| {
        let hovered = if row.marked { theme.accent_text.opacity(0.2) } else { theme.hover };
        disclosure
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(if row.marked { theme.accent_text } else { theme.text })
            .hover(move |style| style.bg(hovered))
            .child(Icon::new(if row.expanded { IconName::ChevronDown } else { IconName::ChevronRight }).size(gpui::rems(CONTENTS_DISCLOSURE_SIZE_REM)))
    })
}

/// One row, laid out and stated. The caller supplies the disclosure and hangs
/// its own click on the returned row.
///
/// A row reads caret, title, amount, read status. The caret column is there only
/// in a list that has parents — `disclosure` is `None` in a flat one, where it
/// would be a column of nothing.
///
/// The current entry — where the reader is — is the solid accent, the keyboard
/// cursor is `accent_wash` and the pointer is the light `hover`, as the ladder
/// in `tokens.rs` sets out. The cursor on the current entry steps it to
/// `accent_hover`. Finished entries use muted ink and a checkmark.
pub fn contents_row(id: impl Into<ElementId>, row: ContentsRow, disclosure: Option<impl IntoElement>, title: impl IntoElement, amount: Option<SharedString>, reset: Option<ContentsReadReset>, theme: BrowserTheme) -> Stateful<Div> {
    let (background, hovered, ink) = match (row.marked, row.focused) {
        (true, true) => (Some(theme.accent_hover), theme.accent_hover, theme.accent_text),
        (true, false) => (Some(theme.accent), theme.accent_hover, theme.accent_text),
        (false, true) => (Some(theme.accent_wash), theme.accent_wash_hover, if row.read { theme.text_muted } else { theme.text }),
        (false, false) => (None, theme.hover, if row.read { theme.text_muted } else { theme.text }),
    };
    let quiet = if row.marked { theme.accent_text } else { theme.text_muted };
    let depth = row.depth.min(MAXIMUM_DRAWN_DEPTH) as f32;
    let indent = SPACE_SM + CONTENTS_ROW_INDENT * depth;
    div()
        .id(id)
        .w_full()
        .min_w_0()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(CONTENTS_ROW_GAP))
        .min_h(gpui::rems(CONTENTS_ROW_MIN_HEIGHT_REM))
        .py(px(ROW_PADDING_Y))
        .px(px(SPACE_SM))
        .pl(px(indent))
        .cursor_pointer()
        .text_size(gpui::rems(TEXT_SM))
        .text_color(ink)
        .border_b_1()
        .border_color(theme.rule.opacity(0.45))
        .when_some(background, |element, colour| element.bg(colour))
        .hover(move |style| style.bg(hovered))
        .children(disclosure)
        .child(div().flex_1().min_w_0().whitespace_normal().line_height_auto().line_clamp(2).text_ellipsis().child(title))
        .when_some(row.timing, |row, timing| {
            row.child(contents_amount_cell(format_chapter_time(timing.start)).text_color(quiet)).child(contents_amount_cell(timing.duration.map(format_chapter_time).unwrap_or_else(|| "—".into())).text_color(quiet))
        })
        .children(
            amount.map(|amount| div().debug_selector(|| "contents-row-amount".into()).w(gpui::rems(CONTENTS_AMOUNT_WIDTH_REM)).flex_none().whitespace_nowrap().text_right().text_size(gpui::rems(TEXT_XS)).text_color(quiet).child(amount)),
        )
        // Reserve the status column before completion so the title keeps its
        // width. Its contents fill the existing row without setting its height.
        .child(div().relative().w(gpui::rems(2.0)).flex_none().self_stretch().my(px(-ROW_PADDING_Y)).when(row.read, |status| match reset {
            Some(reset) => status.child(contents_read_checkmark("chapter-read-checkmark", reset).absolute().inset_0().size_full().text_color(quiet)),
            None => status.child(div().absolute().inset_0().flex().items_center().justify_center().child(Icon::new(IconName::Check).size(gpui::rems(TEXT_XS)).text_color(quiet))),
        }))
}

/// Below this the indent stops growing. A fourth level is still drawn and still
/// navigable; it simply stops marching rightwards in a column that may be a
/// sidebar's width.
const MAXIMUM_DRAWN_DEPTH: usize = 3;

/// Above and below a row's title, the same whether it holds one line or two.
/// Owned here rather than by each surface because the disclosure's margins
/// cancel exactly this, to run the row's full height.
const ROW_PADDING_Y: f32 = SPACE_SM;

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, Render, TestAppContext, Window};

    struct CompletedContents;

    impl Render for CompletedContents {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let row = ContentsRow { read: true, ..Default::default() };
            div().w(px(260.0)).child(contents_row("chapter", row, None::<Div>, "Chapter title", Some("123 min".into()), Some(Rc::new(|_, _, _| {})), BrowserTheme::default()))
        }
    }

    #[gpui::test]
    fn completed_contents_keep_amount_on_one_line_and_checkmark_last(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (_, cx) = cx.add_window_view(|_, _| CompletedContents);
        cx.run_until_parked();
        let amount = cx.debug_bounds("contents-row-amount").unwrap();
        let check = cx.debug_bounds("chapter-read-checkmark").unwrap();
        assert!(amount.size.height <= px(24.0), "a three-digit duration must remain one line");
        assert!(check.left() >= amount.right(), "the completion checkmark follows the duration at the far right");
    }
}
