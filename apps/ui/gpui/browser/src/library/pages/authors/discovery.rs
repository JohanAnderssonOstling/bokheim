//! Remote catalogue results for one selected author.

use epub_svelte_lib::AuthorId;
use epub_svelte_lib::catalog::{CatalogPublication, CatalogSource};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, SharedString, Task, div, px};
use gpui_component::alert::Alert;
use ui_components as components;

use crate::catalog::{CatalogCardGroup, CatalogCardItem, CatalogCardServices};
use crate::library::services::LibraryContext;

const DISCLOSURE_ROW_HEIGHT: f32 = 58.0;
const STATUS_ROW_HEIGHT: f32 = 66.0;
const SOURCE_ROW_HEIGHT: f32 = 58.0;

#[derive(Clone)]
pub(super) enum AuthorDiscoveryVirtualRow {
    Disclosure { expanded: bool },
    Status(SharedString),
    Error(SharedString),
    SourceHeader { title: String, detail: String },
    Publications(Vec<CatalogCardItem>),
}

impl AuthorDiscoveryVirtualRow {
    pub(super) fn content_height(&self, publication_height: f32) -> f32 {
        match self {
            Self::Disclosure { .. } => DISCLOSURE_ROW_HEIGHT,
            Self::Status(_) | Self::Error(_) => STATUS_ROW_HEIGHT,
            Self::SourceHeader { .. } => SOURCE_ROW_HEIGHT,
            Self::Publications(_) => publication_height,
        }
    }
}

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

    pub(super) fn virtual_rows(&self, columns: usize) -> Vec<AuthorDiscoveryVirtualRow> {
        let mut rows = vec![AuthorDiscoveryVirtualRow::Disclosure { expanded: self.expanded }];
        if !self.expanded {
            return rows;
        }
        if self.task.is_some() {
            rows.push(AuthorDiscoveryVirtualRow::Status("Searching catalogues…".into()));
            return rows;
        }
        if let Some(error) = &self.error {
            rows.push(AuthorDiscoveryVirtualRow::Error(format!("Catalogue search unavailable: {error}").into()));
            return rows;
        }
        let Some(groups) = self.groups.as_ref() else { return rows };
        if groups.is_empty() {
            rows.push(AuthorDiscoveryVirtualRow::Status("No additional books found in the configured catalogues".into()));
            return rows;
        }
        for group in groups {
            rows.push(AuthorDiscoveryVirtualRow::SourceHeader { title: group.source.title.clone(), detail: format!("{} results", group.count) });
            rows.extend(group.cards.packed_rows(columns).into_iter().map(AuthorDiscoveryVirtualRow::Publications));
        }
        rows
    }

    pub(super) fn render_virtual_row(
        row: &AuthorDiscoveryVirtualRow, index: usize, entity: Entity<Self>, columns: usize, unit_width: f32, publication_height: f32, trailing_gap: f32, theme: components::BrowserTheme,
    ) -> AnyElement {
        let content = match row {
            AuthorDiscoveryVirtualRow::Disclosure { expanded } => {
                components::author_discovery_disclosure("author-discovery-disclosure", "More books by this author", "Search your catalogues", *expanded, theme)
                    .on_click(move |_, _, cx| entity.update(cx, |view, cx| view.toggle(cx)))
                    .into_any_element()
            }
            AuthorDiscoveryVirtualRow::Status(message) => components::browser_inline_note(message.clone(), theme).into_any_element(),
            AuthorDiscoveryVirtualRow::Error(message) => Alert::error(format!("author-discovery-error-{index}"), message.clone()).into_any_element(),
            AuthorDiscoveryVirtualRow::SourceHeader { title, detail } => components::section_header(title.clone(), detail.clone(), theme).into_any_element(),
            AuthorDiscoveryVirtualRow::Publications(items) => {
                let cards = items.iter().map(|item| (item.card.clone(), item.span)).collect::<Vec<_>>();
                crate::virtual_grid::fixed_spanned_entity_row(&cards, columns, unit_width, publication_height, 16.0).into_any_element()
            }
        };
        div().w_full().h(px(row.content_height(publication_height) + trailing_gap)).flex_none().child(content).into_any_element()
    }
}
