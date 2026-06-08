use crate::home_carousels::library_home_carousels;
use crate::page_navigation::Signals;
use floem::View;

pub fn home_tab_view(library_id: String, signals: Signals) -> impl View {
    library_home_carousels(library_id, signals)
}
