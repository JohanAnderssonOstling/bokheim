//! EPUB document surface and its selection/table/image context menu.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, IntoElement, px};
use gpui_component::menu::ContextMenuExt;
use ui_components as components;

use crate::epub::{LoadState, ReaderView};

/// Keeps glyph ink and logical-to-framebuffer rounding inside the Kobo panel.
/// This is a renderer guard band, not an authored EPUB margin.
const KOBO_READER_EDGE_INSET: f32 = 2.0;

impl ReaderView {
    pub(super) fn render_document(&self, entity: Entity<Self>, _cx: &mut Context<Self>) -> AnyElement {
        let LoadState::Ready(renderer) = &self.load_state else {
            return components::reader_content().child(gpui::div().size_full()).into_any_element();
        };

        let selection_renderer = renderer.clone();
        let selection_owner = entity.clone();
        let highlight_target = entity.clone();
        let citation_target = entity;
        let table_renderer = renderer.clone();
        let image_renderer = renderer.clone();
        components::reader_content()
            .id("epub-reader-document")
            .when(cfg!(feature = "kobo"), |document| document.px(px(KOBO_READER_EDGE_INSET)))
            .child(renderer.clone())
            .context_menu(move |menu, window, cx| {
                let position = window.mouse_position();
                let selection_hit = selection_owner.read(cx).selection.is_some() && selection_renderer.read(cx).selection_contains(position);
                let table_hit = table_renderer.read(cx).table_at(position);
                let image_hit = image_renderer.read(cx).image_at(position);
                let mut menu = components::ReaderContextMenu::new(menu);
                if selection_hit {
                    let highlight_target = highlight_target.clone();
                    let citation_target = citation_target.clone();
                    menu = menu.selection(move |window, cx| highlight_target.update(cx, |this, cx| this.highlight_selection(window, cx)), move |_, cx| citation_target.update(cx, |this, cx| this.copy_selection_citation(cx)));
                }
                if table_hit {
                    let copy_markdown = table_renderer.clone();
                    let copy_unstyled_html = table_renderer.clone();
                    let copy_styled_html = table_renderer.clone();
                    menu = menu.table(
                        move |_, cx| {
                            copy_markdown.update(cx, |renderer, _| renderer.copy_table_at(position));
                        },
                        move |_, cx| {
                            copy_unstyled_html.update(cx, |renderer, _| renderer.copy_table_unstyled_html_at(position));
                        },
                        move |_, cx| {
                            copy_styled_html.update(cx, |renderer, _| renderer.copy_table_styled_html_at(position));
                        },
                    );
                }
                if image_hit {
                    let copy_image = image_renderer.clone();
                    menu = menu.image(move |_, cx| {
                        copy_image.update(cx, |renderer, _| renderer.copy_image_at(position));
                    });
                }
                menu.finish()
            })
            .into_any_element()
    }
}
