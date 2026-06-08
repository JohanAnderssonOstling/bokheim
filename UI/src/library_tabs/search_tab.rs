use crate::components::library_views::message_row;
use floem::View;

pub fn search_tab_view() -> impl View {
    message_row("Search")
}
