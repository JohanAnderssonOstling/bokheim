//! Synchronous library data adapter for the shared browse page.

use std::collections::HashMap;
use std::rc::Rc;

use app_contract::library::{BrowseBookSort, BrowseContents, BrowseRow, LibraryBrowseQuery, LibraryFileTypeFilter, ROOT_SUBJECT_PATH, SubjectPath};
use gpui::{AnyElement, App, AppContext, Context, Entity, IntoElement};
use sync_common::{DirId, ROOT_DIR_ID};

use super::adapter::{BrowseAdapter, BrowseCrumb, BrowseQuery, BrowseRequest};
use super::controls::{BrowseBookOrder, BrowseControls, BrowseFormatFilter};
use super::folder_actions::FolderActions;
use super::page::{BrowsePage, BrowsePageConfig};
use crate::library::pages::subjects::{MetadataTriage, RawSubjectsLane};
use crate::library::services::LibraryContext;
use crate::library::widgets::BookCardView;

const METADATA_REVIEW_LOCATION: &str = "__metadata_review__";
const RAW_SUBJECTS_LOCATION: &str = "__raw_subjects__";

#[derive(Clone, Copy)]
enum LibraryBrowseKind {
    Folder,
    Subject,
}

pub(crate) struct LibraryBrowseAdapter {
    ui: LibraryContext,
    kind: LibraryBrowseKind,
    controls: BrowseControls,
    children: Rc<Vec<BrowseRow>>,
    books: Rc<Vec<Entity<BookCardView>>>,
    metadata_review: Option<Entity<MetadataTriage>>,
    raw_subjects: Option<Entity<RawSubjectsLane>>,
}

pub(crate) enum LibraryBrowseResponse {
    Contents { contents: BrowseContents, metadata_review_count: Option<usize>, raw_subject_count: Option<usize> },
    MetadataReview,
    RawSubjects(Vec<String>),
}

impl LibraryBrowseAdapter {
    fn new(ui: LibraryContext, kind: LibraryBrowseKind) -> Self {
        Self { ui, kind, controls: BrowseControls::library(), children: Rc::new(Vec::new()), books: Rc::new(Vec::new()), metadata_review: None, raw_subjects: None }
    }

    fn backend_query(query: &BrowseQuery) -> LibraryBrowseQuery {
        let file_types = query
            .formats
            .iter()
            .filter_map(|format| match format {
                BrowseFormatFilter::Library(format) => Some(*format),
                BrowseFormatFilter::Any | BrowseFormatFilter::Catalog(_) => None,
            })
            .collect::<Vec<LibraryFileTypeFilter>>();
        let book_sort = match query.book_sort {
            Some(BrowseBookOrder::Library(sort)) => sort,
            Some(BrowseBookOrder::Catalog(_)) | None => BrowseBookSort::default(),
        };
        LibraryBrowseQuery {
            location: query.location.clone(),
            search: query.search.clone(),
            file_types,
            chip_sort: query.chip_sort.unwrap_or_default(),
            book_sort,
            languages: query.languages.iter().map(|language| (*language).to_owned()).collect(),
        }
    }
}

impl BrowseAdapter for LibraryBrowseAdapter {
    type Response = LibraryBrowseResponse;
    type Book = BookCardView;

    fn request(&self, query: &BrowseQuery) -> BrowseRequest<Self::Response> {
        let query = Self::backend_query(query);
        if matches!(self.kind, LibraryBrowseKind::Subject) && query.location == METADATA_REVIEW_LOCATION {
            return BrowseRequest::Ready(LibraryBrowseResponse::MetadataReview);
        }
        if matches!(self.kind, LibraryBrowseKind::Subject) && query.location == RAW_SUBJECTS_LOCATION {
            let load = self.ui.backend().load_unmapped_subject_texts();
            return BrowseRequest::Async(Box::pin(async move { load.await.map(LibraryBrowseResponse::RawSubjects) }));
        }
        let subject_root = matches!(self.kind, LibraryBrowseKind::Subject) && query.location == ROOT_SUBJECT_PATH;
        let search = query.search.trim().to_lowercase();
        let show_metadata_review = subject_root && (search.is_empty() || "uncategorized".contains(&search));
        let show_raw_subjects = subject_root && (search.is_empty() || "raw subjects".contains(&search));
        let metadata_reviews = show_metadata_review.then(|| self.ui.backend().load_prepared_edition_identity_reviews());
        let raw_subjects = show_raw_subjects.then(|| self.ui.backend().load_unmapped_subject_texts());
        let request = match self.kind {
            LibraryBrowseKind::Folder => self.ui.backend().folder_contents(query),
            LibraryBrowseKind::Subject => self.ui.backend().subject_contents(query),
        };
        BrowseRequest::Async(Box::pin(async move {
            let contents = request.await?;
            let metadata_review_count = match metadata_reviews {
                Some(metadata_reviews) => {
                    let count = match metadata_reviews.await {
                        Ok(reviews) => reviews.len(),
                        Err(error) => {
                            log::warn!("failed to count metadata reviews for the subject root: {error}");
                            0
                        }
                    };
                    (count > 0).then_some(count)
                }
                None => None,
            };
            let raw_subject_count = match raw_subjects {
                Some(raw_subjects) => {
                    let count = match raw_subjects.await {
                        Ok(subjects) => subjects.len(),
                        Err(error) => {
                            log::warn!("failed to count raw subjects for the subject root: {error}");
                            0
                        }
                    };
                    (count > 0).then_some(count)
                }
                None => None,
            };
            Ok(LibraryBrowseResponse::Contents { contents, metadata_review_count, raw_subject_count })
        }))
    }

    fn install(&mut self, response: Self::Response, cx: &mut App) {
        let (mut contents, metadata_review_count, raw_subject_count) = match response {
            LibraryBrowseResponse::Contents { contents, metadata_review_count, raw_subject_count } => (contents, metadata_review_count, raw_subject_count),
            LibraryBrowseResponse::MetadataReview => {
                self.children = Rc::new(Vec::new());
                self.books = Rc::new(Vec::new());
                self.raw_subjects = None;
                self.metadata_review = Some(cx.new(|cx| MetadataTriage::new(self.ui.clone(), cx)));
                return;
            }
            LibraryBrowseResponse::RawSubjects(subjects) => {
                self.children = Rc::new(Vec::new());
                self.books = Rc::new(Vec::new());
                self.metadata_review = None;
                self.raw_subjects = Some(cx.new(|cx| RawSubjectsLane::new(subjects, cx)));
                return;
            }
        };
        self.metadata_review = None;
        self.raw_subjects = None;
        if let Some(metadata_review_count) = metadata_review_count {
            contents.children.push(BrowseRow { id: METADATA_REVIEW_LOCATION.to_owned(), name: "Uncategorized".to_owned(), path: None, book_count: metadata_review_count, downloaded_book_count: 0 });
        }
        if let Some(raw_subject_count) = raw_subject_count {
            contents.children.push(BrowseRow { id: RAW_SUBJECTS_LOCATION.to_owned(), name: "Raw subjects".to_owned(), path: None, book_count: raw_subject_count, downloaded_book_count: 0 });
        }
        self.children = Rc::new(contents.children);
        let mut existing = self.books.iter().map(|book| (book.read(cx).content_hash(), book.clone())).collect::<HashMap<_, _>>();
        self.books = Rc::new(
            contents
                .books
                .into_iter()
                .map(|book| {
                    let source_directory = book.source_directory;
                    if let Some(existing) = existing.remove(&book.content_hash) {
                        existing.update(cx, |existing, _| existing.replace_row(book, source_directory));
                        existing
                    } else {
                        match source_directory {
                            Some(source_directory) => BookCardView::create_in_folder(book, self.ui.clone(), source_directory, cx),
                            None => BookCardView::create(book, self.ui.clone(), cx),
                        }
                    }
                })
                .collect(),
        );
    }

    fn children(&self) -> Rc<Vec<BrowseRow>> {
        self.children.clone()
    }

    fn books(&self) -> Rc<Vec<Entity<Self::Book>>> {
        self.books.clone()
    }

    fn controls(&self) -> &BrowseControls {
        &self.controls
    }

    fn custom_body(&self) -> Option<AnyElement> {
        self.metadata_review.clone().map(IntoElement::into_any_element).or_else(|| self.raw_subjects.clone().map(IntoElement::into_any_element))
    }

    fn prepare_book(book: &Entity<Self::Book>, cx: &mut App) {
        book.update(cx, |book, cx| book.prepare(cx));
    }

    fn book_ready(book: &Entity<Self::Book>, cx: &App) -> bool {
        book.read(cx).is_prepared()
    }

    fn activate_book(book: &Entity<Self::Book>, window: &mut gpui::Window, cx: &mut gpui::App) {
        let _ = book.update(cx, |book, book_cx| book.open(window, book_cx));
    }

    fn path_to(&self, child: &BrowseRow) -> Result<Option<Vec<BrowseCrumb>>, String> {
        match self.kind {
            LibraryBrowseKind::Folder => {
                let id = DirId::parse_str(&child.id).map_err(|error| error.to_string())?;
                let path = self.ui.backend().directory_path(&id)?.into_iter().map(|directory| BrowseCrumb { location: directory.id.to_string(), label: directory.name }).collect();
                Ok(Some(path))
            }
            LibraryBrowseKind::Subject => {
                let segments = child.id.split(" / ").filter(|segment| !segment.is_empty() && *segment != ROOT_SUBJECT_PATH).collect::<Vec<_>>();
                let mut location = String::new();
                let path = segments
                    .iter()
                    .enumerate()
                    .map(|(index, segment)| {
                        if !location.is_empty() {
                            location.push_str(" / ");
                        }
                        location.push_str(segment);
                        let label = if index + 1 == segments.len() { child.name.clone() } else { (*segment).to_owned() };
                        BrowseCrumb { location: location.clone(), label }
                    })
                    .collect();
                Ok(Some(path))
            }
        }
    }
}

pub(crate) fn folder(ui: LibraryContext, cx: &mut Context<BrowsePage<LibraryBrowseAdapter>>) -> BrowsePage<LibraryBrowseAdapter> {
    let changes = ui.updates();
    let persisted_path = ui.app_backend().folder_navigation_path(ui.backend().id());
    let current_id = persisted_path.last().map(|crumb| crumb.id).unwrap_or(ROOT_DIR_ID);
    let current_name = persisted_path.last().map(|crumb| crumb.label.clone());
    let path: Vec<_> = persisted_path.into_iter().map(|crumb| BrowseCrumb { location: crumb.id.to_string(), label: crumb.label }).collect();
    let persistence = ui.clone();
    let folder_actions = cx.new(|_| FolderActions::new(ui.clone(), current_id, current_name));
    let adapter = LibraryBrowseAdapter::new(ui, LibraryBrowseKind::Folder);
    let config = BrowsePageConfig::new(ROOT_DIR_ID.to_string(), "Folders", "folder").with_path(path).with_changes(changes).with_folder_actions(folder_actions).with_persist_path(move |path| {
        let persisted = path.iter().map(|crumb| DirId::parse_str(&crumb.location).map(|id| app_contract::NavigationCrumb { id, label: crumb.label.clone() })).collect::<Result<Vec<_>, _>>();
        match persisted {
            Ok(path) => {
                if let Err(error) = persistence.app_backend().save_folder_navigation_path(persistence.backend().id(), path) {
                    log::warn!("failed to persist folder path: {error}");
                }
            }
            Err(error) => log::warn!("failed to persist folder navigation: {error}"),
        }
    });
    BrowsePage::new(adapter, config, cx)
}

pub(crate) fn subject(ui: LibraryContext, cx: &mut Context<BrowsePage<LibraryBrowseAdapter>>) -> BrowsePage<LibraryBrowseAdapter> {
    let changes = ui.updates();
    let path: Vec<_> = ui.app_backend().subject_navigation_path(ui.backend().id()).into_iter().map(|crumb| BrowseCrumb { location: crumb.id.to_string(), label: crumb.label }).collect();
    let persistence = ui.clone();
    let adapter = LibraryBrowseAdapter::new(ui, LibraryBrowseKind::Subject);
    let config = BrowsePageConfig::new(ROOT_SUBJECT_PATH, "Subjects", "subject").with_path(path).with_changes(changes).with_persist_path(move |path| {
        if path.iter().any(|crumb| matches!(crumb.location.as_str(), METADATA_REVIEW_LOCATION | RAW_SUBJECTS_LOCATION)) {
            return;
        }
        let path = path.iter().map(|crumb| app_contract::NavigationCrumb { id: SubjectPath::new(crumb.location.clone()), label: crumb.label.clone() }).collect();
        if let Err(error) = persistence.app_backend().save_subject_navigation_path(persistence.backend().id(), path) {
            log::warn!("failed to persist subject path: {error}");
        }
    });
    BrowsePage::new(adapter, config, cx)
}
