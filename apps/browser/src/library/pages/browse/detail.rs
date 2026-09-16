//! One book's page.
//!
//! A location like any other: the browse page navigates to `book:<hash>`, the
//! crumb for it is the title, and leaving is the same `open_path` that leaves a
//! folder. Nothing here knows about Back, because nothing here has to.
//!
//! It opens already drawn. The card that was clicked holds the whole
//! `BookCardRow` and a decoded cover, and both are handed over at construction,
//! so the top of the page paints in the same frame as the click with the same
//! image the grid was showing. The request that follows fills in only what a
//! card never knew: the credits, the subjects, the contents, and which entry
//! the reader is inside.

use std::collections::{HashMap, HashSet};

use gpui::prelude::*;
use gpui::{App, AppContext, Context, Entity, FocusHandle, Focusable, IntoElement, Render, SharedString, Task, Window, div};
use gpui_component::IconName;
use library_model::{BookCardRow, BookDetail, BookTocEntry, LibraryFileTypeFilter};
use sync_common::ContentHash;
use ui_components as components;
use ui_components::ScrollableElement as _;

use crate::library::services::{LatestRequest, LibraryContext, LibraryDownloadChanged};
use crate::library::widgets::{LoadedCover, loaded_cover_from_jpeg};

pub(crate) struct BookDetailPage {
    ui: LibraryContext,
    content_hash: ContentHash,
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
    download_state: bokheim_backend::DownloadState,
    focus: FocusHandle,
    request: LatestRequest,
    _cover_task: Option<Task<()>>,
    _download_updates: gpui::Subscription,
}

impl BookDetailPage {
    pub(crate) fn create(book: BookCardRow, cover: Option<LoadedCover>, ui: LibraryContext, cx: &mut impl AppContext) -> Entity<Self> {
        let content_hash = book.content_hash;
        cx.new(|cx| {
            let download_updates = {
                let updates = ui.updates();
                cx.subscribe(&updates, move |page: &mut Self, _, event: &LibraryDownloadChanged, cx| {
                    if event.status.content_hash == content_hash {
                        page.download_state = event.status.state.clone();
                        cx.notify();
                    }
                })
            };
            let download_state = download_state_of(&book);
            let mut page = Self {
                ui,
                content_hash,
                book,
                detail: None,
                error: None,
                cover,
                expanded: HashSet::new(),
                download_state,
                focus: cx.focus_handle(),
                request: LatestRequest::default(),
                _cover_task: None,
                _download_updates: download_updates,
            };
            page.load(cx);
            // Only when the grid had not got to it yet: a card that is drawn is
            // a card whose cover is already decoded.
            if page.cover.is_none() {
                page.load_cover(cx);
            }
            page
        })
    }

    pub(super) fn content_hash(&self) -> ContentHash {
        self.content_hash
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let generation = self.request.begin();
        let content_hash = self.content_hash;
        let library = self.ui.backend().clone();
        let load = async move { library.book_detail(content_hash).await };
        let task = cx.spawn(async move |page, cx| {
            let result = load.await;
            let _ = page.update(cx, |page, cx| {
                if !page.request.is_current(generation) {
                    return;
                }
                match result {
                    Ok(detail) => {
                        page.download_state = download_state_of(&detail.book);
                        page.book = detail.book.clone();
                        page.detail = Some(detail);
                        page.error = None;
                    }
                    Err(error) => page.error = Some(error.into()),
                }
                cx.notify();
            });
        });
        self.request.install(task);
    }

    /// The page's own cover, at the resolution a page-sized cover needs rather
    /// than the one a card in a grid does.
    fn load_cover(&mut self, cx: &mut Context<Self>) {
        let content_hash = self.content_hash;
        let library = self.ui.backend().clone();
        let load = async move { library.thumbnail(content_hash, bokheim_backend::ThumbnailResolution::HighDensity).await };
        let decoder = cx.background_executor().clone();
        self._cover_task = Some(cx.spawn(async move |page, cx| {
            let cover = match load.await {
                Ok(Some(bytes)) => decoder.spawn(async move { loaded_cover_from_jpeg(bytes) }).await,
                Ok(None) => None,
                Err(error) => {
                    log::error!("failed to load the cover for book {content_hash}: {error}");
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
        self.ui.open_book_at(self.content_hash, self.book.title.clone(), target, window, cx);
    }

    fn download(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.download_state, bokheim_backend::DownloadState::NotDownloaded) {
            return;
        }
        self.download_state = bokheim_backend::DownloadState::Queued;
        let content_hash = self.content_hash;
        let library = self.ui.backend().clone();
        cx.spawn(async move |page, cx| {
            let result = library.download_book(content_hash).await;
            let _ = page.update_in(cx, |page, window, cx| {
                match result {
                    Ok(state) => page.download_state = state,
                    Err(error) => {
                        page.download_state = bokheim_backend::DownloadState::NotDownloaded;
                        crate::services::notify_error("book-download-error", format!("Could not download book: {error}"), window, cx);
                    }
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

fn download_state_of(book: &BookCardRow) -> bokheim_backend::DownloadState {
    if book.downloaded {
        bokheim_backend::DownloadState::Downloaded
    } else if book.download_requested {
        bokheim_backend::DownloadState::Queued
    } else {
        bokheim_backend::DownloadState::NotDownloaded
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
    let positions = entries.iter().map(|entry| library_model::audiobook_toc_position(&entry.target)).collect::<Vec<_>>();
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let page = cx.entity();

        // The card's row until the request lands, then the request's own. There
        // is no frame in which the page has nothing to say about the book.
        let book = self.book.clone();
        let detail = self.detail.clone();

        let audiobook = book.format_category == LibraryFileTypeFilter::Audiobook;

        let cover = components::book_detail_cover_slot()
            .child(match self.cover.clone() {
                Some(cover) => components::book_cover(theme).child(components::book_cover_image(format!("book-detail-cover-{}", self.content_hash), cover.image, 1.0, false, theme)),
                None => components::missing_book_cover(if audiobook { "Audiobook" } else { "No cover" }, theme),
            })
            // Nothing under an unopened book: the line beside it already says
            // "Not started", and an empty track says it again at length.
            .children(components::book_cover_progress(book.progress, theme));

        let mut headline = components::book_detail_headline().child(components::book_detail_title(book.title.clone(), book.subtitle.as_deref(), theme));

        // The card knows one author line; the request knows who each of them is
        // and where their page lives. Until it lands the line is stated rather
        // than linked, so the byline neither appears late nor moves when it
        // becomes clickable.
        match detail.as_ref().filter(|detail| !detail.authors.is_empty()) {
            Some(detail) => {
                let mut byline = components::book_detail_credit_line(None::<SharedString>, theme);
                for author in &detail.authors {
                    byline = byline.child(components::book_detail_name_link(format!("book-detail-author-{}", author.id), author.name.clone(), theme));
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
                "trl" => "Translated by",
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
        if let Some(chapters) = book.audiobook_chapter_count {
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

        let downloaded = matches!(self.download_state, bokheim_backend::DownloadState::Downloaded);
        let downloading = matches!(self.download_state, bokheim_backend::DownloadState::Queued | bokheim_backend::DownloadState::Downloading(_));
        let mut actions = components::book_detail_actions().child(components::primary_action_button("book-detail-open", if audiobook { "Listen" } else { "Read" }, IconName::BookOpen, false).on_click({
            let page = page.clone();
            move |_, window, cx| page.update(cx, |page, cx| page.open(None, window, cx))
        }));
        if !downloaded {
            let label = if downloading { "Downloading…" } else { "Download" };
            actions = actions.child(components::secondary_action_button("book-detail-download", label, IconName::ArrowDown, downloading).on_click({
                let page = page.clone();
                move |_, _, cx| page.update(cx, |page, cx| page.download(cx))
            }));
        }
        headline = headline.child(position).child(actions);

        // The chips close the block: they are where the reader goes *next*, not
        // what they came here to do.
        let subject_paths = detail.as_ref().map(|detail| detail.subject_paths.as_slice()).unwrap_or_default();
        if !subject_paths.is_empty() {
            let mut subjects = components::book_detail_subjects();
            for path in subject_paths {
                let mut segments = path.rsplit(" / ");
                let leaf = segments.next().unwrap_or(path.as_str());
                let parent = segments.next().map(|parent| SharedString::from(parent.to_owned()));
                subjects = subjects.child(components::subject_chip(parent, SharedString::from(leaf.to_owned()), theme));
            }
            headline = headline.child(subjects);
        }

        // What is in the book before what the book is: a reader who has opened
        // this page has already decided to read it. A section with nothing in it
        // is left out rather than headed and empty — an absent blurb is the
        // publisher's silence, and saying so takes more room than it is worth.
        let mut sections = components::book_detail_sections();
        if let Some(detail) = detail.as_ref().filter(|detail| !detail.toc.is_empty()) {
            sections = sections.child(components::book_detail_section().child(components::book_detail_heading("Contents", theme)).child(self.render_contents(detail, audiobook, theme, cx)));
        }
        // Asked of the words rather than of the markup: a description that is
        // only tags has nothing to head.
        if let Some(description) = components::book_detail_description(&book.description, theme) {
            sections = sections.child(components::book_detail_section().child(components::book_detail_heading("Description", theme)).child(description));
        }

        let body =
            components::book_detail_page().track_focus(&self.focus).child(components::book_detail_measure().child(components::book_detail_hero().child(cover).child(headline)).child(sections)).overflow_y_scrollbar().into_any_element();

        // A failed request loses the credits, the subjects and the contents —
        // not the book. Everything the card already knew is on screen and stays
        // there, with the banner over it saying what is missing.
        match self.error.clone() {
            Some(error) => {
                let retry = components::topbar_action_button("retry-book-detail", "Retry", IconName::Redo2, theme).on_click({
                    let page = page.clone();
                    move |_, _, cx| page.update(cx, |page, cx| page.load(cx))
                });
                components::browser_error_banner(error, retry, body).into_any_element()
            }
            None => body,
        }
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

        let page = cx.entity();
        let mut panel = components::contents_panel(theme);
        for (index, DrawnEntry { entry, depth }) in drawn.into_iter().enumerate() {
            let row = components::ContentsRow { depth, has_children: !entry.children.is_empty(), expanded: self.expanded.contains(&entry.target), marked: marked == Some(entry.target.as_str()), focused: false };
            // The chevron opens the entry and the row goes to it, so the two
            // clicks have to be separable — which is why the toggle is handed to
            // the disclosure rather than wrapped around it.
            let disclosure = components::contents_toggle_disclosure(format!("book-detail-disclosure-{index}"), row, theme, {
                let page = page.clone();
                let target = entry.target.clone();
                move |_, cx| page.update(cx, |page, cx| page.toggle_entry(&target, cx))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, target: &str, children: Vec<BookTocEntry>) -> BookTocEntry {
        BookTocEntry { title: title.to_owned(), target: target.to_owned(), children }
    }

    #[test]
    fn contents_open_collapsed_and_reveal_only_what_was_opened() {
        let toc = vec![entry("Part One", "p1", vec![entry("Chapter 1", "c1", Vec::new()), entry("Chapter 2", "c2", Vec::new())]), entry("Part Two", "p2", vec![entry("Chapter 3", "c3", Vec::new())])];

        let mut drawn = Vec::new();
        drawn_entries(&toc, &HashSet::new(), 0, &mut drawn);
        assert_eq!(drawn.iter().map(|drawn| drawn.entry.target.as_str()).collect::<Vec<_>>(), ["p1", "p2"], "a book with two hundred entries opens showing its parts");

        let expanded = HashSet::from(["p1".to_owned()]);
        let mut drawn = Vec::new();
        drawn_entries(&toc, &expanded, 0, &mut drawn);
        assert_eq!(drawn.iter().map(|drawn| (drawn.entry.target.as_str(), drawn.depth)).collect::<Vec<_>>(), [("p1", 0), ("c1", 1), ("c2", 1), ("p2", 0)]);
    }

    #[test]
    fn a_chapter_runs_until_the_next_one_starts() {
        let toc =
            vec![entry("One", &library_model::audiobook_toc_target(0), Vec::new()), entry("Two", &library_model::audiobook_toc_target(3_600_000), Vec::new()), entry("Three", &library_model::audiobook_toc_target(5_400_000), Vec::new())];
        let durations = chapter_durations(&toc, Some(9_000_000));
        assert_eq!(durations[library_model::audiobook_toc_target(0).as_str()], SharedString::from("1 h"));
        assert_eq!(durations[library_model::audiobook_toc_target(3_600_000).as_str()], SharedString::from("30 min"));
        assert_eq!(durations[library_model::audiobook_toc_target(5_400_000).as_str()], SharedString::from("1 h"), "the last chapter runs to the end of the book");
    }

    #[test]
    fn a_documents_entries_carry_no_duration() {
        let toc = vec![entry("Chapter 1", "text/ch1.xhtml", Vec::new())];
        assert!(chapter_durations(&toc, None).is_empty(), "an EPUB href says nothing about how long anything takes");
    }

    #[test]
    fn durations_are_spoken_rather_than_clocked() {
        assert_eq!(spoken_duration(45_000), "1 min", "a chapter that plays is never no time at all");
        assert_eq!(spoken_duration(90_000), "1 min");
        assert_eq!(spoken_duration(3_600_000), "1 h");
        assert_eq!(spoken_duration(45_660_000), "12 h 41 min");
    }
}
