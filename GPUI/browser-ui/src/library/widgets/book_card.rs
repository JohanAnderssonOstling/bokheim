//! One book, as a retained entity.
//!
//! Holds the typed book presentation, owns its cover request, and opens its
//! reader directly. Collection mutations are intentionally not exposed here.

use std::sync::Arc;

use bokheim_ui_components as components;
use epub_svelte_lib::library::BookCardRow;
use gpui::prelude::*;
use gpui::{Context, Image, ImageFormat, IntoElement, Render, Task, Window};

use crate::library::services::LibraryContext;

pub(crate) struct BookCardView {
    book: BookCardRow,
    cover: Option<Arc<Image>>,
    ui: LibraryContext,
    cover_task: Option<Task<()>>,
    show_progress: bool,
}

impl BookCardView {
    pub(crate) fn book_id(&self) -> epub_svelte_lib::BookId {
        self.book.book_id
    }

    pub(crate) fn new(row: BookCardRow, ui: LibraryContext, show_progress: bool, _cx: &mut Context<Self>) -> Self {
        Self { book: row, cover: None, ui, cover_task: None, show_progress }
    }

    pub(crate) fn set_book(&mut self, row: BookCardRow, show_progress: bool, cx: &mut Context<Self>) {
        if self.book == row && self.show_progress == show_progress {
            return;
        }
        if self.book.book_id != row.book_id {
            self.cover = None;
            self.cover_task.take();
        }
        self.book = row;
        self.show_progress = show_progress;
        cx.notify();
    }

    fn load_cover(&mut self, cx: &mut Context<Self>) {
        let book_id = self.book.book_id;
        let load = self.ui.load_cover(book_id);
        self.cover_task = Some(cx.spawn(async move |view, cx| {
            let result = load.await;
            let _ = view.update(cx, |view, cx| {
                view.cover_task.take();
                if view.book.book_id != book_id {
                    return;
                }
                match result {
                    Ok(Some(bytes)) if bytes.starts_with(&[0xff, 0xd8, 0xff]) => {
                        view.cover = Some(Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes)));
                        cx.notify();
                    }
                    Ok(Some(_)) => log::error!("thumbnail for book {book_id} is not JPEG"),
                    Ok(None) => {}
                    Err(error) => log::error!("failed to load thumbnail for book {book_id}: {error}"),
                }
            });
        }));
    }
}

impl Render for BookCardView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.cover.is_none() && self.cover_task.is_none() {
            self.load_cover(cx);
        }
        let theme = components::browser_theme(cx);
        let book = self.book.clone();
        let cover = self.cover.clone();
        let element_id = format!("book-card-{}", book.book_id);

        let open = {
            let book_id = book.book_id;
            let title = book.title.clone();
            let ui = self.ui.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
                ui.open_reader(book_id, title.clone(), cx);
            }
        };

        let has_cover = cover.is_some();
        let progress = (self.show_progress && book.progress > 0.0).then(|| book.progress.clamp(0.0, 100.0));
        let author = if book.author.is_empty() { "Unknown author" } else { &book.author };
        let cover_frame = components::book_cover_frame()
            .when_some(cover, |frame, cover| frame.child(components::book_cover(theme).child(components::book_cover_image(cover))))
            .when(!has_cover, |frame| frame.child(components::missing_book_cover(theme).child(if book.audiobook { "Audiobook" } else { "No cover" })))
            .when(book.audiobook, |frame| frame.child(components::audiobook_badge("AUDIOBOOK", theme).absolute().left(gpui::px(8.0)).bottom(gpui::px(8.0))));

        components::book_card(element_id)
            .child(cover_frame)
            .child(components::book_card_title().child(book.title))
            .child(components::book_card_author(theme).child(author.to_owned()))
            .child(components::book_progress(progress, theme))
            .on_click(open)
            .into_any_element()
    }
}
