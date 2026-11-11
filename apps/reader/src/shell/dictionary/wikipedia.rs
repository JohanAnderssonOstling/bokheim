//! Full articles keep their own reading surface and image working set.
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    io,
    rc::Rc,
    sync::Arc,
};

use futures_lite::future::zip;
use gpui::prelude::*;
use gpui::{AnyElement, App, BackgroundExecutor, Context, Entity, FocusHandle, IntoElement, KeyDownEvent, Subscription, Task, Window, div, px};
use gpui_component::{FocusTrapElement, Theme};
use book_model::BookTocEntry;
use content_address::{BookId, hash_reader};
use html_dom::{Document, ImageSource, NodeRef};
use html_parse::{StylesheetReference, document_stylesheets};
use html::ParsedDocumentSource;
use html::style::StylesheetCache;
use html_style::ResourceStylesheetEntry;
use html_view_gpui::ChapterPreparationWorker;
use library_backend::LibraryClient;
use library_model::BookLocator;
use skrift_column_paginator::RendererPreparation;
use html_resources::{ImageLoadCompletion, ImageLoader, ImageResourceContext, ResourceMetadata, ResourceProvider, load_image_bytes_async};
use language_tags::LanguageTag;
use reader_lookups::{WikipediaArticle, WikipediaChoice, LookupError, parse_language_tag};
use percent_encoding::percent_decode_str;
use ui_components as components;
use ui_text::text;
use url::Url;

use super::DictionaryPopup;
use crate::shell::history::NavigationHistory;
use crate::{
    book::NativeBook,
    epub::{DocumentNavigation, ReaderView},
};

const IMAGE_CONCURRENCY: usize = 4;
const STYLESHEET_CONCURRENCY: usize = 4;
const MAX_RETAINED_DOCUMENTS: usize = 2;
const MAX_CACHED_ARTICLES: usize = 4;

pub(super) struct Equivalents {
    pub(super) targets: Vec<LanguageTag>,
    source: Option<String>,
    result: Option<Result<Vec<WikipediaChoice>, LookupError>>,
    _request: Task<()>,
}

impl Default for Equivalents {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl Equivalents {
    pub(super) fn new(targets: Vec<LanguageTag>) -> Self {
        Self { targets, source: None, result: None, _request: Task::ready(()) }
    }


    pub(super) fn is_loading(&self) -> bool {
        self.source.is_some() && self.result.is_none()
    }

    pub(super) fn render(&self, cx: &mut Context<DictionaryPopup>) -> Option<AnyElement> {
        self.source.as_ref()?;
        let mut body = div().flex().flex_col().gap(px(8.0));
        match &self.result {
            None => return None,
            Some(Err(error)) => {
                body = body.child(div().flex().items_center().gap(px(8.0)).child(components::user_facing_error(error, text!(reader::wikipedia_unavailable))).child(components::outlined_text_button("wikipedia-equivalents-retry", text!(common::retry)).on_click(cx.listener(
                    |popup, _, _, cx| {
                        popup.wikipedia_equivalents.source = None;
                        popup.refresh_wikipedia_equivalents(cx);
                    },
                ))));
            }
            Some(Ok(links)) => {
                if links.is_empty() {
                    return None;
                }
                for target in &self.targets {
                    let Some(link) = links.iter().find(|link| &link.language == target) else { continue };
                    let label = components::LANGUAGE_NAMES
                        .iter()
                        .find(|(code, _)| code.eq_ignore_ascii_case(target.as_str()))
                        .or_else(|| components::LANGUAGE_NAMES.iter().find(|(code, _)| *code == target.primary_language()))
                        .map_or_else(|| target.to_string(), |(_, name)| (*name).to_owned());
                    let article = WikipediaArticle { title: link.title.clone(), extract: String::new(), url: link.url.clone(), language: link.language.clone() };
                    let row = div().flex().items_start().gap(px(8.0))
                        .when(self.targets.len() > 1, |row| row.child(div().flex_none().max_w(px(100.0)).text_sm().text_color(components::browser_theme(cx).text_muted).child(label)))
                        .child(components::outlined_text_button(format!("wikipedia-equivalent-{target}"), link.title.clone()).ghost().no_fill().flex_1().min_w_0().h_auto().whitespace_normal().justify_start().on_click(cx.listener(move |popup, _, window, cx| {
                            popup.open_wikipedia_article(article.clone(), window, cx);
                        })));
                    body = body.child(row);
                }
            }
        }
        Some(body.into_any_element())
    }
}

impl DictionaryPopup {
    pub(super) fn refresh_wikipedia_equivalents(&mut self, cx: &mut Context<Self>) {
        let Some(article) = self.visible_wikipedia().cloned() else { return };
        let equivalents = &mut self.wikipedia_equivalents;
        if equivalents.targets.is_empty() || equivalents.source.as_deref() == Some(&article.url) {
            return;
        }
        equivalents.source = Some(article.url.clone());
        equivalents.result = None;
        let targets = equivalents.targets.clone();
        let lookups = super::super::lookups::client(cx);
        equivalents._request = cx.spawn(async move |this, cx| {
            let result = lookups.wikipedia_equivalents(article.clone(), targets).await;
            let _ = this.update(cx, |popup, cx| {
                if popup.wikipedia_equivalents.source.as_deref() == Some(&article.url) {
                    let empty = result.as_ref().is_ok_and(Vec::is_empty);
                    popup.wikipedia_equivalents.result = Some(result);
                    if empty && popup.translation.is_some() {
                        // A translation result needs an equivalent in at least one
                        // target language. Keep checking the remaining search hits.
                        popup._wikipedia_task = Task::ready(());
                        popup.wikipedia_loading = false;
                        popup.wikipedia_failed = None;
                        popup.wikipedia = None;
                        if popup.wikipedia_index < popup.wikipedia_hits.len() {
                            popup.wikipedia_hits.remove(popup.wikipedia_index);
                        }
                        popup.wikipedia_index = popup.wikipedia_index.min(popup.wikipedia_hits.len().saturating_sub(1));
                        popup.popup_height = None;
                        popup.wikipedia_hit(popup.wikipedia_index, cx);
                    }
                    if !popup.showing_translation() {
                        popup.popup_height = None;
                    }
                    cx.notify();
                }
            });
        });
    }
}

#[derive(Default)]
enum ArticleLoad {
    #[default]
    Idle,
    Loading,
    Failed(ArticleRequest, LookupError),
}

#[derive(Clone)]
struct ArticleLocation {
    article: WikipediaArticle,
    cfi: Option<String>,
}

impl PartialEq for ArticleLocation {
    fn eq(&self, other: &Self) -> bool {
        self.article.url == other.article.url && self.cfi == other.cfi
    }
}

#[derive(Clone)]
struct ArticleRequest {
    location: ArticleLocation,
    fragment: Option<String>,
    history: NavigationHistory<ArticleLocation>,
}

struct RetainedArticle {
    uri: String,
    prepared: RendererPreparation,
}

struct CachedArticle {
    article: WikipediaArticle,
    book: Arc<NativeBook>,
    aliases: Vec<String>,
    images: BTreeMap<String, Arc<[u8]>>,
}

impl CachedArticle {
    fn matches(&self, uri: &str) -> bool {
        self.article.url == uri || self.aliases.iter().any(|alias| alias == uri)
    }
}

#[derive(Default)]
pub(super) struct ArticlePage {
    current: Option<(WikipediaArticle, Entity<ReaderView>)>,
    cache: VecDeque<CachedArticle>,
    stylesheet_cache: StylesheetCache,
    documents: VecDeque<RetainedArticle>,
    pub(super) context: Option<(Rc<LibraryClient>, BookLocator)>,
    pub(super) reuse_reader: bool,
    standalone: bool,
    history: NavigationHistory<ArticleLocation>,
    load: ArticleLoad,
    generation: u64,
    image_requests: Option<async_channel::Receiver<ImageRequest>>,
    _image_workers: Vec<Task<()>>,
    _request: Option<Task<()>>,
    _navigation: Option<Subscription>,
}

impl ArticlePage {
    pub(super) fn close(&mut self) {
        self.current = None;
        self.documents.clear();
        self.cancel_request();
        self._navigation = None;
        self.history = NavigationHistory::default();
        self.stop_images();
    }

    fn cancel_request(&mut self) {
        self._request = None;
        self.load = ArticleLoad::Idle;
        self.generation = self.generation.wrapping_add(1);
    }

    fn retain_document(&mut self, uri: String, prepared: RendererPreparation) {
        self.documents.retain(|entry| entry.uri != uri && self.cache.iter().any(|cached| cached.article.url == entry.uri));
        if !self.cache.iter().any(|cached| cached.article.url == uri) {
            return;
        }
        while self.documents.len() >= MAX_RETAINED_DOCUMENTS {
            self.documents.pop_front();
        }
        self.documents.push_back(RetainedArticle { uri, prepared });
    }

    fn cached_article(&mut self, uri: &str) -> Option<&CachedArticle> {
        let index = self.cache.iter().position(|entry| entry.matches(uri))?;
        let entry = self.cache.remove(index)?;
        self.cache.push_back(entry);
        self.cache.back()
    }

    fn cache_article(&mut self, mut entry: CachedArticle) {
        if let Some(index) = self.cache.iter().position(|cached| cached.article.url == entry.article.url) {
            let previous = self.cache.remove(index).expect("cached article index");
            entry.images = previous.images;
            for alias in previous.aliases {
                if !entry.matches(&alias) {
                    entry.aliases.push(alias);
                }
            }
        }
        while self.cache.len() >= MAX_CACHED_ARTICLES {
            self.cache.pop_front();
        }
        self.cache.push_back(entry);
        self.documents.retain(|entry| self.cache.iter().any(|cached| cached.article.url == entry.uri));
    }

    fn stop_images(&mut self) {
        self._image_workers.clear();
        if let Some(requests) = self.image_requests.take() {
            while requests.try_recv().is_ok() {}
        }
    }

    fn location(&self, cx: &App) -> Option<ArticleLocation> {
        let (article, reader) = self.current.as_ref()?;
        let cfi = reader.read(cx).current_cfi(cx);
        Some(ArticleLocation { article: article.clone(), cfi })
    }

    pub(super) fn render(&self, focus: &FocusHandle, window: &Window, cx: &mut Context<DictionaryPopup>) -> Option<AnyElement> {
        if !self.standalone && self.current.is_none() && !matches!(self.load, ArticleLoad::Failed(..)) {
            return None;
        }
        let theme = components::browser_theme(cx);
        let body = self.current.as_ref().map(|(_, reader)| reader.clone());
        let status = match &self.load {
            ArticleLoad::Idle | ArticleLoad::Loading => None,
            ArticleLoad::Failed(request, error) => {
                let request = request.clone();
                Some(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .p(px(components::SPACE_MD))
                        .child(components::user_facing_error(error, text!(reader::wikipedia_unavailable)))
                        .child(components::outlined_text_button("wikipedia-article-retry", text!(common::retry)).on_click(cx.listener(move |popup, _, window, cx| popup.load_wikipedia_article(request.clone(), window, cx))))
                        .into_any_element(),
                )
            }
        };
        let panel = components::modal_surface(theme)
            .id("wikipedia-article-surface")
            .font_family(Theme::global(cx).font_family.clone())
            .text_size(Theme::global(cx).font_size)
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_click(|_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .track_focus(focus)
            .on_key_down(cx.listener(|popup, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    popup.close_wikipedia_article(window, cx);
                    cx.stop_propagation();
                }
            }))
            .children(status)
            .child(div().size_full().flex_1().min_h_0().min_w_0().children(body))
            .focus_trap("wikipedia-article-focus", focus);
        Some(gpui::deferred(components::anchor_to_window(components::absolute_full_size().child(panel), window)).into_any_element())
    }
}

impl DictionaryPopup {
    pub(crate) fn with_reader(mut self, library: Rc<LibraryClient>, locator: BookLocator, reuse_reader: bool) -> Self {
        self.wikipedia_page.context = Some((library, locator));
        self.wikipedia_page.reuse_reader = reuse_reader;
        self
    }

    fn close_wikipedia_article(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.wikipedia_page.standalone {
            window.remove_window();
            return;
        }
        self.wikipedia_page.close();
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn open_wikipedia_article(&mut self, article: WikipediaArticle, window: &mut Window, cx: &mut Context<Self>) {
        if crate::windows::native_windows_available() && !self.wikipedia_page.reuse_reader {
            if let Some((library, locator)) = self.wikipedia_page.context.clone() {
                match Self::open_article_window(article, None, library, locator, window, cx) {
                    Ok(()) => cx.emit(super::Closed),
                    Err(error) => log::error!("could not open Wikipedia window: {error}"),
                }
                return;
            }
        }
        if self.wikipedia_page.reuse_reader {
            cx.emit(article);
            return;
        }
        self.wikipedia_page.close();
        self.focus.focus(window, cx);
        self.load_wikipedia_article(ArticleRequest { location: ArticleLocation { article, cfi: None }, fragment: None, history: NavigationHistory::default() }, window, cx);
    }

    pub(crate) fn open_article_window(article: WikipediaArticle, fragment: Option<String>, library: Rc<LibraryClient>, locator: BookLocator, parent: &Window, cx: &mut App) -> Result<(), String> {
        crate::windows::open_reader_window(article.title.clone(), parent, cx, move |window, cx| {
            cx.new(|cx| {
                let mut popup = Self::empty(article.title.clone(), vec![article.language.to_string()], None, window, cx).with_reader(library, locator, false);
                popup.wikipedia_page.standalone = true;
                popup.load_wikipedia_article(ArticleRequest { location: ArticleLocation { article, cfi: None }, fragment, history: NavigationHistory::default() }, window, cx);
                popup
            })
        })
    }

    pub(crate) fn native_back(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(popup) = self.wikipedia_popup.clone() {
            window.defer(cx, move |window, cx| {
                popup.update(cx, |popup, cx| {
                    if !popup.native_back(window, cx) {
                        cx.emit(super::Closed);
                    }
                })
            });
            return true;
        }
        if self.wikipedia_page.current.is_some() || !matches!(self.wikipedia_page.load, ArticleLoad::Idle) {
            if let Some((_, reader)) = self.wikipedia_page.current.as_ref() {
                let reader = reader.clone();
                window.defer(cx, move |window, cx| reader.update(cx, |reader, cx| reader.native_back(window, cx)));
            } else {
                self.close_wikipedia_article(window, cx);
            }
            return true;
        }
        if self.settings.is_some() {
            self.close_settings(window, cx);
            return true;
        }
        false
    }

    fn handle_article_navigation(&mut self, event: &DocumentNavigation, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self.wikipedia_page.location(cx) else { return };
        match event {
            DocumentNavigation::Link(href) => {
                if let Some((article, fragment)) = article_link(href) {
                    let article = self.wikipedia_page.cache.iter().find(|entry| entry.matches(&article.url)).map(|entry| entry.article.clone()).unwrap_or(article);
                    if article.url == current.article.url {
                        if let Some((_, reader)) = self.wikipedia_page.current.as_ref() {
                            let href = format!("{}{}", article.url, fragment.as_deref().unwrap_or_default());
                            reader.update(cx, |reader, cx| reader.navigate_to(href, cx));
                        }
                        return;
                    }
                    let mut history = self.wikipedia_page.history.clone();
                    history.begin_jump(current);
                    self.load_wikipedia_article(ArticleRequest { location: ArticleLocation { article, cfi: None }, fragment, history }, window, cx);
                } else if let Ok(url) = Url::parse(href) {
                    if matches!(url.scheme(), "https" | "http" | "mailto" | "tel") {
                        cx.open_url(url.as_str());
                    }
                }
            }
            DocumentNavigation::History(forward) => {
                let mut history = self.wikipedia_page.history.clone();
                if let Some(location) = history.navigate(current.clone(), *forward) {
                    if location.article.url == current.article.url
                        && let Some(cfi) = location.cfi.clone()
                        && let Some((_, reader)) = self.wikipedia_page.current.as_ref()
                    {
                        let reader = reader.clone();
                        self.wikipedia_page.cancel_request();
                        self.wikipedia_page.history = history;
                        let availability = self.wikipedia_page.history.availability();
                        reader.update(cx, |reader, cx| {
                            reader.set_document_history(availability, cx);
                            reader.restore_document_position(cfi, window, cx);
                        });
                        cx.notify();
                        return;
                    }
                    self.load_wikipedia_article(ArticleRequest { location, fragment: None, history }, window, cx);
                }
            }
            DocumentNavigation::BeginJump(from) => {
                // A new in-article action supersedes a pending article load.
                self.wikipedia_page.cancel_request();
                self.wikipedia_page.history.begin_jump(ArticleLocation { cfi: Some(from.clone()), ..current });
                cx.notify();
            }
            DocumentNavigation::Settled(cfi) => {
                self.wikipedia_page.history.settle(&ArticleLocation { cfi: Some(cfi.clone()), ..current });
                if let Some((_, reader)) = self.wikipedia_page.current.as_ref() {
                    let availability = self.wikipedia_page.history.availability();
                    reader.update(cx, |reader, cx| reader.set_document_history(availability, cx));
                }
            }
        }
    }

    fn load_wikipedia_article(&mut self, mut request: ArticleRequest, window: &mut Window, cx: &mut Context<Self>) {
        let cached = self.wikipedia_page.cached_article(&request.location.article.url).map(|entry| {
            request.location.article = entry.article.clone();
            entry.book.clone()
        });
        self.wikipedia_page.cancel_request();
        self.wikipedia_page.load = ArticleLoad::Loading;
        let generation = self.wikipedia_page.generation;
        let Some((library, locator)) = self.wikipedia_page.context.clone() else {
            self.wikipedia_page.load = ArticleLoad::Failed(request, LookupError::from("Wikipedia reader context is unavailable"));
            cx.notify();
            return;
        };
        let retained = self.wikipedia_page.documents.iter().position(|entry| entry.uri == request.location.article.url).and_then(|index| self.wikipedia_page.documents.remove(index)).filter(|_| cached.is_some());
        let stylesheet_cache = self.wikipedia_page.stylesheet_cache.clone();
        let lookups = super::super::lookups::client(cx);
        let executor = cx.background_executor().clone();
        let preparation_worker = ChapterPreparationWorker::for_app(cx);
        let config = match self.wikipedia_page.current.as_ref() {
            Some((_, reader)) => reader.read(cx).document_config(window, cx),
            _ => crate::epub::state::renderer_initial_config(window, cx),
        };
        self.wikipedia_page._request = Some(cx.spawn_in(window, async move |this, cx| {
            let (entry, source, fetch_error) = match cached.as_ref() {
                Some(_) => (None, None, None),
                None => match load_article(&request.location.article, lookups, executor, stylesheet_cache.clone()).await {
                    Ok((entry, source)) => (Some(entry), Some(source), None),
                    Err(error) => (None, None, Some(error)),
                },
            };
            if let Some(entry) = &entry {
                request.location.article = entry.article.clone();
            }
            let book = cached.or_else(|| entry.as_ref().map(|entry| entry.book.clone()));
            let (sender, requests) = async_channel::unbounded::<ImageRequest>();
            let loader = Arc::new(ImageRequests(sender));
            let prepared = if let Some(retained) = retained {
                Ok(retained.prepared.with_image_loader(loader))
            } else if let Some(book) = book.as_ref() {
                let provider = book.provider.clone();
                let uri = request.location.article.url.clone();
                let cfi = request.location.cfi.clone();
                let fragment = request.fragment.clone();
                preparation_worker
                    .prepare(move || {
                        let source = match source {
                            Some(source) => source,
                            None => {
                                let bytes = provider.read_bytes(&uri).map_err(LookupError::from)?;
                                let html = String::from_utf8(bytes).map_err(|error| LookupError::operation(error))?;
                                article_source(html)
                            }
                        };
                        RendererPreparation::from_parsed_document(provider, uri, source, stylesheet_cache, cfi.as_deref(), fragment.as_deref(), config).map(|prepared| prepared.with_image_loader(loader)).map_err(LookupError::operation)
                    })
                    .recv()
                    .await
                    .unwrap_or_else(|error| Err(LookupError::operation(error)))
            } else {
                Err(fetch_error.unwrap_or_else(|| LookupError::from("Wikipedia article unavailable")))
            };
            let _ = this.update_in(cx, |popup, window, cx| {
                if popup.wikipedia_page.generation != generation {
                    return;
                }
                popup.wikipedia_page.load = ArticleLoad::Idle;
                let (book, prepared) = match (book, prepared) {
                    (Some(book), Ok(prepared)) => (book, prepared),
                    (_, result) => {
                    let error = result.err().unwrap_or_else(|| LookupError::from("Wikipedia article unavailable"));
                    popup.wikipedia_page.load = ArticleLoad::Failed(request, error);
                    cx.notify();
                    return;
                    }
                };
                let config = match popup.wikipedia_page.current.as_ref() {
                    Some((_, reader)) => reader.read(cx).document_config(window, cx),
                    _ => crate::epub::state::renderer_initial_config(window, cx),
                };
                let prepared = match prepared.resume(request.location.cfi.as_deref(), request.fragment.as_deref(), config) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        log::warn!("could not restore Wikipedia article: {error}");
                        popup.wikipedia_page.load = ArticleLoad::Failed(request, LookupError::operation(error));
                        cx.notify();
                        return;
                    }
                };
                let previous_uri = popup.wikipedia_page.current.as_ref().map(|(article, _)| article.url.clone());
                let article = request.location.article.clone();
                let Ok(id) = hash_reader(&mut article.url.as_bytes()).map(BookId::from) else { return };
                let locator = BookLocator::new(*locator.library_id(), id);
                let existing = popup.wikipedia_page.current.as_ref().map(|(_, reader)| reader.clone());
                let (reader, previous) = if let Some(reader) = existing {
                    let result = reader.update(cx, |reader, cx| reader.replace_document(locator, article.title.clone(), book.clone(), prepared, window, cx));
                    let previous = match result {
                        Ok(previous) => previous,
                        Err(error) => {
                            log::warn!("could not display Wikipedia article: {error}");
                            popup.wikipedia_page.load = ArticleLoad::Failed(request, LookupError::operation(error));
                            cx.notify();
                            return;
                        }
                    };
                    (reader, Some(previous))
                } else {
                    let close = cx.weak_entity();
                    let close_reader: crate::CloseReader = Rc::new(move |window, cx| {
                        let _ = close.update(cx, |popup, cx| popup.close_wikipedia_article(window, cx));
                    });
                    let reader = cx.new(|cx| {
                        ReaderView::new(
                            locator,
                            article.title.clone(),
                            library,
                            book.clone(),
                            None,
                            prepared,
                            false,
                            close_reader,
                            #[cfg(feature = "kobo")]
                            None,
                            window,
                            cx,
                        )
                    });
                    popup.wikipedia_page._navigation = Some(cx.subscribe_in(&reader, window, |popup, _, event: &DocumentNavigation, window, cx| popup.handle_article_navigation(event, window, cx)));
                    (reader, None)
                };
                popup.wikipedia_page.history = request.history;
                let availability = popup.wikipedia_page.history.availability();
                reader.update(cx, |reader, cx| reader.set_document_history(availability, cx));
                if let Some(entry) = entry {
                    popup.wikipedia_page.cache_article(entry);
                }
                if let Some((uri, previous)) = previous_uri.zip(previous) {
                    popup.wikipedia_page.retain_document(uri, previous);
                }
                popup.wikipedia_page.stop_images();
                popup.wikipedia_page.image_requests = Some(requests.clone());
                for _ in 0..IMAGE_CONCURRENCY {
                    let requests = requests.clone();
                    let lookups = super::super::lookups::client(cx);
                    let article_uri = article.url.clone();
                    popup.wikipedia_page._image_workers.push(cx.spawn(async move |this, cx| {
                        while let Ok(mut request) = requests.recv().await {
                            let bytes = load_image_bytes_async(&ArticleImages, &request.source, |uri| {
                                let lookups = lookups.clone();
                                let article_uri = article_uri.clone();
                                let this = this.clone();
                                let mut cx = cx.clone();
                                async move {
                                    let cached = this.update(&mut cx, |popup, _| popup.wikipedia_page.cache.iter().find(|entry| entry.article.url == article_uri).and_then(|entry| entry.images.get(&uri)).cloned()).ok().flatten();
                                    if let Some(bytes) = cached {
                                        return Ok(bytes.to_vec());
                                    }
                                    let bytes = lookups.wikipedia_image(uri.clone()).await.map_err(io::Error::other)?;
                                    let bytes: Arc<[u8]> = bytes.into();
                                    let _ = this.update(&mut cx, |popup, _| {
                                        if let Some(entry) = popup.wikipedia_page.cache.iter_mut().find(|entry| entry.article.url == article_uri) {
                                            entry.images.insert(uri, bytes.clone());
                                        }
                                    });
                                    Ok(bytes.to_vec())
                                }
                            })
                            .await;
                            request.finish(bytes);
                        }
                    }));
                }
                popup.wikipedia_page.current = Some((article, reader));
                cx.notify();
            });
        }));
        cx.notify();
    }
}

/// Only article views belong in the reader; website actions keep their browser UI.
pub(crate) fn article_link(href: &str) -> Option<(WikipediaArticle, Option<String>)> {
    let url = Url::parse(href).ok()?;
    if !matches!(url.scheme(), "https" | "http") || url.port().is_some() || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let host = url.host_str()?.strip_suffix(".wikipedia.org")?;
    let language = host.strip_suffix(".m").unwrap_or(host);
    if language == "www" || language.contains('.') {
        return None;
    }
    let language = parse_language_tag(language).filter(|tag| tag.primary_language() == language)?;
    if url.query_pairs().any(|(key, value)| key == "action" && value != "view" || matches!(key.as_ref(), "oldid" | "diff" | "curid" | "veaction")) {
        return None;
    }
    let title = if let Some(title) = url.path().strip_prefix("/wiki/") {
        percent_decode_str(title).decode_utf8().ok()?.into_owned()
    } else if url.path() == "/w/index.php" {
        url.query_pairs().find(|(key, _)| key == "title")?.1.into_owned()
    } else {
        return None;
    };
    let title = title.replace('_', " ");
    if title.trim().is_empty() || title.chars().count() > 255 || title.chars().any(char::is_control) || title.starts_with("Special:") {
        return None;
    }
    let mut canonical = Url::parse(&format!("https://{}.wikipedia.org/wiki/", language.primary_language())).ok()?;
    canonical.path_segments_mut().ok()?.pop_if_empty().push(&title.replace(' ', "_"));
    let fragment = url.fragment().map(|fragment| format!("#{fragment}"));
    Some((WikipediaArticle { title, extract: String::new(), url: canonical.into(), language }, fragment))
}

fn article_source(html: String) -> ParsedDocumentSource {
    ParsedDocumentSource::html(html)
}

async fn load_article(article: &WikipediaArticle, lookups: reader_lookups::Client, executor: BackgroundExecutor, stylesheet_cache: StylesheetCache) -> Result<(CachedArticle, ParsedDocumentSource), LookupError> {
    let mut html = lookups.wikipedia_html(article.title.clone(), article.language.clone()).await?;
    // Share the bundled defaults and their compilation across article loads.
    html.push_str(concat!("<style>", include_str!("wikipedia.css"), "</style>"));
    let uri = article.url.clone();
    let requested_article = article.clone();
    let discovery_cache = stylesheet_cache.clone();
    let (html, source, canonical, mut pending, toc) = executor
        .spawn(async move {
            let source = article_source(html.clone());
            let document = source.document();
            // Redirected articles link to their canonical title, not the requested alias.
            let canonical = document
                .node_ids()
                .filter_map(|node| document.try_element_ref(node))
                .filter(|element| element.tag().eq_ignore_ascii_case("link"))
                .filter(|element| element.attr("rel").is_some_and(|rel| rel.split_ascii_whitespace().any(|rel| matches!(rel, "canonical" | "dc:isVersionOf"))))
                .find_map(|element| article_link(&resolve(&uri, element.attr("href")?)).map(|(article, _)| article))
                .unwrap_or(requested_article);
            let uri = &canonical.url;
            let mut pending = VecDeque::new();
            for (sheet, _) in document_stylesheets(document) {
                match sheet {
                    StylesheetReference::Inline(css) => add_imports(&mut pending, uri, &css, &discovery_cache),
                    StylesheetReference::External(href) | StylesheetReference::ExternalWithCharset { href, .. } => pending.push_back(resolve(uri, &href)),
                }
            }
            let toc = article_toc(document, uri);
            (html, source, canonical, pending, toc)
        })
        .await;
    let uri = canonical.url.clone();
    let mut resources = ArticleResources { document: uri.clone(), language: canonical.language.to_string(), bytes: BTreeMap::new() };
    let mut visited = HashSet::new();
    let mut total_bytes = 0;
    'stylesheets: while !pending.is_empty() && visited.len() < 16 {
        let mut batch: [Option<String>; STYLESHEET_CONCURRENCY] = Default::default();
        for slot in &mut batch {
            while visited.len() < 16 {
                let Some(uri) = pending.pop_front() else { break };
                if visited.insert(uri.clone()) {
                    *slot = Some(uri);
                    break;
                }
            }
        }
        let [a, b, c, d] = batch.map(|uri| {
            let client = lookups.clone();
            async move {
                let uri = uri?;
                let bytes = match client.wikipedia_stylesheet(uri.clone()).await {
                    Ok(bytes) => bytes,
                    Err(error) => { log::warn!("Wikipedia stylesheet unavailable: {error}"); return None; }
                };
                Some((uri, bytes))
            }
        });
        // Poll the four requests together, including on browser targets where
        // network futures must stay on the calling thread.
        let ((a, b), (c, d)) = zip(zip(a, b), zip(c, d)).await;
        for (uri, bytes) in [a, b, c, d].into_iter().flatten() {
            total_bytes += bytes.len();
            if total_bytes > 4 * 1024 * 1024 {
                break 'stylesheets;
            }
            let base = uri.clone();
            let import_cache = stylesheet_cache.clone();
            let (bytes, imports) = executor
                .spawn(async move {
                    let mut imports = VecDeque::new();
                    add_imports(&mut imports, &base, &String::from_utf8_lossy(&bytes), &import_cache);
                    (bytes, imports)
                })
                .await;
            pending.extend(imports);
            resources.bytes.insert(uri, bytes);
        }
    }
    let weight = html.len() as u64;
    resources.bytes.insert(uri.clone(), html.into_bytes().into());
    let book = Arc::new(NativeBook::from_document(Arc::new(resources), uri, toc, weight));
    let aliases = (canonical.url != article.url).then(|| article.url.clone()).into_iter().collect();
    Ok((CachedArticle { article: canonical, book, aliases, images: BTreeMap::new() }, source))
}

fn add_imports(pending: &mut VecDeque<String>, base: &str, css: &str, cache: &StylesheetCache) {
    for entry in cache.resource_entries(css) {
        if let ResourceStylesheetEntry::Import(import) = entry {
            pending.push_back(resolve(base, import.url()));
        }
    }
}

fn article_toc(document: &Document, base: &str) -> Vec<BookTocEntry> {
    fn text(document: &Document, node: html_dom::DomNodeId, output: &mut String) {
        match document.node_ref(node) {
            NodeRef::Text(value) => output.push_str(value.text()),
            NodeRef::Element(element) => {
                if !element.attr("class").is_some_and(|classes| classes.split_ascii_whitespace().any(|class| class == "mw-editsection")) {
                    element.children().for_each(|child| text(document, child, output));
                }
            }
        }
    }
    let mut pending = document.dom_root().into_iter().collect::<Vec<_>>();
    let headings = std::iter::from_fn(|| {
        let node = pending.pop()?;
        if let Some(element) = document.try_element_ref(node) {
            pending.extend(element.children().rev());
        }
        Some(node)
    })
        .filter_map(|node| {
            let element = document.try_element_ref(node)?;
            if !matches!(element.tag(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
                return None;
            }
            let anchor = element.attr("id").or_else(|| element.children().find_map(|child| document.try_element_ref(child)?.attr("id")))?;
            let mut title = String::new();
            text(document, node, &mut title);
            let title = title.trim().to_owned();
            if title.is_empty() {
                return None;
            }
            Some((element.tag().as_bytes()[1] - b'0', BookTocEntry { title, target: format!("{base}#{anchor}"), children: Vec::new() }))
        });
    fn finish_heading(stack: &mut Vec<(u8, BookTocEntry)>, roots: &mut Vec<BookTocEntry>) {
        let Some((_, entry)) = stack.pop() else { return };
        if let Some((_, parent)) = stack.last_mut() {
            parent.children.push(entry);
        } else {
            roots.push(entry);
        }
    }
    let mut roots = Vec::new();
    let mut stack = Vec::new();
    for (level, entry) in headings {
        // The nearest preceding lower-level heading is the parent, even if
        // the article skips levels (for example, h2 followed by h4).
        while stack.last().is_some_and(|(parent_level, _)| *parent_level >= level) {
            finish_heading(&mut stack, &mut roots);
        }
        stack.push((level, entry));
    }
    while !stack.is_empty() {
        finish_heading(&mut stack, &mut roots);
    }
    roots
}

struct ImageRequest {
    source: ImageSource,
    complete: Option<ImageLoadCompletion>,
}

impl ImageRequest {
    fn finish(&mut self, result: io::Result<Vec<u8>>) {
        if let Some(complete) = self.complete.take() {
            complete(result);
        }
    }
}

impl Drop for ImageRequest {
    fn drop(&mut self) {
        if self.complete.is_some() {
            self.finish(Err(io::Error::new(io::ErrorKind::Interrupted, "article closed")));
        }
    }
}

struct ImageRequests(async_channel::Sender<ImageRequest>);

impl ImageLoader for ImageRequests {
    fn load(&self, source: ImageSource, complete: ImageLoadCompletion) {
        let _ = self.0.try_send(ImageRequest { source, complete: Some(complete) });
    }
}

struct ArticleResources {
    document: String,
    language: String,
    bytes: BTreeMap<String, Arc<[u8]>>,
}

fn resolve(base: &str, href: &str) -> String {
    Url::parse(base).and_then(|base| base.join(href)).map_or_else(|_| href.to_owned(), |url| url.into())
}

impl ResourceProvider for ArticleResources {
    fn read_bytes(&self, uri: &str) -> io::Result<Vec<u8>> {
        self.bytes.get(uri).map(|bytes| bytes.to_vec()).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "article resource unavailable"))
    }
    fn metadata(&self, uri: &str) -> io::Result<ResourceMetadata> {
        let media_type = if uri == self.document {
            Some("text/html")
        } else if self.bytes.contains_key(uri) {
            Some("text/css")
        } else {
            None
        };
        Ok(ResourceMetadata { media_type: media_type.map(str::to_owned), charset: None })
    }
    fn primary_language(&self) -> Option<String> {
        Some(self.language.clone())
    }
    fn exists(&self, uri: &str) -> bool {
        self.bytes.contains_key(uri)
    }
    fn resolve(&self, base: &str, href: &str) -> String {
        resolve(base, href)
    }
    fn list_html_candidates(&self, _: &str) -> io::Result<Vec<String>> {
        Ok(vec![self.document.clone()])
    }
}

struct ArticleImages;

impl ImageResourceContext for ArticleImages {
    fn resolve(&self, base: &str, href: &str) -> String {
        resolve(base, href)
    }
    fn metadata(&self, _: &str) -> io::Result<ResourceMetadata> {
        Ok(ResourceMetadata::default())
    }
}
