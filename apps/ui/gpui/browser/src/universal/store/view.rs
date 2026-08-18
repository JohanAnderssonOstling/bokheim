//! Store presentation. Behaviour and retained state stay in the parent page.

use super::*;
use gpui::{Entity, size};
use gpui_component::alert::Alert;
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::v_virtual_list;
use gpui_component::{Sizable, Size};

use crate::catalog::CatalogPublicationCardView;
use crate::virtual_grid;

fn catalog_link_label(link: &CatalogLink, catalogue: &str) -> String {
    let title = catalog_page_title(catalogue, &link.title);
    link.number_of_items.map_or_else(|| title.to_owned(), |count| format!("{title} ({count})"))
}

/// Catalogue identity is already shown beside the back button, so repeating it
/// at the start of every page crumb adds noise (for example, "Unglue.it
/// Literature"). Keep the provider's title intact when it is not a prefix or
/// when removing it would leave an empty page name.
fn catalog_page_title<'a>(catalogue: &str, title: &'a str) -> &'a str {
    let Some(remainder) = title.strip_prefix(catalogue) else { return title };
    let remainder = remainder.trim_start_matches(|character: char| character.is_whitespace() || matches!(character, ':' | '-' | '–' | '—'));
    if remainder.is_empty() { title } else { remainder }
}

const STORE_SECTION_ROW_HEIGHT: f32 = 58.0;
const STORE_SOURCE_ROW_HEIGHT: f32 = 66.0;
const STORE_LINK_ROW_HEIGHT: f32 = components::HIERARCHY_CELL_HEIGHT + components::HIERARCHY_CELL_MARGIN * 2.0;
const STORE_STATUS_ROW_HEIGHT: f32 = 66.0;

#[derive(Clone)]
pub(super) enum StoreVirtualRow {
    SectionHeader { title: String, detail: String, locator: Option<String> },
    SourceHeader(CatalogSource),
    Publications(Vec<(Entity<CatalogPublicationCardView>, usize)>),
    Links { rows: Vec<Vec<CatalogLink>>, id_prefix: String },
    Status { source: Option<CatalogSource>, message: String, error: bool },
}

#[derive(Clone)]
pub(super) struct StoreVirtualRows {
    rows: Rc<Vec<StoreVirtualRow>>,
    sizes: Rc<Vec<gpui::Size<gpui::Pixels>>>,
    measurement_index: usize,
}

impl StoreVirtualRow {
    fn height(&self, publication_height: f32) -> f32 {
        match self {
            Self::SectionHeader { .. } => STORE_SECTION_ROW_HEIGHT,
            Self::SourceHeader(_) => STORE_SOURCE_ROW_HEIGHT,
            Self::Publications(_) => publication_height,
            Self::Links { rows, .. } => STORE_LINK_ROW_HEIGHT * rows.len() as f32,
            Self::Status { .. } => STORE_STATUS_ROW_HEIGHT,
        }
    }
}

impl StorePage {
    fn follow_internal_link(&mut self, locator: String, title: String, cx: &mut Context<Self>) {
        if matches!(self.mode, StoreMode::Subjects(_)) {
            self.load_subjects(Some(locator), Some(title), cx);
        } else {
            self.load_page(Some(locator), cx);
        }
    }

    fn catalog_link(&self, link: &CatalogLink, catalogue: Option<&str>, id: String, width: f32, theme: components::BrowserTheme, cx: &mut Context<Self>) -> AnyElement {
        let count = link.number_of_items.map(|count| count as usize);
        let title = catalogue.map_or_else(|| link.title.clone(), |catalogue| catalog_page_title(catalogue, &link.title).to_owned());
        match &link.target {
            CatalogLinkTarget::Page { locator } => {
                let locator = locator.clone();
                let target_title = link.title.clone();
                let entity = cx.entity();
                components::catalog_hierarchy_cell(id, title, count, IconName::Folder, width, theme)
                    .disabled(link.active || self.loading)
                    .on_click(move |_, _, cx| entity.update(cx, |page, cx| page.follow_internal_link(locator.clone(), target_title.clone(), cx)))
                    .into_any_element()
            }
            CatalogLinkTarget::External { url } => {
                let url = url.clone();
                components::catalog_hierarchy_cell(id, title, count, IconName::ExternalLink, width, theme).disabled(link.active).on_click(move |_, _, cx| cx.open_url(&url)).into_any_element()
            }
        }
    }

    /// One facet axis as a dropdown labelled by its applied option, so a
    /// filtered page reads as filtered with every menu closed.
    fn facet_dropdown(&self, facet: &CatalogFacet, catalogue: &str, index: usize, theme: components::BrowserTheme, cx: &mut Context<Self>) -> AnyElement {
        let applied = facet.links.iter().enumerate().find(|(_, link)| link.active);
        let is_filtered = applied.is_some_and(|(active_index, _)| active_index != 0);
        let label = applied.map(|(_, link)| link).or_else(|| facet.links.first()).map_or_else(|| facet.title.clone(), |link| catalog_page_title(catalogue, &link.title).to_owned());
        let links = facet.links.clone();
        let catalogue = catalogue.to_owned();
        let entity = cx.entity();
        components::catalog_dropdown_button(format!("store-facet-{index}"), label, is_filtered, self.loading, theme)
            .dropdown_caret(true)
            .dropdown_menu(move |menu, _, _| {
                links.iter().fold(menu, |menu, link| {
                    let target = link.target.clone();
                    let entity = entity.clone();
                    menu.item(PopupMenuItem::new(catalog_link_label(link, &catalogue)).checked(link.active).on_click(move |_, _, cx| match &target {
                        CatalogLinkTarget::Page { locator } => entity.update(cx, |page, cx| page.load_page(Some(locator.clone()), cx)),
                        CatalogLinkTarget::External { url } => cx.open_url(url),
                    }))
                })
            })
            .into_any_element()
    }

    /// The catalogue's own subsections, behind one menu. They are somewhere to
    /// go once rather than a control to keep on screen, and as fixed-width
    /// cards they were the most expensive band on the page.
    fn browse_menu(&self, navigation: &[CatalogLink], catalogue: &str, theme: components::BrowserTheme, cx: &mut Context<Self>) -> AnyElement {
        let links = navigation.to_vec();
        let catalogue = catalogue.to_owned();
        let entity = cx.entity();
        components::catalog_dropdown_button("store-browse", format!("Browse ({})", links.len()), false, self.loading, theme)
            .dropdown_caret(true)
            .dropdown_menu(move |menu, _, _| {
                links.iter().fold(menu, |menu, link| {
                    let target = link.target.clone();
                    let entity = entity.clone();
                    menu.item(PopupMenuItem::new(catalog_link_label(link, &catalogue)).checked(link.active).on_click(move |_, _, cx| match &target {
                        CatalogLinkTarget::Page { locator } => entity.update(cx, |page, cx| page.load_page(Some(locator.clone()), cx)),
                        CatalogLinkTarget::External { url } => cx.open_url(url),
                    }))
                })
            })
            .into_any_element()
    }

    fn search_controls(&self) -> Div {
        components::catalog_toolbar_search().child(Input::new(&self.search_input))
    }

    /// Catalogue-wide search uses one persisted language at a time. The
    /// preference model still accepts multiple values for account/profile
    /// compatibility, but this compact control avoids the previous ambiguous
    /// "all selected" chip state where clicking English excluded English.
    fn language_dropdown(
        &self, preferences: &epub_svelte_lib::app::preferences::BrowsingPreferences, browsing_contract: &epub_svelte_lib::app::preferences::BrowsingPreferencesContract, all_languages_label: &str, theme: components::BrowserTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let included = preferences.included_catalog_languages();
        let options = browsing_contract.options.catalog_languages.iter().filter(|option| self.sources.iter().any(|source| catalog_language_available(&source.languages, &option.value, &option.label))).cloned().collect::<Vec<_>>();
        let label = if included.is_empty() {
            all_languages_label.to_owned()
        } else if included.len() == 1 {
            options.iter().find(|option| option.value == included[0]).map_or_else(|| included[0].clone(), |option| option.label.clone())
        } else {
            format!("{} languages", included.len())
        };
        let selected = included.to_vec();
        let all_label = all_languages_label.to_owned();
        let entity = cx.entity();
        components::catalog_dropdown_button("store-language", label, !included.is_empty(), self.loading, theme)
            .dropdown_caret(true)
            .dropdown_menu(move |menu, _, _| {
                let all_entity = entity.clone();
                options.iter().fold(
                    menu.item(PopupMenuItem::new(all_label.clone()).checked(selected.is_empty()).on_click(move |_, _, cx| {
                        all_entity.update(cx, |page, cx| page.update_catalog_languages(Vec::new(), cx));
                    })),
                    |menu, option| {
                        let value = option.value.clone();
                        let chosen = selected.contains(&value);
                        let option_entity = entity.clone();
                        menu.item(PopupMenuItem::new(option.label.clone()).checked(chosen).on_click(move |_, _, cx| {
                            option_entity.update(cx, |page, cx| page.toggle_catalog_language(&value, cx));
                        }))
                    },
                )
            })
            .into_any_element()
    }

    fn subject_source_dropdown(&self, theme: components::BrowserTheme, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.subject_source_ids.clone();
        let label = if selected.is_empty() {
            "All catalogues".to_owned()
        } else if selected.len() == 1 {
            self.sources.iter().find(|source| source.id == selected[0]).map_or_else(|| selected[0].as_str().to_owned(), |source| source.title.clone())
        } else {
            format!("{} catalogues", selected.len())
        };
        let sources = self.sources.clone();
        let entity = cx.entity();
        components::catalog_dropdown_button("store-subject-sources", label, !selected.is_empty(), self.loading, theme)
            .dropdown_caret(true)
            .dropdown_menu(move |menu, _, _| {
                let all_entity = entity.clone();
                sources.iter().fold(
                    menu.item(PopupMenuItem::new("All catalogues").checked(selected.is_empty()).on_click(move |_, _, cx| {
                        all_entity.update(cx, |page, cx| page.set_subject_sources(Vec::new(), cx));
                    })),
                    |menu, source| {
                        let source_id = source.id.clone();
                        let chosen = selected.contains(&source_id);
                        let source_entity = entity.clone();
                        menu.item(PopupMenuItem::new(source.title.clone()).checked(chosen).on_click(move |_, _, cx| {
                            source_entity.update(cx, |page, cx| page.toggle_subject_source(&source_id, cx));
                        }))
                    },
                )
            })
            .into_any_element()
    }

    /// Names every filter narrowing the list, in one place, only while one is.
    ///
    /// Each non-default facet links back to its first (unfiltered) option, and
    /// the query chip returns to the catalogue root.
    fn applied_filters(&self, page: &CatalogueView, search_label: &str, theme: components::BrowserTheme, cx: &mut Context<Self>) -> Option<Div> {
        let applied = page
            .facets
            .iter()
            .filter_map(|facet| {
                let (active_index, active) = facet.links.iter().enumerate().find(|(_, link)| link.active)?;
                (active_index != 0).then(|| (facet.title.clone(), catalog_page_title(&page.source.title, &active.title).to_owned(), facet.links.first().map(|link| link.target.clone())))
            })
            .collect::<Vec<_>>();
        if applied.is_empty() && page.query.is_none() {
            return None;
        }
        let mut row = components::catalog_chip_row();
        for (index, (axis, value, reset)) in applied.iter().enumerate() {
            let chip = components::catalog_removable_chip(format!("store-chip-{index}"), axis.clone(), value.clone(), theme);
            row = match reset.clone() {
                Some(CatalogLinkTarget::Page { locator }) => {
                    let entity = cx.entity();
                    row.child(chip.on_click(move |_, _, cx| entity.update(cx, |page, cx| page.load_page(Some(locator.clone()), cx))))
                }
                Some(CatalogLinkTarget::External { url }) => row.child(chip.on_click(move |_, _, cx| cx.open_url(&url))),
                None => row.child(chip.disabled(true)),
            };
        }
        if let Some(query) = page.query.clone() {
            let entity = cx.entity();
            row = row.child(components::catalog_removable_chip("store-chip-query", search_label.to_owned(), query, theme).on_click(move |_, window, cx| entity.update(cx, |page, cx| page.clear_search_within_source(window, cx))));
        }
        let entity = cx.entity();
        Some(row.child(components::catalog_clear_filters_button("store-clear-filters", "Clear all", self.loading).on_click(move |_, window, cx| entity.update(cx, |page, cx| page.clear_search_within_source(window, cx)))))
    }

    fn append_publication_rows(rows: &mut Vec<StoreVirtualRow>, cards: &CatalogCardGroup, columns: usize) {
        for row in cards.packed_rows(columns) {
            rows.push(StoreVirtualRow::Publications(row.into_iter().map(|item| (item.card, item.span)).collect()));
        }
    }

    fn append_link_rows(rows: &mut Vec<StoreVirtualRow>, links: &[CatalogLink], id_prefix: String, columns: usize) {
        if !links.is_empty() {
            rows.push(StoreVirtualRow::Links { rows: links.chunks(columns.max(1)).map(|row| row.to_vec()).collect(), id_prefix });
        }
    }

    fn maximum_link_count(&self) -> usize {
        match &self.mode {
            StoreMode::Catalogues => 0,
            StoreMode::Subjects(subjects) => subjects.response.navigation.len(),
            StoreMode::WideSearch(search) => {
                search.sections.iter().flat_map(|section| section.sources.iter().map(|source| source.external_links.len())).chain(search.statuses.iter().map(|status| status.external_links.len())).max().unwrap_or(0)
            }
            StoreMode::Catalogue(page) => std::iter::once(page.navigation.len()).chain(page.groups.iter().map(|group| group.navigation.len())).max().unwrap_or(0),
        }
    }

    fn build_virtual_rows(&self, columns: usize, link_columns: usize) -> Option<StoreVirtualRows> {
        let mut rows = Vec::new();
        match &self.mode {
            StoreMode::Catalogues => return None,
            StoreMode::WideSearch(search) => {
                for section in &search.sections {
                    rows.push(StoreVirtualRow::SectionHeader { title: section.title.clone(), detail: String::new(), locator: None });
                    for source in &section.sources {
                        rows.push(StoreVirtualRow::SourceHeader(source.source.clone()));
                        Self::append_publication_rows(&mut rows, &source.cards, columns);
                        if !source.external_links.is_empty() {
                            Self::append_link_rows(&mut rows, &source.external_links, format!("store-wide-link-{}", source.source.id.as_str()), link_columns);
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
                            Self::append_link_rows(&mut rows, &status.external_links, format!("store-wide-status-link-{}", status.source.id.as_str()), link_columns);
                        }
                    }
                }
                if rows.is_empty() {
                    rows.push(StoreVirtualRow::Status { source: None, message: "No matching books were returned.".to_owned(), error: false });
                }
            }
            StoreMode::Subjects(subjects) => {
                Self::append_link_rows(&mut rows, &subjects.response.navigation, "store-subject-link".to_owned(), link_columns);
                for source in &subjects.sources {
                    rows.push(StoreVirtualRow::SourceHeader(source.source.clone()));
                    Self::append_publication_rows(&mut rows, &source.cards, columns);
                }
                if rows.is_empty() {
                    rows.push(StoreVirtualRow::Status { source: None, message: "No catalogue books are assigned to this subject yet.".to_owned(), error: false });
                }
            }
            StoreMode::Catalogue(page) => {
                let navigation_only = page.publications.is_empty() && page.groups.is_empty();
                if navigation_only && !page.navigation.is_empty() {
                    Self::append_link_rows(&mut rows, &page.navigation, "store-page-link".to_owned(), link_columns);
                }
                if !page.publications.is_empty() {
                    if let Some(title) = &page.primary_section_title {
                        rows.push(StoreVirtualRow::SectionHeader { title: title.clone(), detail: format!("{} books", page.publications.len()), locator: None });
                    }
                    Self::append_publication_rows(&mut rows, &page.publications, columns);
                }
                for (group_index, group) in page.groups.iter().enumerate() {
                    rows.push(StoreVirtualRow::SectionHeader { title: group.title.clone(), detail: format!("{} books", group.number_of_items), locator: group.page_locator.clone() });
                    if !group.navigation.is_empty() {
                        Self::append_link_rows(&mut rows, &group.navigation, format!("store-group-link-{group_index}"), link_columns);
                    }
                    Self::append_publication_rows(&mut rows, &group.cards, columns);
                }
                if rows.is_empty() {
                    rows.push(StoreVirtualRow::Status { source: None, message: "No publications are available on this page.".to_owned(), error: false });
                }
            }
        }
        let publication_height = self.services.contracts.store.publication_height_px as f32;
        let measurement_index = rows.iter().position(|row| !matches!(row, StoreVirtualRow::Publications(_))).unwrap_or(0);
        let sizes = Rc::new(rows.iter().map(|row| size(px(0.0), px(row.height(publication_height)))).collect());
        Some(StoreVirtualRows { rows: Rc::new(rows), sizes, measurement_index })
    }

    fn retained_virtual_rows(&mut self, columns: usize, link_columns: usize) -> Option<StoreVirtualRows> {
        let columns = columns.max(1);
        let link_columns = link_columns.max(1);
        let key = (columns, link_columns);
        if !self.virtual_rows.contains_key(&key) {
            if let Some(rows) = self.build_virtual_rows(columns, link_columns) {
                self.virtual_rows.insert(key, rows);
            }
        }
        self.virtual_rows.get(&key).cloned()
    }

    fn virtual_rows(&self, id: &'static str, retained: StoreVirtualRows, columns: usize, link_width: f32, catalogue: Option<String>, theme: components::BrowserTheme, cx: &mut Context<Self>) -> AnyElement {
        let publication_height = self.services.contracts.store.publication_height_px as f32;
        let publication_unit_width = crate::catalog::catalog_grid_unit_width(self.services.contracts.store.publication_min_width_px);
        let rows = retained.rows;
        let list = v_virtual_list(cx.entity(), id, retained.sizes, move |page, range, _, cx| {
            if range.end.saturating_add(2) >= rows.len() && !page.loading {
                if id == "store-catalog-results" {
                    page.load_next_page(cx);
                } else if id == "store-subject-results" {
                    page.load_next_subject_page(cx);
                }
            }
            range
                .map(|index| match &rows[index] {
                    StoreVirtualRow::SectionHeader { title, detail, locator } => {
                        let title = catalogue.as_deref().map_or_else(|| title.clone(), |catalogue| catalog_page_title(catalogue, title).to_owned());
                        let mut row = div().w_full().h(px(STORE_SECTION_ROW_HEIGHT)).flex_none().flex().items_center().justify_between().gap(px(10.0)).child(components::section_header(title, detail.clone(), theme));
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
                            .child(components::section_header(source.title.clone(), "", theme))
                            .child(
                                components::secondary_action_button(format!("{id}-source-{index}"), "Open catalog", IconName::ChevronRight, page.loading)
                                    .on_click(move |_, _, cx| entity.update(cx, |page, cx| page.select_source(source_to_open.clone(), cx))),
                            )
                            .into_any_element()
                    }
                    StoreVirtualRow::Publications(cards) => virtual_grid::fixed_spanned_entity_row(cards, columns, publication_unit_width, publication_height, STORE_GRID_GAP).into_any_element(),
                    StoreVirtualRow::Links { rows, id_prefix } => {
                        let mut group = components::hierarchy_grid(theme).flex_none();
                        for (row_index, links) in rows.iter().enumerate() {
                            let mut row = components::hierarchy_grid_row();
                            for (link_index, link) in links.iter().enumerate() {
                                row = row.child(page.catalog_link(link, catalogue.as_deref(), format!("{id_prefix}-{row_index}-{link_index}"), link_width, theme, cx));
                            }
                            group = group.child(row);
                        }
                        group.into_any_element()
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
        .with_item_to_measure_index(retained.measurement_index)
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
        let contracts = self.services.contracts.clone();
        let contract = &contracts.store;
        let settings = &contracts.settings;
        let browsing_contract = &contracts.browsing_preferences;
        let preferences = self.preferences.read(cx).browsing();
        let publication_grid = virtual_grid::ResponsiveColumns::for_element(
            "store-publication-grid",
            virtual_grid::ColumnPolicy::MinimumItemWidth { width: crate::catalog::catalog_grid_unit_width(contract.publication_min_width_px), gap: STORE_GRID_GAP },
            window,
            cx,
        );
        let source_min_width = (contract.catalog_two_column_breakpoint_px as f32 - STORE_GRID_GAP) / 2.0;
        let source_grid_columns = virtual_grid::ResponsiveColumns::for_element("store-source-grid", virtual_grid::ColumnPolicy::MinimumItemWidth { width: source_min_width, gap: STORE_GRID_GAP }, window, cx);
        let publication_columns = publication_grid.columns(cx);
        let content_width = f32::from(window.viewport_size().width) - components::nav_rail_width(f32::from(window.rem_size())) - components::CONTENT_PADDING * 2.0;
        let hierarchy_width = (content_width - components::HIERARCHY_LAYOUT_SAFETY).max(components::HIERARCHY_CELL_MIN_WIDTH);
        let (link_columns, link_width) = components::hierarchy_grid_layout(hierarchy_width, self.maximum_link_count(), self.catalog_link_columns.get());
        self.catalog_link_columns.set(link_columns);
        let retained_rows = self.retained_virtual_rows(publication_columns, link_columns);

        let mut content = div().w_full().mx_auto().flex_1().min_h_0().flex().flex_col().gap(px(components::SPACE_LG));

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

        if let StoreMode::Subjects(subjects) = &self.mode {
            let levels = subjects.levels.clone();
            let entity = cx.entity();
            let mut identity = components::catalog_bar()
                .child(components::catalog_back_button("store-subject-back", contract.catalogs_label.clone()).on_click(move |_, _, cx| entity.update(cx, |page, cx| page.show_catalogues(cx))))
                .child(components::catalog_source_name("Browse by subject"));
            if levels.len() > 1 {
                let mut breadcrumbs = components::folder_breadcrumb();
                for (index, (title, locator)) in levels.iter().enumerate().skip(1) {
                    let current = index + 1 == levels.len();
                    let item = components::folder_breadcrumb_item(title.clone(), current);
                    breadcrumbs = if current || self.loading {
                        breadcrumbs.child(item)
                    } else {
                        let title = title.clone();
                        let locator = locator.clone();
                        let entity = cx.entity();
                        breadcrumbs.child(item.on_click(move |_, _, cx| entity.update(cx, |page, cx| page.load_subjects(locator.clone(), Some(title.clone()), cx))))
                    };
                }
                identity = identity.child(breadcrumbs);
            }
            content = content.child(identity);
            let entity = cx.entity();
            let mut controls = div().w_full().flex().items_center().gap(px(8.0));
            if levels.len() > 1 {
                controls = controls
                    .child(div().flex_1().min_w_0().max_w(px(480.0)).child(Input::new(&self.subject_search_input)))
                    .child(components::secondary_action_button("store-subject-search", "Search", IconName::Search, self.loading).on_click(move |_, _, cx| {
                        entity.update(cx, |page, cx| page.search_subjects(cx));
                    }));
            } else {
                controls = controls.child(div().flex_1());
            }
            content = content
                .child(controls.child(self.subject_source_dropdown(theme, cx)))
                .child(self.virtual_rows("store-subject-results", retained_rows.expect("subject browse rows"), publication_columns, link_width, None, theme, cx));
            return components::folder_dashboard().child(publication_grid.track(content)).into_any_element();
        }

        if !matches!(&self.mode, StoreMode::Catalogue(_)) {
            let entity = cx.entity();
            content = content.child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(div().flex_1().min_w_0().max_w(px(480.0)).child(Input::new(&self.wide_search_input)))
                    .child(components::secondary_action_button("store-subjects", "Browse by subject", IconName::Folder, self.loading).on_click(move |_, _, cx| {
                        entity.update(cx, |page, cx| page.load_subjects(None, None, cx));
                    }))
                    .child(self.language_dropdown(&preferences, browsing_contract, &settings.all_catalog_languages_label, theme, cx)),
            );

            if let StoreMode::WideSearch(search) = &self.mode {
                let entity = cx.entity();
                content = content.child(div().flex().items_center().justify_between().child(components::section_title(format!("Results for “{}”", search.query))).child(
                    components::secondary_action_button("store-clear-wide", "Clear search", IconName::Close, false).on_click(move |_, _, cx| {
                        entity.update(cx, |page, cx| {
                            page.show_catalogues(cx);
                        });
                    }),
                ));
                content = content.child(self.virtual_rows("store-wide-results", retained_rows.expect("wide search rows"), publication_columns, link_width, None, theme, cx));
                return components::folder_dashboard().child(publication_grid.track(content)).into_any_element();
            } else {
                let source_columns = source_grid_columns.columns(cx);
                let mut source_grid = div().w_full().flex().flex_col().gap(px(STORE_GRID_GAP));
                for source_row in self.sources.chunks(source_columns) {
                    let mut row = div().w_full().flex().flex_row().gap(px(STORE_GRID_GAP));
                    for source in source_row.iter().cloned() {
                        let entity = cx.entity();
                        let clicked = source.clone();
                        row = row.child(
                            div()
                                .id(format!("store-source-{}", source.id.as_str()))
                                .min_w_0()
                                .flex_1()
                                .min_h(px(76.0))
                                .p(px(14.0))
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
                                .child(components::item_identity(source.title.clone(), source.description.clone(), theme))
                                .child(gpui_component::Icon::new(IconName::ChevronRight).text_color(theme.text_muted))
                                .on_click(move |_, _, cx| entity.update(cx, |page, cx| page.select_source(clicked.clone(), cx))),
                        );
                    }
                    row = row.children((source_row.len()..source_columns).map(|_| div().min_w_0().flex_1()));
                    source_grid = source_grid.child(row);
                }
                content = content.child(source_grid);
            }
            return components::scrollable_page().child(source_grid_columns.track(content)).into_any_element();
        }

        let StoreMode::Catalogue(page) = &self.mode else { unreachable!() };

        let entity = cx.entity();
        let mut identity = components::catalog_bar()
            .child(components::catalog_back_button("store-back", contract.catalogs_label.clone()).on_click(move |_, _, cx| {
                entity.update(cx, |page, cx| {
                    page.show_catalogues(cx);
                });
            }))
            .child(components::catalog_source_name(page.source.title.clone()));
        if !page.levels.is_empty() {
            let mut breadcrumbs = components::folder_breadcrumb();
            for (index, (title, locator)) in page.levels.iter().enumerate().skip(1) {
                let current = index + 1 == page.levels.len();
                let item = components::folder_breadcrumb_item(catalog_page_title(&page.source.title, title).to_owned(), current);
                breadcrumbs = if current || self.loading {
                    breadcrumbs.child(item)
                } else {
                    let locator = locator.clone();
                    let entity = cx.entity();
                    breadcrumbs.child(item.on_click(move |_, _, cx| entity.update(cx, |page, cx| page.load_page(Some(locator.clone()), cx))))
                };
            }
            identity = identity.child(breadcrumbs);
        }
        content = content.child(identity);

        {
            let mut section = div().w_full().flex_1().min_h_0().flex().flex_col().gap(px(components::SPACE_SM));

            let mut toolbar = components::catalog_toolbar();
            if page.search.is_some() {
                toolbar = toolbar.child(self.search_controls());
            }
            if !page.navigation.is_empty() {
                toolbar = toolbar.child(self.browse_menu(&page.navigation, &page.source.title, theme, cx));
            }
            for (facet_index, facet) in page.facets.iter().enumerate() {
                toolbar = toolbar.child(self.facet_dropdown(facet, &page.source.title, facet_index, theme, cx));
            }
            section = section.child(toolbar);
            if let Some(chips) = self.applied_filters(&page, &contract.search_label, theme, cx) {
                section = section.child(chips);
            }

            section = section.child(self.virtual_rows("store-catalog-results", retained_rows.expect("catalogue rows"), publication_columns, link_width, Some(page.source.title.clone()), theme, cx));

            content = content.child(section);
        }

        components::folder_dashboard().child(publication_grid.track(content)).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::catalog_page_title;

    #[test]
    fn catalogue_name_is_removed_from_page_crumbs() {
        assert_eq!(catalog_page_title("Unglue.it", "Unglue.it Literature"), "Literature");
        assert_eq!(catalog_page_title("Unglue.it", "Unglue.it: Literature"), "Literature");
        assert_eq!(catalog_page_title("Unglue.it", "New Releases"), "New Releases");
        assert_eq!(catalog_page_title("Unglue.it", "Unglue.it"), "Unglue.it");
    }
}
