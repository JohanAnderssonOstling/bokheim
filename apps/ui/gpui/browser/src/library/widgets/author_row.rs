//! One author in the index, as a retained entity.
//!
//! The row draws the covers of the author's most recently read books, so it
//! owns their thumbnail reads the way a book card owns its own. Keeping the
//! row retained is what stops a scroll from re-reading the same JPEGs.

use std::rc::Rc;
use std::sync::Arc;

use epub_svelte_lib::AuthorId;
use epub_svelte_lib::library::AuthorSummary;
use gpui::prelude::*;
use gpui::{AppContext, Context, Entity, Image, ImageFormat, IntoElement, Render, SharedString, Task, Window};
use ui_components as components;

use crate::ActivateAuthor;
use crate::library::services::LibraryContext;

/// What the page does when a row is chosen. The row renders itself, so the
/// page hands it the one behaviour it owns rather than the row reaching back
/// into the page.
pub(crate) type SelectAuthor = Rc<dyn Fn(AuthorId, &mut gpui::App)>;

/// What is known about one of the row's covers. `Pending` and `Missing` are
/// kept apart for the same reason the book card keeps them apart: an empty
/// frame that later fills is calmer than a placeholder that pops.
#[derive(Clone)]
enum CoverState {
    Pending,
    Loaded(Arc<Image>),
    Missing,
}

pub(crate) struct AuthorRowView {
    author: AuthorSummary,
    ui: LibraryContext,
    covers: Vec<CoverState>,
    selected: bool,
    tab_index: isize,
    select: SelectAuthor,
    _cover_tasks: Vec<Task<()>>,
}

impl AuthorRowView {
    pub(crate) fn create<P: 'static>(author: AuthorSummary, ui: LibraryContext, tab_index: isize, select: SelectAuthor, cx: &mut Context<P>) -> Entity<Self> {
        cx.new(|_| Self { covers: vec![CoverState::Pending; author.cover_hashes().len()], author, ui, selected: false, tab_index, select, _cover_tasks: Vec::new() })
    }

    pub(crate) fn author(&self) -> &AuthorSummary {
        &self.author
    }

    pub(crate) fn set_selected(&mut self, selected: bool, cx: &mut Context<Self>) {
        if self.selected == selected {
            return;
        }
        self.selected = selected;
        cx.notify();
    }

    pub(crate) fn set_tab_index(&mut self, tab_index: isize, cx: &mut Context<Self>) {
        if self.tab_index == tab_index {
            return;
        }
        self.tab_index = tab_index;
        cx.notify();
    }

    fn load_covers(&mut self, cx: &mut Context<Self>) {
        self._cover_tasks = self
            .author
            .cover_hashes()
            .iter()
            .enumerate()
            .map(|(slot, content_hash)| {
                let load = self.ui.load_cover(content_hash.clone());
                cx.spawn(async move |view, cx| {
                    let result = load.await;
                    let _ = view.update(cx, |view, cx| {
                        let state = match result {
                            // The same JPEG check the book card makes: a
                            // thumbnail that is not one would otherwise reach
                            // the image decoder as an image of unknown format.
                            Ok(Some(bytes)) if bytes.starts_with(&[0xff, 0xd8, 0xff]) => CoverState::Loaded(Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes))),
                            Ok(_) => CoverState::Missing,
                            Err(error) => {
                                log::error!("failed to load author row thumbnail: {error}");
                                CoverState::Missing
                            }
                        };
                        if let Some(cover) = view.covers.get_mut(slot) {
                            *cover = state;
                        }
                        cx.notify();
                    });
                })
            })
            .collect();
    }
}

impl Render for AuthorRowView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self._cover_tasks.is_empty() && !self.author.cover_hashes().is_empty() && self.covers.iter().all(|cover| matches!(cover, CoverState::Pending)) {
            self.load_covers(cx);
        }
        let theme = components::browser_theme(cx);
        let author_id = self.author.id();

        // One frame per book the author actually has: a second slot for a book
        // that does not exist would draw an absence. The container keeps its
        // full width regardless, so the names still line up down the list.
        let covers = self.covers.iter().map(|cover| match cover {
            CoverState::Loaded(image) => components::author_row_cover_frame().child(components::author_row_cover_image(image.clone(), theme)).into_any_element(),
            // The book is there; its thumbnail is not, or not yet.
            CoverState::Pending => components::author_row_cover_frame().into_any_element(),
            CoverState::Missing => components::author_row_missing_cover(theme).into_any_element(),
        });

        let activate = self.select.clone();
        let click = self.select.clone();
        components::author_list_row(format!("author-{author_id}"), self.selected, theme)
            .tab_index(self.tab_index)
            .key_context("BrowserAuthorList")
            .child(components::author_row_covers().children(covers))
            .child(components::author_list_identity().child(components::author_list_name(SharedString::from(self.author.name().as_str().to_owned()))))
            .on_action(move |_: &ActivateAuthor, _, cx| activate(author_id, cx))
            .on_click(move |_, _, cx| click(author_id, cx))
    }
}
