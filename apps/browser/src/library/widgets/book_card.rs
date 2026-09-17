use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

/// Long enough to read as a settle rather than a flash, short enough that a
/// fast-filling grid does not feel sluggish.
const COVER_FADE_DURATION: Duration = Duration::from_millis(180);

use gpui::prelude::*;
use gpui::{Anchor, AppContext, Context, DismissEvent, Entity, Focusable as _, IntoElement, Pixels, Point, PromptLevel, Render, RenderImage, Subscription, Task, Window, anchored, deferred, px};
use gpui_component::menu::PopupMenu;
use gpui_component::tag::Tag;
use gpui_component::{IconName, Sizable, Size};
use library_model::BookCardRow;
use ui_components as components;

use crate::library::services::{LibraryContext, LibraryCoversChanged, LibraryDownloadChanged};
use sync_common::DirId;
use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

#[derive(Clone)]
pub(crate) struct LoadedCover {
    pub image: Arc<RenderImage>,
}

/// Navigates the grid this card is in to the book's own page.
///
/// The card hands over its row and its decoded cover rather than being looked
/// up afterwards, and not only to save the lookup: this is called from inside
/// the card's own update, so anything that went back and read the cards would
/// read one that is currently leased, and panic.
pub(crate) type OpenBookDetail = std::rc::Rc<dyn Fn(BookCardRow, Option<LoadedCover>, &mut gpui::App)>;

#[derive(Clone, Copy)]
pub(crate) enum BookPlacement {
    Copy,
    Move,
}

pub(crate) type PlaceBook = std::rc::Rc<dyn Fn(sync_common::ContentHash, DirId, BookPlacement, &mut gpui::App)>;
pub(crate) type RemoveBook = std::rc::Rc<dyn Fn(sync_common::ContentHash, DirId, String, &mut Window, &mut gpui::App)>;

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
    Loaded(LoadedCover, bokheim_backend::ThumbnailResolution),
    Missing,
}

pub(crate) struct BookCardView {
    book: BookCardRow,
    /// Whether this card holds the paginator's keyboard cursor. Pushed in by
    /// the page rather than read out of anything: the card knows what focused
    /// looks like, not where the cursor is.
    focused: bool,
    /// Whether a rubber band has picked this card up. Painted exactly like
    /// `focused` — the two cannot coexist, since moving the cursor drops the
    /// selection — but held apart, because a selected card hands its secondary
    /// click to the page's menu over the whole selection.
    selected: bool,
    /// Whether a rubber band is currently sweeping the grid. Suppresses the
    /// card's hover style: a band drags the pointer across every card it picks
    /// up, and a card answering that with hover reports where the pointer went
    /// rather than what is selected.
    marquee: bool,
    /// Whether the card names its author. False on a shelf that is already one
    /// person's, where the line would repeat the name above it.
    states_author: bool,
    /// Whether the card is drawn in its detailed mode: twice as wide, with the
    /// blurb beside the cover instead of nothing but the title under it. Pushed
    /// in by the page, exactly like the cursor — the grid decides how wide a
    /// cell is, and the card decides what to put in one.
    detailed: bool,
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
    /// How this card asks for the book's page. Absent on cards that are not in
    /// a browse grid — an author's shelf, the home page — which still open the
    /// reader directly until those pages route through it too.
    open_detail: Option<OpenBookDetail>,
    cover_task: Option<Task<()>>,
    cover_updates: Option<Subscription>,
    download_state: bokheim_backend::DownloadState,
    _download_updates: Subscription,
    download_task: Option<Task<()>>,
    source_directory: Option<DirId>,
    place_book: Option<PlaceBook>,
    remove_book: Option<RemoveBook>,
    context_menu: Option<BookCardContextMenu>,
    context_menu_subscription: Option<Subscription>,
}

struct BookCardContextMenu {
    menu: Entity<PopupMenu>,
    position: Point<Pixels>,
}

impl BookCardView {
    pub(crate) fn title(&self) -> &str {
        &self.book.title
    }
    pub(crate) fn thumbnail_resolution(window: &Window) -> bokheim_backend::ThumbnailResolution {
        let logical_width = gpui::px(components::BOOK_CARD_CONTENT_WIDTH);
        let physical_width = f32::from(logical_width) * window.scale_factor();
        if physical_width <= bokheim_backend::ThumbnailResolution::Browse.width() as f32 { bokheim_backend::ThumbnailResolution::Browse } else { bokheim_backend::ThumbnailResolution::HighDensity }
    }

    pub(crate) fn create(row: BookCardRow, ui: LibraryContext, cx: &mut impl AppContext) -> Entity<Self> {
        cx.new(|cx| Self::new(row, ui, None, cx))
    }

    pub(crate) fn create_in_folder(row: BookCardRow, ui: LibraryContext, source_directory: DirId, cx: &mut impl AppContext) -> Entity<Self> {
        cx.new(|cx| Self::new(row, ui, Some(source_directory), cx))
    }

    /// A card in a browse grid, which is what gives it somewhere to navigate to.
    pub(crate) fn create_in_grid(row: BookCardRow, ui: LibraryContext, source_directory: Option<DirId>, open_detail: OpenBookDetail, place_book: Option<PlaceBook>, remove_book: Option<RemoveBook>, cx: &mut impl AppContext) -> Entity<Self> {
        cx.new(|cx| {
            let mut card = Self::new(row, ui, source_directory, cx);
            card.open_detail = Some(open_detail);
            card.place_book = place_book;
            card.remove_book = remove_book;
            card
        })
    }

    /// A card for a shelf that is already filed under one person — the author
    /// index. It states no author, because the name over the shelf says it once
    /// for every card under it, and the card is shorter by that line.
    pub(crate) fn create_on_author_shelf(row: BookCardRow, ui: LibraryContext, cx: &mut impl AppContext) -> Entity<Self> {
        cx.new(|cx| {
            let mut card = Self::new(row, ui, None, cx);
            card.states_author = false;
            card
        })
    }

    fn new(row: BookCardRow, ui: LibraryContext, source_directory: Option<DirId>, cx: &mut Context<Self>) -> Self {
        let cover_updates = Self::subscribe_to_cover_updates(&ui, row.content_hash, cx);
        let download_state = Self::row_download_state(&row);
        let download_updates = Self::subscribe_to_download_updates(&ui, row.content_hash, cx);
        Self {
            book: row,
            focused: false,
            selected: false,
            marquee: false,
            states_author: true,
            detailed: false,
            cover_loaded_at: None,
            cover: CoverState::Pending,
            ui,
            open_detail: None,
            cover_task: None,
            cover_updates: Some(cover_updates),
            download_state,
            _download_updates: download_updates,
            download_task: None,
            source_directory,
            place_book: None,
            remove_book: None,
            context_menu: None,
            context_menu_subscription: None,
        }
    }

    pub(crate) fn content_hash(&self) -> sync_common::ContentHash {
        self.book.content_hash
    }

    /// Repaints only on a real change: the page pushes the cursor to every card
    /// on every move, so most of these calls are for cards that are already in
    /// the state being set.
    pub(crate) fn set_focused(&mut self, focused: bool, cx: &mut Context<Self>) {
        if self.focused == focused {
            return;
        }
        self.focused = focused;
        cx.notify();
    }

    pub(crate) fn set_selected(&mut self, selected: bool, cx: &mut Context<Self>) {
        if self.selected == selected {
            return;
        }
        self.selected = selected;
        cx.notify();
    }

    pub(crate) fn set_marquee(&mut self, marquee: bool, cx: &mut Context<Self>) {
        if self.marquee == marquee {
            return;
        }
        self.marquee = marquee;
        cx.notify();
    }

    /// Switches the card between its two modes. The paginator has to be told
    /// separately, because the cell's width is the grid's business — a card
    /// drawn detailed in an ordinary cell would have no room for the blurb.
    pub(crate) fn set_detailed(&mut self, detailed: bool, cx: &mut Context<Self>) {
        if self.detailed == detailed {
            return;
        }
        self.detailed = detailed;
        cx.notify();
    }

    pub(crate) fn replace_row(&mut self, row: BookCardRow, source_directory: Option<DirId>) {
        debug_assert_eq!(self.book.content_hash, row.content_hash);
        self.download_state = Self::row_download_state(&row);
        self.book = row;
        self.source_directory = source_directory;
    }

    fn row_download_state(row: &BookCardRow) -> bokheim_backend::DownloadState {
        if row.downloaded {
            bokheim_backend::DownloadState::Downloaded
        } else if row.download_requested {
            bokheim_backend::DownloadState::Queued
        } else {
            bokheim_backend::DownloadState::NotDownloaded
        }
    }

    fn download(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.download_state, bokheim_backend::DownloadState::NotDownloaded) || self.download_task.is_some() {
            return;
        }
        let content_hash = self.book.content_hash;
        self.download_state = bokheim_backend::DownloadState::Queued;
        let library = self.ui.backend().clone();
        let operation = async move { library.download_book(content_hash).await };
        self.download_task = Some(cx.spawn(async move |card, cx| {
            let result = operation.await;
            let _ = card.update_in(cx, |card, window, cx| {
                card.download_task.take();
                match result {
                    Ok(state) => card.download_state = state,
                    Err(error) => {
                        crate::services::notify_error("book-download-error", format!("Could not download book: {error}"), window, cx);
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

    fn subscribe_to_download_updates(ui: &LibraryContext, content_hash: sync_common::ContentHash, cx: &mut Context<Self>) -> Subscription {
        let updates = ui.updates();
        cx.subscribe(&updates, move |view, _, event: &LibraryDownloadChanged, cx| {
            if event.status.content_hash == content_hash {
                view.download_state = event.status.state.clone();
                cx.notify();
            }
        })
    }

    fn thumbnail_available(&mut self, cx: &mut Context<Self>) {
        if matches!(self.cover, CoverState::Loaded(..)) {
            return;
        }
        self.cover_updates.take();
        // The scanner has committed a thumbnail, so any in-flight or missing
        // result is stale.
        self.cover = CoverState::Pending;
        // A prior asynchronous lookup may still be about to report "missing".
        // Cancelling it ensures the next render starts a fresh read of the file
        // that the scanner has just committed.
        self.cover_task.take();
        cx.notify();
    }

    fn listen_for_thumbnail(&mut self, cx: &mut Context<Self>) {
        if self.cover_updates.is_some() {
            return;
        }
        self.cover_updates = Some(Self::subscribe_to_cover_updates(&self.ui, self.book.content_hash, cx));
    }

    fn load_cover(&mut self, resolution: bokheim_backend::ThumbnailResolution, cx: &mut Context<Self>) {
        let content_hash = self.book.content_hash;
        let library = self.ui.backend().clone();
        let load = async move { library.thumbnail(content_hash, resolution).await };
        let decoder = cx.background_executor().clone();
        self.cover_task = Some(cx.spawn(async move |view, cx| {
            let result = match load.await {
                Ok(Some(bytes)) => decoder.spawn(async move { loaded_cover_from_jpeg(bytes) }).await.map(Some).ok_or_else(|| format!("thumbnail for book {content_hash} is not a readable JPEG")),
                Ok(None) => Ok(None),
                Err(error) => Err(error),
            };
            let _ = view.update(cx, |view, cx| {
                view.cover_task.take();
                if view.book.content_hash != content_hash {
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
                        log::error!("failed to load thumbnail for book {content_hash}: {error}");
                        view.cover = CoverState::Missing;
                        view.listen_for_thumbnail(cx);
                    }
                }
                cx.notify();
            });
        }));
    }

    pub(crate) fn prepare(&mut self, resolution: bokheim_backend::ThumbnailResolution, cx: &mut Context<Self>) {
        let needs_cover = match &self.cover {
            CoverState::Pending => true,
            CoverState::Loaded(_, loaded_resolution) => *loaded_resolution == bokheim_backend::ThumbnailResolution::Browse && resolution == bokheim_backend::ThumbnailResolution::HighDensity,
            CoverState::Missing => false,
        };
        if needs_cover && self.cover_task.is_none() {
            self.load_cover(resolution, cx);
        }
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
        let (book, cover, open_detail, ui) = {
            let card = card.read(cx);
            let (book, cover) = card.detail_source();
            (book, cover, card.open_detail.clone(), card.ui.clone())
        };
        match open_detail {
            Some(open_detail) => open_detail(book, cover, cx),
            None => ui.open_book(book.content_hash, book.title, window, cx),
        }
    }

    fn open_context_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let new_window = crate::OpenInNewWindow(crate::NewWindowTarget::Book { locator: library_model::BookLocator::new(*self.ui.backend().id(), self.book.content_hash), title: self.book.title.clone() });
        let source_directory = self.source_directory;
        let content_hash = self.book.content_hash;
        let title = self.book.title.clone();
        let removal_title = title.clone();
        let removal_target = cx.entity();
        let place_book = self.place_book.clone();
        let remove_book = self.remove_book.clone();
        let menu = PopupMenu::build(window, cx, move |mut menu, _, _| {
            if crate::native_windows_available() {
                let action = new_window.clone();
                menu = menu
                    .item(components::menu_item("Open in New Window", move |_, window, cx| {
                        window.dispatch_action(Box::new(action.clone()), cx);
                    }))
                    .separator();
            }
            if let (Some(source_directory), Some(place_book)) = (source_directory, place_book) {
                let copy_book = place_book.clone();
                let move_book = place_book;
                let content_hash = content_hash;
                menu = menu
                    .item(components::menu_item_with_icon("Copy to folder…", IconName::Copy, move |_, _, cx| copy_book(content_hash, source_directory, BookPlacement::Copy, cx)))
                    .item(components::menu_item_with_icon("Move to folder…", IconName::FolderOpen, move |_, _, cx| move_book(content_hash, source_directory, BookPlacement::Move, cx)))
                    .separator();
            }
            if let (Some(source_directory), Some(remove_book)) = (source_directory, remove_book) {
                menu.item(components::menu_item_with_icon("Remove from this folder", components::navigation_icon(components::NavigationIcon::Trash2), move |_, window, cx| {
                    remove_book(content_hash, source_directory, format!("“{title}” remains available in its other folders. Its final placement moves to Trash."), window, cx);
                }))
            } else {
                menu.item(components::menu_item_with_icon("Move to Trash", components::navigation_icon(components::NavigationIcon::Trash2), move |_, window, cx| {
                    let choices = if source_directory.is_some() { ["Remove", "Cancel"] } else { ["Move to Trash", "Cancel"] };
                    let answer = window.prompt(
                        PromptLevel::Warning,
                        if source_directory.is_some() { "Remove this book from the folder?" } else { "Move this book to Trash?" },
                        Some(&format!("“{removal_title}” remains available in its other folders. Its final placement moves to Trash.")),
                        &choices,
                        cx,
                    );
                    removal_target.update(cx, |card, cx| {
                        let library = card.ui.backend().clone();
                        let operation = async move {
                            match source_directory {
                                Some(source_id) => library.remove_book_from_directory(content_hash, source_id).await.map(|_| ()),
                                None => library.move_book_to_trash(content_hash).await,
                            }
                        };
                        cx.spawn(async move |card, cx| {
                            if answer.await.ok() != Some(0) {
                                return;
                            }
                            if let Err(error) = operation.await {
                                let _ = card.update_in(cx, |_, window, cx| {
                                    crate::services::notify_error("book-trash-error", format!("Could not move book to Trash: {error}"), window, cx);
                                });
                            }
                        })
                        .detach();
                    });
                }))
            }
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

impl Render for BookCardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let highlighted = self.focused || self.selected;
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
        let element_id = format!("book-card-{}", book.content_hash);
        let cover_id = format!("book-cover-{}", book.content_hash);

        // A book with no author says nothing rather than saying it has none:
        // the line still holds its height, so the grid stays aligned.
        let author = book.author.clone();
        let open_target = cx.entity();
        let cover_frame = components::book_cover_frame(theme)
            .id(cover_id)
            .on_click(move |_, window, cx| Self::activate(&open_target, window, cx))
            .child(match cover {
                CoverState::Loaded(cover, _) => components::book_cover(theme).child(components::book_cover_image(format!("book-cover-art-{}", book.content_hash), cover.image, cover_opacity, highlighted, theme)),
                CoverState::Pending => components::book_cover(theme),
                CoverState::Missing => components::missing_book_cover(if book.format_category == library_model::LibraryFileTypeFilter::Audiobook { "Audiobook" } else { "No cover" }, theme),
            })
            .children(components::book_cover_progress(book.progress, theme))
            .when(book.format_category == library_model::LibraryFileTypeFilter::Audiobook, |frame| {
                frame.child(Tag::primary().rounded(px(components::RADIUS_SM)).with_size(Size::Small).child("AUDIOBOOK").absolute().left(gpui::px(8.0)).bottom(gpui::px(8.0)))
            });

        let context_menu = self.context_menu.as_ref().map(|active| deferred(anchored().position(active.position).snap_to_window_with_margin(px(8.0)).anchor(Anchor::TopLeft).child(active.menu.clone())).with_priority(1));
        let context_menu_target = cx.entity();
        let (downloaded, download_pending) = match self.download_state {
            bokheim_backend::DownloadState::Downloaded => (true, false),
            bokheim_backend::DownloadState::Queued | bokheim_backend::DownloadState::Downloading(_) => (false, true),
            bokheim_backend::DownloadState::NotDownloaded => (false, false),
        };
        let mut download_indicator = components::book_download_indicator(downloaded, download_pending, theme);
        if !downloaded && !download_pending {
            let download_target = cx.entity();
            download_indicator = download_indicator.map(|indicator| {
                gpui::div()
                    .id(format!("book-download-{}", book.content_hash))
                    .flex_none()
                    .cursor_pointer()
                    .tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("Download to this device").build(window, cx))
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        download_target.update(cx, |card, cx| card.download(cx));
                    })
                    .child(indicator)
                    .into_any_element()
            });
        }

        let card = components::book_card(element_id, highlighted, self.marquee, theme).on_aux_click(move |event, window, cx| {
            // A card inside a selection is not the subject of its own menu:
            // the page opens one over everything the band picked up.
            if event.is_secondary() && !context_menu_target.read(cx).selected {
                context_menu_target.update(cx, |card, cx| card.open_context_menu(event.position(), window, cx));
                cx.stop_propagation();
            }
        });

        // The two modes differ in where the cover stands, so each lays the card
        // out itself rather than sharing a stack the cover is threaded into.
        let card = if self.detailed {
            let description = components::book_card_description(format!("book-description-{}", book.content_hash), &book.description, theme);
            card.child(
                components::book_card_detail_body()
                    .child(components::book_card_detail_cover().child(cover_frame).child(
                        components::book_card_text().child(components::book_card_detail_title().child(components::book_title_lines(book.title, book.subtitle.as_deref()))).child(components::book_card_metadata(
                            author,
                            download_indicator,
                            theme,
                        )),
                    ))
                    .child(components::book_card_detail_text().children(description)),
            )
        } else if self.states_author {
            card.child(cover_frame).child(components::book_card_text().child(components::book_card_title().child(components::book_title_lines(book.title, book.subtitle.as_deref()))).child(components::book_card_metadata(
                author,
                download_indicator,
                theme,
            )))
        } else {
            card.child(cover_frame).child(components::book_card_text().child(components::book_card_title_row(book.title, book.subtitle.as_deref(), download_indicator)))
        };

        card.children(context_menu).into_any_element()
    }
}
