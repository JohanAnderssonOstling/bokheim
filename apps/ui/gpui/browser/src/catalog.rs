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
use epub_svelte_lib::catalog::{CatalogBookFormat, CatalogDescription, CatalogImportRequest, CatalogPublication, CatalogPublicationAction, CatalogPublicationKey, CatalogSource, CatalogSourceId};
use gpui::prelude::*;
use gpui::{AnyElement, AppContext, Context, Entity, FontWeight, Image, ImageFormat, IntoElement, Render, SharedString, Task, Window, div, img, px};
use gpui_component::IconName;
use gpui_component::WindowExt;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::text::TextView;
use ui_components as components;

use crate::services::AppServices;

const STORE_GRID_GAP: f32 = 16.0;
const MAX_COVER_RETRIES: u8 = 5;

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
            let expanded = publication.image_url.is_some() && has_description_excerpt(publication.description.as_ref());
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
}

struct CatalogCardPresentation {
    element_id: SharedString,
    title: SharedString,
    authors: SharedString,
    description: Option<SharedString>,
    details: Option<CatalogDescriptionBody>,
    source_title: SharedString,
}

#[derive(Clone)]
enum CatalogDescriptionBody {
    PlainText(SharedString),
    RichHtml(SharedString),
}

impl CatalogPublicationCardView {
    fn new(source: CatalogSource, publication: CatalogPublication, show_source: bool, services: CatalogCardServices, cx: &mut Context<Self>) -> Self {
        let action = publication_action(&source, &publication);
        let format_label = action.as_ref().and_then(action_format).map(|format| SharedString::from(format.label()));
        let title = SharedString::from(publication.title.clone());
        let (description, details) = description_presentation(publication.description.as_ref());
        let cover = publication.image_url.clone().map(|image_url| {
            let cover_services = services.clone();
            let source_id = source.id.clone();
            let source_title: SharedString = source.title.clone().into();
            let initial: SharedString = title.chars().next().unwrap_or('?').to_string().into();
            let cover_format = format_label.clone();
            cx.new(|_| CatalogCoverView { source_id, image_url, source_title, initial, format_label: cover_format, show_source, services: cover_services, image: None, task: None, attempted: false, retries: 0 })
        });
        let presentation = CatalogCardPresentation {
            element_id: format!("catalog-publication-{}", ownership_key(&source.id, &publication.identity.edition_id)).into(),
            authors: if publication.authors.is_empty() { "Unknown author".into() } else { publication.authors.iter().map(|author| author.name.as_str()).collect::<Vec<_>>().join(", ").into() },
            description: description.map(SharedString::from),
            details,
            source_title: source.title.clone().into(),
            title,
        };
        Self { action, presentation, cover, show_source, services, import_task: None, import_error: None, imported: false }
    }

    fn show_details(&self, cx: &mut Context<Self>) {
        let Some(description) = self.presentation.details.clone() else { return };
        let title = self.presentation.title.clone();
        let authors = self.presentation.authors.clone();
        let text_view_id: SharedString = format!("{}/description", self.presentation.element_id).into();
        self.services.app.with_window(cx, move |window, cx| {
            window.open_dialog(cx, move |dialog, _, cx| {
                let content = div().w_full().flex().flex_col().gap(px(14.0)).child(div().text_color(components::browser_theme(cx).text_muted).child(authors.clone())).child(match description.clone() {
                    CatalogDescriptionBody::PlainText(text) => div().line_height(gpui::relative(1.45)).child(text).into_any_element(),
                    CatalogDescriptionBody::RichHtml(html) => TextView::html(text_view_id.clone(), html).selectable(true).into_any_element(),
                });
                dialog.title(title.clone()).w(px(600.0)).max_h(px(640.0)).child(content).button_props(DialogButtonProps::default().ok_text("Close"))
            });
        });
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
    source_title: SharedString,
    initial: SharedString,
    format_label: Option<SharedString>,
    show_source: bool,
    services: CatalogCardServices,
    image: Option<Arc<Image>>,
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
                    Ok(bytes) if bytes.starts_with(&[0xff, 0xd8, 0xff]) => cover.image = Some(Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes))),
                    Ok(_) => log::warn!("catalogue cover was not normalized JPEG"),
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
        let mut frame = div().w_full().relative().overflow_hidden().rounded(px(0.0)).border_1().border_color(theme.border).bg(theme.surface_alt);
        if let Some(image) = self.image.clone() {
            frame = frame.h_auto().max_h_full().child(img(image).w_full().h_auto().max_h_full().object_fit(gpui::ObjectFit::Contain));
        } else {
            frame = frame.h_full().flex().items_center().justify_center().text_size(gpui::rems(24.0 / 16.0)).font_weight(FontWeight::BOLD).child(self.initial.clone());
        }
        frame = frame
            .when_some(self.format_label.clone(), |cover, format| {
                cover.child(
                    div()
                        .absolute()
                        .top(px(7.0))
                        .right(px(7.0))
                        .bg(theme.raised_bg)
                        .border_1()
                        .border_color(theme.border)
                        .rounded(px(components::RADIUS_SM))
                        .px(px(7.0))
                        .py(px(3.0))
                        .text_size(gpui::rems(10.0 / 16.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format),
                )
            })
            .when(self.show_source, |cover| {
                cover.child(
                    div()
                        .absolute()
                        .left(px(6.0))
                        .right(px(6.0))
                        .bottom(px(6.0))
                        .overflow_hidden()
                        .bg(theme.raised_bg)
                        .px(px(6.0))
                        .py(px(3.0))
                        .line_clamp(1)
                        .text_size(gpui::rems(10.0 / 16.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(self.source_title.clone()),
                )
            });
        div().w(px(components::BOOK_COVER_WIDTH)).h(px(components::BOOK_COVER_HEIGHT)).flex_none().flex().flex_col().items_center().justify_end().child(frame)
    }
}

impl Render for CatalogPublicationCardView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let contract = &self.services.app.contracts.store;
        let title = self.presentation.title.clone();
        let authors = self.presentation.authors.clone();
        let has_cover = self.cover.is_some();
        let has_description = self.presentation.description.is_some();
        let show_source = self.show_source;
        let source_title = self.presentation.source_title.clone();

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
            match action {
                CatalogPublicationAction::Import { request } => {
                    let card = cx.entity();
                    components::primary_action_button(action_id, label, IconName::Plus, importing || self.imported)
                        .h(px(30.0))
                        .px(px(9.0))
                        .rounded(px(components::RADIUS_SM))
                        .on_click(move |_, _, cx| card.update(cx, |card, cx| card.import(request.clone(), cx)))
                        .into_any_element()
                }
                CatalogPublicationAction::External { url, .. } => {
                    components::secondary_action_button(action_id, label, IconName::ExternalLink, false).h(px(30.0)).px(px(9.0)).rounded(px(components::RADIUS_SM)).on_click(move |_, _, cx| cx.open_url(&url)).into_any_element()
                }
            }
        });

        let cover_action = if has_cover { publication_action.take() } else { None };
        let identity = self.cover.clone().map(|cover| {
            div()
                .w(px(components::BOOK_COVER_WIDTH))
                .h(px(contract.publication_height_px as f32))
                .flex_none()
                .flex()
                .flex_col()
                .child(
                    div()
                        .w(px(components::BOOK_COVER_WIDTH))
                        .h(px(components::BOOK_COVER_HEIGHT))
                        .flex_none()
                        .relative()
                        .child(cover)
                        .when_some(cover_action, |cover, action| cover.child(div().absolute().top(px(7.0)).left(px(7.0)).child(action))),
                )
                .child(div().w_full().h(px(42.0)).mt(px(9.0)).px(px(2.0)).line_clamp(2).text_size(gpui::rems(17.0 / 16.0)).line_height(gpui::relative(1.15)).font_weight(FontWeight::SEMIBOLD).child(title.clone()))
                .child(div().w_full().h(px(22.0)).px(px(2.0)).line_clamp(1).text_size(gpui::rems(15.0 / 16.0)).text_color(theme.text_muted).child(authors.clone()))
        });

        let mut detail = div().flex_1().min_w_0().h_full().min_h_0().py(px(16.0)).pr(px(16.0)).flex().flex_col().gap(px(10.0));
        if !has_cover {
            let mut metadata = div()
                .w_full()
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(4.0))
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
        if let Some(description) = self.presentation.description.clone() {
            detail = detail.child(div().flex_1().min_h_0().overflow_hidden().line_clamp(12).line_height(gpui::relative(1.35)).text_size(gpui::rems(1.0)).text_color(theme.text_muted).child(description));
        }
        if let Some(error) = &self.import_error {
            detail = detail.child(div().h(px(32.0)).flex_none().overflow_hidden().line_clamp(2).text_size(gpui::rems(11.0 / 16.0)).text_color(theme.danger).child(error.clone()));
        }

        let mut actions = div().flex().justify_end().gap(px(8.0));
        if self.presentation.details.is_some() {
            let card = cx.entity();
            actions = actions.child(components::secondary_action_button("catalog-details", "Details", IconName::BookOpen, false).on_click(move |_, _, cx| card.update(cx, |card, cx| card.show_details(cx))));
        }
        if let Some(action) = publication_action {
            actions = actions.child(action);
        }
        if self.presentation.details.is_some() || (!has_cover && self.action.is_some()) {
            detail = detail.child(actions);
        }

        let detail = (has_description || !has_cover).then_some(detail);

        div()
            .id(self.presentation.element_id.clone())
            .w_full()
            .h(px(contract.publication_height_px as f32))
            .flex_none()
            .flex()
            .gap(px(STORE_GRID_GAP))
            .overflow_hidden()
            .rounded(px(0.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.raised_bg)
            .when_some(identity, |card, identity| card.child(identity))
            .when_some(detail, |card, detail| card.child(detail))
            .into_any_element()
    }
}

fn action_format(action: &CatalogPublicationAction) -> Option<CatalogBookFormat> {
    match action {
        CatalogPublicationAction::Import { request } => Some(request.book_format),
        CatalogPublicationAction::External { available_formats, .. } => available_formats.first().copied(),
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

fn description_presentation(description: Option<&CatalogDescription>) -> (Option<String>, Option<CatalogDescriptionBody>) {
    let Some(description) = description else {
        return (None, None);
    };
    let CatalogDescription::RichHtml { safe_html, plain_text } = description else {
        let CatalogDescription::PlainText { text } = description else { unreachable!() };
        let details = (!text.trim().is_empty()).then(|| CatalogDescriptionBody::PlainText(text.clone().into()));
        let excerpt = (!text.trim().is_empty()).then(|| text.clone());
        return (excerpt, details);
    };
    let excerpt = (!plain_text.trim().is_empty()).then(|| plain_text.clone());
    let details = if !safe_html.trim().is_empty() {
        Some(CatalogDescriptionBody::RichHtml(safe_html.clone().into()))
    } else if !plain_text.trim().is_empty() {
        Some(CatalogDescriptionBody::PlainText(plain_text.clone().into()))
    } else {
        None
    };
    (excerpt, details)
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
        let (excerpt, details) = description_presentation(Some(&description));
        assert_eq!(excerpt.as_deref(), Some("Ready to render"));
        assert!(matches!(details, Some(CatalogDescriptionBody::RichHtml(html)) if html == "<p>expensive markup</p>"));
    }

    #[test]
    fn missing_descriptions_have_no_placeholder_excerpt() {
        let (excerpt, details) = description_presentation(None);
        assert!(excerpt.is_none());
        assert!(details.is_none());
    }

    #[test]
    fn empty_sanitized_markup_falls_back_to_plain_details() {
        let description = CatalogDescription::RichHtml { safe_html: String::new(), plain_text: "Still readable".to_owned() };
        let (_, details) = description_presentation(Some(&description));
        assert!(matches!(details, Some(CatalogDescriptionBody::PlainText(text)) if text == "Still readable"));
    }
}
