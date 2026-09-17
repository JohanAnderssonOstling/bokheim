//! Prepare an enrichment page against a read snapshot, then persist it in a
//! separate short write transaction. A stale book skips all of its actions,
//! including completion markers. Multiple actions for one book merge in order.
use super::external_metadata as old;
use crate::storage_values::invalid_text_column;
use include_sqlite_sql::include_sql;
use book_model::AgentId;
use book_model::ExternalAuthorId;
use book_model::BookSubject;
use book_model::Scheme;
use book_model::Scope;
use book_model::{Identifier, BookMetadata};
use metadata_contract::{PersonEntity, PlaceEntity, RichWorkMetadataMatch};
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::Transaction;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use sync_common::ContentHash;

type Action = Box<dyn FnOnce(&Transaction<'_>, &mut super::authors::IdentityCache) -> rusqlite::Result<()> + Send>;
#[derive(PartialEq)]
struct Snapshot {
    row: i64,
    book: Vec<u8>,
    title: String,
    description: String,
    contributors: Vec<u8>,
}
fn snapshot(conn: &Connection, hash: ContentHash) -> rusqlite::Result<Option<Snapshot>> {
    conn.enrichment_snapshot(hash.as_str(), |r| Ok(Snapshot { row: r.get(0)?, book: r.get(1)?, title: r.get(2)?, description: r.get(3)?, contributors: r.get(4)? })).optional()
}
struct Book {
    original: Snapshot,
    book: BookMetadata,
    description: String,
}
struct Group {
    hash: Option<ContentHash>,
    actions: Vec<Action>,
    count: usize,
}
pub struct Preparation<'a> {
    conn: &'a Connection,
    books: HashMap<ContentHash, Book>,
    groups: Vec<Group>,
    active: Group,
}
pub struct PreparedBatch {
    books: Vec<(ContentHash, Snapshot, Option<super::metadata::PreparedBook>, Option<String>)>,
    groups: Vec<Group>,
}

impl std::ops::Deref for Preparation<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.conn
    }
}
impl<'a> Preparation<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn, books: HashMap::new(), groups: Vec::new(), active: Group { hash: None, actions: Vec::new(), count: 0 } }
    }
    pub fn push(&mut self, prepare: impl FnOnce(&mut Self) -> rusqlite::Result<usize>) -> rusqlite::Result<()> {
        self.active.count = prepare(self)?;
        self.groups.push(std::mem::replace(&mut self.active, Group { hash: None, actions: Vec::new(), count: 0 }));
        Ok(())
    }
    fn book(&mut self, hash: ContentHash) -> rusqlite::Result<Option<&mut Book>> {
        if self.active.hash.is_some_and(|previous| previous != hash) {
            return Err(invalid_text_column(0, "an enrichment action must target one book"));
        }
        self.active.hash = Some(hash);
        if !self.books.contains_key(&hash) {
            let Some(original) = snapshot(self.conn, hash)? else { return Ok(None) };
            let book = old::decode_book_metadata(&original.book)?;
            self.books.insert(hash, Book { description: original.description.clone(), original, book });
        }
        Ok(self.books.get_mut(&hash))
    }
    fn action(&mut self, action: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<()> + Send + 'static) {
        self.active.actions.push(Box::new(move |tx, identities| {
            let result = action(tx);
            identities.invalidate();
            result
        }));
    }
    fn bookkeeping(&mut self, action: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<()> + Send + 'static) {
        self.active.actions.push(Box::new(move |tx, _| action(tx)));
    }
    pub fn finish(self) -> rusqlite::Result<PreparedBatch> {
        let books = self
            .books
            .into_iter()
            .map(|(hash, book)| {
                let previous = old::decode_book_metadata(&book.original.book)?;
                let changed = book.book != previous;
                let prepared = changed.then(|| super::metadata::PreparedBook::new(&book.book, &previous)).transpose()?;
                let description = (book.description != book.original.description).then_some(book.description);
                Ok((hash, book.original, prepared, description))
            })
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(PreparedBatch { books, groups: self.groups })
    }
    /// For cache-only actions and tests; never use for metadata computation.
    pub fn persist(&mut self, action: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<()> + Send + 'static) {
        self.action(action);
    }
}
impl PreparedBatch {
    pub fn apply(self, conn: &mut Connection) -> rusqlite::Result<usize> {
        let tx = crate::schema::library_write_transaction(conn)?;
        let mut identities = super::authors::IdentityCache::default();
        let (count, skipped) = super::metadata_batch::run(&tx, || {
            let mut allowed = HashSet::new();
            // Check every input before our own writes alter any rows or identities.
            for (hash, original, _, _) in &self.books {
                let current: bool = tx.enrichment_snapshot_is_current(hash.as_str(), original.row, &original.book, &original.title, &original.description, &original.contributors, |r| r.get(0))?;
                if current {
                    allowed.insert(*hash);
                }
            }
            let skipped = self.books.len() - allowed.len();
            for (hash, _, book, description) in self.books {
                if !allowed.contains(&hash) {
                    continue;
                }
                if let Some(book) = book {
                    book.apply(&tx, hash, description.as_deref(), &mut identities)?;
                } else if let Some(description) = description {
                    super::descriptions::set(&tx, hash, &description)?;
                }
            }
            let mut count = 0;
            for group in self.groups {
                if group.hash.is_some_and(|hash| !allowed.contains(&hash)) {
                    continue;
                }
                for action in group.actions {
                    action(&tx, &mut identities)?;
                }
                count += group.count;
            }
            Ok((count, skipped))
        })?;
        tx.commit()?;
        if skipped > 0 {
            log::info!("enrichment batch skipped {skipped} stale books; completion markers left untouched");
        }
        Ok(count)
    }
}

pub fn record_external_metadata_attempt(p: &mut Preparation<'_>, hash: &ContentHash, provider: &str, identifier: &str, status: &str, detail: Option<&str>) -> rusqlite::Result<()> {
    if p.book(*hash)?.is_none() {
        return Ok(());
    }
    let (hash, provider, identifier, status, detail) = (*hash, provider.to_owned(), identifier.to_owned(), status.to_owned(), detail.map(|v| v.chars().take(1024).collect::<String>()));
    p.bookkeeping(move |tx| old::record_external_metadata_attempt(tx, &hash, &provider, &identifier, &status, detail.as_deref()));
    Ok(())
}
pub fn apply_description(p: &mut Preparation<'_>, hash: ContentHash, provider: &str, isbns: &str, description: Option<&str>, status: &str) -> rusqlite::Result<usize> {
    let Some(book) = p.book(hash)? else { return Ok(0) };
    let description = description.map(super::metadata::normalized_description).unwrap_or_default();
    let changed = book.description.trim().is_empty() && !description.is_empty();
    if changed {
        book.description = description;
    }
    record_external_metadata_attempt(p, &hash, provider, isbns, status, None)?;
    Ok(usize::from(changed))
}
pub fn apply_rich_enrichment_in_transaction(
    p: &mut Preparation<'_>, hash: ContentHash, provider: &str, isbn: &str, matched: Option<&RichWorkMetadataMatch>, places: &[PlaceEntity], people: &[PersonEntity], subjects: Vec<BookSubject>, status: &str, detail: Option<&str>,
    replace: bool,
) -> rusqlite::Result<usize> {
    // All merging and classification projection takes place before the writer.
    let Some(book) = p.book(hash)? else { return Ok(0) };
    if replace {
        let fallback = book
            .book
            .subjects
            .iter()
            .filter(|s| s.authority().is_some_and(|a| a.eq_ignore_ascii_case("lcc")) && matches!(s.source(), "dc:subject" | "openlibrary:metadata:sibling-edition" | "openlibrary:metadata:work"))
            .map(|s| ("lcc".to_owned(), s.code().unwrap_or_else(|| s.name()).to_owned()))
            .collect::<BTreeSet<_>>();
        book.book.subjects.retain(|s| !fallback.contains(&(s.authority().unwrap_or_default().to_ascii_lowercase(), s.code().unwrap_or_else(|| s.name()).to_owned())));
    }
    let mut count = 0;
    for subject in subjects {
        if !book.book.subjects.contains(&subject) {
            book.book.subjects.push(subject);
            count += 1;
        }
    }
    if book.description.trim().is_empty() {
        if let Some(description) = matched.and_then(|m| m.description.as_deref()) {
            let description = super::metadata::normalized_description(description);
            if !description.is_empty() {
                book.description = description;
                count += 1;
            }
        }
    }
    let (provider, isbn, mut matched, places, people, status, detail) = (provider.to_owned(), isbn.to_owned(), matched.cloned(), places.to_vec(), people.to_vec(), status.to_owned(), detail.map(str::to_owned));
    if let Some(matched) = &mut matched {
        matched.description = None;
    }
    // Reuse the entity/attempt SQL path, with book and description
    // mutation disabled. They have already been prepared above.
    p.action(move |tx| old::apply_rich_enrichment_in_transaction(tx, hash, &provider, &isbn, matched.as_ref(), &places, &people, Vec::new(), &status, detail.as_deref(), false).map(|_| ()));
    Ok(count)
}

pub fn apply_edition_identity_in_transaction(p: &mut Preparation<'_>, hash: ContentHash, provider: &str, identifier: &str, isbn: Option<&str>, status: &str, detail: Option<&str>) -> rusqlite::Result<bool> {
    edition(p, hash, provider, identifier, isbn, status, detail, "openlibrary-edition-match")
}
fn edition(p: &mut Preparation<'_>, hash: ContentHash, provider: &str, identifier: &str, isbn: Option<&str>, status: &str, detail: Option<&str>, provenance: &str) -> rusqlite::Result<bool> {
    let Some(book) = p.book(hash)? else { return Ok(false) };
    // Match the derived book_identifier view, including ISBNs embedded in
    // source subjects, without rebuilding that view under the write lock.
    let mut lookup = book.book.clone();
    lookup.infer_embedded_subject_isbns().map_err(|e| invalid_text_column(0, e.to_string()))?;
    let changed = isbn.is_some() && !lookup.identifiers.iter().any(|id| id.scheme() == &Scheme::Isbn && id.canonical_value().is_some());
    if changed {
        book.book.identifiers.push(Identifier::new(isbn.unwrap(), Scheme::Isbn, Scope::Edition, provenance).map_err(|e| invalid_text_column(0, e.to_string()))?);
    }
    record_external_metadata_attempt(p, &hash, provider, identifier, status, detail)?;
    Ok(changed)
}
pub fn apply_filename_isbn_in_transaction(p: &mut Preparation<'_>, hash: ContentHash, provider: &str, isbn: &str) -> rusqlite::Result<bool> {
    let isbn = book_model::canonical_isbn(isbn).ok_or_else(|| invalid_text_column(0, "filename ISBN is invalid"))?;
    edition(p, hash, provider, &isbn, Some(&isbn), "updated", Some("checksum-valid ISBN extracted from filename"), "filename-isbn")
}
pub fn apply_author_enrichment_in_transaction(p: &mut Preparation<'_>, hash: ContentHash, provider: &str, isbn: &str, resolutions: &[(usize, String, ExternalAuthorId)], status: &str, detail: Option<&str>) -> rusqlite::Result<usize> {
    if p.book(hash)?.is_none() {
        return Ok(0);
    }
    let count = resolutions.len();
    let (provider, isbn, resolutions, status, detail) = (provider.to_owned(), isbn.to_owned(), resolutions.to_vec(), status.to_owned(), detail.map(str::to_owned));
    p.action(move |tx| old::apply_author_enrichment_in_transaction(tx, hash, &provider, &isbn, &resolutions, &status, detail.as_deref()).map(|_| ()));
    Ok(count)
}
pub fn apply_authoritative_authors_in_transaction(p: &mut Preparation<'_>, hash: ContentHash, provider: &str, isbn: &str, authors: &[(usize, String, ExternalAuthorId)]) -> rusqlite::Result<usize> {
    if p.book(hash)?.is_none() || authors.is_empty() {
        return Ok(0);
    }
    let count = authors.len();
    let mut credits = Vec::new();
    p.conn.non_authoritative_contributors(hash.as_str(), |r| {
        let (id, name, sort, role) = (r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Vec<u8>>(3)?);
        credits.push(
            book_model::Contributor::with_id(AgentId::parse_str(&id).map_err(|e| invalid_text_column(0, e.to_string()))?, &name, sort, book_model::MarcRelatorCode(role.try_into().map_err(|_| invalid_text_column(0, "invalid role"))?))
                .map_err(|e| invalid_text_column(0, e.to_string()))?,
        );
        Ok(())
    })?;
    let (provider, isbn, authors) = (provider.to_owned(), isbn.to_owned(), authors.to_vec());
    p.active.actions.push(Box::new(move |tx, identities| {
        let mut authors = authors.iter().map(|(_, name, id)| identities.authoritative(tx, name, id)).collect::<rusqlite::Result<Vec<_>>>()?;
        authors.extend(credits);
        let stored = identities.contributors(tx, &authors)?;
        super::authors::replace_book_contributors(tx, hash, &stored)?;
        old::record_external_metadata_attempt(tx, &hash, &provider, &isbn, "updated", Some("authoritative Open Library author credits from unambiguous ISBN match"))?;
        Ok(())
    }));
    Ok(count)
}
pub fn merge_pages(p: &mut Preparation<'_>, hash: ContentHash, evidence: &BookMetadata) -> rusqlite::Result<bool> {
    let Some(book) = p.book(hash)? else { return Ok(false) };
    let before = book.book.clone();
    for id in &evidence.identifiers {
        if !book.book.identifiers.contains(id) {
            book.book.identifiers.push(id.clone());
        }
    }
    for s in &evidence.subjects {
        if !book.book.subjects.contains(s) {
            book.book.subjects.push(s.clone());
        }
    }
    for property in &evidence.properties {
        if !book.book.properties.contains(property) {
            book.book.properties.push(property.clone());
        }
    }
    Ok(book.book != before)
}
pub fn cache_isbn_json(p: &mut Preparation<'_>, isbn: &str, json: &str) -> rusqlite::Result<()> {
    let (isbn, json) = (isbn.to_owned(), json.to_owned());
    p.bookkeeping(move |tx| super::subject_pipeline::cache_isbn_json(tx, &isbn, &json));
    Ok(())
}
pub fn save(p: &mut Preparation<'_>, hash: ContentHash, state: &book_enrichment::SubjectPipeline, expected: &BookMetadata, title: &str, complete: bool, delay: i64) -> rusqlite::Result<()> {
    if p.book(hash)?.is_none() {
        return Ok(());
    }
    let checkpoint = super::subject_pipeline::prepare_save(hash, state, expected, title, complete, delay)?;
    p.bookkeeping(move |tx| checkpoint.apply(tx));
    Ok(())
}

include_sql!("src/prepared_enrichment/sql/enrichment_snapshot_guard.sql");
