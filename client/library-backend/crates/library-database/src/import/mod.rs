//! Local import commits: inspection projections, placements, observations.

use crate::placements::directory_relative_path;
use crate::sync::apply::registers::{canonical_body, record_local};
use crate::timing::PhaseTimer;
use crate::shared_sql::SharedSql;
use crate::sync::{normalized_description, toc_entry_count, unix_millis};
use crate::{Database, DatabaseError};
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
use crate::books::audiobook::AudiobookSql;
use crate::sync::SyncApplySql;

include_sql!("src/import/sql/import_commit.sql");
use sync_common::{ContentHash, DirId};

/// Filesystem book outcome, recorded verbatim. Produced by the
/// backend's connection-free publish step from a database snapshot.
pub struct PublishedImport {
    pub name: String,
    pub relative_path: String,
    pub published: bool,
    pub fingerprint: Option<String>,
}

/// Snapshot in for one import commit. All filesystem truth (book
/// outcome, thumbnail need, restore paths) is resolved by the caller.
pub struct ImportCommit {
    pub parent_id: DirId,
    pub file_name: String,
    pub format: book_model::BookFormat,
    pub content_hash: ContentHash,
    pub checksum: ContentHash,
    pub size_bytes: u64,
    pub inspection_version: i64,
    pub inspection: Option<book_metadata::InspectedBook>,
    pub published: PublishedImport,
    pub request_thumbnail: bool,
    pub restore_paths: Vec<String>,
}

/// Chapter-derived table of contents and the recording evidence sent to the
/// metadata server.
pub(crate) struct PreparedAudiobookInsert {
    duration_ms: i64,
    tracks_json: Option<String>,
    request_json: String,
    chapters_toc_json: String,
}

/// Everything [`Database::commit_import`] can derive from an [`book_metadata::InspectedBook`]
/// without touching storage: title/description normalization, subject projection,
/// encoded TOC and audiobook chapter JSON, and PDF checksum validation. Computed before
/// the transaction opens so the writer is held only for identity resolution
/// and the actual writes, not for CPU-bound parsing and serialization.
pub(crate) struct PreparedInspection {
    metadata: book_model::BookRecord,
    projection: crate::sync::apply::PreparedBook,
    description: String,
    toc: Option<(i64, String)>,
    audiobook: Option<PreparedAudiobookInsert>,
    pdf_bytes: Option<Vec<u8>>,
}

pub(crate) fn prepare_inspection(inspection: &book_metadata::InspectedBook, checksum: &ContentHash, format: book_model::BookFormat) -> Result<PreparedInspection, DatabaseError> {
    let mut timing = PhaseTimer::new("prepare_inspection");
    let mut metadata = inspection.metadata.clone();
    book_enrichment::title_identity::normalize(&mut metadata).map_err(|error| DatabaseError::message(format!("book title is not readable: {error}")))?;
    let description = normalized_description(&metadata.description);
    timing.mark("normalize_metadata_and_description");
    // A document that yielded nothing is still recorded, so that nothing
    // parses it again hoping for a different answer.
    let toc = match format {
        book_model::BookFormat::Epub | book_model::BookFormat::Mobi | book_model::BookFormat::Pdf => {
            let entry_count = i64::try_from(toc_entry_count(&inspection.toc)).map_err(|_| DatabaseError::message("table of contents entry count exceeds SQLite integer range"))?;
            let toc_json = serde_json::to_string(&inspection.toc).map_err(DatabaseError::operation)?;
            Some((entry_count, toc_json))
        }
        book_model::BookFormat::M4b | book_model::BookFormat::Mp3Folder => None,
    };
    let audiobook = inspection
        .audiobook
        .as_ref()
        .map(|audiobook| -> Result<PreparedAudiobookInsert, DatabaseError> {
            let duration_ms = i64::try_from(audiobook.duration_ms).map_err(|_| DatabaseError::message("audiobook duration exceeds SQLite integer range"))?;
            let entries = navigation_from_chapters(&audiobook.chapters);
            let _ = i64::try_from(toc_entry_count(&entries)).map_err(|_| DatabaseError::message("table of contents entry count exceeds SQLite integer range"))?;
            let chapters_toc_json = serde_json::to_string(&entries).map_err(DatabaseError::operation)?;
            let tracks_json = (!audiobook.tracks.is_empty()).then(|| serde_json::to_string(&audiobook.tracks)).transpose().map_err(DatabaseError::operation)?;
            let request_json = serde_json::to_string(&audiobook.audible_lookup_request()).map_err(DatabaseError::operation)?;
            Ok(PreparedAudiobookInsert { duration_ms, tracks_json, request_json, chapters_toc_json })
        })
        .transpose()?;
    let pdf_bytes = inspection
        .pdf
        .as_ref()
        .map(|pdf| -> Result<Vec<u8>, DatabaseError> {
            if !pdf.valid_for(checksum.as_str()) {
                return Err(DatabaseError::message("PDF ingestion checksum or metadata is invalid"));
            }
            serde_json::to_vec(pdf).map_err(DatabaseError::operation)
        })
        .transpose()?;
    timing.mark("encode_reader_metadata");
    let projection = crate::sync::apply::project_book(&metadata.book)?;
    timing.mark("project_book");
    Ok(PreparedInspection { metadata, projection, description, toc, audiobook, pdf_bytes })
}

impl Database {
    /// Whether a book already has an inspection projection at this exact
    /// format and inspection version, so the caller can skip re-inspecting it.
    pub fn has_import_inspection(&self, hash: &ContentHash, format: book_model::BookFormat, inspection_version: i64) -> Result<bool, DatabaseError> {
        let mut inspected = false;
        self.connection.has_import_inspection_select(hash.as_str(), format as i64, inspection_version, |row| {
            inspected = row.get(0)?;
            Ok(())
        })?;
        Ok(inspected)
    }

    /// Committing this transaction is the import's success point. Callers
    /// resolve all fallible filesystem data before it.
    pub fn commit_import(&self, import: ImportCommit) -> Result<(), DatabaseError> {
        {
            let imports: Vec<ImportCommit> = vec![import];

            if imports.is_empty() {
                return Ok(());
            }
            // Pure CPU work (normalization, TOC/chapter JSON, PDF validation)
            // runs before reserving the writer. Destination liveness is checked
            // inside the transaction: another connection may delete it meanwhile.
            let mut prepared_imports = Vec::with_capacity(imports.len());
            for import in imports {
                let prepared = import.inspection.as_ref().map(|inspection| prepare_inspection(inspection, &import.checksum, import.format)).transpose()?;
                prepared_imports.push((import, prepared));
            }
            self.with_write_transaction(|tx| {
                tx.pragma_update(None, "defer_foreign_keys", "ON").map_err(DatabaseError::operation)?;
                let mut current: Option<String> = None;
                tx.shared_scanner_sequence(|row| {
                    current = row.get(0)?;
                    Ok(())
                })
                .map_err(DatabaseError::operation)?;
                let current = current.unwrap_or_default().parse::<i64>().map_err(|error| DatabaseError::message(format!("scan sequence is not readable: {error}")))?;
                let scan_id = current.checked_add(1).ok_or_else(|| DatabaseError::message("scan sequence overflow"))?;
                tx.scan_seq_update(scan_id).map_err(DatabaseError::operation)?;
                for (import, prepared) in prepared_imports {
                    if directory_relative_path(tx, &import.parent_id)?.is_none() {
                        return Err(DatabaseError::message(format!("library folder does not exist: {}", import.parent_id)));
                    }
                    commit_one(tx, import, prepared)?;
                }
                Ok(crate::transactions::WriteOutcome::Commit(()))
            })
        }
    }

    /// Placement rows for one book with their remembered materialized paths.
    /// The orchestrator filters these against the filesystem (without holding
    /// the writer) and hands the restore set back to the commit.
    pub fn import_restore_candidates(&self, hash: &ContentHash) -> Result<Vec<(library_model::RelativeBookPath, Option<String>)>, DatabaseError> {
        let mut rows = Vec::new();
        self.connection
            .restore_candidates_select(hash.as_str(), |row| {
                rows.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?));
                Ok(())
            })
            .map_err(DatabaseError::operation)?;
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
/// caller owns and commits. Acquiring the writer before this runs means a
/// directory can vanish between [`Database::commit_imports`]'s pre-check and
/// here, so the destination is re-validated.
pub(crate) fn commit_one(tx: &rusqlite::Transaction<'_>, import: ImportCommit, prepared: Option<PreparedInspection>) -> Result<(), DatabaseError> {
    let mut timing = PhaseTimer::new("import_book");
    let ImportCommit { parent_id, file_name: _file_name, format, content_hash, checksum, size_bytes, inspection_version, inspection: _inspection, published, request_thumbnail, restore_paths } = import;
    let hash = content_hash.as_str();
    if directory_relative_path(tx, &parent_id)?.is_none() {
        return Err(DatabaseError::message(format!("library folder does not exist: {parent_id}")));
    }
    if prepared.is_none() {
        let mut inspected: Option<bool> = None;
        tx.has_import_inspection_select(hash, format as i64, inspection_version, |row| {
            inspected = row.get(0)?;
            Ok(())
        })
        .map_err(DatabaseError::operation)?;
        let inspected = inspected.ok_or(rusqlite::Error::QueryReturnedNoRows).map_err(DatabaseError::operation)?;
        if !inspected {
            return Err(DatabaseError::message("book changed during import; retry the import"));
        }
    }
    timing.mark("validate_destination");
    if let Some(prepared) = prepared {
        // Fresh inspection projects exactly like a rescan: normalize the
        // title identity, resolve contributor and publisher agents with
        // provisional-name reuse, then replace every projection row.
        let PreparedInspection { metadata, projection, description, toc, audiobook, pdf_bytes } = prepared;
        // The file's facts become local register values; projection writes
        // the tables when the transaction commits.
        tx.prepare_cached(include_str!("sql/upsert_scanned_book.sql"))?.execute(rusqlite::named_params! {
            ":content_hash": hash,
        })?;
        let added_at: i64 = tx.query_row("SELECT COALESCE(added_at,?2) FROM book WHERE content_hash=?1", rusqlite::params![hash, unix_millis()?], |row| row.get(0))?;
        record_local(tx, library_replica::LibraryStateEntry::BookLifecycle { content_hash, value: library_replica::BookLifecycleState::Present })?;
        record_local(tx, library_replica::LibraryStateEntry::BookFacts { content_hash, value: library_replica::BookFacts { added_at: u64::try_from(added_at).map_err(DatabaseError::operation)?, format } })?;
        let value = {
            use crate::metadata::*;
            let title: &str = &metadata.title;
            let subtitle: Option<&str> = metadata.subtitle.as_deref();
            let contributors: &[Contributor] = &metadata.contributors;
            let mut book: BookMetadata = projection.metadata;

            let previous = match canonical_body(tx, "metadata", hash, "")? {
                Some((bytes, _)) => match sync_common::wire::decode::<LibraryStateEntry>(&bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? {
                    LibraryStateEntry::Metadata { value, .. } => value.contributors,
                    _ => Vec::new(),
                },
                None => Vec::new(),
            };
            let mut credits = Vec::with_capacity(contributors.len());
            for credit in contributors {
                let Some(name) = parse_author_name_opt(credit.name())? else {
                    continue;
                };
                let kept = previous.get(credits.len()).filter(|old| parse_author_name_opt(old.name()).ok().flatten().is_some_and(|old| old.match_key() == name.match_key()));
                let id = match kept {
                    Some(old) => old.agent_id(),
                    None => crate::contributors::resolve_identity(tx, credit.agent_id(), &name, IdentityPolicy::ReuseProvisional)?.1,
                };
                credits.push(Contributor::with_id(id, credit.name(), credit.role_code()).map_err(|error| DatabaseError::message(format!("contributor credit is not writable: {error}")))?);
            }
            book.publishers = crate::contributors::resolve_publishers(tx, &book.publishers, IdentityPolicy::ReuseProvisional)?;
            BookDetails { title: title.to_owned(), subtitle: subtitle.map(str::to_owned), contributors: credits, book }
        };
        record_local(tx, library_replica::LibraryStateEntry::Metadata { content_hash, value })?;
        timing.mark("write_metadata");
        // A book without a description gets no register unless it had one.
        if !description.is_empty() || canonical_body(tx, "description", hash, "")?.is_some() {
            record_local(tx, library_replica::LibraryStateEntry::Description { content_hash, value: description })?;
        }
        timing.mark("write_description");
        let mut navigation = None;
        if let Some(audiobook) = audiobook {
            navigation = Some(book_model::BookTocDocument {
                entries: serde_json::from_str(&audiobook.chapters_toc_json).map_err(DatabaseError::operation)?,
                duration_ms: Some(u64::try_from(audiobook.duration_ms).map_err(DatabaseError::operation)?),
                tracks: audiobook.tracks_json.as_deref().map(serde_json::from_str).transpose().map_err(DatabaseError::operation)?,
            });
            tx.audiobook_record_recording(hash, &audiobook.request_json).map_err(DatabaseError::operation)?;
        }
        if let Some((_entry_count, toc_json)) = toc {
            navigation = Some(book_model::BookTocDocument { entries: serde_json::from_str(&toc_json).map_err(DatabaseError::operation)?, duration_ms: None, tracks: None });
        }
        if let Some(value) = navigation {
            record_local(tx, library_replica::LibraryStateEntry::BookToc { content_hash, value })?;
        }
        if let Some(pdf_bytes) = pdf_bytes {
            let value = serde_json::from_slice(&pdf_bytes).map_err(DatabaseError::operation)?;
            record_local(tx, library_replica::LibraryStateEntry::PdfReaderMetadata { content_hash, value })?;
        }
        tx.record_import_inspection(hash, format as i64, inspection_version).map_err(DatabaseError::operation)?;
        timing.mark("reader_metadata_and_inspection");
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
    timing.mark("check_placement_collision");
    crate::projection::place_book(tx, &parent_id, &ContentHash::new(hash), &published.name)?;
    // File recording and later imports in this batch read the live placement.
    crate::projection::project_dirty(tx)?;
    timing.mark("write_placement");
    let revision = if published.published {
        crate::native::observations::Revision::Local { checksum: &checksum, size: i64::try_from(size_bytes).map_err(|_| DatabaseError::message("book size exceeds SQLite range"))? }
    } else {
        crate::native::observations::Revision::Retained
    };
    crate::native::observations::record_file(tx, &parent_id.to_string(), &ContentHash::new(hash), &published.relative_path, published.fingerprint.as_deref(), revision)?;
    timing.mark("record_path_and_fingerprint");
    if request_thumbnail {
        tx.shared_request_thumbnail(hash).map_err(DatabaseError::operation)?;
    }
    timing.mark("queue_thumbnail");
    timing.mark("version_and_upload_work");
    for path in &restore_paths {
        tx.shared_placements_queue_file_work("materialize", hash, path).map_err(DatabaseError::operation)?;
    }
    timing.mark("restore_work");
    Ok(())
}

#[cfg(test)]
mod commit_tests {
    use crate::*;

    use crate::test_support::TestDatabase;

    fn open_test_database() -> TestDatabase {
        TestDatabase::new("import-commit")
    }

    fn inspected(title: &str, author: &str) -> book_metadata::InspectedBook {
        book_metadata::InspectedBook {
            pdf: None,
            metadata: book_model::BookRecord {
                metadata: book_model::BookDetails {
                    title: title.to_owned(),
                    subtitle: None,
                    contributors: vec![book_model::Contributor::new(author, book_model::MarcRelatorCode(*b"aut")).unwrap()],
                    book: book_model::BookMetadata::default(),
                },
                description: "A synopsis.".to_owned(),
            },
            audiobook: None,
            toc: vec![book_model::BookTocEntry { title: "Chapter 1".to_owned(), target: "ch1".to_owned(), children: Vec::new() }],
        }
    }

    #[test]
    fn commit_import_projects_inspection_placement_and_book() {
        let holder = open_test_database();
        let database = &holder.database;
        let parent = database.create_directory(&sync_common::ROOT_DIR_ID, &"Fiction".to_owned()).expect("create folder");
        let hash = ContentHash::new(&"9".repeat(64));
        let checksum = ContentHash::new(&"a".repeat(64));
        database
            .commit_import(ImportCommit {
                parent_id: parent.id,
                file_name: "book.epub".to_owned(),
                format: book_model::BookFormat::Epub,
                content_hash: hash,
                checksum,
                size_bytes: 1024,
                inspection_version: 1,
                inspection: Some(inspected("Imported Title", "Import Author")),
                published: PublishedImport { name: "book.epub".to_owned(), relative_path: "Fiction/book.epub".to_owned(), published: true, fingerprint: Some("fp1".to_owned()) },
                request_thumbnail: true,
                restore_paths: vec!["Fiction/book.epub".to_owned()],
            })
            .expect("commit import");
        let detail = ({
            use crate::browse::rows::{invalid_integer_column, invalid_text_column};
            use crate::browse::DetailSql;
            use crate::{entry_title, rusqlite, Database, DatabaseError, TocSql};
            use book_model::{AuthorId, AUTHOR_MARC_RELATOR_CODE};
            use library_model::{BookCardRow, BookDetail, BookDetailAuthor};
            use sync_common::ContentHash;
            struct SourceLocation {
                source_directory_id: Option<String>,
                path: Vec<library_model::BrowsePathSegment>,
            }
            let database: &Database = &database;
            let content_hash = hash;

            database.with_browse_snapshot(|| {
                let (book, audiobook_chapter_count) = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut book = None;
                    database.connection.book_detail_book(content_hash.as_str(), |row| {
                        let chapter_count: Option<i64> = row.get(1)?;
                        let chapter_count = chapter_count.map(usize::try_from).transpose().map_err(|error| invalid_integer_column(1, error.to_string()))?;
                        book = Some((row.get::<_, i64>(0)?, chapter_count));
                        Ok(())
                    })?;
                    let (id, chapter_count) = book.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                    let card = database.load_cards(&[id])?.pop().ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                    (card, chapter_count)
                };
                let description = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut description = String::new();
                    database.connection.book_detail_description(content_hash.as_str(), |row| {
                        description = row.get(0)?;
                        Ok(())
                    })?;
                    description
                };
                let SourceLocation { source_directory_id, path } = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut source_directory_id: Option<String> = None;
                    database.connection.book_detail_directory(content_hash.as_str(), |row| {
                        source_directory_id = row.get(0)?;
                        Ok(())
                    })?;

                    let path: Vec<library_model::BrowsePathSegment> = source_directory_id
                        .as_deref()
                        .map(|directory| {
                            let directory = sync_common::DirId::parse_str(directory).map_err(DatabaseError::operation)?;
                            database.dir_path_components(&directory)
                        })
                        .transpose()?
                        .unwrap_or_default()
                        .into_iter()
                        .map(|component| library_model::BrowsePathSegment { id: library_model::BrowseLocation::Folder(component.id), name: component.name.clone() })
                        .collect();

                    SourceLocation { source_directory_id, path }
                };
                let authors = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut authors = Vec::new();
                    database.connection.book_detail_authors(content_hash.as_str(), AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                        let id: String = row.get(0)?;
                        authors.push(BookDetailAuthor { id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?, name: row.get(1)? });
                        Ok(())
                    })?;

                    authors
                };
                let credits = {
                    let content_hash: &ContentHash = &content_hash;
                    // Only narrator and translator: a reader picks an audiobook by
                    // its narrator and an edition by its translator, and nothing else in the
                    // contributor list answers a question a reader is asking.
                    let mut credits = Vec::new();
                    let narrator = book_model::NARRATOR_MARC_RELATOR_CODE.0.to_vec();
                    let translator = book_model::TRANSLATOR_MARC_RELATOR_CODE.0.to_vec();
                    database.connection.book_detail_credits(content_hash.as_str(), narrator, translator, |row| {
                        let id: String = row.get(0)?;
                        let role: Vec<u8> = row.get(2)?;
                        credits.push(library_model::BookDetailCredit {
                            id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?,
                            name: row.get(1)?,
                            role: String::from_utf8(role).map_err(|error| invalid_text_column(2, error.to_string()))?,
                        });
                        Ok(())
                    })?;

                    credits
                };
                let toc = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut stored: Option<String> = None;
                    database.connection.book_toc_select(content_hash.as_str(), |row| {
                        stored = row.get(0)?;
                        Ok(())
                    })?;
                    let mut entries = stored
                        .map(|json| serde_json::from_str::<Vec<library_model::BookTocEntry>>(&json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}"))))
                        .transpose()?
                        .unwrap_or_default();
                    for (index, entry) in entries.iter_mut().enumerate() {
                        if book_model::audiobook_toc_position(&entry.target).is_some() {
                            entry.title = book_model::audiobook_chapter_display_title(&entry.title, index + 1);
                        }
                    }
                    entries
                };
                let (current_entry, current_target) = {
                    let content_hash: &ContentHash = &content_hash;
                    let format: library_model::LibraryFileTypeFilter = book.format_category;
                    let toc: &[library_model::BookTocEntry] = &toc;
                    'detail: {
                        if format == library_model::LibraryFileTypeFilter::Audiobook {
                            let Some(position) = database.audiobook_reading_position(content_hash)? else { break 'detail (None, None) };
                            let chapter = ({
                                use crate::{chapters_from_navigation, AudiobookSql};
                                use library_model::{AudiobookChapter, BookTocEntry};
                                let hash: &sync_common::ContentHash = content_hash;

                                'chapter: {
                                    let mut stored: Option<(String, i64)> = None;
                                    database.connection.audiobook_chapters_select(hash.as_str(), |row| {
                                        stored = Some((row.get(0)?, row.get(1)?));
                                        Ok(())
                                    })?;
                                    let Some((toc_json, duration_ms)) = stored else { break 'chapter Vec::new() };
                                    let entries: Vec<BookTocEntry> = serde_json::from_str(&toc_json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}")))?;
                                    chapters_from_navigation(&entries, duration_ms.max(0) as u64)
                                }
                            })
                            .into_iter()
                            .find(|chapter| position >= chapter.start_ms && position < chapter.end_ms);
                            break 'detail match chapter {
                                Some(chapter) => (Some(chapter.title), Some(book_model::audiobook_toc_target(chapter.start_ms))),
                                None => (None, None),
                            };
                        }
                        let mut target: Option<String> = None;
                        database.connection.reading_entry_select(content_hash.as_str(), |row| {
                            target = row.get(0)?;
                            Ok(())
                        })?;
                        let Some(target) = target else { break 'detail (None, None) };
                        (entry_title(toc, &target), Some(target))
                    }
                };
                Ok(BookDetail { book, audiobook_chapter_count, description, source_directory_id, path, authors, credits, current_entry, current_target, toc })
            })
        })
        .expect("read detail");
        assert_eq!(detail.book.title, "Imported Title");
        assert_eq!(detail.authors.iter().map(|author| author.name.as_str()).collect::<Vec<_>>(), vec!["Import Author"]);
        let body: Vec<u8> = database.connection.query_row("SELECT body FROM sync_state_version WHERE state_kind='metadata' AND state_key=?1", [hash.as_str()], |row| row.get(0)).unwrap();
        let library_replica::LibraryStateEntry::Metadata { value, .. } = sync_common::wire::decode(&body, sync_common::wire::MAX_DECODED_REQUEST_BYTES).unwrap() else { panic!("metadata register") };
        assert_eq!(value.title, "Imported Title");
        assert_eq!(value.contributors.iter().map(|credit| credit.name()).collect::<Vec<_>>(), ["Import Author"], "the register must contain the complete imported credits");
        assert!(database.thumbnail_book_format(hash).map(|format| format.is_some()).expect("placement"));
        assert!(database.pending_thumbnail_job_count().expect("thumbnail jobs") >= 1);
        let candidates = database.import_restore_candidates(&hash).expect("restore candidates");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].1.as_deref(), Some("Fiction/book.epub"));
        let _ = database.has_pending_asset_work().expect("pending asset work");
        // A cached inspection reuses the recorded proof and still commits.
        let other = database.create_directory(&sync_common::ROOT_DIR_ID, &"Nonfiction".to_owned()).expect("create folder");
        database
            .commit_import(ImportCommit {
                parent_id: other.id,
                file_name: "book.epub".to_owned(),
                format: book_model::BookFormat::Epub,
                content_hash: hash,
                checksum,
                size_bytes: 1024,
                inspection_version: 1,
                inspection: None,
                published: PublishedImport { name: "book.epub".to_owned(), relative_path: "Nonfiction/book.epub".to_owned(), published: true, fingerprint: Some("fp1".to_owned()) },
                request_thumbnail: false,
                restore_paths: Vec::new(),
            })
            .expect("commit cached import");
    }
}
