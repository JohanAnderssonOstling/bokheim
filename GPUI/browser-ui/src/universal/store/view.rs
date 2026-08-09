//! Store presentation. Behaviour and retained state stay in the parent page.

use super::browse::source_detail;
use super::*;

impl StorePage {
    fn catalog_link(&self, link: &CatalogLink, id: String, theme: components::BrowserTheme, cx: &mut Context<Self>) -> AnyElement {
        let label = link.number_of_items.map_or_else(|| link.title.clone(), |count| format!("{} ({count})", link.title));
        match &link.target {
            CatalogLinkTarget::Page { locator } => {
                let locator = locator.clone();
                let entity = cx.entity();
                let open = components::folder_navigation_button(id, label, IconName::Folder, theme).disabled(link.active || self.loading).on_click(move |_, _, cx| entity.update(cx, |page, cx| page.load_page(Some(locator.clone()), cx)));
                components::folder_card(open, None::<Button>).into_any_element()
            }
            CatalogLinkTarget::External { url } => {
                let url = url.clone();
                let open = components::folder_navigation_button(id, label, IconName::ExternalLink, theme).disabled(link.active).on_click(move |_, _, cx| cx.open_url(&url));
                components::folder_card(open, None::<Button>).into_any_element()
            }
        }
    }

    fn catalog_facet(&self, link: &CatalogLink, id: String, cx: &mut Context<Self>) -> Button {
        let label = link.number_of_items.map_or_else(|| link.title.clone(), |count| format!("{} ({count})", link.title));
        match &link.target {
            CatalogLinkTarget::Page { locator } => {
                let locator = locator.clone();
                let entity = cx.entity();
                components::catalog_facet_button(id, label, link.active, self.loading).on_click(move |_, _, cx| entity.update(cx, |page, cx| page.load_page(Some(locator.clone()), cx)))
            }
            CatalogLinkTarget::External { url } => {
                let url = url.clone();
                components::catalog_facet_button(id, label, link.active, false).on_click(move |_, _, cx| cx.open_url(&url))
            }
        }
    }
}

impl Render for StorePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let contract = self.services.contracts.store.clone();
        let heading_font = crate::stores::heading_font(cx);
        let width = (f32::from(window.viewport_size().width) - components::NAV_RAIL_WIDTH - components::CONTENT_PADDING * 2.0).max(components::BOOK_COVER_WIDTH).min(contract.max_width_px as f32);
        let publication_columns = if width >= contract.publication_three_column_breakpoint_px as f32 {
            3
        } else if width >= contract.publication_two_column_breakpoint_px as f32 {
            2
        } else {
            1
        };

        let mut content = div().w_full().max_w(px(contract.max_width_px as f32)).mx_auto().flex().flex_col().gap(px(18.0));

        if let Some(error) = &self.error {
            let entity = cx.entity();
            content = content.child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .child(components::cloud_error(error.clone(), theme))
                    .child(components::cloud_secondary_button("store-retry", contract.retry_label.clone(), IconName::Redo2, false).on_click(move |_, _, cx| entity.update(cx, |page, cx| page.refresh(cx)))),
            );
        }

        let mode = self.mode.clone();
        let sources = self.sources.clone();
        let StoreMode::Catalogue(page) = mode else {
            let entity = cx.entity();
            content = content.child(
                div()
                    .w_full()
                    .max_w(px(480.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(div().flex_1().min_w_0().child(Input::new(&self.wide_search_input)))
                    .child(components::cloud_primary_button("store-wide-search", contract.search_label.clone(), IconName::Search, self.loading).on_click(move |_, _, cx| entity.update(cx, |page, cx| page.search_catalogs(cx)))),
            );

            if let StoreMode::WideSearch(search) = mode {
                let entity = cx.entity();
                content = content.child(
                    div().flex().items_center().justify_between().child(div().text_size(gpui::rems(19.0 / 16.0)).font_family(heading_font.clone()).font_weight(FontWeight::SEMIBOLD).child(format!("Results for “{}”", search.query))).child(
                        components::cloud_secondary_button("store-clear-wide", "Clear search", IconName::Close, false).on_click(move |_, _, cx| {
                            entity.update(cx, |page, cx| {
                                page.show_catalogues(cx);
                            });
                        }),
                    ),
                );
                for (group_index, group) in search.groups.iter().enumerate() {
                    let entity = cx.entity();
                    let source = group.source.clone();
                    let mut group_view = components::cloud_section(theme).child(
                        div().flex().items_center().justify_between().gap(px(12.0)).child(components::cloud_section_header(group.source.title.clone(), source_detail(&group.source), heading_font.clone(), theme)).child(
                            components::cloud_secondary_button(format!("store-wide-open-{group_index}"), "Open catalog", IconName::ChevronRight, false).on_click(move |_, _, cx| {
                                entity.update(cx, |page, cx| page.select_source(source.clone(), cx));
                            }),
                        ),
                    );
                    group_view = match &group.outcome {
                        WideSearchOutcome::Results { cards, external_links: links } => {
                            let results = div().w_full().flex().flex_col().gap(px(10.0)).child(cards.grid(width - 36.0, publication_columns));
                            let mut external_links = components::cloud_actions();
                            for (link_index, link) in links.iter().enumerate() {
                                external_links = external_links.child(self.catalog_link(link, format!("store-wide-link-{group_index}-{link_index}"), theme, cx));
                            }
                            group_view.child(results.child(external_links))
                        }
                        WideSearchOutcome::AuthenticationRequired => group_view.child(components::cloud_message("This catalogue requires sign-in, which is not supported.", theme)),
                        WideSearchOutcome::Unsupported => group_view.child(components::cloud_message("This catalog does not support search.", theme)),
                        WideSearchOutcome::TimedOut => group_view.child(components::cloud_message("This catalog did not respond in time.", theme)),
                        WideSearchOutcome::Failed(message) => group_view.child(components::cloud_error(message.clone(), theme)),
                    };
                    content = content.child(group_view);
                }
            } else {
                let source_columns: usize = if width >= contract.catalog_three_column_breakpoint_px as f32 {
                    3
                } else if width >= contract.catalog_two_column_breakpoint_px as f32 {
                    2
                } else {
                    1
                };
                let card_width = (width - STORE_GRID_GAP * source_columns.saturating_sub(1) as f32) / source_columns as f32;
                let mut source_grid = div().w_full().flex().flex_row().flex_wrap().gap(px(STORE_GRID_GAP));
                for source in sources.iter().cloned() {
                    let entity = cx.entity();
                    let clicked = source.clone();
                    source_grid = source_grid.child(
                        div()
                            .id(format!("store-source-{}", source.id.as_str()))
                            .w(px(card_width))
                            .min_h(px(76.0))
                            .p(px(14.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .rounded(px(12.0))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.card_bg)
                            .cursor_pointer()
                            .hover(move |style| style.bg(theme.surface_alt))
                            .child(
                                div()
                                    .size(px(44.0))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(9.0))
                                    .bg(theme.accent_soft)
                                    .text_color(theme.accent)
                                    .font_weight(FontWeight::BOLD)
                                    .child(source.title.chars().next().unwrap_or('?').to_string()),
                            )
                            .child(components::cloud_row_identity(source.title.clone(), format!("{}\n{}", source.description, source_detail(&source)), theme))
                            .child(gpui_component::Icon::new(IconName::ChevronRight).text_color(theme.text_muted))
                            .on_click(move |_, _, cx| entity.update(cx, |page, cx| page.select_source(clicked.clone(), cx))),
                    );
                }
                content = content.child(source_grid);
            }
            return components::cloud_page().child(content).into_any_element();
        };

        let entity = cx.entity();
        content = content.child(
            div()
                .flex()
                .items_center()
                .gap(px(16.0))
                .child(components::cloud_secondary_button("store-back", contract.catalogs_label.clone(), IconName::ArrowLeft, false).on_click(move |_, _, cx| {
                    entity.update(cx, |page, cx| {
                        page.show_catalogues(cx);
                    });
                }))
                .child(components::cloud_section_header(page.source.title.clone(), source_detail(&page.source), heading_font.clone(), theme)),
        );

        {
            let mut section = div().w_full().flex().flex_col().gap(px(14.0));

            if page.search.is_some() {
                let entity = cx.entity();
                section = section.child(
                    div()
                        .w_full()
                        .max_w(px(480.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(div().flex_1().min_w_0().child(Input::new(&self.search_input)))
                        .child(components::cloud_primary_button("store-search", contract.search_label.clone(), IconName::Search, self.loading).on_click(move |_, _, cx| entity.update(cx, |page, cx| page.search_within_source(cx)))),
                );
            }

            if !page.levels.is_empty() {
                let mut breadcrumbs = components::folder_breadcrumb();
                for (index, (title, locator)) in page.levels.iter().enumerate() {
                    let current = index + 1 == page.levels.len();
                    let item = components::folder_breadcrumb_item(title.clone(), current);
                    breadcrumbs = if current || self.loading {
                        breadcrumbs.child(item)
                    } else {
                        let locator = locator.clone();
                        let entity = cx.entity();
                        breadcrumbs.child(item.on_click(move |_, _, cx| entity.update(cx, |page, cx| page.load_page(Some(locator.clone()), cx))))
                    };
                }
                section = section.child(breadcrumbs);
            }

            let mut links = components::folder_buttons();
            for (index, link) in page.navigation.iter().enumerate() {
                links = links.child(self.catalog_link(link, format!("store-link-{index}"), theme, cx));
            }
            section = section.child(links);

            for (facet_index, facet) in page.facets.iter().enumerate() {
                let mut options = div().flex().flex_row().flex_wrap().gap(px(8.0));
                for (link_index, link) in facet.links.iter().enumerate() {
                    options = options.child(self.catalog_facet(link, format!("store-facet-{facet_index}-{link_index}"), cx));
                }
                section = section.child(components::cloud_section(theme).child(components::cloud_section_header(facet.title.clone(), "Filter", heading_font.clone(), theme)).child(options));
            }

            section = section.child(page.publications.grid(width, publication_columns));
            for (group_index, group) in page.groups.iter().enumerate() {
                let mut group_header =
                    div().w_full().flex().items_center().justify_between().gap(px(10.0)).child(components::cloud_section_header(group.title.clone(), format!("{} books", group.number_of_items), heading_font.clone(), theme));
                if let Some(locator) = group.page_locator.clone() {
                    let entity = cx.entity();
                    group_header = group_header.child(
                        components::cloud_secondary_button(format!("store-group-all-{group_index}"), "View all", IconName::ChevronRight, self.loading)
                            .on_click(move |_, _, cx| entity.update(cx, |page, cx| page.load_page(Some(locator.clone()), cx))),
                    );
                }
                section = section.child(group_header);
                if !group.navigation.is_empty() {
                    let mut group_links = components::folder_buttons();
                    for (link_index, link) in group.navigation.iter().enumerate() {
                        group_links = group_links.child(self.catalog_link(link, format!("store-group-link-{group_index}-{link_index}"), theme, cx));
                    }
                    section = section.child(group_links);
                }
                section = section.child(group.cards.grid(width, publication_columns));
            }

            let mut pagination = components::cloud_actions();
            if let Some(previous) = page.pagination.previous.clone() {
                let entity = cx.entity();
                pagination =
                    pagination.child(components::cloud_secondary_button("store-previous", "Previous", IconName::ArrowLeft, self.loading).on_click(move |_, _, cx| entity.update(cx, |page, cx| page.load_page(Some(previous.clone()), cx))));
            }
            if let Some(next) = page.pagination.next.clone() {
                let entity = cx.entity();
                pagination = pagination.child(components::cloud_secondary_button("store-next", "Next", IconName::ArrowRight, self.loading).on_click(move |_, _, cx| entity.update(cx, |page, cx| page.load_page(Some(next.clone()), cx))));
            }
            content = content.child(section.child(div().flex().justify_center().child(pagination)));
        }

        components::cloud_page().child(content).into_any_element()
    }
}
