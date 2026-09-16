//! Exact subject hierarchy with durable and bounded filtered views.
use super::*;
use library_model::{BrowseContents, LibraryBrowseQuery, normalize_search_text};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone, Default)]
struct Subject {
    label: String,
    parent: String,
    children: Vec<String>,
    direct_books: BTreeSet<usize>,
    all_books: BTreeSet<usize>,
    contents: Contents,
}

#[derive(Clone, Default)]
struct Contents {
    subjects: BTreeSet<String>,
    books: BTreeSet<usize>,
}

impl Contents {
    fn append_subject(&mut self, path: &str, subject: &Subject) {
        if subject.all_books.is_empty() {
            return;
        }
        self.subjects.insert(path.to_owned());
    }
}

struct Projection {
    subjects: HashMap<String, Subject>,
    cards: Vec<BookCardRow>,
}

impl Projection {
    fn build(conn: &Connection, query: &LibraryBrowseQuery, downloaded_only: bool) -> rusqlite::Result<Self> {
        let mut cards = load_cards(conn, query, downloaded_only, None)?;
        if query.hide_finished {
            cards.retain(|card| card.progress < library_model::FINISHED_PROGRESS_THRESHOLD);
        }
        let card_indices = cards.iter().enumerate().map(|(index, card)| (card.content_hash.as_str(), index)).collect::<HashMap<_, _>>();
        let mut subjects = HashMap::<String, Subject>::new();
        let matched = serde_json::to_string(&cards.iter().map(|card| card.content_hash.as_str()).collect::<Vec<_>>()).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let mut statement = conn.prepare(include_str!("sql/subject_compaction/build_with.sql"))?;
        let mut rows = statement.query([matched])?;
        while let Some(row) = rows.next()? {
            let path: String = row.get(0)?;
            let subject = subjects.entry(path).or_default();
            subject.parent = row.get(1)?;
            subject.label = row.get(2)?;
            if let Some(index) = row.get::<_, Option<String>>(3)?.as_deref().and_then(|hash| card_indices.get(hash)) {
                subject.direct_books.insert(*index);
            }
        }
        subjects.entry(String::new()).or_default();
        subjects.entry(ROOT_SUBJECT_PATH.to_owned()).or_default();
        let paths = subjects.keys().cloned().collect::<Vec<_>>();
        for path in &paths {
            if !path.is_empty() {
                let parent = subjects[path].parent.clone();
                if let Some(parent) = subjects.get_mut(&parent) {
                    parent.children.push(path.clone());
                }
            }
        }
        // Iteration avoids recursive stack growth while resolving every child
        // before its parent, including the virtual roots.
        let mut deepest_first = paths;
        deepest_first.sort_by_key(|path| std::cmp::Reverse(subject_path_depth(path)));
        for path in &deepest_first {
            let subject = &subjects[path];
            let mut all_books = subject.direct_books.clone();
            let mut contents = Contents { books: subject.direct_books.clone(), ..Contents::default() };
            for child_path in &subject.children {
                let child = &subjects[child_path];
                all_books.extend(&child.all_books);
                contents.append_subject(child_path, child);
            }
            let subject = subjects.get_mut(path).expect("loaded subject");
            subject.all_books = all_books;
            subject.contents = contents;
        }

        Ok(Self { subjects, cards })
    }
    fn direct_contents(&self, location: &str) -> Contents {
        self.subjects.get(location).map(|subject| subject.contents.clone()).unwrap_or_default()
    }

    /// Keep the grouped API compatible without flattening intermediate subjects.
    fn grouped_view(&self, query: &LibraryBrowseQuery, downloads: &[(bool, bool)]) -> (BrowseContents, Vec<library_model::BrowseChipGroup>) {
        (self.view(query, downloads), Vec::new())
    }

    fn view(&self, query: &LibraryBrowseQuery, downloads: &[(bool, bool)]) -> BrowseContents {
        let subjects = &self.subjects;
        let cards = &self.cards;
        let search = normalize_search_text(&query.search);
        let contents = if search.is_empty() {
            let mut contents = self.direct_contents(&query.location);
            if query.include_direct_child_books {
                if let Some(subject) = subjects.get(&query.location) {
                    for child in &subject.children {
                        contents.books.extend(&subjects[child].direct_books);
                    }
                }
            }
            contents
        } else {
            let mut reachable = BTreeSet::new();
            let mut pending = vec![query.location.clone()];
            while let Some(path) = pending.pop() {
                if reachable.insert(path.clone()) {
                    if let Some(subject) = subjects.get(&path) {
                        pending.extend(subject.children.iter().cloned());
                    }
                }
            }
            // Search returns subjects at their actual paths, without promotion.
            let mut hits = Contents::default();
            for (path, subject) in subjects {
                if reachable.contains(path) && path != &query.location && normalize_search_text(&subject.label).contains(&search) {
                    hits.append_subject(path, subject);
                }
            }
            let scoped_books = subjects.get(&query.location).map(|s| &s.all_books);
            hits.books.extend(
                cards
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| scoped_books.is_some_and(|books| books.contains(index)))
                    .filter_map(|(index, card)| (normalize_search_text(&card.title).contains(&search) || normalize_search_text(&card.author).contains(&search)).then_some(index)),
            );
            hits
        };
        let mut children = contents
            .subjects
            .into_iter()
            .map(|path| {
                let subject = &subjects[&path];
                BrowseRow {
                    id: path,
                    name: subject.label.clone(),
                    path: (!search.is_empty() && !subject.parent.is_empty()).then(|| subject.parent.clone()),
                    book_count: subject.all_books.len(),
                    downloaded_book_count: subject.all_books.iter().filter(|&&index| downloads[index].0).count(),
                }
            })
            .collect::<Vec<_>>();
        sort_chip_rows(&mut children, query.chip_sort);
        let books = cards
            .iter()
            .enumerate()
            .filter_map(|(index, card)| {
                contents.books.contains(&index).then(|| {
                    let mut card = card.clone();
                    (card.downloaded, card.download_requested) = downloads[index];
                    card
                })
            })
            .collect();
        BrowseContents { children, books }
    }
}

fn sort_chip_rows(rows: &mut [BrowseRow], sort: BrowseChipSort) {
    rows.sort_by(|a, b| {
        let count_order = if sort == BrowseChipSort::BookCount { b.book_count.cmp(&a.book_count) } else { std::cmp::Ordering::Equal };
        count_order.then_with(|| normalize_search_text(&a.name).cmp(&normalize_search_text(&b.name))).then_with(|| a.name.cmp(&b.name)).then_with(|| a.id.cmp(&b.id))
    });
}

fn load_cards(conn: &Connection, query: &LibraryBrowseQuery, downloaded_only: bool, location: Option<&str>) -> rusqlite::Result<Vec<BookCardRow>> {
    let languages = query.languages.iter().map(String::as_str).collect::<Vec<_>>();
    let language_values = languages_query_value(&languages);
    let sql = if location.is_some() { include_str!("sql/subject_compaction/load_cards_location.sql") } else { include_str!("sql/subject_compaction/load_cards_all.sql") };
    let mut statement = conn.prepare(&sql)?;
    statement.query_map(rusqlite::params![location, i32::from(downloaded_only), file_types_query_value(&query.file_types), language_values, book_sort_query_value(query.book_sort)], read_book_card_row)?.collect()
}

/// Refresh only changed books and their ancestor paths.
pub(super) fn ensure_projection(conn: &Connection) -> rusqlite::Result<()> {
    super::subject_incremental::update(conn)
}

#[derive(Clone, PartialEq, Eq)]
struct CacheKey {
    library: String,
    revision: i64,
    downloaded_books: Vec<i64>,
    filters: String,
}
type Cache = VecDeque<(CacheKey, Arc<Projection>)>;
static FILTERED: OnceLock<Mutex<Cache>> = OnceLock::new();
fn cache() -> &'static Mutex<Cache> {
    FILTERED.get_or_init(Default::default)
}

pub fn subject_contents(conn: &Connection, query: &LibraryBrowseQuery, downloaded_only: bool) -> rusqlite::Result<BrowseContents> {
    subject_contents_view(conn, query, downloaded_only, false).map(|(contents, _)| contents)
}

// Compatibility for existing callers; this no longer compacts subjects.
pub use subject_contents as compacted_subject_contents;

pub fn grouped_subject_contents(conn: &Connection, query: &LibraryBrowseQuery, downloaded_only: bool) -> rusqlite::Result<(BrowseContents, Vec<library_model::BrowseChipGroup>)> {
    subject_contents_view(conn, query, downloaded_only, true)
}

fn subject_contents_view(conn: &Connection, query: &LibraryBrowseQuery, downloaded_only: bool, grouped: bool) -> rusqlite::Result<(BrowseContents, Vec<library_model::BrowseChipGroup>)> {
    let allow_cache = conn.is_autocommit();
    let tx = if allow_cache { Some(conn.unchecked_transaction()?) } else { None };
    ensure_projection(conn)?;
    let filtered = downloaded_only || query.hide_finished || query.include_direct_child_books || !query.languages.is_empty() || file_types_query_value(&query.file_types) != 0 || !query.search.is_empty();
    let mut pending_cache = None;
    let contents = if !filtered {
        let contents = BrowseContents { children: stored_children(conn, &query.location, query.chip_sort)?, books: load_cards(conn, query, false, Some(&query.location))? };
        (contents, Vec::new())
    } else {
        let replica: String = conn.query_row(include_str!("sql/subject_compaction/subject_contents_view_select_2.sql"), [], |r| r.get(0))?;
        let file: String = conn.query_row(include_str!("sql/subject_compaction/subject_contents_view_select_3.sql"), [], |r| r.get(0))?;
        let revision = conn.query_row(include_str!("sql/subject_compaction/subject_contents_view_select_4.sql"), [], |r| r.get(0))?;
        // Key the optional filter from current book state, without maintaining
        // a download counter or any other download data in subject storage.
        let downloaded_books = if downloaded_only { conn.prepare(include_str!("sql/subject_compaction/downloaded_books.sql"))?.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<Vec<i64>>>()? } else { Vec::new() };
        let mut languages = query.languages.clone();
        languages.sort();
        languages.dedup();
        let key =
            CacheKey { library: format!("{file}|{replica}"), revision, downloaded_books, filters: format!("{}|{:?}|{}|{}|{:?}", file_types_query_value(&query.file_types), languages, downloaded_only, query.hide_finished, query.book_sort) };
        let cached = if allow_cache { cache().lock().unwrap().iter().find(|(k, _)| *k == key).map(|(_, p)| p.clone()) } else { None };
        let projection = if let Some(projection) = cached {
            projection
        } else {
            let projection = Arc::new(Projection::build(conn, query, downloaded_only)?);
            if allow_cache {
                pending_cache = Some((key, projection.clone()));
            }
            projection
        };
        let hashes = serde_json::to_string(&projection.cards.iter().map(|card| card.content_hash.as_str()).collect::<Vec<_>>()).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let mut downloads = vec![(false, false); projection.cards.len()];
        let mut statement = conn.prepare(include_str!("sql/subject_compaction/download_flags.sql"))?;
        let mut rows = statement.query([hashes])?;
        while let Some(row) = rows.next()? {
            downloads[row.get::<_, i64>(0)? as usize] = (row.get(1)?, row.get(2)?);
        }
        if grouped { projection.grouped_view(query, &downloads) } else { (projection.view(query, &downloads), Vec::new()) }
    };
    if let Some(tx) = tx {
        tx.commit()?;
    }
    // Never publish a graph computed from an uncommitted caller transaction.
    if let Some((key, projection)) = pending_cache {
        let mut cache = cache().lock().unwrap();
        cache.retain(|(k, _)| k.library != key.library || k.revision == key.revision);
        if cache.len() >= 8 {
            cache.pop_front();
        }
        cache.push_back((key, projection));
    }
    Ok(contents)
}

fn stored_children(conn: &Connection, location: &str, sort: BrowseChipSort) -> rusqlite::Result<Vec<BrowseRow>> {
    conn.prepare(include_str!("sql/subject_compaction/stored_children.sql"))?
        .query_map(rusqlite::params![location, chip_sort_query_value(sort)], |row| {
            Ok(BrowseRow { id: row.get(0)?, name: row.get(1)?, path: None, book_count: row.get::<_, i64>(2)? as usize, downloaded_book_count: row.get::<_, i64>(3)? as usize })
        })?
        .collect()
}
