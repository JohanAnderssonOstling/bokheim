//! One book's page.
//!
//! Its owner keeps the browse view mounted in memory and caches this entity
//! when returning to it, preserving its contents. Reader entry drops the
//! page's cover reference; the shared image budget controls pixel retention.
//!
//! Details, cover art and torrent information load concurrently. The page
//! uses its current pane bounds to reveal the complete book in its final
//! layout rather than moving the title and controls as requests finish.

use ui_components::user_facing_error;

use crate::services::{book_transfer_error_message, notify_error, notify_info, track_catalog_import};
use library_backend::{DownloadState, ThumbnailResolution};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui::prelude::*;
use gpui::{AnyElement, App, AppContext, AvailableSpace, Context, Entity, FocusHandle, Focusable, IntoElement, MouseButton, Render, SharedString, Task, Window, canvas, div};
use gpui_component::{IconName, WindowExt as _};
use gpui_image_eviction::track_image;
use library_model::{BookCardRow, BookDetail, BookLocator, BookSource, BookTocEntry, CardKey, LibraryFileTypeFilter};
use sync_common::ContentHash;
use ui_components::ScrollableElement as _;
use ui_components::{self as components, BrowserDownloadPhase, BrowserDownloadUpdate, BrowserDownloads};

use crate::library::pages::with_retry_banner;
use crate::library::services::{DownloadSubscription, LibraryContext};
use crate::library::widgets::{CoversEvicted, LoadedCover, cover_aspect_ratio, cover_generation, generated_cover_variant, loaded_cover_from_jpeg, mark_feed_book_added};

/// Leaves the book's page for the location it was opened from on Back.
/// Owned by the browse page, which is what knows how to leave the detail location.
pub(super) struct CloseBookDetail;

/// The page's own width — its pane's, not the window's — at which it takes
/// each layout. Below the sidebar breakpoint a column beside the player would
/// be too narrow to read; below the stacked one, so would the title beside
/// the cover.
const BOOK_DETAIL_SIDEBAR_BREAKPOINT: f32 = 950.0;
const BOOK_DETAIL_THREE_COLUMN_BREAKPOINT: f32 = 1200.0;
const BOOK_DETAIL_STACKED_BREAKPOINT: f32 = 600.0;
/// Where the title steps up to its larger size outside the columns.
const BOOK_DETAIL_LARGE_TITLE_WIDTH: f32 = 760.0;

#[derive(Clone, Copy, PartialEq)]
enum DetailLayout {
    /// The player column beside the contents and description, each scrolling
    /// on its own.
    Columns,
    /// One page: the cover beside the title, the player under both at full
    /// width, then the contents and description.
    Hero,
    /// One page, too narrow for anything beside the cover: the cover, then the
    /// title centred under it, the player, the contents and description.
    Stacked,
}

pub(crate) struct BookDetailPage {
    ui: LibraryContext,
    /// What the grid knew, replaced by the request's own copy when it lands.
    /// The card supplies the identity used to request the full details and art.
    book: BookCardRow,
    /// Once revealed, background refreshes keep the book visible.
    ready: bool,
    detail: Option<BookDetail>,
    error: Option<SharedString>,
    cover: Option<LoadedCover>,
    cover_missing: bool,
    cover_needs_reload: bool,
    cover_loading: bool,
    _cover_evictions: gpui::Subscription,
    /// Contents open collapsed, so a book with two hundred entries does not bury
    /// everything under it. This is what the reader has opened since.
    expanded: HashSet<String>,
    /// Whether the contents and the description sections are open, where they
    /// can be folded: in two columns and narrower.
    contents_open: bool,
    description_open: bool,
    focus: FocusHandle,
    /// The load in flight. Replacing it drops the one before, which cancels it.
    request: Option<Task<()>>,
    _cover_task: Option<Task<()>>,
    download_requested_by_page: bool,
    /// The window owns the import so leaving this page does not cancel feedback.
    adding: bool,
    _download_updates: Option<DownloadSubscription>,
    #[cfg(not(target_arch = "wasm32"))]
    torrents: Vec<library_backend::BookTorrent>,
    #[cfg(not(target_arch = "wasm32"))]
    torrents_loading: bool,
    #[cfg(not(target_arch = "wasm32"))]
    torrent_task: Option<Task<()>>,
}

impl gpui::EventEmitter<CloseBookDetail> for BookDetailPage {}

/// A feed book's page: what the feed says of it. A feed knows no contents,
/// credits or reading position.
fn feed_detail(book: BookCardRow, description: String) -> BookDetail {
    BookDetail {
        book,
        external_url: None,
        audiobook_chapter_count: None,
        description,
        year: None,
        source_directory_id: None,
        path: Vec::new(),
        authors: Vec::new(),
        credits: Vec::new(),
        current_entry: None,
        current_target: None,
        toc: Vec::new(),
    }
}

impl BookDetailPage {
    /// The page of a library, catalog or feed book.
    pub(crate) fn create(book: BookCardRow, cover: Option<LoadedCover>, ui: LibraryContext, cx: &mut impl AppContext) -> Entity<Self> {
        cx.new(|cx| {
            if let Some(cover) = &cover {
                track_image(&cover.image, cx);
            }
            let download_updates = Self::download_subscription(&book, &ui, cx);
            let cover_evictions = cx.observe_global::<CoversEvicted>(|page: &mut Self, cx| {
                page._cover_task.take();
                page.cover.take();
                page.cover_needs_reload = true;
                page.cover_missing = false;
                cx.notify();
            });
            let mut page = Self {
                ui,
                book,
                ready: false,
                detail: None,
                error: None,
                cover,
                cover_missing: false,
                cover_needs_reload: false,
                cover_loading: false,
                _cover_evictions: cover_evictions,
                expanded: HashSet::new(),
                contents_open: true,
                description_open: true,
                focus: cx.focus_handle(),
                request: None,
                _cover_task: None,
                download_requested_by_page: false,
                adding: false,
                _download_updates: download_updates,
                #[cfg(not(target_arch = "wasm32"))]
                torrents: Vec::new(),
                #[cfg(not(target_arch = "wasm32"))]
                torrents_loading: false,
                #[cfg(not(target_arch = "wasm32"))]
                torrent_task: None,
            };
            page.load(cx);
            // Detail art must be fetched separately because card thumbnails
            // already carry their crop.
            page.load_cover(cx);
            page
        })
    }

    /// The book's identity, as its card has it.
    pub(super) fn key(&self) -> CardKey {
        self.book.key()
    }

    /// Where the book is in this library; none for a catalog or feed book.
    pub(super) fn locator(&self) -> Option<BookLocator> {
        self.book.library_file().map(|content_hash| BookLocator::new(*self.ui.backend().id(), content_hash))
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        match self.book.source {
            BookSource::ExternalCatalog { entry, .. } => {
                let library = self.ui.backend().clone();
                self.finish_load(async move { library.external_catalog_detail(entry).await }, cx);
            }
            BookSource::Opds { entry, .. } => {
                let app = self.ui.app().clone();
                let book = self.book.clone();
                self.finish_load(async move { app.opds_description(entry).await.map(|description| feed_detail(book, description)) }, cx);
            }
            _ => {
                let Some(content_hash) = self.book.content_hash() else { return };
                self.load_library(content_hash, cx);
            }
        }
        cx.notify();
    }

    fn load_library(&mut self, content_hash: ContentHash, cx: &mut Context<Self>) {
        #[cfg(not(target_arch = "wasm32"))]
        self.load_torrents(content_hash, cx);
        let library = self.ui.backend().clone();
        self.finish_load(async move { library.book_detail(content_hash).await }, cx);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_torrents(&mut self, hash: ContentHash, cx: &mut Context<Self>) {
        let library = self.ui.backend().clone();
        self.torrents_loading = true;
        self.torrent_task = Some(cx.spawn(async move |page, cx| {
            let result = library.book_torrents(hash).await;
            let _ = page.update(cx, |page, cx| {
                page.torrents_loading = false;
                match result {
                    Ok(mut links) => {
                        links.dedup_by(|a, b| a.info_hash == b.info_hash);
                        page.torrents = links;
                    }
                    Err(error) => log::warn!("could not load book torrents: {error}"),
                }
                cx.notify();
            });
        }));
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn change_torrent(&mut self, hash: String, keep: Option<bool>, cx: &mut Context<Self>) {
        let library = self.ui.backend().clone();
        let book = self.book.library_file();
        self.torrent_task = Some(cx.spawn(async move |page, cx| {
            let result = match keep {
                Some(keep) => library.keep_torrent_seeding(hash, keep).await,
                None => library.forget_torrent(hash).await,
            };
            let _ = page.update_in(cx, |page, window, cx| match result {
                Ok(()) => {
                    if let Some(book) = book {
                        page.load_torrents(book, cx);
                    }
                    if keep.is_none() {
                        notify_info("torrent-forgotten", "Torrent removed; downloaded files are kept", window, cx);
                    }
                }
                Err(error) => notify_error("torrent-error", user_facing_error(&error, "Could not update this download. Please try again."), window, cx),
            });
        }));
    }

    fn download_subscription(book: &BookCardRow, ui: &LibraryContext, cx: &mut Context<Self>) -> Option<DownloadSubscription> {
        let hash = book.library_file()?;
        ui.updates().update(cx, |updates, _| updates.track_download(hash, book.downloaded, book.download_requested));
        Some(ui.subscribe_download(hash, cx, |_: &mut Self, _, cx| cx.notify()))
    }

    fn finish_load(&mut self, load: impl std::future::Future<Output = Result<BookDetail, String>> + 'static, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |page, cx| {
            let result = load.await;
            cx.wait_for_update_slot().await;
            let _ = page.update(cx, |page, cx| {
                match result {
                    Ok(detail) => {
                        // A catalog or feed book may already be, or have just
                        // become, a library book.
                        if detail.book.library_file() != page.book.library_file() {
                            page._download_updates = Self::download_subscription(&detail.book, &page.ui, cx);
                        }
                        page.book = detail.book.clone();
                        page.detail = Some(detail);
                        page.error = None;
                    }
                    Err(error) => page.error = Some(user_facing_error(&error, "Could not load book details. Please try again.").into()),
                }
                cx.notify();
            });
        });
        self.request = Some(task);
    }

    /// Adds the catalog or feed book, or an external audiobook from its
    /// source, to the library, then loads the library book's page in its
    /// place, with Read.
    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.adding || matches!(self.book.source, BookSource::ExternalCatalog { downloadable: false, .. }) {
            return;
        }
        let (source, title) = (self.book.source, self.book.title.clone());
        let key = self.book.key();
        let library = self.ui.backend().clone();
        let app = self.ui.app().clone();
        let download = uuid::Uuid::new_v4().to_string();
        let mut downloads = BrowserDownloads::default();
        downloads.apply(BrowserDownloadUpdate { download: download.clone(), replaces: None, name: title.to_string(), phase: BrowserDownloadPhase::Downloading, percent: None, message: None }, window, cx);
        self.adding = true;
        cx.spawn_in(window, async move |page, cx| {
            let operation = async {
                match source {
                    BookSource::Catalog { content_hash, .. } => library.add_catalog_book(sync_common::ROOT_DIR_ID, content_hash).await,
                    // The app fetches from the feed; the library imports the file.
                    BookSource::Opds { source, entry, .. } => match app.download_opds_book(source, entry).await {
                        Ok((file_name, bytes)) => library.import_downloaded_book(sync_common::ROOT_DIR_ID, file_name, bytes).await,
                        Err(error) => Err(error),
                    },
                    BookSource::Library { content_hash } => Ok(content_hash),
                    BookSource::ExternalCatalog { entry, downloadable: true } => library.add_external_catalog_audiobook(sync_common::ROOT_DIR_ID, entry).await,
                    BookSource::ExternalCatalog { downloadable: false, .. } => Err("this book is read on its source website".into()),
                }
            };
            let result = if matches!(source, BookSource::Catalog { .. } | BookSource::ExternalCatalog { .. }) { track_catalog_import(&library, key, &download, &mut downloads, cx, operation).await } else { operation.await };
            let _ = cx.update(|window, cx| {
                let (phase, message) = match &result {
                    Ok(_) => {
                        if matches!(source, BookSource::Opds { .. } | BookSource::ExternalCatalog { .. }) {
                            mark_feed_book_added(key, cx);
                        }
                        (BrowserDownloadPhase::Completed, "Added to library".to_owned())
                    }
                    Err(error) => {
                        log::warn!("Could not add catalog/feed book {key:?} to library: {error}");
                        (BrowserDownloadPhase::Failed, book_transfer_error_message(error).to_owned())
                    }
                };
                downloads.apply(BrowserDownloadUpdate { download, replaces: None, name: title.to_string(), phase, percent: None, message: Some(message) }, window, cx);
            });
            let _ = page.update_in(cx, |page, _, cx| {
                page.adding = false;
                if let Ok(content_hash) = result {
                    page.load_library(content_hash, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Opens the store page that sells the feed book.
    fn buy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let BookSource::Opds { entry, .. } = self.book.source else { return };
        let (app, library_id) = (self.ui.app().clone(), *self.ui.backend().id());
        cx.spawn_in(window, async move |page, cx| {
            if let Err(error) = app.buy_opds_book(entry, library_id).await {
                let _ = page.update_in(cx, |_, window, cx| notify_error("book-buy-error", format!("Could not open the store. {}", user_facing_error(&error, "Please try again.")), window, cx));
            }
        })
        .detach();
    }

    /// The page's own cover, at the resolution a page-sized cover needs rather
    /// than the one a card in a grid does.
    fn load_cover(&mut self, cx: &mut Context<Self>) {
        // With "generated covers only", no art is read or decoded at all.
        if crate::stores::generated_covers_only(cx) {
            self._cover_task = None;
            self.cover = None;
            self.cover_loading = false;
            self.cover_missing = true;
            return;
        }
        self.cover_missing = false;
        self.cover_loading = true;
        let generation = cover_generation(cx);
        let title = self.book.title.clone();
        let library = self.ui.backend().clone();
        let app = self.ui.app().clone();
        let (source, content_hash) = (self.book.source, self.book.content_hash());
        let load = async move {
            match (source, content_hash) {
                (BookSource::Opds { source, entry, .. }, _) => app.opds_cover(source, entry, ThumbnailResolution::HighDensity.width()).await,
                (BookSource::ExternalCatalog { entry, .. }, _) => library.external_catalog_cover(entry, ThumbnailResolution::HighDensity).await,
                (_, Some(content_hash)) => library.thumbnail(content_hash, ThumbnailResolution::HighDensity).await,
                (_, None) => Ok(None),
            }
        };
        let decoder = cx.background_executor().clone();
        self._cover_task = Some(cx.spawn(async move |page, cx| {
            let cover = match load.await {
                Ok(Some(bytes)) => decoder.spawn(async move { loaded_cover_from_jpeg(bytes) }).await.map(Some).ok_or_else(|| "cover is not a readable JPEG".to_owned()),
                Ok(None) => Ok(None),
                Err(error) => Err(error),
            };
            let _ = page.update(cx, |page, cx| {
                if generation != cover_generation(cx) {
                    return;
                }
                page.cover_loading = false;
                match cover {
                    Ok(Some(cover)) => {
                        track_image(&cover.image, cx);
                        page.cover_missing = false;
                        page.cover = Some(cover);
                    }
                    Ok(None) => page.cover_missing = page.cover.is_none(),
                    Err(error) => {
                        page.cover_missing = page.cover.is_none();
                        log::error!("failed to load the cover for book {title:?}: {error}");
                    }
                }
                cx.notify();
            });
        }));
    }

    fn open(&mut self, target: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(content_hash) = self.book.library_file() else { return };
        self.ui.open_book_at(content_hash, self.book.title.clone(), target, window, cx);
    }

    fn download(&mut self, cx: &mut Context<Self>) {
        let Some(content_hash) = self.book.library_file() else { return };
        if !matches!(self.ui.updates().read(cx).download_state(content_hash), Some(DownloadState::NotDownloaded)) || self.download_requested_by_page {
            return;
        }
        self.download_requested_by_page = true;
        let library = self.ui.backend().clone();
        cx.spawn(async move |page, cx| {
            let result = library.download_book(content_hash).await;
            let _ = page.update_in(cx, |page, window, cx| {
                page.download_requested_by_page = false;
                if let Err(error) = result {
                    log::warn!("Could not download library book {content_hash}: {error}");
                    notify_error("book-download-error", book_transfer_error_message(&error), window, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn toggle_entry(&mut self, target: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(target) {
            self.expanded.insert(target.to_owned());
        }
        cx.notify();
    }
}

fn download_action_label(state: &DownloadState) -> SharedString {
    match state {
        DownloadState::Queued => "Waiting to download…".into(),
        DownloadState::Downloading(progress) if progress.value() > 0.0 => format!("Downloading… {:.0}%", progress.value() * 100.0).into(),
        DownloadState::Downloading(_) => "Starting download…".into(),
        DownloadState::Downloaded | DownloadState::NotDownloaded => "Download".into(),
    }
}

/// One entry, flattened for drawing, with the depth it was found at.
struct DrawnEntry<'a> {
    entry: &'a BookTocEntry,
    depth: usize,
}

/// Walks the navigation tree into the list actually drawn: an unopened entry
/// keeps its children to itself.
fn drawn_entries<'a>(entries: &'a [BookTocEntry], expanded: &HashSet<String>, depth: usize, drawn: &mut Vec<DrawnEntry<'a>>) {
    for entry in entries {
        drawn.push(DrawnEntry { entry, depth });
        if expanded.contains(&entry.target) {
            drawn_entries(&entry.children, expanded, depth + 1, drawn);
        }
    }
}

/// "12 h 41 min", for a fact read at a glance rather than a clock read against a
/// playhead — the player's `0:41:05` answers a different question.
fn spoken_duration(duration_ms: u64) -> String {
    let minutes = duration_ms / 60_000;
    let hours = minutes / 60;
    let minutes = minutes % 60;
    match (hours, minutes) {
        // Rounded up rather than down, so a chapter that plays for forty
        // seconds is not described as taking no time at all.
        (0, 0) => "1 min".to_owned(),
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{hours} h"),
        (hours, minutes) => format!("{hours} h {minutes} min"),
    }
}

/// How long each chapter runs, from where the next one starts. An audiobook's
/// navigation stores positions, not lengths, and the last chapter's end is the
/// book's.
///
/// Keyed by target rather than returned in order, because the list that asks is
/// the *drawn* list — flattened, and shorter than the tree whenever anything is
/// collapsed. Position in one is not position in the other.
fn chapter_durations(entries: &[BookTocEntry], total_ms: Option<u64>) -> HashMap<&str, SharedString> {
    let positions = entries.iter().map(|entry| book_model::audiobook_toc_position(&entry.target)).collect::<Vec<_>>();
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let start = positions[index]?;
            let end = match positions.get(index + 1) {
                Some(Some(next)) => *next,
                _ => total_ms?,
            };
            Some((entry.target.as_str(), SharedString::from(spoken_duration(end.saturating_sub(start)))))
        })
        .collect()
}

impl Render for BookDetailPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.cover_needs_reload {
            self.cover_needs_reload = false;
            self.load_cover(cx);
        }
        let page = cx.entity();
        // Build at the actual pane width in this frame. No stored measurement
        // or extra invalidation is needed when a split or window changes size.
        canvas(
            move |bounds, window, cx| {
                let mut body = page.update(cx, |page, cx| page.render_pane(f32::from(bounds.size.width), cx));
                body.prepaint_as_root(bounds.origin, bounds.size.map(AvailableSpace::Definite), window, cx);
                body
            },
            |_, mut body, window, cx| body.paint(window, cx),
        )
        .size_full()
        .min_w_0()
        .min_h_0()
    }
}

impl BookDetailPage {
    fn render_pane(&mut self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let back = (cfg!(feature = "kobo") || !components::has_native_back_button()).then(|| components::book_detail_back("book-detail-back", theme).on_click(cx.listener(|_, _, _, cx| cx.emit(CloseBookDetail))));
        let page = cx.entity();
        let loaded = self.detail.is_some() && !self.cover_loading;
        #[cfg(not(target_arch = "wasm32"))]
        let loaded = loaded && !self.torrents_loading;
        self.ready |= loaded && width > 0.0;
        if !self.ready {
            let label = if self.error.is_some() { "Could not load book details" } else { "Loading book…" };
            let body = components::book_detail_page()
                .track_focus(&self.focus)
                .when_some(back, |body, back| body.child(components::book_detail_measure().child(back)))
                .child(div().flex_1().w_full().flex().items_center().justify_center().child(components::browser_loading_message(label, theme)))
                .into_any_element();
            return with_retry_banner(body, self.error.clone(), "retry-book-detail", theme, move |_, _, cx| page.update(cx, |page, cx| page.load(cx)));
        }

        // The initial requests are complete before the book is displayed.
        let book = self.book.clone();
        let detail = self.detail.clone();

        // Columns only when there is something to put beside the player.
        let has_sections = detail.as_ref().is_some_and(|detail| !detail.toc.is_empty() || !detail.description.trim().is_empty());
        let layout = if width >= BOOK_DETAIL_SIDEBAR_BREAKPOINT && has_sections {
            DetailLayout::Columns
        } else if width >= BOOK_DETAIL_STACKED_BREAKPOINT {
            DetailLayout::Hero
        } else {
            DetailLayout::Stacked
        };
        let beside_cover = layout == DetailLayout::Hero;
        let large_title = !beside_cover || width >= BOOK_DETAIL_LARGE_TITLE_WIDTH;

        let audiobook = book.format_category == LibraryFileTypeFilter::Audiobook;
        // The book that is playing: its controls, and where playback is now.
        // The library's saved position lags behind, so its cover bar and
        // chapter mark follow the player instead.
        let locator = self.locator();
        let playing = cx.try_global::<crate::NowPlaying>().and_then(|now_playing| now_playing.book.as_ref()).filter(|playing| Some(&playing.locator) == locator.as_ref()).map(|playing| (playing.controls.clone(), playing.position.clone()));
        let listening = playing.as_ref().and_then(|(_, position)| position(cx));
        let progress = listening.map(|(_, progress)| progress).unwrap_or(book.progress);

        let cover = (if beside_cover { components::book_detail_cover_slot() } else { components::book_detail_sidebar_cover_slot() })
            .id("book-detail-open-cover")
            .when(book.library_file().is_some(), |cover| cover.cursor_pointer().on_click(cx.listener(|page, _, window, cx| page.open(None, window, cx))))
            .child(match self.cover.clone() {
                Some(cover) => components::book_cover(theme, cover_aspect_ratio(&cover.image)).child(components::book_cover_image(format!("book-detail-cover-{}", book.key()), cover.image, 1.0, false, theme)),
                None if self.cover_missing => components::book_cover_generated(book.title.clone(), book.subtitle.as_deref(), &book.author, generated_cover_variant(book.key()), theme),
                None => components::book_cover(theme, if audiobook { 1.0 } else { components::BOOK_COVER_ASPECT_RATIO }),
            })
            // Show the cover's progress bar only after the book has been started.
            .children(components::book_cover_progress(progress, theme));

        let headline = if beside_cover { components::book_detail_headline() } else { components::book_detail_sidebar_headline() };
        let mut headline = headline.child(components::book_detail_title(book.title.clone(), book.subtitle.as_deref(), large_title, theme));

        // The card knows one author line; the request knows who each of them is
        // and where their page lives. Until it lands the line is stated rather
        // than linked, so the byline neither appears late nor moves when it
        // becomes clickable.
        match detail.as_ref().filter(|detail| !detail.authors.is_empty()) {
            Some(detail) => {
                let mut byline = components::book_detail_authors(theme);
                for author in &detail.authors {
                    let open = crate::OpenAuthor { author: author.id, name: author.name.clone() };
                    byline = byline.child(components::book_detail_name_link(format!("book-detail-author-{}", author.id), author.name.clone(), theme).min_w_0().max_w_full().on_click(move |_, window, cx| {
                        if !window.has_text_selection(cx) {
                            window.dispatch_action(Box::new(open.clone()), cx);
                        }
                    }));
                }
                headline = headline.child(byline);
            }
            None if !book.author.trim().is_empty() => {
                headline = headline.child(components::book_detail_authors(theme).child(div().min_w_0().max_w_full().child(components::book_detail_text("book-detail-author", book.author.clone()))));
            }
            None => {}
        }

        for credit in detail.iter().flat_map(|detail| &detail.credits) {
            let role = match credit.role.as_str() {
                "nrt" => "Narrated by",
                _ => continue,
            };
            headline = headline.child(components::book_detail_credit_line(Some(role), theme).id(format!("book-detail-credit-row-{}-{}", credit.role, credit.id)).child(components::book_detail_name_link(
                format!("book-detail-credit-{}-{}", credit.role, credit.id),
                credit.name.clone(),
                theme,
            )));
        }

        // The year it was published, then how long it runs and how many
        // chapters, for a book where that is a question. Nothing about the
        // file — neither what kind it is nor when it arrived, which are facts
        // about the library rather than about the book. An audiobook with
        // chapters listed heads that list with the last two instead.
        // The year is bare rather than "Published 1998": for an ebook edition
        // it can be the edition's year, not the work's.
        let chapter_list = audiobook && detail.as_ref().is_some_and(|detail| !detail.toc.is_empty());
        let mut facts = Vec::new();
        if let Some(year) = detail.as_ref().and_then(|detail| detail.year) {
            facts.push(SharedString::from(year.to_string()));
        }
        if !chapter_list {
            if let Some(duration) = book.audiobook_duration_ms {
                facts.push(SharedString::from(spoken_duration(duration)));
            }
            if let Some(chapters) = detail.as_ref().and_then(|detail| detail.audiobook_chapter_count) {
                facts.push(SharedString::from(format!("{chapters} chapters")));
            }
        }
        if !facts.is_empty() {
            headline = headline.child(components::book_detail_facts(facts, theme).id("book-detail-metadata"));
        }

        // Where the reader is and what they can do about it, beside the cover
        // rather than under the hero: those two lines are what the page is for,
        // and a reader who opened it to carry on reading should not have to look
        // past the cover to find them.
        // `progress` is already a percentage, as the grid's bar has always read
        // it. Multiplying it by a hundred again put "2201%" beside the title of
        // a book a fifth of the way through.
        let position = book.library_file().and_then(|_| {
            let entry = detail.as_ref().and_then(|detail| detail.current_entry.clone()).filter(|entry| !entry.trim().is_empty()).map(SharedString::from);
            let percentage = (book.progress > 0.0).then(|| SharedString::from(format!("{}%", book.progress.round() as i32)));
            (entry.is_some() || percentage.is_some()).then(|| components::book_detail_position(entry, percentage, theme))
        });

        let download_state = book.library_file().and_then(|content_hash| self.ui.updates().read(cx).download_state(content_hash)).unwrap_or(DownloadState::Queued);
        let downloaded = matches!(download_state, DownloadState::Downloaded);
        let downloading = matches!(download_state, DownloadState::Queued | DownloadState::Downloading(_));
        let open_icon: gpui_component::Icon = if audiobook { components::Headphones.into() } else { IconName::BookOpen.into() };
        let action_padding = if width >= BOOK_DETAIL_LARGE_TITLE_WIDTH { components::SPACE_SM } else { components::SPACE_MD };
        let mut actions = components::book_detail_actions().child(components::primary_action_button("book-detail-open", if audiobook { "Listen" } else { "Read" }, open_icon, false).px(gpui::px(action_padding)).on_click({
            let page = page.clone();
            move |_, window, cx| page.update(cx, |page, cx| page.open(None, window, cx))
        }));
        // The browser streams books it does not hold instead of downloading them.
        if !downloaded && cfg!(not(target_arch = "wasm32")) {
            let label = download_action_label(&download_state);
            actions = actions.child(components::secondary_action_button("book-detail-download", label, IconName::ArrowDown, downloading).px(gpui::px(action_padding)).on_click({
                let page = page.clone();
                move |_, _, cx| page.update(cx, |page, cx| page.download(cx))
            }));
        }
        // Hosted catalog books, open feed books and external audiobooks can
        // be added. Other external entries keep reading on the source website.
        match book.source {
            BookSource::ExternalCatalog { downloadable: false, .. } => {
                let url = detail.as_ref().and_then(|detail| detail.external_url.clone());
                let label = if url.as_deref().is_some_and(|url| url.starts_with("https://litteraturbanken.se/")) { "Open on Litteraturbanken" } else { "Open at source" };
                actions = components::book_detail_actions().child(components::primary_action_button("book-detail-external", label, IconName::ExternalLink, url.is_none()).px(gpui::px(action_padding)).on_click(move |_, _, cx| {
                    if let Some(url) = &url {
                        cx.open_url(url);
                    }
                }));
            }
            BookSource::Library { .. } => {}
            // Sold, not offered: the store's page opens in a store window,
            // and what is bought there goes into the library's root.
            BookSource::Opds { downloadable: false, buyable: true, .. } => {
                actions = components::book_detail_actions().child(components::primary_action_button("book-detail-buy", "Buy", components::buy_glyph(), false).px(gpui::px(action_padding)).on_click({
                    let page = page.clone();
                    move |_, window, cx| page.update(cx, |page, cx| page.buy(window, cx))
                }));
            }
            BookSource::Opds { downloadable: false, .. } => {
                actions = components::book_detail_actions();
                headline = headline.child(components::book_detail_facts(vec![SharedString::from("Not offered for download")], theme).id("book-detail-status"));
            }
            BookSource::Catalog { .. } | BookSource::Opds { downloadable: true, .. } | BookSource::ExternalCatalog { downloadable: true, .. } => {
                actions = components::book_detail_actions().child(
                    components::primary_action_button("book-detail-add", if self.adding { "Adding…" } else { "Add to library" }, components::download_glyph(), self.adding).px(gpui::px(action_padding)).on_click({
                        let page = page.clone();
                        move |_, window, cx| page.update(cx, |page, cx| page.add(window, cx))
                    }),
                );
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        for (index, torrent) in self.torrents.iter().enumerate() {
            let hash = torrent.info_hash.clone();
            let keep = !torrent.keep_seeding;
            let target = page.clone();
            actions = actions.child(
                components::secondary_action_button(("book-torrent-seeding", index), if keep { "Keep seeding torrent" } else { "Stop seeding torrent" }, IconName::ArrowUp, false)
                    .on_click(move |_, _, cx| target.update(cx, |page, cx| page.change_torrent(hash.clone(), Some(keep), cx))),
            );
            let hash = torrent.info_hash.clone();
            let target = page.clone();
            actions = actions
                .child(components::secondary_action_button(("book-torrent-forget", index), "Remove torrent", IconName::Close, false).on_click(move |_, _, cx| target.update(cx, |page, cx| page.change_torrent(hash.clone(), None, cx))));
        }
        // The book that is playing is controlled here rather than offered: its
        // controls stand where Listen and the reading position would. Beside
        // the cover they would get half the row, so there they go under both.
        let player = playing.map(|(controls, _)| controls);
        if player.is_none() {
            headline = headline.children(position).child(actions);
        }
        let player_under_hero = if beside_cover {
            player
        } else {
            headline = headline.children(player);
            None
        };
        if let DownloadState::Downloading(progress) = download_state
            && progress.value() > 0.0
        {
            headline = headline.child(components::progress_track(theme).w(gpui::px(240.0)).max_w_full().child(components::progress_fill(progress.value(), theme)));
        }

        // Three columns show the contents and the description side by side;
        // stacked, in two columns or fewer, either can be folded away.
        let three_columns = layout == DetailLayout::Columns && width >= BOOK_DETAIL_THREE_COLUMN_BREAKPOINT && detail.as_ref().is_some_and(|detail| !detail.toc.is_empty() && !detail.description.trim().is_empty());
        let collapsible = !three_columns;
        // Empty contents and descriptions do not occupy a section.
        let contents = detail.as_ref().filter(|detail| !detail.toc.is_empty()).map(|detail| self.render_contents(detail, audiobook, listening.map(|(position, _)| position), collapsible, theme, cx));
        // A description containing only markup is empty.
        let description = detail.as_ref().and_then(|detail| components::book_detail_description(&detail.description, theme)).map(|description| {
            let open = !collapsible || self.description_open;
            let heading = components::book_detail_section_heading("Description", None::<SharedString>, collapsible.then_some(open), theme).id("book-detail-description-heading").when(collapsible, |heading| {
                heading.on_click(cx.listener(|page, _, window, cx| {
                    if !window.has_text_selection(cx) {
                        page.description_open = !page.description_open;
                        cx.notify();
                    }
                }))
            });
            components::book_detail_section().child(heading).when(open, |section| section.child(description))
        });
        // A description of markup alone has no section after all.
        let three_columns = three_columns && description.is_some();

        // On desktop each column scrolls on its own and the page does not; the
        // narrower pages scroll as one.
        let body = if layout == DetailLayout::Columns {
            let sidebar = components::book_detail_sidebar_column().child(components::book_detail_sidebar().child(components::book_detail_introduction().children(back).child(cover)).child(headline.flex_none()).overflow_y_scrollbar());
            let mut columns = components::book_detail_columns_page().debug_selector(|| "book-detail-columns".into()).track_focus(&self.focus).child(components::book_detail_page_margin()).child(sidebar);
            if three_columns {
                let contents = components::book_detail_column_body().children(contents).overflow_y_scrollbar();
                let description = components::book_detail_column_body().children(description).overflow_y_scrollbar();
                columns = columns
                    .child(components::book_detail_column_gap(theme))
                    .child(components::book_detail_contents_column().child(contents))
                    .child(components::book_detail_column_gap(theme))
                    .child(components::book_detail_description_column().child(description));
            } else {
                let reading = components::book_detail_column_body().children(contents).children(description).overflow_y_scrollbar();
                columns = columns.child(components::book_detail_column_gap(theme)).child(components::book_detail_description_column().child(reading));
            }
            columns.child(components::book_detail_page_margin()).into_any_element()
        } else {
            let introduction = match layout {
                DetailLayout::Stacked => components::book_detail_stacked_introduction().debug_selector(|| "book-detail-stacked".into()).children(back).child(cover).child(headline),
                _ => components::book_detail_introduction().children(back).child(components::book_detail_hero().debug_selector(|| "book-detail-hero".into()).child(cover).child(headline)).children(player_under_hero),
            };
            let content = components::book_detail_measure().child(introduction).child(components::book_detail_sections().children(contents).children(description));
            components::book_detail_page().track_focus(&self.focus).child(content).overflow_y_scrollbar().into_any_element()
        };

        // A failed request loses the credits and the contents —
        // not the book. Everything the card already knew is on screen and stays
        // there, with the banner over it saying what is missing.
        with_retry_banner(body, self.error.clone(), "retry-book-detail", theme, move |_, _, cx| page.update(cx, |page, cx| page.load(cx)))
    }
}

impl BookDetailPage {
    /// Called only for a book that has navigation stored: a page with no
    /// contents has no Contents section at all, rather than a heading over a
    /// sentence explaining the library to the reader.
    /// `listening` is where the player is, when this is the book playing.
    /// `collapsible`: the heading folds the list away, as it does in two
    /// columns and narrower.
    fn render_contents(&self, detail: &BookDetail, audiobook: bool, listening: Option<Duration>, collapsible: bool, theme: components::BrowserTheme, cx: &mut Context<Self>) -> AnyElement {
        let durations = if audiobook { chapter_durations(&detail.toc, detail.book.audiobook_duration_ms) } else { HashMap::new() };
        let mut drawn = Vec::new();
        drawn_entries(&detail.toc, &self.expanded, 0, &mut drawn);
        // Use the full TOC order, including entries hidden under collapsed
        // parents, to decide which visible leaves precede the current entry.
        let mut ordered = Vec::new();
        fn visit<'a>(entries: &'a [BookTocEntry], ordered: &mut Vec<&'a BookTocEntry>) {
            for entry in entries {
                ordered.push(entry);
                visit(&entry.children, ordered);
            }
        }
        visit(&detail.toc, &mut ordered);
        // The entry the reader is inside, and only that one: a parent is not
        // partly read, because a navigation document says where a section
        // starts and never how far into it anyone has got. Matched on the
        // target rather than the wording — two entries can be called the same
        // thing, and `current_entry` is the wording. While the book plays,
        // it is the last chapter starting at or before the player's position.
        let playing_target = listening.and_then(|position| {
            let position = position.as_millis() as u64;
            ordered.iter().filter(|entry| book_model::audiobook_toc_position(&entry.target).is_some_and(|start| start <= position)).last().map(|entry| entry.target.as_str())
        });
        let marked = playing_target.or(detail.current_target.as_deref());
        let read_targets: HashSet<&str> =
            ordered.iter().position(|entry| marked == Some(entry.target.as_str())).map(|current| ordered[..current].iter().filter(|entry| entry.children.is_empty()).map(|entry| entry.target.as_str()).collect()).unwrap_or_default();
        // A closed part holding the current entry carries the mark itself, so
        // where the reader is never disappears into it.
        fn holds(entry: &BookTocEntry, target: &str) -> bool {
            entry.children.iter().any(|child| child.target == target || holds(child, target))
        }

        let page = cx.entity();
        let can_open = self.book.library_file().is_some();
        // Every entry is numbered by its place in reading order, so the number
        // stays put when a part is opened or closed. The caret column is kept
        // only in a contents that has parts at all.
        let number_digits = components::contents_digits(ordered.len());
        let nested = detail.toc.iter().any(|entry| !entry.children.is_empty());
        let mut panel = components::contents_panel(theme);
        for (index, DrawnEntry { entry, depth }) in drawn.into_iter().enumerate() {
            let expanded = self.expanded.contains(&entry.target);
            let row = components::ContentsRow {
                depth,
                has_children: !entry.children.is_empty(),
                expanded,
                marked: marked.is_some_and(|marked| marked == entry.target || (!expanded && holds(entry, marked))),
                read: read_targets.contains(entry.target.as_str()),
                focused: false,
                number: ordered.iter().position(|ordered| std::ptr::eq(*ordered, entry)).map(|position| position + 1),
                number_digits,
            };
            // The chevron opens the entry and the row goes to it, so the two
            // clicks have to be separable. Attach the toggle directly to the
            // disclosure, and leave leaf entries transparent to the row click.
            let disclosure = nested.then(|| {
                components::contents_disclosure(format!("book-detail-disclosure-{index}"), row, theme).when(row.has_children, |disclosure| {
                    let page = page.clone();
                    let target = entry.target.clone();
                    disclosure.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).on_click(move |_, _, cx| page.update(cx, |page, cx| page.toggle_entry(&target, cx)))
                })
            });
            let amount = durations.get(entry.target.as_str()).cloned();
            panel = panel.child(
                components::contents_row(format!("book-detail-entry-{index}"), row, disclosure, components::book_detail_text(format!("book-detail-entry-title-{index}"), entry.title.clone()), amount, theme)
                    .when(can_open, |row| {
                        row.on_click({
                            let page = page.clone();
                            let target = entry.target.clone();
                            move |_, window, cx| {
                                if !window.has_text_selection(cx) {
                                    page.update(cx, |page, cx| page.open(Some(target.clone()), window, cx));
                                }
                            }
                        })
                    })
                    .when(!can_open, |row| row.cursor_default()),
            );
        }
        // An audiobook's heading says how many chapters and how long, on the
        // heading's own line.
        let summary = audiobook.then(|| {
            let chapters = detail.audiobook_chapter_count.unwrap_or(detail.toc.len());
            match detail.book.audiobook_duration_ms {
                Some(duration) => format!("{chapters} chapters · {}", spoken_duration(duration)),
                None => format!("{chapters} chapters"),
            }
        });
        let open = !collapsible || self.contents_open;
        let heading = components::book_detail_section_heading(if audiobook { "Chapters" } else { "Contents" }, summary, collapsible.then_some(open), theme).id("book-detail-contents-heading").when(collapsible, |heading| {
            heading.on_click(cx.listener(|page, _, window, cx| {
                if !window.has_text_selection(cx) {
                    page.contents_open = !page.contents_open;
                    cx.notify();
                }
            }))
        });
        components::book_detail_section().child(heading).when(open, |section| section.child(panel)).into_any_element()
    }
}

impl Focusable for BookDetailPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

#[cfg(all(test, not(target_arch = "wasm32"), not(feature = "kobo")))]
mod tests {
    use super::*;
    use crate::navigation::LibrarySession;
    use gpui::{Pixels, StyleRefinement, TestAppContext, VisualTestContext, px, size};
    use std::{cell::Cell, rc::Rc};

    struct Neighbor(Rc<Cell<usize>>);

    impl Render for Neighbor {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.0.set(self.0.get() + 1);
            div().size_full()
        }
    }

    struct DetailFixture {
        page: Entity<BookDetailPage>,
        neighbor: Entity<Neighbor>,
        width: Pixels,
    }

    impl Render for DetailFixture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().flex().child(div().w(self.width).h_full().flex_none().child(self.page.clone())).child(div().w(px(50.0)).h_full().child(self.neighbor.clone().cached(StyleRefinement::default().size_full())))
        }
    }

    #[gpui::test]
    async fn detail_uses_current_pane_width_without_followup_invalidation(cx: &mut TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_directory, _session, library) = LibrarySession::test_context(cx).await;
        let book = BookCardRow {
            source: BookSource::Library { content_hash: ContentHash::from_bytes([15; 32]) },
            title: "Book".into(),
            subtitle: None,
            author: "Author".into(),
            progress: 0.0,
            downloaded: true,
            download_requested: false,
            format_category: LibraryFileTypeFilter::Book,
            audiobook_duration_ms: None,
            added_at: 0,
            source_directory: None,
        };
        let page = BookDetailPage::create(book.clone(), None, library, cx);
        page.update(cx, |page, _| {
            page.request.take();
            page._cover_task.take();
            page.torrent_task.take();
            page.detail = Some(feed_detail(book, "A description".into()));
            page.cover_loading = false;
            page.cover_missing = true;
            page.torrents_loading = false;
        });
        let notifications = Rc::new(Cell::new(0));
        let observed = notifications.clone();
        let _observation = cx.update(|cx| cx.observe(&page, move |_, _| observed.set(observed.get() + 1)));
        let neighbor_renders = Rc::new(Cell::new(0));
        let neighbor = cx.new(|_| Neighbor(neighbor_renders.clone()));
        let root = cx.open_window(size(px(1500.0), px(800.0)), |_, _| DetailFixture { page, neighbor, width: px(450.0) });
        let mut visual = VisualTestContext::from_window(root.into(), cx);
        for (index, (width, selector)) in [(450.0, "book-detail-stacked"), (800.0, "book-detail-hero"), (1000.0, "book-detail-columns"), (1300.0, "book-detail-columns"), (450.0, "book-detail-stacked")].into_iter().enumerate() {
            if index > 0 {
                root.update(cx, |root, _, cx| {
                    root.width = px(width);
                    cx.notify();
                })
                .unwrap();
            }
            // Opening the window and each notified width change draw a frame
            // immediately. Check it before explicitly drawing another one.
            assert!(visual.debug_bounds(selector).is_some(), "the first frame at {width}px must use the right layout");
            assert_eq!(notifications.get(), 0, "layout must not invalidate the page again");
            let rendered = neighbor_renders.get();
            visual.update(|window, cx| {
                let _ = window.draw(cx);
            });
            assert_eq!(neighbor_renders.get(), rendered, "detail layout must not force a window-wide cache refresh");
        }
    }

    #[gpui::test]
    async fn catalog_detail_subscribes_when_loaded_as_a_library_book(cx: &mut TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_directory, _session, library) = LibrarySession::test_context(cx).await;
        let updates = library.updates();
        let hash = ContentHash::from_bytes([16; 32]);
        let mut book = BookCardRow {
            source: BookSource::Catalog { content_hash: hash },
            title: "Catalog book".into(),
            subtitle: None,
            author: String::new(),
            progress: 0.0,
            downloaded: false,
            download_requested: false,
            format_category: LibraryFileTypeFilter::Book,
            audiobook_duration_ms: None,
            added_at: 0,
            source_directory: None,
        };
        let page = BookDetailPage::create(book.clone(), None, library, cx);
        page.update(cx, |page, cx| {
            page.request.take();
            page._cover_task.take();
            page.torrent_task.take();
            assert!(page._download_updates.is_none());
            book.source = BookSource::Library { content_hash: hash };
            page.finish_load(async move { Ok(feed_detail(book, String::new())) }, cx);
        });
        cx.condition(&page, |page, _| page.book.library_file() == Some(hash)).await;
        let notifications = Rc::new(Cell::new(0));
        let observed = notifications.clone();
        let _observation = cx.update(|cx| cx.observe(&page, move |_, _| observed.set(observed.get() + 1)));
        for state in [DownloadState::Queued, DownloadState::Downloading(library_model::DownloadProgress::ZERO), DownloadState::Downloaded] {
            updates.update(cx, |updates, cx| updates.set_download_state_for_test(hash, state, cx));
        }
        assert_eq!(notifications.get(), 3, "the page must receive progress after its source becomes a library book");
    }
}
