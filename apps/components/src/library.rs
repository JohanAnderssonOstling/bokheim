use gpui::prelude::*;
use gpui::{AnyElement, Div, ElementId, ImageSource, ObjectFit, SharedString, Stateful, StyledImage, div, img, px};
use gpui_component::breadcrumb::{Breadcrumb, BreadcrumbItem};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::spinner::Spinner;
use gpui_component::{Icon, IconName, Selectable, Sizable};

use crate::{
    BOOK_CARD_COVER_CONTENT_GAP, BOOK_CARD_TITLE_LINE_HEIGHT_EM, BOOK_CARD_TITLE_LINES, BOOK_CARD_TITLE_SIZE, BOOK_CARD_TITLE_WEIGHT, BOOK_COVER_ASPECT_RATIO, BrowserTheme, CAROUSEL_CONTENT_GAP, CAROUSEL_CONTROL_GAP,
    CAROUSEL_TITLE_SIZE, CAROUSEL_TITLE_WEIGHT, RADIUS_MD, RADIUS_SM, SHELF_SUBJECT_SIZE, SPACE_SM, SPACE_XS, SPACE_XXS, TEXT_MD, TEXT_SM, TEXT_XS,
};

/// Cover, then the text block. The two levels each have one gap: `SPACE_MD`
/// separates the art from the words, `SPACE_XS` separates the words from each
/// other. Previously the wider first gap was a margin on the title fighting the
/// column's own gap, which meant two spacing mechanisms in one component.
/// A focused card draws `accent_wash` as a rounded tile inside the grid's gap,
/// and leaves every foreground on the card alone.
///
/// It used to invert: `accent` with `accent_text` over it. An inversion has to
/// reach the edges of its cell or it reads as a hole in the fill, which is why
/// the grid ran at a zero gap and the card carried the spacing as its own
/// padding. A tint does not — so the gap came back (`BOOK_GRID_BOOK_GAP`) and
/// the padding became the tile's inset instead of the spacer between cards.
///
/// Hover depends on whether the card is already tinted, because gpui's hover
/// style *replaces* `background` rather than painting over it. On a plain card
/// the translucent `hover` composites against the page, which is what it is
/// for; on a tinted one it would composite against the page too and throw the
/// tile away, so the card would stop looking focused exactly when the pointer
/// reached it. A focused card hovers to `accent_wash_hover` instead — the same
/// tint, lifted.
///
/// `marquee` drops hover entirely: a rubber band drags the pointer over every
/// card it collects, and cards answering that individually report the pointer's
/// path instead of the selection.
pub fn book_card(id: impl Into<ElementId>, focused: bool, marquee: bool, theme: BrowserTheme) -> Stateful<Div> {
    let hovered = if focused { theme.accent_wash_hover } else { theme.hover };
    div()
        .id(id)
        .relative()
        .w_full()
        .min_w_0()
        .flex_none()
        .flex()
        .flex_col()
        .p(px(crate::BOOK_CARD_PADDING))
        .gap(px(BOOK_CARD_COVER_CONTENT_GAP))
        .rounded(px(RADIUS_MD))
        .when(focused, |card| card.bg(theme.accent_wash))
        .when(!marquee, |card| card.hover(move |style| style.bg(hovered)))
}

/// The grid cell a card sits in, at the row's uniform reserved height.
///
/// Carries no background of its own — a card's cover is boxed at its own
/// real proportions rather than the placeholder's, so `book_card` (which does
/// carry the focus tint) comes out shorter or taller than the cell. This cell
/// is plain space around it; `justify_end` is what pins every card to the
/// cell's bottom edge regardless, so covers align along one line and the
/// tint hugs only the card actually drawn, not the row's full reserved height.
pub fn book_card_cell() -> Div {
    div().relative().w_full().h_full().min_w_0().flex().flex_col().justify_end()
}

pub fn book_card_text() -> Div {
    div().w_full().min_w_0().flex_none().flex().flex_col().gap(px(crate::BOOK_CARD_TEXT_GAP))
}

/// The text block under a card's cover — see `book_card_title_row`. Reserves
/// the title budget the paginator's row height is computed from, so a title
/// without a subtitle carries its slack below itself rather than shortening
/// the card.
pub fn book_card_text_title_only() -> Div {
    div().w_full().min_w_0().flex_none().flex().flex_col().h(gpui::rems(crate::BOOK_CARD_TITLE_HEIGHT_REM))
}

/// `aspect_ratio` is the cover's own width-over-height once its art is
/// decoded, so the box takes the real cover's proportions instead of every
/// book being forced into the same frame. Callers without a decoded image yet
/// pass `BOOK_COVER_ASPECT_RATIO` as a placeholder.
pub fn book_cover(_theme: BrowserTheme, aspect_ratio: f32) -> Div {
    div().relative().w_full().min_w_0().aspect_ratio(aspect_ratio)
}

pub fn book_cover_frame(theme: BrowserTheme, aspect_ratio: f32) -> Div {
    crate::clickable_surface(theme).relative().w_full().min_w_0().aspect_ratio(aspect_ratio)
}

/// Cover art scaled to fit the standard 1:1.5 cover frame.
///
/// Scaled rather than cropped: a cover's own proportions are part of what it
/// is, and a square audiobook cover cropped to a paperback's 2:3 would lose
/// its edges rather than just sit in a taller frame than most books need.
///
/// The cover carries a light shadow at rest, so its edge lifts off the page
/// the way a real cover would rather than lying flush with the background,
/// and deepens on hover. The outline itself stays invisible except when
/// focus or selection picks it out in the accent colour — a hover border
/// read as a stray black line rather than a lift.
/// A shaded strip down the leading edge suggests the page block a bound
/// cover wraps around, so the art reads as an object rather than a poster.
///
/// Only the outer edge rounds. A jacket's spine side is a flat fold against
/// the pages, not a curve, so rounding it along with the outer corners would
/// contradict the spine shadow sitting right next to it.
pub fn book_cover_image(id: impl Into<SharedString>, source: impl Into<ImageSource>, opacity: f32, focused: bool, theme: BrowserTheme) -> AnyElement {
    let id = id.into();
    div()
        .id(ElementId::from(id.clone()))
        .relative()
        .size_full()
        .overflow_hidden()
        .rounded_r(px(crate::BOOK_COVER_RADIUS))
        .border_1()
        .border_color(if focused { theme.accent } else { gpui::transparent_black() })
        .shadow(vec![gpui_component::box_shadow(px(0.0), px(2.0), px(6.0), px(0.0), theme.shadow)])
        .hover(move |style| style.shadow(vec![gpui_component::box_shadow(px(0.0), px(6.0), px(16.0), px(0.0), theme.shadow)]))
        // Focus lifts the cover as a supporting cue to the selected card.
        .when(focused, |cover| cover.shadow(crate::overlay_shadow(theme)))
        // `overflow_hidden` on the wrapping div clips with a rectangular
        // mask regardless of its own corner radius — an image paints its
        // corners from its own style, so the rounding has to be set here too.
        .child(img(source).size_full().object_fit(ObjectFit::Contain).opacity(opacity).rounded_r(px(crate::BOOK_COVER_RADIUS)))
        .child(book_cover_spine())
        .into_any_element()
}

/// The gradient strip `book_cover_image` shades its leading edge with.
///
/// Painted as the image's last child so it lands above the art rather than
/// under it — an opaque cover would otherwise hide anything drawn first.
fn book_cover_spine() -> gpui::Div {
    div().absolute().left_0().top_0().bottom_0().w(px(crate::BOOK_COVER_SPINE_WIDTH)).bg(gpui::linear_gradient(90.0, gpui::linear_color_stop(gpui::black().opacity(0.35), 0.0), gpui::linear_color_stop(gpui::transparent_black(), 1.0)))
}

/// The reader's place in the book, drawn on the cover's own bottom edge.
///
/// Laid over the artwork rather than in the gap below it, on a scrim dark
/// enough to keep the mark legible against art of any colour — without it,
/// an accent-coloured bar disappears into a cover already carrying warm or
/// saturated tones there. Length is the entire signal: nothing at zero, full
/// at the end. There is no separate finished treatment — a finished book is
/// one whose rule is full, and hiding those is the filter's business, not the
/// card's. Because length rather than colour carries it, the mark survives
/// greyscale.
pub fn book_cover_progress(progress: f32, theme: BrowserTheme) -> Option<AnyElement> {
    let fraction = (progress / 100.0).clamp(0.0, 1.0);
    if fraction <= 0.0 {
        return None;
    }
    Some(
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(px(crate::BOOK_COVER_PROGRESS_SCRIM_HEIGHT))
            .flex()
            .flex_col()
            .justify_end()
            .child(div().absolute().inset_0().bg(gpui::linear_gradient(0.0, gpui::linear_color_stop(gpui::black().opacity(0.6), 0.0), gpui::linear_color_stop(gpui::transparent_black(), 1.0))))
            .child(div().relative().h(px(crate::BOOK_COVER_PROGRESS_HEIGHT)).flex().flex_row().child(div().w(gpui::relative(fraction)).h_full().bg(theme.accent)))
            .into_any_element(),
    )
}

/// Stand-in for a book with no cover art.
///
/// The label was previously an unstyled raw string at inherited body size,
/// which read as loud as a real title. Muted and small: it is an absence, not
/// a thing to look at.
pub fn missing_book_cover(label: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    book_cover(theme, BOOK_COVER_ASPECT_RATIO)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .rounded_r(px(crate::BOOK_COVER_RADIUS))
        .border_1()
        .border_color(theme.rule)
        .bg(theme.page_bg)
        .gap(px(SPACE_XS))
        .child(Icon::new(IconName::BookOpen).large().text_color(theme.text_muted))
        .child(div().text_size(gpui::rems(TEXT_XS)).text_color(theme.text_muted).child(label.into()))
}

/// Fixed jacket colours a generated cover is drawn from — book cloth, not UI
/// chrome, so these are their own palette rather than derived from `theme`:
/// a cover's colour should not change when the reader switches themes.
const GENERATED_COVER_VARIANTS: [(u32, u32); 5] = [
    (0xb5502e, 0x3a2015), // terracotta
    (0x163a4d, 0x061219), // deep blue
    (0x5a4a2e, 0x2b2216), // ochre
    (0x6b3f45, 0x2c1719), // wine
    (0x4a4560, 0x201d2c), // slate violet
];

/// A cover for a book with no art of its own, used under the "cover text:
/// cover only" setting in place of [`missing_book_cover`]: that placeholder
/// says nothing without the title line the setting has just turned off, so a
/// book with no jacket would otherwise be the one anonymous tile in the grid.
///
/// `variant` picks the jacket colour — pass something stable per book (its
/// content hash, not a random draw) so the same book always generates the
/// same cover. Built from a gradient and text, nothing rasterized, so it
/// costs what any other card element costs.
pub fn book_cover_generated(title: impl Into<SharedString>, subtitle: Option<&str>, variant: usize, theme: BrowserTheme) -> Div {
    let (from, to) = GENERATED_COVER_VARIANTS[variant % GENERATED_COVER_VARIANTS.len()];
    let ink: gpui::Hsla = gpui::rgb(0xf2efe8).into();
    let subtitle = subtitle.map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned);
    book_cover(theme, BOOK_COVER_ASPECT_RATIO)
        .overflow_hidden()
        .rounded_r(px(crate::BOOK_COVER_RADIUS))
        .bg(gpui::linear_gradient(155.0, gpui::linear_color_stop(gpui::rgb(from), 0.0), gpui::linear_color_stop(gpui::rgb(to), 1.0)))
        .flex()
        .flex_col()
        .justify_end()
        .p(px(SPACE_SM * 2.0))
        .gap(px(SPACE_XXS))
        .text_color(ink)
        .child(div().w(px(28.0)).h(px(2.0)).bg(ink).opacity(0.6))
        .child(
            div()
                .w_full()
                .min_w_0()
                .mt(px(SPACE_XS))
                .font_weight(BOOK_CARD_TITLE_WEIGHT)
                .text_size(gpui::rems(BOOK_CARD_TITLE_SIZE))
                .line_height(gpui::relative(BOOK_CARD_TITLE_LINE_HEIGHT_EM))
                .text_ellipsis()
                .line_clamp(BOOK_CARD_TITLE_LINES)
                .child(title.into()),
        )
        .children(subtitle.map(|subtitle| div().w_full().min_w_0().opacity(0.85).text_size(gpui::rems(crate::BOOK_CARD_SUBTITLE_SIZE)).text_ellipsis().line_clamp(1).child(subtitle)))
}

/// No colour of its own: the title inherits from the card, which is what turns
/// it to `accent_text` when the cell fills with the accent.
///
/// Takes only the lines it needs rather than reserving the full
/// title-and-subtitle height itself — see `book_card_text_title_only`, which
/// reserves that budget as one box around the title.
pub fn book_card_title() -> Div {
    div().w_full().min_w_0().flex_none().text_size(gpui::rems(BOOK_CARD_TITLE_SIZE)).line_height(gpui::relative(BOOK_CARD_TITLE_LINE_HEIGHT_EM)).font_weight(BOOK_CARD_TITLE_WEIGHT)
}

/// Show the main title over up to two lines, with one smaller subtitle line
/// under it. The title takes the space first — it's the line a reader scans
/// for — so a long subtitle is what gives way to the ellipsis, never the title.
pub fn book_title_lines(title: impl Into<SharedString>, subtitle: Option<&str>) -> Div {
    let title = title.into();
    let Some(subtitle) = subtitle.map(str::trim).filter(|value| !value.is_empty()) else {
        return div().w_full().min_w_0().text_ellipsis().line_clamp(BOOK_CARD_TITLE_LINES).child(title);
    };
    div().w_full().min_w_0().flex().flex_col().child(div().w_full().min_w_0().text_ellipsis().line_clamp(BOOK_CARD_TITLE_LINES).child(title)).child(
        div()
            .w_full()
            .min_w_0()
            .text_size(gpui::rems(crate::BOOK_CARD_SUBTITLE_SIZE))
            .line_height(gpui::relative(BOOK_CARD_TITLE_LINE_HEIGHT_EM))
            .font_weight(gpui::FontWeight::NORMAL)
            .text_ellipsis()
            .line_clamp(crate::BOOK_CARD_SUBTITLE_LINES)
            .child(subtitle.to_owned()),
    )
}

/// A card's title row: title and subtitle, with the download control beside
/// them. The card states no author — the title is tall enough for both title
/// parts, so the control has room beside it without a metadata line of its own.
pub fn book_card_title_row(title: impl Into<SharedString>, subtitle: Option<&str>, download: Option<AnyElement>) -> Div {
    div().w_full().min_w_0().flex_none().flex().flex_row().items_start().gap(px(SPACE_XS)).child(book_card_title().child(book_title_lines(title, subtitle))).children(download)
}

/// A detailed card's two columns: the cover, then the words beside it.
///
/// The row's height is the cover's, and the text column stretches to it — which
/// is what gives the blurb a definite box to fill and be clipped by.
pub fn book_card_detail_body() -> Div {
    div().w_full().min_w_0().flex().flex_row().gap(px(crate::BOOK_CARD_DETAIL_GAP))
}

/// The left half of a detailed card, which is the ordinary card entire: cover,
/// then title.
///
/// The cover shrinks with narrow cards and stops at its preferred width.
/// Extra card width belongs to the description.
///
/// The words stay under the cover rather than moving in beside it, for
/// scanning: a title set directly above a paragraph of the same measure has a
/// weak boundary, and reading the titles across a row would mean passing over
/// the prose between them. Under the covers every title falls in the same place
/// on every card — the same place it holds in covers mode, so switching modes
/// no longer moves it — and the blurb is left as one uninterrupted block.
pub fn book_card_detail_cover() -> Div {
    div().w(gpui::relative(crate::BOOK_CARD_DETAIL_COVER_FRACTION)).max_w(px(crate::BOOK_CARD_CONTENT_WIDTH)).flex_none().flex().flex_col().gap(px(BOOK_CARD_COVER_CONTENT_GAP))
}

/// The right half: the blurb and nothing else, filling the card's full height.
pub fn book_card_detail_text() -> Div {
    div().flex_1().min_w_0().flex().flex_col()
}

/// The blurb, filling whatever the title leaves and clipped there.
///
/// Takes the description as stored — which is HTML — and sets what a card can
/// use of it: paragraphs as line breaks, emphasis as emphasis, entities as the
/// characters they name. See [`crate::description`] for why this is not the
/// document renderer.
///
/// Set in the author's colour rather than the title's: it is supporting text,
/// and a paragraph at full strength would outweigh the title it sits under.
pub fn book_card_description(id: impl Into<ElementId>, description: &str, theme: BrowserTheme) -> Option<Stateful<Div>> {
    let (text, highlights) = crate::description::rich_text(description);
    if text.is_empty() {
        return None;
    }
    Some(
        div()
            .id(id)
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .cursor_pointer()
            .text_size(gpui::rems(crate::TEXT_SM))
            .line_height(gpui::relative(crate::BOOK_CARD_DESCRIPTION_LINE_HEIGHT_EM))
            .text_color(theme.text_muted)
            .child(crate::description::BlurbText::new(text, highlights)),
    )
}

/// The full blurb, opened from the card that could only show the top of it.
///
/// A panel rather than a page: the card it came from stays where it was, so
/// closing it returns the reader to the same spot in the grid. It holds its own
/// wheel — see the card, which stops the event reaching the paginator, since
/// otherwise reading past the fold would turn the page underneath.
///
/// The panel is as tall as the blurb needs and no taller, up to a share of the
/// window. That bound is a *maximum*, so its own height is content-driven —
/// which is why the body inside it must keep an automatic flex basis. Given
/// `flex_1`, the body's basis is zero, and a zero basis inside an
/// auto-height column resolves to a zero-height box: the heading draws, the
/// blurb does not, and nothing about the panel looks wrong.
pub fn book_description_panel(theme: BrowserTheme) -> Div {
    crate::modal_surface(theme).w(gpui::rems(crate::BOOK_DESCRIPTION_OVERLAY_WIDTH_REM)).max_w_full().max_h(gpui::relative(crate::BOOK_DESCRIPTION_OVERLAY_HEIGHT_FRACTION)).flex().flex_col()
}

/// Holds the blurb between the heading and the bottom of the panel.
///
/// Shrinkable rather than growable — see [`book_description_panel`] — and
/// `min_h_0` so it may be shrunk below its content, which is what gives the
/// scroll area a bounded height to scroll inside.
pub fn book_description_scroll_area() -> Div {
    div().min_h_0().flex_shrink(1.0)
}

/// The panel's heading: the book the blurb belongs to, so an overlay opened
/// from a grid of covers still says which one it is about.
pub fn book_description_heading(title: impl Into<SharedString>, subtitle: Option<&str>, author: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    let author = author.into();
    div()
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(SPACE_XXS))
        .p(px(crate::SPACE_MD))
        .border_b_1()
        .border_color(theme.rule)
        .child(div().text_size(gpui::rems(CAROUSEL_TITLE_SIZE)).font_weight(BOOK_CARD_TITLE_WEIGHT).child(title.into()))
        .children(subtitle.map(str::trim).filter(|value| !value.is_empty()).map(|subtitle| div().text_size(gpui::rems(BOOK_CARD_TITLE_SIZE)).font_weight(gpui::FontWeight::NORMAL).child(subtitle.to_owned())))
        .children((!author.is_empty()).then(|| div().text_size(gpui::rems(TEXT_XS)).text_color(theme.text_muted).child(author)))
}

/// The blurb in full: no clamp and no ellipsis, because whatever does not fit
/// is reached by scrolling rather than cut off.
pub fn book_description_body(description: &str, theme: BrowserTheme) -> Div {
    let (text, highlights) = crate::description::rich_text(description);
    div()
        .w_full()
        .p(px(crate::SPACE_MD))
        .text_size(gpui::rems(crate::TEXT_MD))
        .line_height(gpui::relative(crate::BOOK_DESCRIPTION_OVERLAY_LINE_HEIGHT_EM))
        .text_color(theme.text)
        .child(gpui::StyledText::new(text).with_highlights(highlights))
}

pub fn book_download_indicator(downloaded: bool, pending: bool, theme: BrowserTheme) -> Option<AnyElement> {
    if downloaded {
        return None;
    }
    // Pending is a status, not an action, and the card does not make it
    // clickable — so it stays an unframed glyph. Only the state you can act on
    // gets the button treatment.
    if pending {
        return Some(
            div()
                .w(px(crate::BOOK_CARD_ACTION_SIZE))
                .h(px(crate::BOOK_CARD_ACTION_SIZE))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(Spinner::new().icon(IconName::LoaderCircle).xsmall().color(theme.text_muted))
                .into_any_element(),
        );
    }
    Some(
        div()
            .w(px(crate::BOOK_CARD_ACTION_SIZE))
            .h(px(crate::BOOK_CARD_ACTION_SIZE))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .border_1()
            .border_color(theme.rule)
            .child(Icon::empty().path("icons/cloud-download.svg").size(gpui::rems(crate::BOOK_CARD_ACTION_ICON_SIZE / 16.0)).text_color(theme.text))
            .into_any_element(),
    )
}

/// A page of shelves — Home and the author index are the same object, so they
/// are the same container: rows of a paginator rather than a scrolling column.
/// The paginator supplies the vertical extent, so there is nothing here but the
/// page's own inset.
pub fn shelf_page() -> Div {
    crate::full_size_column().p(px(crate::SPACE_MD))
}

/// One row: the heading, then the shelf it names.
///
/// A whole row per shelf is what lets the width of the page go to books — the
/// heading takes a line rather than a column beside the covers.
pub fn shelf_row() -> Div {
    div().w_full().min_w_0().h_full().flex().flex_col().gap(px(CAROUSEL_CONTENT_GAP))
}

/// The line over a shelf, split down the middle: what the shelf is in the left
/// half, what it is about in the right, and its controls at the far end.
///
/// Inset by `CONTENT_INSET` so the title begins on the same line as the first
/// cover beneath it. The covers carry that inset as card padding — the heading
/// has to match it rather than sit flush, or every title hangs a card's padding
/// to the left of its own row.
///
/// Both halves are `flex_1` rather than a declared 50%, so the gap between them
/// comes out of the middle and the split holds at every window width.
///
/// Its height is stated rather than left to the font because the row's height is
/// computed from it — see `shelf_row_height_rem`. `subject_rows` is how many rows
/// of chips it has to hold: a heading with chips is as tall as they need, so the
/// name and the subjects beside it end up the same height.
pub fn shelf_heading(subject_rows: usize) -> Div {
    div().w_full().min_w_0().flex_none().h(gpui::rems(crate::shelf_heading_height_rem(subject_rows))).flex().flex_row().items_center().px(px(crate::CONTENT_INSET)).gap(px(CAROUSEL_CONTENT_GAP))
}

/// What the shelf is — a section name on Home, an author's name on the index —
/// at the head of the row, tinted when the keyboard cursor is on this row.
/// The title takes only the width it needs so its chips can follow it directly,
/// but never more than half the heading: a longer one is elided there rather
/// than pushing the chips off the row.
pub fn shelf_title(title: impl Into<SharedString>, selected: bool, theme: BrowserTheme) -> Div {
    div()
        .flex_none()
        .max_w(gpui::relative(0.5))
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_size(gpui::rems(CAROUSEL_TITLE_SIZE))
        .font_weight(CAROUSEL_TITLE_WEIGHT)
        .when(selected, |title| title.text_color(theme.accent))
        .child(title.into())
}

/// What the shelf is about, filling the heading from where its title ends.
///
/// The chips follow the title directly rather than waiting for the midpoint, so
/// a short name is not padded out to one. They wrap: the heading is as many chip
/// rows tall as the page declares, and a name with more subjects than fit on one
/// line continues on the next instead of losing them off the end.
///
/// What still does not fit is clipped, because the heading is the budget and a
/// row that grew to fit its chips would no longer be one card tall. The rows are
/// centred rather than top-aligned so a single row still sits on the title's
/// line, and a shelf with nothing to qualify — a Home section — leaves the
/// remainder empty.
pub fn shelf_subjects(subjects: impl IntoIterator<Item = (Option<SharedString>, SharedString)>, theme: BrowserTheme) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_row()
        .flex_wrap()
        .content_center()
        .items_center()
        .gap_x(px(SPACE_XS))
        .gap_y(gpui::rems(crate::SHELF_SUBJECT_ROW_GAP_REM))
        .overflow_hidden()
        .children(subjects.into_iter().map(|(parent, label)| subject_chip(parent, label, theme)))
}

/// The pill that follows the pointer while items are being dragged. It names
/// what is being carried rather than miniaturising it: a half-transparent copy
/// of three book covers reads as a rendering fault, and a count does not.
pub fn drag_preview(label: SharedString, theme: BrowserTheme) -> Div {
    div().px(px(SPACE_SM)).h(px(28.0)).flex().items_center().rounded(px(RADIUS_SM)).bg(theme.accent).text_color(theme.accent_text).text_size(gpui::rems(TEXT_XS)).child(label)
}

/// One subject chip: a quiet outline, because it states what a shelf is about
/// rather than offering somewhere to go.
///
/// A parent is stated ahead of the label and set a shade quieter, because many
/// leaves mean nothing alone — "Communities", "20th century", "Sources" — while
/// the eye should still land on the word the chip is about. A top-level subject
/// passes `None` and reads as the bare word it already is.
///
/// A chip is as wide as what it says. It is not elided and does not shrink: a
/// subject is a short phrase, and half of one is worse than the row it costs —
/// where chips no longer fit across, they wrap onto the next row.
///
/// Its height is stated rather than left to text plus padding, because the
/// heading stacks chips and has to know how tall a row of them is.
pub fn subject_chip(parent: Option<SharedString>, label: SharedString, theme: BrowserTheme) -> Div {
    div()
        .flex_none()
        .whitespace_nowrap()
        .h(gpui::rems(crate::SHELF_SUBJECT_CHIP_HEIGHT_REM))
        .flex()
        .flex_row()
        .items_center()
        .px(px(crate::CONTENT_INSET))
        .border_1()
        .border_color(theme.rule)
        .rounded(px(RADIUS_SM))
        .text_size(gpui::rems(SHELF_SUBJECT_SIZE))
        .text_color(theme.text)
        .children(parent.map(|parent| div().flex_none().text_color(theme.text_muted).child(format!("{parent} \u{203a} "))))
        .child(label)
}

/// The shelf under the heading, taking whatever the heading leaves.
pub fn shelf_carousel() -> Div {
    div().w_full().min_w_0().flex_1().min_h_0()
}

pub fn paginator_controls(previous: Option<AnyElement>, next: Option<AnyElement>) -> Div {
    div().flex_none().flex().flex_row().items_center().gap(px(CAROUSEL_CONTROL_GAP)).children(previous).children(next)
}

/// Breadcrumbs use the medium body token. Items own the vertical padding so
/// the trail stays aligned with the action buttons in the topbar.
///
/// The gap is zeroed against the toolkit's `gap_1p5`, which would otherwise add
/// 6px to the padding each crumb already carries — a chevron only 3.5 units
/// wide, floating in more air than it separates. Crumb padding is then the
/// single owner of the trail's rhythm: change it in [`folder_breadcrumb_item`]
/// and the label, its hit area and its distance from the separator all move
/// together.
pub fn folder_breadcrumb() -> Breadcrumb {
    Breadcrumb::new().text_size(gpui::rems(crate::TEXT_MD)).gap_0()
}

/// Holds the trail in a topbar too narrow for it, and decides which end of the
/// trail survives that.
///
/// The current folder is what the trail is *for*, so it is the crumb that must
/// stay on screen; the root is the one the reader already knows. A left-anchored
/// row (which is what a horizontal scroll surface gives you, parked at offset 0)
/// keeps the wrong end and pushes the leaf out of sight, so overflow is taken
/// off the start instead: the row is clipped and justified to the end, and the
/// crumbs slide left under the topbar's edge as the path deepens.
///
/// The spacer is what keeps that from right-aligning a trail that fits. It
/// absorbs the free space while there is any, leaving justification nothing to
/// do; once the trail overflows it collapses and the end-justification takes
/// over. The trail itself never shrinks — crumbs are elided by character count
/// in [`folder_breadcrumb_item`], not squeezed.
pub fn folder_breadcrumb_trail(breadcrumb: Breadcrumb) -> Div {
    div().w_full().min_w_0().overflow_x_hidden().flex().items_center().justify_end().child(breadcrumb.flex_none()).child(div().flex_1().min_w_0())
}

/// The trail recedes and the current folder does not: ancestors are muted, the
/// last crumb keeps full ink. Weight used to carry this, but the interface has
/// no semibold face — see the type scale in `tokens.rs`.
///
/// The horizontal padding is `SPACE_XS` rather than the `CONTENT_INSET` the
/// rest of the browser is inset by, and it is the only thing between a label
/// and its chevron — see [`folder_breadcrumb`], which zeroes the toolkit's gap.
/// A chevron is a separator, not a neighbour: it belongs to the pair of crumbs
/// around it, and at the inset it reads as an item of its own.
pub fn folder_breadcrumb_item(label: impl Into<SharedString>, current: bool, theme: BrowserTheme) -> BreadcrumbItem {
    let label = label.into();
    let max_characters = 24;
    let mut characters = label.chars();
    let visible = characters.by_ref().take(max_characters).collect::<String>();
    let visible = if characters.next().is_some() { format!("{}…", visible.chars().take(max_characters.saturating_sub(1)).collect::<String>()) } else { visible };
    BreadcrumbItem::new(visible).px(px(SPACE_XS)).py(px(SPACE_XS)).text_color(if current { theme.text } else { theme.text_muted })
}

/// The first crumb, set on the left edge of the covers below it.
///
/// Nothing above it carries a horizontal inset — the topbar and the column it
/// sits in are both unpadded — so this padding is the whole of the distance
/// between the rail and the trail, and `CONTENT_INSET` is where a book cover
/// starts: the card grid is flush with the column and the cover sits one
/// `BOOK_CARD_PADDING` inside its card. The two are the same token for exactly
/// this reason. Only the leading edge takes it; the trailing edge keeps the
/// tighter padding the chevrons are spaced on.
pub fn folder_breadcrumb_root_item(label: impl Into<SharedString>, current: bool, theme: BrowserTheme) -> BreadcrumbItem {
    folder_breadcrumb_item(label, current, theme).pl(px(crate::CONTENT_INSET))
}

/// Natural chip width, including the action slot only when it is shown.
/// Use the same text and icon layout as the rendered chip so font changes
/// affect wrapping without estimating label widths from character counts.
pub fn compact_hierarchy_item_width(icon: crate::NavigationIcon, label: SharedString, count: Option<usize>, show_download: bool, window: &mut gpui::Window, cx: &mut gpui::App) -> gpui::Pixels {
    let mut content = div()
        .flex()
        .items_center()
        .whitespace_nowrap()
        .text_size(gpui::rems(crate::COMPACT_HIERARCHY_ITEM_TEXT_SIZE_REM))
        .gap(px(crate::COMPACT_HIERARCHY_ITEM_CONTENT_GAP))
        .child(crate::navigation_icon(icon))
        .child(div().flex_none().child(label))
        .children(count.map(|count| div().flex_none().child(count.to_string())))
        .when(show_download, |content| content.child(div().w(px(crate::BOOK_CARD_ACTION_SIZE)).flex_none()))
        .into_any_element();
    let measured = content.layout_as_root(gpui::size(gpui::AvailableSpace::MaxContent, gpui::AvailableSpace::MaxContent), window, cx);
    measured.width + px(crate::COMPACT_HIERARCHY_ITEM_HORIZONTAL_PADDING * 2.0 + 2.0)
}

/// A compact hierarchy item for mixed paginator pages.
///
/// This function owns presentation only. Feature code remains responsible for
/// attaching click handlers and navigation behavior.
pub fn compact_hierarchy_item(
    id: impl Into<ElementId>, icon: crate::NavigationIcon, label: impl Into<SharedString>, path: Option<SharedString>, count: Option<usize>, download: Option<AnyElement>, focused: bool, marquee: bool, theme: BrowserTheme,
) -> Button {
    let label = label.into();
    let text =
        div().relative().flex_1().min_w_0().h_full().child(div().size_full().min_w_0().flex().items_center().child(div().w_full().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(label))).children(path.map(|path| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(gpui::relative(0.5))
                .min_w_0()
                .flex()
                .items_center()
                .child(div().w_full().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis_start().text_size(gpui::rems(0.5)).text_color(theme.text_muted).child(path))
        }));
    // A focused chip tints to `accent_wash` and keeps every foreground it had.
    //
    // `selected(true)` is what makes the hover reachable at all: a `Button`
    // writes its own hover slot unless it is selected, and gpui allows that slot
    // to be written once. Claiming it is the only way to say that a tinted chip
    // lifts rather than darkens, and the only way for `marquee` to say nothing
    // at all. See `selectable_button`, which takes the same state for the same
    // reason.
    //
    // Hover stays on the shared sidebar/list tone so chips and navigation rows
    // read as the same pointer state.
    let hovered = theme.hover;
    let base = if focused { theme.accent_wash } else { theme.page_bg };
    let button = crate::base_button(id).ghost().selected(true).bg(base).hover(move |style| if marquee { style.bg(base) } else { style.bg(hovered) });
    button.w_full().max_w_full().h_full().p_0().px(px(crate::COMPACT_HIERARCHY_ITEM_HORIZONTAL_PADDING)).rounded(px(0.0)).border_1().border_color(theme.rule).child(
        div()
            .size_full()
            .min_w_0()
            .text_size(gpui::rems(crate::COMPACT_HIERARCHY_ITEM_TEXT_SIZE_REM))
            .flex()
            .items_center()
            .gap(px(crate::COMPACT_HIERARCHY_ITEM_CONTENT_GAP))
            .child(crate::navigation_icon(icon).text_color(theme.accent))
            .child(text)
            .children(count.map(|count| div().flex_none().text_color(theme.text_muted).child(count.to_string())))
            .when_some(download, |content, download| content.child(div().w(px(crate::BOOK_CARD_ACTION_SIZE)).h(px(crate::BOOK_CARD_ACTION_SIZE)).flex_none().child(download))),
    )
}
