use bokheim_ui_components as components;
use gpui::prelude::*;
use gpui::{AnyElement, Entity, uniform_list};
use gpui_component::input::Input;

use crate::BrowserView;

impl BrowserView {
    pub(crate) fn render_authors(&self, entity: Entity<Self>, book_columns: usize, filter: &str, theme: components::BrowserTheme) -> AnyElement {
        let filter = filter.trim().to_lowercase();
        let authors = self.authors.iter().filter(|author| filter.is_empty() || author.name().as_str().to_lowercase().contains(&filter)).cloned().collect::<Vec<_>>();
        let has_filtered_authors = !authors.is_empty();
        let selected_author = self.selected_author.clone();
        let heading_font = self.heading_font_name();
        let list_target = entity.clone();
        let author_list = components::virtual_list(uniform_list("browser-authors", authors.len(), move |range, _, _| {
            range
                .map(|index| {
                    let author = &authors[index];
                    let name = author.name().as_str().to_owned();
                    let selected = selected_author.as_deref() == Some(name.as_str());
                    let initial = name.chars().next().unwrap_or('?').to_uppercase().to_string();
                    let target = list_target.clone();
                    components::author_list_row(format!("author-{index}"), selected, theme)
                        .child(components::author_avatar(initial, heading_font.clone(), theme))
                        .child(components::author_list_identity().child(components::author_list_name(name.clone(), heading_font.clone())).child(components::author_book_count(author.book_count(), theme)))
                        .child(components::author_list_chevron(theme))
                        .on_click(move |_, _, cx| {
                            target.update(cx, |this, cx| {
                                this.select_author(name.clone(), cx);
                            });
                        })
                })
                .collect::<Vec<_>>()
        }));
        let author_list_panel = components::author_list_panel(theme)
            .child(components::author_filter_box().child(Input::new(&self.author_filter_input)))
            .when(self.authors.is_empty(), |panel| panel.child(components::browser_inline_message("No authors found", theme)))
            .when(!self.authors.is_empty() && !has_filtered_authors, |panel| panel.child(components::browser_inline_message("No matching authors", theme)))
            .when(has_filtered_authors, |panel| panel.child(author_list));

        let mut books_panel = components::author_books_panel();
        if let Some(author) = &self.selected_author {
            let books = self.author_groups.iter().flat_map(|group| group.books.iter().cloned()).collect::<Vec<_>>();
            let book_count = books.len();
            books_panel = books_panel.child(components::author_books_heading(author.clone(), book_count, self.heading_font_name(), theme));
            if books.is_empty() {
                books_panel = books_panel.child(components::browser_inline_message("No books found for this author", theme));
            } else {
                books_panel = books_panel.child(self.render_book_grid_cards(entity, std::sync::Arc::new(books), book_columns, "author-books".to_owned(), false, theme));
            }
        } else {
            books_panel = books_panel.child(components::author_selection_hint("Select an author to view their books", theme));
        }

        components::authors_split_page().child(author_list_panel).child(books_panel).into_any_element()
    }
}
