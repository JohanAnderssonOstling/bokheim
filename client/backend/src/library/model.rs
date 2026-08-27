use crate::BoxedBookReader;
use library_model::BookFormat;

pub struct ResolvedBook {
    pub path: String,
    pub format: BookFormat,
    pub reader: BoxedBookReader,
}

pub(crate) struct LibraryAccountState {
    pub user_id: Option<String>,
    pub bound_user_id: Option<String>,
    pub signed_in: bool,
}

#[cfg(test)]
mod tests {
    use library_model::{agent_id_from_migration_seed, AuthorSort, AuthorSummary, Dir, FolderSort};
    use test_support::fixture_dir_id;

    #[test]
    fn author_name_sort_is_case_insensitive_and_deterministic() {
        let mut authors = vec![
            AuthorSummary::try_new(agent_id_from_migration_seed(b"zelda"), "zelda".to_string(), None, 3, Vec::new(), Vec::new()).unwrap(),
            AuthorSummary::try_new(agent_id_from_migration_seed(b"Alice"), "Alice".to_string(), None, 1, Vec::new(), Vec::new()).unwrap(),
            AuthorSummary::try_new(agent_id_from_migration_seed(b"alice"), "alice".to_string(), None, 2, Vec::new(), Vec::new()).unwrap(),
        ];

        AuthorSort::Name.sort(&mut authors);
        assert_eq!(authors.iter().map(|author| author.name().as_str()).collect::<Vec<_>>(), vec!["Alice", "alice", "zelda"]);
    }

    #[test]
    fn author_book_count_sort_uses_name_as_a_deterministic_tie_breaker() {
        let mut authors = vec![
            AuthorSummary::try_new(agent_id_from_migration_seed(b"Zelda"), "Zelda".to_string(), None, 2, Vec::new(), Vec::new()).unwrap(),
            AuthorSummary::try_new(agent_id_from_migration_seed(b"Alice"), "Alice".to_string(), None, 7, Vec::new(), Vec::new()).unwrap(),
            AuthorSummary::try_new(agent_id_from_migration_seed(b"Bob"), "Bob".to_string(), None, 2, Vec::new(), Vec::new()).unwrap(),
        ];

        AuthorSort::BookCount.sort(&mut authors);
        assert_eq!(authors.iter().map(|author| (author.name().as_str(), author.book_count())).collect::<Vec<_>>(), vec![("Alice", 7), ("Bob", 2), ("Zelda", 2)]);
    }

    #[test]
    fn folder_book_count_sort_uses_name_as_a_deterministic_tie_breaker() {
        let mut directories = vec![Dir::new(fixture_dir_id("z"), "Zelda".to_owned(), 2, 0), Dir::new(fixture_dir_id("a"), "Alice".to_owned(), 7, 0), Dir::new(fixture_dir_id("b"), "Bob".to_owned(), 2, 0)];

        FolderSort::BookCount.sort(&mut directories);
        assert_eq!(directories.iter().map(|directory| (directory.name().as_str(), directory.book_count())).collect::<Vec<_>>(), vec![("Alice", 7), ("Bob", 2), ("Zelda", 2)]);
    }
}
