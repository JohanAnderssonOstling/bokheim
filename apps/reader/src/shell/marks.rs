//! What the reader has annotated in a book.
//!
//! The library owns the durable copy; this is the in-memory mirror kept in
//! step with it. Every reader that can annotate keeps the same mirror, so it
//! lives here rather than beside one of them. How a mark is anchored, and how
//! it becomes something painted over the page, is the reader's own business.

use ui_text::text;

use std::collections::HashSet;
use std::rc::Rc;

use async_channel::{Receiver, Sender};
use book_model::{AnnotationStyle, RecurringScope};
use content_address::BookId;
use gpui::{App, Context, Task};
use library_backend::{Annotation, LibraryClient, LibraryUpdate};
use library_model::RecurringAnnotation;

use crate::shell::persistence::{MutationWriter, PersistenceSubject};
use crate::shell::recurring::RecurringMatcher;
use html_pipeline::engine::TextPatternMatch;

#[derive(Default)]
struct MarksState {
    annotations: Vec<Annotation>,
    bookmarks: Vec<Annotation>,
    draft_bookmark: Option<Annotation>,
    /// The annotation whose note editor is open, if any.
    selected_id: Option<String>,
    deleted_annotation_ids: HashSet<String>,
}

impl MarksState {
    fn annotation(&self, id: &str) -> Option<&Annotation> {
        self.annotations.iter().chain(&self.bookmarks).chain(self.draft_bookmark.iter()).find(|annotation| annotation.id == id)
    }

    /// Replaces a stored annotation in place, or does nothing if it is gone.
    fn replace(&mut self, annotation: Annotation) {
        let items = if annotation.is_bookmark() { &mut self.bookmarks } else { &mut self.annotations };
        if let Some(existing) = items.iter_mut().find(|item| item.id == annotation.id) {
            *existing = annotation;
        }
    }

    /// Merges an asynchronous load into marks already changed optimistically.
    /// Local entries win duplicate ids, while local deletions must not be
    /// resurrected by a load that started before the deletion.
    fn merge_loaded(&mut self, annotations: Vec<Annotation>) -> (bool, bool) {
        let mut bookmarks_changed = false;
        let mut annotations_changed = false;
        let local_annotation_ids = self.annotations.iter().chain(&self.bookmarks).map(|annotation| annotation.id.clone()).collect::<HashSet<_>>();
        for annotation in annotations.into_iter().filter(|annotation| !local_annotation_ids.contains(annotation.id.as_str()) && !self.deleted_annotation_ids.contains(&annotation.id)) {
            if annotation.is_bookmark() {
                self.bookmarks.push(annotation);
                bookmarks_changed = true;
            } else {
                self.annotations.push(annotation);
                annotations_changed = true;
            }
        }
        (bookmarks_changed, annotations_changed)
    }

    /// Forgets an annotation, also closing its editor if it was open.
    fn remove_annotation(&mut self, id: &str, bookmark: bool) -> bool {
        let closed_editor = self.selected_id.as_deref() == Some(id);
        self.deleted_annotation_ids.insert(id.to_owned());
        if bookmark {
            self.bookmarks.retain(|item| item.id != id);
        } else {
            self.annotations.retain(|item| item.id != id);
        }
        if closed_editor {
            self.selected_id = None;
        }
        closed_editor
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BookmarkChange {
    Contents,
    Selection,
}

/// Annotation state plus its serialized durable mutation queue.
///
/// Readers still decide how annotations are anchored and painted, but the
/// optimistic in-memory mutation and matching backend write happen together.
pub(crate) struct MarkSession {
    state: MarksState,
    inherited: Vec<RecurringAnnotation>,
    recurring: RecurringMatcher,
    writes: MutationWriter,
    revision: u64,
    bookmark_changes: Sender<BookmarkChange>,
    bookmark_updates: Receiver<BookmarkChange>,
}

impl MarkSession {
    pub(crate) fn new(cx: &App, library: Rc<LibraryClient>) -> Self {
        let (bookmark_changes, bookmark_updates) = async_channel::bounded(1);
        Self { state: MarksState::default(), inherited: Vec::new(), recurring: RecurringMatcher::default(), writes: MutationWriter::new(cx, library, PersistenceSubject::Annotations), revision: 0, bookmark_changes, bookmark_updates }
    }

    /// The sidebar observes its model once. Changes in one UI turn coalesce
    /// into one update using the latest in-memory state.
    pub(crate) fn watch_bookmarks<V: 'static>(&self, cx: &Context<V>, apply: fn(&mut V, BookmarkChange, &mut Context<V>)) -> Task<()> {
        let updates = self.bookmark_updates.clone();
        let persistence = self.writes.changes();
        cx.spawn(async move |this, cx| {
            loop {
                let change = futures_lite::future::race(async { updates.recv().await.map(Some) }, async { persistence.recv().await.map(|_| None) }).await;
                let Ok(change) = change else { break };
                if this
                    .update(cx, |this, cx| {
                        if let Some(change) = change {
                            apply(this, change, cx);
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    pub(crate) fn with_persistence_notice(&self, document: gpui::AnyElement, theme: ui_components::BrowserTheme) -> gpui::AnyElement {
        self.writes.with_notice(document, theme)
    }

    fn notify_bookmarks(&self, change: BookmarkChange) {
        // Content changes supersede a pending selection notification. Selection
        // changes can reuse either notification already queued for this turn.
        if change == BookmarkChange::Contents {
            let _ = self.bookmark_updates.try_recv();
        }
        let _ = self.bookmark_changes.try_send(change);
    }

    pub(crate) fn merge_loaded(&mut self, annotations: Vec<Annotation>) {
        let (bookmarks_changed, annotations_changed) = self.state.merge_loaded(annotations);
        if bookmarks_changed {
            self.notify_bookmarks(BookmarkChange::Contents);
        }
        if annotations_changed {
            self.refresh_recurring_index();
        }
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns whether ordinary or inherited annotations changed and their
    /// document overlays need refreshing. Bookmarks have no painted overlay.
    pub(crate) fn refresh_loaded(&mut self, annotations: Vec<Annotation>, inherited: Vec<RecurringAnnotation>, revision: u64) -> bool {
        let (bookmarks_changed, annotations_changed) = if self.revision == revision {
            let (bookmarks, annotations): (Vec<_>, Vec<_>) = annotations.into_iter().partition(Annotation::is_bookmark);
            let bookmarks_changed = self.state.bookmarks != bookmarks;
            let annotations_changed = self.state.annotations != annotations || self.inherited != inherited;
            self.state.bookmarks = bookmarks;
            self.state.annotations = annotations;
            self.inherited = inherited;
            // A confirmed backend snapshot includes prior deletions. A later
            // explicit restore should be allowed to appear again.
            self.state.deleted_annotation_ids.clear();
            (bookmarks_changed, annotations_changed)
        } else {
            let (bookmarks_changed, annotations_changed) = self.state.merge_loaded(annotations);
            let inherited_changed = self.merge_inherited(inherited);
            (bookmarks_changed, annotations_changed || inherited_changed)
        };
        let selection_removed = self.state.selected_id.as_deref().is_some_and(|id| self.annotation(id).is_none());
        if selection_removed {
            self.state.selected_id = None;
        }
        if bookmarks_changed {
            self.notify_bookmarks(BookmarkChange::Contents);
        } else if selection_removed {
            self.notify_bookmarks(BookmarkChange::Selection);
        }
        if annotations_changed {
            self.refresh_recurring_index();
        }
        annotations_changed
    }

    pub(crate) fn annotations(&self) -> &[Annotation] {
        &self.state.annotations
    }

    pub(crate) fn bookmarks(&self) -> &[Annotation] {
        &self.state.bookmarks
    }

    /// A new bookmark stays local until the reader presses Save.
    pub(crate) fn start_bookmark(&mut self, bookmark: Annotation) {
        self.state.selected_id = Some(bookmark.id.clone());
        self.state.draft_bookmark = Some(bookmark);
        self.notify_bookmarks(BookmarkChange::Selection);
    }

    pub(crate) fn annotation(&self, id: &str) -> Option<&Annotation> {
        self.state.annotation(id).or_else(|| self.inherited.iter().map(|item| &item.annotation).find(|annotation| annotation.id == id))
    }

    pub(crate) fn selected(&self) -> Option<&Annotation> {
        self.state.selected_id.as_deref().and_then(|id| self.annotation(id))
    }

    pub(crate) fn selected_id(&self) -> Option<&str> {
        self.state.selected_id.as_deref()
    }

    /// Selects an annotation and returns the note the UI should edit.
    pub(crate) fn select(&mut self, id: String) -> String {
        let was_bookmark = self.selected().is_some_and(Annotation::is_bookmark);
        if self.state.draft_bookmark.as_ref().is_some_and(|draft| draft.id != id) {
            self.state.draft_bookmark = None;
        }
        let note = self.annotation(&id).map(|annotation| annotation.note.clone()).unwrap_or_default();
        self.state.selected_id = Some(id);
        if was_bookmark || self.selected().is_some_and(Annotation::is_bookmark) {
            self.notify_bookmarks(BookmarkChange::Selection);
        }
        note
    }

    pub(crate) fn clear_selection(&mut self) -> bool {
        let was_bookmark = self.selected().is_some_and(Annotation::is_bookmark);
        self.state.draft_bookmark = None;
        let changed = self.state.selected_id.take().is_some();
        if was_bookmark {
            self.notify_bookmarks(BookmarkChange::Selection);
        }
        changed
    }

    pub(crate) fn append(&mut self, annotation: Annotation, what: &'static str) {
        self.revision = self.revision.wrapping_add(1);
        if annotation.is_bookmark() {
            self.state.bookmarks.push(annotation.clone());
            self.notify_bookmarks(BookmarkChange::Contents);
        } else {
            self.state.annotations.push(annotation.clone());
            self.refresh_recurring_index();
        }
        self.persist(annotation, what);
    }

    pub(crate) fn prepend(&mut self, annotation: Annotation, what: &'static str) {
        self.revision = self.revision.wrapping_add(1);
        self.state.annotations.insert(0, annotation.clone());
        self.refresh_recurring_index();
        self.persist(annotation, what);
    }

    fn update(&mut self, annotation: Annotation, what: &'static str) {
        self.revision = self.revision.wrapping_add(1);
        if !annotation.is_bookmark() {
            if let Some(item) = self.inherited.iter_mut().find(|item| item.annotation.id == annotation.id) {
                item.annotation = annotation.clone();
            }
        }
        self.state.replace(annotation.clone());
        if annotation.is_bookmark() {
            self.notify_bookmarks(BookmarkChange::Contents);
        } else {
            self.refresh_recurring_index();
        }
        self.persist(annotation, what);
    }

    /// Updates and persists the annotation currently selected by the UI.
    /// Bookmark editors commit through `save_bookmark` explicitly.
    pub(crate) fn update_selected(&mut self, note: String, style: Option<AnnotationStyle>, color: Option<u32>, what: &'static str) -> Option<Annotation> {
        let id = self.state.selected_id.clone()?;
        let mut annotation = self.annotation(&id)?.clone();
        if annotation.is_bookmark() {
            return None;
        }
        if annotation.note == note && style.as_ref().is_none_or(|style| *style == annotation.style) && color.is_none_or(|color| color == annotation.color) {
            return Some(annotation);
        }
        annotation.note = note;
        if let Some(style) = style {
            annotation.style = style;
        }
        if let Some(color) = color {
            annotation.color = color;
        }
        annotation.modified_at = now_millis();
        self.update(annotation.clone(), what);
        Some(annotation)
    }

    /// Only the bookmark editor's Save action commits its name.
    pub(crate) fn save_bookmark(&mut self, name: String, what: &'static str) -> Option<Annotation> {
        let mut bookmark = self.selected()?.clone();
        let name = name.trim();
        if !bookmark.is_bookmark() || bookmark.toc_ordinal.is_none() || name.is_empty() {
            return None;
        }
        let draft = self.state.draft_bookmark.as_ref().is_some_and(|draft| draft.id == bookmark.id);
        if !draft && bookmark.note == name {
            return Some(bookmark);
        }
        bookmark.note = name.to_owned();
        bookmark.modified_at = now_millis();
        if draft {
            self.state.draft_bookmark = None;
            self.append(bookmark.clone(), what);
        } else {
            self.update(bookmark.clone(), what);
        }
        Some(bookmark)
    }

    /// Deletes an annotation and reports whether its editor was closed.
    pub(crate) fn delete(&mut self, id: String, what: &'static str) -> bool {
        let bookmark = self.annotation(&id).is_some_and(Annotation::is_bookmark);
        self.revision = self.revision.wrapping_add(1);
        if !bookmark {
            self.inherited.retain(|item| item.annotation.id != id);
        }
        let closed_editor = self.state.remove_annotation(&id, bookmark);
        if bookmark {
            self.notify_bookmarks(BookmarkChange::Contents);
        } else {
            self.refresh_recurring_index();
        }
        let modified_at = now_millis() as i64;
        self.writes.record(what, move |library| {
            let library = library.clone();
            let id = id.clone();
            Box::pin(async move { library.delete_annotation(id, modified_at).await })
        });
        closed_editor
    }

    fn merge_inherited(&mut self, annotations: Vec<RecurringAnnotation>) -> bool {
        let inherited = annotations
            .into_iter()
            .filter(|item| !self.state.deleted_annotation_ids.contains(&item.annotation.id))
            .map(|mut item| {
                if let Some(local) = self.inherited.iter().find(|local| local.annotation.id == item.annotation.id && local.annotation.modified_at > item.annotation.modified_at) {
                    item.annotation = local.annotation.clone();
                }
                item
            })
            .collect::<Vec<_>>();
        let changed = self.inherited != inherited;
        self.inherited = inherited;
        changed
    }

    pub(crate) fn recurring_annotations(&self) -> impl Iterator<Item = &Annotation> {
        recurring_annotations(&self.state.annotations, &self.inherited)
    }

    fn refresh_recurring_index(&mut self) {
        self.recurring.update(recurring_annotations(&self.state.annotations, &self.inherited));
    }

    pub(crate) fn recurring_patterns(&self) -> Option<&html::search::TextPatternMatcher> {
        self.recurring.patterns()
    }

    pub(crate) fn recurring_annotation_id(&self, pattern: usize) -> Option<&str> {
        self.recurring.annotation_id(pattern)
    }

    pub(crate) fn recurring_matches<'a>(&'a self, text: &str) -> impl Iterator<Item = (&'a Annotation, TextPatternMatch)> {
        self.recurring.matching_annotations(text, self.recurring_annotations())
    }

    pub(crate) fn add_selected_alias(&mut self, alias: &str, what: &'static str) {
        let Some(mut annotation) = self.selected().filter(|annotation| annotation.anchor.recurring_scope().is_some()).cloned() else { return };
        let alias = alias.split_whitespace().collect::<Vec<_>>().join(" ");
        if alias.is_empty() || alias.contains('\0') || annotation.recurring_phrases().any(|phrase| phrase.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase() == alias.to_lowercase()) {
            return;
        }
        let mut aliases = annotation.anchor.recurring_aliases().to_vec();
        aliases.push(alias);
        annotation.anchor = annotation.anchor.with_recurring_aliases(aliases);
        annotation.modified_at = now_millis();
        self.update(annotation, what);
    }

    pub(crate) fn remove_selected_alias(&mut self, alias: &str, what: &'static str) {
        let Some(mut annotation) = self.selected().cloned() else { return };
        let mut aliases = annotation.anchor.recurring_aliases().to_vec();
        let previous = aliases.len();
        aliases.retain(|candidate| candidate != alias);
        if aliases.len() == previous {
            return;
        }
        annotation.anchor = annotation.anchor.with_recurring_aliases(aliases);
        annotation.modified_at = now_millis();
        self.update(annotation, what);
    }

    /// A missing scope gets its own empty note; existing notes keep their scopes.
    pub(crate) fn create_scoped_annotation(&mut self, mut annotation: Annotation, scope: Option<RecurringScope>, what: &'static str) -> String {
        annotation.id = uuid::Uuid::new_v4().to_string();
        annotation.anchor = annotation.anchor.with_recurring_scope(scope);
        annotation.note.clear();
        annotation.created_at = now_millis();
        annotation.modified_at = annotation.created_at;
        annotation.deleted = false;
        let id = annotation.id.clone();
        self.append(annotation, what);
        self.select(id)
    }

    pub(crate) fn editor_context(&self, book: BookId, passage: &str, local_ids: &[String], chapter_available: bool) -> super::annotation_editor::RecurringEditorContext {
        build_editor_context(book, passage, local_ids, chapter_available, self.selected(), self.annotations(), &self.inherited, &self.recurring)
    }

    fn persist(&self, annotation: Annotation, what: &'static str) {
        self.writes.record(what, move |library| {
            let library = library.clone();
            let annotation = annotation.clone();
            Box::pin(async move { library.upsert_annotation(annotation).await })
        });
    }
}

fn recurring_annotations<'a>(annotations: &'a [Annotation], inherited: &'a [RecurringAnnotation]) -> impl Iterator<Item = &'a Annotation> + Clone {
    annotations.iter().chain(inherited.iter().map(|item| &item.annotation)).filter(|annotation| annotation.anchor.recurring_scope().is_some() && !annotation.deleted)
}

fn build_editor_context(
    book: BookId, passage: &str, local_ids: &[String], chapter_available: bool, selected: Option<&Annotation>, annotations: &[Annotation], inherited: &[RecurringAnnotation], recurring: &RecurringMatcher,
) -> super::annotation_editor::RecurringEditorContext {
    use super::annotation_editor::{EditorBook, RecurringEditorContext};
    let matching_ids = recurring.matching_ids(passage).collect::<HashSet<_>>();
    let mut books = Vec::new();
    let local = annotations.iter().filter(|annotation| local_ids.contains(&annotation.id)).cloned().collect::<Vec<_>>();
    if !local.is_empty() {
        books.push(EditorBook { id: book, title: text!(reader::this_book).into(), annotations: local });
    }
    for item in inherited {
        if item.annotation.anchor.recurring_scope() != Some(RecurringScope::Library) || (!matching_ids.contains(item.annotation.id.as_str()) && !local_ids.contains(&item.annotation.id)) {
            continue;
        }
        let index = books.iter().position(|entry| entry.id == item.annotation.book_id).unwrap_or_else(|| {
            books.push(EditorBook { id: item.annotation.book_id, title: item.book_title.clone(), annotations: Vec::new() });
            books.len() - 1
        });
        books[index].annotations.push(item.annotation.clone());
    }
    RecurringEditorContext { books, chapter_available, scope_editable: selected.is_some_and(|annotation| annotation.book_id == book) }
}

pub(crate) fn now_millis() -> u64 {
    // Deletions use signed database timestamps; keep both representations in
    // range, including on devices whose clocks have not been initialized.
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|duration| duration.as_millis().min(i64::MAX as u128) as u64).unwrap_or(0)
}

/// Shared reader loading and subscription lifecycle. Adapters filter their own
/// anchors before projecting the resulting snapshot.
pub(crate) fn load_annotations<V: 'static>(cx: &Context<V>, library: Rc<LibraryClient>, book: BookId, marks: &MarkSession, apply: impl FnOnce(&mut V, &mut Context<V>, (Vec<Annotation>, Vec<RecurringAnnotation>)) + 'static) {
    let flushed = marks.writes.flush();
    super::persistence::write(
        cx,
        library,
        "load reader annotations",
        move |library| {
            let library = library.clone();
            Box::pin(async move {
                flushed.recv().await.map_err(|_| library_backend::BackendError::message("Reader annotation flush was interrupted"))??;
                library.reader_annotations(book).await
            })
        },
        apply,
    );
}

pub(crate) fn watch_updates<V: 'static>(library: Rc<LibraryClient>, reload: fn(&mut V, &mut Context<V>), cx: &Context<V>) -> Task<()> {
    cx.spawn(async move |this, cx| {
        let Ok((updates, _)) = library.updates().await else { return };
        // Subscribe before taking the follow-up snapshot, covering edits that
        // arrive while the reader is opening. Coalesce queued notifications.
        if this.update(cx, reload).is_err() {
            return;
        }
        while let Ok(update) = updates.recv().await {
            // Structural changes can alter inherited notes or arrive from sync.
            // ReadingProgress updates only affect shelf cards, not reader marks.
            let relevant = |update| matches!(update, LibraryUpdate::Annotations | LibraryUpdate::Contents);
            let mut refresh = relevant(update);
            while let Ok(update) = updates.try_recv() {
                refresh |= relevant(update);
            }
            if refresh && this.update(cx, reload).is_err() {
                return;
            }
        }
    })
}

#[cfg(test)]
mod recurring_tests {
    use super::*;
    use book_model::AnnotationAnchor;

    fn annotation(id: &str, book: BookId, scope: Option<RecurringScope>, text: &str) -> Annotation {
        Annotation {
            id: id.into(),
            book_id: book,
            anchor: AnnotationAnchor::epub_cfi("epubcfi(/6/2!/4/2/1:0)").with_recurring_scope(scope),
            exact_text: text.into(),
            note: format!("Note {id}"),
            style: AnnotationStyle::Highlight,
            color: 0xffcc00,
            created_at: 1,
            toc_ordinal: Some(0),
            progress: Some(1.0),
            modified_at: 2,
            deleted: false,
        }
    }

    #[test]
    fn recurring_editor_groups_only_explicit_matching_origin_books() {
        let current = BookId::from_bytes([1; 32]);
        let source = BookId::from_bytes([2; 32]);
        let unrelated = BookId::from_bytes([3; 32]);
        let local = vec![annotation("local", current, None, "Elizabeth"), annotation("elsewhere", current, None, "Elizabeth")];
        let inherited = vec![
            RecurringAnnotation { annotation: annotation("source-one", source, Some(RecurringScope::Library), "Elizabeth"), book_title: "Source book".into() },
            RecurringAnnotation { annotation: annotation("source-two", source, Some(RecurringScope::Library), "ELIZABETH"), book_title: "Source book".into() },
            RecurringAnnotation { annotation: annotation("unrelated", unrelated, Some(RecurringScope::Library), "Darcy"), book_title: "Unrelated".into() },
            RecurringAnnotation { annotation: annotation("private", unrelated, Some(RecurringScope::Book), "Elizabeth"), book_title: "Private".into() },
        ];
        let mut matcher = RecurringMatcher::default();
        matcher.update(recurring_annotations(&local, &inherited));
        let context = build_editor_context(current, "Elizabeth", &["local".into()], true, Some(&local[0]), &local, &inherited, &matcher);
        assert!(context.scope_editable);
        assert_eq!(context.books.len(), 2);
        assert_eq!(context.books[0].id, current);
        assert_eq!(context.books[0].annotations.len(), 1, "another explicit passage in this book is not the selected passage");
        assert_eq!(context.books[1].id, source);
        assert_eq!(context.books[1].annotations.len(), 2);
        let context = build_editor_context(current, "Elizabeth", &[], true, Some(&inherited[0].annotation), &local, &inherited, &matcher);
        assert!(!context.scope_editable);
        assert_eq!(context.books.len(), 1, "inherited appearances don't create a source entry for this book");
        assert_eq!(context.books[0].id, source);
        let context = build_editor_context(current, "Elizabethan", &[], true, None, &local, &inherited, &matcher);
        assert!(context.books.is_empty(), "partial words don't match");
    }

    #[test]
    fn recurring_index_reuses_patterns_and_tracks_membership_text_and_deletion() {
        let book = BookId::from_bytes([1; 32]);
        let mut annotations = vec![annotation("one", book, Some(RecurringScope::Book), "Elizabeth"), annotation("two", book, Some(RecurringScope::Library), "ELIZABETH")];
        let mut matcher = RecurringMatcher::default();
        assert!(matcher.update(recurring_annotations(&annotations, &[])));
        assert_eq!(matcher.matching_ids("Elizabeth").collect::<Vec<_>>(), ["one", "two"], "duplicate phrases retain every explicit source");
        assert!(!matcher.update(recurring_annotations(&annotations, &[])));
        assert!(!matcher.update(annotations.iter().rev()), "backend reordering does not change the compiled phrases");
        annotations[0].note = "Updated note".into();
        annotations[0].color = 0x112233;
        annotations[0].anchor = annotations[0].anchor.with_recurring_scope(Some(RecurringScope::Chapter));
        assert!(!matcher.update(recurring_annotations(&annotations, &[])), "scope and formatting use current annotations without recompiling text");
        annotations[0].exact_text = "Darcy".into();
        assert!(matcher.update(recurring_annotations(&annotations, &[])));
        assert_eq!(matcher.matching_ids("Elizabeth").collect::<Vec<_>>(), ["two"]);
        annotations[1].deleted = true;
        assert!(matcher.update(recurring_annotations(&annotations, &[])));
        assert!(matcher.matches("Elizabeth").is_empty());
        annotations[0].anchor = annotations[0].anchor.with_recurring_scope(None);
        assert!(matcher.update(recurring_annotations(&annotations, &[])));
        assert!(matcher.matches("Darcy").is_empty());
    }
}

#[cfg(test)]
mod bookmark_tests {
    use super::*;
    use book_model::AnnotationAnchor;
    use gpui::TestAppContext;

    #[gpui::test]
    async fn snapshots_refresh_document_overlays_only_when_annotations_change(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let launch = app::host::BackendLaunch::initialize(app::AppDataLocation::native_path(directory.path())).unwrap();
        let (backend, _) = app::AppClient::start_native(launch).unwrap();
        let library = Rc::new(backend.management.library_for_import(None).await.unwrap());
        backend.management.delete_library(*library.id()).await.unwrap();
        cx.update(|cx| {
            let mut marks = MarkSession::new(cx, library);
            let mut bookmark = super::super::bookmarks::bookmark(BookId::from_bytes([1; 32]), AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2/1:0)"), 0, 0.0);
            bookmark.note = "Bookmark".into();
            assert!(!marks.refresh_loaded(vec![bookmark.clone()], vec![], marks.revision()));
            let mut note = bookmark.clone();
            note.id = "note".into();
            note.exact_text = "Passage".into();
            assert!(marks.refresh_loaded(vec![bookmark.clone(), note.clone()], vec![], marks.revision()));
            assert!(!marks.refresh_loaded(vec![bookmark.clone(), note.clone()], vec![], marks.revision()));
            bookmark.note = "Renamed bookmark".into();
            assert!(!marks.refresh_loaded(vec![bookmark.clone(), note.clone()], vec![], marks.revision()));
            note.color = 0xffcc00;
            assert!(marks.refresh_loaded(vec![bookmark.clone(), note], vec![], marks.revision()));
            assert!(marks.refresh_loaded(vec![bookmark], vec![], marks.revision()), "removing a highlight must refresh overlays");
        });
    }

    #[gpui::test]
    async fn bookmark_model_notifications_coalesce_edits_and_include_sync_and_deletion(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let launch = app::host::BackendLaunch::initialize(app::AppDataLocation::native_path(directory.path())).unwrap();
        let (backend, _) = app::AppClient::start_native(launch).unwrap();
        let library = Rc::new(backend.management.library_for_import(None).await.unwrap());
        backend.management.delete_library(*library.id()).await.unwrap();
        cx.update(|cx| {
            let mut marks = MarkSession::new(cx, library);
            let updates = marks.bookmark_updates.clone();
            let draft = super::super::bookmarks::bookmark(BookId::from_bytes([1; 32]), AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2/1:0)"), 0, 0.0);
            marks.start_bookmark(draft.clone());
            assert!(marks.update_selected("Must not save".into(), None, None, "implicit save").is_none());
            assert!(marks.bookmarks().is_empty());
            marks.save_bookmark("First".into(), "save bookmark");
            marks.save_bookmark("Renamed".into(), "rename bookmark");
            marks.clear_selection();
            marks.select(draft.id.clone());
            assert_eq!(updates.try_recv().unwrap(), BookmarkChange::Contents);
            assert!(updates.try_recv().is_err(), "one pending update covers all edits in a UI turn");
            assert_eq!(marks.bookmarks()[0].note, "Renamed");
            marks.save_bookmark("Renamed".into(), "save unchanged bookmark");
            assert!(updates.try_recv().is_err(), "unchanged names do not update the sidebar");
            let mut remote = marks.bookmarks()[0].clone();
            remote.note = "Synced name".into();
            marks.refresh_loaded(vec![remote], vec![], marks.revision());
            assert!(updates.try_recv().is_ok());
            assert_eq!(marks.bookmarks()[0].note, "Synced name");
            assert!(marks.update_selected("Renamed".into(), None, None, "implicit save after sync").is_none());
            assert_eq!(marks.bookmarks()[0].note, "Synced name");
            marks.clear_selection();
            assert_eq!(updates.try_recv().unwrap(), BookmarkChange::Selection);
            marks.select(draft.id.clone());
            assert_eq!(updates.try_recv().unwrap(), BookmarkChange::Selection);
            marks.refresh_loaded(marks.bookmarks().to_vec(), vec![], marks.revision());
            assert!(updates.try_recv().is_err(), "an unchanged snapshot preserves the existing bookmark model");
            marks.delete(draft.id, "delete bookmark");
            assert!(updates.try_recv().is_ok());
            assert!(marks.bookmarks().is_empty());
            marks.refresh_loaded(vec![], vec![], marks.revision());
            assert!(updates.try_recv().is_err(), "an unchanged snapshot does not rebuild bookmark rows");
        });
    }

    #[gpui::test]
    async fn bookmark_drafts_edits_and_deletions_survive_in_flight_snapshots(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let launch = app::host::BackendLaunch::initialize(app::AppDataLocation::native_path(directory.path())).unwrap();
        let (backend, _) = app::AppClient::start_native(launch).unwrap();
        let library = Rc::new(backend.management.library_for_import(None).await.unwrap());
        backend.management.delete_library(*library.id()).await.unwrap();
        cx.update(|cx| {
            let mut marks = MarkSession::new(cx, library);
            let draft = super::super::bookmarks::bookmark(BookId::from_bytes([1; 32]), AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2/1:0)"), 0, 0.0);
            marks.start_bookmark(draft.clone());
            marks.save_bookmark(" \n ".into(), "save bookmark");
            assert!(marks.bookmarks().is_empty());
            assert!(marks.selected().is_some());
            marks.refresh_loaded(vec![], vec![], marks.revision());
            assert!(marks.selected().is_some(), "loading saved marks retains the unnamed draft");
            marks.clear_selection();
            assert!(marks.annotation(&draft.id).is_none());

            marks.start_bookmark(draft.clone());
            marks.save_bookmark(" My bookmark ".into(), "save bookmark");
            assert_eq!(marks.bookmarks()[0].note, "My bookmark");
            assert!(marks.annotations().is_empty());
            let old = marks.bookmarks()[0].clone();
            let revision = marks.revision();
            marks.save_bookmark("Renamed".into(), "rename bookmark");
            marks.refresh_loaded(vec![old.clone()], vec![], revision);
            assert_eq!(marks.bookmarks()[0].note, "Renamed");
            assert!(marks.delete(draft.id.clone(), "delete bookmark"));
            marks.refresh_loaded(vec![old], vec![], revision);
            assert!(marks.bookmarks().is_empty());
            assert!(marks.selected().is_none());
        });
    }
}
