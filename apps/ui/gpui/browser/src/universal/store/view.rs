//! Store presentation. Behaviour and retained state stay in the parent page.

use super::browse::source_detail;
use super::*;
use gpui::{size, Entity};
use gpui_component::alert::Alert;
use gpui_component::group_box::{GroupBox, GroupBoxVariants};
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::scroll::ScrollableElement;
use gpui_component::v_virtual_list;
use gpui_component::{Sizable, Size};

use crate::catalog::CatalogPublicationCardView;
use crate::virtual_grid;

const STORE_SECTION_ROW_HEIGHT: f32 = 58.0;
const STORE_SOURCE_ROW_HEIGHT: f32 = 66.0;
const STORE_LINK_ROW_HEIGHT: f32 = 54.0;
const STORE_STATUS_ROW_HEIGHT: f32 = 66.0;

#[derive(Clone)]
enum StoreVirtualRow {
    SectionHeader { title: String, detail: String, locator: Option<String> },
    SourceHeader(CatalogSource),
    Publications(Vec<Entity<CatalogPublicationCardView>>),
    Links { links: Vec<CatalogLink>, id_prefix: String },
    Status { source: Option<CatalogSource>, message: String, error: bool },
}

impl StoreVirtualRow {
    fn height(&self, publication_height: f32) -> f32 {
        match self {
            Self::SectionHeader { .. } => STORE_SECTION_ROW_HEIGHT,
            Self::SourceHeader(_) => STORE_SOURCE_ROW_HEIGHT,
            Self::Publications(_) => publication_height,
            Self::Links { .. } => STORE_LINK_ROW_HEIGHT,
            Self::Status { .. } => STORE_STATUS_ROW_HEIGHT,
        }
    }
}

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

    fn append_publication_rows(rows: &mut Vec<StoreVirtualRow>, cards: &CatalogCardGroup, columns: usize) {
        for row in 0..virtual_grid::row_count(cards.len(), columns) {
            let range = virtual_grid::row_range(row, columns, cards.len());
            rows.push(StoreVirtualRow::Publications(cards.cards()[range].to_vec()));
        }
    }

    fn virtual_rows(&self, id: &'static str, rows: Vec<StoreVirtualRow>, width: f32, columns: usize, theme: components::BrowserTheme, cx: &mut Context<Self>) -> AnyElement {
        let publication_height = self.services.contracts.store.publication_height_px as f32;
        let card_width = virtual_grid::item_width(width, columns, STORE_GRID_GAP);
        let sizes = Rc::new(rows.iter().map(|row| size(px(width), px(row.height(publication_height)))).collect::<Vec<_>>());
        let rows = Rc::new(rows);
        let list = v_virtual_list(cx.entity(), id, sizes, move |page, range, _, cx| {
            range
                .map(|index| match &rows[index] {
                    StoreVirtualRow::SectionHeader { title, detail, locator } => {
                        let mut row = div().w_full().h(px(STORE_SECTION_ROW_HEIGHT)).flex_none().flex().items_center().justify_between().gap(px(10.0)).child(components::section_header(title.clone(), detail.clone(), theme));
                        if let Some(locator) = locator.clone() {
                            let entity = cx.entity();
                            row = row.child(
                                components::secondary_action_button(format!("{id}-section-{index}"), "View all", IconName::ChevronRight, page.loading)
                                    .on_click(move |_, _, cx| entity.update(cx, |page, cx| page.load_page(Some(locator.clone()), cx))),
                            );
                        }
                        row.into_any_element()
                    }
                    StoreVirtualRow::SourceHeader(source) => {
                        let source_to_open = source.clone();
                        let entity = cx.entity();
                        div()
                            .w_full()
                            .h(px(STORE_SOURCE_ROW_HEIGHT))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.0))
                            .child(components::section_header(source.title.clone(), source_detail(source), theme))
                            .child(
                                components::secondary_action_button(format!("{id}-source-{index}"), "Open catalog", IconName::ChevronRight, page.loading)
                                    .on_click(move |_, _, cx| entity.update(cx, |page, cx| page.select_source(source_to_open.clone(), cx))),
                            )
                            .into_any_element()
                    }
                    StoreVirtualRow::Publications(cards) => virtual_grid::entity_row(cards, 0..cards.len(), card_width, publication_height, STORE_GRID_GAP).into_any_element(),
                    StoreVirtualRow::Links { links, id_prefix } => {
                        let mut row = div().w_full().h(px(STORE_LINK_ROW_HEIGHT)).flex_none().flex().flex_row().items_center().gap(px(8.0)).overflow_x_scrollbar();
                        for (link_index, link) in links.iter().enumerate() {
                            row = row.child(page.catalog_link(link, format!("{id_prefix}-{link_index}"), theme, cx));
                        }
                        row.into_any_element()
                    }
                    StoreVirtualRow::Status { source, message, error } => {
                        let mut row = div().w_full().h(px(STORE_STATUS_ROW_HEIGHT)).flex_none().flex().items_center().justify_between().gap(px(12.0));
                        if let Some(source) = source.clone() {
                            let source_to_open = source.clone();
                            let entity = cx.entity();
                            row = row.child(div().flex_1().min_w_0().child(components::section_header(source.title, message.clone(), theme))).child(
                                components::secondary_action_button(format!("{id}-status-{index}"), "Open catalog", IconName::ChevronRight, page.loading)
                                    .on_click(move |_, _, cx| entity.update(cx, |page, cx| page.select_source(source_to_open.clone(), cx))),
                            );
                        } else {
                            let alert = if *error { Alert::error(format!("{id}-error-{index}"), message.clone()) } else { Alert::info(format!("{id}-info-{index}"), message.clone()) };
                            row = row.child(alert.with_size(Size::Small));
                        }
                        row.into_any_element()
                    }
                })
                .collect::<Vec<_>>()
        })
        .w_full()
        .flex_1()
        .min_h_0()
        .gap(px(STORE_GRID_GAP));
        components::sized_virtual_list(list).w_full().flex_1().min_h_0().into_any_element()
    }
}

impl Render for StorePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let contract = self.services.contracts.store.clone();
        let settings = self.services.contracts.settings.clone();
        let browsing_contract = self.services.contracts.browsing_preferences.clone();
        let preferences = self.preferences.read(cx).browsing();
        let width = (f32::from(window.viewport_size().width) - components::nav_rail_width(f32::from(window.rem_size())) - components::CONTENT_PADDING * 2.0).max(components::BOOK_COVER_WIDTH).min(contract.max_width_px as f32);
        let publication_columns = if width >= contract.publication_three_column_breakpoint_px as f32 {
            3
        } else if width >= contract.publication_two_column_breakpoint_px as f32 {
            2
        } else {
            1
        };

        let mut content = div().w_full().max_w(px(contract.max_width_px as f32)).mx_auto().flex_1().min_h_0().flex().flex_col().gap(px(18.0));

        if let Some(error) = &self.error {
            let entity = cx.entity();
            content = content.child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .child(Alert::error("store-error", error.clone()).with_size(Size::Small))
                    .child(components::secondary_action_button("store-retry", contract.retry_label.clone(), IconName::Redo2, false).on_click(move |_, _, cx| entity.update(cx, |page, cx| page.refresh(cx)))),
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
                    .child(components::primary_action_button("store-wide-search", contract.search_label.clone(), IconName::Search, self.loading).on_click(move |_, _, cx| entity.update(cx, |page, cx| page.search_catalogs(cx)))),
            );

            let all_values = browsing_contract.options.catalog_languages.iter().map(|option| option.value.clone()).collect::<Vec<_>>();
            let all_selected = preferences.included_catalog_languages().is_empty();
            let all_entity = cx.entity();
            let mut language_choices =
                components::browser_settings_chip_group().child(components::browser_settings_chip_button("store-language-all", settings.all_catalog_languages_label.clone(), all_selected, theme).on_click(move |_, _, cx| {
                    all_entity.update(cx, |page, cx| page.update_catalog_languages(Vec::new(), cx));
                }));
            for (index, option) in browsing_contract.options.catalog_languages.iter().enumerate() {
                let active = all_selected || preferences.included_catalog_languages().contains(&option.value);
                let mut selected = if all_selected { all_values.clone() } else { preferences.included_catalog_languages().to_vec() };
                if active {
                    selected.retain(|language| language != &option.value);
                } else {
                    selected.push(option.value.clone());
                    selected.sort();
                    selected.dedup();
                }
                if selected.len() == all_values.len() {
                    selected.clear();
                }
                let entity = cx.entity();
                language_choices = language_choices.child(components::browser_settings_chip_button(format!("store-language-{index}"), option.label.clone(), active, theme).on_click(move |_, _, cx| {
                    entity.update(cx, |page, cx| page.update_catalog_languages(selected.clone(), cx));
                }));
            }
            content = content.child(
                components::browser_settings_fieldset(settings.included_catalog_languages_label.clone(), theme)
                    .child(div().text_size(gpui::rems(11.0 / 16.0)).text_color(theme.text_muted).child(settings.included_catalog_languages_description.clone()))
                    .child(language_choices),
            );

            if let StoreMode::WideSearch(search) = mode {
                let entity = cx.entity();
                content = content.child(div().flex().items_center().justify_between().child(components::section_title(format!("Results for “{}”", search.query))).child(
                    components::secondary_action_button("store-clear-wide", "Clear search", IconName::Close, false).on_click(move |_, _, cx| {
                        entity.update(cx, |page, cx| {
                            page.show_catalogues(cx);
                        });
                    }),
                ));
                let mut rows = Vec::new();
                for section in &search.sections {
                    rows.push(StoreVirtualRow::SectionHeader { title: section.title.clone(), detail: String::new(), locator: None });
                    for source in &section.sources {
                        rows.push(StoreVirtualRow::SourceHeader(source.source.clone()));
                        Self::append_publication_rows(&mut rows, &source.cards, publication_columns);
                        if !source.external_links.is_empty() {
                            rows.push(StoreVirtualRow::Links { links: source.external_links.clone(), id_prefix: format!("store-wide-link-{}", source.source.id.as_str()) });
                        }
                    }
                }
                if !search.statuses.is_empty() {
                    rows.push(StoreVirtualRow::SectionHeader { title: "Other catalogues".to_owned(), detail: String::new(), locator: None });
                    for status in &search.statuses {
                        let (message, error) = match &status.outcome {
                            WideSearchStatusKind::NoMatches => ("No matching books were returned.".to_owned(), false),
                            WideSearchStatusKind::Unsupported => ("This catalog does not support search.".to_owned(), false),
                            WideSearchStatusKind::TimedOut => ("This catalog did not respond in time.".to_owned(), false),
                            WideSearchStatusKind::Failed(message) => (message.clone(), true),
                        };
                        rows.push(StoreVirtualRow::Status { source: Some(status.source.clone()), message, error });
                        if !status.external_links.is_empty() {
                            rows.push(StoreVirtualRow::Links { links: status.external_links.clone(), id_prefix: format!("store-wide-status-link-{}", status.source.id.as_str()) });
                        }
                    }
                }
                if rows.is_empty() {
                    rows.push(StoreVirtualRow::Status { source: None, message: "No matching books were returned.".to_owned(), error: false });
                }
                content = content.child(self.virtual_rows("store-wide-results", rows, width, publication_columns, theme, cx));
                return div().size_full().min_h_0().min_w_0().p(px(18.0)).flex().flex_col().child(content).into_any_element();
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
                            .rounded(px(0.0))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.raised_bg)
                            .cursor_pointer()
                            .hover(move |style| style.bg(theme.surface_alt))
                            .child(
                                div()
                                    .size(px(44.0))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(0.0))
                                    .bg(theme.accent_soft)
                                    .text_color(theme.accent)
                                    .font_weight(FontWeight::BOLD)
                                    .child(source.title.chars().next().unwrap_or('?').to_string()),
                            )
                            .child(components::item_identity(source.title.clone(), format!("{}\n{}", source.description, source_detail(&source)), theme))
                            .child(gpui_component::Icon::new(IconName::ChevronRight).text_color(theme.text_muted))
                            .on_click(move |_, _, cx| entity.update(cx, |page, cx| page.select_source(clicked.clone(), cx))),
                    );
                }
                content = content.child(source_grid);
            }
            return components::scrollable_page().child(content).into_any_element();
        };

        let entity = cx.entity();
        content = content.child(
            div()
                .flex()
                .items_center()
                .gap(px(16.0))
                .child(components::secondary_action_button("store-back", contract.catalogs_label.clone(), IconName::ArrowLeft, false).on_click(move |_, _, cx| {
                    entity.update(cx, |page, cx| {
                        page.show_catalogues(cx);
                    });
                }))
                .child(components::section_header(page.source.title.clone(), source_detail(&page.source), theme)),
        );

        {
            let mut section = div().w_full().flex_1().min_h_0().flex().flex_col().gap(px(14.0));

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
                        .child(components::primary_action_button("store-search", contract.search_label.clone(), IconName::Search, self.loading).on_click(move |_, _, cx| entity.update(cx, |page, cx| page.search_within_source(cx)))),
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

            if !page.facets.is_empty() {
                let mut controls = div().w_full().flex().flex_row().flex_wrap().items_end().gap(px(8.0));
                for (facet_index, facet) in page.facets.iter().enumerate() {
                    let selected = facet.links.iter().find(|link| link.active).or_else(|| facet.links.first());
                    let selected_label = selected.map(|link| link.title.clone()).unwrap_or_else(|| "Choose".to_owned());
                    let links = facet.links.clone();
                    let entity = cx.entity();
                    let dropdown = Button::new(format!("store-facet-{facet_index}")).label(selected_label).dropdown_caret(true).outline().h(px(32.0)).dropdown_menu(move |menu, _, _| {
                        links.iter().fold(menu, |menu, link| {
                            let checked = link.active;
                            let target = link.target.clone();
                            let entity = entity.clone();
                            menu.item(PopupMenuItem::new(link.title.clone()).checked(checked).on_click(move |_, _, cx| match &target {
                                CatalogLinkTarget::Page { locator } => entity.update(cx, |page, cx| page.load_page(Some(locator.clone()), cx)),
                                CatalogLinkTarget::External { url } => cx.open_url(url),
                            }))
                        })
                    });
                    controls = controls.child(div().flex().flex_col().gap(px(3.0)).child(div().text_size(gpui::rems(10.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme.text_muted).child(facet.title.clone())).child(dropdown));
                }
                section = section.child(GroupBox::new().fill().child(controls));
            }

            let mut rows = Vec::new();
            if !page.publications.is_empty() {
                if let Some(title) = &page.primary_section_title {
                    rows.push(StoreVirtualRow::SectionHeader { title: title.clone(), detail: format!("{} books", page.publications.len()), locator: None });
                }
                Self::append_publication_rows(&mut rows, &page.publications, publication_columns);
            }
            for (group_index, group) in page.groups.iter().enumerate() {
                rows.push(StoreVirtualRow::SectionHeader { title: group.title.clone(), detail: format!("{} books", group.number_of_items), locator: group.page_locator.clone() });
                if !group.navigation.is_empty() {
                    rows.push(StoreVirtualRow::Links { links: group.navigation.clone(), id_prefix: format!("store-group-link-{group_index}") });
                }
                Self::append_publication_rows(&mut rows, &group.cards, publication_columns);
            }
            if rows.is_empty() {
                rows.push(StoreVirtualRow::Status { source: None, message: "No publications are available on this page.".to_owned(), error: false });
            }
            section = section.child(self.virtual_rows("store-catalog-results", rows, width, publication_columns, theme, cx));

            let mut pagination = components::action_row();
            if let Some(previous) = page.pagination.previous.clone() {
                let entity = cx.entity();
                pagination =
                    pagination.child(components::secondary_action_button("store-previous", "Previous", IconName::ArrowLeft, self.loading).on_click(move |_, _, cx| entity.update(cx, |page, cx| page.load_page(Some(previous.clone()), cx))));
            }
            if let Some(next) = page.pagination.next.clone() {
                let entity = cx.entity();
                pagination = pagination.child(components::secondary_action_button("store-next", "Next", IconName::ArrowRight, self.loading).on_click(move |_, _, cx| entity.update(cx, |page, cx| page.load_page(Some(next.clone()), cx))));
            }
            content = content.child(section.child(div().flex_none().flex().justify_center().child(pagination)));
        }

        div().size_full().min_h_0().min_w_0().p(px(18.0)).flex().flex_col().child(content).into_any_element()
    }
}
