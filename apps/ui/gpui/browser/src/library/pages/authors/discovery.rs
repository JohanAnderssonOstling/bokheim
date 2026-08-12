//! Remote catalogue results for one selected author.

use epub_svelte_lib::AuthorId;
use epub_svelte_lib::catalog::{CatalogPublication, CatalogSource};
use gpui::prelude::*;
use gpui::{Context, IntoElement, Render, SharedString, Task, Window};
use ui_components as components;

use crate::catalog::{CatalogCardGroup, CatalogCardServices};
use crate::library::services::LibraryContext;

struct DiscoveryGroup {
    source: CatalogSource,
    count: usize,
    cards: CatalogCardGroup,
}

pub(crate) struct AuthorDiscoveryView {
    ui: LibraryContext,
    author: Option<AuthorId>,
    groups: Option<Vec<DiscoveryGroup>>,
    error: Option<SharedString>,
    task: Option<Task<()>>,
    /// Whether the reader has asked for this author's catalogue results.
    /// Selecting an author used to send a network search on its own; now
    /// nothing leaves the machine until this is opened, and it closes again
    /// with every new selection.
    expanded: bool,
    cards: CatalogCardServices,
}

impl AuthorDiscoveryView {
    pub(crate) fn new(ui: LibraryContext) -> Self {
        let cards = ui.catalog_cards();
        Self { ui, author: None, groups: None, error: None, task: None, expanded: false, cards }
    }

    pub(crate) fn set_author(&mut self, author: AuthorId, cx: &mut Context<Self>) {
        if self.author == Some(author) {
            return;
        }
        self.task.take();
        self.author = Some(author);
        self.groups = None;
        self.error = None;
        self.expanded = false;
        cx.notify();
    }

    fn toggle(&mut self, cx: &mut Context<Self>) {
        self.expanded = !self.expanded;
        // Results already fetched for this author are kept: reopening the
        // disclosure is free, and closing it is not a reason to discard them.
        if self.expanded && self.groups.is_none() && self.task.is_none() && self.error.is_none() {
            if let Some(author) = self.author {
                self.search(author, cx);
            }
        }
        cx.notify();
    }

    fn search(&mut self, author: AuthorId, cx: &mut Context<Self>) {
        let operation = self.ui.discover_author_books(author);
        self.task = Some(cx.spawn(async move |view, cx| {
            let result = operation.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = view.update(cx, |view, cx| {
                view.task.take();
                match result {
                    Ok(discovery) => {
                        view.install(discovery.result, &discovery.owned_publications, cx);
                    }
                    Err(error) => {
                        log::warn!("failed to discover catalogue books by author: {error}");
                        view.error = Some(error.into());
                        cx.notify();
                    }
                }
            });
        }));
    }

    fn install(&mut self, result: epub_svelte_lib::catalog::CatalogAuthorDiscoveryResult, owned: &std::collections::HashSet<epub_svelte_lib::catalog::CatalogPublicationKey>, cx: &mut Context<Self>) {
        let mut grouped = Vec::<(CatalogSource, Vec<CatalogPublication>)>::new();
        for discovered in result.matches {
            if let Some((_, publications)) = grouped.iter_mut().find(|(source, _)| source.id == discovered.source.id) {
                publications.push(discovered.publication);
            } else {
                grouped.push((discovered.source, vec![discovered.publication]));
            }
        }
        self.groups = Some(
            grouped
                .into_iter()
                .map(|(source, publications)| {
                    let count = publications.len();
                    let cards = self.cards.group(&source, &publications, owned, true, cx);
                    DiscoveryGroup { source, count, cards }
                })
                .collect(),
        );
        cx.notify();
    }
}

impl Render for AuthorDiscoveryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let page_width = (f32::from(window.viewport_size().width) - components::nav_rail_width(f32::from(window.rem_size()))).min(components::AUTHOR_PAGE_MAX_WIDTH);
        let width = (page_width * 2.0 / 3.0 - components::CONTENT_PADDING * 2.0).max(components::BOOK_COVER_WIDTH);
        let contract = &self.ui.contracts().store;
        let columns = if width >= contract.publication_three_column_breakpoint_px as f32 {
            3
        } else if width >= contract.publication_two_column_breakpoint_px as f32 {
            2
        } else {
            1
        };

        let entity = cx.entity();
        let mut section =
            components::author_discovery_section().child(components::author_discovery_disclosure("author-discovery-disclosure", "More books by this author", "Search your catalogues", self.expanded, theme).on_click(move |_, _, cx| {
                entity.update(cx, |view, cx| view.toggle(cx));
            }));

        if !self.expanded {
            return section;
        }
        if self.task.is_some() {
            return section.child(components::browser_inline_note("Searching catalogues…", theme));
        }
        if let Some(error) = &self.error {
            return section.child(gpui_component::alert::Alert::error("author-discovery-error", format!("Catalogue search unavailable: {error}")));
        }
        let Some(groups) = self.groups.as_ref() else { return section };
        if groups.is_empty() {
            return section.child(components::browser_inline_note("No additional books found in the configured catalogues", theme));
        }

        for group in groups {
            section = section.child(components::section_header(group.source.title.clone(), format!("{} results", group.count), theme)).child(group.cards.grid(width, columns));
        }
        section
    }
}
