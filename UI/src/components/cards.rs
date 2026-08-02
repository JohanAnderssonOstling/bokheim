use crate::components::text::{subtle_label, title_label, ui_label};
use crate::components::tokens::TOKENS;
use crate::style::palette;
use epub_svelte_lib::library::Book;
use floem::event::{Event, EventPropagation};
use floem::prelude::{button, container, label, v_stack, Button, Decorators, View};
use floem::style::{AlignItems, CursorStyle};
use floem::text::Weight;
use floem::views::img;

pub fn book_card(book: Book, footer: impl View + 'static, on_open: impl FnMut(&Event) -> EventPropagation + 'static) -> impl View {
    let title = book.title().as_str().to_owned();
    v_stack((
        book_cover(book, on_open),
        title_label(move || title.clone()).style(|s| s.width(TOKENS.size.book_cover_width).height(46).font_size(TOKENS.typography.text_lg).font_weight(Weight::SEMIBOLD).text_ellipsis().margin_top(TOKENS.spacing.sm)),
        footer,
    ))
    .style(|s| s.width(TOKENS.size.book_cover_width).gap(TOKENS.spacing.xs))
}

fn book_cover(book: Book, on_open: impl FnMut(&Event) -> EventPropagation + 'static) -> impl View {
    let content: Box<dyn View> = match book.cover().map(ToOwned::to_owned) {
        None => Box::new(label(|| "Placeholder").style(|s| s.width_full().height_full().items_center().justify_center())),
        Some(book_cover) => Box::new(img(move || book_cover.clone()).style(|s| s.width(TOKENS.size.book_cover_width).height(TOKENS.size.book_cover_height))),
    };

    container(content).on_click(on_open).style(|s| {
        let theme = palette();
        s.width(TOKENS.size.book_cover_width)
            .height(TOKENS.size.book_cover_height)
            .border_radius(TOKENS.radius.sm)
            .border(1)
            .border_color(theme.border_color)
            .box_shadow_blur(14)
            .box_shadow_color(theme.box_shadow)
            .box_shadow_spread(0)
            .box_shadow_h_offset(2)
            .box_shadow_v_offset(8)
            .cursor(CursorStyle::Pointer)
    })
}

pub fn library_item_card(title: String, detail: String) -> Button {
    let title_for_label = title.clone();
    button(v_stack((ui_label(move || title_for_label.clone()).style(|s| s.font_size(16).font_bold().width(260).text_ellipsis()), subtle_label(move || detail.clone()).style(|s| s.font_size(TOKENS.typography.text_sm))))).style(|s| {
        let theme = palette();
        s.width(280)
            .padding_horiz(TOKENS.spacing.sm)
            .padding_vert(TOKENS.spacing.sm)
            .justify_start()
            .align_items(AlignItems::Start)
            .border(1)
            .border_color(theme.border_color)
            .border_radius(TOKENS.radius.lg)
            .background(theme.white_surface)
            .hover(|s| s.background(theme.active_highlight))
    })
}
