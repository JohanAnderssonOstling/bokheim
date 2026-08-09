//! Catalogue presentation shared by the Store and Authors pages.
//!
//! Each retained publication card owns its cover and import tasks. Pages only
//! own ordered groups of those cards.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use ui_components as components;
use ui_components::ScrollableElement;
use epub_svelte_lib::FrontendContract;
use epub_svelte_lib::app::executor::BackendTask;
use epub_svelte_lib::catalog::{CatalogDescription, CatalogImportRequest, CatalogPublication, CatalogPublicationAction, CatalogSource, CatalogSourceId};
use gpui::prelude::*;
use gpui::{AnyElement, AppContext, Context, Div, Entity, FontWeight, Image, ImageFormat, IntoElement, Render, SharedString, Task, Window, div, img, px};
use gpui_component::IconName;
use html::parse::{HtmlFragmentContext, HtmlParserOptions, HtmlSyntaxNode, parse_html_fragment};

use crate::services::AppServices;

const STORE_GRID_GAP: f32 = 16.0;

/// Identifies one edition within one source, for ownership and import errors.
pub(crate) fn ownership_key(source_id: &CatalogSourceId, edition_id: &str) -> String {
    format!("{}:{edition_id}", source_id.as_str())
}

pub(crate) type CatalogImport = Rc<dyn Fn(CatalogImportRequest) -> Result<BackendTask<Result<(), String>>, String>>;

#[derive(Clone)]
pub(crate) struct CatalogCardServices {
    core: epub_svelte_lib::app::AppBackend,
    import: CatalogImport,
    contracts: Arc<FrontendContract>,
}

impl CatalogCardServices {
    pub(crate) fn new(services: &AppServices, import: CatalogImport) -> Self {
        Self { core: services.backend.clone(), import, contracts: services.contracts.clone() }
    }

    pub(crate) fn group<P: 'static>(&self, source: &CatalogSource, publications: &[CatalogPublication], owned: &HashSet<String>, show_source: bool, cx: &mut Context<P>) -> CatalogCardGroup {
        let mut cards = Vec::new();
        let mut seen = HashSet::new();
        for publication in publications {
            let key = ownership_key(&source.id, &publication.identity.edition_id);
            if owned.contains(&key) || !seen.insert(key) {
                continue;
            }
            let services = self.clone();
            let source = source.clone();
            let publication = publication.clone();
            cards.push(cx.new(|_| CatalogPublicationCardView::new(source, publication, show_source, services)));
        }
        CatalogCardGroup { cards, contracts: self.contracts.clone() }
    }

    fn load_cover(&self, source_id: CatalogSourceId, image_url: String) -> BackendTask<Result<Vec<u8>, String>> {
        self.core.spawn(move |core| async move { core.catalog_cover(&source_id, &image_url).await.map_err(|error| error.to_string()) })
    }
}

#[derive(Clone)]
pub(crate) struct CatalogCardGroup {
    cards: Vec<Entity<CatalogPublicationCardView>>,
    contracts: Arc<FrontendContract>,
}

impl CatalogCardGroup {
    pub(crate) fn grid(&self, width: f32, columns: usize) -> AnyElement {
        let contract = &self.contracts.store;
        let card_width = ((width - STORE_GRID_GAP * columns.saturating_sub(1) as f32) / columns as f32).max(contract.publication_min_width_px as f32);
        div().w_full().flex().flex_row().flex_wrap().gap(px(STORE_GRID_GAP)).children(self.cards.iter().cloned().map(|card| div().w(px(card_width)).h(px(contract.publication_height_px as f32)).flex_none().child(card))).into_any_element()
    }
}

struct CatalogPublicationCardView {
    source: CatalogSource,
    publication: CatalogPublication,
    show_source: bool,
    services: CatalogCardServices,
    cover: Option<Arc<Image>>,
    cover_task: Option<Task<()>>,
    import_task: Option<Task<()>>,
    import_error: Option<SharedString>,
    imported: bool,
}

impl CatalogPublicationCardView {
    fn new(source: CatalogSource, publication: CatalogPublication, show_source: bool, services: CatalogCardServices) -> Self {
        Self { source, publication, show_source, services, cover: None, cover_task: None, import_task: None, import_error: None, imported: false }
    }

    fn load_cover(&mut self, cx: &mut Context<Self>) {
        let Some(image_url) = self.publication.image_url.clone() else { return };
        let operation = self.services.load_cover(self.source.id.clone(), image_url);
        self.cover_task = Some(cx.spawn(async move |card, cx| {
            let result = operation.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = card.update(cx, |card, cx| {
                card.cover_task.take();
                match result {
                    Ok(bytes) if bytes.starts_with(&[0xff, 0xd8, 0xff]) => card.cover = Some(Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes))),
                    Ok(_) => log::warn!("catalogue cover was not normalized JPEG"),
                    Err(error) => log::warn!("failed to load catalogue cover: {error}"),
                }
                cx.notify();
            });
        }));
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

impl Render for CatalogPublicationCardView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.cover.is_none() && self.cover_task.is_none() && self.publication.image_url.is_some() {
            self.load_cover(cx);
        }
        let theme = components::browser_theme(cx);
        let contract = &self.services.contracts.store;
        let title = self.publication.title.clone();
        let authors = if self.publication.authors.is_empty() { "Unknown author".to_owned() } else { self.publication.authors.iter().map(|author| author.name.as_str()).collect::<Vec<_>>().join(", ") };
        let cover_missing = self.cover.is_none();
        let show_source = self.show_source;
        let source_title = self.source.title.clone();
        let cover = div()
            .w(px(components::BOOK_COVER_WIDTH))
            .h(px(components::BOOK_COVER_HEIGHT))
            .flex_none()
            .relative()
            .overflow_hidden()
            .rounded(px(8.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface_alt)
            .when_some(self.cover.clone(), |cover, image| cover.child(img(image).size_full().object_fit(gpui::ObjectFit::Cover)))
            .when(cover_missing, |cover| cover.flex().items_center().justify_center().text_size(gpui::rems(24.0 / 16.0)).font_weight(FontWeight::BOLD).child(title.chars().next().unwrap_or('?').to_string()))
            .when(show_source, |cover| {
                cover.child(
                    div()
                        .absolute()
                        .left(px(6.0))
                        .right(px(6.0))
                        .bottom(px(6.0))
                        .overflow_hidden()
                        .rounded(px(5.0))
                        .bg(theme.card_bg)
                        .px(px(6.0))
                        .py(px(3.0))
                        .line_clamp(1)
                        .text_size(gpui::rems(10.0 / 16.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(source_title),
                )
            });

        let identity = div()
            .w(px(components::BOOK_COVER_WIDTH))
            .h(px(contract.publication_height_px as f32))
            .flex_none()
            .flex()
            .flex_col()
            .child(cover)
            .child(div().w_full().h(px(42.0)).mt(px(9.0)).px(px(2.0)).line_clamp(2).text_size(gpui::rems(17.0 / 16.0)).line_height(gpui::relative(1.15)).font_weight(FontWeight::SEMIBOLD).child(title))
            .child(div().w_full().h(px(22.0)).px(px(2.0)).line_clamp(1).text_size(gpui::rems(15.0 / 16.0)).text_color(theme.text_muted).child(authors));

        let mut detail = div().flex_1().min_w_0().h_full().min_h_0().py(px(16.0)).pr(px(16.0)).flex().flex_col().gap(px(10.0));
        detail = detail.child(
            div()
                .flex_1()
                .min_h_0()
                .pr(px(contract.description_scrollbar_width_px as f32))
                .overflow_y_scrollbar()
                .text_size(gpui::rems(13.0 / 16.0))
                .text_color(theme.text_muted)
                .child(rich_description(self.publication.description.as_ref(), theme)),
        );
        if let Some(error) = &self.import_error {
            detail = detail.child(div().max_h(px(48.0)).overflow_y_scrollbar().text_size(gpui::rems(11.0 / 16.0)).text_color(theme.danger).child(error.clone()));
        }

        if let Some(action) = publication_action(&self.source, &self.publication) {
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
            let button = match action {
                CatalogPublicationAction::Import { request } => {
                    let card = cx.entity();
                    components::cloud_primary_button("catalog-add", label, IconName::Plus, importing || self.imported).on_click(move |_, _, cx| card.update(cx, |card, cx| card.import(request.clone(), cx)))
                }
                CatalogPublicationAction::External { url, .. } => components::cloud_secondary_button("catalog-open", label, IconName::ExternalLink, false).on_click(move |_, _, cx| cx.open_url(&url)),
            };
            detail = detail.child(div().flex().justify_end().child(button));
        }

        div()
            .id(format!("catalog-publication-{}", ownership_key(&self.source.id, &self.publication.identity.edition_id)))
            .w_full()
            .h(px(contract.publication_height_px as f32))
            .flex_none()
            .flex()
            .gap(px(STORE_GRID_GAP))
            .overflow_hidden()
            .rounded(px(12.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.card_bg)
            .child(identity)
            .child(detail)
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

fn rich_description(description: Option<&CatalogDescription>, theme: components::BrowserTheme) -> Div {
    let Some(description) = description else {
        return div().flex().flex_col().child("No description available.");
    };
    let CatalogDescription::RichHtml { safe_html, .. } = description else {
        let CatalogDescription::PlainText { text } = description else { unreachable!() };
        return div().flex().flex_col().child(text.clone());
    };
    let parsed = parse_html_fragment(safe_html, &HtmlFragmentContext::html("div"), HtmlParserOptions::default());
    let mut content = div().w_full().flex().flex_col().gap(px(7.0));
    for node in parsed.syntax_tree().nodes {
        let HtmlSyntaxNode::Element(element) = node else {
            let text = syntax_text(std::slice::from_ref(&node));
            if !text.is_empty() {
                content = content.child(div().child(text));
            }
            continue;
        };
        let text = syntax_text(&element.children);
        if text.is_empty() {
            continue;
        }
        let block = match element.local_name.as_str() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => div().text_color(theme.text).font_weight(FontWeight::SEMIBOLD).child(text),
            "blockquote" => div().pl(px(9.0)).border_l_2().border_color(theme.border).child(text),
            "ul" => div().pl(px(8.0)).child(format!("• {text}")),
            "ol" => div().pl(px(8.0)).child(format!("1. {text}")),
            "pre" | "code" => div().p(px(7.0)).rounded(px(6.0)).bg(theme.surface_alt).child(text),
            _ => div().child(text),
        };
        content = content.child(block);
    }
    content
}

#[cfg(test)]
mod tests {
    use super::*;
    use epub_svelte_lib::catalog::{CatalogAuthentication, CatalogExternalActionKind, CatalogProviderKind, CatalogPublicationIdentity};

    fn source(id: &str) -> CatalogSource {
        CatalogSource {
            id: CatalogSourceId::parse(id).unwrap(),
            provider: CatalogProviderKind::Opds,
            title: "Test catalogue".to_owned(),
            description: String::new(),
            languages: Vec::new(),
            authentication: CatalogAuthentication::None,
            capabilities: Vec::new(),
        }
    }

    fn metadata_only_publication(edition_id: &str) -> CatalogPublication {
        CatalogPublication { identity: CatalogPublicationIdentity { edition_id: edition_id.to_owned(), work_id: None }, title: "A catalogue record".to_owned(), authors: Vec::new(), description: None, image_url: None, actions: Vec::new() }
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
    fn ownership_and_cover_keys_do_not_collide_across_sources() {
        let left = CatalogSourceId::parse("opds-a").unwrap();
        let right = CatalogSourceId::parse("opds-b").unwrap();

        assert_ne!(ownership_key(&left, "edition"), ownership_key(&right, "edition"));
        assert_ne!(catalog_cover_key(&left, "cover.jpg"), catalog_cover_key(&right, "cover.jpg"));
    }
}

fn syntax_text(nodes: &[HtmlSyntaxNode]) -> String {
    let mut text = String::new();
    for node in nodes {
        match node {
            HtmlSyntaxNode::Text(value) => text.push_str(value),
            HtmlSyntaxNode::Element(element) => text.push_str(&syntax_text(&element.children)),
            HtmlSyntaxNode::TemplateContents(children) => text.push_str(&syntax_text(children)),
            _ => {}
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
