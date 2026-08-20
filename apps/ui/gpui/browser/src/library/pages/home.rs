//! Landing page: recently read, finished, and recently added carousels.

use std::collections::HashSet;

use gpui::prelude::*;
use gpui::{Context, Entity, IntoElement, Render, SharedString, Task, Window};
use ui_components as components;

use crate::library::pages::library_empty_state;
use crate::library::services::{HomeBrowseRows, LatestRequest, LibraryContext};
use crate::library::widgets::BookCarousel;
use crate::library::{DetailOwner, context_with_page_detail, detail::BookDetailPage, page_detail};

pub(crate) struct HomePage {
    ui: LibraryContext,
    detail: Option<Entity<BookDetailPage>>,
    sections: Vec<BookCarousel>,
    error: Option<SharedString>,
    refresh: LatestRequest,
    /// Whether a load has resolved at least once.
    ///
    /// Empty sections mean "still loading" before the first result and "this
    /// library has no books" after it. Without this the page cannot tell those
    /// apart, and a library that is still being scanned is described as empty.
    loaded: bool,
    _updates: Task<()>,
}

impl HomePage {
    pub(crate) fn new(ui: LibraryContext, cx: &mut Context<Self>) -> Self {
        let ui = context_with_page_detail(ui, cx);
        let recently_read = BookCarousel::new("Recently read", ui.clone(), true);
        // A finished book's bar is full on every card in the section, so it
        // draws the same mark for every book and separates none of them; the
        // heading has already said what the bar would.
        let finished = BookCarousel::new("Finished", ui.clone(), false);
        let recently_added = BookCarousel::new("Recently added", ui.clone(), false);
        let sections = vec![recently_read, finished, recently_added];
        let updates = ui.updates();
        let update_task = cx.spawn(async move |page, cx| {
            while let Ok(update) = updates.recv().await {
                let _ = page.update(cx, |page, cx| match update {
                    epub_svelte_lib::app::LibraryUpdate::Contents => page.refresh(cx),
                    epub_svelte_lib::app::LibraryUpdate::Covers(content_hashes) => crate::library::invalidate_covers(page.ui.library.id().clone(), &content_hashes, cx),
                });
            }
        });
        let mut page = Self { ui, detail: None, sections, error: None, refresh: LatestRequest::default(), loaded: false, _updates: update_task };
        page.refresh(cx);
        page
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let limit = i32::from(self.ui.contracts().browsing_content.home_book_limit);
        let generation = self.refresh.begin();
        let load = self.ui.load_home(limit);
        let task = cx.spawn(async move |page, cx| {
            let result = load.await;
            let _ = page.update(cx, |page, cx| {
                if !page.refresh.is_current(generation) {
                    return;
                }
                // Either outcome settles the question of whether the page is
                // still waiting: an error surfaces its own banner and must not
                // leave the body claiming to be loading forever.
                page.loaded = true;
                match result {
                    Ok(data) => {
                        page.error = None;
                        page.install(data, cx);
                        cx.notify();
                    }
                    Err(error) => {
                        log::error!("failed to load Home section: {error}");
                        page.error = Some(error.into());
                        cx.notify();
                    }
                }
            });
        });
        self.refresh.install(task);
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
        let recent_ids = in_progress.iter().chain(&finished).map(|book| book.content_hash.clone()).collect::<HashSet<_>>();
        data.recently_added.retain(|book| !recent_ids.contains(&book.content_hash));

        let groups = [in_progress, finished, data.recently_added];
        for (section, books) in self.sections.iter_mut().zip(groups) {
            section.set_books(books, cx);
        }
    }
}

impl Render for HomePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(detail) = page_detail(self) {
            return detail;
        }
        let theme = components::browser_theme(cx);

        let mut page = components::home_page();
        let mut rendered_any = false;
        for (index, section) in self.sections.iter().enumerate() {
            if section.is_empty() {
                continue;
            }
            rendered_any = true;
            page = page.child(section.render(index, window, cx));
        }

        // Every carousel is empty either because nothing has loaded yet or
        // because the library holds no books. Saying nothing at all leaves a
        // blank page in both cases.
        if !rendered_any && self.error.is_none() {
            let states = &self.ui.contracts().browsing_states;
            page = page.child(if self.loaded && !self.ui.scanner_running() {
                library_empty_state(theme)
            } else {
                let label = if self.ui.scanner_running() { "Scanning library…".into() } else { states.loading_library_label.clone() };
                components::browser_loading_message(label, theme).into_any_element()
            });
        }

        let entity = cx.entity();
        let retry_label = self.ui.contracts().browsing_states.retry_label.clone();
        components::browser_error_banner(self.error.clone(), retry_label.into(), theme, page.into_any_element(), move |cx| {
            entity.update(cx, |page, cx| page.refresh(cx));
        })
    }
}

impl DetailOwner for HomePage {
    fn detail(&self) -> Option<&Entity<BookDetailPage>> {
        self.detail.as_ref()
    }

    fn detail_mut(&mut self) -> &mut Option<Entity<BookDetailPage>> {
        &mut self.detail
    }
}
