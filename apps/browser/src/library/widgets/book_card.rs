use ui_components::user_facing_error;

use crate::library::pages::browse::profile::Stage;
use crate::services::{BookCommand, book_transfer_error_message, notify_error, notify_info};
use app_preferences::CoverText;
use library_backend::{DownloadState, ThumbnailResolution};
use std::collections::HashSet;
use std::future::Future;
use std::mem::discriminant;
use std::rc::Rc;
use std::sync::{Arc, Weak};
use std::time::Duration;

use web_time::Instant;

/// Long enough to read as a settle rather than a flash, short enough that a
/// fast-filling grid does not feel sluggish.
const COVER_FADE_DURATION: Duration = Duration::from_millis(180);

use gpui::prelude::*;
use gpui::{App, AppContext, Context, DismissEvent, Entity, Focusable as _, IntoElement, Pixels, Point, Render, RenderImage, StyleRefinement, Subscription, Task, Window, div, px};
use gpui_component::IconName;
use gpui_component::menu::PopupMenu;
use library_model::{BookCardRow, BookSource, CardKey};
use ui_components as components;

use crate::library::services::{DownloadSubscription, LibraryContext, LibraryCoversChanged};
use sync_common::{ContentHash, DirId};
use uuid::Uuid;
use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

use super::cover_cache::CoverCache;
use super::cover_frame::retain_visible_cover;
use super::mobile_context_menu;
use gpui_image_eviction::track_image;

#[derive(Clone)]
pub(crate) struct LoadedCover {
    pub image: Arc<RenderImage>,
}

/// Book lookups are weak; the shared 64 MiB image cache retains the pixels.
#[derive(Default)]
struct BrowseCovers {
    cache: CoverCache<(Uuid, CardKey)>,
}

impl gpui::Global for BrowseCovers {}

#[derive(Default)]
pub(crate) struct CoversEvicted(u64);

impl gpui::Global for CoversEvicted {}

pub(crate) fn cover_generation(cx: &App) -> u64 {
    cx.try_global::<CoversEvicted>().map_or(0, |epoch| epoch.0)
}

pub(crate) fn evict_covers(cx: &mut App) {
    let generation = cover_generation(cx).wrapping_add(1);
    cx.set_global(CoversEvicted(generation));
    // Drop lookup state and cancel hidden cover work through CoversEvicted.
    // Pixels stay in the shared budget until pressure makes them evictable.
    cx.default_global::<BrowseCovers>().cache.clear();
}

#[derive(Clone, Copy)]
pub(crate) enum BookPlacement {
    Copy,
    Move,
}

/// What a card asks of the page its grid is on.
pub(crate) enum BookCardEvent {
    /// Show the book's own page. The card hands over its row and its decoded
    /// cover rather than being looked up afterwards, and not only to save the
    /// lookup: anything that went back and read the cards while this one is
    /// being activated would read one that is leased, and panic.
    OpenDetail(BookCardRow, Option<LoadedCover>),
    /// Copy a flat listing's selection while retaining every existing placement.
    CopyToFolder(Vec<ContentHash>),
    /// Copy or move the book out of the folder it is listed in.
    Place(sync_common::ContentHash, DirId, BookPlacement),
    /// Take the book out of the folder it is listed in.
    Remove(sync_common::ContentHash, DirId),
}

pub(crate) type BookCardEvents = Rc<dyn Fn(BookCardEvent, &mut Window, &mut gpui::App)>;

/// A decoded cover's width over its height, for boxing it at its own
/// proportions instead of the placeholder's.
pub(crate) fn cover_aspect_ratio(image: &RenderImage) -> f32 {
    let size = image.size(0);
    size.width.0.max(1) as f32 / size.height.0.max(1) as f32
}

/// Which generated cover a book with no art gets. Its content hash or
/// source-entry identity keeps the cover consistent across cards and detail
/// pages; both are hashes already, so their first bytes are evenly spread.
pub(crate) fn generated_cover_variant(key: CardKey) -> u64 {
    u64::from_le_bytes(key.0[..8].try_into().expect("a card key has 32 bytes"))
}

/// Short enough to fit on a cover while still distinguishing, for example,
/// a seven-hour book from one that runs nearly eight hours.
fn audiobook_card_length(duration_ms: u64) -> String {
    let total_minutes = (duration_ms / 60_000).max(1);
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    match (hours, minutes) {
        (0, minutes) => format!("{minutes} m"),
        (hours, 0) => format!("{hours} h"),
        (hours, minutes) => format!("{hours} h {minutes} m"),
    }
}

pub(crate) fn loaded_cover_from_jpeg(bytes: Vec<u8>) -> Option<LoadedCover> {
    if !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return None;
    }
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::BGRA);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes.as_slice()), options);
    let pixels = decoder.decode().ok()?;
    let (width, height) = decoder.dimensions()?;
    if width == 0 || height == 0 {
        return None;
    }
    let width = u32::try_from(width).ok()?;
    let height = u32::try_from(height).ok()?;
    let frame = image::Frame::new(image::RgbaImage::from_raw(width, height, pixels)?);
    let image = Arc::new(RenderImage::new(vec![frame]));
    Some(LoadedCover { image })
}

#[derive(Clone)]
enum CoverState {
    Pending,
    Loaded(Weak<RenderImage>, ThumbnailResolution),
    Missing,
    Unavailable,
}

impl CoverState {
    fn loaded(&self) -> Option<LoadedCover> {
        match self {
            Self::Loaded(image, _) => image.upgrade().map(|image| LoadedCover { image }),
            Self::Pending | Self::Missing | Self::Unavailable => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BookCardPresentation {
    Grid,
    Section,
    GraphCover,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct BookCardInteraction {
    pub(crate) focused: bool,
    pub(crate) selected: bool,
    pub(crate) marquee: bool,
}

pub(crate) struct BookCardElement {
    card: Entity<BookCardView>,
    presentation: BookCardPresentation,
    interaction: BookCardInteraction,
    _subscription: Subscription,
    #[cfg(test)]
    renders: usize,
}

impl Render for BookCardElement {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        self.card.update(cx, |card, cx| card.render_card(self.presentation, self.interaction, window, cx))
    }
}

/// The feed books added to the library this session. The library does not
/// know which feed entry a file came from, so a feed card learns that its
/// book is held only from an add made here; after a restart it offers Add
/// again, and adding it again finds the same file.
#[derive(Default)]
pub(crate) struct AddedFeedBooks(HashSet<library_model::CardKey>);

impl gpui::Global for AddedFeedBooks {}

pub(crate) fn mark_feed_book_added(key: library_model::CardKey, cx: &mut App) {
    cx.default_global::<AddedFeedBooks>().0.insert(key);
}

fn feed_book_added(key: library_model::CardKey, cx: &App) -> bool {
    cx.try_global::<AddedFeedBooks>().is_some_and(|added| added.0.contains(&key))
}

/// A catalog or feed book added this session. The catalog's next listing
/// leaves an added catalog book out; a feed lists it until the session ends.
fn held(book: &BookCardRow, cx: &App) -> bool {
    match book.source {
        BookSource::Catalog { .. } | BookSource::Opds { .. } | BookSource::ExternalCatalog { downloadable: true, .. } => feed_book_added(book.key(), cx),
        BookSource::Library { .. } => true,
        BookSource::ExternalCatalog { downloadable: false, .. } => false,
    }
}

pub(crate) struct BookCardView {
    book: BookCardRow,
    /// When the artwork arrived, which is what the fade is computed from.
    ///
    /// A timestamp rather than an element animation: the paginator repaints its
    /// children on every cursor move, and an animation keyed to an element
    /// restarts each time that element is rebuilt — which is what made covers
    /// flash. Deriving opacity from *when the cover loaded* is unaffected by how
    /// often the card is drawn.
    cover_loaded_at: Option<Instant>,
    cover: CoverState,
    ui: LibraryContext,
    events: BookCardEvents,
    /// Whether the page offers the folder commands: copying, moving and
    /// removing the book from the folder it is listed in.
    folder_commands: bool,
    copy_from_library: bool,
    cover_task: Option<Task<()>>,
    cover_updates: Option<Subscription>,
    _download_updates: Option<DownloadSubscription>,
    download_task: Option<Task<()>>,
    /// Adding catalog books to the library, while it runs.
    add_task: Option<Task<()>>,
    context_menu: Option<BookCardContextMenu>,
    context_menu_subscription: Option<Subscription>,
    /// Repaints the card when the "cover text" setting changes, so an
    /// already-visible card hides or shows its title without waiting for
    /// some unrelated event to trigger a redraw.
    _cover_text_updates: Subscription,
    /// Starts over when "generated covers only" changes: switching it on
    /// drops the art, switching it off loads it.
    _generated_covers_updates: Subscription,
    _cover_evictions: Subscription,
    /// Repaints a feed card when a feed book is added, to mark it held.
    _feed_added_updates: Option<Subscription>,
}

struct BookCardContextMenu {
    menu: Entity<PopupMenu>,
    position: Point<Pixels>,
}

impl BookCardView {
    pub(crate) fn title(&self) -> &str {
        &self.book.title
    }
    pub(crate) fn thumbnail_resolution(window: &Window) -> ThumbnailResolution {
        let logical_width = gpui::px(components::BOOK_CARD_CONTENT_WIDTH);
        let physical_width = f32::from(logical_width) * window.scale_factor();
        if physical_width <= ThumbnailResolution::Browse.width() as f32 { ThumbnailResolution::Browse } else { ThumbnailResolution::CardHighDensity }
    }

    /// A card in a browse grid, which is what gives it somewhere to navigate to.
    pub(crate) fn create_in_grid(row: BookCardRow, ui: LibraryContext, events: BookCardEvents, folder_commands: bool, copy_from_library: bool, cx: &mut impl AppContext) -> Entity<Self> {
        cx.new(|cx| Self::new(row, ui, events, folder_commands, copy_from_library, cx))
    }

    fn new(row: BookCardRow, ui: LibraryContext, events: BookCardEvents, folder_commands: bool, copy_from_library: bool, cx: &mut Context<Self>) -> Self {
        // A book without a file has no thumbnail to wait for.
        let cover_updates = row.content_hash().map(|content_hash| Self::subscribe_to_cover_updates(&ui, content_hash, cx));
        // Only library books have a download state; catalog and feed books are added.
        if let Some(content_hash) = row.library_file() {
            ui.updates().update(cx, |updates, _| updates.track_download(content_hash, row.downloaded, row.download_requested));
        }
        let download_updates = Self::subscribe_to_download_updates(&ui, row.library_file(), cx);
        let cover_text_updates = cx.observe_global::<crate::stores::CoverTextSetting>(|_, cx| cx.notify());
        let generated_covers_updates = cx.observe_global::<crate::stores::GeneratedCoversOnly>(|card: &mut Self, cx| {
            card.cover_task.take();
            card.cover = CoverState::Pending;
            cx.notify();
        });
        let cover_evictions = cx.observe_global::<CoversEvicted>(|card: &mut Self, cx| {
            card.cover_task.take();
            card.cover = CoverState::Pending;
            cx.notify();
        });
        let feed_added_updates = matches!(row.source, BookSource::Opds { .. }).then(|| cx.observe_global::<AddedFeedBooks>(|_, cx| cx.notify()));
        Self {
            book: row,
            cover_loaded_at: None,
            cover: CoverState::Pending,
            ui,
            events,
            folder_commands,
            copy_from_library,
            cover_task: None,
            cover_updates,
            _download_updates: download_updates,
            download_task: None,
            add_task: None,
            context_menu: None,
            context_menu_subscription: None,
            _cover_text_updates: cover_text_updates,
            _generated_covers_updates: generated_covers_updates,
            _cover_evictions: cover_evictions,
            _feed_added_updates: feed_added_updates,
        }
    }

    /// The book's file, when it has one.
    pub(crate) fn content_hash(&self) -> Option<ContentHash> {
        self.book.content_hash()
    }

    /// The card's identity in a listing.
    pub(crate) fn key(&self) -> library_model::CardKey {
        self.book.key()
    }

    /// The concrete placement represented by a folder card.
    pub(crate) fn source_directory(&self) -> Option<DirId> {
        self.book.source_directory
    }

    /// Each placement has a cache and interaction state of its own, even when
    /// the same book appears in both a section and the graph's cover rail.
    pub(crate) fn presentation(card: Entity<Self>, presentation: BookCardPresentation, cx: &mut App) -> Entity<BookCardElement> {
        cx.new(|cx| {
            let subscription = cx.observe(&card, |_, _, cx| cx.notify());
            BookCardElement {
                card,
                presentation,
                interaction: BookCardInteraction::default(),
                _subscription: subscription,
                #[cfg(test)]
                renders: 0,
            }
        })
    }

    pub(crate) fn present(view: Entity<BookCardElement>, interaction: BookCardInteraction, window: &Window, cx: &mut App) -> impl IntoElement {
        view.update(cx, |view, _| view.interaction = interaction);
        // Interaction comes from the parent during layout, after GPUI has
        // collected this frame's dirty views. A changed element path busts
        // the cache in this frame without requesting another frame.
        let interaction_key = usize::from(interaction.focused) | (usize::from(interaction.selected) << 1) | (usize::from(interaction.marquee) << 2);
        // rem and display density can change while a cell keeps its bounds.
        let key = format!("card-presentation-{interaction_key}-{}-{}", f32::from(window.rem_size()).to_bits(), window.scale_factor().to_bits());
        div().id(key).size_full().child(view.cached(StyleRefinement::default().size_full()))
    }

    pub(crate) fn replace_row(&mut self, row: BookCardRow, cx: &mut Context<Self>) {
        debug_assert_eq!(self.book.key(), row.key());
        if self.book != row {
            self.book = row;
            cx.notify();
        }
    }

    fn download(&mut self, cx: &mut Context<Self>) {
        let Some(content_hash) = self.book.library_file() else { return };
        if !matches!(self.ui.updates().read(cx).download_state(content_hash), Some(DownloadState::NotDownloaded)) || self.download_task.is_some() {
            return;
        }
        let library = self.ui.backend().clone();
        let operation = async move { library.download_book(content_hash).await };
        self.download_task = Some(cx.spawn(async move |card, cx| {
            let result = operation.await;
            let _ = card.update_in(cx, |card, window, cx| {
                card.download_task.take();
                if let Err(error) = result {
                    log::warn!("Could not download library book {content_hash}: {error}");
                    notify_error("book-download-error", book_transfer_error_message(&error), window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Adds catalog and feed books to the library, and marks them held for the
    /// session. The catalog's next listing then leaves the catalog books out.
    /// Opens the store page that sells a feed book; what is bought there
    /// goes into the library's root.
    fn buy(&mut self, entry: library_model::CardKey, window: &mut Window, cx: &mut Context<Self>) {
        let (app, library_id) = (self.ui.app().clone(), *self.ui.backend().id());
        cx.spawn_in(window, async move |card, cx| {
            if let Err(error) = app.buy_opds_book(entry, library_id).await {
                let _ = card.update_in(cx, |_, window, cx| notify_error("book-buy-error", format!("Could not open the store. {}", user_facing_error(&error, "Please try again.")), window, cx));
            }
        })
        .detach();
    }

    fn add(&mut self, books: Vec<BookCardRow>, cx: &mut Context<Self>) {
        if self.add_task.is_some() {
            return;
        }
        let library = self.ui.backend().clone();
        let app = self.ui.app().clone();
        self.add_task = Some(cx.spawn(async move |card, cx| {
            let mut results = Vec::new();
            for book in books {
                let result = match book.source {
                    BookSource::Catalog { content_hash } => library.add_catalog_book(sync_common::ROOT_DIR_ID, content_hash).await,
                    BookSource::ExternalCatalog { entry, downloadable: true } => library.add_external_catalog_audiobook(sync_common::ROOT_DIR_ID, entry).await,
                    // The app fetches from the feed; the library imports the file.
                    BookSource::Opds { source, entry, downloadable: true, .. } => match app.download_opds_book(source, entry).await {
                        Ok((file_name, bytes)) => library.import_downloaded_book(sync_common::ROOT_DIR_ID, file_name, bytes).await,
                        Err(error) => Err(error),
                    },
                    _ => continue,
                };
                results.push((book.key(), book.title, result));
            }
            let _ = card.update_in(cx, |card, window, cx| {
                card.add_task.take();
                for (key, title, result) in results {
                    match result {
                        Ok(_) => {
                            mark_feed_book_added(key, cx);
                            notify_info("catalog-add", format!("Added “{title}” to the library"), window, cx);
                        }
                        Err(error) => notify_error("catalog-add-error", format!("Could not add “{title}”. {}", user_facing_error(&error, "Please try again.")), window, cx),
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn subscribe_to_cover_updates(ui: &LibraryContext, content_hash: sync_common::ContentHash, cx: &mut Context<Self>) -> Subscription {
        let updates = ui.updates();
        cx.subscribe(&updates, move |view, _, event: &LibraryCoversChanged, cx| {
            if event.contains(&content_hash) {
                view.thumbnail_available(cx);
            }
        })
    }

    fn subscribe_to_download_updates(ui: &LibraryContext, content_hash: Option<ContentHash>, cx: &mut Context<Self>) -> Option<DownloadSubscription> {
        let content_hash = content_hash?;
        let mut previous = ui.updates().read(cx).download_state(content_hash);
        Some(ui.subscribe_download(content_hash, cx, move |_view, state, cx| {
            // A card draws the phase, not the percentage. Detail pages
            // still receive every progress event for their progress bar.
            let changed = previous.as_ref().is_none_or(|previous| discriminant(previous) != discriminant(state));
            previous = Some(state.clone());
            if changed {
                cx.notify();
            }
        }))
    }

    fn thumbnail_available(&mut self, cx: &mut Context<Self>) {
        if self.cover.loaded().is_some() {
            return;
        }
        // The worker has committed a thumbnail, so any in-flight or missing
        // result is stale.
        self.cover = CoverState::Pending;
        // A prior asynchronous lookup may still be about to report "missing".
        // Cancelling it ensures the next render starts a fresh read of the file
        // that the scanner has just committed.
        self.cover_task.take();
        cx.notify();
    }

    fn listen_for_thumbnail(&mut self, cx: &mut Context<Self>) {
        let Some(content_hash) = self.book.content_hash().filter(|_| self.cover_updates.is_none()) else { return };
        self.cover_updates = Some(Self::subscribe_to_cover_updates(&self.ui, content_hash, cx));
    }

    fn load_cover(&mut self, resolution: ThumbnailResolution, cx: &mut Context<Self>) {
        // A feed book's cover comes from its feed.
        if let BookSource::Opds { source, entry, .. } = self.book.source {
            self.load_feed_cover(source, entry, resolution, cx);
            return;
        }
        if let BookSource::ExternalCatalog { entry, .. } = self.book.source {
            let library = self.ui.backend().clone();
            self.load_remote_cover(entry, resolution, async move { library.external_catalog_cover(entry, resolution).await }, cx);
            return;
        }
        let Some(content_hash) = self.book.content_hash() else {
            self.cover = CoverState::Missing;
            return;
        };
        let library = self.ui.backend().clone();
        let load = async move { library.thumbnail(content_hash, resolution).await };
        let generation = cover_generation(cx);
        let decoder = cx.background_executor().clone();
        self.cover_task = Some(cx.spawn(async move |view, cx| {
            let cover_stage = Stage::deferred("cover retrieval and decoding");
            let result = match load.await {
                Ok(Some(bytes)) => decoder.spawn(async move { loaded_cover_from_jpeg(bytes) }).await.map(Some).ok_or_else(|| format!("thumbnail for book {content_hash} is not a readable JPEG")),
                Ok(None) => Ok(None),
                Err(error) => Err(error),
            };
            cover_stage.finish(match &result {
                Ok(Some(_)) => "decoded",
                Ok(None) => "missing",
                Err(_) => "failed",
            });
            cx.defer_update(view, move |view, cx| {
                if generation != cover_generation(cx) {
                    return;
                }
                view.cover_task.take();
                if view.book.content_hash() != Some(content_hash) {
                    return;
                }
                match result {
                    Ok(Some(cover)) => {
                        // Only the first arrival fades; a higher-resolution
                        // thumbnail replacing a lower one is not a new cover.
                        view.remember_cover(cover, resolution, cx);
                        view.cover_updates.take();
                    }
                    Ok(None) => {
                        if view.cover.loaded().is_none() {
                            view.cover = CoverState::Missing;
                        }
                        view.listen_for_thumbnail(cx);
                    }
                    Err(error) => {
                        log::error!("failed to load thumbnail for book {:?}: {error}", view.book.title);
                        if view.cover.loaded().is_none() {
                            view.cover = CoverState::Unavailable;
                        }
                        view.listen_for_thumbnail(cx);
                    }
                }
                cx.notify();
            });
        }));
    }

    /// A feed's cover arrives once or not at all: no worker will publish one
    /// later, so a missing cover stays missing.
    fn load_feed_cover(&mut self, source: library_model::OpdsSourceId, entry: library_model::CardKey, resolution: ThumbnailResolution, cx: &mut Context<Self>) {
        let app = self.ui.app().clone();
        let load = async move { app.opds_card_cover(source, entry, resolution.width()).await };
        self.load_remote_cover(entry, resolution, load, cx);
    }

    fn load_remote_cover(&mut self, entry: CardKey, resolution: ThumbnailResolution, load: impl Future<Output = Result<Option<Vec<u8>>, String>> + 'static, cx: &mut Context<Self>) {
        let generation = cover_generation(cx);
        let decoder = cx.background_executor().clone();
        self.cover_task = Some(cx.spawn(async move |view, cx| {
            let cover_stage = Stage::deferred("cover retrieval and decoding");
            let cover = match load.await {
                Ok(Some(bytes)) => decoder.spawn(async move { loaded_cover_from_jpeg(bytes) }).await.map(Some).ok_or_else(|| "catalog or feed cover is not a readable JPEG".to_owned()),
                Ok(None) => Ok(None),
                Err(error) => Err(error),
            };
            cover_stage.finish(match &cover {
                Ok(Some(_)) => "decoded",
                Ok(None) => "missing",
                Err(_) => "failed",
            });
            cx.defer_update(view, move |view, cx| {
                if generation != cover_generation(cx) {
                    return;
                }
                view.cover_task.take();
                if view.book.key() != entry {
                    return;
                }
                match cover {
                    Ok(Some(cover)) => view.remember_cover(cover, resolution, cx),
                    Ok(None) => {
                        if view.cover.loaded().is_none() {
                            view.cover = CoverState::Missing;
                        }
                    }
                    Err(error) => {
                        log::warn!("failed to load a catalog or feed cover: {error}");
                        if view.cover.loaded().is_none() {
                            view.cover = CoverState::Unavailable;
                        }
                    }
                }
                cx.notify();
            });
        }));
    }

    fn cover_cache_key(&self) -> (Uuid, CardKey) {
        (*self.ui.backend().id(), self.book.key())
    }

    fn remember_cover(&mut self, cover: LoadedCover, resolution: ThumbnailResolution, cx: &mut Context<Self>) {
        // A slower request must not replace an already decoded denser cover.
        if let Some((image, cached_resolution)) = cx.default_global::<BrowseCovers>().cache.get(&self.cover_cache_key()) {
            if cached_resolution == resolution || (cached_resolution == ThumbnailResolution::CardHighDensity && resolution == ThumbnailResolution::Browse) {
                track_image(&image, cx);
                self.cover = CoverState::Loaded(Arc::downgrade(&image), cached_resolution);
                self.cover_loaded_at.get_or_insert_with(Instant::now);
                return;
            }
        }
        track_image(&cover.image, cx);
        cx.default_global::<BrowseCovers>().cache.insert(self.cover_cache_key(), &cover.image, resolution);
        self.cover = CoverState::Loaded(Arc::downgrade(&cover.image), resolution);
        self.cover_loaded_at.get_or_insert_with(Instant::now);
    }

    pub(crate) fn prepare(&mut self, resolution: ThumbnailResolution, cx: &mut Context<Self>) {
        // With "generated covers only", no art is read or decoded at all.
        if crate::stores::generated_covers_only(cx) {
            self.cover_task.take();
            self.cover_updates.take();
            self.cover = CoverState::Missing;
            return;
        }
        // Touch covers as they are drawn or prefetched, and share a decoded
        // image with other cards for this book. An evicted card reads its JPEG
        // again on demand. Higher densities replace the same cache entry.
        if let Some((image, resolution)) = cx.default_global::<BrowseCovers>().cache.get(&self.cover_cache_key()) {
            track_image(&image, cx);
            self.cover = CoverState::Loaded(Arc::downgrade(&image), resolution);
            self.cover_loaded_at.get_or_insert_with(Instant::now);
            self.cover_updates.take();
        } else if let CoverState::Loaded(image, _) = &self.cover {
            // A painted frame or detail can still own this image. Keep using
            // it without reinserting it and displacing other cached covers.
            if image.upgrade().is_none() {
                self.cover = CoverState::Pending;
            }
        }
        let needs_cover = match &self.cover {
            CoverState::Pending => true,
            CoverState::Loaded(image, loaded_resolution) => image.upgrade().is_none() || (*loaded_resolution == ThumbnailResolution::Browse && resolution == ThumbnailResolution::CardHighDensity),
            CoverState::Missing | CoverState::Unavailable => false,
        };
        if needs_cover && self.cover_task.is_none() {
            // Listen before the asynchronous read: a synced thumbnail can be
            // published while that read is still reporting the old absence.
            if matches!(self.cover, CoverState::Pending) {
                self.listen_for_thumbnail(cx);
            }
            self.load_cover(resolution, cx);
        }
    }

    /// Allows a container to fetch a card's cover before the card is visible.
    /// A visible card also starts this request from its own render path.
    pub(crate) fn prefetch_for_window(card: &Entity<Self>, window: &Window, cx: &mut gpui::App) {
        let resolution = Self::thumbnail_resolution(window);
        card.update(cx, |card, cx| card.prepare(resolution, cx));
    }

    /// What a book's page opens with: the row this card is drawing, and the
    /// cover it has already decoded. Handing both over is why that page paints
    /// complete in the frame the card was clicked in.
    pub(crate) fn detail_source(&self) -> (BookCardRow, Option<LoadedCover>) {
        (self.book.clone(), self.cover.loaded())
    }

    /// Activating the cover opens the book's page. The overlay's play button
    /// uses `play` to open the reader or start audiobook playback directly.
    /// Takes what it needs out of the card and lets go of it before acting.
    ///
    /// Deliberately not `card.update(|card, cx| …)`: going to the book's page is
    /// navigation, and the load that follows touches every card in the grid —
    /// their cursor, their card mode — including this one. Holding this card
    /// leased across that is a panic, and was one.
    pub(crate) fn activate(card: &Entity<Self>, window: &mut Window, cx: &mut gpui::App) {
        let (book, cover, events) = {
            let card = card.read(cx);
            let (book, cover) = card.detail_source();
            (book, cover, card.events.clone())
        };
        events(BookCardEvent::OpenDetail(book, cover), window, cx);
    }

    /// Release the card before either navigation or playback touches the grid.
    fn play(card: &Entity<Self>, window: &mut Window, cx: &mut App) {
        let (book, cover, events, ui) = {
            let card = card.read(cx);
            let (book, cover) = card.detail_source();
            (book, cover, card.events.clone(), card.ui.clone())
        };
        let Some(content_hash) = book.library_file() else { return };
        if book.format_category == library_model::LibraryFileTypeFilter::Audiobook {
            events(BookCardEvent::OpenDetail(book.clone(), cover), window, cx);
        }
        ui.open_book_at(content_hash, book.title, None, window, cx);
    }

    /// Runs a book command from the card menu: the backend call, with failures
    /// surfaced as toasts.
    fn run_menu_action(target: Entity<Self>, command: BookCommand, operation: impl std::future::Future<Output = Result<(), String>> + 'static, cx: &mut App) {
        let keep_alive = target.clone();
        target.update(cx, |_, cx| {
            cx.spawn(async move |card, cx| {
                let _keep_alive = keep_alive;
                if let Err(error) = operation.await {
                    let _ = card.update_in(cx, |_, window, cx| {
                        command.report_failure(error, window, cx);
                    });
                }
            })
            .detach();
        });
    }

    fn open_context_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.open_books_menu(vec![self.book.clone()], position, window, cx);
    }

    /// Mouse selections and keyboard menus use the card's ordinary book actions.
    pub(crate) fn open_items_menu(cards: &[Entity<Self>], position: Point<Pixels>, window: &mut Window, cx: &mut App) {
        let Some(target) = cards.first() else { return };
        let mut seen = HashSet::new();
        let books = cards.iter().map(|card| card.read(cx).book.clone()).filter(|book| seen.insert(book.key())).collect();
        target.update(cx, |card, cx| card.open_books_menu(books, position, window, cx));
    }

    fn open_books_menu(&mut self, books: Vec<BookCardRow>, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        if books.iter().any(|book| book.library_file().is_none()) {
            self.open_catalog_menu(books, position, window, cx);
            return;
        }
        // Library books only from here, so every book has its file's hash.
        let hashes = books.iter().filter_map(BookCardRow::library_file).collect::<Vec<_>>();
        let mobile_size = mobile_context_menu::size(window);
        let new_windows =
            books.iter().zip(&hashes).map(|(book, hash)| crate::OpenInNewWindow(crate::NewWindowTarget::Book { locator: library_model::BookLocator::new(*self.ui.backend().id(), *hash), title: book.title.clone() })).collect::<Vec<_>>();
        let source_directory = (books.len() == 1).then_some(books[0].source_directory).flatten();
        let content_hash = hashes[0];
        let removal_target = cx.entity();
        let backend = self.ui.backend().clone();
        let downloaded = hashes.iter().copied().filter(|hash| matches!(self.ui.updates().read(cx).download_state(*hash), Some(DownloadState::Downloaded))).collect::<Vec<_>>();
        let events = self.events.clone();
        let folder_commands = self.folder_commands;
        let copy_from_library = self.copy_from_library;
        let previous_focus = window.focused(cx);
        let menu = PopupMenu::build(window, cx, move |mut menu, _, _| {
            if let Some(focus) = previous_focus {
                menu = menu.action_context(focus);
            }
            if crate::native_windows_available() {
                let actions = new_windows.clone();
                menu = menu
                    .item(components::menu_item("Open in New Window", move |_, window, cx| {
                        for action in &actions {
                            window.dispatch_action(Box::new(action.clone()), cx);
                        }
                    }))
                    .separator();
            }
            if copy_from_library {
                let copy_events = events.clone();
                let hashes = hashes.clone();
                menu = menu.item(components::menu_item_with_icon("Copy to folder…", IconName::Copy, move |_, window, cx| copy_events(BookCardEvent::CopyToFolder(hashes.clone()), window, cx))).separator();
            }
            if let Some(source_directory) = source_directory.filter(|_| folder_commands) {
                let (copy_events, move_events) = (events.clone(), events.clone());
                menu = menu
                    .item(components::menu_item_with_icon("Copy to folder…", IconName::Copy, move |_, window, cx| copy_events(BookCardEvent::Place(content_hash, source_directory, BookPlacement::Copy), window, cx)))
                    .item(components::menu_item_with_icon("Move to folder…", IconName::FolderOpen, move |_, window, cx| move_events(BookCardEvent::Place(content_hash, source_directory, BookPlacement::Move), window, cx)))
                    .separator();
            }
            if !downloaded.is_empty() {
                let library = backend.clone();
                let evict_target = removal_target.clone();
                let downloads = downloaded.clone();
                menu = menu
                    .item(components::menu_item_with_icon("Remove download", IconName::HardDrive, move |_, _window, cx| {
                        let library = library.clone();
                        let downloads = downloads.clone();
                        let operation = async move {
                            for book in downloads {
                                library.remove_local_book_copy(book).await?;
                            }
                            Ok::<_, String>(())
                        };
                        evict_target.update(cx, |_, cx| {
                            cx.spawn(async move |card, cx| {
                                if let Err(error) = operation.await {
                                    let _ = card.update_in(cx, |_, window, cx| {
                                        notify_error("book-evict-error", format!("Could not remove download. {}", user_facing_error(&error, "Please try again.")), window, cx);
                                    });
                                }
                            })
                            .detach();
                        });
                    }))
                    .separator();
            }
            // Leaving a folder and leaving the library are separate commands:
            // the first keeps the book in its other folders, the second moves
            // it to Trash outright.
            if source_directory.is_some() {
                if let Some(source_directory) = source_directory.filter(|_| folder_commands) {
                    let events = events.clone();
                    menu = menu.item(components::menu_item_with_icon("Remove from folder", components::navigation_icon(components::NavigationIcon::Folder), move |_, window, cx| {
                        events(BookCardEvent::Remove(content_hash, source_directory), window, cx);
                    }));
                } else if let Some(source_id) = source_directory {
                    let library = backend.clone();
                    let direct_target = removal_target.clone();
                    menu = menu.item(components::menu_item_with_icon("Remove from folder", components::navigation_icon(components::NavigationIcon::Folder), move |_, _, cx| {
                        let library = library.clone();
                        let operation = async move { library.remove_book_from_directory(content_hash, source_id).await.map(|_| ()) };
                        Self::run_menu_action(direct_target.clone(), BookCommand::RemoveFromFolder, operation, cx);
                    }));
                }
            }
            let library = backend.clone();
            let trash_target = removal_target.clone();
            let command = if books.len() == 1 { BookCommand::TrashBook } else { BookCommand::TrashBooks };
            let hashes = hashes.clone();
            menu = menu.item(components::menu_item_with_icon("Move to Trash", components::navigation_icon(components::NavigationIcon::Trash2), move |_, _, cx| {
                let library = library.clone();
                let hashes = hashes.clone();
                let operation = async move {
                    for book in hashes {
                        library.move_book_to_trash(book).await?;
                    }
                    Ok(())
                };
                Self::run_menu_action(trash_target.clone(), command, operation, cx);
            }));
            mobile_context_menu::configure(menu, mobile_size)
        });
        let subscription = cx.subscribe(&menu, |card, _, _: &DismissEvent, cx| {
            card.context_menu = None;
            card.context_menu_subscription = None;
            cx.notify();
        });
        menu.focus_handle(cx).focus(window, cx);
        self.context_menu = Some(BookCardContextMenu { menu, position });
        self.context_menu_subscription = Some(subscription);
        cx.notify();
    }

    /// Catalog and feed books offer one command: adding those the library
    /// does not hold yet and that can be downloaded.
    fn open_catalog_menu(&mut self, books: Vec<BookCardRow>, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let mobile_size = mobile_context_menu::size(window);
        let external = (books.len() == 1).then(|| books.first().cloned()).flatten().filter(|book| matches!(book.source, BookSource::ExternalCatalog { downloadable: false, .. }));
        let events = self.events.clone();
        let not_offered = |book: &BookCardRow| matches!(book.source, BookSource::Opds { downloadable: false, .. } | BookSource::ExternalCatalog { downloadable: false, .. });
        let nothing_offered = books.iter().all(not_offered);
        let missing = books.into_iter().filter(|book| !held(book, cx) && !not_offered(book)).collect::<Vec<_>>();
        let target = cx.entity();
        let menu = PopupMenu::build(window, cx, move |menu, _, _| {
            let menu = if let Some(book) = &external {
                let (book, events) = (book.clone(), events.clone());
                menu.item(components::menu_item_with_icon("Book details", IconName::ExternalLink, move |_, window, cx| {
                    events(BookCardEvent::OpenDetail(book.clone(), None), window, cx);
                }))
            } else if missing.is_empty() && nothing_offered {
                menu.item(components::menu_item_with_icon("Not offered for download", IconName::Info, |_, _, _| {}).disabled(true))
            } else if missing.is_empty() {
                menu.item(components::menu_item_with_icon("In your library", IconName::Check, |_, _, _| {}).disabled(true))
            } else {
                let (target, missing) = (target.clone(), missing.clone());
                menu.item(components::menu_item_with_icon("Add to library", IconName::Plus, move |_, _, cx| {
                    let missing = missing.clone();
                    target.update(cx, |card, cx| card.add(missing, cx));
                }))
            };
            mobile_context_menu::configure(menu, mobile_size)
        });
        let subscription = cx.subscribe(&menu, |card, _, _: &DismissEvent, cx| {
            card.context_menu = None;
            card.context_menu_subscription = None;
            cx.notify();
        });
        menu.focus_handle(cx).focus(window, cx);
        self.context_menu = Some(BookCardContextMenu { menu, position });
        self.context_menu_subscription = Some(subscription);
        cx.notify();
    }
}

impl BookCardView {
    fn render_card(&mut self, presentation: BookCardPresentation, interaction: BookCardInteraction, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        self.prepare(Self::thumbnail_resolution(window), cx);
        let theme = components::browser_theme(cx);
        let grid_cover = presentation == BookCardPresentation::Grid;
        let cover_opacity = match self.cover_loaded_at {
            Some(_) if cfg!(feature = "kobo") || cx.reduce_motion() => 1.0,
            Some(loaded_at) => {
                let progress = loaded_at.elapsed().as_secs_f32() / COVER_FADE_DURATION.as_secs_f32();
                if progress < 1.0 {
                    window.request_animation_frame();
                }
                progress.clamp(0.0, 1.0)
            }
            None => 1.0,
        };
        let book = self.book.clone();
        let cover = self.cover.loaded();
        let key = book.key();
        let element_id = format!("book-card-{key}");
        let cover_id = format!("book-cover-{key}");

        let cover_text = if presentation == BookCardPresentation::GraphCover { CoverText::CoverOnly } else { crate::stores::current_cover_text(cx) };
        let open_target = cx.entity();
        let cover_ratio = match &cover {
            Some(cover) => cover_aspect_ratio(&cover.image),
            // A generated cover keeps its own shape, audiobook or not.
            None if book.format_category == library_model::LibraryFileTypeFilter::Audiobook && !matches!(self.cover, CoverState::Missing) => 1.0,
            None => components::BOOK_COVER_ASPECT_RATIO,
        };
        // A book not on this device says so on its cover, in the corner nothing
        // else uses, and the mark is the way to fetch it. A downloaded book
        // carries no mark: which state is the rarer one differs from device to
        // device, and marking only the one that needs acting on keeps a mostly
        // downloaded library quiet and a mostly remote one legible.
        let download_badge = match book.source {
            // A catalog or feed book's corner adds it, or marks that the library holds it.
            BookSource::Catalog { .. } | BookSource::Opds { .. } | BookSource::ExternalCatalog { downloadable: true, .. } if held(&book, cx) => {
                Some(components::book_cover_in_library_badge(format!("book-in-library-{key}"), theme).tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("In your library").build(window, cx)))
            }
            // A feed book the feed sells opens its store page.
            BookSource::Opds { entry, downloadable: false, buyable: true, .. } => {
                let buy_target = cx.entity();
                Some(
                    components::book_cover_buy_badge(format!("book-buy-{key}"), theme)
                        .tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("Buy").build(window, cx))
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            buy_target.update(cx, |card, cx| card.buy(entry, window, cx));
                        }),
                )
            }
            // A feed book the feed only lends or locks has nothing to add.
            BookSource::Opds { downloadable: false, .. } | BookSource::ExternalCatalog { downloadable: false, .. } => None,
            BookSource::Catalog { .. } | BookSource::Opds { downloadable: true, .. } | BookSource::ExternalCatalog { downloadable: true, .. } => {
                let add_target = cx.entity();
                let added = book.clone();
                Some(
                    components::book_cover_add_badge(format!("book-add-{key}"), self.add_task.is_some(), theme)
                        .tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("Add to library").build(window, cx))
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            let added = added.clone();
                            add_target.update(cx, |card, cx| card.add(vec![added], cx));
                        }),
                )
            }
            BookSource::Library { content_hash } => match self.ui.updates().read(cx).download_state(content_hash).unwrap_or(DownloadState::Queued) {
                DownloadState::Downloaded => None,
                DownloadState::Queued => Some(components::book_cover_queued_badge(format!("book-download-{key}"), theme).tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("Queued for download").build(window, cx))),
                DownloadState::Downloading(_) => Some(components::book_cover_download_badge(format!("book-download-{key}"), true, theme)),
                // The browser streams books it does not hold instead of downloading them.
                DownloadState::NotDownloaded if cfg!(target_arch = "wasm32") => None,
                DownloadState::NotDownloaded => {
                    let download_target = cx.entity();
                    Some(
                        components::book_cover_download_badge(format!("book-download-{key}"), false, theme)
                            .tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("Download to this device").build(window, cx))
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                download_target.update(cx, |card, cx| card.download(cx));
                            }),
                    )
                }
            },
        };
        let play_button = book.library_file().filter(|_| !interaction.marquee).map(|_| {
            let play_target = cx.entity();
            components::book_cover_play_button(format!("book-play-{key}"), theme)
                .tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("Play").build(window, cx))
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    Self::play(&play_target, window, cx);
                })
        });
        let cover_frame = components::book_cover_frame(theme, cover_ratio)
            .id(cover_id)
            .when(presentation == BookCardPresentation::Section, |frame| frame.flex_none())
            .on_click(move |_, window, cx| Self::activate(&open_target, window, cx))
            .child(match cover {
                Some(cover) => components::book_cover(theme, cover_ratio).child(retain_visible_cover(cover.image.clone())).child(components::book_cover_image(format!("book-cover-art-{key}"), cover.image, cover_opacity, false, theme)),
                None if matches!(self.cover, CoverState::Missing) => components::book_cover_generated(book.title.clone(), book.subtitle.as_deref(), &book.author, generated_cover_variant(key), theme),
                // Keep the slot empty until absence is confirmed, including
                // reloads after eviction and failed cover requests.
                None => components::book_cover(theme, cover_ratio),
            })
            .when(grid_cover, |frame| frame.children(components::book_cover_progress(book.progress, theme)))
            .children(play_button)
            // The cover's action — add, download, or the held mark — is on every
            // cover, including those in section cards and the graph.
            .children(download_badge)
            .when(grid_cover && book.format_category == library_model::LibraryFileTypeFilter::Audiobook, |frame| frame.child(components::book_cover_audiobook_length(book.audiobook_duration_ms.map(audiobook_card_length), theme)));

        let context_menu = self.context_menu.as_ref().map(|active| mobile_context_menu::render(active.menu.clone(), active.position, window));
        let context_menu_target = cx.entity();
        let long_press_target = context_menu_target.clone();
        let hover_enabled = !interaction.marquee && presentation != BookCardPresentation::Section;
        let card = components::book_card(element_id, interaction.focused, interaction.selected, hover_enabled, theme)
            .when(presentation == BookCardPresentation::Section, |card| {
                // The section's keyboard frame adds a 1px border outside this
                // card's 2px outline. Together with padding they total 8px.
                card.p(px((components::BOOK_CARD_PADDING - 3.0).max(0.0)))
            })
            .on_aux_click(move |event, window, cx| {
                // A card inside a selection is not the subject of its own menu:
                // the page opens one over everything the band picked up.
                if event.is_secondary() && !interaction.selected {
                    context_menu_target.update(cx, |card, cx| card.open_context_menu(event.position(), window, cx));
                    cx.stop_propagation();
                }
            })
            .on_long_press(move |event, window, cx| {
                if !interaction.selected {
                    long_press_target.update(cx, |card, cx| card.open_context_menu(event.position, window, cx));
                    cx.stop_propagation();
                }
            });

        let card = card.child(cover_frame);
        // No text block at all under "cover only", not an empty one: an empty
        // child would still hold the cover-to-text gap open under a cover that
        // has nothing under it. The download mark is on the cover, so it stays.
        let card = match cover_text {
            CoverText::Always => card.child(components::book_card_text_title_only().child(components::book_card_title_row(book.title, book.subtitle.as_deref()))),
            CoverText::CoverOnly => card,
        };

        components::book_card_cell().when(presentation == BookCardPresentation::GraphCover, |cell| cell.justify_start()).child(card.children(context_menu)).into_any_element()
    }
}

impl Render for BookCardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_card(BookCardPresentation::Grid, BookCardInteraction::default(), window, cx)
    }
}

#[cfg(all(test, not(target_arch = "wasm32"), not(feature = "kobo")))]
mod tests;
