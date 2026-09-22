//! The author index: one full-width row per credited identity, the author's
//! name over their shelf.
//!
//! It is Home's page with different rows — same paginator, same shelves, same
//! cards — so it is [`ShelfPage`] with an author-shaped source. What is left
//! here is the query and the one thing the index says that Home does not: the
//! subjects an author writes about.

use library_backend::LibraryClient;
use library_model::AuthorSort;

use crate::library::pages::shelves::{ShelfFuture, ShelfPage, ShelfRow, ShelfSort, ShelfSource};

pub(crate) type AuthorsPage = ShelfPage<AuthorShelves>;

pub(crate) struct AuthorShelves;

impl ShelfSource for AuthorShelves {
    const ID: &'static str = "authors";
    // A name writes about more than fits on one line beside it.
    const SUBJECT_ROWS: usize = 2;
    const LOADING_MESSAGE: &'static str = "Loading authors…";
    const TITLE: Option<&'static str> = Some("Authors");

    fn count_label(rows: usize) -> String {
        if rows == 1 { "1 name".to_owned() } else { format!("{rows} names") }
    }

    const SORTS: &'static [ShelfSort] = &[ShelfSort::Name, ShelfSort::BookCount];

    fn load(library: LibraryClient, sort: Option<ShelfSort>) -> ShelfFuture {
        let sort = match sort {
            Some(ShelfSort::BookCount) => AuthorSort::BookCount,
            _ => AuthorSort::Name,
        };
        Box::pin(async move {
            let authors = library.authors(sort).await?;
            Ok(authors
                .into_iter()
                .map(|author| ShelfRow {
                    id: author.id.to_string().into(),
                    title: author.name.as_str().to_owned().into(),
                    subjects: author.subjects.into_iter().map(|subject| (subject.parent.map(Into::into), subject.label.into())).collect(),
                    books: author.books,
                })
                .collect())
        })
    }
}
