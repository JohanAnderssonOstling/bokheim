//! EPUB search controls.

use gpui::{AnyElement, Context, IntoElement};
use ui_components as components;

use crate::epub::ReaderView;
use crate::shell;

impl ReaderView {
    pub(super) fn render_search(&self, theme: components::BrowserTheme, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.search.query.is_none() {
            return None;
        }
        let (current, total) = self.search.match_position.unwrap_or((0, 0));
        let filters = vec![
            components::reader_choice_button("reader-search-case", "Aa", self.search.filters.match_case)
                .tooltip("Match case")
                .on_click(cx.listener(|this, _, _, cx| this.update_search_options(|filters| filters.match_case = !filters.match_case, cx)))
                .into_any_element(),
            components::reader_choice_button("reader-search-word", "Word", self.search.filters.whole_word)
                .tooltip("Match whole words")
                .on_click(cx.listener(|this, _, _, cx| this.update_search_options(|filters| filters.whole_word = !filters.whole_word, cx)))
                .into_any_element(),
            components::reader_choice_button("reader-search-diacritics", "Marks", self.search.filters.match_diacritics)
                .tooltip("Match diacritics exactly")
                .on_click(cx.listener(|this, _, _, cx| this.update_search_options(|filters| filters.match_diacritics = !filters.match_diacritics, cx)))
                .into_any_element(),
            components::reader_choice_button("reader-search-scope", if self.search.filters.whole_book { "Book" } else { "Section" }, self.search.filters.whole_book)
                .tooltip("Search current section or whole book")
                .on_click(cx.listener(|this, _, _, cx| this.update_search_options(|filters| filters.whole_book = !filters.whole_book, cx)))
                .into_any_element(),
        ];
        Some(shell::search::search_bar(&cx.entity(), &self.search_input, (current, total), filters, theme))
    }
}
