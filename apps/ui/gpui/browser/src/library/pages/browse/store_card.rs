//! Publication cards for subject-based Store browsing.
//!
//! Each publication card owns its cover and import tasks. Pages only own
//! ordered groups of those cards.

use std::rc::Rc;
use std::time::Duration;

use app_contract::catalog::{CatalogImportRequest, CatalogPublication, CatalogPublicationAction, CatalogSourceId};
use app_contract::library::BrowseFuture;
use gpui::prelude::*;
use gpui::{AppContext, Context, Entity, IntoElement, Render, SharedString, Task, WeakEntity, Window};
use ui_components as components;

use crate::library::services::LibraryContext;
use crate::library::{LoadedCover, loaded_cover_from_jpeg};

const MAX_COVER_RETRIES: u8 = 5;

/// Identifies one edition within one source for stable element IDs.
fn ownership_key(source_id: &CatalogSourceId, edition_id: &str) -> String {
    format!("{}:{edition_id}", source_id.as_str())
}

pub(crate) type StoreImport = Rc<dyn Fn(CatalogImportRequest) -> BrowseFuture<Result<(), String>>>;

#[derive(Clone)]
pub(crate) struct StoreBookServices {
    import: StoreImport,
    library: LibraryContext,
}

impl StoreBookServices {
    pub(crate) fn new(library: LibraryContext, import: StoreImport) -> Self {
        Self { import, library }
    }

    fn load_cover(&self, source_id: CatalogSourceId, image_url: String) -> BrowseFuture<Result<Vec<u8>, String>> {
        self.library.app_backend().catalog_cover(source_id, image_url)
    }
}

pub(crate) struct StoreBookCardView {
    import_request: Option<CatalogImportRequest>,
    presentation: StoreBookPresentation,
    cover: Entity<StoreCoverView>,
    services: StoreBookServices,
    import_task: Option<Task<()>>,
    imported: bool,
}

struct StoreBookPresentation {
    element_id: SharedString,
    title: SharedString,
    authors: SharedString,
}

impl StoreBookCardView {
    pub(crate) fn new(source_id: CatalogSourceId, publication: CatalogPublication, services: StoreBookServices, cx: &mut Context<Self>) -> Self {
        let import_request = publication.actions.iter().find_map(|action| match action {
            CatalogPublicationAction::Import { request } => Some(request.clone()),
            CatalogPublicationAction::External { .. } => None,
        });
        let title = SharedString::from(publication.title.clone());
        let cover_services = services.clone();
        let image_url = publication.image_url.clone();
        let initial: SharedString = title.chars().next().unwrap_or('?').to_string().into();
        let element_id = format!("catalog-publication-{}", ownership_key(&source_id, &publication.identity.edition_id));
        let cover_element_id = format!("{element_id}/interactive").into();
        let card = cx.entity().downgrade();
        let cover_import_request = import_request.clone();
        let cover = cx.new(|_| StoreCoverView {
            source_id,
            image_url,
            initial,
            services: cover_services,
            card,
            element_id: cover_element_id,
            import_request: cover_import_request,
            image: None,
            task: None,
            attempted: false,
            settled: false,
            retries: 0,
        });
        let presentation = StoreBookPresentation {
            element_id: element_id.into(),
            authors: if publication.authors.is_empty() { "Unknown author".into() } else { publication.authors.iter().map(|author| author.name.as_str()).collect::<Vec<_>>().join(", ").into() },
            title,
        };
        Self { import_request, presentation, cover, services, import_task: None, imported: false }
    }

    fn import(&mut self, request: CatalogImportRequest, cx: &mut Context<Self>) {
        if self.import_task.is_some() || self.imported {
            return;
        }
        let operation = (self.services.import)(request);
        self.import_task = Some(cx.spawn(async move |card, cx| {
            let result = operation.await;
            let _ = card.update(cx, |card, cx| {
                card.import_task.take();
                match result {
                    Ok(()) => card.imported = true,
                    Err(error) => log::warn!("catalogue import failed: {error}"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(crate) fn prepare(&mut self, cx: &mut Context<Self>) {
        self.cover.update(cx, |cover, cx| cover.prepare(cx));
    }

    pub(crate) fn is_prepared(&self, cx: &gpui::App) -> bool {
        self.cover.read(cx).settled
    }
}

struct StoreCoverView {
    source_id: CatalogSourceId,
    image_url: Option<String>,
    initial: SharedString,
    services: StoreBookServices,
    card: WeakEntity<StoreBookCardView>,
    element_id: SharedString,
    import_request: Option<CatalogImportRequest>,
    image: Option<LoadedCover>,
    task: Option<Task<()>>,
    attempted: bool,
    settled: bool,
    retries: u8,
}

impl StoreCoverView {
    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(image_url) = self.image_url.clone() else {
            self.settled = true;
            return;
        };
        let operation = self.services.load_cover(self.source_id.clone(), image_url);
        let decoder = cx.background_executor().clone();
        let svg_renderer = cx.svg_renderer();
        self.task = Some(cx.spawn(async move |cover, cx| {
            let result = match operation.await {
                Ok(bytes) => Ok(decoder.spawn(async move { loaded_cover_from_jpeg(bytes, svg_renderer) }).await),
                Err(error) => Err(error),
            };
            let _ = cover.update(cx, |cover, cx| {
                cover.task.take();
                cover.settled = true;
                match result {
                    Ok(decoded) => match decoded {
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

    fn prepare(&mut self, cx: &mut Context<Self>) {
        if !self.attempted {
            self.attempted = true;
            self.load(cx);
        }
    }
}

impl Render for StoreCoverView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let cover = if let Some(cover) = self.image.clone() { components::book_cover(theme).child(components::book_cover_image(cover.image, theme)) } else { components::missing_book_cover(self.initial.clone(), theme) };
        cover.id(self.element_id.clone()).when(self.import_request.is_none(), |cover| cover.cursor_default()).when_some(self.import_request.clone(), |cover, request| {
            let card = self.card.clone();
            cover.on_click(move |_, _, cx| {
                let _ = card.update(cx, |card, cx| card.import(request.clone(), cx));
            })
        })
    }
}

impl Render for StoreBookCardView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let title = self.presentation.title.clone();
        let authors = self.presentation.authors.clone();
        let import_request = (self.import_task.is_none() && !self.imported).then(|| self.import_request.clone()).flatten();

        let identity_text = components::book_card_text().child(components::book_card_title().child(title.clone())).child(components::book_card_author(theme).child(authors.clone()));
        let cover = components::book_cover_frame().id(format!("{}/cover", self.presentation.element_id)).child(self.cover.clone()).when(import_request.is_none(), |cover| cover.cursor_default());
        components::book_card(self.presentation.element_id.clone()).child(cover).child(identity_text).into_any_element()
    }
}
