//! Library notebook requests and bounded pages of annotated books.
use crate::{BookCardRow, BrowseLocation, BrowsePageState, LibraryBrowseQuery, LibraryFileTypeFilter};
use book_model::Annotation;
use content_address::BookId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const NOTEBOOK_ANNOTATION_PAGE_SIZE: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum NotebookSort {
    #[default]
    Recent,
    BookName,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotebookQuery {
    pub search: String,
    pub formats: Vec<LibraryFileTypeFilter>,
    pub sort: NotebookSort,
}

impl NotebookQuery {
    pub fn browse_query(&self) -> LibraryBrowseQuery {
        LibraryBrowseQuery {
            location: BrowseLocation::Notebook(self.sort),
            search: self.search.clone(),
            file_types: self.formats.clone(),
            languages: Vec::new(),
            chip_sort: Default::default(),
            book_sort: Default::default(),
            hide_finished: false,
        }
    }
}

/// Initial loads and continuations share one request. A missing revision
/// starts a fresh selection; offsets retain each book's annotation window.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NotebookRequest {
    pub page: crate::BrowsePageRequest,
    pub revision: Option<String>,
    #[serde(default)]
    pub annotation_offsets: HashMap<BookId, usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NotebookBook {
    pub book: BookCardRow,
    pub page: NotebookAnnotationPage,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NotebookAnnotationPage {
    pub annotations: Vec<Annotation>,
    pub has_more: bool,
    pub offset: usize,
    pub revision: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NotebookPage {
    pub books: Vec<NotebookBook>,
    pub page: BrowsePageState,
}
