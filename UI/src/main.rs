use floem::views::Decorators;
use floem::{IntoView, View};

use crate::page_navigation::page_navigation_view;

mod components;
mod epub_reader;
mod home_page;
mod library_page;
mod page_navigation;
mod toc;
mod functions;

fn app_view() -> impl View {
    /*let mut epub_renderer = EpubReader::new("/home/johan/Hem/Downloads/A Concise History of Switzerland.epub");
    epub_renderer = epub_renderer.style(move |style| {style.width_full()});
    epub_renderer.into_view()*/
    page_navigation_view()
}

fn main() {
    floem::launch(app_view);
}
