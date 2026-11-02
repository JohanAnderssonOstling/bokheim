//! Local import commits: inspection projections, placements, observations.

use crate::SqlParam;
use crate::contributors::{ContributorNameMatcher, resolve_credits};
use crate::placements::directory_relative_path;
use crate::query_methods::{first_column, query_rows};
use crate::shared_sql::SharedSql;
use crate::sync::apply::registers::{canonical_body, record_local};
use crate::sync::{normalized_description, toc_entry_count, unix_millis};
use crate::transfer::BookTransferState;
use crate::{CreatedDirectory, Database, DatabaseError};
use book_inspection::{InspectedBook, PdfSourceIdentity};
use content_address::ContentHash;
use library_model::PublishedStagedBook;
use rusqlite::Transaction;
use include_sqlite_sql::include_sql;

// ---- commit ----
// Import commits share transaction-scoped identity and projection primitives.
//
// The backend orchestrates everything outside storage: validation, staging,
// inspection, leases, filesystem book, and thumbnail/file existence
// checks arrive as plain data. This method verifies the destination, records
// the inspection projection, the placement, and the book observation
// atomically. Shared write helpers use the caller-owned transaction.
use crate::books::audiobook::navigation_from_chapters;

include_sql!("src/import/sql/import_commit.sql");
use book_model::{BookRecord, BookTocDocument};
use library_replica::{BookAddition, BookFacts, LibraryStateEntry};
use std::collections::HashSet;
use sync_common::{BookId, DirId, StateKey};

/// Snapshot in for one import commit. All filesystem truth (book
/// outcome, thumbnail need, restore paths) is resolved by the caller.
pub struct ImportCommit {
    pub source_identity: Option<PdfSourceIdentity>,
    pub byte_checksum: Option<ContentHash>,
    pub parent_id: DirId,
    pub file_name: String,
    pub format: book_model::BookFormat,
    pub content_hash: BookId,
    pub size_bytes: u64,
    pub inspection_version: i64,
    pub inspection: Option<InspectedBook>,
    /// Filesystem book outcome, recorded verbatim.
    pub published: PublishedStagedBook,
    pub request_thumbnail: bool,
    pub restore_paths: Vec<String>,
}

/// Typed, validated file metadata prepared before acquiring the database writer.
pub(crate) struct PreparedInspection {
    metadata: book_model::BookRecord,
    navigation: Option<BookTocDocument>,
    recording: Option<metadata_contract::audible::LookupRequest>,
    pdf: Option<pdf_view_common::PdfReaderMetadata>,
}

impl PreparedInspection {
    pub(crate) fn with_identity(mut self, identity: BookId) -> Self {
        if let Some(pdf) = &mut self.pdf { pdf.content_hash = identity.to_hex(); }
        if let Some(tracks) = self.navigation.as_mut().and_then(|navigation| navigation.tracks.as_mut()) {
            for track in tracks { if track.archive_hash.is_some() { track.archive_hash = Some(identity); } }
        }
        self
    }
}

pub(crate) fn prepare_inspection(inspection: InspectedBook, content_hash: &BookId, format: book_model::BookFormat) -> Result<PreparedInspection, DatabaseError> {
    let InspectedBook { mut metadata, pdf, mut audiobook, toc } = inspection;
    metadata.description = normalized_description(&metadata.description);
    // Empty navigation still records that inspection found no entries.
    let navigation = if let Some(audio) = &mut audiobook {
        i64::try_from(audio.duration_ms).map_err(|_| DatabaseError::message("audiobook duration exceeds SQLite integer range"))?;
        Some(BookTocDocument { entries: navigation_from_chapters(&audio.chapters), duration_ms: Some(audio.duration_ms), tracks: (!audio.tracks.is_empty()).then(|| std::mem::take(&mut audio.tracks)) })
    } else {
        match format {
            book_model::BookFormat::Epub | book_model::BookFormat::Mobi | book_model::BookFormat::Pdf | book_model::BookFormat::Fb2 | book_model::BookFormat::Cbz => Some(BookTocDocument::entries(toc)),
            book_model::BookFormat::M4b | book_model::BookFormat::Mp3Folder => None,
        }
    };
    if let Some(navigation) = &navigation {
        i64::try_from(toc_entry_count(&navigation.entries)).map_err(|_| DatabaseError::message("table of contents entry count exceeds SQLite integer range"))?;
    }
    if pdf.as_ref().is_some_and(|pdf| !pdf.valid_for(&content_hash.to_hex())) {
        return Err(DatabaseError::message("PDF ingestion content hash or metadata is invalid"));
    }
    let recording = audiobook.map(|audio| audio.into_audible_lookup_request());
    Ok(PreparedInspection { metadata, navigation, recording, pdf }.with_identity(*content_hash))
}

impl Database {
    /// Whether a book already has an inspection projection at this exact
    /// format and inspection version, so the caller can skip re-inspecting it.
    pub fn has_import_inspection(&self, hash: &BookId, format: book_model::BookFormat, inspection_version: i64) -> Result<bool, DatabaseError> {
        Ok(self.connection.has_import_inspection_select(hash, format as i64, inspection_version, |row| row.get(0))?)
    }

    /// Committing this transaction is the import's success point. Callers
    /// resolve all fallible filesystem data before it.
    pub fn commit_import(&self, import: ImportCommit) -> Result<(), DatabaseError> {
        self.commit_import_with_contributor_matching(import, None)
    }

    /// Apply the caller's provisional contributor matching rule atomically with the import.
    pub fn commit_import_with_contributor_matching(&self, import: ImportCommit, matcher: Option<ContributorNameMatcher>) -> Result<(), DatabaseError> {
        self.commit_import_with_catalog_answer(import, matcher, None)
    }

    /// Save matching catalog metadata before the imported draft can be enriched.
    pub fn commit_import_with_catalog_answer(&self, mut import: ImportCommit, matcher: Option<ContributorNameMatcher>, catalog_record: Option<BookRecord>) -> Result<(), DatabaseError> {
        let content_hash = import.content_hash;
        // Pure CPU work (normalization, navigation, PDF validation) runs before
        // reserving the writer. The commit helper validates destination liveness
        // inside the transaction: another connection may delete it meanwhile.
        let prepared = import.inspection.take().map(|inspection| prepare_inspection(inspection, &content_hash, import.format)).transpose()?;
        self.write(|tx| {
            tx.pragma_update(None, "defer_foreign_keys", "ON").map_err(DatabaseError::operation)?;
            let current = first_column::<Option<String>>(|read| tx.shared_scanner_sequence(read)).map_err(DatabaseError::operation)?.flatten();
            let current = current.unwrap_or_default().parse::<i64>().map_err(|error| DatabaseError::message(format!("scan sequence is not readable: {error}")))?;
            let scan_id = current.checked_add(1).ok_or_else(|| DatabaseError::message("scan sequence overflow"))?;
            tx.scan_seq_update(scan_id).map_err(DatabaseError::operation)?;
            commit_one(tx, import, prepared, matcher)?;
            if let Some(record) = catalog_record {
                crate::enrichment::facts::record_catalog_answer(tx, &content_hash, record)?;
            }
            Ok(())
        })
    }

    pub fn commit_transfer_import(&self, import: ImportCommit, source: BookTransferState) -> Result<bool, DatabaseError> {
        self.write(|tx| commit_one_with_transfer(tx, import, None, None, Some(source)))
    }

    /// Record the reader adapter's admission version: either the current parser
    /// version or the distinct metadata-only PDF version. General transfers do
    /// not establish either admission contract merely by carrying metadata.
    pub fn commit_inspected_transfer_import(&self, import: ImportCommit, source: BookTransferState) -> Result<bool, DatabaseError> {
        let (hash, format, version) = (import.content_hash, import.format, import.inspection_version);
        self.write(|tx| {
            let created = commit_one_with_transfer(tx, import, None, None, Some(source))?;
            tx.record_import_inspection(&hash, format as i64, version).map_err(DatabaseError::operation)?;
            Ok(created)
        })
    }

    /// The caller has flushed folder markers, book bytes and proofs together.
    /// Folder registration and every book merge succeed or roll back together.
    pub fn commit_inspected_transfer_batch_with_directories(&self, imports: Vec<(ImportCommit, BookTransferState)>, sources: Vec<(String, u64, ContentHash)>, directories: Vec<CreatedDirectory>) -> Result<Vec<bool>, DatabaseError> {
        if imports.is_empty() && directories.is_empty() { return Ok(Vec::new()); }
        self.write(|tx| {
            for directory in &directories {
                if !crate::placements::can_recover_import_directory(tx, directory.id)? {
                    return Err(DatabaseError::message("import folder identity changed before commit; retry the import"));
                }
            }
            crate::placements::register_created_directories(tx, &directories)?;
            let mut pending = Vec::with_capacity(imports.len());
            let mut books = HashSet::new();
            let mut names = HashSet::new();
            let created = imports.into_iter().map(|(import, source)| {
                let (hash, format, version) = (import.content_hash, import.format, import.inspection_version);
                let name = (import.parent_id, library_replica::portable_name_key(&import.published.name));
                // A repeated book needs the previous annotation projection for
                // Foliate reconciliation. A repeated destination name needs
                // live conflict checks, including books restored in this batch.
                if books.contains(&hash) || names.contains(&name) {
                    crate::projection::project_dirty(tx)?;
                    books.clear();
                    names.clear();
                }
                books.insert(hash);
                names.insert(name);
                let (created, observation) = admit_import(tx, import, None, None, Some(source))?;
                pending.push(observation);
                tx.record_import_inspection(&hash, format as i64, version)?;
                Ok(created)
            }).collect::<Result<Vec<_>, DatabaseError>>()?;
            // Project the complete metadata set together. File observations
            // depend on live placement rows, so record them after projection.
            crate::projection::project_dirty(tx)?;
            for observation in pending { observation.record(tx)?; }
            for (path, fingerprint, checksum) in sources {
                tx.execute("INSERT INTO local_import_source(path,fingerprint,byte_hash) VALUES(?1,?2,?3) ON CONFLICT(path) DO UPDATE SET fingerprint=excluded.fingerprint,byte_hash=excluded.byte_hash", rusqlite::params![path, fingerprint.to_string(), checksum])?;
            }
            Ok(created)
        })
    }

    /// Placement rows for one book with their remembered materialized paths.
    /// The orchestrator filters these against the filesystem (without holding
    /// the writer) and hands the restore set back to the commit.
    pub fn import_restore_candidates(&self, hash: &BookId) -> Result<Vec<(library_model::RelativeBookPath, Option<String>)>, DatabaseError> {
        let rows = query_rows(|read| self.connection.restore_candidates_select(hash, read), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?))).map_err(DatabaseError::operation)?;
        let mut folders = crate::placements::FolderPaths::new(&self.connection);
        let mut candidates = Vec::new();
        for (dir_id, file_name, projected) in rows {
            if let Some(path) = folders.placement(&dir_id, &file_name)? {
                candidates.push((path, projected));
            }
        }
        Ok(candidates)
    }
}

/// One book's identity resolution and writes, inside a transaction the
/// caller owns and commits. Validate the destination here so every import
/// checks its liveness while holding the writer.
pub(crate) fn commit_one(tx: &Transaction<'_>, import: ImportCommit, prepared: Option<PreparedInspection>, matcher: Option<ContributorNameMatcher>) -> Result<(), DatabaseError> {
    commit_one_with_transfer(tx, import, prepared, matcher, None).map(|_| ())
}

fn commit_one_with_transfer(tx: &Transaction<'_>, import: ImportCommit, prepared: Option<PreparedInspection>, matcher: Option<ContributorNameMatcher>, transfer: Option<BookTransferState>) -> Result<bool, DatabaseError> {
    let (created, observation) = admit_import(tx, import, prepared, matcher, transfer)?;
    crate::projection::project_dirty(tx)?;
    observation.record(tx)?;
    Ok(created)
}

/// Admit canonical state without rebuilding derived rows after every book.
fn admit_import(tx: &Transaction<'_>, import: ImportCommit, prepared: Option<PreparedInspection>, matcher: Option<ContributorNameMatcher>, transfer: Option<BookTransferState>) -> Result<(bool, ImportObservation), DatabaseError> {
    let ImportCommit { source_identity, byte_checksum, parent_id, file_name: _file_name, format, content_hash, size_bytes, inspection_version, inspection: _inspection, published, request_thumbnail, restore_paths } = import;
    let hash = &content_hash;
    if content_hash.is_local() && byte_checksum.is_none() {
        return Err(DatabaseError::message("a local book identity requires a separate byte checksum"));
    }
    if directory_relative_path(tx, &parent_id)?.is_none() {
        return Err(DatabaseError::message(format!("library folder does not exist: {parent_id}")));
    }
    if prepared.is_none() && transfer.is_none() && !tx.has_import_inspection_select(hash, format as i64, inspection_version, |row| row.get::<_, bool>(0)).map_err(DatabaseError::operation)? {
        return Err(DatabaseError::message("book changed during import; retry the import"));
    }
    let placement_created = tx.query_row(
        "SELECT NOT EXISTS(SELECT 1 FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE bd.dir_id=?1 AND b.content_hash=?2 AND bd.deleted_at IS NULL AND b.deleted_at IS NULL)",
        rusqlite::params![parent_id.to_string(), hash],
        |row| row.get::<_, bool>(0),
    )?;
    if let Some(mut source) = transfer {
        crate::transfer::reconcile_reader_annotations(tx, &mut source)?;
        let BookTransferState { content_hash: source_hash, addition, reading, chapter_read, annotations, .. } = source;
        if source_hash != content_hash || addition.facts.format != format {
            return Err(DatabaseError::message("transferred book identity does not match its bytes"));
        }
        let has_facts: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM local_book_draft WHERE content_hash=?1) OR EXISTS(SELECT 1 FROM sync_book_record WHERE state_kind='book_add' AND content_hash=?1)", [hash], |row| row.get(0))?;
        tx.ingestion_ensure_book(hash)?;
        record_local(tx, LibraryStateEntry::BookLifecycle { content_hash, value: library_replica::BookLifecycleState::Present })?;
        if !has_facts {
            if addition.enrichment.is_some() {
                record_local(tx, LibraryStateEntry::BookAdd { content_hash, value: addition })?;
            } else {
                crate::sync::apply::records::stage(tx, hash, addition)?;
            }
        }
        crate::transfer::merge_transfer_progress(tx, content_hash, reading, chapter_read, annotations)?;
    }
    if let Some(prepared) = prepared {
        // The first inspection supplies the draft. Projection owns all
        // metadata writes; reimports retain the same immutable file facts.
        let PreparedInspection { metadata, navigation, recording, pdf } = prepared;
        // One immutable addition carries every file fact; lifecycle and
        // placement remain independently mutable.
        tx.ingestion_ensure_book(&hash)?;
        let added_at: i64 = tx.ingestion_added_at(&hash, &(unix_millis()?), |row| row.get(0))?;
        record_local(tx, LibraryStateEntry::BookLifecycle { content_hash, value: library_replica::BookLifecycleState::Present })?;
        // Re-imports may change placements and local observations, but cannot
        // rewrite the immutable addition for the same file hash.
        if canonical_body(tx, "book_add", &StateKey::Book(*hash), "")?.is_none() {
            let mut metadata = metadata;
            metadata.contributors = resolve_credits(tx, &metadata.contributors, matcher)?.into_iter().map(|(_, credit)| credit).collect();
            crate::sync::apply::records::stage(tx, hash, BookAddition { facts: BookFacts { added_at: u64::try_from(added_at).map_err(DatabaseError::operation)?, format }, record: metadata, navigation, pdf, recording, enrichment: None })?;
        }
        tx.record_import_inspection(hash, format as i64, inspection_version).map_err(DatabaseError::operation)?;
    }
    // The published file name is authoritative: connection-free book
    // may have uniquified it while this transaction was not held. Browser
    // imports have no placement filesystem, so they skip this recheck.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let conflict: bool = tx.shared_book_name_conflict(&parent_id.to_string(), &library_replica::portable_name_key(&published.name), hash, |row| row.get(0)).map_err(DatabaseError::operation)?;
        if conflict {
            return Err(DatabaseError::message("destination filename changed during import; retry the import"));
        }
    }
    crate::projection::place_book(tx, &parent_id, hash, &published.name)?;
    Ok((placement_created, ImportObservation { content_hash, parent_id, source_identity, byte_checksum, size_bytes, published, request_thumbnail, restore_paths }))
}

struct ImportObservation {
    content_hash: BookId,
    parent_id: DirId,
    source_identity: Option<PdfSourceIdentity>,
    byte_checksum: Option<ContentHash>,
    size_bytes: u64,
    published: PublishedStagedBook,
    request_thumbnail: bool,
    restore_paths: Vec<String>,
}

impl ImportObservation {
    fn record(self, tx: &Transaction<'_>) -> Result<(), DatabaseError> {
        let Self { content_hash, parent_id, source_identity, byte_checksum, size_bytes, published, request_thumbnail, restore_paths } = self;
        let hash = &content_hash;
        crate::content_identity::record_origin(tx, content_hash, source_identity)?;
        if let Some(checksum) = byte_checksum.or_else(|| content_hash.content_hash()) {
            crate::content_identity::observe_content(tx, content_hash, parent_id, checksum)?;
        }
        let revision = if published.published {
            crate::native::observations::Revision::Local { size: i64::try_from(size_bytes).map_err(|_| DatabaseError::message("book size exceeds SQLite range"))? }
        } else {
            crate::native::observations::Revision::Retained
        };
        crate::native::observations::record_file(tx, &parent_id.to_string(), hash, &published.relative_path, published.fingerprint.as_deref(), revision)?;
        if request_thumbnail {
            tx.shared_request_thumbnail(hash).map_err(DatabaseError::operation)?;
        }
        for path in &restore_paths {
            tx.shared_placements_queue_file_work("materialize", hash, path).map_err(DatabaseError::operation)?;
        }
        Ok(())
    }
}

include_sqlite_sql::include_sql!("src/import/sql/ingestion.sql");

#[cfg(test)]
mod tests {
    use super::*;
    use book_model::{Annotation, AnnotationAnchor, AnnotationStyle, BookFormat};
    use library_replica::{BookFacts, EnrichmentResult};
    use crate::transfer::ReaderAnnotationSource;
    use sync_common::ROOT_DIR_ID;

    fn database() -> Database {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        db
    }

    fn entry(bytes: &[u8], name: &str, note_id: &str) -> (ImportCommit, BookTransferState) {
        let checksum = blake3::hash(bytes).into();
        let hash = BookId::from(blake3::hash(bytes));
        let mut record = BookRecord { metadata: Default::default(), description: String::new() };
        record.title = name.into();
        let source = BookTransferState {
            content_hash: hash,
            byte_checksum: Some(checksum),
            addition: BookAddition { facts: BookFacts { added_at: 1, format: BookFormat::Epub }, record, navigation: Some(BookTocDocument::entries(Vec::new())), pdf: None, recording: None, enrichment: Some(EnrichmentResult::NoMatch) },
            reading: None,
            chapter_read: None,
            annotations: vec![Annotation {
                id: note_id.into(),
                content_hash: hash,
                anchor: AnnotationAnchor::epub_cfi("epubcfi(/6/2!/4/2,/1:0,/1:1)"),
                exact_text: "Text".into(), style: AnnotationStyle::Underline,
                color: 0xffffff, note: "Keep original note".into(),
                created_at: 7, modified_at: 9, toc_ordinal: None, progress: Some(42.0), deleted: false,
            }],
            annotation_source: Some(ReaderAnnotationSource::Foliate),
        };
        (ImportCommit {
            source_identity: None, byte_checksum: Some(checksum), parent_id: ROOT_DIR_ID,
            file_name: name.into(), format: BookFormat::Epub, content_hash: hash,
            size_bytes: bytes.len() as u64, inspection_version: -2, inspection: None,
            published: PublishedStagedBook {
                name: name.into(), relative_path: format!("/{name}"),
                published: true, fingerprint: Some("1".into()),
            },
            request_thumbnail: true, restore_paths: Vec::new(),
        }, source)
    }

    #[test]
    fn batch_matches_sequential_imports_including_duplicate_foliate_notes_and_file_observations() {
        let sequential = database();
        let batched = database();
        let entries = || {
            let mut duplicate = entry(b"first", "first.epub", "external-copy");
            duplicate.1.annotations[0].note = "Do not replace the first note".into();
            vec![entry(b"first", "first.epub", "first-note"), entry(b"second", "second.epub", "second-note"), duplicate]
        };
        let expected = entries().into_iter().map(|(import, source)| sequential.commit_inspected_transfer_import(import, source).unwrap()).collect::<Vec<_>>();
        assert_eq!(batched.commit_inspected_transfer_batch_with_directories(entries(), Vec::new(), Vec::new()).unwrap(), expected);
        assert_eq!(expected, vec![true, true, false]);
        for bytes in [b"first".as_slice(), b"second".as_slice()] {
            let checksum = blake3::hash(bytes).into();
            let hash = BookId::from(blake3::hash(bytes));
            assert_eq!(serde_json::to_value(batched.book_transfer_state(hash).unwrap()).unwrap(), serde_json::to_value(sequential.book_transfer_state(hash).unwrap()).unwrap());
            assert_eq!(batched.book_paths(&hash).unwrap(), sequential.book_paths(&hash).unwrap());
            assert_eq!(batched.local_content_revision(hash, ROOT_DIR_ID, checksum).unwrap().as_deref(), Some("1"));
        }
        let first = BookId::from(blake3::hash(b"first"));
        assert_eq!(batched.annotations(first).unwrap().len(), 1);
        assert_eq!(batched.annotations(first).unwrap()[0].note, "Keep original note");
    }

    #[test]
    fn batch_rejects_portable_name_conflicts_and_rolls_back_reader_state() {
        let db = database();
        let error = db.commit_inspected_transfer_batch_with_directories(vec![entry(b"first", "BOOK.epub", "first-note"), entry(b"second", "book.epub", "second-note")], Vec::new(), Vec::new()).unwrap_err();
        assert!(error.to_string().contains("destination filename changed"), "{error}");
        assert_eq!(db.book_count().unwrap(), 0);
        assert!(db.annotations(BookId::from(blake3::hash(b"first"))).unwrap().is_empty());
    }
}
