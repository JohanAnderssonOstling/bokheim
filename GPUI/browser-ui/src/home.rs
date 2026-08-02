use bokheim_ui_components as components;
use gpui::AnyElement;
use gpui::prelude::*;

use crate::BrowserView;

impl BrowserView {
    pub(crate) fn render_home(&self, theme: components::BrowserTheme) -> AnyElement {
        if self.home_sections.is_empty() {
            return components::browser_inline_message("No recent books", theme).into_any_element();
        }
        let Some(library_id) = self.active_library_id.clone() else {
            return components::browser_inline_message("No library configured", theme).into_any_element();
        };
        let mut page = components::home_page();
        let heading_font = self.heading_font_name();
        for (section_index, section) in self.home_sections.iter().enumerate() {
            let mut row = components::carousel_row(format!("home-carousel-{section_index}"));
            for card in section.books.iter() {
                row = row.child(Self::render_book_card(card.clone(), library_id.clone(), self.open_reader.clone(), format!("home-{section_index}-book-{}", card.id), theme));
            }
            page = page.child(components::carousel_section().child(components::carousel_title(section.title.clone(), heading_font.clone())).child(row));
        }
        page.into_any_element()
    }
}
