//! Library-scoped pages and services.
//!
//! [`BrowserRoot`](crate::BrowserRoot) owns the selected library and current
//! flat page. Each page owns any book detail it opens, while folder and subject
//! pages own their own nested navigation.

use std::rc::Rc;

use gpui::{AnyElement, AppContext, Context, Entity, IntoElement};

mod detail;
mod pages;
mod services;
mod state;
mod title_filter;
mod widgets;

use self::detail::BookDetailPage;
use self::widgets::CoverInvalidationRegistry;

pub(crate) use self::pages::authors::AuthorsPage;
pub(crate) use self::pages::browse_next::{folder as new_folder_browse_page, store as new_store_browse_page, subject as new_subject_browse_page};
pub(crate) use self::pages::home::HomePage;
pub(crate) use self::pages::trash::TrashPage;
pub(crate) use self::services::LibraryContext;
pub(crate) use self::state::LibraryState;
pub(crate) use self::widgets::{LoadedCover, loaded_cover_from_jpeg};

pub(crate) fn invalidate_covers(library_id: epub_svelte_lib::LibraryId, content_hashes: &[epub_svelte_lib::ContentHash], cx: &mut gpui::App) {
    CoverInvalidationRegistry::invalidate(library_id, content_hashes, cx);
}

/// Implemented by pages which can replace their own contents with a book
/// detail. Keeping this here avoids duplicating the weak-entity callback
/// wiring in every page constructor.
pub(super) trait DetailOwner: Sized + 'static {
    fn detail(&self) -> Option<&Entity<BookDetailPage>>;
    fn detail_mut(&mut self) -> &mut Option<Entity<BookDetailPage>>;
}

pub(super) fn context_with_page_detail<P: DetailOwner>(mut context: LibraryContext, cx: &mut Context<P>) -> LibraryContext {
    let target = cx.entity().downgrade();
    let detail_context = context.clone();
    context.set_open_detail(Rc::new(move |content_hash, cx| {
        let target_for_back = target.clone();
        let detail_context = detail_context.clone();
        let _ = target.update(cx, move |page, cx| {
            let detail = cx.new(|cx| {
                BookDetailPage::new(
                    detail_context,
                    content_hash,
                    Rc::new(move |cx| {
                        let _ = target_for_back.update(cx, |page, cx| {
                            *page.detail_mut() = None;
                            cx.notify();
                        });
                    }),
                    cx,
                )
            });
            *page.detail_mut() = Some(detail);
            cx.notify();
        });
    }));
    context
}

pub(super) fn page_detail<P: DetailOwner>(page: &P) -> Option<AnyElement> {
    page.detail().cloned().map(IntoElement::into_any_element)
}
