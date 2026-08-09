//! Remote catalogue results for one selected author.

use bokheim_ui_components as components;
use epub_svelte_lib::AuthorId;
use epub_svelte_lib::catalog::{CatalogPublication, CatalogSource};
use gpui::prelude::*;
use gpui::{Context, FontWeight, IntoElement, Render, SharedString, Task, Window, px};

use crate::catalog::{CatalogCardGroup, CatalogCardServices};
use crate::library::services::LibraryContext;
use crate::stores::heading_font;

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
    cards: CatalogCardServices,
}

impl AuthorDiscoveryView {
    pub(crate) fn new(ui: LibraryContext) -> Self {
        let cards = ui.catalog_cards();
        Self { ui, author: None, groups: None, error: None, task: None, cards }
    }

    pub(crate) fn set_author(&mut self, author: AuthorId, cx: &mut Context<Self>) {
        if self.author == Some(author) {
            return;
        }
        self.task.take();
        self.author = Some(author);
        self.groups = None;
        self.error = None;
        let operation = self.ui.discover_author_books(author);
        self.task = Some(cx.spawn(async move |view, cx| {
            let result = operation.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = view.update(cx, |view, cx| {
                view.task.take();
                match result {
                    Ok(discovery) => {
                        view.install(discovery.result, &discovery.owned_editions, cx);
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

    fn install(&mut self, result: epub_svelte_lib::catalog::CatalogAuthorDiscoveryResult, owned: &std::collections::HashSet<String>, cx: &mut Context<Self>) {
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
        let heading_font = heading_font(cx);
        let page_width = (f32::from(window.viewport_size().width) - components::NAV_RAIL_WIDTH).min(components::AUTHOR_PAGE_MAX_WIDTH);
        let width = (page_width * 2.0 / 3.0 - components::CONTENT_PADDING * 2.0).max(components::BOOK_COVER_WIDTH);
        let contract = &self.ui.contracts().store;
        let columns = if width >= contract.publication_three_column_breakpoint_px as f32 {
            3
        } else if width >= contract.publication_two_column_breakpoint_px as f32 {
            2
        } else {
            1
        };

        let mut section = gpui::div().mt(px(8.0)).flex().flex_col().gap(px(12.0)).child(
            gpui::div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(gpui::div().text_size(gpui::rems(19.0 / 16.0)).font_family(heading_font.clone()).font_weight(FontWeight::SEMIBOLD).child("More books by this author"))
                .child(gpui::div().text_size(gpui::rems(12.0 / 16.0)).text_color(theme.text_muted).child("Available from your catalogues")),
        );

        if self.task.is_some() {
            return section.child(components::browser_inline_message("Searching catalogues…", theme));
        }
        if let Some(error) = &self.error {
            return section.child(components::browser_inline_message(format!("Catalogue search unavailable: {error}"), theme));
        }
        let Some(groups) = self.groups.as_ref() else { return section };
        if groups.is_empty() {
            return section.child(components::browser_inline_message("No additional books found in the configured catalogues", theme));
        }

        for group in groups {
            section = section.child(components::cloud_section_header(group.source.title.clone(), format!("{} results", group.count), heading_font.clone(), theme)).child(group.cards.grid(width, columns));
        }
        section
    }
}
