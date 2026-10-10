use crate::services::{BookConfirmation, notify_error, notify_info};
use library_backend::{DownloadState, ThumbnailResolution};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

/// Long enough to read as a settle rather than a flash, short enough that a
/// fast-filling grid does not feel sluggish.
const COVER_FADE_DURATION: Duration = Duration::from_millis(180);

use gpui::prelude::*;
use gpui::{App, AppContext, Context, DismissEvent, Entity, Focusable as _, IntoElement, Pixels, Point, Render, RenderImage, Subscription, Task, Window, px};
use gpui_component::IconName;
use gpui_component::menu::PopupMenu;
use library_model::{BookCardRow, BookSource};
use ui_components as components;

use crate::library::services::{LibraryContext, LibraryCoversChanged, LibraryDownloadChanged};
use sync_common::{ContentHash, DirId};
use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

use super::mobile_context_menu;

#[derive(Clone)]
pub(crate) struct LoadedCover {
    pub image: Arc<RenderImage>,
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
    /// Take the book out of the folder it is listed in, confirming with this
    /// description of what follows.
    Remove(sync_common::ContentHash, DirId, String),
}

pub(crate) type BookCardEvents = Rc<dyn Fn(BookCardEvent, &mut Window, &mut gpui::App)>;

/// A decoded cover's width over its height, for boxing it at its own
/// proportions instead of the placeholder's.
pub(crate) fn cover_aspect_ratio(image: &RenderImage) -> f32 {
    let size = image.size(0);
    size.width.0.max(1) as f32 / size.height.0.max(1) as f32
}

/// Which of the generated cover's fixed jacket colours a book with no art
/// gets. Keyed off the card's key (a library book's content hash) rather than
/// drawn at random, so a book generates the same cover every time it is shown.
fn generated_cover_variant(key: library_model::CardKey) -> usize {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    key.0.iter().flat_map(|byte| [HEX[(byte >> 4) as usize], HEX[(byte & 15) as usize]]).fold(0usize, |acc, byte| acc.wrapping_add(byte as usize))
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
    Loaded(LoadedCover, ThumbnailResolution),
    Missing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BookCardPresentation {
    Grid,
    Section,
    GraphCover,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct BookCardInteraction {
    pub(crate) focused: bool,
    pub(crate) selected: bool,
    pub(crate) marquee: bool,
}

pub(crate) struct BookCardElement {
    card: Entity<BookCardView>,
    presentation: BookCardPresentation,
    interaction: BookCardInteraction,
}

impl gpui::View for BookCardElement {
    fn entity_id(&self) -> Option<gpui::EntityId> {
        Some(self.card.entity_id())
    }
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
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

/// A feed book in the library is drawn as a catalog book held there is.
fn held(book: &BookCardRow, cx: &App) -> bool {
    match book.source {
        BookSource::Catalog { in_library, .. } => in_library,
        BookSource::Opds { .. } => feed_book_added(book.key(), cx),
        BookSource::Library { .. } => true,
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
    _download_updates: Subscription,
    download_task: Option<Task<()>>,
    /// Adding catalog books to the library, while it runs.
    add_task: Option<Task<()>>,
    context_menu: Option<BookCardContextMenu>,
    context_menu_subscription: Option<Subscription>,
    /// Repaints the card when the "cover text" setting changes, so an
    /// already-visible card hides or shows its title without waiting for
    /// some unrelated event to trigger a redraw.
    _cover_text_updates: Subscription,
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
        if physical_width <= ThumbnailResolution::Browse.width() as f32 { ThumbnailResolution::Browse } else { ThumbnailResolution::HighDensity }
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
        let download_updates = Self::subscribe_to_download_updates(&ui, row.content_hash(), cx);
        let cover_text_updates = cx.observe_global::<crate::stores::CoverTextSetting>(|_, cx| cx.notify());
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


    /// Supplies presentation to this drawing while retaining the card's reactive identity.
    pub(crate) fn present(card: Entity<Self>, presentation: BookCardPresentation, interaction: BookCardInteraction) -> gpui::ViewElement<BookCardElement> {
        gpui::ViewElement::new(BookCardElement { card, presentation, interaction })
    }

    pub(crate) fn replace_row(&mut self, row: BookCardRow) {
        debug_assert_eq!(self.book.key(), row.key());
        self.book = row;
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
                    notify_error("book-download-error", format!("Could not download book: {error}"), window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Adds catalog and feed books to the library. The listing then marks
    /// catalog books as held; feed books are marked for the session.
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
                    BookSource::Catalog { content_hash, in_library: false } => library.add_catalog_book(sync_common::ROOT_DIR_ID, content_hash).await,
                    // The app fetches from the feed; the library imports the file.
                    BookSource::Opds { source, entry, downloadable: true } => match app.download_opds_book(source, entry).await {
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
                        Err(error) => notify_error("catalog-add-error", format!("Could not add “{title}”: {error}"), window, cx),
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

    fn subscribe_to_download_updates(ui: &LibraryContext, content_hash: Option<ContentHash>, cx: &mut Context<Self>) -> Subscription {
        let updates = ui.updates();
        cx.subscribe(&updates, move |_view, _, event: &LibraryDownloadChanged, cx| {
            if Some(event.status.content_hash) == content_hash {
                cx.notify();
            }
        })
    }

    fn thumbnail_available(&mut self, cx: &mut Context<Self>) {
        if matches!(self.cover, CoverState::Loaded(..)) {
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
        let Some(content_hash) = self.book.content_hash() else {
            self.cover = CoverState::Missing;
            return;
        };
        let library = self.ui.backend().clone();
        let load = async move { library.thumbnail(content_hash, resolution).await };
        let decoder = cx.background_executor().clone();
        self.cover_task = Some(cx.spawn(async move |view, cx| {
            let result = match load.await {
                Ok(Some(bytes)) => decoder.spawn(async move { loaded_cover_from_jpeg(bytes) }).await.map(Some).ok_or_else(|| format!("thumbnail for book {content_hash} is not a readable JPEG")),
                Ok(None) => Ok(None),
                Err(error) => Err(error),
            };
            cx.defer_update(view, move |view, cx| {
                view.cover_task.take();
                if view.book.content_hash() != Some(content_hash) {
                    return;
                }
                match result {
                    Ok(Some(cover)) => {
                        // Only the first arrival fades; a higher-resolution
                        // thumbnail replacing a lower one is not a new cover.
                        view.cover_loaded_at.get_or_insert_with(Instant::now);
                        view.cover = CoverState::Loaded(cover, resolution);
                        view.cover_updates.take();
                    }
                    Ok(None) => {
                        view.cover = CoverState::Missing;
                        view.listen_for_thumbnail(cx);
                    }
                    Err(error) => {
                        log::error!("failed to load thumbnail for book {:?}: {error}", view.book.title);
                        view.cover = CoverState::Missing;
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
        let load = async move { app.opds_cover(source, entry, resolution.width()).await };
        let decoder = cx.background_executor().clone();
        self.cover_task = Some(cx.spawn(async move |view, cx| {
            let cover = match load.await {
                Ok(Some(bytes)) => decoder.spawn(async move { loaded_cover_from_jpeg(bytes) }).await,
                Ok(None) => None,
                Err(error) => {
                    log::warn!("failed to load a feed cover: {error}");
                    None
                }
            };
            cx.defer_update(view, move |view, cx| {
                view.cover_task.take();
                if view.book.key() != entry {
                    return;
                }
                view.cover = match cover {
                    Some(cover) => {
                        view.cover_loaded_at.get_or_insert_with(Instant::now);
                        CoverState::Loaded(cover, resolution)
                    }
                    None => CoverState::Missing,
                };
                cx.notify();
            });
        }));
    }

    pub(crate) fn prepare(&mut self, resolution: ThumbnailResolution, cx: &mut Context<Self>) {
        let needs_cover = match &self.cover {
            CoverState::Pending => true,
            CoverState::Loaded(_, loaded_resolution) => *loaded_resolution == ThumbnailResolution::Browse && resolution == ThumbnailResolution::HighDensity,
            CoverState::Missing => false,
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
        let cover = match &self.cover {
            CoverState::Loaded(cover, _) => Some(cover.clone()),
            CoverState::Pending | CoverState::Missing => None,
        };
        (self.book.clone(), cover)
    }

    /// Activating a card goes to the book's page, not into the book. Reading is
    /// one deliberate step further in, from the page's own Read button, so that
    /// a click on a cover can mean "tell me about this" — which is what it
    /// usually means in a library.
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

    /// Runs a confirmed book command from the card menu: one prompt, then the
    /// backend call, with failures surfaced as toasts.
    fn confirm_menu_action(target: Entity<Self>, confirmation: BookConfirmation, detail: String, operation: impl std::future::Future<Output = Result<(), String>> + 'static, window: &mut Window, cx: &mut App) {
        let answer = confirmation.prompt(&detail, window, cx);
        let keep_alive = target.clone();
        target.update(cx, |_, cx| {
            cx.spawn(async move |card, cx| {
                let _keep_alive = keep_alive;
                if !answer.await {
                    return;
                }
                if let Err(error) = operation.await {
                    let _ = card.update_in(cx, |_, window, cx| {
                        confirmation.report_failure(error, window, cx);
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
        let new_windows = books.iter().zip(&hashes).map(|(book, hash)| crate::OpenInNewWindow(crate::NewWindowTarget::Book { locator: library_model::BookLocator::new(*self.ui.backend().id(), *hash), title: book.title.clone() })).collect::<Vec<_>>();
        let source_directory = (books.len() == 1).then_some(books[0].source_directory).flatten();
        let content_hash = hashes[0];
        let title = books[0].title.clone();
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
                                        notify_error("book-evict-error", format!("Could not remove download: {error}"), window, cx);
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
                    let (description, events) = (format!("“{title}” stays in its other folders. If this is its last folder, it moves to Trash."), events.clone());
                    menu = menu.item(components::menu_item_with_icon("Remove from folder", components::navigation_icon(components::NavigationIcon::Folder), move |_, window, cx| {
                        events(BookCardEvent::Remove(content_hash, source_directory, description.clone()), window, cx);
                    }));
                } else if let Some(source_id) = source_directory {
                    let library = backend.clone();
                    let direct_target = removal_target.clone();
                    let detail = format!("“{title}” stays in its other folders. If this is its last folder, it moves to Trash.");
                    menu = menu.item(components::menu_item_with_icon("Remove from folder", components::navigation_icon(components::NavigationIcon::Folder), move |_, window, cx| {
                        let library = library.clone();
                        let operation = async move { library.remove_book_from_directory(content_hash, source_id).await.map(|_| ()) };
                        Self::confirm_menu_action(direct_target.clone(), BookConfirmation::RemoveFromFolder, detail.clone(), operation, window, cx);
                    }));
                }
            }
            let library = backend.clone();
            let trash_target = removal_target.clone();
            let detail = if books.len() == 1 { format!("“{title}” will be moved to Trash.") } else { format!("{} books will be moved to Trash.", books.len()) };
            let confirmation = BookConfirmation::TrashBooks(books.len());
            let hashes = hashes.clone();
            menu = menu.item(components::menu_item_with_icon("Move to Trash", components::navigation_icon(components::NavigationIcon::Trash2), move |_, window, cx| {
                let library = library.clone();
                let hashes = hashes.clone();
                let operation = async move {
                    for book in hashes {
                        library.move_book_to_trash(book).await?;
                    }
                    Ok(())
                };
                Self::confirm_menu_action(trash_target.clone(), confirmation, detail.clone(), operation, window, cx);
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
        let not_offered = |book: &BookCardRow| matches!(book.source, BookSource::Opds { downloadable: false, .. });
        let nothing_offered = books.iter().all(not_offered);
        let missing = books.into_iter().filter(|book| !held(book, cx) && !not_offered(book)).collect::<Vec<_>>();
        let target = cx.entity();
        let menu = PopupMenu::build(window, cx, move |menu, _, _| {
            let menu = if missing.is_empty() && nothing_offered {
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
        let highlighted = interaction.focused || interaction.selected;
        let cover_only = presentation != BookCardPresentation::Grid;
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
        let cover = self.cover.clone();
        let key = book.key();
        let element_id = format!("book-card-{key}");
        let cover_id = format!("book-cover-{key}");

        let cover_text = if cover_only { app_preferences::CoverText::CoverOnly } else { crate::stores::current_cover_text(cx) };
        let open_target = cx.entity();
        let cover_ratio = match &cover {
            CoverState::Loaded(cover, _) => cover_aspect_ratio(&cover.image),
            CoverState::Pending | CoverState::Missing if book.format_category == library_model::LibraryFileTypeFilter::Audiobook => 1.0,
            CoverState::Pending | CoverState::Missing => components::BOOK_COVER_ASPECT_RATIO,
        };
        // A book not on this device says so on its cover, in the corner nothing
        // else uses, and the mark is the way to fetch it. A downloaded book
        // carries no mark: which state is the rarer one differs from device to
        // device, and marking only the one that needs acting on keeps a mostly
        // downloaded library quiet and a mostly remote one legible.
        let download_badge = match book.source {
            // A catalog or feed book's corner adds it, or marks that the library holds it.
            BookSource::Catalog { .. } | BookSource::Opds { .. } if held(&book, cx) => {
                Some(components::book_cover_in_library_badge(format!("book-in-library-{key}")).tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("In your library").build(window, cx)))
            }
            // A feed book the feed only sells, lends or locks has nothing to add.
            BookSource::Opds { downloadable: false, .. } => None,
            BookSource::Catalog { .. } | BookSource::Opds { downloadable: true, .. } => {
                let add_target = cx.entity();
                let added = book.clone();
                Some(
                    components::book_cover_add_badge(format!("book-add-{key}"), self.add_task.is_some())
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
            DownloadState::Queued | DownloadState::Downloading(_) => Some(components::book_cover_download_badge(format!("book-download-{key}"), true)),
            // The browser streams books it does not hold instead of downloading them.
            DownloadState::NotDownloaded if cfg!(target_arch = "wasm32") => None,
            DownloadState::NotDownloaded => {
                let download_target = cx.entity();
                Some(
                    components::book_cover_download_badge(format!("book-download-{key}"), false)
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
        let cover_frame = components::book_cover_frame(theme, cover_ratio)
            .id(cover_id)
            .on_click(move |_, window, cx| Self::activate(&open_target, window, cx))
            .child(match cover {
                CoverState::Loaded(cover, _) => components::book_cover(theme, cover_ratio).child(components::book_cover_image(format!("book-cover-art-{key}"), cover.image, cover_opacity, highlighted, theme)),
                CoverState::Pending => components::book_cover(theme, cover_ratio),
                // A blank placeholder says nothing without the title line
                // under it, so "cover only" replaces it with a generated
                // cover that carries the title itself.
                CoverState::Missing if cover_text == app_preferences::CoverText::CoverOnly => {
                    components::book_cover_generated(book.title.clone(), book.subtitle.as_deref(), generated_cover_variant(key), theme).aspect_ratio(cover_ratio)
                }
                CoverState::Missing => components::missing_book_cover("No cover", theme).aspect_ratio(cover_ratio),
            })
            .when(!cover_only, |frame| frame.children(components::book_cover_progress(book.progress, theme)).children(download_badge))
            .when(!cover_only && book.format_category == library_model::LibraryFileTypeFilter::Audiobook, |frame| frame.child(components::book_cover_audiobook_length(book.audiobook_duration_ms.map(audiobook_card_length))));

        let context_menu = self.context_menu.as_ref().map(|active| mobile_context_menu::render(active.menu.clone(), active.position, window));
        let context_menu_target = cx.entity();
        let card = components::book_card(element_id, highlighted, interaction.marquee, theme).when(presentation == BookCardPresentation::Section, |card| card.pl(px(4.0)).pr(px(components::BOOK_CARD_PADDING * 2.0 - 4.0))).on_aux_click(
            move |event, window, cx| {
                // A card inside a selection is not the subject of its own menu:
                // the page opens one over everything the band picked up.
                if event.is_secondary() && !interaction.selected {
                    context_menu_target.update(cx, |card, cx| card.open_context_menu(event.position(), window, cx));
                    cx.stop_propagation();
                }
            },
        );

        let card = card.child(cover_frame);
        // No text block at all under "cover only", not an empty one: an empty
        // child would still hold the cover-to-text gap open under a cover that
        // has nothing under it. The download mark is on the cover, so it stays.
        let card = match cover_text {
            app_preferences::CoverText::Always => card.child(components::book_card_text_title_only().child(components::book_card_title_row(book.title, book.subtitle.as_deref()))),
            app_preferences::CoverText::CoverOnly => card,
        };

        components::book_card_cell().when(presentation == BookCardPresentation::GraphCover, |cell| cell.justify_start()).child(card.children(context_menu)).into_any_element()
    }
}

impl Render for BookCardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_card(BookCardPresentation::Grid, BookCardInteraction::default(), window, cx)
    }
}
