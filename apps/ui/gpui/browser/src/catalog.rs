//! Catalogue presentation shared by the Store and Authors pages.
//!
//! Each retained publication card owns its cover and import tasks. Pages only
//! own ordered groups of those cards.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use epub_svelte_lib::FrontendContract;
use epub_svelte_lib::app::executor::BackendTask;
use epub_svelte_lib::catalog::{CatalogDescription, CatalogImportRequest, CatalogPublication, CatalogPublicationAction, CatalogPublicationKey, CatalogSource, CatalogSourceId};
use gpui::prelude::*;
use gpui::{AnyElement, AppContext, Context, Entity, FontWeight, IntoElement, Render, SharedString, Task, TextRun, Window, div, font, px};
use gpui_component::ElementExt as _;
use gpui_component::IconName;
use html_view_core::RendererInitialConfig;
use html_view_gpui::HtmlView;
use ui_components as components;

use crate::description::description_provider;
use crate::library::{LoadedCover, loaded_cover_from_jpeg};
use crate::services::AppServices;

const STORE_GRID_GAP: f32 = 8.0;
const STORE_CARD_CONTENT_GAP: f32 = 16.0;
const STORE_CARD_DETAIL_RIGHT_INSET: f32 = 16.0;
const MAX_COVER_RETRIES: u8 = 5;
const DESCRIPTION_MIN_WIDTH_REM: f32 = 20.0;
const DESCRIPTION_DEFAULT_CHARACTER_COLUMNS: f32 = 36.0;
const DESCRIPTION_MAX_CHARACTER_COLUMNS: f32 = 80.0;
const DESCRIPTION_CARD_BOTTOM_INSET: f32 = 4.0;
const DESCRIPTION_TEXT_RIGHT_INSET: f32 = 8.0;

pub(crate) fn catalog_description_width_bounds(window: &Window, font_family: SharedString) -> (f32, f32) {
    let character_width = catalog_character_width(window, font_family);
    let minimum = f32::from(window.rem_size()) * DESCRIPTION_MIN_WIDTH_REM;
    (minimum, (character_width * DESCRIPTION_MAX_CHARACTER_COLUMNS).max(minimum))
}

/// Stable width used by embedded catalogue rows that do not own a responsive
/// Store viewport, such as author discovery.
pub(crate) fn catalog_description_width(window: &Window, font_family: SharedString) -> f32 {
    let character_width = catalog_character_width(window, font_family.clone());
    let (minimum, maximum) = catalog_description_width_bounds(window, font_family);
    (character_width * DESCRIPTION_DEFAULT_CHARACTER_COLUMNS).clamp(minimum, maximum)
}

fn catalog_character_width(window: &Window, font_family: SharedString) -> f32 {
    let sample = "0";
    let run = TextRun { len: sample.len(), font: font(font_family), ..Default::default() };
    f32::from(window.text_system().layout_line(sample, window.rem_size(), &[run], None).width)
}

pub(crate) fn catalog_grid_unit_width_for_description(publication_min_width_px: u16, description_width: f32) -> f32 {
    let expanded_width = components::BOOK_COVER_WIDTH + STORE_CARD_CONTENT_GAP + description_width + STORE_CARD_DETAIL_RIGHT_INSET + DESCRIPTION_TEXT_RIGHT_INSET;
    catalog_grid_unit_width(publication_min_width_px).max((expanded_width - STORE_GRID_GAP) / 2.0)
}

/// Lets catalogue cards consume the width their responsive column owns until
/// the description reaches its readable maximum. Unlike the regular book
/// grid, the column gap stays fixed: descriptions absorb the flexible space.
pub(crate) fn catalog_grid_unit_width_for_available(available_width: f32, columns: usize, minimum_unit_width: f32, maximum_unit_width: f32, gap: f32) -> f32 {
    let columns = columns.max(1);
    let available_unit_width = (available_width - gap * columns.saturating_sub(1) as f32) / columns as f32;
    available_unit_width.clamp(minimum_unit_width, maximum_unit_width.max(minimum_unit_width))
}

pub(crate) fn catalog_publication_height(publication_height_px: u16, rem_size: f32) -> f32 {
    catalog_description_card_height(publication_height_px, rem_size)
}

fn catalog_description_card_height(_publication_height_px: u16, rem_size: f32) -> f32 {
    catalog_author_bottom(rem_size) + DESCRIPTION_CARD_BOTTOM_INSET
}

fn catalog_author_bottom(rem_size: f32) -> f32 {
    components::BOOK_COVER_HEIGHT + components::BOOK_CARD_COVER_CONTENT_GAP + components::BOOK_CARD_TITLE_HEIGHT_REM * rem_size + 2.0 + components::BOOK_CARD_AUTHOR_HEIGHT_REM * rem_size
}

fn catalog_description_action_bottom_padding(publication_height_px: u16, rem_size: f32) -> f32 {
    (catalog_description_card_height(publication_height_px, rem_size) - catalog_author_bottom(rem_size)).max(0.0)
}

pub(crate) fn catalog_grid_unit_width(publication_min_width_px: u16) -> f32 {
    ((publication_min_width_px as f32 - STORE_GRID_GAP) / 2.0).max(components::BOOK_COVER_WIDTH + 2.0)
}

/// Identifies one edition within one source, for ownership and import errors.
pub(crate) fn ownership_key(source_id: &CatalogSourceId, edition_id: &str) -> String {
    format!("{}:{edition_id}", source_id.as_str())
}

pub(crate) type CatalogImport = Rc<dyn Fn(CatalogImportRequest) -> Result<BackendTask<Result<(), String>>, String>>;

#[derive(Clone)]
pub(crate) struct CatalogCardServices {
    import: CatalogImport,
    app: AppServices,
}

impl CatalogCardServices {
    pub(crate) fn new(services: &AppServices, import: CatalogImport) -> Self {
        Self { import, app: services.clone() }
    }

    pub(crate) fn group<P: 'static>(&self, source: &CatalogSource, publications: &[CatalogPublication], owned: &HashSet<CatalogPublicationKey>, show_source: bool, cx: &mut Context<P>) -> CatalogCardGroup {
        self.group_unique(source, publications, owned, show_source, &mut HashSet::new(), cx)
    }

    /// Builds a group while sharing publication identity deduplication across
    /// sections. Call sections in display-priority order so the first match
    /// owns a publication that occurs in more than one section.
    pub(crate) fn group_unique<P: 'static>(
        &self, source: &CatalogSource, publications: &[CatalogPublication], owned: &HashSet<CatalogPublicationKey>, show_source: bool, seen: &mut HashSet<CatalogPublicationKey>, cx: &mut Context<P>,
    ) -> CatalogCardGroup {
        let mut cards = Vec::new();
        for publication in publications {
            let key = CatalogPublicationKey { source_id: source.id.clone(), publication: publication.identity.clone() };
            if owned.contains(&key) || !seen.insert(key) {
                continue;
            }
            let services = self.clone();
            let source = source.clone();
            // A readable description needs the expanded measure regardless of
            // whether the provider supplied cover art.
            let expanded = has_description_excerpt(publication.description.as_ref());
            let publication = publication.clone();
            let card = cx.new(|cx| CatalogPublicationCardView::new(source, publication, show_source, services, cx));
            cards.push(CatalogCardItem { card, span: if expanded { 2 } else { 1 } });
        }
        CatalogCardGroup { cards, contracts: self.app.contracts.clone() }
    }

    fn load_cover(&self, source_id: CatalogSourceId, image_url: String) -> BackendTask<Result<Vec<u8>, String>> {
        self.app.backend.spawn(move |core| async move { core.catalog_cover(&source_id, &image_url).await.map_err(|error| error.to_string()) })
    }

    pub(crate) fn empty_group(&self) -> CatalogCardGroup {
        CatalogCardGroup { cards: Vec::new(), contracts: self.app.contracts.clone() }
    }
}

#[derive(Clone)]
pub(crate) struct CatalogCardGroup {
    cards: Vec<CatalogCardItem>,
    contracts: Arc<FrontendContract>,
}

#[derive(Clone)]
pub(crate) struct CatalogCardItem {
    pub(crate) card: Entity<CatalogPublicationCardView>,
    pub(crate) span: usize,
}

impl CatalogCardGroup {
    pub(crate) fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.cards.len()
    }

    pub(crate) fn packed_rows(&self, units: usize) -> Vec<Vec<CatalogCardItem>> {
        pack_card_rows(&self.cards, units)
    }

    pub(crate) fn append(&mut self, mut other: CatalogCardGroup) {
        self.cards.append(&mut other.cards);
    }

    pub(crate) fn grid(&self, columns: usize) -> AnyElement {
        let contract = &self.contracts.store;
        let unit_width = catalog_grid_unit_width(contract.publication_min_width_px);
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(STORE_GRID_GAP))
            .children(self.packed_rows(columns).into_iter().map(|row| {
                let cards = row.into_iter().map(|item| (item.card, item.span)).collect::<Vec<_>>();
                crate::virtual_grid::fixed_spanned_entity_row(&cards, columns, unit_width, contract.publication_height_px as f32, STORE_GRID_GAP)
            }))
            .into_any_element()
    }
}

fn pack_card_rows(cards: &[CatalogCardItem], units: usize) -> Vec<Vec<CatalogCardItem>> {
    let units = units.max(1);
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut occupied = 0usize;
    for item in cards {
        let span = item.span.min(units);
        if occupied > 0 && occupied + span > units {
            rows.push(std::mem::take(&mut row));
            occupied = 0;
        }
        let mut item = item.clone();
        item.span = span;
        occupied += span;
        row.push(item);
    }
    if !row.is_empty() {
        rows.push(row);
    }
    rows
}

pub(crate) struct CatalogPublicationCardView {
    action: Option<CatalogPublicationAction>,
    presentation: CatalogCardPresentation,
    cover: Option<Entity<CatalogCoverView>>,
    show_source: bool,
    services: CatalogCardServices,
    import_task: Option<Task<()>>,
    import_error: Option<SharedString>,
    imported: bool,
    description_view: Option<Entity<HtmlView>>,
    description_view_key: Option<CatalogDescriptionViewKey>,
    description_viewport_width: Option<f32>,
    description_page: usize,
    description_has_next: bool,
}

#[derive(Clone, PartialEq)]
struct CatalogDescriptionViewKey {
    font: SharedString,
    font_size_bits: u32,
    width_bits: u32,
    foreground: u32,
    background: u32,
}

struct CatalogCardPresentation {
    element_id: SharedString,
    title: SharedString,
    authors: SharedString,
    description: Option<SharedString>,
    source_title: SharedString,
}

impl CatalogPublicationCardView {
    fn new(source: CatalogSource, publication: CatalogPublication, show_source: bool, services: CatalogCardServices, cx: &mut Context<Self>) -> Self {
        let action = publication_action(&source, &publication);
        let title = SharedString::from(publication.title.clone());
        let description = description_presentation(publication.description.as_ref());
        let cover = publication.image_url.clone().map(|image_url| {
            let cover_services = services.clone();
            let source_id = source.id.clone();
            let initial: SharedString = title.chars().next().unwrap_or('?').to_string().into();
            cx.new(|_| CatalogCoverView { source_id, image_url, initial, services: cover_services, image: None, task: None, attempted: false, retries: 0 })
        });
        let presentation = CatalogCardPresentation {
            element_id: format!("catalog-publication-{}", ownership_key(&source.id, &publication.identity.edition_id)).into(),
            authors: if publication.authors.is_empty() { "Unknown author".into() } else { publication.authors.iter().map(|author| author.name.as_str()).collect::<Vec<_>>().join(", ").into() },
            description: description.map(SharedString::from),
            source_title: source.title.clone().into(),
            title,
        };
        Self {
            action,
            presentation,
            cover,
            show_source,
            services,
            import_task: None,
            import_error: None,
            imported: false,
            description_view: None,
            description_view_key: None,
            description_viewport_width: None,
            description_page: 0,
            description_has_next: true,
        }
    }

    fn import(&mut self, request: CatalogImportRequest, cx: &mut Context<Self>) {
        if self.import_task.is_some() || self.imported {
            return;
        }
        self.import_error = None;
        let operation = match (self.services.import)(request) {
            Ok(operation) => operation,
            Err(error) => {
                self.import_error = Some(error.into());
                cx.notify();
                return;
            }
        };
        self.import_task = Some(cx.spawn(async move |card, cx| {
            let result = operation.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = card.update(cx, |card, cx| {
                card.import_task.take();
                match result {
                    Ok(()) => card.imported = true,
                    Err(error) => card.import_error = Some(error.into()),
                }
                cx.notify();
            });
        }));
    }
}

struct CatalogCoverView {
    source_id: CatalogSourceId,
    image_url: String,
    initial: SharedString,
    services: CatalogCardServices,
    image: Option<LoadedCover>,
    task: Option<Task<()>>,
    attempted: bool,
    retries: u8,
}

impl CatalogCoverView {
    fn load(&mut self, cx: &mut Context<Self>) {
        let operation = self.services.load_cover(self.source_id.clone(), self.image_url.clone());
        self.task = Some(cx.spawn(async move |cover, cx| {
            let result = operation.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = cover.update(cx, |cover, cx| {
                cover.task.take();
                match result {
                    Ok(bytes) => match loaded_cover_from_jpeg(bytes) {
                        Some(image) => cover.image = Some(image),
                        None => log::warn!("catalogue cover was not a valid normalized JPEG"),
                    },
                    Err(error) => {
                        log::warn!("failed to load catalogue cover: {error}");
                        if cover.retries < MAX_COVER_RETRIES {
                            let delay = Duration::from_secs(2_u64.pow(u32::from(cover.retries)).min(30));
                            cover.retries += 1;
                            let timer = cx.background_executor().timer(delay);
                            cover.task = Some(cx.spawn(async move |cover, cx| {
                                timer.await;
                                let _ = cover.update(cx, |cover, cx| {
                                    cover.task.take();
                                    cover.attempted = false;
                                    cx.notify();
                                });
                            }));
                        }
                    }
                }
                cx.notify();
            });
        }));
    }
}

impl Render for CatalogCoverView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.attempted {
            self.attempted = true;
            self.load(cx);
        }
        let theme = components::browser_theme(cx);
        let mut frame = div().w(px(components::BOOK_COVER_WIDTH)).h(px(components::BOOK_COVER_HEIGHT)).flex_none().relative();
        if let Some(cover) = self.image.clone() {
            frame = frame.child(
                div().w_full().min_w(px(components::BOOK_COVER_WIDTH)).aspect_ratio(components::BOOK_COVER_WIDTH / components::BOOK_COVER_HEIGHT).flex().flex_col().items_center().justify_start().child(components::book_cover_image(
                    cover.image,
                    cover.aspect_ratio,
                    theme,
                )),
            );
        } else {
            frame = frame.child(components::missing_book_cover(self.initial.clone(), theme));
        }
        frame
    }
}

impl Render for CatalogPublicationCardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let contract = &self.services.app.contracts.store;
        let title = self.presentation.title.clone();
        let authors = self.presentation.authors.clone();
        let has_cover = self.cover.is_some();
        let has_description = self.presentation.description.is_some();
        let show_source = self.show_source;
        let source_title = self.presentation.source_title.clone();
        let rem_size = f32::from(window.rem_size());
        let card_height = catalog_publication_height(contract.publication_height_px, rem_size);

        let mut publication_action = self.action.clone().map(|action| {
            let importing = self.import_task.is_some();
            let label = if self.imported {
                "Added".into()
            } else if self.import_error.is_some() {
                contract.retry_label.clone()
            } else {
                match &action {
                    CatalogPublicationAction::Import { .. } if importing => contract.adding_label.clone(),
                    CatalogPublicationAction::Import { .. } => contract.add_label.clone(),
                    CatalogPublicationAction::External { .. } => contract.open_label.clone(),
                }
            };
            let action_id = format!("{}/action", self.presentation.element_id);
            let action_height = gpui::rems(components::CAROUSEL_ARROW_SIZE / 16.0);
            match action {
                CatalogPublicationAction::Import { request } => {
                    let card = cx.entity();
                    components::primary_action_button(action_id, label, IconName::Plus, importing || self.imported)
                        .h(action_height)
                        .px(px(9.0))
                        .rounded(px(components::RADIUS_SM))
                        .on_click(move |_, _, cx| card.update(cx, |card, cx| card.import(request.clone(), cx)))
                        .into_any_element()
                }
                CatalogPublicationAction::External { url, .. } => {
                    components::secondary_action_button(action_id, label, IconName::ExternalLink, false).h(action_height).px(px(9.0)).rounded(px(components::RADIUS_SM)).on_click(move |_, _, cx| cx.open_url(&url)).into_any_element()
                }
            }
        });

        let identity_action = (has_cover && !has_description).then(|| publication_action.take()).flatten();
        let identity = self.cover.clone().map(|cover| {
            let identity_text = components::book_card_text().child(components::book_card_title().child(title.clone())).child(components::book_card_author(theme).child(authors.clone()));
            let cover = div()
                .relative()
                .w(px(components::BOOK_COVER_WIDTH))
                .h(px(components::BOOK_COVER_HEIGHT))
                .flex_none()
                .child(cover)
                .when_some(identity_action, |cover, action| cover.child(div().absolute().right(px(4.0)).bottom(px(4.0)).child(action)));
            components::book_card(format!("{}/identity", self.presentation.element_id)).w(px(components::BOOK_COVER_WIDTH)).h(px(card_height)).child(cover).child(identity_text)
        });

        let detail_bottom_padding = if has_description { catalog_description_action_bottom_padding(contract.publication_height_px, rem_size) } else { 16.0 };
        let mut detail = div().flex_1().min_w_0().h_full().min_h_0().pt(px(16.0)).pb(px(detail_bottom_padding)).pr(px(16.0)).flex().flex_col().gap_0();
        if !has_cover {
            let mut metadata = div()
                .w_full()
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .pb(px(10.0))
                .border_b_1()
                .border_color(theme.border)
                .child(div().line_clamp(2).text_size(gpui::rems(17.0 / 16.0)).line_height(gpui::relative(1.15)).font_weight(FontWeight::SEMIBOLD).child(title))
                .child(div().line_clamp(1).text_size(gpui::rems(15.0 / 16.0)).text_color(theme.text_muted).child(authors));
            if show_source {
                metadata = metadata.child(div().line_clamp(1).text_size(gpui::rems(11.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme.accent).child(source_title));
            }
            detail = detail.pl(px(16.0)).child(metadata);
        }
        let interface_font = crate::stores::interface_font(cx);
        let (description_min_width, description_max_width) = catalog_description_width_bounds(window, interface_font.clone());
        let description_width = self.description_viewport_width.unwrap_or(description_min_width).clamp(description_min_width, description_max_width);
        if let Some(description) = self.presentation.description.as_deref() {
            let foreground = u32::from(gpui::Rgba::from(theme.text_muted));
            let background = u32::from(gpui::Rgba::from(theme.page_bg));
            let view_key = CatalogDescriptionViewKey { font: interface_font.clone(), font_size_bits: rem_size.to_bits(), width_bits: description_width.to_bits(), foreground, background };
            if self.description_view_key.as_ref() != Some(&view_key) {
                let provider = description_provider(description);
                let config = RendererInitialConfig {
                    font_size: rem_size,
                    column_width: description_width as f64,
                    max_column_count: Some(1),
                    style_overrides: html::pipeline::ReaderStyleOverrides {
                        font_family: Some(interface_font.to_string()),
                        text_align: Some(html::pipeline::TextAlign::Justify),
                        line_height: Some(1.35),
                        foreground: Some(foreground),
                        background: Some(background),
                        ..Default::default()
                    },
                    paint_palette: html_view_core::ReaderPaintPalette { foreground: Some(foreground), background: Some(background) },
                    ..Default::default()
                };
                self.description_view = Some(cx.new(|cx| {
                    let mut view = HtmlView::from_provider(provider, vec!["description.html".to_owned()], 0, None, config, window, cx);
                    view.set_wheel_scroll_enabled(false);
                    view
                }));
                self.description_view_key = Some(view_key);
                self.description_page = 0;
                self.description_has_next = true;
            }
        } else {
            self.description_view = None;
            self.description_view_key = None;
            self.description_page = 0;
            self.description_has_next = false;
        }
        if let Some(description) = self.description_view.clone() {
            let card = cx.entity();
            detail = detail.child(
                div().w_full().max_w(px(description_max_width + DESCRIPTION_TEXT_RIGHT_INSET)).flex_1().min_h_0().overflow_hidden().pr(px(DESCRIPTION_TEXT_RIGHT_INSET)).child(
                    div()
                        .w_full()
                        .h_full()
                        .min_h_0()
                        .overflow_hidden()
                        .on_prepaint(move |bounds, _, cx| {
                            let width = f32::from(bounds.size.width);
                            let _ = card.update(cx, |card, cx| {
                                if card.description_viewport_width.map_or(true, |current| (current - width).abs() >= 1.0) {
                                    card.description_viewport_width = Some(width);
                                    cx.notify();
                                }
                            });
                        })
                        .child(description),
                ),
            );
        }
        if let Some(error) = &self.import_error {
            detail = detail.child(div().h(px(32.0)).flex_none().overflow_hidden().line_clamp(2).text_size(gpui::rems(11.0 / 16.0)).text_color(theme.danger).child(error.clone()));
        }

        let mut actions = div().w_full().flex_none().flex().items_center().justify_end().gap(px(8.0));
        if let Some(description) = self.description_view.clone() {
            if self.description_page > 0 {
                let previous_card = cx.entity();
                let previous_description = description.clone();
                actions = actions.child(components::carousel_arrow_button(format!("{}/description-previous", self.presentation.element_id), "Previous description page", IconName::ChevronLeft, theme).on_click(move |_, _, cx| {
                    let moved = previous_description.update(cx, |view, cx| view.previous_page(cx));
                    previous_card.update(cx, |card, cx| {
                        if moved {
                            card.description_page = card.description_page.saturating_sub(1);
                            card.description_has_next = true;
                        }
                        cx.notify();
                    });
                }));
            }
            if self.description_has_next {
                let next_card = cx.entity();
                actions = actions.child(components::carousel_arrow_button(format!("{}/description-next", self.presentation.element_id), "Next description page", IconName::ChevronRight, theme).on_click(move |_, _, cx| {
                    let moved = description.update(cx, |view, cx| view.next_page(cx));
                    next_card.update(cx, |card, cx| {
                        if moved {
                            card.description_page += 1;
                        } else {
                            card.description_has_next = false;
                        }
                        cx.notify();
                    });
                }));
            }
        }
        if let Some(action) = publication_action {
            actions = actions.child(action);
        }
        if self.action.is_some() || self.description_view.is_some() {
            detail = detail.child(actions);
        }

        let detail = (has_description || !has_cover).then_some(detail);

        div()
            .id(self.presentation.element_id.clone())
            .when(has_description, |card| card.w_full())
            .when(!has_description, |card| card.w(px(components::BOOK_COVER_WIDTH)))
            .h(px(card_height))
            .flex_none()
            .flex()
            .items_start()
            .gap(px(STORE_CARD_CONTENT_GAP))
            .overflow_hidden()
            .border_1()
            .border_color(theme.border)
            .bg(theme.page_bg)
            .when_some(identity, |card, identity| card.child(identity))
            .when_some(detail, |card, detail| card.child(detail))
            .into_any_element()
    }
}

/// Prefers importing over opening externally, and falls back to a plain link
/// when a source offers no explicit action but the edition id is a usable URL.
pub(crate) fn publication_action(source: &CatalogSource, publication: &CatalogPublication) -> Option<CatalogPublicationAction> {
    publication.actions.iter().find(|action| matches!(action, CatalogPublicationAction::Import { .. })).or_else(|| publication.actions.iter().find(|action| matches!(action, CatalogPublicationAction::External { .. }))).cloned().or_else(
        || {
            let identity = publication.identity.edition_id.trim();
            let open_library_path = identity.strip_prefix("https://openlibrary.org/opds/books/");
            let url = if source.id.as_str() == "opds-open-library" { open_library_path.map(|edition| format!("https://openlibrary.org/books/{edition}")).unwrap_or_else(|| identity.to_owned()) } else { identity.to_owned() };
            (url.len() <= 2048 && !url.chars().any(char::is_control) && (url.starts_with("https://") || url.starts_with("http://"))).then(|| CatalogPublicationAction::External {
                purpose: epub_svelte_lib::catalog::CatalogExternalActionKind::Open,
                url,
                price: None,
                available_formats: Vec::new(),
            })
        },
    )
}

fn has_description_excerpt(description: Option<&CatalogDescription>) -> bool {
    match description {
        Some(CatalogDescription::PlainText { text }) => !text.trim().is_empty(),
        Some(CatalogDescription::RichHtml { plain_text, .. }) => !plain_text.trim().is_empty(),
        None => false,
    }
}

fn description_presentation(description: Option<&CatalogDescription>) -> Option<String> {
    let Some(description) = description else {
        return None;
    };
    let CatalogDescription::RichHtml { plain_text, .. } = description else {
        let CatalogDescription::PlainText { text } = description else { unreachable!() };
        return (!text.trim().is_empty()).then(|| text.clone());
    };
    (!plain_text.trim().is_empty()).then(|| plain_text.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use epub_svelte_lib::catalog::{CatalogAuthentication, CatalogExternalActionKind, CatalogPublicationIdentity};

    fn source(id: &str) -> CatalogSource {
        CatalogSource { id: CatalogSourceId::parse(id).unwrap(), title: "Test catalogue".to_owned(), description: String::new(), languages: Vec::new(), authentication: CatalogAuthentication::None, capabilities: Vec::new() }
    }

    fn metadata_only_publication(edition_id: &str) -> CatalogPublication {
        CatalogPublication {
            identity: CatalogPublicationIdentity { edition_id: edition_id.to_owned(), work_id: None },
            title: "A catalogue record".to_owned(),
            authors: Vec::new(),
            languages: Vec::new(),
            description: None,
            image_url: None,
            actions: Vec::new(),
        }
    }

    #[test]
    fn metadata_only_web_editions_remain_actionable_without_claiming_they_are_downloadable() {
        let publication = metadata_only_publication("https://catalog.example/books/edition-1");
        assert!(matches!(publication_action(&source("opds-test"), &publication), Some(CatalogPublicationAction::External { purpose: CatalogExternalActionKind::Open, ref url, .. }) if url == "https://catalog.example/books/edition-1"));
    }

    #[test]
    fn opaque_metadata_identifiers_do_not_become_external_actions() {
        let publication = metadata_only_publication("urn:isbn:9780306406157");
        assert!(publication_action(&source("opds-test"), &publication).is_none());
    }

    #[test]
    fn cached_open_library_records_open_the_human_book_page() {
        let publication = metadata_only_publication("https://openlibrary.org/opds/books/OL8019918M");
        assert!(matches!(publication_action(&source("opds-open-library"), &publication), Some(CatalogPublicationAction::External { ref url, .. }) if url == "https://openlibrary.org/books/OL8019918M"));
    }

    #[test]
    fn ownership_keys_do_not_collide_across_sources() {
        let left = CatalogSourceId::parse("opds-a").unwrap();
        let right = CatalogSourceId::parse("opds-b").unwrap();

        assert_ne!(ownership_key(&left, "edition"), ownership_key(&right, "edition"));
    }

    #[test]
    fn rich_catalogue_descriptions_use_the_precomputed_plain_text() {
        let description = CatalogDescription::RichHtml { safe_html: "<p>expensive markup</p>".to_owned(), plain_text: "Ready to render".to_owned() };
        let excerpt = description_presentation(Some(&description));
        assert_eq!(excerpt.as_deref(), Some("Ready to render"));
    }

    #[test]
    fn missing_descriptions_have_no_placeholder_excerpt() {
        let excerpt = description_presentation(None);
        assert!(excerpt.is_none());
    }

    #[test]
    fn rich_descriptions_do_not_require_sanitized_markup_for_the_excerpt() {
        let description = CatalogDescription::RichHtml { safe_html: String::new(), plain_text: "Still readable".to_owned() };
        let excerpt = description_presentation(Some(&description));
        assert_eq!(excerpt.as_deref(), Some("Still readable"));
    }

    #[test]
    fn store_columns_grow_descriptions_without_changing_the_gap() {
        assert_eq!(catalog_grid_unit_width_for_available(656.0, 3, 180.0, 240.0, 16.0), 208.0);
        assert_eq!(catalog_grid_unit_width_for_available(800.0, 3, 180.0, 240.0, 16.0), 240.0);
        assert_eq!(catalog_grid_unit_width_for_available(500.0, 3, 180.0, 240.0, 16.0), 180.0);
    }
}
