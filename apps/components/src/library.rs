use crate::{
    BOOK_CARD_SUBTITLE_SIZE, BOOK_COVER_BADGE_INSET, BOOK_COVER_RADIUS, COMPACT_HIERARCHY_ITEM_CONTENT_GAP, COMPACT_HIERARCHY_ITEM_HORIZONTAL_PADDING, COMPACT_HIERARCHY_ITEM_TEXT_SIZE_REM, CONTENT_INSET, DOWNLOAD_ACTION_SIZE_REM,
    NavigationIcon, SPACE_MD, TEXT_LG, TEXT_MD,
};
use gpui::prelude::*;
use gpui::{AnyElement, App, AvailableSpace, BoxShadow, Div, ElementId, ImageSource, ObjectFit, Pixels, SharedString, Stateful, StyledImage, Window, black, canvas, div, img, px, size};
use gpui_component::breadcrumb::{Breadcrumb, BreadcrumbItem};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::spinner::Spinner;
use gpui_component::{Icon, IconName, Selectable, Sizable, box_shadow};
use unicode_normalization::UnicodeNormalization;

use crate::{
    BOOK_CARD_COVER_CONTENT_GAP, BOOK_CARD_TEXT_LINES, BOOK_CARD_TITLE_HEIGHT_REM, BOOK_CARD_TITLE_LINE_HEIGHT_EM, BOOK_CARD_TITLE_SIZE, BOOK_CARD_TITLE_WEIGHT, BOOK_COVER_ASPECT_RATIO, BrowserTheme, RADIUS_MD, RADIUS_SM, SPACE_SM,
    SPACE_XS, TEXT_SM, TEXT_XS,
};

/// Cover, then the text block. The two levels each have one gap: `SPACE_MD`
/// separates the art from the words, `SPACE_XS` separates the words from each
/// other. Previously the wider first gap was a margin on the title fighting the
/// column's own gap, which meant two spacing mechanisms in one component.
/// Selected cards draw an accent tint. The keyboard cursor gets a contrasting
/// outline around the entire card, independently of selection.
///
/// It used to invert: `accent` with `accent_text` over it. An inversion has to
/// reach the edges of its cell or it reads as a hole in the fill, which is why
/// the grid ran at a zero gap and the card carried the spacing as its own
/// padding. A tint does not — so the gap came back (`BOOK_GRID_BOOK_GAP`) and
/// the padding became the tile's inset instead of the spacer between cards.
///
/// Hover depends on whether the card is already selected, because gpui's hover
/// style *replaces* `background` rather than painting over it. On a plain card
/// the translucent `hover` composites against the page, which is what it is
/// for; on a tinted one it would composite against the page too and throw the
/// tile away, so the card would stop looking selected exactly when the pointer
/// reached it. A selected card hovers to `accent_wash_hover` instead — the same
/// tint, lifted.
///
/// Disable hover while selecting a band or when a section's outer keyboard
/// frame already paints hover for the whole cell.
pub fn book_card(id: impl Into<ElementId>, focused: bool, selected: bool, hover_enabled: bool, theme: BrowserTheme) -> Stateful<Div> {
    let hovered = if selected { theme.accent_wash_hover } else { theme.hover };
    div()
        .id(id)
        .relative()
        .w_full()
        .min_w_0()
        .flex_none()
        .flex()
        .flex_col()
        // Reserve the outline inside the existing padding so moving the
        // cursor does not resize covers or alter pagination.
        .border_2()
        .border_color(if focused {
            theme.accent
        } else if selected {
            theme.border
        } else {
            gpui::transparent_black()
        })
        .p(px((crate::BOOK_CARD_PADDING - 2.0).max(0.0)))
        .gap(px(BOOK_CARD_COVER_CONTENT_GAP))
        .rounded(px(RADIUS_MD))
        .when(selected, |card| card.bg(theme.accent_wash))
        .when(hover_enabled, |card| card.hover(move |style| style.bg(hovered)))
}

/// The grid cell a card sits in, at the row's uniform reserved height.
///
/// Carries no background of its own — a card's cover is boxed at its own
/// real proportions rather than the placeholder's, so `book_card` (which does
/// carry the focus tint) comes out shorter or taller than the cell. This cell
/// is plain space around it. The default `justify_end` pins grid and section
/// cards to the bottom edge; graph covers override it to align at the top.
pub fn book_card_cell() -> Div {
    div().relative().w_full().h_full().min_w_0().flex().flex_col().justify_end()
}

/// The text block under a card's cover — see `book_card_title_row`. Reserves
/// the title budget the paginator's row height is computed from, so a title
/// without a subtitle carries its slack below itself rather than shortening
/// the card.
pub fn book_card_text_title_only() -> Div {
    div().w_full().min_w_0().flex_none().flex().flex_col().h(gpui::rems(BOOK_CARD_TITLE_HEIGHT_REM))
}

/// `aspect_ratio` is the cover's own width-over-height once its art is
/// decoded, so the box takes the real cover's proportions instead of every
/// book being forced into the same frame. Callers without a decoded image yet
/// pass `BOOK_COVER_ASPECT_RATIO` as a placeholder.
pub fn book_cover(_theme: BrowserTheme, aspect_ratio: f32) -> Div {
    div().relative().w_full().min_w_0().aspect_ratio(aspect_ratio)
}

pub fn book_cover_frame(theme: BrowserTheme, aspect_ratio: f32) -> Div {
    crate::clickable_surface(theme).group(BOOK_COVER_GROUP).relative().w_full().min_w_0().aspect_ratio(aspect_ratio)
}

const BOOK_COVER_GROUP: &str = "book-cover";

/// A centered transport action revealed while the pointer is over the cover.
pub fn book_cover_play_button(id: impl Into<ElementId>, theme: BrowserTheme) -> Stateful<Div> {
    book_cover_badge_plate(theme)
        .id(id)
        .absolute()
        .left(gpui::relative(0.5))
        .top(gpui::relative(0.5))
        .ml(gpui::rems(-1.25))
        .mt(gpui::rems(-1.25))
        .size(gpui::rems(2.5))
        .rounded_full()
        .cursor_pointer()
        .invisible()
        .group_hover(BOOK_COVER_GROUP, |style| style.visible())
        .hover(move |style| style.bg(theme.accent_wash_hover))
        .child(Icon::empty().path("icons/play-fill.svg").size(gpui::rems(1.25)).text_color(theme.text))
}

/// A compact length mark that also identifies an audiobook by its headphones.
pub fn book_cover_audiobook_length(length: Option<impl Into<SharedString>>, theme: BrowserTheme) -> Div {
    div()
        .absolute()
        .left(px(8.0))
        .bottom(px(8.0))
        .flex()
        .items_center()
        .gap(px(5.0))
        .px(px(7.0))
        .py(px(5.0))
        .bg(theme.page_bg)
        .border_1()
        .border_color(theme.rule)
        .text_color(theme.text)
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(Icon::new(crate::Headphones).size(px(11.0)))
        .children(length.map(|length| length.into()))
}

/// Cover art scaled to fit its frame without cropping.
///
/// Scaled rather than cropped: a cover's own proportions are part of what it
/// is, and a square audiobook cover cropped to a paperback's 2:3 would lose
/// its edges rather than just sit in a taller frame than most books need.
///
/// A small neutral shadow lifts the jacket without making a larger coloured
/// rectangle around it. The outline itself stays invisible except when
/// focus or selection picks it out in the accent colour — a hover border
/// read as a stray black line rather than a lift.
/// A shaded strip down the binding edge gives the front board a little shape.
///
/// Only the outer edge rounds. A jacket's spine side is a flat fold against
/// the pages, not a curve, so rounding it along with the outer corners would
/// contradict the spine shade and hinge sitting right next to it.
pub fn book_cover_image(id: impl Into<SharedString>, source: impl Into<ImageSource>, opacity: f32, focused: bool, theme: BrowserTheme) -> AnyElement {
    let id = id.into();
    div()
        .id(ElementId::from(id.clone()))
        .relative()
        .size_full()
        .overflow_hidden()
        .rounded_r(px(BOOK_COVER_RADIUS))
        .border_1()
        .border_color(if focused { theme.accent } else { gpui::transparent_black() })
        .shadow(book_cover_shadow(theme, false))
        .hover(move |style| style.shadow(book_cover_shadow(theme, true)))
        // Focus lifts the cover as a supporting cue to the selected card.
        .when(focused, |cover| cover.shadow(book_cover_shadow(theme, true)))
        // `overflow_hidden` on the wrapping div clips with a rectangular
        // mask regardless of its own corner radius — an image paints its
        // corners from its own style, so the rounding has to be set here too.
        .child(img(source).size_full().object_fit(ObjectFit::Contain).opacity(opacity).rounded_r(px(BOOK_COVER_RADIUS)))
        .when(!cfg!(feature = "kobo"), |cover| cover.child(book_cover_spine()).child(book_cover_hinge()))
        .into_any_element()
}

/// The palette's general overlay shadow can be gold in dark mode. Covers
/// instead cast a short neutral shadow; the alpha still follows the theme so
/// e-ink keeps its zero-shadow treatment.
fn book_cover_shadow(theme: BrowserTheme, lifted: bool) -> Vec<BoxShadow> {
    let mut color = black();
    color.a = theme.shadow.a * if lifted { 1.2 } else { 0.8 };
    let (offset, blur) = if lifted { (4.0, 12.0) } else { (2.0, 6.0) };
    vec![box_shadow(px(0.0), px(offset), px(blur), px(0.0), color)]
}

/// The gradient strip `book_cover_image` shades its leading edge with.
///
/// Painted as the image's last child so it lands above the art rather than
/// under it — an opaque cover would otherwise hide anything drawn first.
fn book_cover_spine() -> gpui::Div {
    div().absolute().left_0().top_0().bottom_0().w(px(crate::BOOK_COVER_SPINE_WIDTH)).bg(gpui::linear_gradient(90.0, gpui::linear_color_stop(gpui::black().opacity(0.35), 0.0), gpui::linear_color_stop(gpui::transparent_black(), 1.0)))
}

/// The shallow hinge groove just inside the spine shade. Its recessed line is
/// centered at 12 px; a faint catchlight on the board side keeps it legible on
/// dark jacket artwork without drawing a full bright outline.
fn book_cover_hinge() -> gpui::Div {
    // Four two-stop bands reconstruct the original five-point curve exactly —
    // `linear_gradient` here takes only one `from` and one `to`.
    let stops = [gpui::transparent_black(), gpui::black().opacity(0.12), gpui::black().opacity(0.28), gpui::white().opacity(0.14), gpui::transparent_black()];
    let band = |left: f32, from: gpui::Hsla, to: gpui::Hsla| div().absolute().left(px(left)).top_0().bottom_0().w(px(1.0)).bg(gpui::linear_gradient(90.0, gpui::linear_color_stop(from, 0.0), gpui::linear_color_stop(to, 1.0)));
    div().absolute().left(px(10.0)).top_0().bottom_0().w(px(4.0)).children((0..4).map(|index| band(index as f32, stops[index], stops[index + 1])))
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
/// greyscale — but e-ink's accent is black, which the scrim swallows, so there
/// the bar runs on a page-coloured track.
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
            .when(!cfg!(feature = "kobo"), |cover| cover.child(div().absolute().inset_0().bg(gpui::linear_gradient(0.0, gpui::linear_color_stop(gpui::black().opacity(0.6), 0.0), gpui::linear_color_stop(gpui::transparent_black(), 1.0)))))
            .child(div().relative().h(px(crate::BOOK_COVER_PROGRESS_HEIGHT)).flex().flex_row().when(cfg!(feature = "kobo"), |track| track.bg(theme.page_bg)).child(div().w(gpui::relative(fraction)).h_full().bg(theme.accent)))
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
        .rounded_r(px(BOOK_COVER_RADIUS))
        .border_1()
        .border_color(theme.rule)
        .bg(theme.page_bg)
        .gap(px(SPACE_XS))
        .child(Icon::new(IconName::BookOpen).large().text_color(theme.text_muted))
        .child(div().text_size(gpui::rems(TEXT_XS)).text_color(theme.text_muted).child(label.into()))
}

/// Fixed book-cloth colours shared by generated covers and section headings.
/// They stay the same when the reader switches themes.
const LIBRARY_GRADIENTS: [(u32, u32); 12] = [
    (0x794e5c, 0x5b3b45), // burgundy
    (0x8b453f, 0x68342f), // brick
    (0x955341, 0x703e31), // terracotta
    (0x806335, 0x604a28), // amber
    (0x666b37, 0x4d5029), // olive
    (0x5c7045, 0x455434), // sage
    (0x48715a, 0x365544), // forest
    (0x3d7374, 0x2e5657), // teal
    (0x4b698e, 0x384f6b), // blue
    (0x566777, 0x414d59), // slate
    (0x62618a, 0x4a4968), // indigo
    (0x795278, 0x5b3e5a), // plum
];

/// A repeatable palette choice for a folder name or canonical subject path.
/// Keep the palette order and this hash fixed once colours are in use: changing
/// either would recolour existing folders and subjects.
pub fn section_card_band_index(kind: NavigationIcon, key: &str) -> usize {
    let mut hash = 0xcbf29ce484222325_u64;
    let namespace = match kind {
        NavigationIcon::Folder => b'F',
        NavigationIcon::User => b'A',
        NavigationIcon::House => b'H',
        _ => b'S',
    };
    hash = (hash ^ u64::from(namespace)).wrapping_mul(0x100000001b3);
    let normalized = key.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase().nfc().collect::<String>();
    for byte in normalized.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    // Avalanche the FNV result so neighbouring names do not favour nearby
    // palette slots, including when the slot count has small factors.
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51afd7ed558ccd);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xc4ceb9fe1a85ec53);
    hash ^= hash >> 33;
    // Reduce before converting to usize so 32- and 64-bit devices agree.
    (hash % LIBRARY_GRADIENTS.len() as u64) as usize
}

#[cfg_attr(feature = "kobo", allow(dead_code))]
fn section_card_band_colors(index: usize) -> (gpui::Hsla, gpui::Hsla) {
    #[cfg(feature = "kobo")]
    let (from, to) = (0x000000, 0x333333);
    #[cfg(not(feature = "kobo"))]
    let (from, to) = LIBRARY_GRADIENTS[index % LIBRARY_GRADIENTS.len()];
    #[cfg(feature = "kobo")]
    let _ = index;
    (gpui::rgb(from).into(), gpui::rgb(to).into())
}

/// Grounds for generated covers: ten hues spaced in OKLCH at alternating
/// lightness, plus indigo, charcoal, raspberry and teal. The closest pair
/// (moss and forest) is 0.065 apart in OKLab. Keep the order fixed once in
/// use: it decides which colour every coverless book gets.
#[cfg(not(feature = "kobo"))]
const COVER_GROUNDS: [u32; 14] = [
    0x461307, // oxblood
    0x503401, // bronze
    0x344e1d, // moss
    0x0d401e, // forest
    0x00291a, // racing green
    0x013741, // peacock
    0x05436c, // sapphire
    0x00183f, // navy
    0x3e315f, // amethyst
    0x3e1332, // mulberry
    0x261d57, // indigo
    0x23171a, // charcoal
    0x7b3548, // raspberry
    0x055c5c, // teal
];
#[cfg(not(feature = "kobo"))]
const COVER_GOLD: u32 = 0xc9a24a;
#[cfg(not(feature = "kobo"))]
const COVER_INK: u32 = 0xefe9dc;

/// E-ink shows 16 greys, so hues would collapse into a few near-blacks, and a
/// dark ground is the most ink to drive and to clear on every refresh. Every
/// Kobo cover is plain white paper with black lines and type; variety comes
/// from the motif and ornament instead.
#[cfg(feature = "kobo")]
const COVER_GROUNDS: [u32; 1] = [0xffffff];
#[cfg(feature = "kobo")]
const COVER_GOLD: u32 = 0x000000;
#[cfg(feature = "kobo")]
const COVER_INK: u32 = 0x000000;

#[derive(Clone, Copy)]
enum CoverOrnament {
    Bars,
    Lozenge,
    Chevrons,
}

/// The line pattern behind the frame. Rays fall from the top; chevrons,
/// lattice and hexagons cover the whole background; ziggurat and skyline stand
/// at the name plate and reach up to the title panel.
#[derive(Clone, Copy)]
enum CoverMotif {
    Rays,
    Chevrons,
    Ziggurat,
    Lattice,
    Skyline,
    Hexagons,
}

/// The art deco design a coverless book gets. Each choice reads its own digit
/// of `variant`, so they vary independently: ground (desktop only), motif and
/// ornament — 14 × 6 × 3 = 252 covers on desktop, 6 × 3 = 18 on Kobo. Keep the
/// order of every list fixed once in use: it decides which cover a book gets.
#[derive(Clone, Copy)]
struct CoverDesign {
    ground: u32,
    motif: CoverMotif,
    ornament: CoverOrnament,
}

impl CoverDesign {
    fn new(variant: u64) -> Self {
        let mut digits = variant;
        let mut digit = |radix: usize| {
            let value = digits % radix as u64;
            digits /= radix as u64;
            value as usize
        };
        let ground = COVER_GROUNDS[digit(COVER_GROUNDS.len())];
        let motif = [CoverMotif::Rays, CoverMotif::Chevrons, CoverMotif::Ziggurat, CoverMotif::Lattice, CoverMotif::Skyline, CoverMotif::Hexagons][digit(6)];
        let ornament = [CoverOrnament::Bars, CoverOrnament::Lozenge, CoverOrnament::Chevrons][digit(3)];
        Self { ground, motif, ornament }
    }
}

/// The book's own words on a generated cover; the author is already uppercase.
struct CoverLettering {
    title: SharedString,
    subtitle: Option<SharedString>,
    author: SharedString,
}

/// Shapes `text` in Raleway to wrap at `width`, keeping at most `lines` lines.
fn shape_cover_text(text: &SharedString, font: gpui::Font, color: gpui::Hsla, font_size: f32, width: f32, lines: usize, window: &Window) -> Vec<gpui::WrappedLine> {
    let run = gpui::TextRun { len: text.len(), font, color, background_color: None, underline: None, strikethrough: None };
    window.text_system().shape_text(text.clone(), px(font_size), &[run], Some(px(width)), Some(lines)).map(|shaped| shaped.into_iter().collect()).unwrap_or_default()
}

fn cover_text_height(lines: &[gpui::WrappedLine], line_height: f32) -> f32 {
    lines.iter().map(|line| f32::from(line.size(px(line_height)).height)).sum()
}

/// Paints shaped lines centred in a column `width` wide starting at `origin`.
fn paint_cover_text(lines: &[gpui::WrappedLine], origin: gpui::Point<Pixels>, width: f32, line_height: f32, window: &mut Window, cx: &mut App) {
    let mut y = origin.y;
    for line in lines {
        let at = gpui::point(origin.x, y);
        let _ = line.paint(at, px(line_height), gpui::TextAlign::Center, Some(gpui::Bounds::new(at, size(px(width), px(line_height)))), window, cx);
        y += line.size(px(line_height)).height;
    }
}

/// The ornament above the title, hanging from `top_centre`. `unit` is one
/// percent of the cover's width, the unit every measurement here uses.
fn paint_cover_ornament(ornament: CoverOrnament, top_centre: gpui::Point<Pixels>, unit: f32, line: f32, window: &mut Window) {
    let gold = gpui::rgb(COVER_GOLD);
    let at = |x: f32, y: f32| top_centre + gpui::point(px(x * unit), px(y * unit));
    let polylines = match ornament {
        CoverOrnament::Bars => {
            for (index, length) in [16.0_f32, 11.0, 6.0].into_iter().enumerate() {
                let origin = top_centre + gpui::point(px(-length * unit / 2.0), px(index as f32 * (line + 1.5 * unit)));
                window.paint_quad(gpui::fill(gpui::Bounds::new(origin, size(px(length * unit), px(line))), gold));
            }
            return;
        }
        // A rule, a diamond between gaps, and a rule.
        CoverOrnament::Lozenge => vec![vec![at(-11.4, 2.4), at(-4.4, 2.4)], vec![at(0.0, 0.0), at(2.4, 2.4), at(0.0, 4.8), at(-2.4, 2.4), at(0.0, 0.0)], vec![at(4.4, 2.4), at(11.4, 2.4)]],
        // Two nested chevrons, the inner one lower and narrower.
        CoverOrnament::Chevrons => vec![vec![at(-8.0, 4.025), at(0.0, 0.575), at(8.0, 4.025)], vec![at(-4.8, 4.6), at(0.0, 2.444), at(4.8, 4.6)]],
    };
    let mut builder = gpui::PathBuilder::stroke(px(line));
    for polyline in polylines {
        builder.add_polygon(&polyline, false);
    }
    if let Ok(path) = builder.build() {
        window.paint_path(path, gold);
    }
}

/// The motif inside the frame, as one stroked path. `panel_bottom` and
/// `plate_top` are the title panel's lower edge and the name plate's upper
/// edge, from the top of the cover: ziggurat and skyline stand between them.
///
/// On Kobo the lines are black hairlines one device pixel wide. Library page
/// turns use the A2 waveform, which thresholds every pixel at 160, so a grey
/// motif would vanish on each swipe and reappear at the next greyscale
/// cleanup; black looks the same under both. On desktop they are
/// half-strength gold, one logical pixel wide.
fn paint_cover_pattern(design: CoverDesign, bounds: gpui::Bounds<Pixels>, line: f32, panel_bottom: f32, plate_top: f32, window: &mut Window) {
    #[cfg(feature = "kobo")]
    let (stroke, colour): (f32, gpui::Hsla) = (1.0 / window.scale_factor(), gpui::rgb(COVER_INK).into());
    #[cfg(not(feature = "kobo"))]
    let (stroke, colour): (f32, gpui::Hsla) = (1.0, gpui::Hsla::from(gpui::rgb(COVER_GOLD)).opacity(0.5));
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let across = |percent: f32| width * percent / 100.0;
    let (left, right) = (across(4.0) + line, width - across(4.0) - line);
    let (top, bottom) = (height * 0.04 + line, height * 0.96 - line);
    let centre = width / 2.0;
    let at = |x: f32, y: f32| bounds.origin + gpui::point(px(x), px(y));
    let inside = gpui::Bounds::new(at(left, top), size(px(right - left), px(bottom - top)));
    window.with_content_mask(Some(gpui::ContentMask { bounds: inside }), |window| {
        let mut builder = gpui::PathBuilder::stroke(px(stroke));
        let mut segment = |from: (f32, f32), to: (f32, f32)| builder.add_polygon(&[at(from.0, from.1), at(to.0, to.1)], false);
        match design.motif {
            // Lines fanning down from just above the cover, 5° apart.
            CoverMotif::Rays => {
                let (origin, length) = (-height * 0.04, height * 1.2);
                let mut angle: f32 = 0.4;
                while angle < 180.0 {
                    let direction = angle.to_radians();
                    segment((centre, origin), (centre + length * direction.cos(), origin + length * direction.sin()));
                    angle += 5.0;
                }
            }
            // Stacked chevrons pointing up, laid out from the top so the first
            // one's point sits on the frame instead of being clipped through.
            CoverMotif::Chevrons => {
                let mut apex = top;
                while apex < bottom {
                    builder.add_polygon(&[at(left, apex + across(14.0)), at(centre, apex), at(right, apex + across(14.0))], false);
                    apex += across(6.0);
                }
            }
            // A stepped pyramid as wide as the name plate, standing on it with
            // its flat top meeting the title panel, drawn like contour lines:
            // each inner outline is the outer one moved in and down by the
            // same distance, so gaps stay equal and treads keep their width.
            // A tier whose inset height falls below the base is skipped and a
            // tier whose treads would meet becomes the top, so the outlines
            // carry on until the whole pyramid is filled.
            CoverMotif::Ziggurat => {
                let tiers = 5;
                let rise = (plate_top - panel_bottom) / tiers as f32;
                let (start, end) = (across(14.0), width - across(14.0));
                let tread = (end - start) / (2 * tiers + 4) as f32;
                let inset = across(3.0);
                for nest in 0..200 {
                    if rise <= 0.0 {
                        break;
                    }
                    let offset = nest as f32 * inset;
                    let (mut rising, mut falling) = (Vec::new(), Vec::new());
                    for tier in 1..=tiers {
                        let y = plate_top - tier as f32 * rise + offset;
                        if y >= plate_top {
                            continue;
                        }
                        let (riser_left, riser_right) = (start + (tier - 1) as f32 * tread + offset, end - (tier - 1) as f32 * tread - offset);
                        if riser_right - riser_left < inset {
                            break;
                        }
                        if rising.is_empty() {
                            rising.push(at(riser_left, plate_top));
                            falling.push(at(riser_right, plate_top));
                        }
                        rising.push(at(riser_left, y));
                        falling.push(at(riser_right, y));
                        let (tread_left, tread_right) = (start + tier as f32 * tread + offset, end - tier as f32 * tread - offset);
                        if tread_right - tread_left < inset {
                            break;
                        }
                        rising.push(at(tread_left, y));
                        falling.push(at(tread_right, y));
                    }
                    if rising.is_empty() {
                        break;
                    }
                    falling.reverse();
                    rising.extend(falling);
                    builder.add_polygon(&rising, false);
                }
            }
            // A diamond lattice of continuous diagonals.
            CoverMotif::Lattice => {
                let span = bottom - top;
                let mut offset = left - span;
                while offset < right + span {
                    segment((offset, bottom), (offset + span, top));
                    segment((offset, bottom), (offset - span, top));
                    offset += across(9.0);
                }
            }
            // Pinstripes in mirrored pairs around one centre line, which alone
            // reaches the title panel. The spacing, about 4% of the width,
            // divides the half width exactly, so the frame stands one space
            // from the outermost line; each pair is 3.5% of the height shorter,
            // so the steps keep their proportion on any cover.
            CoverMotif::Skyline => {
                let half_width = (right - left) / 2.0;
                let pairs = ((half_width / across(4.0)).round() as i32 - 1).max(0);
                let spacing = half_width / (pairs + 1) as f32;
                let tallest = bottom - panel_bottom;
                for pair in 0..=pairs {
                    let tall = (tallest - pair as f32 * height * 0.035).max(0.0);
                    let x = pair as f32 * spacing;
                    segment((centre + x, bottom), (centre + x, bottom - tall));
                    if pair > 0 {
                        segment((centre - x, bottom), (centre - x, bottom - tall));
                    }
                }
            }
            // A honeycomb of flat-topped hexagons centred on the middle column.
            CoverMotif::Hexagons => {
                let radius = across(5.0);
                let (row_step, column_step) = (3.0_f32.sqrt() * radius, 1.5 * radius);
                let mut column = -((centre - left) / column_step).ceil() as i32 - 1;
                while column as f32 * column_step < right - centre + column_step {
                    let x = centre + column as f32 * column_step;
                    let mut y = top - row_step + if column % 2 != 0 { row_step / 2.0 } else { 0.0 };
                    while y < bottom + row_step {
                        let corners: Vec<_> = (0..6).map(|side| side as f32 * std::f32::consts::PI / 3.0).map(|angle| at(x + radius * angle.cos(), y + radius * angle.sin())).collect();
                        builder.add_polygon(&corners, true);
                        y += row_step;
                    }
                    column += 1;
                }
            }
        }
        if let Ok(path) = builder.build() {
            window.paint_path(path, colour);
        }
    });
}

/// Frame, motif, title panel and name plate, painted straight into `bounds`;
/// the title is always on top and the author at the bottom. Measurements
/// follow the cover's width like the HTML mock's `cqw` and positions follow
/// its height, so a square audiobook cover keeps the same proportions as a
/// book's. The ground is the host's background.
fn paint_generated_cover(design: CoverDesign, bounds: gpui::Bounds<Pixels>, lettering: Option<&CoverLettering>, window: &mut Window, cx: &mut App) {
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let across = |percent: f32| width * percent / 100.0;
    let down = |percent: f32| height * percent / 100.0;
    let line = (width * 0.0045).max(1.0);
    let gold: gpui::Hsla = gpui::rgb(COVER_GOLD).into();
    let ground: gpui::Hsla = gpui::rgb(design.ground).into();
    let ink: gpui::Hsla = gpui::rgb(COVER_INK).into();
    let rect = |x: f32, y: f32, w: f32, h: f32| gpui::Bounds::new(bounds.origin + gpui::point(px(x), px(y)), size(px(w), px(h)));
    let ruled = |area: gpui::Bounds<Pixels>, fill: gpui::Hsla| gpui::quad(area, px(0.0), fill, px(line), gold, gpui::BorderStyle::Solid);

    window.paint_quad(ruled(rect(across(4.0), down(4.0), width - 2.0 * across(4.0), height - 2.0 * down(4.0)), gpui::transparent_black()));
    let Some(lettering) = lettering else {
        // Unlettered: the motif alone, with room where the panels would be.
        paint_cover_pattern(design, bounds, line, down(40.0), height - down(9.0), window);
        return;
    };
    let raleway = gpui::font("Raleway");

    // The title panel: a ruled box with a second rule standing off it. Both
    // are filled, so the pattern stops at the outer rule.
    let (stand_off, pad_x, pad_top, pad_bottom, gap) = (across(1.4), across(6.0), across(7.0), across(8.0), across(3.0));
    let outer_x = across(10.6);
    let outer_width = width - 2.0 * outer_x;
    let inner_x = outer_x + line + stand_off;
    let inner_width = outer_width - 2.0 * (line + stand_off);
    let text_x = inner_x + line + pad_x;
    let text_width = inner_width - 2.0 * (line + pad_x);
    let (title_size, subtitle_size) = (across(8.4), across(4.6));
    let (title_line, subtitle_line) = (title_size * 1.18, subtitle_size * 1.25);
    let title = shape_cover_text(&lettering.title, raleway.clone(), ink, title_size, text_width, 5, window);
    let italic = gpui::Font { style: gpui::FontStyle::Italic, ..raleway.clone() };
    // Softened on desktop; solid black on Kobo, where grey thresholds away on page turns.
    let subtitle_colour = if cfg!(feature = "kobo") { ink } else { ink.opacity(0.8) };
    let subtitle = lettering.subtitle.as_ref().map(|subtitle| shape_cover_text(subtitle, italic, subtitle_colour, subtitle_size, text_width, 2, window)).unwrap_or_default();
    let ornament_height = match design.ornament {
        CoverOrnament::Bars => 3.0 * line + 2.0 * across(1.5),
        CoverOrnament::Lozenge => across(4.8),
        CoverOrnament::Chevrons => across(4.6),
    };
    let (title_height, subtitle_height) = (cover_text_height(&title, title_line), cover_text_height(&subtitle, subtitle_line));
    let content_height = ornament_height + gap + title_height + if subtitle.is_empty() { 0.0 } else { gap + subtitle_height };
    let inner_height = 2.0 * line + pad_top + content_height + pad_bottom;
    let outer_height = inner_height + 2.0 * (line + stand_off);
    let outer_y = down(12.0);
    let inner_y = outer_y + line + stand_off;

    // The name plate is laid out before anything is painted: the motif goes
    // underneath both panels, and the ziggurat stands on the plate. Long
    // names wrap onto a second line rather than being cut off.
    let (plate_x, pad) = (across(14.0), across(2.0));
    let plate_width = width - 2.0 * plate_x;
    let author_width = plate_width - 2.0 * (line + pad);
    let author_size = across(3.3).max(5.0);
    let author_line = author_size * 1.35;
    let bold = gpui::Font { weight: gpui::FontWeight::BOLD, ..raleway };
    // Up to four lines: a book with several authors wraps rather than being cut off.
    let author = shape_cover_text(&lettering.author, bold, ink, author_size, author_width, 4, window);
    let plate_height = 2.0 * line + across(2.4) + cover_text_height(&author, author_line) + pad;
    let plate_y = height - down(9.0) - plate_height;
    let plate_top = if author.is_empty() { height - down(9.0) } else { plate_y };
    paint_cover_pattern(design, bounds, line, outer_y + outer_height, plate_top, window);

    window.paint_quad(ruled(rect(outer_x, outer_y, outer_width, outer_height), ground));
    window.paint_quad(ruled(rect(inner_x, inner_y, inner_width, inner_height), ground));
    let mut y = inner_y + line + pad_top;
    paint_cover_ornament(design.ornament, bounds.origin + gpui::point(px(width / 2.0), px(y)), across(1.0), line, window);
    y += ornament_height + gap;
    paint_cover_text(&title, bounds.origin + gpui::point(px(text_x), px(y)), text_width, title_line, window, cx);
    y += title_height + gap;
    paint_cover_text(&subtitle, bounds.origin + gpui::point(px(text_x), px(y)), text_width, subtitle_line, window, cx);

    if author.is_empty() {
        return;
    }
    window.paint_quad(ruled(rect(plate_x, plate_y, plate_width, plate_height), ground));
    paint_cover_text(&author, bounds.origin + gpui::point(px(plate_x + line + pad), px(plate_y + line + across(2.4))), author_width, author_line, window, cx);
}

/// The generated jacket: one element whose canvas paints the whole design at
/// the cover's real size, so the type scales with the card. Always 1.6 times
/// as tall as it is wide, audiobooks included.
fn generated_cover(design: CoverDesign, lettering: Option<CoverLettering>, theme: BrowserTheme) -> Div {
    book_cover(theme, BOOK_COVER_ASPECT_RATIO)
        .overflow_hidden()
        .rounded_r(px(BOOK_COVER_RADIUS))
        .bg(gpui::rgb(design.ground))
        .child(canvas(|_, _, _| {}, move |bounds, _, window, cx| paint_generated_cover(design, bounds, lettering.as_ref(), window, cx)).absolute().size_full())
}

/// The generated jacket without its text, for a cover too small to letter.
pub fn book_thumbnail_generated(variant: u64, theme: BrowserTheme) -> Div {
    generated_cover(CoverDesign::new(variant), None, theme)
}

/// A cover for a book with no available image, also shown while its image loads.
/// Cards and detail pages use it regardless of the cover-text setting.
///
/// `variant` picks the design — pass something stable per book (its content
/// hash or source-entry identity) so the same book always generates the same
/// cover. Painted directly from quads, shaped text and paths, nothing
/// rasterized, so no cover keeps a bitmap in memory.
pub fn book_cover_generated(title: impl Into<SharedString>, subtitle: Option<&str>, author: &str, variant: u64, theme: BrowserTheme) -> Div {
    let subtitle = subtitle.map(str::trim).filter(|value| !value.is_empty()).map(SharedString::from);
    let author = SharedString::from(author.trim().to_uppercase());
    generated_cover(CoverDesign::new(variant), Some(CoverLettering { title: title.into(), subtitle, author }), theme)
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

/// Share three lines, giving the title priority while reserving one subtitle
/// line. Measure at the actual card width so a one-line title leaves two
/// subtitle lines, including after a font or window-size change.
pub fn book_title_lines(title: impl Into<SharedString>, subtitle: Option<&str>) -> Div {
    let title = title.into();
    let subtitle = subtitle.map(str::trim).filter(|value| !value.is_empty()).map(SharedString::from);
    div().w_full().min_w_0().child(
        canvas(
            move |bounds, window, cx| {
                let mut content = book_title_content(title, subtitle, bounds.size.width, window, cx).into_any_element();
                content.prepaint_as_root(bounds.origin, size(AvailableSpace::Definite(bounds.size.width), AvailableSpace::MaxContent), window, cx);
                content
            },
            |_, mut content, window, cx| content.paint(window, cx),
        )
        .w_full()
        .h(gpui::rems(BOOK_CARD_TITLE_HEIGHT_REM)),
    )
}

fn book_title_content(title: SharedString, subtitle: Option<SharedString>, width: Pixels, window: &mut Window, cx: &mut App) -> Div {
    let has_title = !title.trim().is_empty();
    let title_limit = BOOK_CARD_TEXT_LINES - usize::from(subtitle.is_some());
    let title_element = || div().w_full().min_w_0().flex_none().text_ellipsis().line_clamp(title_limit).child(title.clone());
    // The line count only determines how much space a subtitle receives.
    let title_lines = if has_title && subtitle.is_some() {
        let mut measured = title_element().into_any_element();
        let height = measured.layout_as_root(size(AvailableSpace::Definite(width), AvailableSpace::MaxContent), window, cx).height;
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.pixel_snap(style.line_height.to_pixels(font_size.into(), window.rem_size()));
        (f32::from(height) / f32::from(line_height)).round().max(1.0) as usize
    } else {
        0
    };
    div().w_full().min_w_0().flex().flex_col().when(has_title, |content| content.child(title_element())).children(subtitle.map(|subtitle| {
        div()
            .w_full()
            .min_w_0()
            .flex_none()
            .text_size(gpui::rems(BOOK_CARD_SUBTITLE_SIZE))
            .line_height(gpui::relative(BOOK_CARD_TITLE_LINE_HEIGHT_EM))
            .font_weight(gpui::FontWeight::NORMAL)
            .text_ellipsis()
            .line_clamp(BOOK_CARD_TEXT_LINES.saturating_sub(title_lines).max(1))
            .child(subtitle)
    }))
}

#[cfg(test)]
mod book_title_tests {
    use super::*;
    use gpui::{Context, Render, TestAppContext};

    struct TitleBudget;

    impl Render for TitleBudget {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            book_card_title().child(canvas(
                |_, window, cx| {
                    let long = "A long book title or subtitle that needs many lines of text ".repeat(12);
                    for rem in [16.0, 24.0] {
                        window.set_rem_size(px(rem));
                        let title_height = window.pixel_snap(px(rem * BOOK_CARD_TITLE_SIZE * BOOK_CARD_TITLE_LINE_HEIGHT_EM));
                        let subtitle_height = window.pixel_snap(px(rem * BOOK_CARD_SUBTITLE_SIZE * BOOK_CARD_TITLE_LINE_HEIGHT_EM));
                        for width in [px(120.0), px(200.0)] {
                            for (title, subtitle, title_lines, subtitle_lines) in [
                                (long.as_str(), None, 3, 0),
                                (long.as_str(), Some(long.as_str()), 2, 1),
                                ("Book", Some(long.as_str()), 1, 2),
                                ("", Some(long.as_str()), 0, 3),
                                ("   ", Some(long.as_str()), 0, 3),
                                (long.as_str(), Some("Brief"), 2, 1),
                                ("Book", None, 1, 0),
                                ("", None, 0, 0),
                            ] {
                                let mut content = book_title_content(title.into(), subtitle.map(SharedString::from), width, window, cx).into_any_element();
                                let measured = content.layout_as_root(size(AvailableSpace::Definite(width), AvailableSpace::MaxContent), window, cx);
                                assert_eq!(measured.height, title_height * title_lines as f32 + subtitle_height * subtitle_lines as f32, "title={title:?}, subtitle={subtitle:?}, width={width:?}, rem={rem}");
                            }
                        }
                    }
                },
                |_, _, _, _| {},
            ))
        }
    }

    #[gpui::test]
    fn book_titles_share_three_lines_at_different_widths_and_font_sizes(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| TitleBudget);
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }
}

/// A card's title row: title and subtitle across the card's whole width.
///
/// The download control used to sit beside them, which took width from only
/// the cards whose books were not on the device — so titles in one row wrapped
/// at different places. It is on the cover now; see
/// [`book_cover_download_badge`].
pub fn book_card_title_row(title: impl Into<SharedString>, subtitle: Option<&str>) -> Div {
    div().w_full().min_w_0().flex_none().child(book_card_title().child(book_title_lines(title, subtitle)))
}

/// The mark of a book that is not on this device, in its cover's top-right
/// corner — the one the progress line, the audiobook tag and the spine leave
/// free — and the control that fetches it.
///
/// An opaque theme surface and a contrasting glyph keep the control readable
/// over any cover art, including black ink on white paper on Kobo. `pending`
/// swaps the glyph for a spinner in the same place, so nothing moves when the mark is pressed; the
/// caller only makes the idle mark clickable.
pub fn book_cover_download_badge(id: impl Into<ElementId>, pending: bool, theme: BrowserTheme) -> Stateful<Div> {
    book_cover_action_badge(id, pending, download_glyph(), theme)
}

/// Waiting for transfer policy or another download requires no animation.
pub fn book_cover_queued_badge(id: impl Into<ElementId>, theme: BrowserTheme) -> Stateful<Div> {
    book_cover_badge_hit_area(id).child(book_cover_badge_plate(theme).child(Icon::empty().path("icons/hourglass.svg").size(gpui::rems(crate::DOWNLOAD_ICON_SIZE_REM)).text_color(theme.text)))
}

/// The same corner for a catalog book: adding it to the library, which
/// downloads it.
pub fn book_cover_add_badge(id: impl Into<ElementId>, pending: bool, theme: BrowserTheme) -> Stateful<Div> {
    book_cover_action_badge(id, pending, download_glyph(), theme)
}

/// The same corner for a feed book its feed sells: opening its store page.
pub fn book_cover_buy_badge(id: impl Into<ElementId>, theme: BrowserTheme) -> Stateful<Div> {
    book_cover_action_badge(id, false, buy_glyph().size(gpui::rems(crate::DOWNLOAD_ICON_SIZE_REM)), theme)
}

/// The same corner for a catalog book the library already holds: a mark,
/// not an action.
pub fn book_cover_in_library_badge(id: impl Into<ElementId>, theme: BrowserTheme) -> Stateful<Div> {
    book_cover_badge_hit_area(id).child(book_cover_badge_plate(theme).child(Icon::new(IconName::Check).size(gpui::rems(crate::DOWNLOAD_ICON_SIZE_REM)).text_color(theme.text)))
}

fn book_cover_action_badge(id: impl Into<ElementId>, pending: bool, glyph: Icon, theme: BrowserTheme) -> Stateful<Div> {
    let plate_hover = theme.accent_wash_hover;
    book_cover_badge_hit_area(id).when(!pending, |badge| badge.cursor_pointer()).child(
        book_cover_badge_plate(theme)
            .when(pending, |plate| plate.child(Spinner::new().icon(IconName::LoaderCircle).xsmall().color(theme.text)))
            .when(!pending, |plate| plate.group_hover(BOOK_COVER_BADGE_GROUP, move |style| style.bg(plate_hover)).child(glyph.text_color(theme.text))),
    )
}

const BOOK_COVER_BADGE_GROUP: &str = "book-cover-badge";

/// The badge's hit area, which takes the clicks and the tooltip. With a
/// pointer it is the plate itself, at `BOOK_COVER_BADGE_INSET`. On a phone it
/// is a touch target reaching in from the cover's corner, holding the plate at
/// the same inset, so the badge keeps its size on a small cover.
fn book_cover_badge_hit_area(id: impl Into<ElementId>) -> Stateful<Div> {
    let area = div().id(id).group(BOOK_COVER_BADGE_GROUP).absolute().flex().items_start().justify_end();
    if crate::touch_ui() {
        area.top_0().right_0().min_w(gpui::rems(crate::TOUCH_TARGET_REM)).min_h(gpui::rems(crate::TOUCH_TARGET_REM)).p(px(BOOK_COVER_BADGE_INSET))
    } else {
        area.top(px(BOOK_COVER_BADGE_INSET)).right(px(BOOK_COVER_BADGE_INSET))
    }
}

fn book_cover_badge_plate(theme: BrowserTheme) -> Div {
    div().size(gpui::rems(DOWNLOAD_ACTION_SIZE_REM)).flex().items_center().justify_center().rounded(px(RADIUS_SM)).border_1().border_color(theme.rule).bg(theme.page_bg).text_color(theme.text)
}

/// The cloud-and-arrow glyph every download action carries, on a book's cover
/// and on a folder or subject chip alike.
pub fn download_glyph() -> Icon {
    Icon::empty().path("icons/cloud-download.svg").size(gpui::rems(crate::DOWNLOAD_ICON_SIZE_REM))
}

/// Buying a book at its store.
pub fn buy_glyph() -> Icon {
    Icon::empty().path("icons/shopping-bag.svg")
}

/// The pill that follows the pointer while items are being dragged. It names
/// what is being carried rather than miniaturising it: a half-transparent copy
/// of three book covers reads as a rendering fault, and a count does not.
pub fn drag_preview(label: SharedString, theme: BrowserTheme) -> Div {
    div().px(px(SPACE_SM)).h(px(28.0)).flex().items_center().rounded(px(RADIUS_SM)).bg(theme.accent).text_color(theme.accent_text).text_size(gpui::rems(TEXT_XS)).child(label)
}

/// Breadcrumbs use regular text at the medium body size. Items own the vertical
/// padding so the trail stays aligned with the action buttons in the topbar.
///
/// The gap is zeroed against the toolkit's `gap_1p5`, which would otherwise add
/// 6px to the padding each crumb already carries — a chevron only 3.5 units
/// wide, floating in more air than it separates. Crumb padding is then the
/// single owner of the trail's rhythm: change it in [`folder_breadcrumb_item`]
/// and the label, its hit area and its distance from the separator all move
/// together.
pub fn folder_breadcrumb() -> Breadcrumb {
    Breadcrumb::new().text_size(gpui::rems(TEXT_MD)).gap_0()
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
    // One face for the whole trail: the display face at this size read thinner
    // than the interface face beside it, so the current folder looked lighter
    // than its ancestors. Ink against muted carries "current" on its own.
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
    folder_breadcrumb_item(label, current, theme).pl(px(CONTENT_INSET))
}

/// The content that determines a hierarchy item's natural width.
#[derive(Clone)]
pub struct HierarchyItemWidth {
    pub icon: NavigationIcon,
    pub label: SharedString,
    pub count: Option<usize>,
    pub show_download: bool,
}

impl HierarchyItemWidth {
    /// Uses the rendered chip's text and icon layout so font changes affect
    /// wrapping, including the action slot only when it is shown.
    pub fn measure(&self, window: &mut Window, cx: &mut App) -> Pixels {
        let mut content = div()
            .flex()
            .items_center()
            .whitespace_nowrap()
            .text_size(gpui::rems(COMPACT_HIERARCHY_ITEM_TEXT_SIZE_REM))
            .gap(px(COMPACT_HIERARCHY_ITEM_CONTENT_GAP))
            .child(crate::navigation_icon(self.icon))
            .child(div().flex_none().child(self.label.clone()))
            .children(self.count.map(|count| div().flex_none().child(count.to_string())))
            .when(self.show_download, |content| content.child(div().w(gpui::rems(DOWNLOAD_ACTION_SIZE_REM)).flex_none()))
            .into_any_element();
        let measured = content.layout_as_root(gpui::size(gpui::AvailableSpace::MaxContent, gpui::AvailableSpace::MaxContent), window, cx);
        measured.width + px(COMPACT_HIERARCHY_ITEM_HORIZONTAL_PADDING * 2.0 + 2.0)
    }
}

/// A compact hierarchy item for mixed paginator pages.
///
/// This function owns presentation only. Feature code remains responsible for
/// attaching click handlers and navigation behavior.
pub fn compact_hierarchy_item(
    id: impl Into<ElementId>, icon: NavigationIcon, label: impl Into<SharedString>, path: Option<SharedString>, count: Option<usize>, download: Option<AnyElement>, selected: bool, cursor: bool, marquee: bool, theme: BrowserTheme,
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
    // A selected chip and the chip under the keyboard cursor are both the
    // keyboard's tier, `accent_wash`, and keep every foreground they had; see
    // the ladder in `tokens.rs`. The pointer lifts a washed chip to
    // `accent_wash_hover` and a plain one to the light `hover`.
    //
    // `selected(true)` is what makes the hover reachable at all: a `Button`
    // writes its own hover slot unless it is selected, and gpui allows that slot
    // to be written once. Claiming it is the only way to say that a tinted chip
    // lifts rather than darkens, and the only way for `marquee` to say nothing
    // at all. See `selectable_button`, which takes the same state for the same
    // reason.
    let washed = selected || cursor;
    let hovered = if washed { theme.accent_wash_hover } else { theme.hover };
    let base = if washed { theme.accent_wash } else { theme.page_bg };
    let button = crate::base_button(id).ghost().selected(true).bg(base).hover(move |style| if marquee { style.bg(base) } else { style.bg(hovered) });
    button.w_full().max_w_full().h_full().p_0().px(px(COMPACT_HIERARCHY_ITEM_HORIZONTAL_PADDING)).rounded(px(0.0)).border_1().border_color(theme.rule).child(
        div()
            .size_full()
            .min_w_0()
            .text_size(gpui::rems(COMPACT_HIERARCHY_ITEM_TEXT_SIZE_REM))
            .flex()
            .items_center()
            .gap(px(COMPACT_HIERARCHY_ITEM_CONTENT_GAP))
            .child(crate::navigation_icon(icon).text_color(theme.text_accent))
            .child(text)
            .children(count.map(|count| div().flex_none().text_color(theme.text_muted).child(count.to_string())))
            .when_some(download, |content, download| content.child(div().size(gpui::rems(DOWNLOAD_ACTION_SIZE_REM)).flex_none().flex().items_center().justify_center().child(download))),
    )
}

/// A gentle shadow for a container card. Its outer grid cell paints this
/// before clipping the card's contents, so the shadow can reach the gaps.
pub fn section_card_shadow(theme: BrowserTheme) -> Vec<BoxShadow> {
    let mut color = black();
    color.a = theme.shadow.a * if theme.page_bg.l < 0.5 { 1.0 } else { 0.32 };
    vec![box_shadow(px(0.0), px(2.0), px(8.0), px(0.0), color)]
}

/// A section card's tile, sharing the page background.
/// `washed` — selected, or under the keyboard cursor — fills it with the
/// keyboard's `accent_wash`, the same mark a folder chip takes; see the ladder
/// in `tokens.rs`.
pub fn section_card_frame(washed: bool, theme: BrowserTheme) -> Div {
    div().absolute().inset_0().min_w_0().flex().flex_col().bg(if washed { theme.accent_wash } else { theme.page_bg })
}

/// Full-width colour behind a section card's heading. On Kobo it is not
/// filled: the page shows through, ruled in black above and at the sides, and
/// the content box's own border closes it below.
pub fn section_card_band(index: usize, heading: impl IntoElement) -> Div {
    let band = div().flex_none().min_w_0().py(gpui::rems(crate::SECTION_CARD_HEADING_PADDING_REM)).px(px(SPACE_MD));
    #[cfg(feature = "kobo")]
    {
        let _ = index;
        band.bg(gpui::white()).border_t_1().border_x_1().border_color(gpui::black()).child(heading)
    }
    #[cfg(not(feature = "kobo"))]
    {
        let (from, to) = section_card_band_colors(index);
        band.bg(gpui::linear_gradient(120.0, gpui::linear_color_stop(from, 0.0), gpui::linear_color_stop(to, 1.0))).child(heading)
    }
}

/// The area below a section card's heading, enclosed by its own square border.
pub fn section_card_content(theme: BrowserTheme) -> Div {
    div().flex_1().min_h_0().min_w_0().flex().flex_col().px(px(SPACE_MD - 1.0)).border_1().border_color(theme.accent)
}

/// A section card's title, total and paging controls on a shared colour band.
/// The existing heading layout and pointer behavior stay the same.
///
/// Hover underlines carry an id on the text itself: the underline is shaped
/// with the text at layout, when only an element with its own state can know
/// that its group is hovered.
///
/// A heading that `opens` nothing — a shelf of Home — neither points nor
/// underlines.
pub fn section_card_heading(id: impl Into<ElementId>, title: impl Into<SharedString>, count: Option<usize>, pager: AnyElement, opens: bool, focused: bool, marquee: bool, theme: BrowserTheme) -> Stateful<Div> {
    div()
        .id(id)
        .group("section-card-heading")
        .flex_none()
        .h(gpui::rems(crate::SECTION_CARD_HEADING_HEIGHT_REM))
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(SPACE_SM))
        // Light on the colour band; black on Kobo's unfilled one.
        .text_color(if cfg!(feature = "kobo") { theme.text } else { gpui::rgb(0xf2efe8).into() })
        .when(opens, |heading| heading.cursor_pointer())
        .child(
            div()
                .id("title")
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .font_family(theme.display_font_family())
                .text_size(gpui::rems(TEXT_LG))
                .when(focused && !cfg!(feature = "kobo"), |title| title.text_color(gpui::white()))
                .when(opens && !marquee, |title| title.group_hover("section-card-heading", |style| style.underline()))
                .child(title.into()),
        )
        .children(count.map(|count| div().flex_none().child(count.to_string())))
        .child(pager)
}

/// One child in a section card's list, with its navigation icon and a quiet
/// count pill, when it has a count. The whole row is the hover and click target.
pub fn section_card_subject(id: impl Into<ElementId>, icon: NavigationIcon, label: impl Into<SharedString>, count: Option<usize>, theme: BrowserTheme) -> Stateful<Div> {
    div()
        .id(id)
        .group("section-card-subject")
        .h(gpui::rems(crate::SECTION_CARD_SUBJECT_ROW_HEIGHT_REM))
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(SPACE_SM))
        .mx(px(-SPACE_XS))
        .px(px(SPACE_XS))
        .cursor_pointer()
        .text_size(gpui::rems(TEXT_SM))
        .hover(move |style| style.bg(theme.hover))
        .child(crate::navigation_icon(icon).size(gpui::rems(0.8)).text_color(theme.text_accent))
        .child(div().id("label").flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(label.into()))
        .children(count.map(|count| div().flex_none().min_w(gpui::rems(1.6)).px(px(SPACE_XS)).text_center().text_size(gpui::rems(0.8)).text_color(theme.text_muted).bg(theme.hover).child(count.to_string())))
}

/// A section card's contents, side by side: its child entries, then its covers.
pub fn section_card_body() -> Div {
    // Cells provide the full vertical inset. Offset the content border so it
    // does not add another pixel above or below their padding.
    div().flex_1().min_h_0().min_w_0().my(px(-1.0)).flex().gap(px(crate::SECTION_CARD_COLUMN_GAP))
}

/// A section card's child entries fill one paginator cell.
pub fn section_card_subject_rows(rows: Vec<AnyElement>) -> Div {
    div().flex_none().h_full().w_full().min_w_0().py(px(crate::BOOK_CARD_PADDING)).flex().flex_col().children(rows)
}

/// Paging control for a mixed strip of entry columns and covers, in its heading.
pub fn section_card_cover_arrow(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, enabled: bool) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id)
        .size(gpui::rems(1.5))
        .flex()
        .items_center()
        .justify_center()
        .when(enabled, |arrow| arrow.cursor_pointer())
        .border_1()
        // Translucent white on the colour band; outlined black on Kobo's white one.
        .when(!cfg!(feature = "kobo"), |arrow| {
            arrow.text_color(gpui::rgb(0xf2efe8)).bg(gpui::white().opacity(0.12)).border_color(gpui::white().opacity(0.28)).when(enabled, |arrow| arrow.hover(|style| style.bg(gpui::white().opacity(0.22))))
        })
        .when(cfg!(feature = "kobo"), |arrow| arrow.text_color(gpui::black()).bg(gpui::white()).border_color(gpui::black()))
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(label.clone()).build(window, cx))
        .child(Icon::new(icon).size(px(12.0)))
}

/// Marks the place a drag will land: an accent outline and a faint wash,
/// shown while a drag of `T`, or of files from the desktop, is over it and
/// `lands_here` says so. It is laid over the target's own content, so it shows
/// on a button as well as on a plain row, and it takes no clicks of its own.
/// The target it sits in has to be `relative`.
pub fn drop_highlight<T: 'static>(lands_here: impl Fn(&gpui::App) -> bool + 'static, theme: BrowserTheme) -> Div {
    let accent = theme.accent;
    let wash = theme.accent.opacity(0.08);
    let lands_here = std::rc::Rc::new(lands_here);
    let lands_here_files = lands_here.clone();
    div()
        .absolute()
        .inset_0()
        .drag_over::<T>(move |style, _, _, cx| if lands_here(cx) { style.border_2().border_color(accent).bg(wash) } else { style })
        .drag_over::<gpui::ExternalPaths>(move |style, _, _, cx| if lands_here_files(cx) { style.border_2().border_color(accent).bg(wash) } else { style })
}
