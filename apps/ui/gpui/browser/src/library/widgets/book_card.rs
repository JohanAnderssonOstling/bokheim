//! One book, as a retained entity.
//!
//! Holds the typed book presentation, owns its cover request, and opens its
//! reader directly. Collection mutations are intentionally not exposed here.

use std::io::Cursor;
use std::rc::Rc;
use std::sync::Arc;

use epub_svelte_lib::library::BookCardRow;
use gpui::prelude::*;
use gpui::{AppContext, Context, Entity, Image, ImageFormat, IntoElement, Render, Task, Window, div, px};
use gpui_component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_component::progress::Progress;
use gpui_component::tag::Tag;
use gpui_component::{Sizable, Size};
use ui_components as components;

use app_contract::preferences::LibraryView;

use crate::library::services::LibraryContext;
use crate::library::widgets::{CoverInvalidationRegistry, LibraryDrag, LibraryDragPreview};

#[derive(Clone)]
pub(crate) struct LoadedCover {
    pub image: Arc<Image>,
    pub aspect_ratio: f32,
}

pub(crate) fn loaded_cover_from_jpeg(bytes: Vec<u8>) -> Option<LoadedCover> {
    if !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return None;
    }
    let (width, height) = image::io::Reader::with_format(Cursor::new(bytes.as_slice()), image::ImageFormat::Jpeg).into_dimensions().ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some(LoadedCover { image: Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes)), aspect_ratio: width as f32 / height as f32 })
}

#[derive(Clone)]
pub(crate) enum BookManagementAction {
    Download(epub_svelte_lib::ContentHash),
    RemoveDownload(epub_svelte_lib::ContentHash),
    ChooseCopyDestination(epub_svelte_lib::ContentHash),
    ChooseMoveDestination(epub_svelte_lib::ContentHash),
    RemoveFromFolder(epub_svelte_lib::ContentHash),
    MoveToTrash(epub_svelte_lib::ContentHash),
}

#[derive(Clone)]
pub(crate) struct BookManagement {
    pub on_action: Rc<dyn Fn(BookManagementAction, &mut gpui::App)>,
    pub source_id: epub_svelte_lib::DirId,
}

/// What is known about this book's cover art.
///
/// `Pending` and `Missing` used to be the same value — both `None` — so a card
/// that had not finished reading its thumbnail drew "No cover" and then popped
/// to the image once the JPEG resolved. That is what made a grid fill in
/// raggedly.
#[derive(Clone)]
enum CoverState {
    Pending,
    Loaded(LoadedCover),
    Missing,
}

pub(crate) struct BookCardView {
    book: BookCardRow,
    cover: CoverState,
    ui: LibraryContext,
    cover_task: Option<Task<()>>,
    show_progress: bool,
    /// Covers or detailed. Both presentations share this entity so that
    /// switching between them reuses the already-decoded cover rather than
    /// re-reading every thumbnail from disk.
    view: LibraryView,
    management: Option<BookManagement>,
}

impl BookCardView {
    pub(crate) fn create<P: 'static>(row: BookCardRow, ui: LibraryContext, show_progress: bool, library_view: LibraryView, cx: &mut Context<P>) -> Entity<Self> {
        let locator = epub_svelte_lib::BookLocator::new(ui.library.id().clone(), row.content_hash.clone());
        let view = cx.new(|cx| Self::new(row, ui, show_progress, library_view, cx));
        CoverInvalidationRegistry::register(locator, view.downgrade(), cx);
        view
    }

    fn new(row: BookCardRow, ui: LibraryContext, show_progress: bool, library_view: LibraryView, _cx: &mut Context<Self>) -> Self {
        Self { book: row, cover: CoverState::Pending, ui, cover_task: None, show_progress, view: library_view, management: None }
    }

    pub(crate) fn set_library_view(&mut self, library_view: LibraryView, cx: &mut Context<Self>) {
        if self.view == library_view {
            return;
        }
        self.view = library_view;
        cx.notify();
    }

    pub(crate) fn set_book(&mut self, row: BookCardRow, show_progress: bool, cx: &mut Context<Self>) {
        if self.book == row && self.show_progress == show_progress {
            return;
        }
        if self.book.content_hash != row.content_hash {
            self.cover = CoverState::Pending;
            self.cover_task.take();
        }
        self.book = row;
        self.show_progress = show_progress;
        cx.notify();
    }

    pub(crate) fn set_management(&mut self, management: Option<BookManagement>, cx: &mut Context<Self>) {
        self.management = management;
        cx.notify();
    }

    fn menu(_ui: LibraryContext, management: Option<BookManagement>, downloaded: bool, hash: epub_svelte_lib::ContentHash, mut menu: PopupMenu, _window: &mut Window, _cx: &mut Context<PopupMenu>) -> PopupMenu {
        #[cfg(not(feature = "kobo"))]
        if downloaded {
            let reveal_hash = hash.clone();
            menu = menu.item(PopupMenuItem::new("Show in folder").on_click(move |_, _, cx| {
                let reveal = _ui.reveal_local_book(reveal_hash.clone());
                cx.background_spawn(async move {
                    if let Err(error) = reveal.await {
                        log::error!("failed to show local book in file manager: {error}");
                    }
                })
                .detach();
            }));
        }

        let Some(management) = management else { return menu };
        #[cfg(not(feature = "kobo"))]
        if downloaded {
            menu = menu.separator();
        }
        let callback = management.on_action.clone();
        let download_action = if downloaded { BookManagementAction::RemoveDownload(hash.clone()) } else { BookManagementAction::Download(hash.clone()) };
        let download_label = if downloaded { "Remove download" } else { "Download" };
        menu = menu.item(PopupMenuItem::new(download_label).on_click(move |_, _, cx| callback(download_action.clone(), cx)));

        let copy_callback = management.on_action.clone();
        let copy_hash = hash.clone();
        menu = menu.item(PopupMenuItem::new("Copy to folder…").on_click(move |_, _, cx| copy_callback(BookManagementAction::ChooseCopyDestination(copy_hash.clone()), cx)));

        let move_callback = management.on_action.clone();
        let move_hash = hash.clone();
        menu = menu.item(PopupMenuItem::new("Move to folder…").on_click(move |_, _, cx| move_callback(BookManagementAction::ChooseMoveDestination(move_hash.clone()), cx)));
        let remove_callback = management.on_action.clone();
        let remove_hash = hash.clone();
        let trash_callback = management.on_action;
        menu.separator()
            .item(PopupMenuItem::new("Remove from folder").on_click(move |_, _, cx| remove_callback(BookManagementAction::RemoveFromFolder(remove_hash.clone()), cx)))
            .item(PopupMenuItem::new("Move to Trash").on_click(move |_, _, cx| trash_callback(BookManagementAction::MoveToTrash(hash.clone()), cx)))
    }

    pub(crate) fn thumbnail_available(&mut self, cx: &mut Context<Self>) {
        if matches!(self.cover, CoverState::Loaded(_)) {
            return;
        }
        // The scanner has just committed a thumbnail for a book previously
        // known to have none, so the earlier answer is stale.
        self.cover = CoverState::Pending;
        // A prior asynchronous lookup may still be about to report "missing".
        // Cancelling it ensures the next render starts a fresh read of the file
        // that the scanner has just committed.
        self.cover_task.take();
        cx.notify();
    }

    fn load_cover(&mut self, cx: &mut Context<Self>) {
        let content_hash = self.book.content_hash.clone();
        let load = self.ui.load_cover(content_hash.clone());
        self.cover_task = Some(cx.spawn(async move |view, cx| {
            let result = load.await;
            let _ = view.update(cx, |view, cx| {
                view.cover_task.take();
                if view.book.content_hash != content_hash {
                    return;
                }
                match result {
                    Ok(Some(bytes)) => {
                        let Some(cover) = loaded_cover_from_jpeg(bytes) else {
                            log::error!("thumbnail for book {content_hash} is not a readable JPEG");
                            view.cover = CoverState::Missing;
                            cx.notify();
                            return;
                        };
                        view.cover = CoverState::Loaded(cover);
                    }
                    Ok(None) => view.cover = CoverState::Missing,
                    Err(error) => {
                        log::error!("failed to load thumbnail for book {content_hash}: {error}");
                        view.cover = CoverState::Missing;
                    }
                }
                cx.notify();
            });
        }));
    }
}

impl BookCardView {
    /// The book as a full-width row: art beside the text, a blurb, and actions.
    fn render_detailed(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let book = self.book.clone();
        let cover = self.cover.clone();
        let author = if book.author.is_empty() { "Unknown author" } else { &book.author };
        let progress = (self.show_progress && book.progress > 0.0).then(|| book.progress.clamp(0.0, 100.0));

        let open = {
            let content_hash = book.content_hash.clone();
            let title = book.title.clone();
            let ui = self.ui.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
                let _ = &title;
                ui.open_detail(content_hash.clone(), cx);
            }
        };

        let cover_frame = match cover {
            CoverState::Loaded(cover) => components::book_row_cover(theme).child(components::book_row_cover_image(cover.image)),
            // The empty mat is the loading state: nothing to animate, and it
            // occupies exactly the space the art will.
            CoverState::Pending => components::book_row_cover(theme),
            CoverState::Missing => components::book_row_cover(theme).child(components::missing_cover_mark(if book.audiobook { "Audiobook" } else { "No cover" }, theme)),
        };

        let mut text = components::book_row_text().child(components::book_row_title().child(book.title.clone())).child(components::book_row_author(theme).child(author.to_owned()));
        if !book.description.is_empty() {
            text = text.child(components::book_row_description(theme).child(book.description.clone()));
        }
        if let Some(progress) = progress {
            text = text.child(div().w_full().h(px(16.0)).flex_none().child(Progress::new(format!("book-row-progress-{}", book.content_hash)).value(progress).color(theme.accent).with_size(Size::XSmall).w_full()));
        }

        // Actions only earn their place once the row is wide enough to hold
        // them beside a full-measure text column.
        let available = f32::from(window.viewport_size().width) - components::nav_rail_width(f32::from(window.rem_size())) - components::CONTENT_PADDING * 2.0;
        let actions = (available >= components::DETAILED_ROW_ACTIONS_BREAKPOINT).then(|| {
            let labels = &self.ui.contracts().book_actions;
            components::book_row_actions().child(components::topbar_action_button(format!("book-row-open-{}", book.content_hash), labels.open_label.clone(), gpui_component::IconName::BookOpen, theme).on_click(open.clone()))
        });

        let mut row = components::book_row(format!("book-row-{}", book.content_hash), theme)
            .child(div().flex_none().id(format!("book-row-cover-{}", book.content_hash)).child(cover_frame).on_click(open))
            .child(text)
            .child(div().flex_1().min_w_0())
            .children(actions);
        if let Some(management) = self.management.clone() {
            let downloaded = self.book.downloaded;
            let hash = self.book.content_hash.clone();
            let drag = LibraryDrag::Book { content_hash: hash.clone(), source_id: management.source_id, title: self.book.title.clone() };
            row = row.cursor_move().on_drag(drag, |item, _, _, cx| cx.new(|_| LibraryDragPreview::new(item.clone())));
            let ui = self.ui.clone();
            row.context_menu(move |menu, window, cx| Self::menu(ui.clone(), Some(management.clone()), downloaded, hash.clone(), menu, window, cx)).into_any_element()
        } else if cfg!(not(feature = "kobo")) && self.book.downloaded {
            let hash = self.book.content_hash.clone();
            let ui = self.ui.clone();
            row.context_menu(move |menu, window, cx| Self::menu(ui.clone(), None, true, hash.clone(), menu, window, cx)).into_any_element()
        } else {
            row.into_any_element()
        }
    }
}

impl Render for BookCardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if matches!(self.cover, CoverState::Pending) && self.cover_task.is_none() {
            self.load_cover(cx);
        }
        if self.view == LibraryView::Detailed {
            return self.render_detailed(window, cx);
        }
        let theme = components::browser_theme(cx);
        let book = self.book.clone();
        let cover = self.cover.clone();
        let element_id = format!("book-card-{}", book.content_hash);
        let cover_id = format!("book-cover-{}", book.content_hash);
        let progress_id = format!("book-progress-{}", book.content_hash);

        let open = {
            let content_hash = book.content_hash.clone();
            let title = book.title.clone();
            let ui = self.ui.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
                let _ = &title;
                ui.open_detail(content_hash.clone(), cx);
            }
        };

        let progress = (self.show_progress && book.progress > 0.0).then(|| book.progress.clamp(0.0, 100.0));
        let author = if book.author.is_empty() { "Unknown author" } else { &book.author };
        let cover_frame = components::book_cover_frame()
            .id(cover_id)
            .child(match cover {
                CoverState::Loaded(cover) => components::book_cover(theme).child(components::book_cover_image(cover.image, cover.aspect_ratio, theme)),
                CoverState::Pending => components::book_cover(theme),
                CoverState::Missing => components::missing_book_cover(if book.audiobook { "Audiobook" } else { "No cover" }, theme),
            })
            .when(book.audiobook, |frame| frame.child(Tag::primary().rounded(px(components::RADIUS_SM)).with_size(Size::Small).child("AUDIOBOOK").absolute().left(gpui::px(8.0)).bottom(gpui::px(8.0))))
            .on_click(open);

        let progress = div()
            .w_full()
            .min_w(px(components::BOOK_COVER_WIDTH))
            .h(px(components::BOOK_CARD_PROGRESS_HEIGHT))
            .flex_none()
            .pt(px(6.0))
            .when_some(progress, |track, progress| track.child(Progress::new(progress_id).value(progress).color(theme.accent).with_size(Size::XSmall).w_full()));

        let mut card =
            components::book_card(element_id).child(cover_frame).child(components::book_card_text().child(components::book_card_title().child(book.title)).child(components::book_card_author(theme).child(author.to_owned())).child(progress));
        if let Some(management) = self.management.clone() {
            let downloaded = self.book.downloaded;
            let hash = self.book.content_hash.clone();
            let drag = LibraryDrag::Book { content_hash: hash.clone(), source_id: management.source_id, title: self.book.title.clone() };
            card = card.cursor_move().on_drag(drag, |item, _, _, cx| cx.new(|_| LibraryDragPreview::new(item.clone())));
            let ui = self.ui.clone();
            card.context_menu(move |menu, window, cx| Self::menu(ui.clone(), Some(management.clone()), downloaded, hash.clone(), menu, window, cx)).into_any_element()
        } else if cfg!(not(feature = "kobo")) && self.book.downloaded {
            let hash = self.book.content_hash.clone();
            let ui = self.ui.clone();
            card.context_menu(move |menu, window, cx| Self::menu(ui.clone(), None, true, hash.clone(), menu, window, cx)).into_any_element()
        } else {
            card.into_any_element()
        }
    }
}
