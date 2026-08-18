//! Landing page: recently read, finished, and recently added carousels.

use std::collections::HashSet;

use gpui::prelude::*;
use gpui::{Context, IntoElement, Render, SharedString, Window};
use gpui_component::VirtualListScrollHandle;
use ui_components as components;

use crate::library::services::{HomeBrowseRows, LatestRequest, LibraryContext};
use crate::library::widgets::BookGrid;

/// One carousel. Each owns its own [`BookGrid`] so a card belongs to exactly
/// one section and cover patches land in the right place, and its own scroll
/// handle so its arrows move that row and no other.
struct HomeSection {
    title: SharedString,
    books: BookGrid,
    scroll: VirtualListScrollHandle,
}

pub(crate) struct HomePage {
    ui: LibraryContext,
    sections: Vec<HomeSection>,
    error: Option<SharedString>,
    refresh: LatestRequest,
    /// Whether a load has resolved at least once.
    ///
    /// Empty sections mean "still loading" before the first result and "this
    /// library has no books" after it. Without this the page cannot tell those
    /// apart, and a library that is still being scanned is described as empty.
    loaded: bool,
}

impl HomePage {
    pub(crate) fn new(ui: LibraryContext, _cx: &mut Context<Self>) -> Self {
        let recently_read = BookGrid::new(ui.clone(), true);
        // A finished book's bar is full on every card in the section, so it
        // draws the same mark for every book and separates none of them; the
        // heading has already said what the bar would.
        let finished = BookGrid::new(ui.clone(), false);
        let recently_added = BookGrid::new(ui.clone(), false);
        let sections = vec![
            HomeSection { title: "Recently read".into(), books: recently_read, scroll: VirtualListScrollHandle::new() },
            HomeSection { title: "Finished".into(), books: finished, scroll: VirtualListScrollHandle::new() },
            HomeSection { title: "Recently added".into(), books: recently_added, scroll: VirtualListScrollHandle::new() },
        ];
        Self { ui, sections, error: None, refresh: LatestRequest::default(), loaded: false }
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
            section.books.set_books(books, cx);
        }
    }

    /// The pair of arrows beside a section title. A carousel holds up to twenty
    /// books and shows three or four, and a mouse without a horizontal wheel
    /// could reach none of the rest; these are the same controls the mobile
    /// shelf carries.
    fn controls(index: usize, scroll: &VirtualListScrollHandle, theme: components::BrowserTheme) -> impl IntoElement {
        let step = |scroll: VirtualListScrollHandle, direction: f32| {
            move |_: &gpui::ClickEvent, window: &mut Window, _: &mut gpui::App| {
                components::step_carousel(scroll.base_handle(), direction, components::BOOK_COVER_WIDTH + components::BOOK_GRID_GAP, window);
            }
        };
        components::carousel_controls()
            .child(components::carousel_arrow_button(format!("home-carousel-{index}-previous"), "Previous", gpui_component::IconName::ChevronLeft, theme).on_click(step(scroll.clone(), -1.0)))
            .child(components::carousel_arrow_button(format!("home-carousel-{index}-next"), "Next", gpui_component::IconName::ChevronRight, theme).on_click(step(scroll.clone(), 1.0)))
    }
}

impl Render for HomePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);

        let mut page = components::home_page();
        let mut rendered_any = false;
        for (index, section) in self.sections.iter().enumerate() {
            if section.books.is_empty() {
                continue;
            }
            rendered_any = true;
            let source = format!("home-carousel-{index}");
            let heading = components::carousel_heading().child(components::carousel_title(section.title.clone())).child(Self::controls(index, &section.scroll, theme));
            let body = section.books.render_carousel(source, &section.scroll, window, cx);
            page = page.child(components::carousel_section().child(heading).child(body));
        }

        // Every carousel is empty either because nothing has loaded yet or
        // because the library holds no books. Saying nothing at all leaves a
        // blank page in both cases.
        if !rendered_any && self.error.is_none() {
            let states = &self.ui.contracts().browsing_states;
            page = page.child(if self.loaded && !self.ui.scanner_running() {
                components::browser_actionable_empty(states.empty_library_title.clone(), states.empty_library_description.clone(), gpui::div(), theme).into_any_element()
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
