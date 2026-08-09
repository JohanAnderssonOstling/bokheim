//! Landing page: recently read, finished, and recently added carousels.

use std::collections::HashSet;

use bokheim_ui_components as components;
use gpui::prelude::*;
use gpui::{Context, IntoElement, Render, SharedString, Window};

use crate::library::services::{HomeBrowseRows, LibraryContext};
use crate::library::widgets::BookGrid;
use crate::stores::heading_font;

/// One carousel. Each owns its own [`BookGrid`] so a card belongs to exactly
/// one section and cover patches land in the right place.
struct HomeSection {
    title: SharedString,
    books: BookGrid,
}

pub(crate) struct HomePage {
    ui: LibraryContext,
    sections: Vec<HomeSection>,
    error: Option<SharedString>,
}

impl HomePage {
    pub(crate) fn new(ui: LibraryContext, _cx: &mut Context<Self>) -> Self {
        let recently_read = BookGrid::new(ui.clone(), true);
        let finished = BookGrid::new(ui.clone(), true);
        let recently_added = BookGrid::new(ui.clone(), false);
        let sections = vec![HomeSection { title: "Recently read".into(), books: recently_read }, HomeSection { title: "Finished".into(), books: finished }, HomeSection { title: "Recently added".into(), books: recently_added }];
        Self { ui, sections, error: None }
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let limit = i32::from(self.ui.contracts().browsing_content.home_book_limit);
        match self.ui.library.home(limit) {
            Ok(data) => {
                self.error = None;
                self.install(data, cx);
            }
            Err(error) => {
                log::error!("failed to load Home section: {error}");
                self.error = Some(error.to_string().into());
                cx.notify();
            }
        }
    }

    fn install(&mut self, mut data: HomeBrowseRows, cx: &mut Context<Self>) {
        let threshold = f32::from(self.ui.contracts().browsing_content.finished_progress_threshold);
        let mut in_progress = Vec::new();
        let mut finished = Vec::new();
        for book in data.recently_read.drain(..) {
            if book.progress >= threshold {
                finished.push(book);
            } else {
                in_progress.push(book);
            }
        }
        let recent_ids = in_progress.iter().chain(&finished).map(|book| book.book_id).collect::<HashSet<_>>();
        data.recently_added.retain(|book| !recent_ids.contains(&book.book_id));

        let groups = [in_progress, finished, data.recently_added];
        for (section, books) in self.sections.iter_mut().zip(groups) {
            section.books.set_books(books, cx);
        }
    }
}

impl Render for HomePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let heading_font = heading_font(cx);

        let mut page = components::home_page();
        for (index, section) in self.sections.iter().enumerate() {
            if section.books.is_empty() {
                continue;
            }
            let title = components::carousel_title(section.title.clone(), heading_font.clone());
            let source = format!("home-carousel-{index}");
            let body = section.books.render_carousel(source, cx);
            page = page.child(components::carousel_section().child(title).child(body));
        }

        let entity = cx.entity();
        let retry_label = self.ui.contracts().browsing_states.retry_label.clone();
        components::browser_error_banner(self.error.clone(), retry_label.into(), theme, page.into_any_element(), move |cx| {
            entity.update(cx, |page, cx| page.refresh(cx));
        })
    }
}
