use crate::components::inline_message;
use floem::View;

pub fn search_tab_view() -> impl View {
    inline_message("Search")
}
