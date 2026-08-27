//! Landing page: progress, recent activity, finished, and recently added carousels.

use std::collections::HashSet;

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, IntoElement, Render, SharedString, Subscription, Window, div, px, rems};
use gpui_component::{Disableable, IconName};
use ui_components as components;

use crate::library::pages::library_empty_state;
use crate::library::services::{HomeBrowseRows, LatestRequest, LibraryContentsChanged, LibraryContext};
use crate::library::widgets::{BookCardView, Paginator, PaginatorAxis, PaginatorChild, PaginatorGapPolicy, PaginatorGroup, PaginatorGroupPolicy, PaginatorSizing, PaginatorWidthPolicy};
const HOME_BOOK_PREFERRED_WIDTH_REM: f32 = components::BOOK_COVER_WIDTH / 16.0;
const HOME_BOOK_ASPECT_RATIO: f32 = components::BOOK_COVER_ASPECT_RATIO;
const HOME_BOOK_EXTRA_HEIGHT_REM: f32 = components::BOOK_CARD_TITLE_HEIGHT_REM + components::BOOK_CARD_AUTHOR_HEIGHT_REM;
const HOME_BOOK_LIMIT: i32 = 20;

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
    _updates: Subscription,
}

struct HomeSection {
    id: &'static str,
    title: SharedString,
    paginator: Entity<Paginator>,
}

impl HomeSection {
    fn new(id: &'static str, title: &'static str, cx: &mut Context<HomePage>) -> Self {
        let paginator = cx.new(|_| Paginator::new(id, []).max_rows(1).flow_axis(PaginatorAxis::Horizontal).horizontal_only());
        Self { id, title: title.into(), paginator }
    }

    fn render(&self, index: usize, show_controls: bool, cx: &mut Context<HomePage>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let mut heading = components::carousel_heading(self.title.clone());
        if show_controls {
            let paginator = self.paginator.read(cx);
            let has_previous = paginator.has_previous();
            let has_next = paginator.has_next();
            let can_previous = paginator.can_previous();
            let can_next = paginator.can_next();
            let previous = has_previous.then(|| {
                components::carousel_arrow_button(format!("{}-previous", self.id), "Previous", IconName::ChevronLeft, theme)
                    .disabled(!can_previous)
                    .on_click(cx.listener(move |page, _, _, cx| {
                        page.sections[index].paginator.update(cx, |paginator, cx| paginator.previous(cx));
                    }))
                    .into_any_element()
            });
            let next = has_next.then(|| {
                components::carousel_arrow_button(format!("{}-next", self.id), "Next", IconName::ChevronRight, theme)
                    .disabled(!can_next)
                    .on_click(cx.listener(move |page, _, _, cx| {
                        page.sections[index].paginator.update(cx, |paginator, cx| paginator.next(cx));
                    }))
                    .into_any_element()
            });
            if has_previous || has_next {
                heading = heading.child(components::paginator_controls(previous, next));
            }
        }
        components::carousel_section().child(heading).child(self.paginator.clone()).into_any_element()
    }
}

impl HomePage {
    pub(crate) fn new(ui: LibraryContext, cx: &mut Context<Self>) -> Self {
        let continue_reading = HomeSection::new("home-continue-reading", "Furthest along", cx);
        let recently_read = HomeSection::new("home-recently-read", "Recently read", cx);
        let finished = HomeSection::new("home-finished", "Finished", cx);
        let recently_added = HomeSection::new("home-recently-added", "Recently added", cx);
        let sections = vec![continue_reading, recently_read, finished, recently_added];
        let updates = ui.updates();
        let update_subscription = cx.subscribe(&updates, |page, _, _: &LibraryContentsChanged, cx| page.refresh(cx));
        let mut page = Self { ui, sections, error: None, refresh: LatestRequest::default(), loaded: false, _updates: update_subscription };
        page.refresh(cx);
        page
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let limit = HOME_BOOK_LIMIT;
        let generation = self.refresh.begin();
        let load = crate::services::backend::library_blocking(self.ui.backend(), move |library| library.home(limit));
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
        let threshold = library_model::FINISHED_PROGRESS_THRESHOLD;
        let mut in_progress = Vec::new();
        let mut finished = Vec::new();
        for book in data.recently_read.drain(..) {
            if book.progress >= threshold {
                finished.push(book);
            } else {
                in_progress.push(book);
            }
        }
        let recent_ids = data.most_progress.iter().chain(&in_progress).chain(&finished).map(|book| book.content_hash).collect::<HashSet<_>>();
        data.recently_added.retain(|book| !recent_ids.contains(&book.content_hash));

        let groups = [data.most_progress, in_progress, finished, data.recently_added];
        for (section, books) in self.sections.iter().zip(groups) {
            let children = books
                .into_iter()
                .map(|book| {
                    let card = BookCardView::create(book, self.ui.clone(), cx);
                    let prepared_card = card.clone();
                    let ready_card = card.clone();
                    let open_card = card.clone();
                    PaginatorChild::new(move |_, _| div().w_full().min_w_0().child(card.clone()).into_any_element())
                        .with_activate(move |window, cx| {
                            let _ = open_card.update(cx, |card, card_cx| card.open(window, card_cx));
                        })
                        .with_prepare(move |cx| prepared_card.update(cx, |card, cx| card.prepare(cx)))
                        .with_ready(move |cx| ready_card.read(cx).is_prepared())
                })
                .collect::<Vec<_>>();
            let policy = PaginatorGroupPolicy::new(
                // Home is a set of horizontal shelves, not a fill-the-window
                // grid. Keeping every card at the established cover width
                // also keeps the paginator's intrinsic row height identical
                // to the geometry it paints when a shelf contains only a few
                // books.
                PaginatorWidthPolicy::fixed(rems(HOME_BOOK_PREFERRED_WIDTH_REM)),
                PaginatorSizing::aspect_ratio(HOME_BOOK_ASPECT_RATIO).with_rems(rems(HOME_BOOK_EXTRA_HEIGHT_REM)).with_pixels(px(components::BOOK_CARD_FIXED_HEIGHT)),
                PaginatorGapPolicy::new(px(components::BOOK_GRID_GAP), px(components::BOOK_GRID_BOOK_GAP)).with_maximum(px(components::BOOK_GRID_MAX_GAP)),
                PaginatorGapPolicy::new(px(components::BOOK_GRID_GAP), px(components::BOOK_GRID_BOOK_GAP)),
            );
            section.paginator.update(cx, |paginator, cx| paginator.set_groups([PaginatorGroup::new(policy, children)], cx));
        }
    }
}

impl Render for HomePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let show_paginator_controls = !cx.supports_touch_input();

        let mut page = components::home_page();
        let mut rendered_any = false;
        for (index, section) in self.sections.iter().enumerate() {
            if section.paginator.read(cx).is_empty() {
                continue;
            }
            rendered_any = true;
            page = page.child(section.render(index, show_paginator_controls, cx));
        }

        // Every carousel is empty either because nothing has loaded yet or
        // because the library holds no books. Saying nothing at all leaves a
        // blank page in both cases.
        if !rendered_any && self.error.is_none() {
            page = page.child(if self.loaded && !self.ui.backend().scanner_running() {
                library_empty_state(theme)
            } else {
                let label = if self.ui.backend().scanner_running() { "Scanning library…" } else { "Loading library…" };
                components::browser_loading_message(label, theme).into_any_element()
            });
        }

        let entity = cx.entity();
        if let Some(error) = self.error.clone() {
            let retry = components::topbar_action_button("retry-section", "Retry", IconName::Redo2, theme).on_click(move |_, _, cx| entity.update(cx, |page, cx| page.refresh(cx)));
            components::browser_error_banner(error, retry, page.into_any_element()).into_any_element()
        } else {
            page.into_any_element()
        }
    }
}
