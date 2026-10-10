//! One book's page.
//!
//! Its owner keeps the browse view mounted in memory and caches this entity
//! when returning to it, preserving its contents and decoded cover.
//!
//! It opens already drawn. The card that was clicked holds the whole
//! `BookCardRow` and a decoded cover, and both are handed over at construction,
//! so the top of the page paints in the same frame as the click with the same
//! image the grid was showing. The request that follows fills in only what a
//! card never knew: the credits, the contents, and which entry
//! the reader is inside.

use crate::services::{BookConfirmation, notify_error, notify_info};
use library_backend::{DownloadState, ThumbnailResolution};
use std::collections::{HashMap, HashSet};

use gpui::prelude::*;
use gpui::{Anchor, App, AppContext, Context, Entity, FocusHandle, Focusable, IntoElement, MouseButton, Render, SharedString, Task, Window, div};
use gpui_component::IconName;
use gpui_component::menu::DropdownMenu as _;
use library_model::{BookCardRow, BookDetail, BookSource, BookTocEntry, CardKey, LibraryFileTypeFilter};
use sync_common::{ContentHash, DirId};
use ui_components as components;
use ui_components::ScrollableElement as _;

use crate::library::pages::with_retry_banner;
use crate::library::services::{LibraryContext, LibraryDownloadChanged};
use crate::library::widgets::{LoadedCover, cover_aspect_ratio, loaded_cover_from_jpeg, mark_feed_book_added};

/// Leaves a discarded book's page for the folder it was opened from. Owned by
/// the browse page, which is what knows how to leave the detail location.
pub(super) struct CloseBookDetail;

const BOOK_DETAIL_SIDEBAR_BREAKPOINT: f32 = 760.0;
const BOOK_DETAIL_THREE_COLUMN_BREAKPOINT: f32 = 1200.0;

pub(crate) struct BookDetailPage {
    ui: LibraryContext,
    /// What the grid knew, replaced by the request's own copy when it lands.
    /// The two agree about almost everything; the point of starting from the
    /// card is that there is no moment where the page has nothing to say.
    book: BookCardRow,
    detail: Option<BookDetail>,
    error: Option<SharedString>,
    cover: Option<LoadedCover>,
    /// Contents open collapsed, so a book with two hundred entries does not bury
    /// everything under it. This is what the reader has opened since.
    expanded: HashSet<String>,
    focus: FocusHandle,
    /// The load in flight. Replacing it drops the one before, which cancels it.
    request: Option<Task<()>>,
    _cover_task: Option<Task<()>>,
    download_requested_by_page: bool,
    /// Adding a catalog or feed book to the library, while it runs.
    add_task: Option<Task<()>>,
    _download_updates: gpui::Subscription,
}

impl gpui::EventEmitter<CloseBookDetail> for BookDetailPage {}

/// A feed book's page: what the feed says of it. A feed knows no contents,
/// credits or reading position.
fn feed_detail(book: BookCardRow, description: String) -> BookDetail {
    BookDetail { book, audiobook_chapter_count: None, description, source_directory_id: None, path: Vec::new(), authors: Vec::new(), credits: Vec::new(), current_entry: None, current_target: None, toc: Vec::new() }
}

impl BookDetailPage {
    /// The page of a library, catalog or feed book.
    pub(crate) fn create(book: BookCardRow, cover: Option<LoadedCover>, ui: LibraryContext, cx: &mut impl AppContext) -> Entity<Self> {
        cx.new(|cx| {
            // Only library books have a download state; catalog and feed books are added.
            if let Some(content_hash) = book.library_file() {
                ui.updates().update(cx, |updates, _| updates.track_download(content_hash, book.downloaded, book.download_requested));
            }
            let download_updates = {
                let updates = ui.updates();
                cx.subscribe(&updates, move |page: &mut Self, _, event: &LibraryDownloadChanged, cx| {
                    if Some(event.status.content_hash) == page.book.library_file() {
                        cx.notify();
                    }
                })
            };
            let mut page = Self { ui, book, detail: None, error: None, cover, expanded: HashSet::new(), focus: cx.focus_handle(), request: None, _cover_task: None, download_requested_by_page: false, add_task: None, _download_updates: download_updates };
            page.load(cx);
            // Only when the grid had not got to it yet: a card that is drawn is
            // a card whose cover is already decoded.
            if page.cover.is_none() {
                page.load_cover(cx);
            }
            page
        })
    }

    /// The book's identity, as its card has it.
    pub(super) fn key(&self) -> CardKey {
        self.book.key()
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        match self.book.source {
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
    }

    fn load_library(&mut self, content_hash: ContentHash, cx: &mut Context<Self>) {
        let library = self.ui.backend().clone();
        self.finish_load(async move { library.book_detail(content_hash).await }, cx);
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
                        if let (Some(content_hash), None) = (detail.book.library_file(), page.book.library_file()) {
                            page.ui.updates().update(cx, |updates, _| updates.track_download(content_hash, detail.book.downloaded, detail.book.download_requested));
                        }
                        page.book = detail.book.clone();
                        page.detail = Some(detail);
                        page.error = None;
                    }
                    Err(error) => page.error = Some(error.into()),
                }
                cx.notify();
            });
        });
        self.request = Some(task);
    }

    /// Adds the catalog or feed book to the library, then loads the library
    /// book's page in its place, with Read.
    fn add(&mut self, cx: &mut Context<Self>) {
        if self.add_task.is_some() {
            return;
        }
        let (source, title) = (self.book.source, self.book.title.clone());
        let library = self.ui.backend().clone();
        let app = self.ui.app().clone();
        self.add_task = Some(cx.spawn(async move |page, cx| {
            let result = match source {
                BookSource::Catalog { content_hash, .. } => library.add_catalog_book(sync_common::ROOT_DIR_ID, content_hash).await,
                // The app fetches from the feed; the library imports the file.
                BookSource::Opds { source, entry, .. } => match app.download_opds_book(source, entry).await {
                    Ok((file_name, bytes)) => library.import_downloaded_book(sync_common::ROOT_DIR_ID, file_name, bytes).await,
                    Err(error) => Err(error),
                },
                BookSource::Library { content_hash } => Ok(content_hash),
            };
            let _ = page.update_in(cx, |page, window, cx| {
                page.add_task.take();
                match result {
                    Ok(content_hash) => {
                        if matches!(source, BookSource::Opds { .. }) {
                            mark_feed_book_added(page.book.key(), cx);
                        }
                        notify_info("catalog-add", format!("Added “{title}” to the library"), window, cx);
                        page.load_library(content_hash, cx);
                    }
                    Err(error) => notify_error("catalog-add-error", format!("Could not add “{title}”: {error}"), window, cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// The page's own cover, at the resolution a page-sized cover needs rather
    /// than the one a card in a grid does.
    fn load_cover(&mut self, cx: &mut Context<Self>) {
        let title = self.book.title.clone();
        let library = self.ui.backend().clone();
        let app = self.ui.app().clone();
        let (source, content_hash) = (self.book.source, self.book.content_hash());
        let load = async move {
            match (source, content_hash) {
                (BookSource::Opds { source, entry, .. }, _) => app.opds_cover(source, entry, ThumbnailResolution::HighDensity.width()).await,
                (_, Some(content_hash)) => library.thumbnail(content_hash, ThumbnailResolution::HighDensity).await,
                (_, None) => Ok(None),
            }
        };
        let decoder = cx.background_executor().clone();
        self._cover_task = Some(cx.spawn(async move |page, cx| {
            let cover = match load.await {
                Ok(Some(bytes)) => decoder.spawn(async move { loaded_cover_from_jpeg(bytes) }).await,
                Ok(None) => None,
                Err(error) => {
                    log::error!("failed to load the cover for book {title:?}: {error}");
                    None
                }
            };
            let _ = page.update(cx, |page, cx| {
                page.cover = cover;
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
                    notify_error("book-download-error", format!("Could not download book: {error}"), window, cx);
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

    /// Runs a confirmed auxiliary command from the overflow menu. The
    /// operation reports whether the page's book is gone afterwards: a
    /// discarded page closes onto its folder, while a book that stays in the
    /// library reloads the detail instead.
    fn confirm_page_action(target: Entity<Self>, confirmation: BookConfirmation, detail: String, operation: impl std::future::Future<Output = Result<bool, String>> + 'static, window: &mut Window, cx: &mut App) {
        let answer = confirmation.prompt(&detail, window, cx);
        target.update(cx, |_, cx| {
            cx.spawn(async move |page, cx| {
                if !answer.await {
                    return;
                }
                match operation.await {
                    Ok(discard) => {
                        if discard {
                            let _ = page.update(cx, |_, cx| cx.emit(CloseBookDetail));
                        } else {
                            let _ = page.update(cx, |page, cx| page.load(cx));
                        }
                    }
                    Err(error) => {
                        let _ = page.update_in(cx, |_, window, cx| {
                            confirmation.report_failure(error, window, cx);
                        });
                    }
                }
            })
            .detach();
        });
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let page = cx.entity();
        let viewport_width = f32::from(window.viewport_size().width);
        let desktop_layout = viewport_width >= BOOK_DETAIL_SIDEBAR_BREAKPOINT;

        // The card's row until the request lands, then the request's own. There
        // is no frame in which the page has nothing to say about the book.
        let book = self.book.clone();
        let detail = self.detail.clone();

        let audiobook = book.format_category == LibraryFileTypeFilter::Audiobook;

        let cover = (if desktop_layout { components::book_detail_sidebar_cover_slot() } else { components::book_detail_cover_slot() })
            .child(match self.cover.clone() {
                Some(cover) => components::book_cover(theme, cover_aspect_ratio(&cover.image)).child(components::book_cover_image(format!("book-detail-cover-{}", book.key()), cover.image, 1.0, false, theme)),
                None => components::missing_book_cover(if audiobook { "Audiobook" } else { "No cover" }, theme),
            })
            // Nothing under an unopened book: the line beside it already says
            // "Not started", and an empty track says it again at length.
            .children(components::book_cover_progress(book.progress, theme));

        let mut headline = components::book_detail_headline().child(components::book_detail_title(book.title.clone(), book.subtitle.as_deref(), desktop_layout, theme));

        // The card knows one author line; the request knows who each of them is
        // and where their page lives. Until it lands the line is stated rather
        // than linked, so the byline neither appears late nor moves when it
        // becomes clickable.
        match detail.as_ref().filter(|detail| !detail.authors.is_empty()) {
            Some(detail) => {
                let mut byline = components::book_detail_credit_line(None::<SharedString>, theme);
                for author in &detail.authors {
                    let open = crate::OpenAuthor { author: author.id, name: author.name.clone() };
                    byline = byline.child(components::book_detail_name_link(format!("book-detail-author-{}", author.id), author.name.clone(), theme).on_click(move |_, window, cx| window.dispatch_action(Box::new(open.clone()), cx)));
                }
                headline = headline.child(byline);
            }
            None if !book.author.trim().is_empty() => {
                headline = headline.child(components::book_detail_credit_line(None::<SharedString>, theme).child(div().child(book.author.clone())));
            }
            None => {}
        }

        for credit in detail.iter().flat_map(|detail| &detail.credits) {
            let role = match credit.role.as_str() {
                "nrt" => "Narrated by",
                _ => continue,
            };
            headline = headline.child(components::book_detail_credit_line(Some(role), theme).child(components::book_detail_name_link(format!("book-detail-credit-{}-{}", credit.role, credit.id), credit.name.clone(), theme)));
        }

        // How long it runs and how many chapters, for a book where that is a
        // question. Nothing about the file — neither what kind it is nor when it
        // arrived, which are facts about the library rather than about the book.
        let mut facts = Vec::new();
        if let Some(duration) = book.audiobook_duration_ms {
            facts.push(SharedString::from(spoken_duration(duration)));
        }
        if let Some(chapters) = detail.as_ref().and_then(|detail| detail.audiobook_chapter_count) {
            facts.push(SharedString::from(format!("{chapters} chapters")));
        }
        if !facts.is_empty() {
            headline = headline.child(components::book_detail_facts(facts, theme));
        }

        // Where the reader is and what they can do about it, beside the cover
        // rather than under the hero: those two lines are what the page is for,
        // and a reader who opened it to carry on reading should not have to look
        // past the cover to find them.
        // `progress` is already a percentage, as the grid's bar has always read
        // it. Multiplying it by a hundred again put "2201%" beside the title of
        // a book a fifth of the way through.
        let percentage = (book.progress > 0.0).then(|| SharedString::from(format!("{}%", book.progress.round() as i32)));
        let position = components::book_detail_position(detail.as_ref().and_then(|detail| detail.current_entry.clone()).map(SharedString::from), percentage, theme);

        let download_state = book.library_file().and_then(|content_hash| self.ui.updates().read(cx).download_state(content_hash)).unwrap_or(DownloadState::Queued);
        let downloaded = matches!(download_state, DownloadState::Downloaded);
        let downloading = matches!(download_state, DownloadState::Queued | DownloadState::Downloading(_));
        let open_icon: gpui_component::Icon = if audiobook { components::Headphones.into() } else { IconName::BookOpen.into() };
        let action_padding = if desktop_layout { components::SPACE_SM } else { components::SPACE_MD };
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
        // Auxiliary commands live behind an overflow button rather than as
        // more buttons: reading and downloading are the page's work, while
        // discarding the download or the book itself is the exception.
        if let Some(content_hash) = book.library_file() {
            let trigger = components::outlined_icon_button("book-detail-more", "More actions", IconName::Ellipsis, theme);
            let action_page = page.clone();
            let title = book.title.clone();
            // The detail is not scoped to a folder, but one live placement can
            // still act as the source of a folder removal.
            let source = book.source_directory.or_else(|| detail.as_ref().and_then(|detail| detail.source_directory_id.as_deref()).and_then(|id| DirId::parse_str(id).ok()));
            let backend = self.ui.backend().clone();
            actions = actions.child(trigger.dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, _| {
                if downloaded {
                    let library = backend.clone();
                    let evict_page = action_page.clone();
                    menu = menu
                        .item(components::menu_item_with_icon(if cfg!(target_arch = "wasm32") { "Remove local copy" } else { "Remove download" }, IconName::HardDrive, move |_, _, cx| {
                            let library = library.clone();
                            let operation = async move { library.remove_local_book_copy(content_hash).await.map(|_| ()) };
                            evict_page.update(cx, |_, cx| {
                                cx.spawn(async move |page, cx| {
                                    if let Err(error) = operation.await {
                                        let _ = page.update_in(cx, |_, window, cx| {
                                            notify_error("book-evict-error", format!("Could not remove download: {error}"), window, cx);
                                        });
                                    }
                                })
                                .detach();
                            });
                        }))
                        .separator();
                }
                if let Some(source_id) = source {
                    let library = backend.clone();
                    let remove_page = action_page.clone();
                    let remove_detail = format!("“{title}” stays in its other folders. If this is its last folder, it moves to Trash.");
                    menu = menu.item(components::menu_item_with_icon("Remove from folder", components::navigation_icon(components::NavigationIcon::Folder), move |_, window, cx| {
                        let library = library.clone();
                        let operation = async move { library.remove_book_from_directory(content_hash, source_id).await };
                        Self::confirm_page_action(remove_page.clone(), BookConfirmation::RemoveFromFolder, remove_detail.clone(), operation, window, cx);
                    }));
                }
                let library = backend.clone();
                let trash_page = action_page.clone();
                let trash_detail = format!("“{title}” will be moved to Trash.");
                menu = menu.item(components::menu_item_with_icon("Move to Trash", components::navigation_icon(components::NavigationIcon::Trash2), move |_, window, cx| {
                    let library = library.clone();
                    let operation = async move { library.move_book_to_trash(content_hash).await.map(|_| true) };
                    Self::confirm_page_action(trash_page.clone(), BookConfirmation::TrashBook, trash_detail.clone(), operation, window, cx);
                }));
                menu
            }));
        }
        // A catalog or feed book the library does not hold has one action:
        // adding it. A feed book the feed only sells, lends or locks has none.
        match book.source {
            BookSource::Library { .. } => {}
            BookSource::Opds { downloadable: false, .. } => {
                actions = components::book_detail_actions();
                headline = headline.child(components::book_detail_facts(vec![SharedString::from("Not offered for download")], theme));
            }
            BookSource::Catalog { .. } | BookSource::Opds { downloadable: true, .. } => {
                actions = components::book_detail_actions().child(components::primary_action_button("book-detail-add", "Add to library", IconName::Plus, self.add_task.is_some()).px(gpui::px(action_padding)).on_click({
                    let page = page.clone();
                    move |_, _, cx| page.update(cx, |page, cx| page.add(cx))
                }));
            }
        }
        headline = headline.child(position).child(actions);
        if let DownloadState::Downloading(progress) = download_state
            && progress.value() > 0.0
        {
            headline = headline.child(components::progress_track(theme).w(gpui::px(240.0)).max_w_full().child(components::progress_fill(progress.value(), theme)));
        }

        // Empty contents and descriptions do not occupy a section.
        let contents = detail
            .as_ref()
            .filter(|detail| !detail.toc.is_empty())
            .map(|detail| components::book_detail_section().max_w(gpui::rems(components::BOOK_DETAIL_CONTENTS_WIDTH_REM)).child(self.render_contents(detail, audiobook, theme, cx)));
        // A description containing only markup is empty.
        let description = detail
            .as_ref()
            .and_then(|detail| components::book_detail_description(&detail.description, theme))
            .map(|description| components::book_detail_section().max_w(gpui::rems(components::BOOK_DETAIL_DESCRIPTION_WIDTH_REM)).child(description));

        let content = if desktop_layout && (contents.is_some() || description.is_some()) {
            let sidebar = components::book_detail_sidebar().child(cover).child(headline.flex_none());
            if viewport_width >= BOOK_DETAIL_THREE_COLUMN_BREAKPOINT && contents.is_some() && description.is_some() {
                components::book_detail_wide_measure()
                    .child(
                        components::book_detail_columns()
                            .child(sidebar.flex_1().w_0())
                            .child(components::book_detail_contents_column().child(contents.expect("three columns require contents")))
                            .child(components::book_detail_description_column().child(description.expect("three columns require description"))),
                    )
                    .into_any_element()
            } else {
                let mut reading = components::book_detail_description_column();
                if let Some(contents) = contents {
                    reading = reading.child(contents);
                }
                if let Some(description) = description {
                    reading = reading.child(description);
                }
                components::book_detail_medium_measure().child(components::book_detail_columns().child(sidebar).child(reading)).into_any_element()
            }
        } else {
            let hero = components::book_detail_hero().child(cover).child(headline);
            let mut sections = components::book_detail_sections();
            if let Some(contents) = contents {
                sections = sections.child(contents);
            }
            if let Some(description) = description {
                sections = sections.child(description);
            }
            let introduction = div().w_full().min_w_0().flex().flex_col().gap(gpui::px(components::SPACE_SM)).child(hero);
            components::book_detail_measure().child(introduction).child(sections).into_any_element()
        };
        let body = components::book_detail_page().track_focus(&self.focus).child(content).overflow_y_scrollbar().into_any_element();

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
    fn render_contents(&self, detail: &BookDetail, audiobook: bool, theme: components::BrowserTheme, cx: &mut Context<Self>) -> gpui::AnyElement {
        // The entry the reader is inside, and only that one: a parent is not
        // partly read, because a navigation document says where a section
        // starts and never how far into it anyone has got. Matched on the
        // target rather than the wording — two entries can be called the same
        // thing, and `current_entry` is the wording.
        let marked = detail.current_target.as_deref();
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
        let read_targets: HashSet<&str> =
            ordered.iter().position(|entry| marked == Some(entry.target.as_str())).map(|current| ordered[..current].iter().filter(|entry| entry.children.is_empty()).map(|entry| entry.target.as_str()).collect()).unwrap_or_default();

        let page = cx.entity();
        let mut panel = components::contents_panel(theme);
        for (index, DrawnEntry { entry, depth }) in drawn.into_iter().enumerate() {
            let row = components::ContentsRow {
                depth,
                has_children: !entry.children.is_empty(),
                expanded: self.expanded.contains(&entry.target),
                marked: marked == Some(entry.target.as_str()),
                read: read_targets.contains(entry.target.as_str()),
                focused: false,
            };
            // The chevron opens the entry and the row goes to it, so the two
            // clicks have to be separable. Attach the toggle directly to the
            // disclosure, and leave leaf entries transparent to the row click.
            let disclosure = components::contents_disclosure(format!("book-detail-disclosure-{index}"), row, theme).when(row.has_children, |disclosure| {
                let page = page.clone();
                let target = entry.target.clone();
                disclosure.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).on_click(move |_, _, cx| page.update(cx, |page, cx| page.toggle_entry(&target, cx)))
            });
            let amount = durations.get(entry.target.as_str()).cloned();
            let number = audiobook.then_some(index + 1);
            panel = panel.child(components::contents_row(format!("book-detail-entry-{index}"), row, disclosure, entry.title.clone(), number, amount, theme).on_click({
                let page = page.clone();
                let target = entry.target.clone();
                move |_, window, cx| page.update(cx, |page, cx| page.open(Some(target.clone()), window, cx))
            }));
        }
        panel.into_any_element()
    }
}

impl Focusable for BookDetailPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
