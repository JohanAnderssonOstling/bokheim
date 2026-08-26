use std::io::Cursor;
use std::sync::Arc;

use app_contract::library::BookCardRow;
use gpui::prelude::*;
use gpui::{Anchor, AppContext, Context, DismissEvent, Entity, Focusable, Image, ImageFormat, IntoElement, Pixels, Point, PromptLevel, Render, RenderImage, Subscription, SvgRenderer, Task, Window, anchored, deferred, px};
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use gpui_component::notification::Notification;
use gpui_component::tag::Tag;
use gpui_component::{Sizable, Size, WindowExt};
use ui_components as components;

use crate::library::pages::browse::{FolderPicker, FolderPickerEvent};
use crate::library::services::{LibraryContext, LibraryCoversChanged};
use sync_common::DirId;

#[derive(Clone)]
pub(crate) struct LoadedCover {
    pub image: Arc<RenderImage>,
    pub aspect_ratio: f32,
}

pub(crate) fn loaded_cover_from_jpeg(bytes: Vec<u8>, svg_renderer: SvgRenderer) -> Option<LoadedCover> {
    if !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return None;
    }
    let (width, height) = image::io::Reader::with_format(Cursor::new(bytes.as_slice()), image::ImageFormat::Jpeg).into_dimensions().ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    let image = Image::from_bytes(ImageFormat::Jpeg, bytes).to_image_data(svg_renderer).ok()?;
    Some(LoadedCover { image, aspect_ratio: width as f32 / height as f32 })
}

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
    cover_updates: Option<Subscription>,
    source_directory: Option<DirId>,
    folder_picker: Option<Entity<FolderPicker>>,
    placement_action: Option<PlacementAction>,
    folder_picker_subscription: Option<Subscription>,
    context_menu: Option<BookCardContextMenu>,
    context_menu_subscription: Option<Subscription>,
}

struct BookCardContextMenu {
    menu: Entity<PopupMenu>,
    position: Point<Pixels>,
}

#[derive(Clone, Copy)]
enum PlacementAction {
    Copy,
    Move,
}

impl BookCardView {
    pub(crate) fn create(row: BookCardRow, ui: LibraryContext, cx: &mut impl AppContext) -> Entity<Self> {
        cx.new(|cx| Self::new(row, ui, None, cx))
    }

    pub(crate) fn create_in_folder(row: BookCardRow, ui: LibraryContext, source_directory: DirId, cx: &mut impl AppContext) -> Entity<Self> {
        cx.new(|cx| Self::new(row, ui, Some(source_directory), cx))
    }

    fn new(row: BookCardRow, ui: LibraryContext, source_directory: Option<DirId>, cx: &mut Context<Self>) -> Self {
        let cover_updates = Self::subscribe_to_cover_updates(&ui, row.content_hash, cx);
        Self {
            book: row,
            cover: CoverState::Pending,
            ui,
            cover_task: None,
            cover_updates: Some(cover_updates),
            source_directory,
            folder_picker: None,
            placement_action: None,
            folder_picker_subscription: None,
            context_menu: None,
            context_menu_subscription: None,
        }
    }

    pub(crate) fn content_hash(&self) -> sync_common::ContentHash {
        self.book.content_hash
    }

    pub(crate) fn replace_row(&mut self, row: BookCardRow, source_directory: Option<DirId>) {
        debug_assert_eq!(self.book.content_hash, row.content_hash);
        self.book = row;
        self.source_directory = source_directory;
    }

    fn choose_destination(&mut self, action: PlacementAction, cx: &mut Context<Self>) {
        let Some(source_directory) = self.source_directory else { return };
        let load = self.ui.backend().folder_destinations();
        cx.spawn(async move |card, cx| {
            let result = load.await;
            let _ = card.update(cx, |card, cx| match result {
                Ok(destinations) => {
                    let picker = cx.new(|_| FolderPicker::new(source_directory, destinations).disabling(source_directory));
                    card.folder_picker_subscription = Some(cx.subscribe(&picker, |card, _, event, cx| card.folder_destination_selected(*event, cx)));
                    card.folder_picker = Some(picker);
                    card.placement_action = Some(action);
                    cx.notify();
                }
                Err(error) => log::error!("failed to load folder destinations: {error}"),
            });
        })
        .detach();
    }

    fn folder_destination_selected(&mut self, event: FolderPickerEvent, cx: &mut Context<Self>) {
        self.folder_picker.take();
        self.folder_picker_subscription.take();
        let Some(action) = self.placement_action.take() else {
            cx.notify();
            return;
        };
        let FolderPickerEvent::Selected(destination_id) = event else {
            cx.notify();
            return;
        };
        let Some(source_id) = self.source_directory else { return };
        let content_hash = self.book.content_hash;
        let operation = match action {
            PlacementAction::Copy => self.ui.backend().copy_book_to_directory(content_hash, source_id, destination_id),
            PlacementAction::Move => self.ui.backend().move_book_to_directory(content_hash, source_id, destination_id),
        };
        cx.spawn(async move |card, cx| {
            let result = operation.await;
            let _ = card.update_in(cx, |_, window, cx| match result {
                Ok(_) => cx.notify(),
                Err(error) => window.push_notification(
                    Notification::error(format!(
                        "Could not {} book: {error}",
                        match action {
                            PlacementAction::Copy => "copy",
                            PlacementAction::Move => "move",
                        }
                    ))
                    .id1::<BookCardView>("book-placement-error")
                    .autohide(false),
                    cx,
                ),
            });
        })
        .detach();
    }

    fn subscribe_to_cover_updates(ui: &LibraryContext, content_hash: sync_common::ContentHash, cx: &mut Context<Self>) -> Subscription {
        let updates = ui.updates();
        cx.subscribe(&updates, move |view, _, event: &LibraryCoversChanged, cx| {
            if event.contains(&content_hash) {
                view.thumbnail_available(cx);
            }
        })
    }

    fn thumbnail_available(&mut self, cx: &mut Context<Self>) {
        if matches!(self.cover, CoverState::Loaded(_)) {
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

    fn load_cover(&mut self, cx: &mut Context<Self>) {
        let content_hash = self.book.content_hash;
        let load = self.ui.backend().load_cover(content_hash);
        let decoder = cx.background_executor().clone();
        let svg_renderer = cx.svg_renderer();
        self.cover_task = Some(cx.spawn(async move |view, cx| {
            let result = match load.await {
                Ok(Some(bytes)) => decoder.spawn(async move { loaded_cover_from_jpeg(bytes, svg_renderer) }).await.map(Some).ok_or_else(|| format!("thumbnail for book {content_hash} is not a readable JPEG")),
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
                        view.cover = CoverState::Loaded(cover);
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

    pub(crate) fn prepare(&mut self, cx: &mut Context<Self>) {
        if matches!(self.cover, CoverState::Pending) && self.cover_task.is_none() {
            self.load_cover(cx);
        }
    }

    pub(crate) fn is_prepared(&self) -> bool {
        !matches!(self.cover, CoverState::Pending)
    }

    pub(crate) fn open(&self, window: &mut Window, cx: &mut gpui::App) {
        self.ui.open_reader(self.book.content_hash, self.book.title.clone(), window, cx);
    }

    fn open_context_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let source_directory = self.source_directory;
        let content_hash = self.book.content_hash;
        let title = self.book.title.clone();
        let placement_target = cx.entity();
        let removal_target = cx.entity();
        let menu = PopupMenu::build(window, cx, move |mut menu, _, _| {
            if source_directory.is_some() {
                let copy_target = placement_target.clone();
                let move_target = placement_target.clone();
                menu = menu
                    .item(PopupMenuItem::new("Copy to folder…").on_click(move |_, _, cx| copy_target.update(cx, |card, cx| card.choose_destination(PlacementAction::Copy, cx))))
                    .item(PopupMenuItem::new("Move to folder…").on_click(move |_, _, cx| move_target.update(cx, |card, cx| card.choose_destination(PlacementAction::Move, cx))))
                    .separator();
            }
            menu.item(PopupMenuItem::new(if source_directory.is_some() { "Remove from this folder" } else { "Move to Trash" }).on_click(move |_, window, cx| {
                let choices = if source_directory.is_some() { ["Remove", "Cancel"] } else { ["Move to Trash", "Cancel"] };
                let answer = window.prompt(
                    PromptLevel::Warning,
                    if source_directory.is_some() { "Remove this book from the folder?" } else { "Move this book to Trash?" },
                    Some(&format!("“{title}” remains available in its other folders. Its final placement moves to Trash.")),
                    &choices,
                    cx,
                );
                removal_target.update(cx, |card, cx| {
                    let operation = match source_directory {
                        Some(source_id) => {
                            let operation = card.ui.backend().remove_book_from_directory(content_hash, source_id);
                            Box::pin(async move { operation.await.map(|_| ()) }) as app_contract::backend::BackendFuture<()>
                        }
                        None => card.ui.backend().trash_book(content_hash),
                    };
                    cx.spawn(async move |card, cx| {
                        if answer.await.ok() != Some(0) {
                            return;
                        }
                        if let Err(error) = operation.await {
                            let _ = card.update_in(cx, |_, window, cx| {
                                window.push_notification(Notification::error(format!("Could not move book to Trash: {error}")).id1::<BookCardView>("book-trash-error").autohide(false), cx);
                            });
                        }
                    })
                    .detach();
                });
            }))
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let book = self.book.clone();
        let cover = self.cover.clone();
        let element_id = format!("book-card-{}", book.content_hash);
        let cover_id = format!("book-cover-{}", book.content_hash);

        let author = if book.author.is_empty() { "Unknown author" } else { &book.author };
        let cover_frame = components::book_cover_frame()
            .id(cover_id)
            .child(match cover {
                CoverState::Loaded(cover) => components::book_cover(theme).child(components::book_cover_image(cover.image, theme)),
                CoverState::Pending => components::book_cover(theme),
                CoverState::Missing => components::missing_book_cover(if book.audiobook { "Audiobook" } else { "No cover" }, theme),
            })
            .when(book.audiobook, |frame| frame.child(Tag::primary().rounded(px(components::RADIUS_SM)).with_size(Size::Small).child("AUDIOBOOK").absolute().left(gpui::px(8.0)).bottom(gpui::px(8.0))));

        let context_menu = self.context_menu.as_ref().map(|active| deferred(anchored().position(active.position).snap_to_window_with_margin(px(8.0)).anchor(Anchor::TopLeft).child(active.menu.clone())).with_priority(1));
        let context_menu_target = cx.entity();

        components::book_card(element_id)
            .cursor_pointer()
            .on_aux_click(move |event, window, cx| {
                if event.is_secondary() {
                    context_menu_target.update(cx, |card, cx| card.open_context_menu(event.position(), window, cx));
                    cx.stop_propagation();
                }
            })
            .child(cover_frame)
            .child(components::book_card_text().child(components::book_card_title().child(book.title)).child(components::book_card_author(theme).child(author.to_owned())))
            .children(self.folder_picker.clone())
            .children(context_menu)
            .into_any_element()
    }
}
